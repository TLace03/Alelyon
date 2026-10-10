//! A debug aid: how often the window updates, rebuilds its view and draws a frame, and which messages drive it.
//!
//! Off unless `CENTCOM_FRAME_STATS` is set. With `1` (or `true`) each second's counts go to standard error; with any
//! other value they are appended to the file it names. One line a second, while anything happened that second:
//!
//! ```text
//! frame-stats t=12 section=overview updates=30 views=30 frames=30 kinds=Tick:30
//! ```
//!
//! `updates` counts messages handled by `App::update`, `views` the times the window's view was built, and `frames` the
//! frames iced drew (its `RedrawRequested` events, which include frames a widget asked for without a message, such as
//! an animation). Counting costs an atomic add per event and, per message, its variant's name (the text before the
//! first bracket or space of its debug form, cut off there so a large payload is never formatted).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Where the lines go, when counting is on.
enum Sink {
    Stderr,
    File(std::path::PathBuf),
}

struct Stats {
    sink: Sink,
    started: Instant,
    views: AtomicU64,
    frames: AtomicU64,
    /// Message kinds handled this second, by name, and the section on show.
    kinds: Mutex<BTreeMap<String, u64>>,
    section: Mutex<&'static str>,
}

static STATS: OnceLock<Option<Stats>> = OnceLock::new();

fn stats() -> Option<&'static Stats> {
    STATS
        .get_or_init(|| {
            let value = std::env::var("CENTCOM_FRAME_STATS").ok().filter(|v| !v.trim().is_empty() && v.trim() != "0")?;
            let sink = if matches!(value.trim(), "1" | "true") { Sink::Stderr } else { Sink::File(value.trim().into()) };
            let _ = std::thread::Builder::new().name("centcom-frame-stats".into()).spawn(report_every_second);
            Some(Stats {
                sink,
                started: Instant::now(),
                views: AtomicU64::new(0),
                frames: AtomicU64::new(0),
                kinds: Mutex::new(BTreeMap::new()),
                section: Mutex::new(""),
            })
        })
        .as_ref()
}

/// Whether counting is on (`CENTCOM_FRAME_STATS`).
pub fn enabled() -> bool {
    stats().is_some()
}

/// A message is about to be handled.
pub fn message(message: &impl std::fmt::Debug) {
    if let Some(s) = stats() {
        let name = kind(message);
        if let Ok(mut kinds) = s.kinds.lock() {
            *kinds.entry(name).or_default() += 1;
        }
    }
}

/// The section on show, named as `--section` names it.
pub fn section(name: &'static str) {
    if let Some(s) = stats()
        && let Ok(mut section) = s.section.lock()
    {
        *section = name;
    }
}

/// The view was built.
pub fn view() {
    if let Some(s) = stats() {
        s.views.fetch_add(1, Ordering::Relaxed);
    }
}

/// For `iced::event::listen_raw`: counts each frame drawn and never makes a message (a message would draw another).
pub fn frames<Message>(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    if let iced::Event::Window(iced::window::Event::RedrawRequested(_)) = event
        && let Some(s) = stats()
    {
        s.frames.fetch_add(1, Ordering::Relaxed);
    }
    None
}

/// A message's name: its debug form up to the second bracket or the first space after a nested variant's name, so
/// `Fleet(Refresh)` and `Lattice(Ide(ConfirmTick))` are told apart, but a payload is never formatted whole.
pub fn kind(message: &impl std::fmt::Debug) -> String {
    /// Takes the debug text until it has `depth` names, then refuses more (which ends the formatting early).
    struct Names {
        out: String,
        depth: usize,
    }
    impl std::fmt::Write for Names {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            for c in s.chars() {
                match c {
                    '(' if self.depth < 3 => {
                        self.depth += 1;
                        self.out.push('(');
                    }
                    c if c.is_alphanumeric() || c == '_' => self.out.push(c),
                    _ => return Err(std::fmt::Error),
                }
            }
            Ok(())
        }
    }
    let mut names = Names { out: String::new(), depth: 0 };
    let _ = write!(names, "{message:?}");
    let mut out = names.out.trim_end_matches('(').to_string();
    let open = out.matches('(').count();
    out.extend(std::iter::repeat_n(')', open));
    out
}

/// Formats one second's line, or None when nothing happened in it.
fn line(at: Duration, section: &str, views: u64, frames: u64, kinds: &BTreeMap<String, u64>) -> Option<String> {
    let updates: u64 = kinds.values().sum();
    if updates == 0 && views == 0 && frames == 0 {
        return None;
    }
    let mut by_kind: Vec<(&String, &u64)> = kinds.iter().collect();
    by_kind.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let kinds = by_kind.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(",");
    Some(format!("frame-stats t={} section={section} updates={updates} views={views} frames={frames} kinds={kinds}", at.as_secs()))
}

fn report_every_second() {
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(s) = stats() else { return };
        let kinds = s.kinds.lock().map(|mut k| std::mem::take(&mut *k)).unwrap_or_default();
        let section = s.section.lock().map(|s| *s).unwrap_or("");
        let (views, frames) = (s.views.swap(0, Ordering::Relaxed), s.frames.swap(0, Ordering::Relaxed));
        let Some(line) = line(s.started.elapsed(), section, views, frames, &kinds) else { continue };
        match &s.sink {
            Sink::Stderr => eprintln!("{line}"),
            Sink::File(path) => {
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    #[derive(Debug)]
    enum Inner {
        Refresh,
        Ide(Deep),
        Rows(Vec<u32>),
    }
    #[allow(dead_code)]
    #[derive(Debug)]
    enum Deep {
        ConfirmTick,
        Typed(String),
    }
    #[allow(dead_code)]
    #[derive(Debug)]
    enum Top {
        Tick,
        Fleet(Inner),
        Pair(u8, u8),
        Named { at: u8 },
    }

    #[test]
    fn a_kind_is_the_variant_names_without_their_payload() {
        assert_eq!(kind(&Top::Tick), "Tick");
        assert_eq!(kind(&Top::Fleet(Inner::Refresh)), "Fleet(Refresh)");
        assert_eq!(kind(&Top::Fleet(Inner::Ide(Deep::ConfirmTick))), "Fleet(Ide(ConfirmTick))");
        assert_eq!(kind(&Top::Fleet(Inner::Ide(Deep::Typed("a long (text)".into())))), "Fleet(Ide(Typed))");
        assert_eq!(kind(&Top::Fleet(Inner::Rows(vec![1, 2, 3]))), "Fleet(Rows)");
        assert_eq!(kind(&Top::Pair(1, 2)), "Pair(1)");
        assert_eq!(kind(&Top::Named { at: 1 }), "Named");
    }

    #[test]
    fn a_quiet_second_writes_no_line_and_a_busy_one_lists_its_kinds_most_first() {
        assert_eq!(line(Duration::from_secs(3), "overview", 0, 0, &BTreeMap::new()), None);
        let kinds = BTreeMap::from([("Look".to_string(), 1), ("Tick".to_string(), 30)]);
        assert_eq!(
            line(Duration::from_secs(4), "fleet", 31, 31, &kinds).unwrap(),
            "frame-stats t=4 section=fleet updates=31 views=31 frames=31 kinds=Tick:30,Look:1"
        );
    }
}
