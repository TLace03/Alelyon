//! CENTCOM's black and gold: lattice-app's palette, fonts and widget styles (`lattice_app::theme`, re-exported whole),
//! with the window's own panels, buttons and type drawn here.
//!
//! On 2026-10-09 the application's style was brought in line with the login screen, which had drifted apart from
//! it in buttons and fonts. So the pages take the sign-in screen's gold: a panel and a floating card
//! are edged with a gold hairline; a primary button is a bar of polished gold; a plain button and a tab are dark with a
//! gold edge that brightens under the pointer; a page's title is set in the display face over a short gold rule, and a
//! section's name in small spaced capitals, as the sign-in screen's own labels are. The surfaces themselves stay the
//! palette's solid black and greys (the sign-in screen's carbon fibre was tried behind the pages the same day and
//! the solid colours were kept).
//!
//! Invariants:
//! - The palette is lattice-app's, untouched (its tests pin it to `lattice_desktop`'s tokens); only how CENTCOM draws
//!   its panels, buttons and type changes here, so the native Lattice window looks as it did.
//! - Every fill is solid or a wash laid on the palette's solid surfaces; the words each style carries keep a contrast
//!   of at least 4.5:1 over its fill in every state, washes taken over the canvas and over a panel, and the words on a
//!   gold button keep it over every stop of the gold. The tests below assert it, so a new style joins them.
//! - Every style is a function of its state only, as lattice-app's are: no style reads the iced `Theme`'s palette.

use std::sync::OnceLock;

use iced::widget::{button, container, pick_list, scrollable, text_editor, text_input};
use iced::{Background, Border, Color, Font, Gradient, Radians, Shadow, Theme, Vector, border, font, gradient, overlay};

pub use lattice_app::theme::*;

// ----------------------------------------------------------------- colours

/// The gold hairline round a panel: the sign-in screen's lamp catching its edge.
pub const EDGE: Color = Color { a: 0.18, ..GOLD };
/// A fainter one, between parts of the window (the rail's edge, a divider).
pub const EDGE_SOFT: Color = Color { a: 0.11, ..GOLD };
/// The faint warm rim round a well (a block of code, a long text): sunk into its panel rather than standing on it.
pub const RIM: Color = Color::from_rgba(1.0, 0.957, 0.839, 0.08);
/// The darker gold round a primary button, so its edge stays crisp on the black.
pub const GOLD_RIM: Color = Color::from_rgba(0.42, 0.33, 0.13, 0.9);
/// The palette's hover and raised greys, warmed towards the gold at the same lightness or less, so every colour of
/// words that holds on the palette's greys holds on these (a gold wash would not: the window blends in linear light,
/// where a tenth of gold over black is already as light as a mid grey's shadow).
pub const WARM_HOVER: Color = Color::from_rgb8(0x24, 0x21, 0x1b);
pub const WARM_RAISED: Color = Color::from_rgb8(0x1b, 0x1a, 0x16);

/// `a` mixed `t` (0..1) of the way to `b`, as one solid colour.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let m = |x: f32, y: f32| x + (y - x) * t;
    Color::from_rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

fn top_to_bottom(top: Color, foot: Color) -> Background {
    Background::Gradient(Gradient::Linear(gradient::Linear::new(Radians(std::f32::consts::PI)).add_stop(0.0, top).add_stop(1.0, foot)))
}

fn outlined(radius: f32, color: Color) -> Border {
    Border { color, width: 1.0, radius: radius.into() }
}

// ----------------------------------------------------------------- surfaces

/// A panel on the page: the palette's surface, edged with the gold hairline.
pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        text_color: Some(TEXT),
        border: outlined(12.0, EDGE),
        ..container::Style::default()
    }
}

/// A divider between parts of the window: a faint gold hairline.
pub fn line(_: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(EDGE_SOFT)), ..container::Style::default() }
}

/// A block of code or a long text: the canvas's black, sunk into its panel, with a faint rim.
pub fn well(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(CANVAS)),
        text_color: Some(TEXT),
        border: outlined(8.0, RIM),
        ..container::Style::default()
    }
}

/// What floats over the page (a menu, a tooltip, a dialog's card): the surface, with a stronger gold hairline and a
/// shadow.
pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        text_color: Some(TEXT),
        border: outlined(18.0, Color { a: 0.26, ..GOLD }),
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.6), offset: Vector::new(0.0, 14.0), blur_radius: 44.0 },
        ..container::Style::default()
    }
}

/// A short gold rule under a page's title: the lamp's light along an edge, fading out to the right.
pub fn rule(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            gradient::Linear::new(Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(0.0, Color { a: 0.85, ..GOLD })
                .add_stop(0.35, Color { a: 0.45, ..GOLD })
                .add_stop(1.0, Color { a: 0.0, ..GOLD }),
        ))),
        ..container::Style::default()
    }
}

// ----------------------------------------------------------------- buttons

/// The gold of a primary button, top to foot, in each state: a bar of polished gold, lit from above. Its words
/// (`ON_GOLD`) keep 4.5:1 over every stop.
pub fn gold_stops(status: button::Status) -> Option<(Color, Color)> {
    match status {
        button::Status::Active => Some((GOLD_STRONG, Color::from_rgb8(0xd4, 0xb0, 0x58))),
        button::Status::Hovered => Some((Color::from_rgb8(0xf6, 0xe1, 0xa8), GOLD)),
        button::Status::Pressed => Some((GOLD, Color::from_rgb8(0xbf, 0x9c, 0x4c))),
        button::Status::Disabled => None,
    }
}

pub fn primary_button(_: &Theme, status: button::Status) -> button::Style {
    match gold_stops(status) {
        Some((top, foot)) => button::Style {
            background: Some(top_to_bottom(top, foot)),
            text_color: ON_GOLD,
            border: outlined(8.0, GOLD_RIM),
            ..button::Style::default()
        },
        None => button::Style {
            background: Some(Background::Color(SURFACE)),
            text_color: TEXT_FAINT,
            border: outlined(8.0, LINE),
            ..button::Style::default()
        },
    }
}

/// The fill of a plain button and an unchosen tab: darker than the panel it sits on, so the gold edge frames it.
pub const BUTTON: Color = SIDEBAR;

/// A plain button: dark behind a gold hairline, which brightens, and washes the button gold, under the pointer.
pub fn secondary_button(_: &Theme, status: button::Status) -> button::Style {
    let (bg, text, edge) = match status {
        button::Status::Active => (BUTTON, TEXT, Color { a: 0.30, ..GOLD }),
        button::Status::Hovered => (mix(BUTTON, GOLD, 0.10), TEXT, Color { a: 0.70, ..GOLD }),
        button::Status::Pressed => (mix(BUTTON, GOLD, 0.16), TEXT, GOLD),
        button::Status::Disabled => (BUTTON, TEXT_FAINT, LINE),
    };
    button::Style { background: Some(Background::Color(bg)), text_color: text, border: outlined(8.0, edge), ..button::Style::default() }
}

/// A button that is only its words until the pointer is on it, when it warms.
pub fn ghost_button(_: &Theme, status: button::Status) -> button::Style {
    let (bg, text) = match status {
        button::Status::Active => (None, TEXT_DIM),
        button::Status::Hovered => (Some(WARM_HOVER), TEXT),
        button::Status::Pressed => (Some(LINE), TEXT),
        button::Status::Disabled => (None, TEXT_FAINT),
    };
    button::Style { background: bg.map(Background::Color), text_color: text, border: border::rounded(6.0), ..button::Style::default() }
}

/// One segment of a row of tabs: the chosen one washed gold behind a gold edge, the others dark with a faint one.
pub fn segment_button(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, text, edge) = if selected {
            (mix(BUTTON, GOLD, 0.12), GOLD, Color { a: 0.65, ..GOLD })
        } else if hovered {
            (mix(BUTTON, GOLD, 0.06), TEXT, Color { a: 0.40, ..GOLD })
        } else {
            (BUTTON, TEXT_DIM, Color { a: 0.16, ..GOLD })
        };
        button::Style { background: Some(Background::Color(bg)), text_color: text, border: outlined(8.0, edge), ..button::Style::default() }
    }
}

/// A row of a list: bare until the pointer is on it, when it warms; the chosen one raised behind a gold edge.
pub fn list_row(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, edge) = if selected {
            (Some(WARM_RAISED), Color { a: 0.45, ..GOLD })
        } else if hovered {
            (Some(WARM_HOVER), Color { a: 0.18, ..GOLD })
        } else {
            (None, Color::TRANSPARENT)
        };
        button::Style { background: bg.map(Background::Color), text_color: TEXT, border: outlined(8.0, edge), ..button::Style::default() }
    }
}

/// A section's button on the rail: the chosen one washed gold (the palette's own wash) behind a gold edge, its words
/// gold; the others warm under the pointer.
pub fn rail_button(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, text, edge) = if selected {
            (Some(GOLD_WASH), GOLD, Color { a: 0.42, ..GOLD })
        } else if hovered {
            (Some(WARM_HOVER), TEXT, Color::TRANSPARENT)
        } else {
            (None, TEXT_DIM, Color::TRANSPARENT)
        };
        button::Style { background: bg.map(Background::Color), text_color: text, border: outlined(10.0, edge), ..button::Style::default() }
    }
}

// ----------------------------------------------------------------- what is typed in, picked and scrolled

/// The edge of something typed in or picked: a faint gold until the pointer is on it, gold with the focus.
fn field_edge(focused: bool, hovered: bool) -> Color {
    if focused {
        GOLD
    } else if hovered {
        Color { a: 0.45, ..GOLD }
    } else {
        Color { a: 0.16, ..GOLD }
    }
}

/// A text box: the canvas's black, gold when it has the focus.
pub fn input(_: &Theme, status: text_input::Status) -> text_input::Style {
    use text_input::Status;
    let (focused, hovered) = match status {
        Status::Focused { is_hovered } => (true, is_hovered),
        Status::Hovered => (false, true),
        Status::Active | Status::Disabled => (false, false),
    };
    text_input::Style {
        background: Background::Color(CANVAS),
        border: outlined(8.0, field_edge(focused, hovered)),
        icon: TEXT_DIM,
        placeholder: TEXT_FAINT,
        value: TEXT,
        selection: with_alpha(GOLD, 0.30),
    }
}

pub fn editor(_: &Theme, status: text_editor::Status) -> text_editor::Style {
    let (focused, hovered) = match status {
        text_editor::Status::Focused { is_hovered } => (true, is_hovered),
        text_editor::Status::Hovered => (false, true),
        text_editor::Status::Active | text_editor::Status::Disabled => (false, false),
    };
    text_editor::Style {
        background: Background::Color(CANVAS),
        border: outlined(8.0, field_edge(focused, hovered)),
        placeholder: TEXT_FAINT,
        value: TEXT,
        selection: with_alpha(GOLD, 0.30),
    }
}

pub fn picker(_: &Theme, status: pick_list::Status) -> pick_list::Style {
    let hovered = matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened { .. });
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: TEXT_FAINT,
        handle_color: if hovered { GOLD } else { TEXT_DIM },
        background: Background::Color(CANVAS),
        border: outlined(8.0, field_edge(false, hovered)),
    }
}

pub fn picker_menu(_: &Theme) -> overlay::menu::Style {
    overlay::menu::Style {
        background: Background::Color(SURFACE),
        border: outlined(8.0, Color { a: 0.26, ..GOLD }),
        text_color: TEXT,
        selected_text_color: ON_GOLD,
        selected_background: Background::Color(GOLD),
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.55), offset: Vector::new(0.0, 10.0), blur_radius: 28.0 },
    }
}

pub fn scrollbars(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let thumb = match status {
        scrollable::Status::Active { .. } => Color { a: 0.22, ..GOLD },
        scrollable::Status::Hovered { is_vertical_scrollbar_hovered, is_horizontal_scrollbar_hovered, .. } => {
            if is_vertical_scrollbar_hovered || is_horizontal_scrollbar_hovered {
                GOLD_DIM
            } else {
                Color { a: 0.32, ..GOLD }
            }
        }
        scrollable::Status::Dragged { .. } => GOLD,
    };
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.background = None;
        rail.border = Border::default();
        rail.scroller.background = Background::Color(thumb);
        rail.scroller.border = border::rounded(3.0);
    }
    style.container = container::Style::default();
    style
}

// ----------------------------------------------------------------- type

/// The window's faces: lattice-app's (`ui`, `ui_strong`, `mono`), and the display face a page's title is set in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fonts {
    pub ui: Font,
    pub ui_strong: Font,
    pub mono: Font,
    pub display: Font,
}

/// Segoe UI Variable's display cut, drawn for large sizes, beside its text cut, which the window's words are set in.
const DISPLAY_FAMILIES: [&str; 1] = ["Segoe UI Variable Display"];

static DISPLAY: OnceLock<Font> = OnceLock::new();

/// The display face from what is installed, against the fonts iced's font system found (as lattice-app finds its
/// own): a semibold display cut, or none, when titles are set in the text face's semibold.
pub fn detect_display() -> Option<Font> {
    let installed = |name: &str| -> bool {
        let system = iced::advanced::graphics::text::font_system();
        let Ok(mut guard) = system.write() else { return false };
        guard.raw().db().faces().any(|face| face.families.iter().any(|(family, _)| family.eq_ignore_ascii_case(name)))
    };
    lattice_app::textmetrics::pick_family(&DISPLAY_FAMILIES, installed)
        .map(|name| Font { weight: font::Weight::Semibold, ..Font::with_name(name) })
}

/// Fix the display face for the process (the first call wins); without one, titles take the text face's semibold.
pub fn set_display(display: Option<Font>) {
    if let Some(font) = display {
        let _ = DISPLAY.set(font);
    }
}

/// The process's faces.
pub fn fonts() -> Fonts {
    let base = lattice_app::theme::fonts();
    Fonts { ui: base.ui, ui_strong: base.ui_strong, mono: base.mono, display: DISPLAY.get().copied().unwrap_or(base.ui_strong) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What shows where `fill` lies over `under`, blended in linear light as the window blends.
    fn over(fill: Color, under: Color) -> Color {
        let linear = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let encoded = |c: f32| if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        let blend = |f: f32, u: f32| encoded(linear(f) * fill.a + linear(u) * (1.0 - fill.a));
        Color::from_rgb(blend(fill.r, under.r), blend(fill.g, under.g), blend(fill.b, under.b))
    }

    /// The fills a style shows: its colour, or every stop of its gradient; none, a bare one.
    fn fills(background: Option<Background>) -> Vec<Color> {
        match background {
            Some(Background::Color(color)) => vec![color],
            Some(Background::Gradient(Gradient::Linear(l))) => l.stops.iter().flatten().map(|s| s.color).collect(),
            None => vec![Color::TRANSPARENT],
        }
    }

    /// Every colour the window writes words in.
    const WORDS: [(&str, Color); 9] = [
        ("text", TEXT),
        ("text-dim", TEXT_DIM),
        ("text-faint", TEXT_FAINT),
        ("gold", GOLD),
        ("gold-strong", GOLD_STRONG),
        ("caution", CAUTION),
        ("danger", DANGER),
        ("positive", POSITIVE),
        ("model", MODEL),
    ];

    /// The solid surfaces a button or a wash lies on: the canvas, a panel, a raised surface and the rail.
    const UNDER: [(&str, Color); 4] = [("canvas", CANVAS), ("surface", SURFACE), ("raised", RAISED), ("sidebar", SIDEBAR)];

    fn at_least(what: &str, words: Color, ground: Color, failures: &mut Vec<String>) {
        let ratio = contrast(words, ground);
        if ratio < 4.5 {
            failures.push(format!("{what}: {ratio:.2}"));
        }
    }

    #[test]
    fn every_colour_of_words_holds_45_to_1_on_every_surface() {
        let theme = Theme::Dark;
        let surfaces = [("panel", panel(&theme)), ("well", well(&theme)), ("card", card(&theme))];
        let mut failures = Vec::new();
        for (surface, style) in surfaces {
            for fill in fills(style.background) {
                for (under_name, under) in UNDER {
                    for (name, words) in WORDS {
                        at_least(&format!("{name} on {surface} over {under_name}"), words, over(fill, under), &mut failures);
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn every_button_keeps_its_words_legible_in_every_state_on_what_it_sits_on() {
        let theme = Theme::Dark;
        let statuses = [button::Status::Active, button::Status::Hovered, button::Status::Pressed, button::Status::Disabled];
        let mut failures = Vec::new();
        let mut check = |what: String, style: button::Style, under: &[(&str, Color)], also: &[Color]| {
            for fill in fills(style.background) {
                for (under_name, under) in under {
                    let ground = over(fill, *under);
                    at_least(&format!("{what}'s own words over {under_name}"), style.text_color, ground, &mut failures);
                    for words in also {
                        at_least(&format!("{what} with {words:?} over {under_name}"), *words, ground, &mut failures);
                    }
                }
            }
        };
        // the rail lies on the page's black (`view::rail`)
        let rail = [("canvas", CANVAS)];
        for s in statuses {
            let pressed = s == button::Status::Pressed;
            check(format!("primary {s:?}"), primary_button(&theme, s), &UNDER, &[]);
            check(format!("secondary {s:?}"), secondary_button(&theme, s), &UNDER, &[]);
            // a link's words are set dim, and Motion's faint, in a ghost button (a press is a blink: its own words only)
            let ghost_words: &[Color] = if pressed { &[] } else { &[TEXT_DIM, TEXT_FAINT] };
            check(format!("ghost {s:?}"), ghost_button(&theme, s), &UNDER, ghost_words);
            for selected in [false, true] {
                // a tab's words are its own colour; a list row holds dim and faint words and chips
                check(format!("segment {selected} {s:?}"), segment_button(selected)(&theme, s), &UNDER, &[]);
                check(format!("row {selected} {s:?}"), list_row(selected)(&theme, s), &UNDER, &[TEXT_DIM, TEXT_FAINT, GOLD, POSITIVE]);
                // a rail button's small words are faint, and gold once it is chosen (`view::rail`)
                let rail_words: &[Color] = if selected { &[GOLD] } else { &[TEXT_DIM, TEXT_FAINT] };
                check(format!("rail {selected} {s:?}"), rail_button(selected)(&theme, s), &rail, rail_words);
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn the_warm_greys_are_no_lighter_than_the_palettes_own() {
        assert!(luminance(WARM_HOVER) <= luminance(HOVER), "{} > {}", luminance(WARM_HOVER), luminance(HOVER));
        assert!(luminance(WARM_RAISED) <= luminance(RAISED), "{} > {}", luminance(WARM_RAISED), luminance(RAISED));
        assert!(WARM_HOVER.r > WARM_HOVER.b && WARM_RAISED.r > WARM_RAISED.b, "warmed towards the gold");
    }

    #[test]
    fn the_gold_buttons_words_hold_45_to_1_over_every_stop_of_the_gold() {
        for status in [button::Status::Active, button::Status::Hovered, button::Status::Pressed] {
            let (top, foot) = gold_stops(status).expect("enabled: gold");
            for stop in [top, foot] {
                let ratio = contrast(ON_GOLD, stop);
                assert!(ratio >= 4.5, "{status:?} {stop:?}: {ratio:.2}");
            }
        }
        assert!(gold_stops(button::Status::Disabled).is_none(), "disabled: no gold");
    }

    #[test]
    fn fields_keep_their_words_legible() {
        let theme = Theme::Dark;
        for status in [text_input::Status::Active, text_input::Status::Hovered, text_input::Status::Focused { is_hovered: false }] {
            let Background::Color(field) = input(&theme, status).background else { panic!("a flat field") };
            for (name, words) in [("value", TEXT), ("placeholder", TEXT_FAINT)] {
                assert!(contrast(words, field) >= 4.5, "{name} in {status:?}");
            }
        }
        let Background::Color(menu) = picker_menu(&theme).background else { panic!("a flat menu") };
        assert!(contrast(TEXT, menu) >= 4.5 && contrast(ON_GOLD, GOLD) >= 4.5);
    }

    #[test]
    fn mixing_runs_from_one_colour_to_the_other() {
        assert_eq!(mix(CANVAS, GOLD, 0.0), CANVAS);
        assert_eq!(mix(CANVAS, GOLD, 1.0), Color { a: 1.0, ..GOLD });
        let half = mix(Color::BLACK, Color::WHITE, 0.5);
        assert!((half.r - 0.5).abs() < 1e-6 && half.a == 1.0);
        assert_eq!(mix(CANVAS, GOLD, 7.0), mix(CANVAS, GOLD, 1.0), "held to the way between them");
    }

    #[test]
    fn the_display_face_falls_back_to_the_text_faces_semibold() {
        // nothing set in this test process: titles take the text face's semibold
        let f = fonts();
        assert_eq!(f.ui, lattice_app::theme::fonts().ui);
        assert_eq!(f.display.weight, font::Weight::Semibold);
        // detection names the display cut, or nothing
        if let Some(found) = detect_display() {
            assert_eq!(found.family, font::Family::Name(DISPLAY_FAMILIES[0]));
            assert_eq!(found.weight, font::Weight::Semibold);
        }
    }
}
