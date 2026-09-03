// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The path grammar, and the space a path walks (D4.12).
//!
//! A reach is spelled the way a filesystem is spelled, because the scope tree
//! *is* the directory tree:
//!
//! ```text
//! ../shared/Service     up one directory, then into `shared`, then `Service`
//! ../../core/Base       up two
//! sibling/Service       a peer file in this directory
//! Service               this file
//! Service.port          a member of it
//! ```
//!
//! **The path performs the reach.** There is no declaration list to keep in
//! step with it: `..` walks up the scope tree exactly as it walks up
//! directories, a bare segment names a peer directory or a peer file, and `.`
//! addresses members. Naming a peer *is* reaching it.
//!
//! # Grammar
//!
//! ```text
//! path    := prefix segment ("/" segment)* ("." member)*
//! prefix  := "./" | "../"+ | ε
//! segment := name
//! member  := name
//! name    := [A-Za-z_] [A-Za-z0-9_-]*
//! ```
//!
//! `name` deliberately excludes `.`, `:` and digits-first, so `7`,
//! `acme::billing/invoice` and `http://host/thing` are **not** paths. That is
//! what lets a plain scalar be read as a path where a path is expected without
//! silently reinterpreting data that merely contains a slash.
//!
//! # What each form resolves against
//!
//! | written | resolved against |
//! |---|---|
//! | `Name` | a `!ref` binding of this document, else a definition of **this file** |
//! | `./Name` | a definition of **this directory** |
//! | `dir/Name`, `file/Name` | a child scope of this directory, else a peer file of it |
//! | `../…` | the same, one scope higher per `..` |
//!
//! A segment that names both a child directory and a file stem resolves to the
//! **directory**. The alternative — file first — would let adding a directory
//! silently move an existing path, and a directory is the more public address
//! of the two because it is what a namespace is claimed on (D6.1).
//!
//! Only Yamlfication source files answer a path. A base YAML file declares
//! nothing and has no addressable definitions (D6.6), so it is reachable by
//! `imports:` and by nothing else — which is also why a `service.yfy` beside a
//! `service.yaml` is not ambiguous.

use std::collections::HashMap;

use yfi_syntax::{FileId, NodeId};

use super::table::Table;
use super::Ctx;
use crate::scope::ScopeId;

/// A parsed path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Path {
    /// How many `..` the path opens with.
    pub ascents: u32,
    /// Whether it was written with an explicit `./` or `../` prefix. A data
    /// position accepts only an anchored path, because there every scalar could
    /// otherwise have been data.
    pub anchored: bool,
    /// The `/`-separated segments, at least one.
    pub segments: Vec<Box<str>>,
    /// The `.`-separated members addressed within whatever the segments named.
    pub members: Vec<Box<str>>,
}

impl Path {
    /// Whether the path is the bare one-segment form, which is the only form
    /// that may name a `!ref` binding rather than a file or a directory.
    #[must_use]
    pub fn is_bare(&self) -> bool {
        !self.anchored && self.ascents == 0 && self.segments.len() == 1
    }
}

/// Parse `text` as a path, or `None` if it is not one.
#[must_use]
pub fn parse(text: &str) -> Option<Path> {
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    let (ascents, anchored) = prefix(&mut rest);
    let mut parts = rest.split('/');
    let last = parts.next_back()?;
    let mut segments: Vec<Box<str>> = Vec::new();
    for part in parts {
        segments.push(is_name(part)?.into());
    }
    let mut tail = last.split('.');
    segments.push(is_name(tail.next()?)?.into());
    let mut members: Vec<Box<str>> = Vec::new();
    for member in tail {
        members.push(is_name(member)?.into());
    }
    Some(Path { ascents, anchored, segments, members })
}

/// Consume the `./` or `../`+ prefix, returning the ascent count and whether
/// one was written at all.
fn prefix(rest: &mut &str) -> (u32, bool) {
    let mut ascents = 0;
    while let Some(next) = rest.strip_prefix("../") {
        *rest = next;
        ascents += 1;
    }
    if ascents > 0 {
        return (ascents, true);
    }
    match rest.strip_prefix("./") {
        Some(next) => {
            *rest = next;
            (0, true)
        }
        None => (0, false),
    }
}

/// `part` if it is a legal name, else `None`.
fn is_name(part: &str) -> Option<&str> {
    let mut chars = part.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-').then_some(part)
}

/// Why a path named nothing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Failure {
    /// More `..` than there are scopes above the referencing file.
    AboveRoot,
    /// A segment names neither a child directory nor a peer source file.
    NoSegment(Box<str>),
    /// A segment after one that named a file: a file holds definitions, not
    /// further directories.
    NotADirectory(Box<str>),
    /// The final segment names no definition where the walk landed. The second
    /// field is **where that was** — the file or the directory the walk reached
    /// — because "no definition called `X` was found" is unanswerable without
    /// it: for `Nowhere` the place is this file, and for `dir/Nowhere` it is
    /// that directory.
    NoDefinition(Box<str>, Box<str>),
    /// A `.` member the node it resolved to does not hold.
    NoMember(Box<str>),
    /// A `!ref` whose operand is not a path at all.
    NotAPath,
    /// A `!ref` binding whose own path resolves back through itself.
    BindingCycle,
    /// The walk landed in a scope the referencing scope cannot see. The fields
    /// are the outermost scope that shut the observer out and the observer
    /// itself, both as qualified directory names — never a file, a line or a
    /// column, because a position inside a scope the reader may not see is
    /// itself the disclosure.
    NotVisible(Box<str>, Box<str>),
}

/// The shape of the project a path walks: which directories hold which
/// directories, and which hold which source files.
///
/// Built once per link, because every path asks the same two questions and the
/// answers are a property of the tree rather than of any one reference.
pub(crate) struct Space {
    children: HashMap<(ScopeId, Box<str>), ScopeId>,
    stems: HashMap<(ScopeId, Box<str>), FileId>,
}

impl Space {
    /// Index the project's directories and source files.
    pub(crate) fn build(ctx: &Ctx) -> Self {
        let mut space = Space { children: HashMap::new(), stems: HashMap::new() };
        for scope in ctx.project.scopes().scopes() {
            if let Some(parent) = scope.parent {
                space.children.insert((parent, scope.name.clone()), scope.id);
            }
        }
        for file in ctx.project.files() {
            if !ctx.is_source(file.id) {
                continue;
            }
            let Some(stem) = file.relative.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            space.stems.entry((file.scope, stem.into())).or_insert(file.id);
        }
        space
    }

    fn child(&self, scope: ScopeId, name: &str) -> Option<ScopeId> {
        self.children.get(&(scope, name.into())).copied()
    }

    fn stem(&self, scope: ScopeId, name: &str) -> Option<FileId> {
        self.stems.get(&(scope, name.into())).copied()
    }
}

/// Where a walk landed before the final segment is read.
enum Landing {
    /// A directory: the final segment is sought across every source file of it.
    Scope(ScopeId),
    /// One file: the final segment is sought in that file alone.
    File(FileId),
}

/// Resolve the **structural** part of a path — everything except a bare
/// one-segment form, which may name a binding and is resolved by the caller.
///
/// Members are applied here too, because a member is addressing within
/// whatever the segments named and needs the same arena walk.
///
/// # The gate stands in front of the lookup
///
/// Visibility is asked of the place the walk landed **before** the final
/// segment is sought there and before any `.` member is addressed. D4.12 gives
/// an outsider no access to a private definition *at all* — not its members,
/// not its public surface, not its name — and a lookup performed first is an
/// oracle whatever the diagnostic says afterwards: a member that exists, a
/// member that does not and a name that does not would each earn a
/// distinguishable answer, and between them an outsider enumerates the scope.
/// So an invisible landing resolves to **nothing**, with one shape of answer
/// that differs only in the path the author wrote.
pub(crate) fn resolve(
    ctx: &Ctx,
    space: &Space,
    table: &Table,
    origin: FileId,
    path: &Path,
) -> Result<(FileId, NodeId), Failure> {
    let landing = walk(ctx, space, origin, path)?;
    visible(ctx, origin, &landing)?;
    let name = path.segments.last().expect("a path has at least one segment");
    let found = match landing {
        Landing::File(file) => table.in_file(file, name),
        Landing::Scope(scope) => table.in_scope(scope, name),
    };
    let base =
        found.ok_or_else(|| Failure::NoDefinition(name.clone(), where_it_landed(ctx, &landing)))?;
    members(ctx, base, &path.members)
}

/// Whether the scope a walk landed in is visible to the file that wrote the
/// path, composed over the whole `root → landing` path (D6.5).
///
/// Every scope that can be a blocker lies strictly between the root — which
/// encloses every observer and therefore never blocks — and the landing, so it
/// is a directory the author named in the path or an ancestor of one. Naming it
/// discloses nothing the author did not already write.
fn visible(ctx: &Ctx, origin: FileId, landing: &Landing) -> Result<(), Failure> {
    let Some(observer) = ctx.project.file(origin).map(|held| held.scope) else {
        return Ok(());
    };
    let target = match landing {
        Landing::Scope(scope) => *scope,
        Landing::File(file) => match ctx.project.file(*file) {
            Some(held) => held.scope,
            None => return Ok(()),
        },
    };
    let scopes = ctx.project.scopes();
    match scopes.blocked_by(target, observer) {
        Some(blocker) => Err(Failure::NotVisible(
            scopes.qualified(blocker).into(),
            scopes.qualified(observer).into(),
        )),
        None => Ok(()),
    }
}

/// How the place a walk landed is named back to the author.
///
/// A file is named by its path relative to the project root and a directory by
/// its scope, because those are the two things an author can act on: either the
/// definition belongs in that file, or the path meant a different directory.
fn where_it_landed(ctx: &Ctx, landing: &Landing) -> Box<str> {
    match landing {
        Landing::File(file) => ctx
            .project
            .file(*file)
            .map_or_else(|| "this file".into(), |held| held.relative.display().to_string()),
        Landing::Scope(scope) => ctx.project.scopes().qualified(*scope),
    }
    .into()
}

/// Walk the segments before the last, leaving the walk on a directory or a
/// file.
fn walk(ctx: &Ctx, space: &Space, origin: FileId, path: &Path) -> Result<Landing, Failure> {
    let mut scope = ctx.project.file(origin).map(|f| f.scope).ok_or(Failure::AboveRoot)?;
    for _ in 0..path.ascents {
        scope = ctx
            .project
            .scopes()
            .get(scope)
            .and_then(|held| held.parent)
            .ok_or(Failure::AboveRoot)?;
    }
    if path.segments.len() == 1 && !path.anchored {
        return Ok(Landing::File(origin));
    }
    let mut landing = Landing::Scope(scope);
    for segment in &path.segments[..path.segments.len() - 1] {
        let Landing::Scope(at) = landing else {
            return Err(Failure::NotADirectory(segment.clone()));
        };
        landing = match space.child(at, segment) {
            Some(child) => Landing::Scope(child),
            None => {
                Landing::File(space.stem(at, segment).ok_or(Failure::NoSegment(segment.clone()))?)
            }
        };
    }
    Ok(landing)
}

/// Address `names` in turn, each a member of what the last one produced.
///
/// Chaining is allowed and means what it reads as: `Service.tls.port` is the
/// `port` of the `tls` of `Service`. Each step is an ordinary mapping lookup in
/// the arena, so a member that is itself an alias or a nested mapping is
/// addressed exactly like the top-level one.
pub(crate) fn members(
    ctx: &Ctx,
    base: (FileId, NodeId),
    names: &[Box<str>],
) -> Result<(FileId, NodeId), Failure> {
    let mut at = base;
    for name in names {
        at = (at.0, member(ctx, at, name).ok_or_else(|| Failure::NoMember(name.clone()))?);
    }
    Ok(at)
}

/// One `.` step: the member of `at` called `name`.
///
/// The comparison is against the **member name**, which is what a flag prefix
/// has already been taken off (`crate::member`), so `Service.port` addresses
/// `pub port:` exactly as it addresses `port:`. A flag is a declaration about a
/// member, never part of its name.
///
/// Both spellings of a member list answer. In the mapping form the step yields
/// the member's value; in the sequence form the item *is* the member, and
/// yields itself.
fn member(ctx: &Ctx, at: (FileId, NodeId), name: &str) -> Option<NodeId> {
    let ast = ctx.ast(at.0)?;
    let named = |node: NodeId| {
        ctx.interned.key_of(at.0, node).and_then(|symbol| ctx.interned.symbols().resolve(symbol))
            == Some(name)
    };
    if let Some(items) = ast.items(at.1) {
        return items.iter().copied().find(|item| named(*item));
    }
    ast.entries(at.1)?
        .iter()
        .filter(|entry| !entry.merge)
        .find(|entry| named(entry.key))
        .map(|entry| entry.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(path: &Path) -> Vec<&str> {
        path.segments.iter().map(|s| &**s).collect()
    }

    #[test]
    fn a_bare_name_is_a_path_with_one_segment_and_no_prefix() {
        let path = parse("Service").expect("a path");
        assert!(path.is_bare());
        assert_eq!(segments(&path), ["Service"]);
        assert!(path.members.is_empty());
    }

    #[test]
    fn ascents_are_counted_and_mark_the_path_anchored() {
        let path = parse("../../core/Base").expect("a path");
        assert_eq!(path.ascents, 2);
        assert!(path.anchored);
        assert_eq!(segments(&path), ["core", "Base"]);
    }

    #[test]
    fn a_leading_dot_slash_anchors_without_ascending() {
        let path = parse("./Service").expect("a path");
        assert_eq!(path.ascents, 0);
        assert!(path.anchored && !path.is_bare());
    }

    #[test]
    fn members_chain_after_the_last_segment() {
        let path = parse("../shared/Service.tls.port").expect("a path");
        assert_eq!(segments(&path), ["shared", "Service"]);
        let members: Vec<&str> = path.members.iter().map(|s| &**s).collect();
        assert_eq!(members, ["tls", "port"]);
    }

    #[test]
    fn what_is_not_a_path_stays_data() {
        // Each of these is something a `.yfy` legitimately holds as a value, and
        // reading any of them as a reach is the failure the grammar exists to
        // prevent.
        for text in ["7", "acme::billing/invoice", "http://host/thing", "", "..", "a//b", "-x"] {
            assert_eq!(parse(text), None, "`{text}` is data, not a path");
        }
    }
}
