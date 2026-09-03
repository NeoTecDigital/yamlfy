// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scope tree and its two orthogonal axes.
//!
//! The tree is the **directory hierarchy**: one scope per directory, the root
//! directory included. Files are not scopes. A file's header declares the axes
//! and the namespace *of the directory it sits in*, which is what makes several
//! files contributing to one namespace the ordinary arrangement rather than an
//! error, and what makes a header-less file inherit everything from its
//! directory scope with no special case.
//!
//! Each scope carries a *visibility* and a *mutability*, each inherited from the
//! parent unless a header states it. The root states both, and both are the
//! closed value: **`private` and `immutable`**. Access and mutation are opt-in,
//! so a scope that says nothing grants nothing.
//!
//! Resolution is **path-composed, never node-local**. `visible(n, o)` holds when
//! every scope on `path(root → n)` is open to `o`; likewise `writable`. Deciding
//! either axis by looking only at `n` would make an `immutable` or `private`
//! parent mean nothing, so each scope stores its whole root-to-self path and the
//! predicates walk it.
//!
//! Both axes compose by one rule: a scope is open to an observer when the scope
//! resolves to the permissive value, **or** the observer sits inside that scope.
//! That is why the root may be `private` without cutting the project off from
//! itself — every scope in the project is inside the root.

use yfi_syntax::{FileId, Span};

/// Handle to a scope.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ScopeId(pub u32);

impl ScopeId {
    /// The handle as a `usize` index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Who may see a scope.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    /// Reachable only from inside the scope.
    Private,
    /// Reachable from anywhere.
    Public,
}

/// Who may write a scope. Phase 1 records this and ships no writer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutability {
    /// Writable only from inside the scope.
    Immutable,
    /// Writable from anywhere.
    Mutable,
}

impl Visibility {
    /// The spelling used in a header.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Public => "public",
        }
    }

    /// Parse a header value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "private" => Some(Visibility::Private),
            "public" => Some(Visibility::Public),
            _ => None,
        }
    }

    /// Whether an observer outside the scope is admitted.
    #[must_use]
    pub fn is_open(self) -> bool {
        self == Visibility::Public
    }

    /// Every legal spelling, for diagnostics.
    #[must_use]
    pub fn choices() -> &'static str {
        "private, public"
    }
}

impl Mutability {
    /// The spelling used in a header.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Mutability::Immutable => "immutable",
            Mutability::Mutable => "mutable",
        }
    }

    /// Parse a header value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "immutable" => Some(Mutability::Immutable),
            "mutable" => Some(Mutability::Mutable),
            _ => None,
        }
    }

    /// Whether an observer outside the scope is admitted.
    #[must_use]
    pub fn is_open(self) -> bool {
        self == Mutability::Mutable
    }

    /// Every legal spelling, for diagnostics.
    #[must_use]
    pub fn choices() -> &'static str {
        "immutable, mutable"
    }
}

/// What a scope was built from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeKind {
    /// The project root directory.
    Root,
    /// A directory below the root.
    Directory,
}

/// What headers stated for a scope, before inheritance fills the gaps.
#[derive(Clone, Copy, Default, Debug)]
pub struct Declared {
    /// Stated visibility, if any.
    pub visibility: Option<Visibility>,
    /// Stated mutability, if any.
    pub mutability: Option<Mutability>,
}

/// One node of the scope tree.
pub struct Scope {
    /// This scope's handle.
    pub id: ScopeId,
    /// The enclosing scope; `None` only for the root.
    pub parent: Option<ScopeId>,
    /// Root or directory.
    pub kind: ScopeKind,
    /// The directory's own name. The root's name is its directory name.
    pub name: Box<str>,
    /// The namespace headers in this directory claimed.
    pub namespace: Option<Box<str>>,
    /// Where that namespace was first written.
    pub namespace_span: Option<Span>,
    /// What headers stated here.
    pub declared: Declared,
    /// Where the surviving `visibility:` claim was written, when one was.
    /// `None` means the scope inherited its visibility rather than stating it,
    /// and is what lets a diagnostic say which of the two happened.
    pub visibility_span: Option<Span>,
    /// Where the surviving `mutability:` claim was written, when one was. The
    /// mutability axis has a diagnostic of its own now (`E0217`), so it needs
    /// the same span the visibility axis has always carried.
    pub mutability_span: Option<Span>,
    /// Visibility after inheritance.
    pub visibility: Visibility,
    /// Mutability after inheritance.
    pub mutability: Mutability,
    /// The whole `root → self` path, inclusive at both ends. Stored rather than
    /// recomputed because every visibility and mutability question walks it.
    pub path: Vec<ScopeId>,
    /// Every file whose header declared something here, in discovery order.
    pub declared_by: Vec<FileId>,
}

/// The project's scope tree. Parents always have a lower [`ScopeId`] than their
/// children, so one forward pass resolves inheritance.
#[derive(Default)]
pub struct ScopeTree {
    scopes: Vec<Scope>,
}

impl ScopeTree {
    /// An empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every scope, parents before children.
    #[must_use]
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// Look up a scope.
    #[must_use]
    pub fn get(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.get(id.index())
    }

    /// The project root, or `None` for an empty tree.
    #[must_use]
    pub fn root(&self) -> Option<ScopeId> {
        (!self.scopes.is_empty()).then_some(ScopeId(0))
    }

    /// The stored `root → id` path.
    #[must_use]
    pub fn path(&self, id: ScopeId) -> &[ScopeId] {
        self.get(id).map_or(&[], |s| &s.path)
    }

    /// The first scope claiming `namespace`, if any.
    #[must_use]
    pub fn by_namespace(&self, namespace: &str) -> Option<ScopeId> {
        self.scopes.iter().find(|s| s.namespace.as_deref() == Some(namespace)).map(|s| s.id)
    }

    /// `root/dir/sub`, for logging and test assertions.
    #[must_use]
    pub fn qualified(&self, id: ScopeId) -> String {
        let names: Vec<&str> =
            self.path(id).iter().filter_map(|s| self.get(*s)).map(|s| &*s.name).collect();
        names.join("/")
    }

    /// Whether `observer` sits inside `scope` — that is, whether `scope` is on
    /// the observer's own root path.
    #[must_use]
    pub fn encloses(&self, scope: ScopeId, observer: ScopeId) -> bool {
        self.path(observer).contains(&scope)
    }

    /// Whether `target` is visible to `observer`, composed over the whole path.
    #[must_use]
    pub fn visible(&self, target: ScopeId, observer: ScopeId) -> bool {
        self.composed(target, observer, |scope| scope.visibility.is_open())
    }

    /// Whether `target` is writable by `observer`, composed over the whole path.
    #[must_use]
    pub fn writable(&self, target: ScopeId, observer: ScopeId) -> bool {
        self.composed(target, observer, |scope| scope.mutability.is_open())
    }

    /// The outermost scope on `target`'s path that shuts `observer` out on the
    /// visibility axis, or `None` when `target` is visible.
    ///
    /// This is the *reason* [`ScopeTree::visible`] answered no, and a
    /// diagnostic needs it: reach is path-composed (D6.5), so the scope that
    /// blocked it may be several directories above the target and naming the
    /// target alone would not tell an author what to change. Outermost rather
    /// than innermost because the outermost gate is the one that has to open
    /// first — opening any scope below it changes nothing.
    #[must_use]
    pub fn blocked_by(&self, target: ScopeId, observer: ScopeId) -> Option<ScopeId> {
        self.closed(target, observer, |scope| scope.visibility.is_open())
    }

    /// The outermost scope on `target`'s path that shuts `observer` out on the
    /// mutability axis, or `None` when `target` is writable.
    ///
    /// The exact analogue of [`ScopeTree::blocked_by`], and deliberately the
    /// same walk: an extended reference is a **write performed at compile
    /// time**, so it asks the mutability axis the question the visibility gate
    /// (`E0216`, raised in pass 4 ahead of resolution) asks the visibility
    /// axis, and the two must never be able to disagree about who blocked what. Outermost for the same reason — opening any scope below
    /// the outermost gate changes nothing.
    #[must_use]
    pub fn not_writable_by(&self, target: ScopeId, observer: ScopeId) -> Option<ScopeId> {
        self.closed(target, observer, |scope| scope.mutability.is_open())
    }

    /// The first scope on `target`'s path that `open` rejects for `observer`.
    /// One implementation of the composition rule, so the predicates and the
    /// diagnostics can never disagree about who blocked what.
    fn closed(
        &self,
        target: ScopeId,
        observer: ScopeId,
        open: fn(&Scope) -> bool,
    ) -> Option<ScopeId> {
        self.get(observer)?;
        self.get(target)?;
        self.path(target)
            .iter()
            .filter_map(|id| self.get(*id))
            .find(|scope| !open(scope) && !self.encloses(scope.id, observer))
            .map(|scope| scope.id)
    }

    fn composed(&self, target: ScopeId, observer: ScopeId, open: fn(&Scope) -> bool) -> bool {
        if self.get(target).is_none() || self.get(observer).is_none() {
            return false;
        }
        self.closed(target, observer, open).is_none()
    }

    /// Append a scope below `parent`. Axes are left at the inherited defaults
    /// until [`ScopeTree::resolve`] runs, because declarations arrive from files
    /// in discovery order rather than in tree order.
    pub(crate) fn push(&mut self, parent: Option<ScopeId>, name: &str) -> ScopeId {
        let id = ScopeId(u32::try_from(self.scopes.len()).expect("scope tree overflow"));
        let kind = if parent.is_some() { ScopeKind::Directory } else { ScopeKind::Root };
        let mut path = parent.map(|p| self.path(p).to_vec()).unwrap_or_default();
        path.push(id);
        self.scopes.push(Scope {
            id,
            parent,
            kind,
            name: name.into(),
            namespace: None,
            namespace_span: None,
            declared: Declared::default(),
            visibility_span: None,
            mutability_span: None,
            visibility: Visibility::Private,
            mutability: Mutability::Immutable,
            path,
            declared_by: Vec::new(),
        });
        id
    }

    pub(crate) fn claim_namespace(&mut self, id: ScopeId, namespace: &str, span: Span) {
        if let Some(scope) = self.get_mut(id) {
            scope.namespace = Some(namespace.into());
            scope.namespace_span = Some(span);
        }
    }

    pub(crate) fn get_mut(&mut self, id: ScopeId) -> Option<&mut Scope> {
        self.scopes.get_mut(id.index())
    }

    /// Fill every scope's axes from its declaration, falling back to its
    /// parent. One forward pass suffices because a parent's id is always lower.
    pub(crate) fn resolve(&mut self) {
        for index in 0..self.scopes.len() {
            let inherited = self.scopes[index]
                .parent
                .map(|p| (self.scopes[p.index()].visibility, self.scopes[p.index()].mutability))
                .unwrap_or((Visibility::Private, Mutability::Immutable));
            let scope = &mut self.scopes[index];
            scope.visibility = scope.declared.visibility.unwrap_or(inherited.0);
            scope.mutability = scope.declared.mutability.unwrap_or(inherited.1);
        }
    }
}
