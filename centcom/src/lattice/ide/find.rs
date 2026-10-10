//! Find and replace in the editor (Ctrl+F, Ctrl+H), Go to line (Ctrl+G), and the inline edit (Ctrl+K).
//!
//! The matches are lattice-core's (`text::find`, the matcher the folder's search uses too) over the open file's
//! text, each drawn as a gold box behind the text ([`marks`]) and the current one selected in the editor. A replace is
//! one edit, which the editor's undo takes back; nothing is written to disk before Save.
//!
//! The inline edit is the agent's, as everything the agent changes is: the selected lines and what the person asks
//! go to it as a message ([`inline_message`]), and its change waits in the review like any other.

use std::ops::Range;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::Tree;
use iced::advanced::{Widget, mouse};
use iced::{Border, Color, Element, Length, Rectangle, Size};

use crate::theme;
use lattice_core::text::find::{Matcher, Query};

use super::{CODE_PAD, LINE_HEIGHT};

/// The most matches found in one file: the count says when there are more.
pub const MAX_MATCHES: usize = 20_000;
/// The editor's tab stops (cosmic-text's, which iced's editor keeps).
pub const TAB: usize = 8;
/// The most lines an inline edit sends as they are (the agent reads longer ones from the file).
pub const INLINE_LINES: usize = 200;

/// A match: its line, its byte range in that line, and the cells it takes on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub cells: (usize, usize),
}

impl Hit {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// A find option, as its toggle names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// Aa: case as typed.
    Case,
    /// ab: whole words only.
    Word,
    /// .*: a regular expression.
    Regex,
}

/// Flip `toggle` on `query`.
pub fn flip(query: &mut Query, toggle: Toggle) {
    match toggle {
        Toggle::Case => query.case_sensitive = !query.case_sensitive,
        Toggle::Word => query.whole_word = !query.whole_word,
        Toggle::Regex => query.regex = !query.regex,
    }
}

/// The find bar of the editor tab on show.
#[derive(Debug)]
pub struct Find {
    pub tab: u64,
    pub query: Query,
    /// The replace row shows, with this text.
    pub replace: Option<String>,
    pub hits: Vec<Hit>,
    /// More matches than [`MAX_MATCHES`] were found.
    pub capped: bool,
    pub current: Option<usize>,
    /// Why the query matches nothing: it is not a valid regular expression.
    pub error: Option<String>,
    /// What the last Replace all did.
    pub note: Option<String>,
    /// Where the cursor was when the bar opened: a query typed matches onwards from there.
    pub origin: (usize, usize),
    matcher: Option<Matcher>,
}

impl Find {
    pub fn new(tab: u64, text: String, replace: bool, origin: (usize, usize)) -> Find {
        Find {
            tab,
            query: Query { text, ..Query::default() },
            replace: replace.then(String::new),
            hits: Vec::new(),
            capped: false,
            current: None,
            error: None,
            note: None,
            origin,
            matcher: None,
        }
    }

    /// Match the query against `lines` again; the current match becomes the first at or after `from` (a line and a
    /// byte in it), else the first.
    pub fn run<'a>(&mut self, lines: impl IntoIterator<Item = &'a str>, from: (usize, usize)) {
        self.hits.clear();
        self.capped = false;
        self.current = None;
        self.matcher = None;
        self.error = None;
        if self.query.text.is_empty() {
            return;
        }
        let matcher = match Matcher::new(&self.query) {
            Ok(matcher) => matcher,
            Err(why) => {
                self.error = Some(why);
                return;
            }
        };
        'lines: for (n, line) in lines.into_iter().enumerate() {
            for range in matcher.find_in(line) {
                if self.hits.len() == MAX_MATCHES {
                    self.capped = true;
                    break 'lines;
                }
                let cells = (cells(line, range.start), cells(line, range.end));
                self.hits.push(Hit { line: n, start: range.start, end: range.end, cells });
            }
        }
        self.matcher = Some(matcher);
        self.current = self.first_from(from);
    }

    fn first_from(&self, from: (usize, usize)) -> Option<usize> {
        if self.hits.is_empty() {
            return None;
        }
        Some(self.hits.iter().position(|h| (h.line, h.start) >= from).unwrap_or(0))
    }

    /// The next match (or the previous), wrapping round.
    pub fn step(&mut self, forward: bool) {
        let n = self.hits.len();
        if n == 0 {
            return;
        }
        self.current = Some(match self.current {
            None => 0,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        });
    }

    pub fn hit(&self) -> Option<Hit> {
        self.current.and_then(|i| self.hits.get(i).copied())
    }

    /// "3 of 17", "No results", or why the query matches nothing.
    pub fn status(&self) -> String {
        if let Some(why) = &self.error {
            return why.clone();
        }
        if self.query.text.is_empty() {
            return String::new();
        }
        let more = if self.capped { "+" } else { "" };
        match (self.current, self.hits.len()) {
            (_, 0) => "No results".to_string(),
            (Some(i), n) => format!("{} of {n}{more}", i + 1),
            (None, n) => format!("{n}{more}"),
        }
    }

    /// What replaces `hit` of `line`: the replace text, with a regular expression's `$1` filled in.
    pub fn replacement(&self, line: &str, hit: Hit) -> Option<String> {
        let with = self.replace.as_deref()?;
        Some(self.matcher.as_ref()?.replacement(line, hit.range(), with))
    }

    /// Each line with every match replaced, and how many were; `None` when nothing can be replaced.
    pub fn replace_lines<'a>(&self, lines: impl IntoIterator<Item = &'a str>) -> Option<(Vec<String>, usize)> {
        let with = self.replace.as_deref()?;
        let matcher = self.matcher.as_ref()?;
        let mut count = 0;
        let lines = lines
            .into_iter()
            .map(|line| {
                let (text, n) = matcher.replace_line(line, with);
                count += n;
                text
            })
            .collect();
        Some((lines, count))
    }
}

/// The cell byte `column` of `line` starts at on the screen: a tab runs to the next stop, a wide character takes two,
/// a combining mark none.
pub fn cells(line: &str, column: usize) -> usize {
    let head = line.get(..column).unwrap_or(line);
    head.chars().fold(0, |at, c| if c == '\t' { (at / TAB + 1) * TAB } else { at + super::term::vt::width(c) })
}

/// The byte where character `column` (from 0) of `line` starts, or the line's end.
pub fn byte_of(line: &str, column: usize) -> usize {
    line.char_indices().nth(column).map_or(line.len(), |(at, _)| at)
}

/// "120", "120:8" or ":8": the line (from 1) and column (from 1, in characters) Go to line goes to.
pub fn parse_goto(text: &str) -> Option<(Option<usize>, Option<usize>)> {
    let text = text.trim();
    let (line, column) = match text.split_once([':', ',']) {
        Some((l, c)) => (l.trim(), Some(c.trim())),
        None => (text, None),
    };
    let line = if line.is_empty() { None } else { Some(line.parse::<usize>().ok()?.max(1)) };
    let column = match column {
        Some(c) if !c.is_empty() => Some(c.parse::<usize>().ok()?.max(1)),
        _ => None,
    };
    (line.is_some() || column.is_some()).then_some((line, column))
}

/// The inline edit of an editor tab (Ctrl+K): the lines it is about (from 0, both included) and what is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    pub tab: u64,
    pub path: String,
    pub from: usize,
    pub to: usize,
    pub prompt: String,
}

/// The message the agent gets for an inline edit: the file and its lines, what to do, and the lines as they are now
/// (up to [`INLINE_LINES`]; past that the agent reads them from the file).
pub fn inline_message(path: &str, from: usize, to: usize, lines: &[&str], prompt: &str) -> String {
    let range = if from == to { format!("line {}", from + 1) } else { format!("lines {}-{}", from + 1, to + 1) };
    let mut out = format!("Edit @{path} {range}: {}\n\nChange only these lines unless the change needs more.", prompt.trim());
    if lines.len() <= INLINE_LINES {
        let fence = if lines.iter().any(|l| l.contains("```")) { "````" } else { "```" };
        let tag = path.rsplit_once('.').map_or("", |(_, ext)| ext);
        out.push_str(&format!(" They are now:\n{fence}{tag}\n{}\n{fence}", lines.join("\n")));
    } else {
        out.push_str(&format!(" ({} lines: read them from the file.)", lines.len()));
    }
    out
}

/// The matches of the editor on show, drawn as boxes behind its text (the editor's own background is clear, so they
/// show through it), the current one with an edge; and the lines an inline edit is about, as a band.
pub struct Marks<'a> {
    hits: &'a [Hit],
    current: Option<usize>,
    /// Lines (from 0, both included) an inline edit is about.
    band: Option<(usize, usize)>,
}

pub fn marks(hits: &[Hit], current: Option<usize>, band: Option<(usize, usize)>) -> Marks<'_> {
    Marks { hits, current, band }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Marks<'_>
where
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if let Some((from, to)) = self.band {
            let y = bounds.y + CODE_PAD + from as f32 * LINE_HEIGHT;
            let height = (to.saturating_sub(from) + 1) as f32 * LINE_HEIGHT;
            renderer.fill_quad(
                Quad { bounds: Rectangle { x: bounds.x, y, width: bounds.width, height }, ..Quad::default() },
                Color { a: 0.08, ..theme::GOLD },
            );
            renderer.fill_quad(Quad { bounds: Rectangle { x: bounds.x, y, width: 2.0, height }, ..Quad::default() }, theme::GOLD);
        }
        let w = super::term::cell().0;
        let top = ((viewport.y - bounds.y - CODE_PAD) / LINE_HEIGHT).floor().max(0.0) as usize;
        let bottom = ((viewport.y + viewport.height - bounds.y - CODE_PAD) / LINE_HEIGHT).ceil().max(0.0) as usize;
        let first = self.hits.partition_point(|h| h.line < top);
        for (i, hit) in self.hits.iter().enumerate().skip(first) {
            if hit.line > bottom {
                break;
            }
            let on = self.current == Some(i);
            let x = bounds.x + CODE_PAD + hit.cells.0 as f32 * w;
            let y = bounds.y + CODE_PAD + hit.line as f32 * LINE_HEIGHT;
            let width = ((hit.cells.1.saturating_sub(hit.cells.0)) as f32 * w).max(2.0);
            let edge = if on { Border { color: theme::GOLD, width: 1.0, radius: 2.0.into() } } else { Border { radius: 2.0.into(), ..Border::default() } };
            renderer.fill_quad(
                Quad { bounds: Rectangle { x, y, width, height: LINE_HEIGHT }, border: edge, ..Quad::default() },
                Color { a: if on { 0.30 } else { 0.17 }, ..theme::GOLD },
            );
        }
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer> From<Marks<'a>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(marks: Marks<'a>) -> Self {
        Element::new(marks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(text: &str, lines: &[&str], from: (usize, usize)) -> Find {
        let mut f = Find::new(1, text.to_string(), true, from);
        f.run(lines.iter().copied(), from);
        f
    }

    #[test]
    fn matches_are_found_in_order_from_the_cursor_and_the_steps_wrap() {
        let lines = ["let a = 1;", "a = a + 1;", "", "fn a() {}"];
        let mut f = find("a", &lines, (1, 2));
        assert_eq!(f.hits.len(), 4);
        assert_eq!(f.hit().map(|h| (h.line, h.start)), Some((1, 4)), "the first match at or after the cursor");
        assert_eq!(f.status(), "3 of 4");
        f.step(true);
        assert_eq!(f.hit().map(|h| (h.line, h.start)), Some((3, 3)));
        f.step(true);
        assert_eq!(f.current, Some(0), "past the last, the first");
        f.step(false);
        assert_eq!(f.current, Some(3), "before the first, the last");
        assert_eq!(find("zzz", &lines, (0, 0)).status(), "No results");
        assert_eq!(find("", &lines, (0, 0)).status(), "");
    }

    #[test]
    fn options_change_what_matches_and_a_bad_pattern_says_why() {
        let lines = ["Value value VALUE", "values"];
        let mut f = find("value", &lines, (0, 0));
        assert_eq!(f.hits.len(), 4);
        flip(&mut f.query, Toggle::Case);
        f.run(lines.iter().copied(), (0, 0));
        assert_eq!(f.hits.len(), 2);
        flip(&mut f.query, Toggle::Word);
        f.run(lines.iter().copied(), (0, 0));
        assert_eq!(f.hits.len(), 1, "values is not the word value");
        let mut bad = find("(", &lines, (0, 0));
        assert_eq!(bad.error, None, "as text it is fine");
        flip(&mut bad.query, Toggle::Regex);
        bad.run(lines.iter().copied(), (0, 0));
        assert!(bad.error.is_some() && bad.hits.is_empty());
        assert_eq!(bad.status(), bad.error.clone().unwrap());
    }

    #[test]
    fn a_replacement_fills_groups_in_and_replace_lines_counts() {
        let lines = ["a1 b2", "c3"];
        let mut f = find(r"(\w)(\d)", &lines, (0, 0));
        flip(&mut f.query, Toggle::Regex);
        f.run(lines.iter().copied(), (0, 0));
        f.replace = Some("$2$1".to_string());
        let hit = f.hit().unwrap();
        assert_eq!(f.replacement(lines[0], hit).as_deref(), Some("1a"));
        let (out, n) = f.replace_lines(lines.iter().copied()).unwrap();
        assert_eq!((out, n), (vec!["1a 2b".to_string(), "3c".to_string()], 3));
        f.replace = None;
        assert!(f.replace_lines(lines.iter().copied()).is_none(), "nothing to replace with when the row is shut");
    }

    #[test]
    fn cells_count_tabs_to_the_next_stop_and_wide_characters_twice() {
        assert_eq!(cells("\tx", 1), 8);
        assert_eq!(cells("ab\tx", 3), 8);
        assert_eq!(cells("日本x", "日本".len()), 4);
        assert_eq!(cells("e\u{301}x", "e\u{301}".len()), 1, "a combining mark takes no cell");
        assert_eq!(byte_of("héllo", 2), 3);
        assert_eq!(byte_of("ab", 9), 2);
    }

    #[test]
    fn go_to_line_reads_a_line_and_a_column() {
        assert_eq!(parse_goto("120"), Some((Some(120), None)));
        assert_eq!(parse_goto(" 12:8 "), Some((Some(12), Some(8))));
        assert_eq!(parse_goto(":4"), Some((None, Some(4))));
        assert_eq!(parse_goto("0"), Some((Some(1), None)));
        assert_eq!(parse_goto("x"), None);
        assert_eq!(parse_goto(""), None);
    }

    #[test]
    fn an_inline_edit_names_the_file_and_lines_and_carries_them() {
        let m = inline_message("src/a.rs", 4, 5, &["let x = 1;", "let y = 2;"], "  sum them ");
        assert_eq!(
            m,
            "Edit @src/a.rs lines 5-6: sum them\n\nChange only these lines unless the change needs more. They are now:\n```rs\nlet x = 1;\nlet y = 2;\n```"
        );
        assert!(inline_message("README.md", 0, 0, &["```"], "x").contains("````md\n```\n````"), "a fence in the lines");
        let many: Vec<&str> = vec!["x"; INLINE_LINES + 1];
        assert!(inline_message("a.txt", 0, INLINE_LINES, &many, "x").ends_with("(201 lines: read them from the file.)"));
    }
}
