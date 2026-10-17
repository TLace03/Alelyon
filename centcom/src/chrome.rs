//! The window's own title bar and edges. The window has no Windows frame (`decorations: false`); this module draws
//! what the frame did: the minimise, maximise and close buttons, an area that drags the window (double-click
//! maximises or restores; Windows' snapping works, since a drag is Windows' own move), and thin edges that resize
//! it. On the sign-in screen these are a bar, transparent over the backdrop. In the main window they take no row of
//! their own: the buttons sit small in the window's top right corner, over the page ([`corner`]), and a strip along
//! the window's top lies under the page ([`under`]), so a press there that the page does not take (beside Lattice's
//! tabs, the margin above a page's heading) drags the window; the rail's mark and empty space drag it too ([`drag`]).

use std::time::{Duration, Instant};

use iced::widget::{Row, Space, button, column, container, mouse_area, row, stack, text};
use iced::window::Direction;
use iced::{Alignment, Background, Border, Color, Element, Length, mouse};
use crate::theme::{self, fonts};

/// The bar's height.
pub const BAR_H: f32 = 32.0;
/// The resize edges' thickness, and their corners' size.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;
/// Two presses on the bar this close together are a double-click.
const DOUBLE: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug)]
pub enum Act {
    /// A press on the bar: a drag, or (the second of two quick ones) maximise/restore.
    Press,
    Minimise,
    ToggleMaximise,
    Close,
    Resize(Direction),
    /// The page on show to a window of its own; in a page's own window, Back to the main window (`windows`).
    PopOut,
    Back,
}

/// What a press on the bar means, given when the last one was.
pub fn press(last: Option<Instant>, now: Instant) -> Act {
    match last {
        Some(t) if now.duration_since(t) <= DOUBLE => Act::ToggleMaximise,
        _ => Act::Press,
    }
}

fn glyph<'a, M: 'a>(s: &'a str) -> Element<'a, M> {
    container(text(s).size(13).font(fonts().ui).color(theme::TEXT_DIM)).center_x(46).center_y(BAR_H).into()
}

fn control<'a, M: Clone + 'a>(s: &'a str, msg: M, danger: bool) -> Element<'a, M> {
    button(glyph(s))
        .padding(0)
        .on_press(msg)
        .style(move |_, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed if danger => Some(Background::Color(Color::from_rgb8(0xc4, 0x2b, 0x1c))),
                button::Status::Hovered | button::Status::Pressed => Some(Background::Color(theme::with_alpha(theme::TEXT, 0.10))),
                _ => None,
            },
            text_color: theme::TEXT,
            border: Border::default(),
            ..button::Style::default()
        })
        .into()
}

/// The bar: a drag area across the window, then minimise, maximise (restore when maximised) and close. `solid` gives
/// it the canvas's black; otherwise it is transparent over whatever is drawn behind it.
pub fn bar<'a, M: Clone + 'a>(solid: bool, maximised: bool, act: impl Fn(Act) -> M + 'a) -> Element<'a, M> {
    let drag = mouse_area(Space::new().width(Length::Fill).height(BAR_H)).on_press(act(Act::Press));
    let strip = row![
        drag,
        control("─", act(Act::Minimise), false),
        control(if maximised { "❐" } else { "☐" }, act(Act::ToggleMaximise), false),
        control("✕", act(Act::Close), true),
    ]
    .align_y(Alignment::Center)
    .height(BAR_H);
    container(strip)
        .width(Length::Fill)
        .style(move |_| container::Style { background: solid.then_some(Background::Color(theme::CANVAS)), ..container::Style::default() })
        .into()
}

/// The three buttons, small: minimise, maximise (restore when maximised) and close; after `lead` (the pop-out button,
/// or a page's window's Back), set a little apart.
pub fn compact<'a, M: Clone + 'a>(lead: Option<Act>, maximised: bool, act: impl Fn(Act) -> M + 'a) -> Element<'a, M> {
    let small = |s: &'a str, msg: M, danger: bool| -> Element<'a, M> {
        button(container(text(s).size(11).font(fonts().ui).color(theme::TEXT_DIM)).center_x(24).center_y(22))
            .padding(0)
            .on_press(msg)
            .style(move |_, status| button::Style {
                background: match status {
                    button::Status::Hovered | button::Status::Pressed if danger => Some(Background::Color(Color::from_rgb8(0xc4, 0x2b, 0x1c))),
                    button::Status::Hovered | button::Status::Pressed => Some(Background::Color(theme::with_alpha(theme::TEXT, 0.10))),
                    _ => None,
                },
                text_color: theme::TEXT,
                border: Border { radius: 4.0.into(), ..Border::default() },
                ..button::Style::default()
            })
            .into()
    };
    let mut buttons = Row::new().spacing(2).align_y(Alignment::Center);
    if let Some(lead) = lead {
        let (glyph, words) = match lead {
            // a plain arrow: Windows draws U+21A9 as a coloured emoji
            Act::Back => ("\u{2190}", "Back to the main window"),
            _ => ("\u{29C9}", "Open this page in a window of its own\n(or drag its place off the rail)"),
        };
        buttons = buttons.push(crate::ui::tip(small(glyph, act(lead), false), words)).push(Space::new().width(8));
    }
    buttons
        .push(small("\u{2500}", act(Act::Minimise), false))
        .push(small(if maximised { "\u{2750}" } else { "\u{2610}" }, act(Act::ToggleMaximise), false))
        .push(small("\u{2715}", act(Act::Close), true))
        .into()
}

/// The [`compact`] buttons in the window's top right corner, for a layer over the page: the rest of the layer is
/// empty, so it takes no press but the buttons'.
pub fn corner<'a, M: Clone + 'a>(lead: Option<Act>, maximised: bool, act: impl Fn(Act) -> M + 'a) -> Element<'a, M> {
    container(compact(lead, maximised, act)).align_right(Length::Fill).padding(iced::padding::top(5).right(14)).into()
}

/// A strip as tall as the bar along the window's top, that drags it, for a layer under the page: a press the page
/// takes (a button, a tab, a field) stays the page's, and one on its empty space falls through to the strip. The
/// layer fills the window, since a stack is as large as its first layer.
pub fn under<'a, M: Clone + 'a>(act: impl Fn(Act) -> M + 'a) -> Element<'a, M> {
    column![drag(Space::new().width(Length::Fill).height(BAR_H), act)].height(Length::Fill).into()
}

/// `inside`, as an area that drags the window (a press drags, a quick second one maximises or restores).
pub fn drag<'a, M: Clone + 'a>(inside: impl Into<Element<'a, M>>, act: impl Fn(Act) -> M + 'a) -> Element<'a, M> {
    mouse_area(inside).on_press(act(Act::Press)).into()
}

/// Thin invisible edges and corners that resize the window, over everything else. None while maximised.
pub fn edges<'a, M: Clone + 'a>(act: impl Fn(Act) -> M + 'a) -> Element<'a, M> {
    let zone = |w: Length, h: Length, d: Direction, cursor: mouse::Interaction| -> Element<'a, M> {
        mouse_area(Space::new().width(w).height(h)).on_press(act(Act::Resize(d))).interaction(cursor).into()
    };
    use mouse::Interaction::{
        ResizingDiagonallyDown as Down, ResizingDiagonallyUp as Up, ResizingHorizontally as H, ResizingVertically as V,
    };
    let top = row![
        zone(Length::Fixed(CORNER), Length::Fixed(EDGE), Direction::NorthWest, Down),
        zone(Length::Fill, Length::Fixed(EDGE), Direction::North, V),
        zone(Length::Fixed(CORNER), Length::Fixed(EDGE), Direction::NorthEast, Up),
    ];
    let bottom = row![
        zone(Length::Fixed(CORNER), Length::Fixed(EDGE), Direction::SouthWest, Up),
        zone(Length::Fill, Length::Fixed(EDGE), Direction::South, V),
        zone(Length::Fixed(CORNER), Length::Fixed(EDGE), Direction::SouthEast, Down),
    ];
    let middle = row![
        zone(Length::Fixed(EDGE), Length::Fill, Direction::West, H),
        Space::new().width(Length::Fill),
        zone(Length::Fixed(EDGE), Length::Fill, Direction::East, H),
    ]
    .height(Length::Fill);
    stack![column![top, middle, bottom].width(Length::Fill).height(Length::Fill)].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_quick_press_maximises_and_a_slow_one_drags() {
        let t = Instant::now();
        assert!(matches!(press(None, t), Act::Press));
        assert!(matches!(press(Some(t), t + Duration::from_millis(250)), Act::ToggleMaximise));
        assert!(matches!(press(Some(t), t + Duration::from_millis(900)), Act::Press));
    }
}
