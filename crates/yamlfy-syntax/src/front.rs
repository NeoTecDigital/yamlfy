// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `.yfy` front end: the pre-pass that runs before the YAML parser.
//!
//! `.yfy` holds three constructs a YAML parser rejects outright — `// comment`,
//! `<?-- … --!>` and `<?-- … -->` are each a scanner error. They are therefore
//! rewritten out of the text **before** the parser sees a byte of it, and only
//! for a [`Dialect::Yamlfication`] file. A `.yaml`/`.yml` is base YAML and is
//! handed to the parser exactly as it was written (D6.6).
//!
//! # The rewrite is a character-for-character substitution
//!
//! Every downstream byte offset, line and column is a position in the file the
//! author wrote, so the rewrite is not allowed to move one. It is therefore
//! constrained to substitution: **one character out for one character in, and a
//! line break is never touched**. `//` becomes `# ` — two characters for two,
//! which is why the comment spelling was chosen — and a block's region becomes
//! filler of the same length holding the same line breaks.
//!
//! Byte offsets are the one thing a substitution can still move, because an
//! ASCII space replacing a multi-byte character is shorter. So a rewritten file
//! keeps **two** offset tables: a position is resolved against the offsets of
//! the text as written, and text is sliced against the offsets of the text the
//! parser read. See [`crate::span::SourceFile`].
//!
//! # What each construct becomes
//!
//! | written | seen by the parser | what reaches the arena |
//! |---|---|---|
//! | `// note` | `#  note` | nothing; it is a comment |
//! | `<?-- … --!>` | spaces | nothing; captured as a [`Block`] for documentation |
//! | `<?-- … -->` | `.` on its first line, spaces after | one scalar, styled [`ScalarStyle::Code`] |
//!
//! A **code block is a value**, so it has to be a node. The filler is chosen so
//! that the parser produces that node itself — a plain scalar with exactly the
//! block's span — and [`install`] then gives the scalar its real text and its
//! style. Nothing is inserted into the arena afterwards and no node's parentage
//! is invented: the node the parser built at that position *is* the code block,
//! and every other node is untouched.
//!
//! **This language never parses a block's contents.** Any syntax may appear
//! inside one; it is compiled or executed by something else.

use crate::ast::{Ast, NodeKind, ScalarStyle};
use crate::span::{SourceFile, Span};

/// Which language a file's text is read as, decided by the caller and never
/// guessed from the contents (D6.6).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Dialect {
    /// Base YAML. The text reaches the parser character for character.
    #[default]
    BaseYaml,
    /// Yamlfication source. `//`, `<?-- --!>` and `<?-- -->` are rewritten out
    /// of the text first.
    Yamlfication,
}

/// What a `<?-- … >` region is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    /// Closed with `--!>`. Documentation: captured, and emits no node.
    Documentation,
    /// Closed with `-->`. A value: it reaches the arena as a scalar carrying
    /// the code flag.
    Code,
}

impl BlockKind {
    /// The name used in a diagnostic.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            BlockKind::Documentation => "documentation",
            BlockKind::Code => "code",
        }
    }
}

/// One `<?-- … >` region of a file.
#[derive(Clone, Debug)]
pub struct Block {
    /// Documentation or code.
    pub kind: BlockKind,
    /// The contents between the delimiters, verbatim and unparsed.
    pub text: Box<str>,
    /// The whole region, delimiters included.
    pub span: Span,
}

/// A block that was opened and never closed.
#[derive(Clone, Copy, Debug)]
pub struct Fault {
    /// Character index of the opening `<?--`.
    pub(crate) start: usize,
    /// Character index one past the end of the line it opened on.
    pub(crate) end: usize,
}

/// A block, recorded in character indices because the file's positions are not
/// known until it is registered.
pub(crate) struct RawBlock {
    pub(crate) kind: BlockKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) text: String,
}

/// The outcome of the pre-pass.
pub(crate) struct Rewrite {
    /// The text the parser will read.
    pub(crate) text: String,
    /// Every block found, in source order.
    pub(crate) blocks: Vec<RawBlock>,
    /// Every unterminated block.
    pub(crate) faults: Vec<Fault>,
    /// Whether anything changed at all.
    pub(crate) changed: bool,
}

/// Run the pre-pass over `text` for `dialect`.
pub(crate) fn preprocess(text: &str, dialect: Dialect) -> Rewrite {
    if dialect == Dialect::BaseYaml {
        return Rewrite {
            text: text.to_owned(),
            blocks: Vec::new(),
            faults: Vec::new(),
            changed: false,
        };
    }
    let source: Vec<char> = text.chars().collect();
    let mut scan = Scan {
        out: source.clone(),
        source,
        at: 0,
        regions: Vec::new(),
        faults: Vec::new(),
    };
    scan.run();
    let changed = scan.out != scan.source;
    let blocks = scan
        .regions
        .iter()
        .map(|region| RawBlock {
            kind: region.kind,
            start: region.start,
            end: region.end,
            text: scan.source[region.content.0..region.content.1].iter().collect(),
        })
        .collect();
    Rewrite { text: scan.out.into_iter().collect(), blocks, faults: scan.faults, changed }
}

/// Give every code block's placeholder scalar its real text and style.
///
/// The parser has already built a node for each one, because the filler was
/// chosen to make it do so. This is therefore the only mutation the pre-pass
/// performs on a finished arena, and it can neither add, drop nor re-parent a
/// node.
pub(crate) fn install(ast: &mut Ast, file: &SourceFile) {
    for block in file.blocks().iter().filter(|held| held.kind == BlockKind::Code) {
        let at = block.span.start.byte;
        for position in 0..ast.nodes.len() {
            if ast.nodes[position].span.start.byte != at {
                continue;
            }
            let NodeKind::Scalar(index) = ast.nodes[position].kind else { continue };
            let scalar = &mut ast.scalars[index as usize];
            scalar.value = block.text.clone();
            scalar.style = ScalarStyle::Code;
        }
    }
}

/// A region, before the file's positions are known.
struct Region {
    kind: BlockKind,
    start: usize,
    end: usize,
    content: (usize, usize),
}

/// The lexer the pre-pass is. It knows only what it must in order to avoid
/// rewriting text that is data: quoted scalars, comments and block scalars are
/// skipped whole, so a `//` written inside any of them stays what it was.
struct Scan {
    source: Vec<char>,
    out: Vec<char>,
    at: usize,
    regions: Vec<Region>,
    faults: Vec<Fault>,
}

impl Scan {
    fn run(&mut self) {
        let mut indent = 0usize;
        let mut fresh = true;
        while self.at < self.source.len() {
            if fresh {
                indent = self.indent();
                fresh = false;
            }
            match self.source[self.at] {
                '\n' => {
                    self.at += 1;
                    fresh = true;
                }
                c @ ('\'' | '"') => self.at = self.quoted(c),
                '#' if self.opens_token() => self.at = self.line_end(self.at),
                '|' | '>' if self.opens_token() && self.is_block_header() => {
                    self.at = self.block_scalar(indent);
                }
                '<' if self.opens_token() && self.matches("<?--") => self.at = self.block(),
                '/' if self.opens_token() && self.matches("//") => self.at = self.comment(),
                _ => self.at += 1,
            }
        }
    }

    /// Indentation of the line the scan is at the start of.
    fn indent(&self) -> usize {
        let mut at = self.at;
        while self.source.get(at) == Some(&' ') {
            at += 1;
        }
        at - self.at
    }

    /// Whether a token may begin here.
    ///
    /// This is YAML's own rule for `#`, and `//` is given exactly that rule
    /// rather than a wider one: a comment opens at the start of a line or after
    /// white space, and nowhere else. That is what keeps `url: http://host` a
    /// URL — the `//` there follows a `:` — and it means an author who already
    /// knows where a `#` may appear knows where a `//` may.
    fn opens_token(&self) -> bool {
        match self.at.checked_sub(1).and_then(|before| self.source.get(before)) {
            None => true,
            Some(c) => matches!(c, ' ' | '\t' | '\n'),
        }
    }

    fn matches(&self, wanted: &str) -> bool {
        wanted.chars().enumerate().all(|(offset, c)| self.source.get(self.at + offset) == Some(&c))
    }

    /// The index of the line break ending the line holding `from`, or the end
    /// of the text.
    fn line_end(&self, from: usize) -> usize {
        let mut at = from;
        while at < self.source.len() && self.source[at] != '\n' {
            at += 1;
        }
        at
    }

    /// Skip a quoted scalar opened at the scan position.
    fn quoted(&self, quote: char) -> usize {
        let mut at = self.at + 1;
        while at < self.source.len() {
            let c = self.source[at];
            if c == '\\' && quote == '"' {
                at += 2;
                continue;
            }
            at += 1;
            if c == quote {
                break;
            }
        }
        at
    }

    /// Whether a `|` or `>` at the scan position opens a block scalar — that
    /// is, whether the rest of its line holds only chomping and indentation
    /// indicators and perhaps a comment.
    fn is_block_header(&self) -> bool {
        let mut at = self.at + 1;
        while matches!(self.source.get(at), Some('+' | '-' | '0'..='9')) {
            at += 1;
        }
        while self.source.get(at) == Some(&' ') {
            at += 1;
        }
        matches!(self.source.get(at), None | Some('\n' | '#'))
    }

    /// Skip a block scalar's content: every following line that is blank or
    /// indented deeper than the line its header was written on.
    fn block_scalar(&self, indent: usize) -> usize {
        let mut at = self.line_end(self.at);
        while at < self.source.len() {
            let start = at + 1;
            let mut cursor = start;
            while self.source.get(cursor) == Some(&' ') {
                cursor += 1;
            }
            if !matches!(self.source.get(cursor), None | Some('\n')) && cursor - start <= indent {
                break;
            }
            at = self.line_end(start);
        }
        at
    }

    /// Rewrite `//` into `# ` and step over the comment it introduces.
    fn comment(&mut self) -> usize {
        self.out[self.at] = '#';
        self.out[self.at + 1] = ' ';
        self.line_end(self.at)
    }

    /// Read a `<?-- … >` region, fill it, and record it.
    fn block(&mut self) -> usize {
        let start = self.at;
        let open = start + 4;
        let Some((kind, close, end)) = self.terminator(open) else {
            let end = self.line_end(start);
            self.fill(start, end, BlockKind::Documentation);
            self.faults.push(Fault { start, end });
            return end;
        };
        self.fill(start, end, kind);
        self.regions.push(Region { kind, start, end, content: (open, close) });
        end
    }

    /// The first terminator at or after `from`, as `(kind, its start, its end)`.
    ///
    /// The **first** one closes the block, whatever the contents are. This
    /// language does not read them, so it has no basis on which to decide that
    /// a `-->` inside one was meant as text.
    fn terminator(&self, from: usize) -> Option<(BlockKind, usize, usize)> {
        let mut at = from;
        while at + 2 < self.source.len() {
            if self.source[at] == '-' && self.source[at + 1] == '-' {
                if self.source[at + 2] == '>' {
                    return Some((BlockKind::Code, at, at + 3));
                }
                if self.source[at + 2] == '!' && self.source.get(at + 3) == Some(&'>') {
                    return Some((BlockKind::Documentation, at, at + 4));
                }
            }
            at += 1;
        }
        None
    }

    /// Replace `[start, end)` with filler, keeping every line break.
    ///
    /// Documentation fills with spaces, so the parser sees nothing there. Code
    /// fills its **first line** with `.`, which is a plain scalar and therefore
    /// the node the block becomes; the rest is spaces, leaving blank lines that
    /// end that scalar and cannot disturb the enclosing indentation.
    fn fill(&mut self, start: usize, end: usize, kind: BlockKind) {
        let first = self.line_end(start);
        for at in start..end.min(self.out.len()) {
            if self.out[at] == '\n' {
                continue;
            }
            self.out[at] = if kind == BlockKind::Code && at < first { '.' } else { ' ' };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewritten(text: &str) -> String {
        preprocess(text, Dialect::Yamlfication).text
    }

    #[test]
    fn base_yaml_is_handed_to_the_parser_exactly_as_written() {
        let text = "a: 1 // not a comment\nb: <?-- x -->\n";
        let done = preprocess(text, Dialect::BaseYaml);
        assert_eq!(done.text, text);
        assert!(done.blocks.is_empty() && !done.changed);
    }

    #[test]
    fn a_line_comment_becomes_a_yaml_comment_of_the_same_length() {
        let text = "// note\na: 1  // trailing\n";
        assert_eq!(rewritten(text), "#  note\na: 1  #  trailing\n");
        assert_eq!(rewritten(text).chars().count(), text.chars().count());
    }

    #[test]
    fn a_slash_that_is_not_a_comment_is_left_alone() {
        assert_eq!(rewritten("url: http://host/thing\n"), "url: http://host/thing\n");
        assert_eq!(rewritten("a: \"x // y\"\n"), "a: \"x // y\"\n");
        assert_eq!(rewritten("# a // b\n"), "# a // b\n");
    }

    #[test]
    fn a_block_scalar_keeps_everything_written_inside_it() {
        let text = "script: |\n  // not a comment\n  <?-- not a block -->\nnext: 1\n";
        assert_eq!(rewritten(text), text);
    }

    #[test]
    fn a_documentation_block_becomes_white_space_and_is_captured() {
        let done = preprocess("a: <?-- why --!>\nb: 2\n", Dialect::Yamlfication);
        assert_eq!(done.text, "a:              \nb: 2\n");
        assert_eq!(done.blocks.len(), 1);
        assert_eq!(done.blocks[0].kind, BlockKind::Documentation);
        assert_eq!(done.blocks[0].text, " why ");
    }

    #[test]
    fn a_code_block_becomes_a_plain_scalar_of_its_own_length() {
        let done = preprocess("a: <?-- fn() -->\n", Dialect::Yamlfication);
        assert_eq!(done.text, "a: .............\n");
        assert_eq!(done.blocks[0].kind, BlockKind::Code);
        assert_eq!(done.blocks[0].text, " fn() ");
    }

    #[test]
    fn a_multi_line_code_block_fills_its_first_line_and_blanks_the_rest() {
        let done = preprocess("a: <?--\n  fn() {}\n  -->\nb: 2\n", Dialect::Yamlfication);
        assert_eq!(done.text, "a: ....\n         \n     \nb: 2\n");
        assert_eq!(done.blocks[0].text, "\n  fn() {}\n  ");
    }

    #[test]
    fn the_first_terminator_closes_the_block() {
        let done = preprocess("a: <?-- x --> y --!>\n", Dialect::Yamlfication);
        assert_eq!(done.blocks.len(), 1);
        assert_eq!(done.blocks[0].kind, BlockKind::Code);
        assert_eq!(done.blocks[0].text, " x ");
    }

    #[test]
    fn an_unterminated_block_costs_its_line_and_nothing_more() {
        let done = preprocess("a: <?-- oops\nb: 2\n", Dialect::Yamlfication);
        assert_eq!(done.text, "a:          \nb: 2\n");
        assert_eq!(done.faults.len(), 1);
        assert!(done.blocks.is_empty());
    }

    #[test]
    fn every_rewrite_preserves_the_character_count_and_the_line_breaks() {
        // Including every way a construct can run into the end of the file. The
        // pre-pass indexes characters by hand, so "it did not panic" is half of
        // what is being asserted here.
        for text in [
            "// c\na: <?-- d --!>\nb: <?--\n x\n-->\nc: 1 // t\n",
            "a: |\n  // keep\nb: <?-- e -->\n",
            "<?-- unterminated\nnext: 1\n",
            "a: <?-- é --!>\nb: 2\n",
            "",
            "/",
            "//",
            "<",
            "<?-",
            "<?--",
            "<?---",
            "<?-- --",
            "a: <?-- b --",
            "a: \"unterminated",
            "a: 'unterminated",
            "a: \"\\",
            "a: |",
            "a: >2-",
            "\n\n\n",
            "a: <?--\u{1f600}-->\n",
            "\u{feff}// bom\n",
        ] {
            let done = rewritten(text);
            assert_eq!(done.chars().count(), text.chars().count(), "{text:?}");
            assert_eq!(done.matches('\n').count(), text.matches('\n').count(), "{text:?}");
        }
    }
}
