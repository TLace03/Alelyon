//! The IDE's terminals: the person's own shell (PowerShell 7 where it is installed, else Windows PowerShell 5.1) on a
//! Windows pseudoconsole, in the bottom panel. It is a real interactive terminal:
//! a PowerShell you type into inside Alelyon, which can run any command you type.
//!
//! - What runs there is what the person types, with the person's own rights, started in the folder the IDE shows.
//!   Keys reach it only from the keyboard while it has the focus (a click on it gives it the focus, a click anywhere
//!   else takes it away, and a gold edge says which), and a paste only from the person's own Ctrl+V, Shift+Insert or
//!   right click; a paste holding a line break the shell would run at once, or a long one, is asked about first.
//!   Nothing a model writes, and no tool of the agent, can type into it, and what it shows is never sent to a model.
//! - Each terminal and everything started in it is in a job object of its own: closing the terminal (asked first
//!   while a program started in it still runs), or Alelyon ending (a crash included), ends them.
//! - Its output is read on a thread of its own and reaches the window through a subscription that runs whichever
//!   page is shown, so a program writing while the page is hidden never fills the pipe and stalls.
//! - Idle costs nothing: no timer and no blinking cursor; the window draws again only when the program writes.

pub mod grid;
pub mod keys;
// The pseudoconsole and its process, through Win32, so it may use unsafe code, as the job object does.
#[allow(unsafe_code)]
pub mod pty;
pub mod vt;

use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use futures::channel::mpsc;
use futures::stream::{BoxStream, StreamExt};
use iced::Color;

use crate::theme;

/// The most terminals open at once.
pub const MAX_TERMINALS: usize = 6;
/// The terminal's padding inside its pane.
pub const PAD: f32 = 8.0;
/// A paste longer than this many characters is asked about first, whatever it holds.
pub const LONG_PASTE: usize = 5_000;

/// What a terminal's subscription hands back: a piece of its output, or its end.
#[derive(Clone, Debug)]
pub enum Event {
    Output(Vec<u8>),
    Ended,
}

/// A terminal's output, for its subscription: taken once, by the stream that reads it.
#[derive(Clone)]
pub struct Feed {
    pub id: u64,
    receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<Vec<u8>>>>>,
}

impl std::fmt::Debug for Feed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feed").field("id", &self.id).finish()
    }
}

impl Hash for Feed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "lattice-terminal".hash(state);
        self.id.hash(state);
    }
}

/// The terminal's output as events: what arrived together as one piece (so a burst is drawn once), then its end.
pub fn feed_stream(feed: &Feed) -> BoxStream<'static, (u64, Event)> {
    let id = feed.id;
    match feed.receiver.lock().unwrap_or_else(|p| p.into_inner()).take() {
        Some(rx) => rx
            .ready_chunks(64)
            .map(move |pieces| (id, Event::Output(pieces.concat())))
            .chain(futures::stream::once(async move { (id, Event::Ended) }))
            .boxed(),
        None => futures::stream::empty().boxed(),
    }
}

/// A started terminal on its way from the thread that started it to the window (a message is cloned; the terminal
/// is taken from it once).
#[derive(Clone)]
pub struct Started(Arc<Mutex<Option<Term>>>);

impl Started {
    pub fn new(term: Term) -> Started {
        Started(Arc::new(Mutex::new(Some(term))))
    }

    pub fn take(&self) -> Option<Term> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

impl std::fmt::Debug for Started {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Started")
    }
}

/// One terminal.
pub struct Term {
    pub id: u64,
    /// Which shell: "PowerShell 7" or "Windows PowerShell".
    pub label: String,
    /// The folder it started in.
    pub cwd: std::path::PathBuf,
    pub screen: vt::Screen,
    pty: Option<pty::Pty>,
    /// The job its shell and everything the shell starts are in: closed after the pseudoconsole.
    job: Option<crate::job::Job>,
    feed: Feed,
    /// Why it ended, once it has.
    pub ended: Option<String>,
    /// Lines scrolled back from the newest.
    pub back: usize,
}

impl std::fmt::Debug for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Term").field("id", &self.id).field("label", &self.label).field("ended", &self.ended).finish()
    }
}

impl Term {
    /// Start the shell in `cwd`, `cols` by `rows` (it blocks while the process starts: call it off the window's
    /// thread).
    pub fn start(id: u64, cwd: &Path, cols: u16, rows: u16) -> Result<Term, String> {
        let (program, args) = pty::shell(|name| std::env::var(name).ok()).ok_or_else(|| {
            "No PowerShell was found on this computer (PowerShell 7 under Program Files, or Windows PowerShell under the \
             Windows folder)."
                .to_string()
        })?;
        if !cwd.is_dir() {
            return Err(format!("{} is not a folder this computer can open.", cwd.display()));
        }
        let job = crate::job::Job::new()?;
        let spawned =
            pty::Pty::spawn(&program, &args, cwd, cols, rows, |process| job.adopt_handle(process, "the terminal"))?;
        let (tx, rx) = mpsc::unbounded();
        let output = spawned.output;
        // The reader drains the pipe until it ends, even once nobody reads what it hands on: closing a pseudoconsole
        // can wait until its output is drained.
        std::thread::Builder::new()
            .name("terminal-read".into())
            .spawn(move || {
                pty::read_all(output, |bytes| {
                    let _ = tx.unbounded_send(bytes.to_vec());
                    true
                })
            })
            .map_err(|e| format!("The terminal's reader could not start: {e}"))?;
        let pwsh = program.file_name().is_some_and(|n| n.eq_ignore_ascii_case("pwsh.exe"));
        Ok(Term {
            id,
            label: if pwsh { "PowerShell 7" } else { "Windows PowerShell" }.to_string(),
            cwd: cwd.to_path_buf(),
            screen: vt::Screen::new(cols, rows),
            pty: Some(spawned.pty),
            job: Some(job),
            feed: Feed { id, receiver: Arc::new(Mutex::new(Some(rx))) },
            ended: None,
            back: 0,
        })
    }

    /// A terminal with no shell behind it, for the page's own tests.
    #[cfg(test)]
    pub fn unstarted(id: u64, cwd: &Path) -> Term {
        Term {
            id,
            label: "PowerShell 7".to_string(),
            cwd: cwd.to_path_buf(),
            screen: vt::Screen::new(80, 24),
            pty: None,
            job: None,
            feed: Feed { id, receiver: Arc::new(Mutex::new(None)) },
            ended: None,
            back: 0,
        }
    }

    pub fn feed(&self) -> Feed {
        self.feed.clone()
    }

    pub fn running(&self) -> bool {
        self.ended.is_none() && self.pty.is_some()
    }

    /// A program the person started in it still runs (besides its shell, while the shell runs: a program can outlive
    /// the shell's own `exit`, and it runs until the terminal closes).
    pub fn busy(&self) -> bool {
        let running = self.job.as_ref().and_then(crate::job::Job::running).unwrap_or(0);
        running > u32::from(self.running())
    }

    /// What the program wrote: onto the screen, and its queries answered. A reader scrolled back keeps reading the
    /// same lines while new ones arrive.
    pub fn output(&mut self, bytes: &[u8]) {
        let before = self.screen.scrolled();
        self.screen.feed(bytes);
        let replies = self.screen.take_replies();
        if !replies.is_empty() {
            let _ = self.write(&replies);
        }
        if self.back > 0 {
            let grown = usize::try_from(self.screen.scrolled() - before).unwrap_or(usize::MAX);
            self.back = self.back.saturating_add(grown).min(self.screen.scrollback_len());
        }
    }

    /// Send what the person typed or pasted; the newest output is shown again.
    pub fn input(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.back = 0;
        self.write(bytes)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        match &mut self.pty {
            Some(pty) if self.ended.is_none() => {
                pty.write(bytes).map_err(|e| format!("The terminal did not take the keys: {e}"))
            }
            _ => Err("The terminal has ended.".to_string()),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if self.screen.size() == (cols, rows) {
            return;
        }
        self.screen.resize(cols, rows);
        self.back = self.back.min(self.screen.scrollback_len());
        if let Some(pty) = &self.pty {
            let _ = pty.resize(cols, rows);
        }
    }

    /// Its output stopped: the shell ended (with its exit code, when Windows still says it). Its job stays, so what
    /// it started runs on until the terminal closes.
    pub fn ended(&mut self) {
        let code = self.pty.as_ref().and_then(pty::Pty::exited);
        self.ended = Some(match code {
            Some(0) | None => format!("{} ended.", self.label),
            Some(code) => format!("{} ended with exit code {code}.", self.label),
        });
        self.pty = None;
    }

    /// Scroll `lines` back into the scrollback (negative: towards the newest).
    pub fn scroll(&mut self, lines: i32) {
        let max = self.screen.scrollback_len() as i64;
        self.back = (self.back as i64 + i64::from(lines)).clamp(0, max) as usize;
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        // The pseudoconsole first (it ends the shell's console), then the job (it ends what the shell started).
        self.pty = None;
        self.job = None;
    }
}

/// What is pasted of `text`: a single line loses its line end, so it is typed and not run (as Windows Terminal's
/// trimPaste does); several lines are pasted as they are.
pub fn paste_text(text: &str) -> &str {
    let line = text.trim_end_matches(['\r', '\n']);
    if line.contains(['\r', '\n']) { text } else { line }
}

/// Whether a paste is asked about first: it holds a line break the shell would run at once (the program did not ask
/// for bracketed pastes), or it is long.
pub fn paste_needs_asking(text: &str, bracketed: bool) -> bool {
    (!bracketed && text.contains(['\n', '\r'])) || text.chars().count() > LONG_PASTE
}

/// The width and height of one cell of the terminal's font, measured once from the font itself.
pub fn cell() -> (f32, f32) {
    static CELL: OnceLock<f32> = OnceLock::new();
    let width = *CELL.get_or_init(|| {
        use iced::advanced::text::Paragraph as _;
        let sample = "M".repeat(100);
        let measured = iced::advanced::graphics::text::Paragraph::with_text(iced::advanced::Text {
            content: sample.as_str(),
            bounds: iced::Size::INFINITE,
            size: iced::Pixels(super::CODE_SIZE),
            line_height: iced::widget::text::LineHeight::Absolute(iced::Pixels(super::LINE_HEIGHT)),
            font: theme::fonts().mono,
            align_x: iced::widget::text::Alignment::Left,
            align_y: iced::alignment::Vertical::Top,
            shaping: iced::widget::text::Shaping::Basic,
            wrapping: iced::widget::text::Wrapping::None,
        })
        .min_width()
            / 100.0;
        // A font system with no monospace face gives nothing sensible: the usual advance of a 13 px mono face.
        if measured.is_finite() && measured > 3.0 { measured } else { super::CODE_SIZE * 0.6 }
    });
    (width, super::LINE_HEIGHT)
}

/// The columns and rows that fit `size`.
pub fn fit(size: iced::Size) -> (u16, u16) {
    let (w, h) = cell();
    let cols = ((size.width - 2.0 * PAD) / w).floor().clamp(10.0, f32::from(vt::MAX_COLS)) as u16;
    let rows = ((size.height - 2.0 * PAD) / h).floor().clamp(3.0, f32::from(vt::MAX_ROWS)) as u16;
    (cols, rows)
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

/// The sixteen named colours in Alelyon's black and gold: every one reads (4.5:1) on the terminal's black.
const NAMED: [Color; 16] = [
    rgb(0x8a, 0x85, 0x78),
    rgb(0xd9, 0x70, 0x7f),
    rgb(0x54, 0xb9, 0xa5),
    rgb(0xe6, 0xc4, 0x6a),
    rgb(0x9f, 0xb4, 0xd9),
    rgb(0xc9, 0xa8, 0xdc),
    rgb(0x8f, 0xc7, 0xd6),
    rgb(0xec, 0xe8, 0xdc),
    rgb(0x9a, 0x95, 0x88),
    rgb(0xf0, 0xa0, 0xa8),
    rgb(0x8f, 0xd8, 0xc4),
    rgb(0xf0, 0xd3, 0x8b),
    rgb(0xc2, 0xd2, 0xee),
    rgb(0xe0, 0xc8, 0xee),
    rgb(0xb8, 0xe2, 0xec),
    rgb(0xff, 0xff, 0xff),
];

/// A cell colour as the window draws it; `None` is the terminal's own (its text colour, or no background).
pub fn colour(c: vt::Colour) -> Option<Color> {
    match c {
        vt::Colour::Default => None,
        vt::Colour::Index(i) if i < 16 => Some(NAMED[usize::from(i)]),
        vt::Colour::Index(i) if i < 232 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            Some(Color::from_rgb8(level(n / 36), level((n / 6) % 6), level(n % 6)))
        }
        vt::Colour::Index(i) => {
            let g = 8 + 10 * (i - 232);
            Some(Color::from_rgb8(g, g, g))
        }
        vt::Colour::Rgb(r, g, b) => Some(Color::from_rgb8(r, g, b)),
    }
}

/// A pen's text and background colours, inverse and dim applied.
pub fn paint(pen: &vt::Pen) -> (Color, Option<Color>) {
    let fg = colour(pen.fg).unwrap_or(theme::TEXT);
    let bg = colour(pen.bg);
    let (fg, bg) = if pen.inverse { (bg.unwrap_or(theme::CANVAS), Some(fg)) } else { (fg, bg) };
    let fg = if pen.dim { Color { a: 0.62, ..fg } } else { fg };
    (fg, bg)
}

/// A key the window read, in the bytes the terminal's program reads; `None` for a key that sends nothing.
pub fn key_bytes(
    key: &iced::keyboard::Key,
    physical: iced::keyboard::key::Physical,
    modifiers: iced::keyboard::Modifiers,
    text: Option<&str>,
    application_cursor: bool,
) -> Option<Vec<u8>> {
    use iced::keyboard::Key as K;
    use iced::keyboard::key::Named as N;
    let mods = keys::Mods { shift: modifiers.shift(), ctrl: modifiers.control(), alt: modifiers.alt() };
    let k = match key.as_ref() {
        K::Named(N::Enter) => keys::Key::Enter,
        K::Named(N::Backspace) => keys::Key::Backspace,
        K::Named(N::Tab) => keys::Key::Tab,
        K::Named(N::Escape) => keys::Key::Escape,
        K::Named(N::ArrowUp) => keys::Key::Up,
        K::Named(N::ArrowDown) => keys::Key::Down,
        K::Named(N::ArrowLeft) => keys::Key::Left,
        K::Named(N::ArrowRight) => keys::Key::Right,
        K::Named(N::Home) => keys::Key::Home,
        K::Named(N::End) => keys::Key::End,
        K::Named(N::PageUp) => keys::Key::PageUp,
        K::Named(N::PageDown) => keys::Key::PageDown,
        K::Named(N::Insert) => keys::Key::Insert,
        K::Named(N::Delete) => keys::Key::Delete,
        K::Named(N::F1) => keys::Key::F(1),
        K::Named(N::F2) => keys::Key::F(2),
        K::Named(N::F3) => keys::Key::F(3),
        K::Named(N::F4) => keys::Key::F(4),
        K::Named(N::F5) => keys::Key::F(5),
        K::Named(N::F6) => keys::Key::F(6),
        K::Named(N::F7) => keys::Key::F(7),
        K::Named(N::F8) => keys::Key::F(8),
        K::Named(N::F9) => keys::Key::F(9),
        K::Named(N::F10) => keys::Key::F(10),
        K::Named(N::F11) => keys::Key::F(11),
        K::Named(N::F12) => keys::Key::F(12),
        K::Named(N::Space) if modifiers.control() && !modifiers.alt() => keys::Key::Ctrl(' '),
        // Ctrl with Alt is AltGr on Windows layouts: what it types is text (an @, a {), not a control code.
        K::Character(_) if modifiers.control() && !modifiers.alt() => keys::Key::Ctrl(key.to_latin(physical)?),
        _ => {
            let alt = keys::Mods { alt: mods.alt && !mods.ctrl, ..keys::Mods::default() };
            return keys::encode(keys::Key::Text(text?), alt, application_cursor);
        }
    };
    keys::encode(k, mods, application_cursor)
}

/// The chords a terminal with the focus does not send to its program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    /// One of the IDE's own: Ctrl+P (go to file), Ctrl+B (the side bar), Ctrl+J and Ctrl+` (the panel).
    Ide,
    /// Ctrl+V, Ctrl+Shift+V, Shift+Insert.
    Paste,
    /// Ctrl+Shift+C, Ctrl+Insert.
    Copy,
}

pub fn reserved(
    key: &iced::keyboard::Key,
    physical: iced::keyboard::key::Physical,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Chord> {
    use iced::keyboard::Key as K;
    use iced::keyboard::key::Named as N;
    if let K::Named(N::Insert) = key.as_ref() {
        return match (modifiers.shift(), modifiers.control()) {
            (true, false) => Some(Chord::Paste),
            (false, true) => Some(Chord::Copy),
            _ => None,
        };
    }
    if !modifiers.control() || modifiers.alt() {
        return None;
    }
    match key.to_latin(physical)? {
        'p' if !modifiers.shift() => Some(Chord::Ide),
        'b' | 'j' | '`' => Some(Chord::Ide),
        'v' => Some(Chord::Paste),
        'c' if modifiers.shift() => Some(Chord::Copy),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn every_named_colour_reads_on_the_terminals_black() {
        use lattice_app::theme::{CANVAS, contrast};
        for (i, c) in NAMED.iter().enumerate() {
            assert!(contrast(*c, CANVAS) >= 4.5, "colour {i}: {:.2}", contrast(*c, CANVAS));
        }
    }

    #[test]
    fn the_256_colours_and_the_pens_paint_as_xterm_maps_them() {
        assert_eq!(colour(vt::Colour::Index(16)), Some(Color::from_rgb8(0, 0, 0)));
        assert_eq!(colour(vt::Colour::Index(231)), Some(Color::from_rgb8(255, 255, 255)));
        assert_eq!(colour(vt::Colour::Index(196)), Some(Color::from_rgb8(255, 0, 0)));
        assert_eq!(colour(vt::Colour::Index(232)), Some(Color::from_rgb8(8, 8, 8)));
        assert_eq!(colour(vt::Colour::Index(255)), Some(Color::from_rgb8(238, 238, 238)));
        assert_eq!(colour(vt::Colour::Default), None);
        assert_eq!(paint(&vt::Pen::default()), (theme::TEXT, None));
        let inverse = vt::Pen { inverse: true, ..vt::Pen::default() };
        assert_eq!(paint(&inverse), (theme::CANVAS, Some(theme::TEXT)));
    }

    #[test]
    fn keys_map_from_the_window_and_the_ides_chords_stay_the_ides() {
        use iced::keyboard::key::{Named, NativeCode, Physical};
        use iced::keyboard::{Key, Modifiers};
        let none = Physical::Unidentified(NativeCode::Unidentified);
        let c = Key::Character("c".into());
        assert_eq!(key_bytes(&c, none, Modifiers::CTRL, None, false), Some(vec![0x03]), "Ctrl+C interrupts");
        assert_eq!(key_bytes(&Key::Named(Named::ArrowUp), none, Modifiers::empty(), None, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(&Key::Character("a".into()), none, Modifiers::empty(), Some("a"), false), Some(b"a".to_vec()));
        assert_eq!(key_bytes(&Key::Named(Named::Shift), none, Modifiers::SHIFT, None, false), None, "a lone modifier");
        let altgr = Modifiers::CTRL | Modifiers::ALT;
        assert_eq!(key_bytes(&Key::Character("q".into()), none, altgr, Some("@"), false), Some(b"@".to_vec()), "AltGr");
        assert_eq!(reserved(&Key::Character("p".into()), none, Modifiers::CTRL), Some(Chord::Ide));
        assert_eq!(reserved(&Key::Character("`".into()), none, Modifiers::CTRL), Some(Chord::Ide));
        assert_eq!(reserved(&Key::Character("v".into()), none, Modifiers::CTRL), Some(Chord::Paste));
        assert_eq!(reserved(&Key::Named(Named::Insert), none, Modifiers::SHIFT), Some(Chord::Paste));
        assert_eq!(reserved(&c, none, Modifiers::CTRL | Modifiers::SHIFT), Some(Chord::Copy));
        assert_eq!(reserved(&c, none, Modifiers::CTRL), None, "Ctrl+C goes to the shell");
        for letter in ["s", "w", "l", "r"] {
            assert_eq!(reserved(&Key::Character(letter.into()), none, Modifiers::CTRL), None, "Ctrl+{letter} is the shell's");
        }
    }

    #[test]
    fn a_paste_that_would_run_lines_or_is_long_is_asked_about_and_one_line_is_typed_not_run() {
        assert_eq!(paste_text("cargo test\r\n"), "cargo test", "one line loses its end: typed, not run");
        assert_eq!(paste_text("a\nb\n"), "a\nb\n", "several lines are pasted as they are");
        assert!(!paste_needs_asking(paste_text("cargo test\r\n"), false));
        assert!(paste_needs_asking(paste_text("cd x\r\nrm -r y"), false));
        assert!(!paste_needs_asking("cd x\nrm -r y", true), "a bracketed paste is not run on arrival");
        assert!(paste_needs_asking(&"x".repeat(LONG_PASTE + 1), true));
    }

    #[test]
    fn a_pane_fits_whole_cells_within_the_bounds() {
        let (w, h) = cell();
        assert!(w > 3.0 && h == super::super::LINE_HEIGHT);
        let (cols, rows) = fit(iced::Size::new(2.0 * PAD + 80.0 * w + 1.0, 2.0 * PAD + 24.0 * h + 1.0));
        assert_eq!((cols, rows), (80, 24));
        assert_eq!(fit(iced::Size::new(1.0, 1.0)), (10, 3));
    }

    /// Read the terminal's output until `done` holds of it or `seconds` pass.
    fn read_until(
        term: &mut Term,
        stream: &mut BoxStream<'static, (u64, Event)>,
        seconds: u64,
        done: impl Fn(&Term) -> bool,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if done(term) {
                return true;
            }
            let next = futures::executor::block_on(async {
                let timer = timer(Duration::from_millis(200));
                futures::pin_mut!(timer);
                match futures::future::select(stream.next(), timer).await {
                    futures::future::Either::Left((item, _)) => Some(item),
                    futures::future::Either::Right(_) => None,
                }
            });
            match next {
                Some(Some((_, Event::Output(bytes)))) => term.output(&bytes),
                Some(Some((_, Event::Ended))) | Some(None) => {
                    term.ended();
                    return done(term);
                }
                None => {}
            }
        }
        done(term)
    }

    async fn timer(d: Duration) {
        let (tx, rx) = futures::channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(d);
            let _ = tx.send(());
        });
        let _ = rx.await;
    }

    #[test]
    fn a_real_shell_starts_in_the_folder_runs_what_is_typed_and_its_children_end_with_it() {
        let dir = std::env::temp_dir();
        let mut term = Term::start(1, &dir, 100, 30).expect("PowerShell starts");
        let feed = term.feed();
        let mut stream = feed_stream(&feed);
        assert!(futures::executor::block_on(feed_stream(&feed).next()).is_none(), "the output is read by one stream");
        term.input(b"Write-Output ('alelyon-' + (6*7)); (Get-Location).Path\r").unwrap();
        let folder = dir.display().to_string().trim_end_matches('\\').to_string();
        assert!(read_until(&mut term, &mut stream, 60, |t| t.screen.text().contains("alelyon-42")), "{}", term.screen.text());
        assert!(read_until(&mut term, &mut stream, 10, |t| t.screen.text().contains(&folder)), "{}", term.screen.text());
        assert!(term.running() && !term.busy(), "only the shell runs at its prompt");
        // A program it starts is in its job: the terminal is busy while it runs.
        term.input(b"ping -n 30 127.0.0.1\r").unwrap();
        assert!(read_until(&mut term, &mut stream, 20, Term::busy), "{}", term.screen.text());
        drop(term);
        // Closing ended the shell and the program: the output ends.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut ended = false;
        while Instant::now() < deadline && !ended {
            ended = futures::executor::block_on(stream.next()).is_none_or(|(_, e)| matches!(e, Event::Ended));
        }
        assert!(ended, "the output ended when the terminal closed");
    }

    #[test]
    fn a_shell_that_exits_says_so_and_takes_no_more_keys() {
        let mut term = Term::start(2, &std::env::temp_dir(), 80, 24).expect("PowerShell starts");
        let feed = term.feed();
        let mut stream = feed_stream(&feed);
        term.input(b"exit 3\r").unwrap();
        assert!(read_until(&mut term, &mut stream, 60, |t| t.ended.is_some()), "{}", term.screen.text());
        assert_eq!(term.ended.as_deref(), Some(format!("{} ended with exit code 3.", term.label).as_str()));
        assert!(!term.running() && term.input(b"x").is_err());
        assert!(!term.busy(), "nothing it started runs on");
    }

    #[test]
    fn an_idle_shell_at_its_prompt_writes_nothing_so_the_window_is_not_woken() {
        let mut term = Term::start(4, &std::env::temp_dir(), 100, 30).expect("PowerShell starts");
        let feed = term.feed();
        let mut stream = feed_stream(&feed);
        term.input(b"Write-Output ready-$PID\r").unwrap();
        assert!(read_until(&mut term, &mut stream, 60, |t| t.screen.text().contains("ready-")), "{}", term.screen.text());
        // What the shell writes in its first quiet seconds (the prompt), then a window of nothing at all.
        let _ = read_until(&mut term, &mut stream, 2, |_| false);
        let generation = term.screen.generation;
        let _ = read_until(&mut term, &mut stream, 5, |_| false);
        assert_eq!(term.screen.generation, generation, "an idle shell wrote: {:?}", term.screen.text());
    }

    #[test]
    fn a_program_started_from_the_shell_outlives_its_exit_until_the_terminal_closes() {
        let mut term = Term::start(3, &std::env::temp_dir(), 120, 24).expect("PowerShell starts");
        let feed = term.feed();
        let mut stream = feed_stream(&feed);
        // A console program with no window of its own (so it is not on the terminal's console), then the shell exits.
        term.input(
            b"$s = New-Object Diagnostics.ProcessStartInfo 'ping.exe', '-n 60 127.0.0.1'; $s.CreateNoWindow = $true; \
              $s.UseShellExecute = $false; [Diagnostics.Process]::Start($s) | Out-Null; exit\r",
        )
        .unwrap();
        assert!(read_until(&mut term, &mut stream, 60, |t| t.ended.is_some()), "{}", term.screen.text());
        assert!(term.busy(), "the program runs on after the shell's exit");
        // Closing the terminal ends it (its job's last handle closes).
        drop(term);
    }
}
