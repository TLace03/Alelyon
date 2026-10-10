//! The words, chips, cards and buttons of the pages that keep their own messages (Fleet, Data, Compute), drawn as
//! `view.rs` draws the others: the same sizes, fonts and black-and-gold styles (`crate::theme`), for any message type.

use iced::widget::{Column, Row, Text, button, column, container, row, space, text};
use iced::{Alignment, Color, Element, Length};

use crate::theme::{self, fonts};

use crate::spinner::spinner;

pub fn label<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(content).size(size).color(color).font(fonts().ui)
}

pub fn strong<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(content).size(size).color(color).font(fonts().ui_strong)
}

pub fn mono<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(content).size(size).color(color).font(fonts().mono)
}

pub fn dot<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(space().width(8).height(8)).style(theme::dot(color)).into()
}

pub fn chip<'a, M: 'a>(words: impl text::IntoFragment<'a>, color: Color) -> Element<'a, M> {
    container(label(words, 11.5, color)).padding([2.0, 8.0]).style(theme::chip(color)).into()
}

/// A page's title in the display face over a short gold rule, and what the page is for.
pub fn heading<'a, M: 'a>(title: &'a str, purpose: &'a str) -> Element<'a, M> {
    column![
        title_text(title),
        container(space()).width(Length::Fixed(RULE_W)).height(Length::Fixed(2.0)).style(theme::rule),
        label(purpose, 14.0, theme::TEXT_DIM),
    ]
    .spacing(9)
    .into()
}

/// How wide the gold rule under a page's title runs.
pub const RULE_W: f32 = 220.0;

/// A page's title, in the display face.
pub fn title_text<'a>(title: impl text::IntoFragment<'a>) -> Text<'a> {
    text(title).size(30.0).color(theme::TEXT).font(fonts().display)
}

/// A part of a page, or a card's title: small spaced capitals in gold, as the sign-in screen's labels are.
pub fn subheading<'a, M: 'a>(words: impl text::IntoFragment<'a>) -> Element<'a, M> {
    caps(words, 11.5, theme::GOLD)
}

/// The space between two letters of small capitals, and between two words, as shares of the size.
pub const LETTER_GAP: f32 = 0.12;
pub const WORD_GAP: f32 = 0.42;

/// Words in small spaced capitals, semibold. iced sets no letter spacing, and a hair space between letters would be a
/// place to break a line (or, beside a word joiner, comes out narrower in some fonts), so each letter is set on its
/// own, `LETTER_GAP` apart, and each word kept whole: a line breaks only between words.
pub fn caps<'a, M: 'a>(words: impl text::IntoFragment<'a>, size: f32, color: Color) -> Element<'a, M> {
    let font = fonts().ui_strong;
    let mut line = Row::new().spacing(size * WORD_GAP);
    for word in capitals(&words.into_fragment()) {
        let mut letters = Row::new().spacing(size * LETTER_GAP);
        for c in word.chars() {
            letters = letters.push(text(c.to_string()).size(size).color(color).font(font));
        }
        line = line.push(letters);
    }
    line.wrap().vertical_spacing(size * 0.3).into()
}

/// The words of a label in capitals, each kept whole.
pub fn capitals(words: &str) -> Vec<String> {
    words.split_whitespace().map(str::to_uppercase).collect()
}

/// A spinner with what it is doing.
pub fn working<'a, M: 'a>(phase: f32, words: impl text::IntoFragment<'a>) -> Element<'a, M> {
    row![spinner(phase, 16.0), label(words, 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center).into()
}

// A button's words take the button's own colour, so a disabled one looks disabled.
pub fn primary<'a, M: Clone + 'a>(words: &'a str, message: Option<M>) -> Element<'a, M> {
    button(text(words).size(13.5).font(fonts().ui_strong)).padding([7.0, 16.0]).on_press_maybe(message).style(theme::primary_button).into()
}

pub fn secondary<'a, M: Clone + 'a>(words: &'a str, message: Option<M>) -> Element<'a, M> {
    button(text(words).size(13.0).font(fonts().ui)).padding([6.0, 12.0]).on_press_maybe(message).style(theme::secondary_button).into()
}

/// A row of tabs: each a segment, the chosen one gold.
pub fn tabs<'a, M: Clone + 'a, T: Copy + PartialEq>(
    all: &[T],
    chosen: T,
    title: impl Fn(T) -> String,
    pick: impl Fn(T) -> M,
) -> Element<'a, M> {
    let mut tabs = Row::new().spacing(8);
    for &tab in all {
        let on = tab == chosen;
        tabs = tabs.push(
            button(label(title(tab), 13.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([7.0, 14.0])
                .on_press(pick(tab))
                .style(theme::segment_button(on)),
        );
    }
    tabs.wrap().into()
}

/// A panel holding a titled column.
pub fn card<'a, M: 'a>(title: impl text::IntoFragment<'a>, body: Column<'a, M>) -> Element<'a, M> {
    container(column![subheading(title), body.spacing(8)].spacing(10)).padding(16).width(Length::Fill).style(theme::panel).into()
}

/// One fact: its name in a fixed column, its value beside it.
pub fn fact<'a, M: 'a>(name: impl text::IntoFragment<'a>, value: impl Into<Element<'a, M>>) -> Element<'a, M> {
    row![container(label(name, 12.5, theme::TEXT_DIM)).width(150), value.into()].spacing(10).align_y(Alignment::Center).into()
}

/// A line of words a person reads before trusting what is shown: where it came from, or what it cannot say.
pub fn note<'a, M: 'a>(words: impl text::IntoFragment<'a>) -> Element<'a, M> {
    label(words, 12.0, theme::TEXT_FAINT).into()
}

/// A notice in its colour: something went wrong, or needs a person.
pub fn notice<'a, M: 'a>(words: impl text::IntoFragment<'a>, color: Color) -> Element<'a, M> {
    container(label(words, 13.0, theme::TEXT)).padding([10.0, 14.0]).width(Length::Fill).style(theme::notice(color)).into()
}

/// Something this build, or this person, does not have (`capability::absent`): what it is, why it is not here and how
/// to get it, on a calm panel. An empty state, never an error.
pub fn absent<'a, M: 'a>(a: crate::capability::Absent) -> Element<'a, M> {
    container(
        column![
            strong(a.what, 14.5, theme::TEXT),
            label(a.why, 13.0, theme::TEXT_DIM),
            label(a.how, 13.0, theme::GOLD),
        ]
        .spacing(6),
    )
    .padding([14.0, 16.0])
    .width(Length::Fill)
    .style(theme::panel)
    .into()
}

/// A feature of the plan with how far it has moved in, as `view.rs` draws one on the catalogue pages.
pub fn feature_row<'a, M: 'a>(feature: &'a crate::catalogue::Feature) -> Element<'a, M> {
    use crate::catalogue::Here;
    let mut chips = Row::new().spacing(6);
    chips = chips.push(match feature.here() {
        Here::Now => chip("Here now", theme::POSITIVE),
        Here::InPart => chip("Here in part", theme::GOLD),
        Here::Later if feature.step == "Later" => chip("Later", theme::TEXT_FAINT),
        Here::Later => chip(format!("Step {}", feature.step), theme::GOLD_DIM),
    });
    if feature.no_screen && feature.here() == Here::Later {
        chips = chips.push(chip("No screen today", theme::CAUTION));
    }
    container(
        column![
            row![strong(feature.name, 14.5, theme::TEXT), space().width(Length::Fill), chips].align_y(Alignment::Center),
            label(feature.what, 13.0, theme::TEXT_DIM),
            label(format!("Today: {}. Plan: {}.", feature.today, feature.plan), 12.0, theme::TEXT_FAINT),
        ]
        .spacing(5),
    )
    .padding([12.0, 16.0])
    .width(Length::Fill)
    .style(theme::panel)
    .into()
}

/// A text box, as `view.rs` draws one: gold when it has the focus (`theme::input`).
pub fn input_style(t: &iced::Theme, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    theme::input(t, status)
}

/// Bytes in the unit a person reads: "7.2 GB".
pub fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1} MB", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1} KB", n / 1e3)
    } else {
        format!("{n} B")
    }
}

/// At most `n` characters of `s`, with an ellipsis when cut.
pub fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

// ------------------------------------------------------------------- long lists, built only where they show

/// Where a long list's scrollable stands: the top of the part on show and its height, as the scrollable's `on_scroll`
/// last said. Until it has said (it says on its first frame when the list is taller than it), a screenful is built
/// from the top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scrolled {
    pub offset: f32,
    pub height: f32,
}

impl Scrolled {
    /// The height built before the scrollable has measured itself: taller than the window's own.
    pub const FIRST_GUESS: f32 = 1600.0;

    pub fn of(viewport: iced::widget::scrollable::Viewport) -> Scrolled {
        Scrolled { offset: viewport.absolute_offset().y, height: viewport.bounds().height }
    }
}

impl Default for Scrolled {
    fn default() -> Scrolled {
        Scrolled { offset: 0.0, height: Scrolled::FIRST_GUESS }
    }
}

/// The rows of a long list that are built: `first..end`, with the space standing for the rows above and below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub first: usize,
    pub end: usize,
    pub above: f32,
    pub below: f32,
}

/// Height built beyond each edge of the part on show, so a short scroll shows rows at once, before the list is
/// rebuilt for where it now stands.
pub const MARGIN: f32 = 480.0;

/// Which of the rows with these heights `at` shows, with `MARGIN` either side. An offset past the end (the list
/// has just become shorter) is read as the end.
pub fn shown(heights: &[f32], at: Scrolled) -> Shown {
    let total: f32 = heights.iter().sum();
    let height = at.height.max(0.0);
    let offset = at.offset.min(total - height).max(0.0);
    let (top, bottom) = (offset - MARGIN, offset + height + MARGIN);
    let (mut first, mut above) = (0, 0.0);
    while first < heights.len() && above + heights[first] <= top {
        above += heights[first];
        first += 1;
    }
    let (mut end, mut y) = (first, above);
    while end < heights.len() && y < bottom {
        y += heights[end];
        end += 1;
    }
    Shown { first, end, above, below: (total - y).max(0.0) }
}

/// A long list built only where it shows: `row(i)` for each row `shown` names, each held to its height (rows taller
/// than theirs are cut), and empty space for the rest, so the scrollbar still measures the whole list. The list must
/// stand at the top of its scrollable, and a row's height must be the height it draws at, or rows jump as it scrolls.
pub fn virtual_rows<'a, M: 'a>(heights: &[f32], at: Scrolled, mut row: impl FnMut(usize) -> Element<'a, M>) -> Column<'a, M> {
    let s = shown(heights, at);
    let mut col = Column::new().width(Length::Fill);
    if s.above > 0.0 {
        col = col.push(space().height(s.above));
    }
    for (i, h) in heights.iter().enumerate().take(s.end).skip(s.first) {
        col = col.push(container(row(i)).width(Length::Fill).height(*h).clip(true));
    }
    if s.below > 0.0 {
        col = col.push(space().height(s.below));
    }
    col
}

/// The height iced lays one line of text out at, at `size` (its default line height, 1.3 times the size).
pub fn line_height(size: f32) -> f32 {
    size * 1.3
}

// ------------------------------------------------------------------- drawings kept between frames

/// A canvas drawing kept between frames, made again only when what it shows (`key`) or its size changes: a canvas
/// without one is tessellated again on every frame the window draws, whatever caused the frame.
#[derive(Default)]
pub struct Kept {
    cache: iced::widget::canvas::Cache,
    key: std::cell::Cell<Option<u64>>,
}

impl Kept {
    pub fn draw(
        &self,
        renderer: &iced::Renderer,
        size: iced::Size,
        key: u64,
        draw: impl FnOnce(&mut iced::widget::canvas::Frame),
    ) -> iced::widget::canvas::Geometry {
        if self.key.replace(Some(key)) != Some(key) {
            self.cache.clear();
        }
        self.cache.draw(renderer, size, draw)
    }
}

/// A key for `Kept` from anything hashable: what the drawing is made from.
pub fn key_of(what: &impl std::hash::Hash) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    BuildHasherDefault::<DefaultHasher>::default().hash_one(what)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset: f32, height: f32) -> Scrolled {
        Scrolled { offset, height }
    }

    #[test]
    fn a_list_at_the_top_builds_what_shows_and_the_margin_below() {
        let heights = vec![20.0; 1000];
        let s = shown(&heights, at(0.0, 400.0));
        // 400 on show and 480 below: 44 rows of 20
        assert_eq!((s.first, s.end), (0, 44));
        assert_eq!(s.above, 0.0);
        assert_eq!(s.below, 20.0 * (1000 - 44) as f32);
    }

    #[test]
    fn scrolled_halfway_the_list_builds_a_window_and_pads_both_sides() {
        let heights = vec![20.0; 1000];
        let s = shown(&heights, at(10_000.0, 400.0));
        // from 10000-480 to 10400+480: rows 476 up to 544
        assert_eq!((s.first, s.end), (476, 544));
        assert_eq!(s.above, 476.0 * 20.0);
        assert_eq!(s.above + (s.end - s.first) as f32 * 20.0 + s.below, 20_000.0, "the padding keeps the whole height");
    }

    #[test]
    fn an_offset_past_the_end_shows_the_end_and_an_empty_list_shows_nothing() {
        let heights = vec![10.0; 50];
        let s = shown(&heights, at(9_999.0, 100.0));
        assert_eq!(s.end, 50, "the last rows are built");
        assert!(s.first < 40, "a screenful before them too");
        assert_eq!(s.below, 0.0);
        assert_eq!(shown(&[], at(0.0, 400.0)), Shown { first: 0, end: 0, above: 0.0, below: 0.0 });
        let short = shown(&heights, at(0.0, 1000.0));
        assert_eq!((short.first, short.end, short.above, short.below), (0, 50, 0.0, 0.0), "a list shorter than its view is all built");
    }

    #[test]
    fn rows_of_two_heights_are_found_by_their_running_total() {
        // a heading of 30 then nine rows of 10, repeated
        let heights: Vec<f32> = (0..1000).map(|i| if i % 10 == 0 { 30.0 } else { 10.0 }).collect();
        let s = shown(&heights, at(6_000.0, 120.0));
        let top_of = |i: usize| heights[..i].iter().sum::<f32>();
        assert_eq!(s.above, top_of(s.first));
        assert!(top_of(s.first) <= 6_000.0 - MARGIN && top_of(s.first + 1) > 6_000.0 - MARGIN);
        assert!(top_of(s.end) >= 6_120.0 + MARGIN && top_of(s.end - 1) < 6_120.0 + MARGIN);
        assert_eq!(s.above + heights[s.first..s.end].iter().sum::<f32>() + s.below, heights.iter().sum::<f32>());
    }

    #[test]
    fn before_the_scrollable_has_measured_itself_a_screenful_is_built() {
        let heights = vec![20.0; 10_000];
        let s = shown(&heights, Scrolled::default());
        assert_eq!(s.first, 0);
        assert_eq!(s.end, ((Scrolled::FIRST_GUESS + MARGIN) / 20.0) as usize);
        assert!(s.end < 200, "not ten thousand rows");
    }

    #[test]
    fn a_kept_drawing_has_one_key_per_content() {
        assert_eq!(key_of(&(1, "a")), key_of(&(1, "a")));
        assert_ne!(key_of(&(1, "a")), key_of(&(2, "a")));
    }

    #[test]
    fn small_capitals_keep_each_word_whole() {
        assert_eq!(capitals("How Sinai listens"), ["HOW", "SINAI", "LISTENS"]);
        assert_eq!(capitals("  Moving   in "), ["MOVING", "IN"], "runs of spaces are one gap");
        assert_eq!(capitals("Straße"), ["STRASSE"], "upper case as Unicode says");
        assert!(capitals("").is_empty());
    }

    #[test]
    fn sizes_and_cuts_read_as_a_person_reads_them() {
        assert_eq!(bytes(7_220_736_000), "7.2 GB");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(cut("abcdef", 4), "abc…");
        assert_eq!(cut("abc", 4), "abc");
    }
}
