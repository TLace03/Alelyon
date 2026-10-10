//! Sinai's memory map, drawn: each retained record a small cube at its display cell, as the Angel window draws it
//! (`brain_ui.rs`): gold when it was included in the last prepared request, blue when it is only retained. Under the
//! pointer, a cube says what it is (its title, when it was made, what it saw, and whether the last request used it) and
//! its links are drawn. The grid is for showing (8 by 8 by 3 cells); it is not neurons, nor the model's KV.

use iced::widget::canvas::{self, Frame, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme;

use crate::machine::Map;

const GOLD: Color = Color::from_rgb(225.0 / 255.0, 178.0 / 255.0, 67.0 / 255.0);
const BLUE: Color = Color::from_rgb(62.0 / 255.0, 112.0 / 255.0, 143.0 / 255.0);

/// A cell's place on the drawing at `scale`, before centring: the Angel window's isometric projection.
pub fn project(at: [u8; 3], scale: f32) -> Point {
    let [x, y, z] = at.map(f32::from);
    Point::new((x - y) * scale, (x + y) * scale * 0.48 - z * scale * 3.0)
}

/// The scale and the offset that fit every cell into `size` (the scale at most 20, as in the Angel window).
pub fn fit(map: &Map, size: Size) -> (f32, Point) {
    let points: Vec<Point> = map.cells.iter().map(|c| project(c.at, 1.0)).collect();
    let (mut lo, mut hi) = (Point::new(f32::MAX, f32::MAX), Point::new(f32::MIN, f32::MIN));
    for p in &points {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    if points.is_empty() {
        return (1.0, Point::ORIGIN);
    }
    let (w, h) = ((hi.x - lo.x) + 2.0, (hi.y - lo.y) + 2.2);
    let scale = (size.width / w).min(size.height / h).clamp(1.0, 20.0);
    let offset =
        Point::new(size.width / 2.0 - (lo.x + hi.x) / 2.0 * scale, scale * (0.6 - lo.y) + (size.height - h * scale).max(0.0) / 2.0);
    (scale, offset)
}

/// The cell under `cursor`, if any: the nearest within a cube's reach.
pub fn under(map: &Map, size: Size, cursor: Point) -> Option<usize> {
    let (scale, offset) = fit(map, size);
    map.cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let p = project(c.at, scale);
            (i, (p.x + offset.x - cursor.x).hypot(p.y + offset.y - cursor.y))
        })
        .filter(|(_, d)| *d <= scale * 0.9)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The height the drawing wants at `width`, so a wide dock does not draw a tall empty box.
pub fn height_for(map: &Map, width: f32) -> f32 {
    let (scale, _) = fit(map, Size::new(width, f32::MAX / 4.0));
    let ys: Vec<f32> = map.cells.iter().map(|c| project(c.at, 1.0).y).collect();
    let span = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
    ((span.max(0.0) + 2.2) * scale).clamp(80.0, 360.0)
}

pub struct Drawing<'a> {
    pub map: &'a Map,
}

fn cube(frame: &mut Frame, p: Point, size: f32, color: Color) {
    let top = Point::new(p.x, p.y - size * 0.5);
    let left = Point::new(p.x - size, p.y);
    let right = Point::new(p.x + size, p.y);
    let middle = Point::new(p.x, p.y + size * 0.5);
    let down = |q: Point| Point::new(q.x, q.y + size);
    let shade = |c: Color, k: f32| Color::from_rgb(c.r * k, c.g * k, c.b * k);
    for (points, fill) in [
        ([top, right, middle, left], color),
        ([left, middle, down(middle), down(left)], shade(color, 0.55)),
        ([middle, right, down(right), down(middle)], shade(color, 0.75)),
    ] {
        let face = Path::new(|b| {
            b.move_to(points[0]);
            for q in &points[1..] {
                b.line_to(*q);
            }
            b.close();
        });
        frame.fill(&face, fill);
    }
}

impl<'a, Message> canvas::Program<Message> for Drawing<'a> {
    /// The cubes, kept between frames: the Sinai page's face redraws the window sixty times a second, and the cubes
    /// change only with a new snapshot (or a new size). The pointer's card and links are drawn fresh over them.
    type State = crate::ui::Kept;

    fn draw(&self, kept: &crate::ui::Kept, renderer: &Renderer, _: &Theme, bounds: Rectangle, cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let (scale, offset) = fit(self.map, bounds.size());
        let at = |c: [u8; 3]| {
            let p = project(c, scale);
            Point::new(p.x + offset.x, p.y + offset.y)
        };
        let key = crate::ui::key_of(&self.map.cells.iter().map(|c| (c.at, c.included)).collect::<Vec<_>>());
        let cubes = kept.draw(renderer, bounds.size(), key, |frame| {
            // back to front, as the Angel window sorts them, so nearer cubes cover farther ones
            let mut order: Vec<usize> = (0..self.map.cells.len()).collect();
            order.sort_by_key(|&i| {
                let [x, y, z] = self.map.cells[i].at;
                (z, x + y, x)
            });
            for i in order {
                let c = &self.map.cells[i];
                cube(frame, at(c.at), scale * 0.75, if c.included { GOLD } else { BLUE });
            }
        });
        let mut frame = Frame::new(renderer, bounds.size());
        let hovered = cursor.position_in(bounds).and_then(|p| under(self.map, bounds.size(), p));
        if let Some(h) = hovered {
            let from = at(self.map.cells[h].at);
            for &(a, b) in &self.map.links {
                if a == h || b == h {
                    let other = at(self.map.cells[if a == h { b } else { a }].at);
                    frame.stroke(&Path::line(from, other), Stroke::default().with_color(GOLD).with_width(1.5));
                }
            }
            let c = &self.map.cells[h];
            let lines = [
                c.title.clone(),
                c.created.clone(),
                crate::ui::cut(&c.description, 90).to_string(),
                if c.included {
                    "Added to the last model request; whether the answer was right is not established."
                } else {
                    "Retained; not in the last request."
                }
                .to_string(),
            ];
            let mut y = 4.0;
            for (k, line) in lines.iter().filter(|l| !l.is_empty()).enumerate() {
                frame.fill_text(Text {
                    content: line.clone(),
                    position: Point::new(6.0, y),
                    color: if k == 0 { theme::TEXT } else { theme::TEXT_DIM },
                    size: (if k == 0 { 12.5 } else { 11.5 }).into(),
                    ..Text::default()
                });
                y += 15.0;
            }
        }
        vec![cubes, frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &crate::ui::Kept, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match cursor.position_in(bounds).and_then(|p| under(self.map, bounds.size(), p)) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Cell;

    fn map(cells: &[[u8; 3]]) -> Map {
        Map {
            cells: cells
                .iter()
                .enumerate()
                .map(|(i, &at)| Cell {
                    id: format!("c{i}"),
                    title: String::new(),
                    created: String::new(),
                    at,
                    included: false,
                    description: String::new(),
                })
                .collect(),
            ..Map::default()
        }
    }

    #[test]
    fn the_projection_is_the_angel_windows() {
        let near = |p: Point, x: f32, y: f32| (p.x - x).abs() < 1e-4 && (p.y - y).abs() < 1e-4;
        assert!(near(project([0, 0, 0], 10.0), 0.0, 0.0));
        assert!(near(project([1, 0, 0], 10.0), 10.0, 4.8));
        assert!(near(project([0, 1, 0], 10.0), -10.0, 4.8));
        assert!(near(project([0, 0, 1], 10.0), 0.0, -30.0), "a layer up stands three cells higher");
    }

    #[test]
    fn every_cell_fits_and_the_pointer_finds_the_one_under_it() {
        let m = map(&[[0, 0, 0], [7, 7, 2], [7, 0, 0], [0, 7, 1]]);
        let size = Size::new(300.0, 220.0);
        let (scale, offset) = fit(&m, size);
        assert!(scale > 1.0 && scale <= 20.0);
        for c in &m.cells {
            let p = project(c.at, scale);
            let (x, y) = (p.x + offset.x, p.y + offset.y);
            assert!((0.0..=size.width).contains(&x) && (-scale..=size.height + scale).contains(&y), "{:?} drawn at {x},{y}", c.at);
        }
        let p = project([7, 0, 0], scale);
        assert_eq!(under(&m, size, Point::new(p.x + offset.x, p.y + offset.y)), Some(2));
        assert_eq!(under(&m, size, Point::new(-500.0, -500.0)), None);
        assert!(height_for(&m, 300.0) >= 80.0);
    }
}
