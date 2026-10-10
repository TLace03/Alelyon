//! The loading symbol: a gold arc turning on a faint ring.
//!
//! One is shown wherever work is happening, so it feels like something is actually happening and the
//! application is not just being slow (2026-10-03). It turns only while the window has
//! work in flight: the window ticks it from a subscription it holds only then, so an idle window draws nothing.

use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke};
use iced::{Element, Radians, Rectangle, Renderer, Theme, mouse};

use crate::theme;

/// How far the arc turns per tick (the window ticks at about 30 per second): one turn in a little over a second.
pub const STEP: f32 = 0.19;

pub struct Spinner {
    phase: f32,
}

impl<Message> canvas::Program<Message> for Spinner {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = (bounds.width.min(bounds.height) / 2.0 - 2.0).max(1.0);
        let width = (radius / 3.5).clamp(1.5, 3.0);
        frame.stroke(&Path::circle(center, radius), Stroke::default().with_color(theme::with_alpha(theme::GOLD, 0.18)).with_width(width));
        let arc = Path::new(|b| {
            b.arc(canvas::path::Arc { center, radius, start_angle: Radians(self.phase), end_angle: Radians(self.phase + 1.9) });
        });
        frame.stroke(&arc, Stroke::default().with_color(theme::GOLD).with_width(width).with_line_cap(LineCap::Round));
        vec![frame.into_geometry()]
    }
}

/// A spinner `size` pixels across, at `phase` radians.
pub fn spinner<'a, Message: 'a>(phase: f32, size: f32) -> Element<'a, Message> {
    Canvas::new(Spinner { phase }).width(size).height(size).into()
}
