//! The pairing QR code, drawn: the approve page's address as a QR code (the `qrcode` crate makes the modules; this draws
//! them), dark on light with its quiet zone, so a phone's camera reads it off the screen.

use iced::widget::canvas::{self, Frame};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};

/// The modules of `text`'s QR code, row by row (true is dark), and its width; None when it does not fit a QR code.
pub fn modules(text: &str) -> Option<(Vec<bool>, usize)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width = code.width();
    let dark = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((dark, width))
}

pub struct Drawing {
    pub dark: Vec<bool>,
    pub width: usize,
}

impl<Message> canvas::Program<Message> for Drawing {
    /// The code, kept between frames: the sign-in screen's backdrop draws the window many times a second, and the code
    /// changes only with a new pairing.
    type State = crate::ui::Kept;

    fn draw(&self, kept: &crate::ui::Kept, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        vec![kept.draw(renderer, bounds.size(), crate::ui::key_of(&(&self.dark, self.width)), |frame| self.modules_on(frame, bounds))]
    }
}

impl Drawing {
    fn modules_on(&self, frame: &mut Frame, bounds: Rectangle) {
        // four modules of quiet zone round the code, as the standard asks
        let side = bounds.width.min(bounds.height);
        let cells = self.width as f32 + 8.0;
        let m = (side / cells).floor().max(1.0);
        let total = m * cells;
        let origin = Point::new((bounds.width - total) / 2.0, (bounds.height - total) / 2.0);
        frame.fill_rectangle(origin, Size::new(total, total), Color::WHITE);
        for (i, dark) in self.dark.iter().enumerate() {
            if *dark {
                let (x, y) = ((i % self.width) as f32 + 4.0, (i / self.width) as f32 + 4.0);
                frame.fill_rectangle(Point::new(origin.x + x * m, origin.y + y * m), Size::new(m, m), Color::BLACK);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_approve_address_makes_a_square_code() {
        let (dark, width) = modules("https://id.alelyon.com/identity/pair/ghjkmnpqrstuvwxy").unwrap();
        assert_eq!(dark.len(), width * width);
        assert!(width >= 21 && (width - 21) % 4 == 0, "a QR version's size");
        // the three finder patterns' corners are dark
        assert!(dark[0] && dark[width - 1] && dark[(width - 1) * width]);
    }
}
