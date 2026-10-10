//! A terminal's screen as a widget: its cells on a fixed grid in the mono font, the cursor, and a selection made
//! with the pointer; keys, pastes and the wheel are read while it has the focus. It changes nothing itself: what it
//! reads becomes an [`Input`] for the page's update, and a copy goes straight to the clipboard.
//!
//! The pointer: a click gives it the focus (and a click anywhere else takes it away), a drag selects, a right click
//! copies the selection or, with none, pastes. The keyboard: Ctrl+C copies while text is selected and interrupts
//! otherwise (as Windows Terminal does), Ctrl+Shift+C and Ctrl+Insert copy, Ctrl+V, Ctrl+Shift+V and Shift+Insert
//! paste; the IDE's own Ctrl+P, Ctrl+B, Ctrl+J and Ctrl+` stay the IDE's; every other key goes to the program.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Shell, Widget, clipboard, mouse};
use iced::{Border, Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, keyboard, window};

use crate::theme;

use super::{Chord, PAD, Term, cell, fit, key_bytes, keys, paint, reserved, vt};

/// What the grid read, for the page's update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A click on it (true), or elsewhere while it had the focus (false).
    Focus(bool),
    /// Keys typed, as the program reads them.
    Keys(Vec<u8>),
    /// The clipboard's text, to paste: the page asks first when it would run lines at once or is long.
    Paste(String),
    /// Its size in cells changed.
    Size(u16, u16),
    /// The wheel: lines back into the scrollback (positive) or towards the newest (negative).
    Scroll(i32),
}

pub struct Grid<'a, Message> {
    term: &'a Term,
    focused: bool,
    on: Box<dyn Fn(Input) -> Message + 'a>,
}

/// The grid of `term`; `focused`: it takes the keys.
pub fn grid<'a, Message>(term: &'a Term, focused: bool, on: impl Fn(Input) -> Message + 'a) -> Grid<'a, Message> {
    Grid { term, focused, on: Box::new(on) }
}

/// A cell, as (line number, column): line numbers stay with their lines while they scroll ([`vt::Screen::line`]).
type At = (u64, u16);

/// The grid's own state: the selection, while it is made and once made. It belongs to one terminal: the panel shows
/// one at a time in the same place, and another one shown starts afresh.
#[derive(Debug, Default)]
struct State {
    term: Option<u64>,
    anchor: Option<At>,
    head: Option<At>,
    dragging: bool,
    /// The size last asked for, so it is asked once.
    asked: Option<(u16, u16)>,
    /// Wheel movement not yet a whole line.
    wheel: f32,
}

impl State {
    /// The selection's first and last cells, in reading order.
    fn selection(&self) -> Option<(At, At)> {
        match (self.anchor, self.head) {
            (Some(a), Some(h)) if a != h => Some(if a <= h { (a, h) } else { (h, a) }),
            _ => None,
        }
    }

    fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.dragging = false;
    }
}

impl<Message> Grid<'_, Message> {
    /// The cell under `p` (from the grid's top left), held to the screen.
    fn at(&self, p: Point) -> At {
        let (w, h) = cell();
        let (cols, rows) = self.term.screen.size();
        let col = ((p.x - PAD) / w).floor().clamp(0.0, f32::from(cols.saturating_sub(1))) as u16;
        let row = ((p.y - PAD) / h).floor().clamp(0.0, f32::from(rows.saturating_sub(1))) as u64;
        (self.term.screen.number_of_first(self.term.back, usize::from(rows)) + row, col)
    }

    fn copy(&self, state: &mut State, clipboard: &mut dyn Clipboard, shell: &mut Shell<'_, Message>) {
        if let Some((a, b)) = state.selection() {
            clipboard.write(clipboard::Kind::Standard, self.term.screen.text_between(a, b));
            state.clear();
            shell.request_redraw();
        }
    }
}

fn quad(x: f32, y: f32, width: f32, height: f32) -> Quad {
    Quad { bounds: Rectangle { x, y, width, height }, ..Quad::default() }
}

fn label(content: String, font: Font, width: f32) -> text::Text<String, Font> {
    text::Text {
        content,
        bounds: Size::new(width, cell().1),
        size: Pixels(super::super::CODE_SIZE),
        line_height: text::LineHeight::Absolute(Pixels(cell().1)),
        font,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Auto,
        wrapping: text::Wrapping::None,
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Grid<'_, Message>
where
    Renderer: iced::advanced::Renderer + text::Renderer<Font = Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if state.term != Some(self.term.id) {
            *state = State { term: Some(self.term.id), ..State::default() };
        }
        let bounds = layout.bounds();
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                let fitted = fit(bounds.size());
                if fitted != self.term.screen.size() && state.asked != Some(fitted) {
                    state.asked = Some(fitted);
                    shell.publish((self.on)(Input::Size(fitted.0, fitted.1)));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => match cursor.position_in(bounds) {
                Some(p) => {
                    if !self.focused {
                        shell.publish((self.on)(Input::Focus(true)));
                    }
                    let at = self.at(p);
                    state.anchor = Some(at);
                    state.head = Some(at);
                    state.dragging = true;
                    shell.capture_event();
                    shell.request_redraw();
                }
                None => {
                    if self.focused {
                        shell.publish((self.on)(Input::Focus(false)));
                    }
                }
            },
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                let at = self.at(Point::new(position.x - bounds.x, position.y - bounds.y));
                if state.head != Some(at) {
                    state.head = Some(at);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) if cursor.is_over(bounds) => {
                if !self.focused {
                    shell.publish((self.on)(Input::Focus(true)));
                }
                if state.selection().is_some() {
                    self.copy(state, clipboard, shell);
                } else if let Some(text) = clipboard.read(clipboard::Kind::Standard) {
                    shell.publish((self.on)(Input::Paste(text)));
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => y / cell().1,
                };
                state.wheel += lines;
                let whole = state.wheel.trunc();
                state.wheel -= whole;
                if whole != 0.0 {
                    let n = whole as i32;
                    if self.term.screen.on_alternate_screen() {
                        // A full-screen program (an editor, a pager) keeps no scrollback here: the wheel is its arrows.
                        let key = if n > 0 { keys::Key::Up } else { keys::Key::Down };
                        if let Some(bytes) = keys::encode(key, keys::Mods::default(), self.term.screen.application_cursor) {
                            shell.publish((self.on)(Input::Keys(bytes.repeat(n.unsigned_abs() as usize))));
                        }
                    } else {
                        shell.publish((self.on)(Input::Scroll(n)));
                    }
                }
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, text, .. }) if self.focused => {
                if let Some(chord) = reserved(key, *physical_key, *modifiers) {
                    match chord {
                        // The IDE's own shortcut: its listener acts on it.
                        Chord::Ide => return,
                        Chord::Paste => {
                            if let Some(text) = clipboard.read(clipboard::Kind::Standard) {
                                shell.publish((self.on)(Input::Paste(text)));
                            }
                        }
                        Chord::Copy => self.copy(state, clipboard, shell),
                    }
                    shell.capture_event();
                    return;
                }
                let ctrl_c = modifiers.control()
                    && !modifiers.shift()
                    && !modifiers.alt()
                    && key.to_latin(*physical_key) == Some('c');
                if ctrl_c && state.selection().is_some() {
                    self.copy(state, clipboard, shell);
                    shell.capture_event();
                    return;
                }
                if let Some(bytes) =
                    key_bytes(key, *physical_key, *modifiers, text.as_deref(), self.term.screen.application_cursor)
                {
                    state.clear();
                    shell.publish((self.on)(Input::Keys(bytes)));
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else { return };
        let state = tree.state.downcast_ref::<State>();
        let (w, h) = cell();
        let screen = &self.term.screen;
        let (cols, rows) = screen.size();
        let first = screen.number_of_first(self.term.back, usize::from(rows));
        let lines = screen.lines(self.term.back, usize::from(rows));
        let selection = state.selection().filter(|_| state.term == Some(self.term.id));
        let mono = theme::fonts().mono;
        let bold = Font { weight: iced::font::Weight::Bold, ..mono };
        let (x0, y0) = (bounds.x + PAD, bounds.y + PAD);
        renderer.fill_quad(Quad { bounds, ..Quad::default() }, theme::CANVAS);
        for (r, row) in lines.iter().enumerate() {
            let y = y0 + r as f32 * h;
            if y > clip.y + clip.height {
                break;
            }
            // The row's runs of one pen: their backgrounds, then the selection over them, then their text.
            let mut runs: Vec<(usize, usize, vt::Pen)> = Vec::new();
            let mut c = 0;
            while c < row.len() {
                let (start, pen) = (c, row[c].pen);
                while c < row.len() && row[c].pen == pen {
                    c += 1;
                }
                runs.push((start, c, pen));
            }
            for &(s, e, pen) in &runs {
                if let (_, Some(bg)) = paint(&pen) {
                    renderer.fill_quad(quad(x0 + s as f32 * w, y, (e - s) as f32 * w, h), bg);
                }
            }
            let number = first + r as u64;
            if let Some((a, b)) = selection
                && number >= a.0
                && number <= b.0
            {
                let from = if number == a.0 { a.1 } else { 0 };
                let to = if number == b.0 { b.1 } else { cols.saturating_sub(1) };
                let width = f32::from(to.saturating_sub(from) + 1) * w;
                renderer.fill_quad(quad(x0 + f32::from(from) * w, y, width, h), Color { a: 0.34, ..theme::GOLD });
            }
            for &(s, e, pen) in &runs {
                let content: String = row[s..e].iter().filter(|c| c.ch != vt::WIDE_TAIL).map(|c| c.ch).collect();
                let content = content.trim_end();
                if content.is_empty() {
                    continue;
                }
                let (fg, _) = paint(&pen);
                let x = x0 + s as f32 * w;
                let font = if pen.bold { bold } else { mono };
                renderer.fill_text(label(content.to_string(), font, (e - s + 2) as f32 * w), Point::new(x, y), fg, clip);
                if pen.underline {
                    renderer.fill_quad(quad(x, y + h - 3.0, (e - s) as f32 * w, 1.0), fg);
                }
            }
        }
        // The cursor, where the newest output is shown and the program shows it: a gold block while the grid has the
        // focus, an outline otherwise.
        if self.term.back == 0 && screen.cursor_visible && self.term.running() {
            let (cr, cc) = screen.cursor();
            let under = lines.get(usize::from(cr)).and_then(|row| row.get(usize::from(cc)));
            let width = if under.is_some_and(|c| vt::width(c.ch) == 2) { 2.0 * w } else { w };
            let (x, y) = (x0 + f32::from(cc) * w, y0 + f32::from(cr) * h);
            if self.focused {
                renderer.fill_quad(quad(x, y, width, h), theme::GOLD);
                if let Some(c) = under.filter(|c| c.ch != ' ' && c.ch != vt::WIDE_TAIL) {
                    let font = if c.pen.bold { bold } else { mono };
                    renderer.fill_text(label(c.ch.to_string(), font, width + w), Point::new(x, y), theme::ON_GOLD, clip);
                }
            } else {
                let outline = Border { color: theme::TEXT_FAINT, width: 1.0, radius: 0.0.into() };
                renderer.fill_quad(Quad { border: outline, ..quad(x, y, width, h) }, Color::TRANSPARENT);
            }
        }
        // How far back the reader is, while it is.
        if self.term.back > 0 {
            let words = format!("{} lines back: type or scroll down to return", self.term.back);
            let width = (words.chars().count() as f32 + 2.0) * w;
            let x = bounds.x + bounds.width - width - PAD;
            renderer.fill_quad(
                Quad { border: Border { color: theme::LINE, width: 1.0, radius: 4.0.into() }, ..quad(x, bounds.y + 4.0, width, h + 4.0) },
                theme::RAISED,
            );
            renderer.fill_text(label(words, mono, width), Point::new(x + w, bounds.y + 6.0), theme::TEXT_DIM, clip);
        }
        // The focus, as a gold edge.
        if self.focused {
            let edge = Border { color: theme::GOLD_DIM, width: 1.0, radius: 0.0.into() };
            renderer.fill_quad(Quad { border: edge, ..Quad { bounds, ..Quad::default() } }, Color::TRANSPARENT);
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) { mouse::Interaction::Text } else { mouse::Interaction::None }
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer> From<Grid<'a, Message>> for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer + text::Renderer<Font = Font> + 'a,
{
    fn from(grid: Grid<'a, Message>) -> Self {
        Element::new(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::{Named, NativeCode, Physical};
    use iced::keyboard::{Key, Location, Modifiers};

    /// The clipboard, as a test holds it.
    struct Board(Option<String>);

    impl Clipboard for Board {
        fn read(&self, _kind: clipboard::Kind) -> Option<String> {
            self.0.clone()
        }

        fn write(&mut self, _kind: clipboard::Kind, contents: String) {
            self.0 = Some(contents);
        }
    }

    const SIZE: Size = Size::new(800.0, 400.0);

    /// The middle of the cell at `row`, `col`.
    fn at(row: u16, col: u16) -> Point {
        let (w, h) = cell();
        Point::new(PAD + (f32::from(col) + 0.5) * w, PAD + (f32::from(row) + 0.5) * h)
    }

    /// The grid of `term` reading `event` with the pointer at `pointer`: what it published, and whether it took the
    /// event for itself.
    fn send(term: &Term, focused: bool, tree: &mut Tree, board: &mut Board, event: Event, pointer: Option<Point>) -> (Vec<Input>, bool) {
        let mut grid = grid(term, focused, |input| input);
        let node = layout::Node::new(SIZE);
        let cursor = pointer.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
        let mut published = Vec::new();
        let mut shell = Shell::new(&mut published);
        Widget::<Input, iced::Theme, ()>::update(
            &mut grid,
            tree,
            &event,
            Layout::new(&node),
            cursor,
            &(),
            board,
            &mut shell,
            &Rectangle::with_size(Size::INFINITE),
        );
        let captured = shell.is_event_captured();
        (published, captured)
    }

    fn tree_of(term: &Term) -> Tree {
        let grid = grid(term, false, |input: Input| input);
        Tree::new(&grid as &dyn Widget<Input, iced::Theme, ()>)
    }

    fn key(key: Key, modifiers: Modifiers, text: Option<&str>) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        })
    }

    fn left(pressed: bool) -> Event {
        Event::Mouse(if pressed {
            mouse::Event::ButtonPressed(mouse::Button::Left)
        } else {
            mouse::Event::ButtonReleased(mouse::Button::Left)
        })
    }

    fn term_showing(text: &[u8]) -> Term {
        let mut term = Term::unstarted(1, &std::env::temp_dir());
        term.screen.feed(text);
        term
    }

    #[test]
    fn a_selection_reads_in_order_whichever_way_it_was_dragged_and_a_click_selects_nothing() {
        let mut s = State { anchor: Some((5, 3)), head: Some((2, 7)), ..State::default() };
        assert_eq!(s.selection(), Some(((2, 7), (5, 3))));
        s.head = Some((5, 3));
        assert_eq!(s.selection(), None, "a click without a drag");
        s.clear();
        assert_eq!((s.anchor, s.head, s.dragging), (None, None, false));
    }

    #[test]
    fn a_click_takes_the_focus_a_drag_selects_and_ctrl_c_copies_the_selection_or_interrupts() {
        let term = term_showing(b"hello world\r\nsecond line");
        let mut tree = tree_of(&term);
        let mut board = Board(None);
        let (got, captured) = send(&term, false, &mut tree, &mut board, left(true), Some(at(0, 0)));
        assert_eq!(got, vec![Input::Focus(true)]);
        assert!(captured);
        let moved = Event::Mouse(mouse::Event::CursorMoved { position: at(1, 5) });
        assert_eq!(send(&term, true, &mut tree, &mut board, moved, Some(at(1, 5))).0, vec![]);
        let _ = send(&term, true, &mut tree, &mut board, left(false), Some(at(1, 5)));
        let ctrl_c = key(Key::Character("c".into()), Modifiers::CTRL, None);
        let (got, captured) = send(&term, true, &mut tree, &mut board, ctrl_c.clone(), None);
        assert_eq!((got, captured), (vec![], true), "a copy, not a key");
        assert_eq!(board.0.as_deref(), Some("hello world\r\nsecond"));
        let (got, _) = send(&term, true, &mut tree, &mut board, ctrl_c, None);
        assert_eq!(got, vec![Input::Keys(vec![0x03])], "with nothing selected Ctrl+C interrupts");
    }

    #[test]
    fn keys_reach_the_program_only_with_the_focus_and_the_ides_chords_pass_through() {
        let term = term_showing(b"");
        let mut tree = tree_of(&term);
        let mut board = Board(None);
        let a = key(Key::Character("a".into()), Modifiers::empty(), Some("a"));
        assert_eq!(send(&term, false, &mut tree, &mut board, a.clone(), None), (vec![], false), "no focus, no keys");
        assert_eq!(send(&term, true, &mut tree, &mut board, a, None), (vec![Input::Keys(b"a".to_vec())], true));
        let esc = key(Key::Named(Named::Escape), Modifiers::empty(), None);
        assert_eq!(send(&term, true, &mut tree, &mut board, esc, None), (vec![Input::Keys(vec![0x1B])], true));
        let ctrl_p = key(Key::Character("p".into()), Modifiers::CTRL, None);
        assert_eq!(send(&term, true, &mut tree, &mut board, ctrl_p, None), (vec![], false), "the IDE's Ctrl+P");
        let ctrl_s = key(Key::Character("s".into()), Modifiers::CTRL, None);
        assert_eq!(send(&term, true, &mut tree, &mut board, ctrl_s, None), (vec![Input::Keys(vec![0x13])], true));
    }

    #[test]
    fn a_paste_comes_from_the_clipboard_and_a_right_click_pastes_or_copies() {
        let term = term_showing(b"one two");
        let mut tree = tree_of(&term);
        let mut board = Board(Some("x\ny".into()));
        let ctrl_v = key(Key::Character("v".into()), Modifiers::CTRL, None);
        assert_eq!(send(&term, true, &mut tree, &mut board, ctrl_v, None).0, vec![Input::Paste("x\ny".into())]);
        let right = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
        assert_eq!(send(&term, true, &mut tree, &mut board, right.clone(), Some(at(0, 1))).0, vec![Input::Paste("x\ny".into())]);
        // A selection, then a right click: it is copied, not pasted.
        let _ = send(&term, true, &mut tree, &mut board, left(true), Some(at(0, 0)));
        let moved = Event::Mouse(mouse::Event::CursorMoved { position: at(0, 2) });
        let _ = send(&term, true, &mut tree, &mut board, moved, Some(at(0, 2)));
        let _ = send(&term, true, &mut tree, &mut board, left(false), Some(at(0, 2)));
        assert_eq!(send(&term, true, &mut tree, &mut board, right, Some(at(0, 1))).0, vec![]);
        assert_eq!(board.0.as_deref(), Some("one"));
    }

    #[test]
    fn a_click_elsewhere_gives_up_the_focus_the_wheel_scrolls_back_and_the_size_follows_the_pane() {
        let term = term_showing(b"x");
        let mut tree = tree_of(&term);
        let mut board = Board(None);
        let outside = Point::new(SIZE.width + 50.0, 10.0);
        assert_eq!(send(&term, true, &mut tree, &mut board, left(true), Some(outside)), (vec![Input::Focus(false)], false));
        let wheel = Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 } });
        assert_eq!(send(&term, false, &mut tree, &mut board, wheel, Some(at(2, 2))).0, vec![Input::Scroll(3)]);
        let redraw = Event::Window(window::Event::RedrawRequested(iced::time::Instant::now()));
        let (cols, rows) = fit(SIZE);
        assert_eq!(send(&term, false, &mut tree, &mut board, redraw.clone(), None).0, vec![Input::Size(cols, rows)]);
        assert_eq!(send(&term, false, &mut tree, &mut board, redraw, None).0, vec![], "asked once");
    }

    #[test]
    fn another_terminal_in_the_same_place_starts_without_the_last_ones_selection() {
        let first = term_showing(b"abc");
        let mut tree = tree_of(&first);
        let mut board = Board(None);
        let _ = send(&first, true, &mut tree, &mut board, left(true), Some(at(0, 0)));
        let moved = Event::Mouse(mouse::Event::CursorMoved { position: at(0, 2) });
        let _ = send(&first, true, &mut tree, &mut board, moved, Some(at(0, 2)));
        let mut second = Term::unstarted(2, &std::env::temp_dir());
        second.screen.feed(b"xyz");
        let ctrl_c = key(Key::Character("c".into()), Modifiers::CTRL, None);
        assert_eq!(send(&second, true, &mut tree, &mut board, ctrl_c, None).0, vec![Input::Keys(vec![0x03])]);
        assert_eq!(board.0, None, "nothing of the first terminal's was copied");
    }

}
