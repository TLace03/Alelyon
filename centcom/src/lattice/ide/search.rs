//! The Search view (Ctrl+Shift+F): a person's search across the open folder through lattice-core's `search`, which
//! reads exactly the files the agent's grep reads (the folder's own rules, so ignored files and secrets are never
//! searched), counting what it could not read. Each matching line opens its file there with the match selected.
//!
//! A search runs when asked (Enter, or a toggle changed with a query typed), on a thread of its own: never on every
//! keystroke, and nothing runs while the view is idle.

use std::collections::HashSet;

use iced::widget::{Column, Row, button, column, container, rich_text, row, scrollable, span, text_input};
use iced::{Alignment, Element, Length};

use crate::theme::{self, fonts};
use lattice_core::text::find::Query;
use lattice_core::tools::read::{FileMatches, LineMatch, SearchReport};

use super::find::Toggle;
use super::view::{file_dot, ghost, section_title};
use crate::lattice::ide::{IdeMsg, file_name};
use crate::lattice::{Msg, State};
use crate::ui::{self, label, mono, note};

/// The most matches a search returns; the count says when there are more.
pub const MAX_RESULTS: usize = 2_000;
/// How many characters of a line before its first match the view keeps, the rest cut with an ellipsis.
const LEAD: usize = 24;

#[derive(Debug, Default)]
pub struct SearchView {
    pub query: Query,
    /// Only files whose path fits this glob ("*.rs", "src/**").
    pub include: String,
    pub running: bool,
    /// The query and the glob the report answers.
    pub asked: Option<(Query, String)>,
    pub report: Option<Result<SearchReport, String>>,
    /// Files whose lines are folded away.
    pub collapsed: HashSet<String>,
}

pub fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-search")
}

type El<'a> = Element<'a, Msg>;

fn go(msg: IdeMsg) -> Msg {
    Msg::Ide(msg)
}

/// A find option's toggle: its glyph, gold while on.
pub fn toggle<'a>(glyph: &'a str, tip: &'a str, on: bool, msg: Msg) -> El<'a> {
    let b = button(mono(glyph, 11.5, if on { theme::GOLD } else { theme::TEXT_FAINT }))
        .padding([3.0, 6.0])
        .style(theme::list_row(on))
        .on_press(msg);
    iced::widget::tooltip(
        b,
        container(label(tip, 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// The three toggles of a query, each sending `msg(toggle)`.
pub fn toggles<'a>(query: &Query, msg: impl Fn(Toggle) -> Msg) -> El<'a> {
    row![
        toggle("Aa", "Match case", query.case_sensitive, msg(Toggle::Case)),
        toggle("ab", "Whole words only", query.whole_word, msg(Toggle::Word)),
        toggle(".*", "A regular expression", query.regex, msg(Toggle::Regex)),
    ]
    .spacing(2)
    .into()
}

/// What the line shows of `line`: from shortly before its first match (an ellipsis for what is cut), as spans with
/// each match on a gold ground.
fn line_spans<'a>(line: &'a LineMatch) -> Vec<iced::widget::text::Span<'a, (), iced::Font>> {
    let first = line.ranges.first().map_or(0, |r| r.start);
    let before = line.text[..first].chars().count();
    let from = if before > LEAD { line.text[..first].char_indices().nth(before - LEAD).map_or(0, |(at, _)| at) } else { 0 };
    let mut spans = Vec::new();
    if from > 0 {
        spans.push(span("\u{2026}").color(theme::TEXT_FAINT));
    }
    let mut at = from;
    for r in &line.ranges {
        if r.start > at {
            spans.push(span(line.text[at..r.start].trim_start_matches(|c: char| at == from && c.is_whitespace())).color(theme::TEXT_DIM));
        }
        spans.push(
            span(&line.text[r.start.max(at)..r.end])
                .color(theme::TEXT)
                .background(theme::with_alpha(theme::GOLD, 0.30)),
        );
        at = r.end.max(at);
    }
    if at < line.text.len() {
        spans.push(span(&line.text[at..]).color(theme::TEXT_DIM));
    }
    spans
}

/// One row of the results: a file's heading (1 below the file before it), or one of its lines while it is unfolded.
#[derive(Clone, Copy)]
enum ResultRow<'a> {
    File { file: &'a FileMatches, folded: bool, first: bool },
    Line { file: &'a FileMatches, line: &'a LineMatch },
}

impl ResultRow<'_> {
    /// The height each row draws at: a heading's 12.5 line and its 3 above and below (and the 1 between files); a
    /// line's 12 and its 2 above and below.
    fn height(&self) -> f32 {
        match self {
            ResultRow::File { first, .. } => 12.5 * 1.3 + 6.0 + if *first { 0.0 } else { 1.0 },
            ResultRow::Line { .. } => 12.0 * 1.3 + 4.0,
        }
    }
}

/// The results as rows, in order: each file's heading, then its lines unless it is folded.
fn result_rows<'a>(files: &'a [FileMatches], folded: &HashSet<String>) -> Vec<ResultRow<'a>> {
    let mut rows = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let shut = folded.contains(&file.path);
        rows.push(ResultRow::File { file, folded: shut, first: i == 0 });
        if !shut {
            rows.extend(file.lines.iter().map(|line| ResultRow::Line { file, line }));
        }
    }
    rows
}

fn file_head<'a>(file: &'a FileMatches, folded: bool) -> El<'a> {
    let (dir, name) = match file.path.rsplit_once('/') {
        Some((dir, _)) => (dir, file_name(&file.path)),
        None => ("", file.path.as_str()),
    };
    let count: usize = file.lines.iter().map(|l| l.ranges.len()).sum();
    let head = button(
        row![
            label(if folded { "\u{25B8}" } else { "\u{25BE}" }, 11.0, theme::TEXT_FAINT),
            file_dot(&file.path),
            label(name, 12.5, theme::TEXT),
            label(dir, 11.0, theme::TEXT_FAINT),
            iced::widget::space().width(Length::Fill),
            container(label(count.to_string(), 10.5, theme::TEXT_DIM)).padding([0.0, 6.0]).style(theme::chip(theme::TEXT_FAINT)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([3.0, 8.0])
    .style(theme::ghost_button)
    .on_press(go(IdeMsg::SearchFold(file.path.clone())));
    head.into()
}

fn line_row<'a>(file: &'a FileMatches, line: &'a LineMatch) -> El<'a> {
    let first = line.ranges.first().cloned().unwrap_or(0..0);
    let text = rich_text::<(), _, _, _>(line_spans(line)).font(fonts().mono).size(12.0).wrapping(iced::widget::text::Wrapping::None);
    button(row![mono(format!("{:>4}", line.line), 11.0, theme::TEXT_FAINT), text].spacing(8).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding(iced::Padding { top: 2.0, bottom: 2.0, left: 22.0, right: 8.0 })
        .style(theme::ghost_button)
        .on_press(go(IdeMsg::SearchOpen(file.path.clone(), line.line, first.start, first.end)))
        .into()
}

/// What a report could not search, or cut, in words; empty when nothing.
pub fn caveats(report: &SearchReport) -> Vec<String> {
    let mut out = Vec::new();
    let n = |count: usize, one: &str, many: &str| if count == 1 { one.to_string() } else { format!("{count} {many}") };
    // Matches not shown: past the cap, or past the 2,000th character of a long line (where a line is cut).
    if report.shown >= MAX_RESULTS && report.matches > report.shown {
        out.push(format!("Showing the first {} of {} results.", report.shown, report.matches));
    } else if report.matches > report.shown {
        let past = report.matches - report.shown;
        out.push(format!("{} past the 2,000th character of a long line, not shown.", n(past, "1 match", "matches")));
    }
    if report.too_large > 0 {
        out.push(format!("{} not searched: over 2 MiB.", n(report.too_large, "1 file", "files")));
    }
    if report.binary > 0 {
        out.push(format!("{} not searched: binary.", n(report.binary, "1 file", "files")));
    }
    if report.unreadable > 0 {
        out.push(format!("{} could not be read.", n(report.unreadable, "1 file", "files")));
    }
    if report.truncated {
        out.push("The folder lists more files than are looked at (the first 100,000).".to_string());
    }
    if !report.withheld.is_empty() {
        out.push(format!("Not searched, as git cannot read their ignore rules: {}.", report.withheld.join(", ")));
    }
    out
}

/// The side bar's Search.
pub fn view<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let s = &ide.search;
    let mut col = Column::new().spacing(6);
    col = col.push(section_title("Search", vec![]));
    if ide.folder.is_none() {
        col = col.push(container(note("Open a folder to search it.")).padding([0.0, 12.0]));
        return col.into();
    }
    let input = text_input("Search the folder", &s.query.text)
        .id(input_id())
        .on_input(|t| go(IdeMsg::SearchQuery(t)))
        .on_submit(go(IdeMsg::SearchRun))
        .padding([6.0, 8.0])
        .size(13)
        .style(ui::input_style);
    col = col.push(
        container(row![input, toggles(&s.query, |t| go(IdeMsg::SearchToggle(t)))].spacing(4).align_y(Alignment::Center))
            .padding([0.0, 8.0]),
    );
    col = col.push(
        container(
            text_input("Files to include: *.rs, src/**", &s.include)
                .on_input(|t| go(IdeMsg::SearchInclude(t)))
                .on_submit(go(IdeMsg::SearchRun))
                .padding([5.0, 8.0])
                .size(12)
                .style(ui::input_style),
        )
        .padding([0.0, 8.0]),
    );
    let mut status = Row::new().spacing(8).align_y(Alignment::Center);
    if s.running {
        status = status.push(crate::spinner::spinner(phase, 12.0)).push(note("Searching\u{2026}"));
    }
    match &s.report {
        Some(Ok(report)) if !s.running => {
            let files = report.files_matched;
            status = status.push(label(
                match report.matches {
                    0 => "No results.".to_string(),
                    1 => "1 result in 1 file.".to_string(),
                    n => format!("{n} results in {files} file{}.", if files == 1 { "" } else { "s" }),
                },
                12.0,
                theme::TEXT_DIM,
            ));
            if !report.files.is_empty() {
                status = status.push(iced::widget::space().width(Length::Fill));
                status = status.push(ghost("Fold all", Some(go(IdeMsg::SearchFoldAll))));
            }
        }
        Some(Err(why)) if !s.running => status = status.push(label(why.as_str(), 12.0, theme::CAUTION)),
        _ => {}
    }
    col = col.push(container(status).padding([2.0, 12.0]));
    if let Some(Ok(report)) = &s.report {
        let mut notes = Column::new().spacing(2);
        for words in caveats(report) {
            notes = notes.push(label(words, 11.0, theme::TEXT_FAINT));
        }
        col = col.push(container(notes).padding([0.0, 12.0]));
        // Up to 2,000 lines: only those on show (and a margin) are built.
        let shown = result_rows(&report.files, &s.collapsed);
        let heights: Vec<f32> = shown.iter().map(ResultRow::height).collect();
        let list = ui::virtual_rows(&heights, ide.search_at, |i| match shown[i] {
            ResultRow::File { file, folded, first } => {
                let head = file_head(file, folded);
                if first { head } else { container(head).padding(iced::padding::top(1)).into() }
            }
            ResultRow::Line { file, line } => line_row(file, line),
        });
        col = col.push(
            scrollable(list)
                .on_scroll(|v| go(IdeMsg::SearchScrolled(ui::Scrolled::of(v))))
                .height(Length::Fill)
                .style(theme::scrollbars),
        );
    } else if s.report.is_none() && !s.running {
        col = col.push(
            container(column![
                note("Finds text in the folder's files: those the agent may read, never an ignored file or a secret."),
                note("Enter searches. Aa matches case, ab whole words, .* a regular expression."),
            ]
            .spacing(6))
            .padding([4.0, 12.0]),
        );
    }
    col.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(line: &LineMatch) -> Vec<String> {
        line_spans(line).into_iter().map(|s| s.text.to_string()).collect()
    }

    #[test]
    fn a_line_shows_its_matches_and_little_before_the_first() {
        let short = LineMatch { line: 3, text: "let alpha = beta(alpha);".into(), ranges: vec![4..9, 17..22] };
        assert_eq!(texts(&short), ["let ", "alpha", " = beta(", "alpha", ");"]);
        let long_text = format!("{}needle end", "x".repeat(60));
        let long = LineMatch { line: 1, text: long_text, ranges: vec![60..66] };
        let shown = texts(&long);
        assert_eq!(shown[0], "\u{2026}");
        assert_eq!(shown[1].chars().count(), LEAD, "the lead before the match");
        assert_eq!(&shown[2..], ["needle", " end"]);
    }

    #[test]
    fn the_results_are_rows_of_known_height_and_a_folded_file_hides_its_lines() {
        let line = |n| LineMatch { line: n, text: "needle".into(), ranges: vec![0..6] };
        let files = vec![
            FileMatches { path: "a.rs".into(), lines: vec![line(1), line(2)] },
            FileMatches { path: "b/c.rs".into(), lines: vec![line(7)] },
        ];
        let rows = result_rows(&files, &HashSet::new());
        let kinds: Vec<&str> = rows.iter().map(|r| if matches!(r, ResultRow::File { .. }) { "file" } else { "line" }).collect();
        assert_eq!(kinds, ["file", "line", "line", "file", "line"]);
        let heights: Vec<f32> = rows.iter().map(ResultRow::height).collect();
        assert_eq!(heights[0] + 1.0, heights[3], "the 1 between files belongs to the later heading");
        assert_eq!(heights[1], heights[4]);
        let folded = result_rows(&files, &HashSet::from(["a.rs".to_string()]));
        assert_eq!(folded.len(), 3, "a.rs's two lines are not rows while it is folded");
    }

    #[test]
    fn what_a_search_could_not_read_or_cut_is_said() {
        let report = SearchReport {
            matches: 2500,
            shown: 2000,
            too_large: 1,
            binary: 3,
            truncated: true,
            withheld: vec!["vendor/".into()],
            ..SearchReport::default()
        };
        assert_eq!(
            caveats(&report),
            [
                "Showing the first 2000 of 2500 results.",
                "1 file not searched: over 2 MiB.",
                "3 files not searched: binary.",
                "The folder lists more files than are looked at (the first 100,000).",
                "Not searched, as git cannot read their ignore rules: vendor/.",
            ]
        );
        assert!(caveats(&SearchReport::default()).is_empty());
        let cut = SearchReport { matches: 5, shown: 4, ..SearchReport::default() };
        assert_eq!(caveats(&cut), ["1 match past the 2,000th character of a long line, not shown."], "not a cap");
    }
}
