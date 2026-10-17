//! Pages' own look (2026-10-15): its colours as a palette, and every style Pages draws with, read from the palette on
//! show. Pages is drawn as the social sites draw theirs rather than in the window's gold and carbon black: one column
//! of posts between hairlines, round buttons, its words larger, and its accent kept for what acts and what links.
//!
//! Three palettes, as X offers three backgrounds: Onyx (black and the brand's gold, the default), Midnight (a
//! blue-grey night with blue) and Paper (white and ink, with a deep gold). One is on show for the whole process
//! (`show`); the window's own pages keep `crate::theme`.
//!
//! Invariants:
//! - Every colour of words holds 4.5:1 (WCAG 2.x) on every ground its palette sets it on, washes taken over what
//!   they lie on, and the main button's words hold it on the button in each state that acts. The tests below
//!   assert it for each palette, so a new colour or ground joins them.
//! - Every style is a function of its palette and state only; none reads the iced `Theme`.

use std::sync::atomic::{AtomicU8, Ordering};

use iced::widget::{button, checkbox, container, pick_list, scrollable, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector, border, overlay};

/// The colours of one look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub name: &'static str,
    /// The page, behind everything.
    pub canvas: Color,
    /// What stands on the page: the cards beside the feed.
    pub surface: Color,
    /// The search field, and a row of a card under the pointer.
    pub raised: Color,
    /// A row of the feed under the pointer.
    pub hover: Color,
    /// The hairlines between posts, and the edge of a round frame.
    pub line: Color,
    pub text: Color,
    /// Handles, times, counts and notes.
    pub dim: Color,
    /// Links, hashtags, the chosen tab's bar and the composer's tools.
    pub accent: Color,
    /// The main button's fill, under the pointer, and its words.
    pub fill: Color,
    pub fill_hover: Color,
    pub on_fill: Color,
    /// A like and a repost, coloured as the social sites colour them.
    pub like: Color,
    pub repost: Color,
    pub caution: Color,
    pub danger: Color,
    /// A light palette: softer shadows, and a frame round a card that the surface alone would not show.
    pub light: bool,
}

const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Black and the brand's gold: the window's own colours, with X's greys for its hairlines and quieter words.
pub static ONYX: Palette = Palette {
    name: "Onyx",
    canvas: rgb(0x000000),
    surface: rgb(0x16181c),
    raised: rgb(0x202327),
    hover: rgb(0x0a0a0b),
    line: rgb(0x2f3336),
    text: rgb(0xe7e9ea),
    dim: rgb(0x8b9095),
    accent: rgb(0xe6c46a),
    fill: rgb(0xe6c46a),
    fill_hover: rgb(0xf0d38b),
    on_fill: rgb(0x0a0a0a),
    like: rgb(0xf91880),
    repost: rgb(0x00ba7c),
    caution: rgb(0xea8027),
    danger: rgb(0xff5c66),
    light: false,
};

/// A blue-grey night with blue, as X's dim background.
pub static MIDNIGHT: Palette = Palette {
    name: "Midnight",
    canvas: rgb(0x15202b),
    surface: rgb(0x1e2732),
    raised: rgb(0x273340),
    hover: rgb(0x1a2633),
    line: rgb(0x38444d),
    text: rgb(0xf7f9f9),
    dim: rgb(0x9aa6b2),
    accent: rgb(0x1d9bf0),
    fill: rgb(0x1a73c8),
    fill_hover: rgb(0x1666b3),
    on_fill: rgb(0xffffff),
    like: rgb(0xff4f9a),
    repost: rgb(0x00ba7c),
    caution: rgb(0xffad1f),
    danger: rgb(0xff7a82),
    light: false,
};

/// White and ink, with a deep gold: a professional network's light page in the brand's colour.
pub static PAPER: Palette = Palette {
    name: "Paper",
    canvas: rgb(0xffffff),
    surface: rgb(0xf7f9f9),
    raised: rgb(0xeff3f4),
    hover: rgb(0xf7f7f7),
    line: rgb(0xe1e8ed),
    text: rgb(0x0f1419),
    dim: rgb(0x536471),
    accent: rgb(0x7f6210),
    fill: rgb(0x0f1419),
    fill_hover: rgb(0x272c30),
    on_fill: rgb(0xffffff),
    like: rgb(0xc8104f),
    repost: rgb(0x007a51),
    caution: rgb(0xa14a07),
    danger: rgb(0xc4231c),
    light: true,
};

/// Which palette is on show. The pictures of the screens draw each (`snapshots`); the reader does not choose one
/// yet, and Onyx is shown.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Which {
    #[default]
    Onyx,
    Midnight,
    Paper,
}

#[cfg_attr(not(test), allow(dead_code))]
impl Which {
    pub const ALL: [Which; 3] = [Which::Onyx, Which::Midnight, Which::Paper];

    pub fn palette(self) -> &'static Palette {
        match self {
            Which::Onyx => &ONYX,
            Which::Midnight => &MIDNIGHT,
            Which::Paper => &PAPER,
        }
    }
}

static ON_SHOW: AtomicU8 = AtomicU8::new(0);

/// The palette on show.
pub fn palette() -> &'static Palette {
    match ON_SHOW.load(Ordering::Relaxed) {
        1 => &MIDNIGHT,
        2 => &PAPER,
        _ => &ONYX,
    }
}

/// Show `which` from the next frame on.
#[cfg_attr(not(test), allow(dead_code))]
pub fn show(which: Which) {
    ON_SHOW.store(which as u8, Ordering::Relaxed);
}

// ----------------------------------------------------------------- sizes

/// A post's words, a name and a handle.
pub const BODY: f32 = 15.0;
/// Counts, notes and the small words under a name.
pub const SMALL: f32 = 13.0;
/// The navigation's words: smaller than X's, as CENTCOM's own rail sets its pages.
pub const NAV: f32 = 16.5;
/// Round enough to be a pill at any height.
pub const ROUND: f32 = 999.0;

// ----------------------------------------------------------------- colours

/// `color` at `alpha`.
pub fn alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// `a` mixed `t` (0..1) of the way to `b`, as one solid colour.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let m = |x: f32, y: f32| x + (y - x) * t;
    Color::from_rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// `color` laid on the page `t` (0..1) of the way: a solid colour, not one with alpha, because the window blends alpha
/// in linear light, where a tenth of gold over black already shows as a mid brown (`crate::theme`'s note), and the
/// pictures of the screens blend otherwise.
pub fn tint(p: &Palette, color: Color, t: f32) -> Color {
    mix(p.canvas, color, t)
}

/// The light wash of a colour behind an icon under the pointer, a chosen choice, a notice.
pub fn wash(p: &Palette, color: Color) -> Color {
    tint(p, color, 0.12)
}

/// The colour of a person's or an organization's initial on its disc, from its name: one of six, so a page of
/// initials is not one colour.
pub fn initial_color(p: &Palette, name: &str) -> Color {
    let hues: [u32; 6] = if p.light {
        [0x7f6210, 0x1a5fa8, 0x8a2f6b, 0x2f6b3a, 0x8a3b1d, 0x4b4fa8]
    } else {
        [0xe6c46a, 0x7fb6f0, 0xe58fc4, 0x7fd19b, 0xf0a77f, 0xa9acf2]
    };
    let sum = name.chars().fold(0u32, |n, c| n.wrapping_mul(31).wrapping_add(c as u32));
    rgb(hues[(sum % 6) as usize])
}

fn bg(color: Color) -> Option<Background> {
    Some(Background::Color(color))
}

fn edge(width: f32, color: Color, radius: f32) -> Border {
    Border { color, width, radius: radius.into() }
}

fn shadow(p: &Palette) -> Shadow {
    if p.light {
        Shadow { color: Color::from_rgba(0.06, 0.08, 0.1, 0.18), offset: Vector::new(0.0, 4.0), blur_radius: 18.0 }
    } else {
        Shadow { color: Color::from_rgba(1.0, 1.0, 1.0, 0.10), offset: Vector::new(0.0, 0.0), blur_radius: 16.0 }
    }
}

// ----------------------------------------------------------------- surfaces

/// The page: the canvas, with the palette's words.
pub fn page(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(p.canvas), text_color: Some(p.text), ..container::Style::default() }
}

/// A card beside the feed: the surface, rounded; on paper also edged, where the surface alone barely shows.
pub fn card(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.surface),
        text_color: Some(p.text),
        border: edge(if p.light { 1.0 } else { 0.0 }, p.line, 16.0),
        ..container::Style::default()
    }
}

/// What floats over the page: a menu, a tip, a choice of reactions.
pub fn menu(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.canvas),
        text_color: Some(p.text),
        border: edge(1.0, p.line, 12.0),
        shadow: shadow(p),
        ..container::Style::default()
    }
}

/// A round frame on the page: a quoted post, a poll, a job, a link's card.
pub fn frame(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { text_color: Some(p.text), border: edge(1.0, p.line, 16.0), ..container::Style::default() }
}

/// A frame laid over a photo, so its edge shows on a page of its own colour.
pub fn photo_edge(p: &'static Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { border: edge(1.0, p.line, radius), ..container::Style::default() }
}

/// The backdrop of a photo open over the page: near black in every palette, as the social sites darken theirs.
pub fn scrim(_: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(Color::from_rgba(0.0, 0.0, 0.0, 0.92)),
        text_color: Some(ONYX.text),
        ..container::Style::default()
    }
}

/// A round button on that backdrop: a light veil, stronger under the pointer.
pub fn over_photo(_: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: bg(Color::from_rgba(1.0, 1.0, 1.0, if pressed_or_hovered(status) { 0.22 } else { 0.12 })),
        text_color: ONYX.text,
        border: border::rounded(ROUND),
        ..button::Style::default()
    }
}

/// A shape standing for words or a picture while they are read: the raised grey, rounded by `radius`.
pub fn bone(p: &'static Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(p.raised), border: border::rounded(radius), ..container::Style::default() }
}

/// A hairline between posts, or along a column's side.
pub fn hairline(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(p.line), ..container::Style::default() }
}

/// A plain band where a page has no banner.
pub fn band(p: &'static Palette, hue: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        // from its top left corner, the page's own colour laid deep on the canvas, fading to the canvas's surface
        let fade = iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_4 * 3.0))
            .add_stop(0.0, tint(p, hue, 0.32))
            .add_stop(1.0, mix(p.surface, hue, 0.04));
        container::Style { background: Some(Background::Gradient(fade.into())), ..container::Style::default() }
    }
}

/// A ring of the page's colour round a page's picture, over the banner's edge: round for a person, a rounded square
/// for an organization.
pub fn ring(p: &'static Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(p.canvas), border: border::rounded(radius), ..container::Style::default() }
}

/// A disc behind an initial where there is no picture, tinted in the initial's colour; `radius` rounds it.
pub fn disc(p: &'static Palette, color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(mix(p.canvas, color, 0.16)),
        border: edge(1.0, tint(p, color, 0.35), radius),
        ..container::Style::default()
    }
}

/// A row washed in `color`: an unread notification.
pub fn washed(p: &'static Palette, color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(tint(p, color, 0.06)), ..container::Style::default() }
}

/// A notice in its colour: something went wrong, or needs the reader.
pub fn notice(p: &'static Palette, color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(wash(p, color)),
        text_color: Some(p.text),
        border: edge(1.0, tint(p, color, 0.45), 12.0),
        ..container::Style::default()
    }
}

/// A small pill of words in its colour.
pub fn chip(p: &'static Palette, color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(tint(p, color, 0.14)),
        text_color: Some(color),
        border: border::rounded(ROUND),
        ..container::Style::default()
    }
}

/// A count on an icon: the accent, round.
pub fn badge(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.fill),
        text_color: Some(p.on_fill),
        border: edge(2.0, p.canvas, ROUND),
        ..container::Style::default()
    }
}

/// The bar under the chosen tab.
pub fn bar(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(p.accent), border: border::rounded(ROUND), ..container::Style::default() }
}

/// A share of a poll's votes, as a bar behind the option: the reader's choice washed in the accent.
pub fn share(p: &'static Palette, chosen: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(if chosen { tint(p, p.accent, 0.2) } else { p.raised }),
        border: border::rounded(6.0),
        ..container::Style::default()
    }
}

// ----------------------------------------------------------------- buttons

fn pressed_or_hovered(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

/// A row of the feed or of a list: bare, tinted under the pointer.
pub fn row(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: pressed_or_hovered(status).then_some(Background::Color(p.hover)),
        text_color: p.text,
        ..button::Style::default()
    }
}

/// The row the keyboard is on: washed in the accent, a little more under the pointer.
pub fn chosen_row(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(tint(p, p.accent, if pressed_or_hovered(status) { 0.1 } else { 0.06 }))),
        text_color: p.text,
        ..button::Style::default()
    }
}

/// A row of a card beside the feed: raised under the pointer.
pub fn card_row(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: pressed_or_hovered(status).then_some(Background::Color(p.raised)),
        text_color: p.text,
        ..button::Style::default()
    }
}

/// A row of a menu: tinted under the pointer.
pub fn menu_row(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: pressed_or_hovered(status).then_some(Background::Color(p.surface)),
        text_color: p.text,
        ..button::Style::default()
    }
}

/// The row of a menu the keyboard is on: tinted as a row under the pointer is, whether or not it is.
pub fn chosen_menu_row(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style { background: bg(p.surface), text_color: p.text, ..button::Style::default() }
}

/// A line of the navigation: the place on show washed in the accent behind a fine accent edge, as CENTCOM's own rail
/// marks its page; the others tinted under the pointer.
pub fn nav(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    nav_item(p, false)
}

pub fn nav_item(p: &'static Palette, on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (fill, line) = if on {
            (Some(tint(p, p.accent, 0.1)), tint(p, p.accent, 0.4))
        } else if pressed_or_hovered(status) {
            (Some(p.surface), Color::TRANSPARENT)
        } else {
            (None, Color::TRANSPARENT)
        };
        button::Style { background: fill.map(Background::Color), text_color: p.text, border: edge(1.0, line, 12.0), ..button::Style::default() }
    }
}

/// An organization's ticker, as a market writes one: a fine accent frame on a faint accent wash.
pub fn ticker(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(tint(p, p.accent, 0.07)),
        text_color: Some(p.accent),
        border: edge(1.0, tint(p, p.accent, 0.45), 4.0),
        ..container::Style::default()
    }
}

/// The brand's mark beside the wordmark: the accent's letter in a fine accent square.
pub fn mark(p: &'static Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(tint(p, p.accent, 0.1)), border: edge(1.5, p.accent, 10.0), ..container::Style::default() }
}

/// One choice of a switch: bare, tinted under the pointer.
pub fn switch_item(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: pressed_or_hovered(status).then_some(Background::Color(p.hover)),
        text_color: p.text,
        border: border::rounded(8.0),
        ..button::Style::default()
    }
}

/// A tool of the composer: a rounded square washed faintly in the accent, more strongly while it is on or under the
/// pointer.
pub fn tool(p: &'static Palette, on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let strength = if on {
            0.2
        } else if pressed_or_hovered(status) {
            0.14
        } else {
            0.06
        };
        button::Style {
            background: bg(tint(p, p.accent, strength)),
            text_color: p.accent,
            border: edge(1.0, tint(p, p.accent, if on { 0.6 } else { 0.25 }), 9.0),
            ..button::Style::default()
        }
    }
}

/// The main button: the fill and its words; faded when it cannot act, as the social sites fade theirs.
pub fn primary(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (fill, words) = match status {
            button::Status::Active => (p.fill, p.on_fill),
            button::Status::Hovered | button::Status::Pressed => (p.fill_hover, p.on_fill),
            button::Status::Disabled => (tint(p, p.fill, 0.45), tint(p, p.on_fill, 0.6)),
        };
        button::Style { background: bg(fill), text_color: words, border: border::rounded(ROUND), ..button::Style::default() }
    }
}

/// A button in the words' own colour, X's Follow: the text colour behind the page's.
pub fn solid(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (fill, words) = match status {
            button::Status::Active => (p.text, p.canvas),
            button::Status::Hovered | button::Status::Pressed => (mix(p.text, p.canvas, 0.12), p.canvas),
            button::Status::Disabled => (tint(p, p.text, 0.45), p.canvas),
        };
        button::Style { background: bg(fill), text_color: words, border: border::rounded(ROUND), ..button::Style::default() }
    }
}

/// An outlined button: Following, Edit page, Save draft.
pub fn outline(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (fill, words, line) = match status {
            button::Status::Active => (None, p.text, p.line),
            button::Status::Hovered | button::Status::Pressed => (bg(p.hover), p.text, p.dim),
            button::Status::Disabled => (None, p.dim, p.line),
        };
        button::Style { background: fill, text_color: words, border: edge(1.0, line, ROUND), ..button::Style::default() }
    }
}

/// An outlined button in a colour: a choice of a poll, Unfollow.
pub fn outline_in(p: &'static Palette, color: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = pressed_or_hovered(status);
        let words = if status == button::Status::Disabled { p.dim } else { color };
        button::Style {
            background: hovered.then(|| Background::Color(wash(p, color))),
            text_color: words,
            border: edge(1.0, if hovered { color } else { tint(p, color, 0.6) }, ROUND),
            ..button::Style::default()
        }
    }
}

/// An icon's button: bare, a round wash of `color` under the pointer.
pub fn icon(p: &'static Palette, color: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: pressed_or_hovered(status).then(|| Background::Color(wash(p, color))),
        text_color: color,
        border: border::rounded(ROUND),
        ..button::Style::default()
    }
}

/// Words that act: bare in every state, their colour their own.
pub fn bare(p: &'static Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style { text_color: p.text, ..button::Style::default() }
}

/// A choice among several (a reaction, a kind of search): washed in the accent when it is the one chosen.
pub fn choice(p: &'static Palette, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = pressed_or_hovered(status);
        let (fill, line) = if chosen {
            (bg(wash(p, p.accent)), p.accent)
        } else if hovered {
            (bg(p.hover), p.dim)
        } else {
            (None, p.line)
        };
        button::Style { background: fill, text_color: p.text, border: edge(1.0, line, ROUND), ..button::Style::default() }
    }
}

// ----------------------------------------------------------------- what is typed in, picked and scrolled

fn field_state(status: text_input::Status) -> (bool, bool) {
    match status {
        text_input::Status::Focused { is_hovered } => (true, is_hovered),
        text_input::Status::Hovered => (false, true),
        text_input::Status::Active | text_input::Status::Disabled => (false, false),
    }
}

/// The search field: a raised pill, the page's colour with an accent edge while it has the focus.
pub fn search(p: &'static Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let (focused, _) = field_state(status);
        text_input::Style {
            background: Background::Color(if focused { p.canvas } else { p.raised }),
            border: edge(1.0, if focused { p.accent } else { p.raised }, ROUND),
            icon: p.dim,
            placeholder: p.dim,
            value: p.text,
            selection: alpha(p.accent, 0.3),
        }
    }
}

/// A form's field: the page's colour inside a hairline, the accent while it has the focus.
pub fn field(p: &'static Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let (focused, hovered) = field_state(status);
        let line = if focused {
            p.accent
        } else if hovered {
            p.dim
        } else {
            p.line
        };
        text_input::Style {
            background: Background::Color(p.canvas),
            border: edge(if focused { 2.0 } else { 1.0 }, line, 8.0),
            icon: p.dim,
            placeholder: p.dim,
            value: p.text,
            selection: alpha(p.accent, 0.3),
        }
    }
}

/// A field without a frame: what the composer and a reply are written in.
pub fn bare_field(p: &'static Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, _| text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: p.dim,
        placeholder: p.dim,
        value: p.text,
        selection: alpha(p.accent, 0.3),
    }
}

/// The composer's text: no frame, as the social sites' composers have none.
pub fn bare_editor(p: &'static Palette) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, _| text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: p.dim,
        value: p.text,
        selection: alpha(p.accent, 0.3),
    }
}

/// A longer text in a form (a biography): framed as a field.
pub fn editor(p: &'static Palette) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, status| {
        let line = match status {
            text_editor::Status::Focused { .. } => p.accent,
            text_editor::Status::Hovered => p.dim,
            _ => p.line,
        };
        text_editor::Style {
            background: Background::Color(p.canvas),
            border: edge(1.0, line, 8.0),
            placeholder: p.dim,
            value: p.text,
            selection: alpha(p.accent, 0.3),
        }
    }
}

/// A choice from a list, framed as a field.
pub fn picker(p: &'static Palette) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |_, status| {
        let hovered = matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened { .. });
        pick_list::Style {
            text_color: p.text,
            placeholder_color: p.dim,
            handle_color: if hovered { p.accent } else { p.dim },
            background: Background::Color(p.canvas),
            border: edge(1.0, if hovered { p.dim } else { p.line }, 8.0),
        }
    }
}

/// A choice set as accent words in a pill, as X sets who may reply.
pub fn audience(p: &'static Palette) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |_, status| {
        let hovered = matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened { .. });
        pick_list::Style {
            text_color: p.accent,
            placeholder_color: p.dim,
            handle_color: p.accent,
            background: Background::Color(if hovered { wash(p, p.accent) } else { Color::TRANSPARENT }),
            border: edge(1.0, tint(p, p.accent, if hovered { 0.8 } else { 0.45 }), ROUND),
        }
    }
}

/// The open list of a choice.
pub fn picker_menu(p: &'static Palette) -> impl Fn(&Theme) -> overlay::menu::Style {
    move |_| overlay::menu::Style {
        background: Background::Color(p.canvas),
        border: edge(1.0, p.line, 12.0),
        text_color: p.text,
        selected_text_color: p.text,
        selected_background: Background::Color(p.surface),
        shadow: shadow(p),
    }
}

/// A box to tick: the main button's fill when ticked.
pub fn tick(p: &'static Palette) -> impl Fn(&Theme, checkbox::Status) -> checkbox::Style {
    move |_, status| {
        let (checked, hovered) = match status {
            checkbox::Status::Active { is_checked } => (is_checked, false),
            checkbox::Status::Hovered { is_checked } => (is_checked, true),
            checkbox::Status::Disabled { is_checked } => (is_checked, false),
        };
        checkbox::Style {
            background: Background::Color(if checked { p.fill } else { p.canvas }),
            icon_color: p.on_fill,
            border: edge(if checked { 0.0 } else { 2.0 }, if hovered { p.accent } else { p.dim }, 4.0),
            text_color: Some(p.text),
        }
    }
}

/// Thin round scrollers on no rail, as the social sites keep theirs out of the way.
pub fn scrollers(p: &'static Palette) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut style = scrollable::default(theme, status);
        let thumb = match status {
            scrollable::Status::Active { .. } => tint(p, p.dim, 0.3),
            scrollable::Status::Hovered { .. } => tint(p, p.dim, 0.6),
            scrollable::Status::Dragged { .. } => p.dim,
        };
        for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = Background::Color(thumb);
            rail.scroller.border = border::rounded(ROUND);
        }
        style.container = container::Style::default();
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x relative luminance and contrast, as `lattice_app::theme` computes them.
    fn contrast(a: Color, b: Color) -> f64 {
        lattice_app::theme::contrast(a, b)
    }

    fn at_least(what: String, words: Color, ground: Color, failures: &mut Vec<String>) {
        let ratio = contrast(words, ground);
        if ratio < 4.5 {
            failures.push(format!("{what}: {ratio:.2}"));
        }
    }

    #[test]
    fn every_palettes_words_hold_45_to_1_where_they_are_set() {
        let mut failures = Vec::new();
        for which in Which::ALL {
            let p = which.palette();
            // the grounds words lie on: the page, a card, a field, a row under the pointer, an accent wash (a chosen
            // choice, an icon's circle), an unread row, a poll's chosen share and a notice's wash
            let grounds = [
                ("canvas", p.canvas),
                ("surface", p.surface),
                ("raised", p.raised),
                ("hover", p.hover),
                ("accent wash", wash(p, p.accent)),
                ("unread", tint(p, p.accent, 0.06)),
                ("chosen row under the pointer", tint(p, p.accent, 0.1)),
                ("the place on show", tint(p, p.accent, 0.1)),
                ("a ticker", tint(p, p.accent, 0.07)),
                ("a tool on", tint(p, p.accent, 0.2)),
                ("chosen share", tint(p, p.accent, 0.2)),
                ("caution notice", wash(p, p.caution)),
            ];
            for (ground, under) in grounds {
                for (name, words) in [("text", p.text), ("dim", p.dim)] {
                    at_least(format!("{} {name} on {ground}", p.name), words, under, &mut failures);
                }
            }
            // the accent's words: links and hashtags on the page and its rows, "Show more" on a card
            for (ground, under) in [("canvas", p.canvas), ("hover", p.hover), ("surface", p.surface)] {
                at_least(format!("{} accent on {ground}", p.name), p.accent, under, &mut failures);
            }
            // a post's lit actions, a warning's words, a menu's Delete: on the page, a row under the pointer, a menu
            for (ground, under) in [("canvas", p.canvas), ("hover", p.hover), ("menu row", p.surface)] {
                for (name, words) in [("like", p.like), ("repost", p.repost), ("caution", p.caution), ("danger", p.danger)] {
                    at_least(format!("{} {name} on {ground}", p.name), words, under, &mut failures);
                }
            }
            // the main button, and X's Follow (the text's colour behind the page's)
            for fill in [p.fill, p.fill_hover] {
                at_least(format!("{} on-fill on {fill:?}", p.name), p.on_fill, fill, &mut failures);
            }
            at_least(format!("{} solid", p.name), p.canvas, p.text, &mut failures);
            at_least(format!("{} solid hovered", p.name), p.canvas, mix(p.text, p.canvas, 0.12), &mut failures);
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn the_hairlines_and_cards_show_on_their_page() {
        for which in Which::ALL {
            let p = which.palette();
            // a hairline and a card are seen, not read: a step from the page, which 1.15:1 is
            assert!(contrast(p.line, p.canvas) >= 1.15, "{} line", p.name);
            assert!(p.light || contrast(p.surface, p.canvas) >= 1.08, "{} surface", p.name);
            assert!(contrast(p.raised, p.canvas) >= 1.08, "{} raised", p.name);
        }
    }

    #[test]
    fn onyx_is_the_default_and_each_palette_is_named_for_itself() {
        assert_eq!(Which::default(), Which::Onyx);
        for which in Which::ALL {
            assert_eq!(which.palette().name, format!("{which:?}"));
        }
    }

    #[test]
    fn an_initials_colour_is_the_same_for_the_same_name() {
        for which in Which::ALL {
            let p = which.palette();
            assert_eq!(initial_color(p, "Ada Lovelace"), initial_color(p, "Ada Lovelace"));
            for name in ["Ada", "Bob", "Carol", "Dan", "Erin", "Lace Labs"] {
                let c = initial_color(p, name);
                assert!(contrast(c, mix(p.canvas, c, 0.16)) >= 3.0, "{} {name}: its initial reads on its disc", p.name);
            }
        }
    }
}
