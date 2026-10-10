//! Live data: each page that shows a store, a file or a folder is told when it changes, so it never needs a Check again.
//!
//! The rule (2026-10-08): no "check again" buttons anywhere if everything is properly streamed from the databases.
//! A page asks for a watch over its sources while it is on show; the watch looks at them on a thread of its own and
//! tells the window only when something changed (iced draws the whole window after every message, so a look that
//! finds nothing new makes no message and no frame). The page then reads again just what it shows.
//!
//! A look is cheap and leaves the disk as it was (`sqlite_ro`'s promise): nothing is opened through SQLite, so no
//! side file is made, no lock is taken and no connection is held.
//!
//! - **An SQLite database**: its header's file change counter (bytes 24-27, moved by every commit in rollback mode),
//!   the main file's size and time, the `-wal`'s size and time, and the first copy of the WAL index's header (the first
//!   48 bytes of `-shm`, whose change counter every commit in WAL mode moves; <https://www.sqlite.org/walformat.html>).
//!   `PRAGMA data_version` says the same thing but only to a connection held open between looks, and a read-only
//!   connection held open keeps a `-wal` and `-shm` on disk after their program closes (`sqlite_ro`'s trap), so it
//!   is used only by the tests, as the reference the header is checked against.
//! - **A file**: its size and time.
//! - **A folder**: on Windows, the system's own change notification (`FindFirstChangeNotificationW`, with the subtree
//!   when asked), which costs nothing until something changes; elsewhere, its entries' names, sizes and times.
//!
//! Local services are watched by `probe::watch` and Sinai's loop speaks on its own socket; the identity service and
//! friends are asked on a slow timer whose answer makes a message only when it differs (the service pushes nothing yet).

use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use iced::widget::row;
use iced::{Alignment, Element, Subscription};
use crate::theme;

/// How often a watch looks, unless it is told otherwise: a look is a few metadata reads.
pub const EVERY: Duration = Duration::from_millis(500);
/// The shortest time between two changes said to the window: a burst of commits makes one read, not many.
pub const GAP: Duration = Duration::from_secs(1);

/// What a watch looks at.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// An SQLite database (see the module's notes).
    Sqlite(PathBuf),
    /// One file: its size and time.
    File(PathBuf),
    /// A folder, and everything under it when `subtree`.
    Folder { path: PathBuf, subtree: bool },
}

impl Source {
    pub fn path(&self) -> &Path {
        match self {
            Source::Sqlite(p) | Source::File(p) | Source::Folder { path: p, .. } => p,
        }
    }
}

/// A source as one look found it: a fingerprint that changes whenever its content does, or why it cannot be looked at.
pub type Look = Result<Vec<u64>, String>;

fn stamp(meta: &std::fs::Metadata) -> [u64; 2] {
    let nanos = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0);
    [meta.len(), nanos]
}

/// The first `n` bytes of `path` (fewer when it is shorter), read without a lock: SQLite's locks lie elsewhere in the
/// file (the main file's at 1 GiB, the index's from byte 120).
fn head(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut out = vec![0u8; n];
    let mut got = 0;
    while got < n {
        match file.read(&mut out[got..])? {
            0 => break,
            k => got += k,
        }
    }
    out.truncate(got);
    Ok(out)
}

fn hash_of(bytes: &[u8]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// One look at `source`.
pub fn look(source: &Source) -> Look {
    match source {
        Source::Sqlite(path) => {
            let meta = std::fs::metadata(path).map_err(|_| "not on this PC".to_string())?;
            if !meta.is_file() {
                return Err("not a file".into());
            }
            let mut out = stamp(&meta).to_vec();
            let header = head(path, 100).map_err(|e| format!("it cannot be read: {e}"))?;
            if !header.is_empty() && (header.len() < 100 || &header[..16] != b"SQLite format 3\0") {
                return Err("it is not an SQLite database".into());
            }
            out.push(header.get(24..28).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64).unwrap_or(0));
            let wal = crate::sqlite_ro::beside(path, "-wal");
            out.extend(std::fs::metadata(&wal).map(|m| stamp(&m)).unwrap_or([u64::MAX, 0]));
            let shm = crate::sqlite_ro::beside(path, "-shm");
            out.push(head(&shm, 48).map(|b| hash_of(&b)).unwrap_or(u64::MAX));
            Ok(out)
        }
        Source::File(path) => {
            let meta = std::fs::metadata(path).map_err(|_| "not on this PC".to_string())?;
            Ok(stamp(&meta).to_vec())
        }
        Source::Folder { path, .. } => listing(path),
    }
}

/// A folder's entries (names, sizes, times), as one number; not what is under them.
fn listing(path: &Path) -> Look {
    let entries = std::fs::read_dir(path).map_err(|_| "not on this PC".to_string())?;
    let mut seen: Vec<(String, [u64; 2])> = entries
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.metadata().map(|m| stamp(&m)).unwrap_or([0, 0])))
        .collect();
    seen.sort();
    let mut h = std::hash::DefaultHasher::new();
    seen.hash(&mut h);
    Ok(vec![h.finish()])
}

/// What a watch tells the window: when it looked, and the sources it could not look at (by name, with why).
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub at: f64,
    /// The watch's first word: what the page already read on opening, so it need not read again.
    pub first: bool,
    pub missing: Vec<(String, String)>,
    /// How many sources the watch has, so "all of them are missing" can be told from "some are".
    pub of: usize,
}

/// Decides what a watch says: the first look, then a look that differs from the last one seen, at most once a `gap`
/// (a change inside the gap is said when it ends, so the last change is never lost).
#[derive(Debug, Default)]
pub struct Tracker {
    latest: Option<Vec<Look>>,
    owed: bool,
    said_at: Option<Instant>,
}

impl Tracker {
    /// Whether to tell the window now, after a look that found `looks` at `now`.
    pub fn see(&mut self, looks: Vec<Look>, now: Instant, gap: Duration) -> bool {
        if self.latest.as_ref() != Some(&looks) {
            self.latest = Some(looks);
            self.owed = true;
        }
        if self.owed && self.said_at.is_none_or(|t| now.duration_since(t) >= gap) {
            self.owed = false;
            self.said_at = Some(now);
            return true;
        }
        false
    }

    /// The first word has been said.
    pub fn said_once(&self) -> bool {
        self.said_at.is_some()
    }
}

/// A watch over named sources, known to iced by `key` and its sources: a page that changes what it watches (another
/// database opened) gets a new watch, and the old thread ends.
#[derive(Clone, Debug)]
pub struct Watch {
    pub key: &'static str,
    pub sources: Vec<(String, Source)>,
    pub every: Duration,
    pub gap: Duration,
}

impl Hash for Watch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.sources.hash(state);
        self.every.hash(state);
        self.gap.hash(state);
    }
}

impl Watch {
    pub fn new(key: &'static str, sources: Vec<(String, Source)>) -> Watch {
        Watch { key, sources, every: EVERY, gap: GAP }
    }

    pub fn gap(mut self, gap: Duration) -> Watch {
        self.gap = gap;
        self
    }

    /// The watch as a subscription: it runs while the page holds it.
    pub fn subscription(self) -> Subscription<Seen> {
        Subscription::run_with(self, watching)
    }
}

fn watching(watch: &Watch) -> impl futures::Stream<Item = Seen> + use<> {
    let watch = watch.clone();
    iced::stream::channel(4, async move |mut output: futures::channel::mpsc::Sender<Seen>| {
        let _ = std::thread::Builder::new().name(format!("centcom-live-{}", watch.key)).spawn(move || {
            // The window stopped listening (the page closed): the channel is closed and the thread ends.
            run(&watch, |seen| match seen {
                None => !output.is_closed(),
                Some(seen) => futures::executor::block_on(futures::SinkExt::send(&mut output, seen)).is_ok(),
            });
        });
    })
}

/// The watch's loop. `tell(Some(seen))` says a change and `tell(None)` asks whether the window still listens; either
/// answering false ends it. A folder on Windows wakes it as soon as it changes.
pub fn run(watch: &Watch, mut tell: impl FnMut(Option<Seen>) -> bool) {
    let mut tracker = Tracker::default();
    let mut notes = Notes::new(&watch.sources);
    while tell(None) {
        let looks: Vec<Look> = watch.sources.iter().enumerate().map(|(i, (_, s))| notes.look(i, s)).collect();
        let first = !tracker.said_once();
        if tracker.see(looks.clone(), Instant::now(), watch.gap) {
            let missing = watch
                .sources
                .iter()
                .zip(&looks)
                .filter_map(|((name, _), l)| l.as_ref().err().map(|why| (name.clone(), why.clone())))
                .collect();
            if !tell(Some(Seen { at: crate::utc::now(), first, missing, of: watch.sources.len() })) {
                return;
            }
        }
        notes.wait(watch.every);
    }
}

// ------------------------------------------------------------------- a process this window started

/// A child process this window started, followed until it ends: its exit in words, said once, as `(pid, how)`. It is
/// asked once a second on the subscription's own thread, so a process that runs on sends the window nothing (a
/// one-second timer redrew the whole window every second for it).
pub fn ended(child: std::sync::Arc<std::sync::Mutex<std::process::Child>>) -> Subscription<(u32, String)> {
    let pid = child.lock().unwrap_or_else(|p| p.into_inner()).id();
    Subscription::run_with(Followed { pid, child }, following)
}

struct Followed {
    pid: u32,
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
}

impl Hash for Followed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "live-ended".hash(state);
        self.pid.hash(state);
    }
}

fn following(followed: &Followed) -> impl futures::Stream<Item = (u32, String)> + use<> {
    let (pid, child) = (followed.pid, followed.child.clone());
    iced::stream::channel(1, async move |mut output: futures::channel::mpsc::Sender<(u32, String)>| {
        let _ = std::thread::Builder::new().name("centcom-live-process".into()).spawn(move || {
            while !output.is_closed() {
                std::thread::sleep(Duration::from_secs(1));
                let how = match child.lock().unwrap_or_else(|p| p.into_inner()).try_wait() {
                    Ok(Some(status)) => Some(status.to_string()),
                    Ok(None) => None,
                    Err(e) => Some(e.to_string()),
                };
                if let Some(how) = how {
                    let _ = futures::executor::block_on(futures::SinkExt::send(&mut output, (pid, how)));
                    return;
                }
            }
        });
    })
}

// ------------------------------------------------------------------- folders: the system's change notification

#[cfg(windows)]
struct Notes {
    /// For each source: its notification handle (folders only, once the folder exists) and how often it fired.
    handles: Vec<Option<windows::Win32::Foundation::HANDLE>>,
    counts: Vec<u64>,
    folders: Vec<Option<(PathBuf, bool)>>,
}

#[cfg(windows)]
impl Notes {
    fn new(sources: &[(String, Source)]) -> Notes {
        let folders: Vec<Option<(PathBuf, bool)>> = sources
            .iter()
            .map(|(_, s)| match s {
                Source::Folder { path, subtree } => Some((path.clone(), *subtree)),
                _ => None,
            })
            .collect();
        let mut notes = Notes { handles: vec![None; sources.len()], counts: vec![0; sources.len()], folders };
        notes.arm();
        notes
    }

    /// Ask for a notification on every folder that exists and has none yet.
    fn arm(&mut self) {
        use windows::Win32::Storage::FileSystem::{
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FindFirstChangeNotificationW,
        };
        use windows::core::HSTRING;
        for (i, folder) in self.folders.iter().enumerate() {
            let Some((path, subtree)) = folder else { continue };
            if self.handles[i].is_some() || !path.is_dir() {
                continue;
            }
            let filter =
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE;
            // SAFETY: a path the call copies; the handle is closed in `drop` or when it fails.
            if let Ok(h) = unsafe { FindFirstChangeNotificationW(&HSTRING::from(path.as_os_str()), *subtree, filter) } {
                self.handles[i] = Some(h);
            }
        }
    }

    fn look(&mut self, i: usize, source: &Source) -> Look {
        match source {
            Source::Folder { path, .. } => {
                if !path.is_dir() {
                    return Err("not on this PC".into());
                }
                Ok(vec![self.counts[i]])
            }
            other => look(other),
        }
    }

    /// Wait up to `every`, or until a folder changes (then a moment more, so a burst of writes is one change).
    fn wait(&mut self, every: Duration) {
        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
        use windows::Win32::Storage::FileSystem::{FindCloseChangeNotification, FindNextChangeNotification};
        use windows::Win32::System::Threading::WaitForMultipleObjects;
        self.arm();
        let armed: Vec<(usize, HANDLE)> = self.handles.iter().enumerate().filter_map(|(i, h)| h.map(|h| (i, h))).collect();
        if armed.is_empty() {
            std::thread::sleep(every);
            return;
        }
        let list: Vec<HANDLE> = armed.iter().map(|(_, h)| *h).collect();
        let mut timeout = every.as_millis().min(u32::MAX as u128) as u32;
        loop {
            // SAFETY: handles this struct owns and keeps open for the call.
            let woke = unsafe { WaitForMultipleObjects(&list, false, timeout) };
            let at = woke.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if at >= list.len() {
                return;
            }
            let (i, h) = armed[at];
            self.counts[i] += 1;
            // SAFETY: as above; a handle that cannot be re-armed (its folder went) is closed and asked for again later.
            if unsafe { FindNextChangeNotification(h) }.is_err() {
                let _ = unsafe { FindCloseChangeNotification(h) };
                self.handles[i] = None;
                return;
            }
            // Gather what else changed in the same moment, then look.
            timeout = 50;
        }
    }
}

#[cfg(windows)]
impl Drop for Notes {
    fn drop(&mut self) {
        use windows::Win32::Storage::FileSystem::FindCloseChangeNotification;
        for h in self.handles.iter().flatten() {
            // SAFETY: each handle was returned by FindFirstChangeNotificationW and is closed once.
            let _ = unsafe { FindCloseChangeNotification(*h) };
        }
    }
}

#[cfg(not(windows))]
struct Notes;

#[cfg(not(windows))]
impl Notes {
    fn new(_sources: &[(String, Source)]) -> Notes {
        Notes
    }

    fn look(&mut self, _i: usize, source: &Source) -> Look {
        look(source)
    }

    fn wait(&mut self, every: Duration) {
        std::thread::sleep(every);
    }
}

// ------------------------------------------------------------------- freshness, as each page shows it

/// How a live view stands: whether its sources can be watched, and when it last heard of a change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Freshness {
    /// Nothing heard yet: the watch has not made its first look.
    pub waiting: bool,
    /// When the view's data last changed (or was first read, before any change).
    pub changed: Option<f64>,
    /// Sources that cannot be looked at, by name, with why.
    pub missing: Vec<(String, String)>,
    /// Every source is missing.
    pub unavailable: bool,
}

impl Freshness {
    pub fn waiting() -> Freshness {
        Freshness { waiting: true, ..Freshness::default() }
    }

    /// What the watch said.
    pub fn saw(&mut self, seen: &Seen) {
        self.waiting = false;
        self.changed = Some(seen.at);
        self.unavailable = seen.of > 0 && seen.missing.len() == seen.of;
        self.missing = seen.missing.clone();
    }

    /// A source that is not a file (a service, the identity service) changed, or could not be reached.
    pub fn heard(&mut self, at: f64, problem: Option<String>) {
        self.waiting = false;
        self.changed = Some(at);
        self.unavailable = problem.is_some();
        self.missing = problem.map(|why| vec![(String::new(), why)]).unwrap_or_default();
    }

    /// The indicator's words. The time is a clock time, not "2 min ago": a relative time would need a clock ticking
    /// in an idle window, which would draw it again and again for nothing.
    pub fn words(&self, now: f64) -> String {
        if self.waiting {
            return "Connecting to the source…".into();
        }
        let when = self.changed.map(|at| crate::utc::when(at, now));
        if self.unavailable {
            let why = self.missing.first().map(|(_, why)| why.as_str()).unwrap_or("it cannot be read");
            return format!("Source unavailable: {why}");
        }
        let mut words = match when {
            Some(when) => format!("Live · last change {when}"),
            None => "Live".to_string(),
        };
        if !self.missing.is_empty() {
            let names: Vec<&str> = self.missing.iter().map(|(n, _)| n.as_str()).filter(|n| !n.is_empty()).collect();
            if names.len() <= 2 {
                words.push_str(&format!(" · not on this PC: {}", names.join(", ")));
            } else {
                words.push_str(&format!(" · {} of its sources are not on this PC", names.len()));
            }
        }
        words
    }

    pub fn color(&self) -> iced::Color {
        if self.waiting || self.unavailable {
            theme::TEXT_FAINT
        } else if self.missing.is_empty() {
            theme::POSITIVE
        } else {
            theme::CAUTION
        }
    }
}

/// The small live indicator: a dot and its words.
pub fn badge<'a, M: 'a>(freshness: &Freshness) -> Element<'a, M> {
    row![crate::ui::dot(freshness.color()), crate::ui::label(freshness.words(crate::utc::now()), 11.5, theme::TEXT_FAINT)]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_ro::tests::scratch;
    use rusqlite::Connection;

    fn wal_db(path: &Path) -> Connection {
        let c = Connection::open(path).unwrap();
        c.pragma_update(None, "journal_mode", "wal").unwrap();
        c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        c.execute_batch("CREATE TABLE t (n INTEGER);").unwrap();
        c
    }

    fn data_version(c: &Connection) -> i64 {
        c.query_row("PRAGMA data_version", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_commit_in_wal_mode_changes_the_look_as_data_version_does_and_a_read_does_not() {
        let dir = scratch("live-wal");
        let db = dir.join("a.db");
        let writer = wal_db(&db);
        let reader = Connection::open(&db).unwrap();
        let source = Source::Sqlite(db.clone());
        let mut before = (look(&source).unwrap(), data_version(&reader));
        for n in 0..20 {
            // a read (ours, through sqlite_ro, or another reader's) changes nothing
            let _ = crate::sqlite_ro::read(&db, |c| c.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))).unwrap();
            let _: i64 = reader.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
            assert_eq!(look(&source).unwrap(), before.0, "a read moved the look");
            writer.execute("INSERT INTO t VALUES (?1)", [n]).unwrap();
            let after = (look(&source).unwrap(), data_version(&reader));
            assert_ne!(after.1, before.1, "the reference: data_version moved for commit {n}");
            assert_ne!(after.0, before.0, "the look moved with it for commit {n}");
            before = after;
        }
        // a checkpoint that resets the WAL to its start keeps its size; the index header still moves
        writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        let mid = look(&source).unwrap();
        writer.execute("INSERT INTO t VALUES (99)", []).unwrap();
        assert_ne!(look(&source).unwrap(), mid);
        drop((writer, reader));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_commit_in_rollback_mode_changes_the_look_and_a_missing_database_says_why() {
        let dir = scratch("live-rollback");
        let db = dir.join("b.db");
        let c = Connection::open(&db).unwrap();
        c.execute_batch("CREATE TABLE t (n INTEGER);").unwrap();
        let source = Source::Sqlite(db.clone());
        let a = look(&source).unwrap();
        assert_eq!(look(&source).unwrap(), a, "nothing changed, nothing moves");
        c.execute("INSERT INTO t VALUES (1)", []).unwrap();
        assert_ne!(look(&source).unwrap(), a);
        assert_eq!(look(&Source::Sqlite(dir.join("none.db"))).unwrap_err(), "not on this PC");
        std::fs::write(dir.join("text.db"), vec![b'x'; 200]).unwrap();
        assert_eq!(look(&Source::Sqlite(dir.join("text.db"))).unwrap_err(), "it is not an SQLite database");
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_is_said_once_nothing_new_is_not_and_a_burst_is_said_after_the_gap() {
        let mut t = Tracker::default();
        let start = Instant::now();
        let gap = Duration::from_secs(1);
        let a = vec![Ok(vec![1])];
        let b = vec![Ok(vec![2])];
        let c = vec![Err("not on this PC".to_string())];
        assert!(t.see(a.clone(), start, gap), "the first look is said");
        assert!(!t.see(a.clone(), start + Duration::from_millis(500), gap), "the same is not");
        assert!(!t.see(a.clone(), start + Duration::from_secs(5), gap), "nor later");
        assert!(t.see(b.clone(), start + Duration::from_secs(6), gap), "a change is said once");
        assert!(!t.see(b.clone(), start + Duration::from_secs(7), gap));
        // two changes inside the gap: the first is held, then said when the gap ends
        assert!(t.see(c.clone(), start + Duration::from_millis(7100), gap), "a source going missing is a change");
        assert!(!t.see(a.clone(), start + Duration::from_millis(7400), gap), "inside the gap: held");
        assert!(!t.see(a.clone(), start + Duration::from_millis(7900), gap), "still inside it");
        assert!(t.see(a.clone(), start + Duration::from_millis(8200), gap), "said when the gap ends");
        assert!(!t.see(a, start + Duration::from_secs(20), gap), "and only once");
    }

    /// The watch's own loop over real files: `act` after its first look, then its words gathered for `wait`.
    fn run_until(watch: Watch, act: impl FnOnce() + Send + 'static, wait: Duration) -> Vec<Seen> {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            run(&watch, |seen| {
                if let Some(seen) = seen {
                    let _ = tx.send(seen);
                }
                !stopping.load(std::sync::atomic::Ordering::SeqCst)
            })
        });
        std::thread::sleep(Duration::from_millis(300));
        act();
        std::thread::sleep(wait);
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let got: Vec<Seen> = rx.try_iter().collect();
        drop(thread);
        got
    }

    #[test]
    fn a_watch_says_its_first_look_then_exactly_one_word_for_one_commit_and_none_without() {
        let dir = scratch("live-run-db");
        let db = dir.join("c.db");
        let writer = wal_db(&db);
        let quiet = run_until(
            Watch::new("test-quiet", vec![("store".into(), Source::Sqlite(db.clone()))]).gap(Duration::ZERO),
            || {},
            Duration::from_millis(600),
        );
        assert_eq!(quiet.len(), 1, "only the first look: {quiet:?}");
        assert!(quiet[0].first && quiet[0].missing.is_empty());
        let path = db.clone();
        let one = run_until(
            Watch::new("test-one", vec![("store".into(), Source::Sqlite(db.clone()))]).gap(Duration::ZERO),
            move || {
                let c = Connection::open(&path).unwrap();
                c.execute("INSERT INTO t VALUES (1)", []).unwrap();
            },
            Duration::from_millis(800),
        );
        assert_eq!(one.len(), 2, "the first look and one change: {one:?}");
        assert!(!one[1].first);
        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_written_in_a_watched_folder_is_said_and_a_missing_source_is_named() {
        let dir = scratch("live-run-folder");
        let watched = dir.join("watched");
        std::fs::create_dir_all(&watched).unwrap();
        let file = watched.join("a.json");
        let target = file.clone();
        let got = run_until(
            Watch::new(
                "test-folder",
                vec![
                    ("folder".into(), Source::Folder { path: watched.clone(), subtree: true }),
                    ("absent".into(), Source::File(dir.join("absent.json"))),
                ],
            )
            .gap(Duration::ZERO),
            move || std::fs::write(&target, b"{}").unwrap(),
            Duration::from_millis(1500),
        );
        assert!(got.len() >= 2, "the first look and the new file: {got:?}");
        assert_eq!(got[0].missing, vec![("absent".to_string(), "not on this PC".to_string())]);
        assert!(!got[1].first);
        // the folder itself gone: the watch says it is not there
        let mut f = Freshness::waiting();
        f.saw(&Seen { at: 0.0, first: true, missing: vec![("folder".into(), "not on this PC".into())], of: 1 });
        assert!(f.unavailable);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_pages_made_live_have_no_check_again_refresh_or_read_now() {
        let pages = [
            ("view.rs (Overview, Sinai, the library)", include_str!("view.rs")),
            ("data/view.rs", include_str!("data/view.rs")),
            ("signin/settings.rs", include_str!("signin/settings.rs")),
            ("lattice/ide/agent.rs (the chats list)", include_str!("lattice/ide/agent.rs")),
        ];
        let gone = ["\"Check again\"", "\"Look again\"", "\"Read now\"", "\"Refresh\"", "\"Read the list again\""];
        for (page, source) in pages {
            for words in gone {
                assert!(!source.contains(words), "{page} still offers {words}: its data is watched, not asked for");
            }
        }
        // what replaced them: each page shows its live mark (the Fleet page, a plug-in, holds itself to the same rule)
        for (page, source) in &pages[..3] {
            assert!(source.contains("live::badge("), "{page} shows no live mark");
        }
    }

    #[test]
    fn freshness_says_live_with_a_clock_time_partial_or_unavailable_with_why() {
        let day = 1_790_000_000.0;
        let mut f = Freshness::waiting();
        assert_eq!(f.words(day), "Connecting to the source…");
        f.saw(&Seen { at: day - 30.0, first: true, missing: vec![], of: 2 });
        assert_eq!(f.words(day), format!("Live · last change {}", crate::utc::stamp(day - 30.0)));
        assert_eq!(f.color(), theme::POSITIVE);
        f.saw(&Seen { at: day, first: false, missing: vec![("the relay".into(), "not on this PC".into())], of: 2 });
        assert!(f.words(day).ends_with("· not on this PC: the relay"), "{}", f.words(day));
        assert_eq!(f.color(), theme::CAUTION);
        f.saw(&Seen { at: day, first: false, missing: vec![("a".into(), "not on this PC".into()), ("b".into(), "x".into())], of: 2 });
        assert_eq!(f.words(day), "Source unavailable: not on this PC");
        f.heard(day, Some("the service could not be reached".into()));
        assert_eq!(f.words(day), "Source unavailable: the service could not be reached");
        f.heard(day - 86_400.0 * 2.0, None);
        assert!(f.words(day).starts_with("Live · last change ") && f.words(day).contains('-'), "an older change gives its day");
    }
}
