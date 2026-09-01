// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event stream to arena AST.
//!
//! The builder is iterative. Collections are assembled on one shared scratch
//! stack and flushed into contiguous side-table ranges when their end event
//! arrives, so nesting depth costs heap, never call stack.
//!
//! # The import prelude
//!
//! A file whose header imports another is parsed from a stream that begins with
//! a synthesised document declaring each imported name (D6.7). That document is
//! the parser's business and nobody else's: its events build no arena nodes and
//! no [`Document`](crate::ast::Document), and no span is ever taken from it.
//! All it does is make the scanner bind an alias, and register one
//! [`AnchorDef`](crate::anchor::AnchorDef) per imported name carrying the span
//! of the `&name` token **in the file that wrote it**.

use saphyr_parser::Event;

use crate::anchor::AnchorId;
use crate::ast::{
    AliasRef, Ast, Node, NodeId, NodeKind, Range32, Scalar, ScalarStyle, Tag,
};
use crate::diagnostic::{Code, Diagnostic, Diagnostics, SeverityMap};
use crate::mapping;
use crate::parse::Import;
use crate::scan;
use crate::span::{Pos, Rebase, SourceFile, Span};

#[derive(Clone, Copy)]
enum FrameKind {
    Sequence,
    Mapping,
}

struct Frame {
    kind: FrameKind,
    base: usize,
    start: Pos,
    anchor: Option<AnchorId>,
    tag: Option<u32>,
}

/// The synthetic document that installs a header's imports.
struct Prelude {
    /// The definitions to install, in authored order — the same order their
    /// anchors appear in the synthetic text.
    imports: Vec<Import>,
    /// How many have been installed in the current parser segment.
    installed: usize,
    /// Whether the events arriving now are still the prelude's.
    active: bool,
}

impl Prelude {
    fn new(imports: Vec<Import>) -> Self {
        let active = !imports.is_empty();
        Prelude { imports, installed: 0, active }
    }
}

/// Incrementally turns events into an [`Ast`].
pub(crate) struct Builder<'a> {
    file: &'a SourceFile,
    ast: Ast,
    diags: Diagnostics,
    frames: Vec<Frame>,
    pending: Vec<NodeId>,
    doc_root: Option<NodeId>,
    doc_start: Pos,
    doc_explicit: bool,
    document: u32,
    started: bool,
    segment: u32,
    char_base: usize,
    rebase: Rebase,
    prelude: Prelude,
    prev: (usize, usize),
    last_anchor_at: Option<usize>,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(
        file: &'a SourceFile,
        severities: SeverityMap,
        imports: Vec<Import>,
        rebase: Rebase,
    ) -> Self {
        let mut ast = Ast::new(file.id());
        ast.anchors.begin_prelude();
        Builder {
            ast,
            file,
            diags: Diagnostics::with_severities(severities),
            frames: Vec::new(),
            pending: Vec::new(),
            doc_root: None,
            doc_start: Pos::default(),
            doc_explicit: false,
            document: 0,
            started: false,
            segment: 0,
            char_base: 0,
            rebase,
            prelude: Prelude::new(imports),
            prev: (0, 0),
            last_anchor_at: None,
        }
    }

    /// Record a diagnostic.
    pub(crate) fn diagnose(&mut self, diagnostic: Diagnostic) {
        self.diags.push(diagnostic);
    }

    /// Consume the builder, yielding the AST and everything found along the way.
    pub(crate) fn finish(self) -> (Ast, Diagnostics) {
        (self.ast, self.diags)
    }

    /// Begin a new parser segment after error recovery. `char_base` is where in
    /// the file the new parser starts and `rebase` corrects every marker it
    /// will produce. The restarted stream carries the import prelude again, so
    /// the same imports are re-installed rather than shadowing the first set.
    pub(crate) fn restart(&mut self, char_base: usize, rebase: Rebase) {
        self.frames.clear();
        self.pending.clear();
        self.doc_root = None;
        self.segment += 1;
        self.char_base = char_base;
        self.rebase = rebase;
        self.prev = (char_base, char_base);
        self.prelude.installed = 0;
        self.prelude.active = !self.prelude.imports.is_empty();
        self.ast.anchors.begin_prelude();
    }

    fn span(&self, raw: saphyr_parser::Span) -> Span {
        Span {
            file: self.file.id(),
            start: self.file.pos(&raw.start, self.rebase),
            end: self.file.pos(&raw.end, self.rebase),
        }
    }

    fn bounds(&self, raw: saphyr_parser::Span) -> (usize, usize) {
        (self.rebase.char(raw.start.index()), self.rebase.char(raw.end.index()))
    }

    /// Feed one event.
    pub(crate) fn event(&mut self, event: &Event<'_>, raw: saphyr_parser::Span) {
        if self.prelude.active {
            self.prelude_event(event);
            return;
        }
        let span = self.span(raw);
        let bounds = self.bounds(raw);
        match event {
            Event::StreamStart | Event::StreamEnd | Event::Nothing => {}
            Event::DocumentStart(explicit) => self.document_start(*explicit, span),
            Event::DocumentEnd => self.document_end(span),
            Event::Scalar(value, style, anchor, tag) => {
                let anchor = self.define_anchor(*anchor, bounds.0, span);
                let tag = self.intern_tag(tag.as_deref());
                let payload = Scalar {
                    value: value.as_ref().into(),
                    style: ScalarStyle::from_parser(*style),
                };
                self.ast.scalars.push(payload);
                let kind = NodeKind::Scalar(last_index(self.ast.scalars.len()));
                self.emit_node(kind, span, anchor, tag);
            }
            Event::Alias(raw_id) => self.alias(*raw_id, span, bounds),
            Event::SequenceStart(anchor, tag) => {
                self.open(FrameKind::Sequence, *anchor, tag.as_deref(), span, bounds.0);
            }
            Event::MappingStart(anchor, tag) => {
                self.open(FrameKind::Mapping, *anchor, tag.as_deref(), span, bounds.0);
            }
            Event::SequenceEnd | Event::MappingEnd => self.close(span),
        }
        self.prev = bounds;
    }

    /// Consume one event of the synthetic import prelude.
    ///
    /// Only two of them mean anything: an anchored scalar installs the next
    /// import, and the document end hands the stream back to the file itself.
    /// Everything else — the document start, the flow sequence holding the
    /// anchors — is scaffolding and is dropped, which is what keeps the prelude
    /// out of the arena, out of `documents()` and out of every span.
    fn prelude_event(&mut self, event: &Event<'_>) {
        match event {
            Event::Scalar(_, _, anchor, _) if *anchor != 0 => self.install_import(*anchor),
            Event::DocumentEnd | Event::StreamEnd => self.close_prelude(),
            _ => {}
        }
    }

    fn install_import(&mut self, raw: usize) {
        let Some(import) = self.prelude.imports.get(self.prelude.installed) else { return };
        let (name, span) = (import.name.clone(), import.span);
        self.prelude.installed += 1;
        let id = self.ast.anchors.import((self.segment, raw), &name, span);
        self.warn_shadowed(id);
    }

    fn close_prelude(&mut self) {
        self.prelude.active = false;
        self.prev = (self.char_base, self.char_base);
        self.last_anchor_at = None;
    }

    fn document_start(&mut self, explicit: bool, span: Span) {
        if self.started {
            self.document += 1;
        }
        self.started = true;
        self.ast.anchors.end_document();
        self.doc_start = span.start;
        self.doc_explicit = explicit;
    }

    fn document_end(&mut self, span: Span) {
        let Some(root) = self.doc_root.take() else { return };
        self.ast.documents.push(crate::ast::Document {
            root,
            span: Span { file: span.file, start: self.doc_start, end: span.end },
            explicit: self.doc_explicit,
        });
    }

    fn open(
        &mut self,
        kind: FrameKind,
        raw_anchor: usize,
        tag: Option<&saphyr_parser::Tag>,
        span: Span,
        start_char: usize,
    ) {
        let anchor = self.define_anchor(raw_anchor, start_char, span);
        let tag = self.intern_tag(tag);
        self.frames.push(Frame { kind, base: self.pending.len(), start: span.start, anchor, tag });
    }

    fn close(&mut self, span: Span) {
        let Some(frame) = self.frames.pop() else { return };
        let children: Vec<NodeId> = self.pending.drain(frame.base..).collect();
        let span = Span { file: span.file, start: frame.start, end: span.end };
        let kind = match frame.kind {
            FrameKind::Sequence => self.build_sequence(&children),
            FrameKind::Mapping => self.build_mapping(&children, span),
        };
        self.emit_node(kind, span, frame.anchor, frame.tag);
    }

    fn build_sequence(&mut self, children: &[NodeId]) -> NodeKind {
        let start = as_u32(self.ast.seq_items.len());
        self.ast.seq_items.extend_from_slice(children);
        let end = as_u32(self.ast.seq_items.len());
        self.ast.seqs.push(Range32 { start, end });
        NodeKind::Sequence(last_index(self.ast.seqs.len()))
    }

    fn build_mapping(&mut self, children: &[NodeId], span: Span) -> NodeKind {
        let entries = mapping::pair_up(&mut self.ast, children, span);
        mapping::check_keys(&self.ast, &entries, &mut self.diags);
        let start = as_u32(self.ast.entries.len());
        self.ast.entries.extend_from_slice(&entries);
        let end = as_u32(self.ast.entries.len());
        self.ast.maps.push(Range32 { start, end });
        NodeKind::Mapping(last_index(self.ast.maps.len()))
    }

    /// Bind one alias.
    ///
    /// **The document's own table answers first.** A document starts with the
    /// bindings its header imported and nothing else (D2.6, D6.7), and
    /// [`AnchorTable::in_document`] is the table that says so. The parser's
    /// anchor ids are namespaced by the stream and outlive a document boundary,
    /// so consulting them first would bind `*Name` in the third document to a
    /// `&Name` that died with the second — reporting `E0130` against an alias
    /// whose name is imported and perfectly in scope.
    ///
    /// The parser's answer is the fallback, and that is what keeps `E0130`
    /// firing: a name this document never bound but the stream still knows is
    /// precisely an alias that crossed a document boundary, and
    /// [`Builder::check_document_scope`] is where it is reported.
    fn alias(&mut self, raw_id: usize, span: Span, bounds: (usize, usize)) {
        let name = scan::alias_name(self.file, bounds.0, bounds.1).unwrap_or("").to_owned();
        let anchor = self
            .ast
            .anchors
            .in_document(&name)
            .or_else(|| self.ast.anchors.by_raw((self.segment, raw_id)));
        let Some(anchor) = anchor else {
            self.diags.push(Diagnostic::new(
                Code::AnchorNameUnrecoverable,
                span,
                format!("alias `*{name}` refers to an anchor that was not recorded"),
            ));
            return;
        };
        let cross = self.check_document_scope(anchor, &name, span);
        self.ast.aliases.push(AliasRef {
            anchor,
            name: name.into(),
            cross_document: cross,
        });
        let kind = NodeKind::Alias(last_index(self.ast.aliases.len()));
        self.emit_node(kind, span, None, None);
    }

    /// D2.6, unchanged: an alias to an anchor of an earlier document is `E0130`.
    ///
    /// An imported definition is exempt, and not as a special case. The import
    /// installed it into *this* document before its first event, so by the time
    /// the alias is written it is a definition of this document — which is
    /// exactly D4.4's argument, and the reason the rule needs no relaxing.
    fn check_document_scope(&mut self, anchor: AnchorId, name: &str, span: Span) -> bool {
        let Some(def) = self.ast.anchors.get(anchor) else { return false };
        if def.is_imported() || def.document == self.document {
            return false;
        }
        let note = def.span;
        self.diags.push(
            Diagnostic::new(
                Code::CrossDocumentAlias,
                span,
                format!(
                    "alias `*{name}` refers to an anchor defined in an earlier document; \
                     anchors do not cross document boundaries"
                ),
            )
            .with_note("anchor defined here", Some(note)),
        );
        true
    }

    fn emit_node(
        &mut self,
        kind: NodeKind,
        span: Span,
        anchor: Option<AnchorId>,
        tag: Option<u32>,
    ) -> NodeId {
        let id = NodeId(as_u32(self.ast.nodes.len()));
        self.ast.nodes.push(Node { kind, span, anchor, tag });
        if let Some(anchor) = anchor {
            self.ast.anchors.set_node(anchor, id);
        }
        match self.frames.last() {
            Some(_) => self.pending.push(id),
            None => self.doc_root = Some(id),
        }
        id
    }

    fn intern_tag(&mut self, tag: Option<&saphyr_parser::Tag>) -> Option<u32> {
        let tag = tag?;
        let existing = self
            .ast
            .tags
            .iter()
            .position(|t| *t.handle == *tag.handle && *t.suffix == *tag.suffix);
        Some(match existing {
            Some(i) => as_u32(i),
            None => {
                self.ast.tags.push(Tag {
                    handle: tag.handle.as_str().into(),
                    suffix: tag.suffix.as_str().into(),
                });
                last_index(self.ast.tags.len())
            }
        })
    }

    /// Recover and record the `&name` property belonging to a node whose
    /// content starts at `start_char`.
    fn define_anchor(&mut self, raw: usize, start_char: usize, span: Span) -> Option<AnchorId> {
        if raw == 0 {
            return None;
        }
        let token = self.locate_anchor(start_char);
        let (name, token_span) = match token {
            Some(t) => (
                self.file.slice_chars(t.start + 1, t.end).to_owned(),
                self.token_span(t.start, t.end),
            ),
            None => {
                self.diags.push(Diagnostic::new(
                    Code::AnchorNameUnrecoverable,
                    span,
                    "this node is anchored but the anchor name could not be read from the source",
                ));
                (String::new(), span)
            }
        };
        let placeholder = NodeId(as_u32(self.ast.nodes.len()));
        let id = self.ast.anchors.define(
            (self.segment, raw),
            &name,
            placeholder,
            token_span,
            self.document,
        );
        self.warn_shadowed(id);
        Some(id)
    }

    fn locate_anchor(&mut self, start_char: usize) -> Option<scan::TokenRange> {
        let (prev_start, prev_end) = self.prev;
        let found = scan::find_anchor_token(self.file, prev_end.min(start_char), start_char)
            .or_else(|| scan::find_anchor_token(self.file, prev_start.min(start_char), start_char));
        let token = found?;
        if self.last_anchor_at.is_some_and(|last| token.start <= last) {
            let span = self.token_span(token.start, token.end);
            self.diags.push(Diagnostic::new(
                Code::AnchorOrderInconsistent,
                span,
                "recovered anchor properties are not in definition order; \
                 the anchor name attributed to this node may be wrong",
            ));
            return None;
        }
        self.last_anchor_at = Some(token.start);
        Some(token)
    }

    fn token_span(&self, start: usize, end: usize) -> Span {
        Span {
            file: self.file.id(),
            start: self.file.pos_at_char(start),
            end: self.file.pos_at_char(end),
        }
    }

    fn warn_shadowed(&mut self, id: AnchorId) {
        let Some(def) = self.ast.anchors.get(id) else { return };
        if def.name.is_empty() {
            return;
        }
        let Some(previous) = def.shadows else { return };
        let (name, span) = (def.name.to_string(), def.span);
        let earlier = self.ast.anchors.get(previous).map(|d| d.span);
        self.diags.push(
            Diagnostic::new(
                Code::AnchorShadowed,
                span,
                format!(
                    "anchor `&{name}` enters a new state; aliases after this point bind to \
                     this definition, and the bare global name denotes the last state"
                ),
            )
            .with_note("the state it supersedes is here", earlier),
        );
    }
}

fn last_index(len: usize) -> u32 {
    as_u32(len.saturating_sub(1))
}

fn as_u32(value: usize) -> u32 {
    u32::try_from(value).expect("arena side table overflow")
}
