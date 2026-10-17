//! Pages' line icons: a post's actions (reply, repost, like, save, share, more) and what the page's navigation and
//! composer name. Drawn here for Pages in a 24-unit box from SVG path data, stroked at 1.75 units with round ends
//! as the social sites draw theirs, on a canvas kept between frames (`ui::Kept`). The heart, the bookmark, a star
//! and a vote's arrow fill when what they mark is on.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineCap, LineJoin, Stroke};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

use crate::lattice::ide::file_icons::path;

/// What an icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Reply,
    Repost,
    Heart,
    Bookmark,
    Share,
    More,
    Link,
    Chevron,
    Home,
    Explore,
    Bell,
    Communities,
    Lists,
    Jobs,
    Drafts,
    Person,
    Building,
    Shield,
    Photo,
    Poll,
    Calendar,
    Pen,
    Pin,
    Trash,
    Mute,
    Block,
    Flag,
    Back,
    Globe,
    Refresh,
    Close,
    Left,
    Right,
    /// A launch, and the Products tab.
    Rocket,
    /// A review's stars.
    Star,
    /// A vote for a piece of feedback.
    Up,
}

/// The parts of a glyph: lines stroked; outlines stroked, or filled when the glyph is on; dots always filled; rings
/// stroked. Circles are (x, y, radius).
struct Parts {
    lines: &'static [&'static str],
    outline: Option<&'static str>,
    dots: &'static [(f32, f32, f32)],
    rings: &'static [(f32, f32, f32)],
}

const fn lines(lines: &'static [&'static str]) -> Parts {
    Parts { lines, outline: None, dots: &[], rings: &[] }
}

fn parts(glyph: Glyph) -> Parts {
    match glyph {
        Glyph::Reply => Parts {
            outline: Some("M5 4.5h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-8l-5 4v-4H5a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2z"),
            ..lines(&[])
        },
        Glyph::Repost => lines(&["M4 11V9a2 2 0 0 1 2-2h13", "M16 4l3 3-3 3", "M20 13v2a2 2 0 0 1-2 2H5", "M8 20l-3-3 3-3"]),
        Glyph::Heart => Parts {
            outline: Some(
                "M12 20.5C6 16.5 3 13.3 3 9.6 3 6.9 5.1 5 7.6 5c1.8 0 3.3 1 4.4 2.6C13.1 6 14.6 5 16.4 5 18.9 5 21 6.9 21 9.6c0 3.7-3 6.9-9 10.9z",
            ),
            ..lines(&[])
        },
        Glyph::Bookmark => Parts { outline: Some("M6 3.5h12v17l-6-4.2-6 4.2z"), ..lines(&[]) },
        Glyph::Share => lines(&["M12 15V3.5", "M7.5 8 12 3.5 16.5 8", "M5 12v7.5h14V12"]),
        Glyph::More => Parts { dots: &[(5.0, 12.0, 1.7), (12.0, 12.0, 1.7), (19.0, 12.0, 1.7)], ..lines(&[]) },
        Glyph::Link => lines(&["M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1", "M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"]),
        Glyph::Chevron => lines(&["M7 10l5 5 5-5"]),
        Glyph::Home => lines(&["M3.5 10.5 12 4l8.5 6.5", "M5.5 9v11h5v-6h3v6h5V9"]),
        Glyph::Explore => Parts { rings: &[(10.5, 10.5, 6.5)], ..lines(&["M15.5 15.5 20.5 20.5"]) },
        Glyph::Bell => lines(&["M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 1.5h-15z", "M10 20.5h4"]),
        Glyph::Communities => Parts {
            rings: &[(9.0, 8.5, 3.2), (16.5, 9.0, 2.6)],
            ..lines(&["M3 20c0-3.3 2.7-5.5 6-5.5s6 2.2 6 5.5", "M16 14.5c2.8 0 5 1.9 5 4.8"])
        },
        Glyph::Lists => Parts {
            dots: &[(4.5, 6.5, 1.3), (4.5, 12.0, 1.3), (4.5, 17.5, 1.3)],
            ..lines(&["M9 6.5h11", "M9 12h11", "M9 17.5h11"])
        },
        Glyph::Jobs => lines(&["M3.5 8h17v11.5h-17z", "M9 8V5.5h6V8", "M3.5 13h17"]),
        Glyph::Drafts => lines(&["M5 3.5h9l5 5v12H5z", "M14 3.5v5h5", "M8.5 13h7", "M8.5 16.5h5"]),
        Glyph::Person => Parts { rings: &[(12.0, 8.0, 4.0)], ..lines(&["M4.5 20.5c0-4 3.4-6.5 7.5-6.5s7.5 2.5 7.5 6.5"]) },
        Glyph::Building => lines(&["M4 20.5V4h10v16.5", "M14 9h6v11.5", "M2.5 20.5h19", "M7 7.5h4", "M7 11h4", "M7 14.5h4"]),
        Glyph::Shield => Parts { outline: Some("M12 3.5 19.5 6v6c0 4.8-3.3 7.8-7.5 9-4.2-1.2-7.5-4.2-7.5-9V6z"), ..lines(&[]) },
        Glyph::Photo => Parts { dots: &[(15.5, 9.0, 1.5)], ..lines(&["M4 5h16v14H4z", "M4 16l5-5 4 4 2.5-2.5L20 17"]) },
        Glyph::Poll => lines(&["M5 7h9", "M5 12h14", "M5 17h6"]),
        Glyph::Calendar => lines(&["M4 6h16v14H4z", "M4 10h16", "M8 4v4", "M16 4v4"]),
        Glyph::Pen => lines(&["M4 20l4.5-1 10-10a2.1 2.1 0 0 0-3-3l-10 10z", "M13.5 7.5l3 3"]),
        Glyph::Pin => lines(&["M9 3.5h6l-1 6.5 3 3H7l3-3z", "M12 13v7.5"]),
        Glyph::Trash => lines(&["M4.5 7h15", "M9.5 7V4.5h5V7", "M6.5 7l1 13h9l1-13", "M10 11v5.5", "M14 11v5.5"]),
        Glyph::Mute => lines(&["M4 9.5h3.5L12 5.5v13l-4.5-4H4z", "M16 9.5l5 5", "M21 9.5l-5 5"]),
        Glyph::Block => Parts { rings: &[(12.0, 12.0, 8.0)], ..lines(&["M6.5 6.5 17.5 17.5"]) },
        Glyph::Flag => lines(&["M5.5 21V4", "M5.5 4.5h11l-2 4 2 4h-11"]),
        Glyph::Back => lines(&["M20 12H4.5", "M10.5 6l-6 6 6 6"]),
        Glyph::Refresh => lines(&["M19.5 12a7.5 7.5 0 1 1-2.2-5.3", "M19.5 4.5V9H15"]),
        Glyph::Close => lines(&["M6 6l12 12", "M18 6 6 18"]),
        Glyph::Left => lines(&["M14.5 5.5 8 12l6.5 6.5"]),
        Glyph::Right => lines(&["M9.5 5.5 16 12l-6.5 6.5"]),
        Glyph::Rocket => Parts {
            outline: Some("M12 3c2.8 2.2 4.2 5.4 4.2 9.3L14.6 16H9.4l-1.6-3.7C7.8 8.4 9.2 5.2 12 3z"),
            rings: &[(12.0, 9.0, 1.6)],
            ..lines(&["M7.8 12.5 5 15.5v3l3.7-1.6", "M16.2 12.5l2.8 3v3l-3.7-1.6", "M10.5 19.5 12 21.5l1.5-2"])
        },
        Glyph::Star => Parts { outline: Some("M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.9L12 16.9l-5.2 2.8 1-5.9-4.3-4.1 5.9-.8z"), ..lines(&[]) },
        Glyph::Up => Parts { outline: Some("M12 4.5l7 8.5h-4v6.5H9V13H5z"), ..lines(&[]) },
        Glyph::Globe => Parts {
            rings: &[(12.0, 12.0, 8.5)],
            ..lines(&["M3.5 12h17", "M12 3.5c2.4 2.5 3.6 5.3 3.6 8.5s-1.2 6-3.6 8.5", "M12 3.5c-2.4 2.5-3.6 5.3-3.6 8.5s1.2 6 3.6 8.5"])
        },
    }
}

/// `glyph`, `size` pixels square, in `color`; `on` fills the heart and the bookmark.
pub fn icon<'a, M: 'a>(glyph: Glyph, size: f32, color: Color, on: bool) -> Element<'a, M> {
    Canvas::new(Drawn { glyph, color, on }).width(Length::Fixed(size)).height(Length::Fixed(size)).into()
}

struct Drawn {
    glyph: Glyph,
    color: Color,
    on: bool,
}

impl<M> canvas::Program<M> for Drawn {
    type State = crate::ui::Kept;

    fn draw(&self, kept: &crate::ui::Kept, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let key = crate::ui::key_of(&(self.glyph, self.color.into_rgba8(), self.on));
        vec![kept.draw(renderer, bounds.size(), key, |frame| paint(frame, self.glyph, self.color, self.on))]
    }
}

fn paint(frame: &mut Frame, glyph: Glyph, color: Color, on: bool) {
    let scale = frame.width().min(frame.height()) / 24.0;
    let (dx, dy) = ((frame.width() - 24.0 * scale) / 2.0, (frame.height() - 24.0 * scale) / 2.0);
    let at = |p: [f32; 2]| Point::new(p[0] * scale + dx, p[1] * scale + dy);
    let stroke = Stroke::default().with_color(color).with_width(1.75 * scale).with_line_cap(LineCap::Round).with_line_join(LineJoin::Round);
    let traced = |d: &str| path::parse(d).ok().map(|segments| canvas::Path::new(|b| path::trace(b, &segments, &at)));
    let parts = parts(glyph);
    for d in parts.lines {
        if let Some(shape) = traced(d) {
            frame.stroke(&shape, stroke);
        }
    }
    if let Some(shape) = parts.outline.and_then(traced) {
        if on {
            frame.fill(&shape, color);
        }
        frame.stroke(&shape, stroke);
    }
    for &(x, y, r) in parts.dots {
        frame.fill(&canvas::Path::circle(at([x, y]), r * scale), color);
    }
    for &(x, y, r) in parts.rings {
        frame.stroke(&canvas::Path::circle(at([x, y]), r * scale), stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Glyph; 36] = [
        Glyph::Reply,
        Glyph::Repost,
        Glyph::Heart,
        Glyph::Bookmark,
        Glyph::Share,
        Glyph::More,
        Glyph::Link,
        Glyph::Chevron,
        Glyph::Home,
        Glyph::Explore,
        Glyph::Bell,
        Glyph::Communities,
        Glyph::Lists,
        Glyph::Jobs,
        Glyph::Drafts,
        Glyph::Person,
        Glyph::Building,
        Glyph::Shield,
        Glyph::Photo,
        Glyph::Poll,
        Glyph::Calendar,
        Glyph::Pen,
        Glyph::Pin,
        Glyph::Trash,
        Glyph::Mute,
        Glyph::Block,
        Glyph::Flag,
        Glyph::Back,
        Glyph::Globe,
        Glyph::Refresh,
        Glyph::Close,
        Glyph::Left,
        Glyph::Right,
        Glyph::Rocket,
        Glyph::Star,
        Glyph::Up,
    ];

    #[test]
    fn every_glyph_parses_draws_something_and_stays_in_its_box() {
        for glyph in ALL {
            let p = parts(glyph);
            let shapes: Vec<&str> = p.lines.iter().copied().chain(p.outline).collect();
            assert!(!shapes.is_empty() || !p.dots.is_empty() || !p.rings.is_empty(), "{glyph:?} draws nothing");
            for d in shapes {
                let segments = path::parse(d).unwrap_or_else(|e| panic!("{glyph:?}: {d}: {e}"));
                for seg in segments {
                    let points: Vec<[f32; 2]> = match seg {
                        path::Seg::Move(p) | path::Seg::Line(p) => vec![p],
                        path::Seg::Cubic(a, b, c) => vec![a, b, c],
                        path::Seg::Quad(a, b) => vec![a, b],
                        path::Seg::Close => vec![],
                    };
                    for [x, y] in points {
                        assert!((0.0..=24.0).contains(&x) && (0.0..=24.0).contains(&y), "{glyph:?} leaves its box at ({x}, {y})");
                    }
                }
            }
            for &(x, y, r) in p.dots.iter().chain(p.rings) {
                assert!(x - r >= 0.0 && x + r <= 24.0 && y - r >= 0.0 && y + r <= 24.0, "{glyph:?}'s circle leaves its box");
            }
        }
    }
}
