//! The parts Pages builds its screens from, in its own look (`look`): round buttons, an action's icon that lights in
//! its colour under the pointer, tabs with a bar under the chosen one, a column's header, the cards beside the feed,
//! a section of the column, fields and choices, notices, chips and empty states.

use iced::widget::{Column, Row, button, column, container, hover, row, space, text, text_input, tooltip};
use iced::{Alignment, Background, Color, Element, Length, border};

use super::Msg;
use super::icons::{self, Glyph};
use super::look::{self, BODY, Palette, ROUND, SMALL};
use crate::theme::fonts;
use crate::ui::{caps, label, mono, strong};

type El<'a> = Element<'a, Msg>;

/// The palette on show.
pub fn p() -> &'static Palette {
    look::palette()
}

// ------------------------------------------------------------------- buttons

/// What a round button is for: the main act, X's Follow, or a quieter one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pill {
    Primary,
    Solid,
    Outline,
}

/// A round button, its words in semibold.
pub fn pill<'a>(words: impl Into<String>, msg: Option<Msg>, kind: Pill) -> El<'a> {
    sized_pill(words, msg, kind, 14.5, [7.0, 16.0])
}

/// The main act of a screen: Post, Save, Apply.
pub fn primary<'a>(words: impl Into<String>, msg: Option<Msg>) -> El<'a> {
    pill(words, msg, Pill::Primary)
}

/// A quieter act beside it, outlined.
pub fn outline<'a>(words: impl Into<String>, msg: Option<Msg>) -> El<'a> {
    pill(words, msg, Pill::Outline)
}

/// X's Following: outlined, and under the pointer Unfollow in the danger colour, which is what it does.
pub fn following<'a>(msg: Msg) -> El<'a> {
    let p = p();
    let face = |words: &'a str| container(text(words).size(14.5).font(fonts().ui_strong)).center_x(Length::Fill);
    let resting = button(face("Following")).width(Length::Fixed(112.0)).padding([7.0, 0.0]).on_press(msg.clone()).style(look::outline(p));
    let pointed = button(face("Unfollow")).width(Length::Fixed(112.0)).padding([7.0, 0.0]).on_press(msg).style(look::outline_in(p, p.danger));
    hover(resting, pointed)
}

/// A round button of a given size: the navigation's Post is the largest.
pub fn sized_pill<'a>(words: impl Into<String>, msg: Option<Msg>, kind: Pill, size: f32, padding: [f32; 2]) -> El<'a> {
    let b = button(text(words.into()).size(size).font(fonts().ui_strong).align_x(Alignment::Center))
        .padding(padding)
        .on_press_maybe(msg);
    match kind {
        Pill::Primary => b.style(look::primary(p())).into(),
        Pill::Solid => b.style(look::solid(p())).into(),
        Pill::Outline => b.style(look::outline(p())).into(),
    }
}

/// An icon on its own as a button (⋯, back, a composer's tool): round, washed in `tint` under the pointer.
pub fn icon_button<'a>(glyph: Glyph, size: f32, color: Color, tint: Color, msg: Option<Msg>, tip_words: &'a str) -> El<'a> {
    let p = p();
    let color = if msg.is_some() { color } else { look::tint(p, color, 0.5) };
    tip(button(icons::icon(glyph, size, color, false)).padding(7).on_press_maybe(msg).style(look::icon(p, tint)), tip_words)
}

/// One of a post's actions as X draws them: its icon and count, dim until the pointer is on it, when both take
/// `tint` over a round wash; `lit` draws it in its colour already (a like given, a post saved), `filled` fills the
/// heart and the bookmark.
pub fn action<'a>(glyph: Glyph, count: u64, tint: Color, lit: bool, msg: Option<Msg>, tip_words: &'a str) -> El<'a> {
    let p = p();
    let n = if count == 0 { String::new() } else { super::view::compact(count) };
    let face = move |color: Color, washed: bool| -> El<'a> {
        let disc = container(icons::icon(glyph, 18.75, color, lit)).padding(8).style(move |_| container::Style {
            background: washed.then(|| Background::Color(look::wash(p, tint))),
            border: border::rounded(ROUND),
            ..container::Style::default()
        });
        row![disc, mono(n.clone(), 12.5, color)].spacing(1).align_y(Alignment::Center).into()
    };
    let resting = face(if lit { tint } else { p.dim }, false);
    let content: El<'a> = if msg.is_some() { hover(resting, face(tint, true)) } else { resting };
    tip(button(content).padding(0).on_press_maybe(msg).style(look::bare(p)), tip_words)
}

/// A name that acts, in semibold, with nothing round it until the pointer is on it.
pub fn strong_link<'a>(words: impl Into<String>, size: f32, color: Color, msg: Option<Msg>) -> El<'a> {
    button(strong(words.into(), size, color)).padding(0).on_press_maybe(msg).style(look::bare(p())).into()
}

/// A row of a menu: an icon and what it does; in the danger colour when it deletes; greyed when it cannot be done.
pub fn menu_item<'a>(glyph: Glyph, words: impl Into<String>, msg: Option<Msg>, danger: bool) -> El<'a> {
    let p = p();
    let color = match (msg.is_some(), danger) {
        (false, _) => look::tint(p, p.dim, 0.7),
        (true, true) => p.danger,
        (true, false) => p.text,
    };
    button(row![icons::icon(glyph, 18.0, color, false), strong(words.into(), BODY, color)].spacing(12).align_y(Alignment::Center))
        .padding([11.0, 16.0])
        .width(Length::Fill)
        .on_press_maybe(msg)
        .style(look::menu_row(p))
        .into()
}

/// A menu: its rows on a floating card (`popover` lays it over the page).
pub fn menu<'a>(items: Column<'a, Msg>, width: f32) -> El<'a> {
    container(items).padding([6.0, 0.0]).width(Length::Fixed(width)).style(look::menu(p())).into()
}

// ------------------------------------------------------------------- Pages' own marks

/// An organization's ticker as a market writes one: its letters in the mono face, in the accent, in a fine frame;
/// pressed, its page. Tickers are Pages' own (every organization has one), so they are drawn as nothing else is.
pub fn ticker<'a>(letters: &str, msg: Option<Msg>) -> El<'a> {
    let p = p();
    let face = container(mono(letters.to_string(), 12.0, p.accent)).padding([1.0, 5.0]).style(look::ticker(p));
    button(face).padding(0).on_press_maybe(msg).style(look::bare(p)).into()
}

/// What kind of post this is, over its words: ARTICLE, JOB, POLL, in the brand's small spaced capitals.
pub fn kind<'a>(words: &'a str) -> El<'a> {
    caps(words, 10.5, p().accent)
}

/// A figure in the mono face, as a ledger sets its numbers: a time, a count.
pub fn figure<'a>(words: impl Into<String>, size: f32, color: Color) -> El<'a> {
    mono(words.into(), size, color).into()
}

/// A choice among a few views, as Pages sets one: their names side by side, the chosen one in the accent over a
/// short rule as wide as its words, the others quiet. It sits beside a title, not across the column as X's tabs do.
pub fn switch<'a, T: Copy + PartialEq>(all: &[T], chosen: T, title: impl Fn(T) -> String, pick: impl Fn(T) -> Msg) -> El<'a> {
    let p = p();
    let mut r = Row::new().spacing(2).align_y(Alignment::End);
    for &choice in all {
        let on = choice == chosen;
        let words = title(choice);
        let face: El<'a> = if on { strong(words, 15.0, p.accent).into() } else { label(words, 15.0, p.dim).into() };
        let rule: El<'a> = if on { container(space()).width(Length::Fill).height(2).style(look::bar(p)).into() } else { space().height(2).into() };
        let inner = column![face, rule].spacing(5).width(Length::Shrink);
        r = r.push(button(inner).padding([8.0, 10.0]).on_press(pick(choice)).style(look::switch_item(p)));
    }
    r.into()
}

// ------------------------------------------------------------------- tabs and headers

/// Tabs as X draws them: each an equal share of the row, the chosen one's words bold over a bar in the accent, a
/// hairline under them all.
pub fn tabs<'a, T: Copy + PartialEq>(all: &[T], chosen: T, title: impl Fn(T) -> String, pick: impl Fn(T) -> Msg) -> El<'a> {
    column![container(switch(all, chosen, title, pick)).padding(iced::padding::left(6).top(4)), hairline()].into()
}

/// A column's header as Pages sets one: a way back when the screen is reached from another; its title in the display
/// face over a short accent rule, as CENTCOM sets a page's title; a line under it in the mono face (a handle, what
/// was searched); and at its right what changes the view (a `switch`).
pub fn header<'a>(back: Option<Msg>, title: impl Into<String>, under: Option<String>, right: Option<El<'a>>) -> El<'a> {
    let p = p();
    let mut r = Row::new().spacing(16).align_y(Alignment::Center);
    if let Some(msg) = back {
        r = r.push(icon_button(Glyph::Back, 20.0, p.text, p.text, Some(msg), "Back"));
    }
    let mut words = Column::new().spacing(4).push(text(title.into()).size(21.0).font(fonts().display).color(p.text));
    match under {
        Some(u) => words = words.push(mono(u, 12.0, p.dim)),
        None => words = words.push(container(space()).width(28).height(2).style(look::bar(p))),
    }
    r = r.push(words.width(Length::Fill));
    if let Some(right) = right {
        r = r.push(right);
    }
    container(r).padding([8.0, 16.0]).height(Length::Fixed(60.0)).align_y(Alignment::Center).width(Length::Fill).into()
}

// ------------------------------------------------------------------- surfaces

/// A hairline across the column.
pub fn hairline<'a>() -> El<'a> {
    container(space()).width(Length::Fill).height(1).style(look::hairline(p())).into()
}

/// A hairline down a column's side.
pub fn side_line<'a>() -> El<'a> {
    container(space()).width(1).height(Length::Fill).style(look::hairline(p())).into()
}

/// A card beside the feed, in the brand's manner: its title in small spaced capitals in the accent, as the sign-in
/// screen labels its parts; its rows under it (each tinted under the pointer by its own button), and "Show more"
/// when there is more.
pub fn card<'a>(title: &'a str, rows: Vec<El<'a>>, more: Option<Msg>) -> El<'a> {
    let p = p();
    let mut col = Column::new().push(container(caps(title, 11.0, p.accent)).padding(iced::padding::top(16).bottom(8).left(16).right(16)));
    for r in rows {
        col = col.push(r);
    }
    if let Some(m) = more {
        col = col.push(button(label("Show more", BODY, p.accent)).padding([14.0, 16.0]).width(Length::Fill).on_press(m).style(look::card_row(p)));
    }
    container(col.push(space().height(6))).width(Length::Fill).style(look::card(p)).into()
}

/// A part of the column: its title, what it holds, and a hairline under it.
pub fn section<'a>(title: impl Into<String>, body: Column<'a, Msg>) -> El<'a> {
    let p = p();
    column![
        container(column![caps(title.into(), 11.5, p.accent), body.spacing(12)].spacing(14)).padding([16.0, 16.0]).width(Length::Fill),
        hairline(),
    ]
    .into()
}

/// A row of the column: what it holds, padded as a post is, with a hairline under it.
pub fn block<'a>(content: impl Into<El<'a>>) -> El<'a> {
    column![container(content).padding([12.0, 16.0]).width(Length::Fill), hairline()].into()
}

/// A heading of the column, as a card beside it is titled: small spaced capitals in the accent, padded as a row is.
pub fn column_title<'a>(words: impl Into<String>) -> El<'a> {
    container(caps(words.into(), 11.5, p().accent)).padding(iced::padding::top(22).bottom(10).left(16).right(16)).width(Length::Fill).into()
}

/// Something of the column padded as a row is, without a hairline: a note under a list, a row of filters.
pub fn padded<'a>(content: impl Into<El<'a>>) -> El<'a> {
    container(content).padding([12.0, 16.0]).width(Length::Fill).into()
}

/// A small part's title inside a section: semibold, in the words' colour.
pub fn part_title<'a>(words: impl Into<String>) -> El<'a> {
    strong(words.into(), 17.0, p().text).into()
}

/// What the column says when it has nothing to show: a line in the display face, then why.
pub fn empty<'a>(title: impl Into<String>, words: impl Into<String>) -> El<'a> {
    let p = p();
    let title = text(title.into()).size(22.0).font(fonts().display).color(p.text);
    container(column![title, label(words.into(), BODY, p.dim)].spacing(8).max_width(420))
        .padding([36.0, 32.0])
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

/// A notice in its colour: something went wrong, or needs the reader.
pub fn notice<'a>(words: impl Into<String>, color: Color) -> El<'a> {
    let p = p();
    container(label(words.into(), 14.0, p.text)).padding([12.0, 16.0]).width(Length::Fill).style(look::notice(p, color)).into()
}

/// A small pill of words in a colour.
pub fn chip<'a>(words: impl Into<String>, color: Color) -> El<'a> {
    container(label(words.into(), 12.5, color)).padding([3.0, 10.0]).style(look::chip(p(), color)).into()
}

/// A line a person reads before trusting what is shown: where it came from, or what it cannot say.
pub fn note<'a>(words: impl Into<String>) -> El<'a> {
    label(words.into(), SMALL, p().dim).into()
}

/// What `content` is, or does, under the pointer.
pub fn tip<'a>(content: impl Into<El<'a>>, words: impl text::IntoFragment<'a>) -> El<'a> {
    let p = p();
    tooltip(content, container(label(words, 12.5, p.text)).padding([5.0, 9.0]).max_width(360).style(look::menu(p)), tooltip::Position::Bottom)
        .gap(4)
        .into()
}

// ------------------------------------------------------------------- fields

/// A form's field.
pub fn field<'a>(hint: &'a str, value: &'a str, on: impl Fn(String) -> Msg + 'a) -> text_input::TextInput<'a, Msg> {
    text_input(hint, value).on_input(on).padding([10.0, 12.0]).size(BODY).font(fonts().ui).style(look::field(p()))
}

/// A field with its name above it, as a professional network's forms set theirs.
pub fn labeled<'a>(name: &'a str, field: impl Into<El<'a>>) -> El<'a> {
    column![label(name, SMALL, p().dim), field.into()].spacing(6).into()
}

/// A choice from a list, framed as a field.
pub fn picked<'a, T, L, V>(list: iced::widget::PickList<'a, T, L, V, Msg>) -> iced::widget::PickList<'a, T, L, V, Msg>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
{
    list.padding([8.0, 12.0]).text_size(14.0).font(fonts().ui).style(look::picker(p())).menu_style(look::picker_menu(p()))
}
