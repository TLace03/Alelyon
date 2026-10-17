//! The file-type icon beside a file's name: in the Explorer's tree, the editor's tabs, quick open and search. The
//! icons are Material Icon Theme's (MIT: `LICENSE` beside this file), the set the website's chat shows.
//! `data.rs` is generated with the website's copies by `tools/sync_file_icons.py` from
//! `assets/file-icons/icons.json`: edit that, never `data.rs`.
//!
//! An icon is drawn from its SVG path data on a canvas, so it is sharp at any scale and needs no SVG renderer. The
//! drawing is kept between frames (`ui::Kept`) and made again only when the icon or its size changes.

// Generated, and written as the generator writes it: a formatter would only make it differ from its source.
#[rustfmt::skip]
mod data;
pub mod path;

use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

pub use data::{FILE, FOLDER, FOLDER_OPEN};

/// One icon: its view box (`x`, `y`, width, height) and the shapes it fills, in order.
pub struct Icon {
    /// The icon's file name in the set; the tests name icons by it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub name: &'static str,
    pub view_box: [f32; 4],
    pub fills: &'static [Fill],
}

/// A shape and its paint.
pub struct Fill {
    pub paint: Paint,
    /// SVG path data, as the icon's file has it.
    pub d: &'static str,
}

/// A flat colour (`0xRRGGBB`), or a linear gradient in the view box's coordinates.
pub enum Paint {
    Solid(u32),
    Linear { from: [f32; 2], to: [f32; 2], stops: &'static [(f32, u32)] },
}

/// The icon for a file name or path: a whole-name rule (`README.md`, `Dockerfile`, `package.json`) before the
/// extension (the part after the last dot, or the whole name when it has none). Anything else is a plain file.
pub fn index_for(path: &str) -> usize {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
    if let Ok(at) = data::NAMES.binary_search_by(|&(n, _)| n.cmp(name.as_str())) {
        return usize::from(data::NAMES[at].1);
    }
    let extension = name.rsplit('.').next().unwrap_or(name.as_str());
    data::EXTENSIONS.binary_search_by(|&(e, _)| e.cmp(extension)).map_or(FILE, |at| usize::from(data::EXTENSIONS[at].1))
}

/// A folder's icon, open or closed.
pub fn folder(open: bool) -> usize {
    if open { FOLDER_OPEN } else { FOLDER }
}

/// Icon `index`, `size` pixels square.
pub fn icon<'a, Message: 'a>(index: usize, size: f32) -> Element<'a, Message> {
    Canvas::new(Drawn(index)).width(Length::Fixed(size)).height(Length::Fixed(size)).into()
}

struct Drawn(usize);

impl<Message> canvas::Program<Message> for Drawn {
    type State = crate::ui::Kept;

    fn draw(&self, kept: &crate::ui::Kept, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        vec![kept.draw(renderer, bounds.size(), self.0 as u64, |frame| paint(frame, self.0))]
    }
}

fn paint(frame: &mut Frame, index: usize) {
    let Some(icon) = data::ICONS.get(index) else { return };
    let [x0, y0, w, h] = icon.view_box;
    let scale = (frame.width() / w).min(frame.height() / h);
    let (dx, dy) = ((frame.width() - w * scale) / 2.0 - x0 * scale, (frame.height() - h * scale) / 2.0 - y0 * scale);
    let at = |p: [f32; 2]| Point::new(p[0] * scale + dx, p[1] * scale + dy);
    for fill in icon.fills {
        // A shape that does not parse is left out rather than drawn wrong; the tests parse every one.
        let Ok(segments) = path::parse(fill.d) else { continue };
        let shape = canvas::Path::new(|b| path::trace(b, &segments, &at));
        let style = match fill.paint {
            Paint::Solid(rgb) => canvas::Style::Solid(color(rgb)),
            Paint::Linear { from, to, stops } => {
                let linear = stops.iter().fold(canvas::gradient::Linear::new(at(from), at(to)), |g, &(offset, rgb)| g.add_stop(offset, color(rgb)));
                canvas::Style::Gradient(canvas::Gradient::Linear(linear))
            }
        };
        frame.fill(&shape, canvas::Fill { style, rule: canvas::fill::Rule::NonZero });
    }
}

fn color(rgb: u32) -> Color {
    Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(path: &str) -> &'static str {
        data::ICONS[index_for(path)].name
    }

    #[test]
    fn a_name_finds_the_icon_a_reader_expects() {
        for (path, icon) in [
            ("src/main.rs", "rust"),
            ("app.py", "python"),
            ("www/src/pages/index.astro", "astro"),
            ("README.md", "readme"),
            ("docs\\notes.md", "markdown"),
            ("Cargo.toml", "toml"),
            ("Cargo.lock", "lock"),
            ("Dockerfile", "docker"),
            ("package.json", "nodejs"),
            ("tsconfig.json", "json"),
            ("App.tsx", "react_ts"),
            (".gitignore", "git"),
            (".env", "tune"),
            ("build.ps1", "powershell"),
            ("shader.wgsl", "shader"),
            ("notes.unknown", "file"),
            ("", "file"),
        ] {
            assert_eq!(named(path), icon, "{path}");
        }
        assert_eq!(data::ICONS[folder(false)].name, "folder");
        assert_eq!(data::ICONS[folder(true)].name, "folder-open");
    }

    #[test]
    fn the_lookup_tables_are_sorted_and_name_real_icons() {
        for table in [&data::EXTENSIONS[..], &data::NAMES[..]] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0), "sorted, each key once");
            assert!(table.iter().all(|&(_, i)| usize::from(i) < data::ICONS.len()));
        }
    }

    #[test]
    fn no_shape_is_black_as_a_lost_fill_would_be() {
        // Five icons hide a frame under their root's fill="none"; a generator that lost that inheritance would give
        // the frame SVG's default fill, black, and draw a black square over the icon. None of the set is black.
        for icon in &data::ICONS {
            assert!(icon.fills.iter().all(|f| !matches!(f.paint, Paint::Solid(0x000000))), "{} fills black", icon.name);
        }
        let readme = data::ICONS.iter().find(|i| i.name == "readme").expect("the readme icon");
        assert_eq!(readme.fills.len(), 1, "its 24x24 frame is not drawn");
    }

    #[test]
    fn every_icon_parses_and_stays_near_its_view_box() {
        for icon in &data::ICONS {
            let [x0, y0, w, h] = icon.view_box;
            assert!(!icon.fills.is_empty(), "{} draws nothing", icon.name);
            for fill in icon.fills {
                let segments = path::parse(fill.d).unwrap_or_else(|e| panic!("{}: {e}", icon.name));
                assert!(!segments.is_empty(), "{}", icon.name);
                for [x, y] in path::points(&segments) {
                    assert!(x >= x0 - 0.1 * w && x <= x0 + 1.1 * w && y >= y0 - 0.1 * h && y <= y0 + 1.1 * h, "{}: ({x}, {y}) is far outside its view box", icon.name);
                }
            }
        }
    }
}
