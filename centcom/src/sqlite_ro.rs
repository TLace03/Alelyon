//! Reading the project's SQLite stores without changing a byte of them.
//!
//! A database in WAL mode keeps its newest commits in a `-wal` file beside it, and its readers share an index of
//! that file, `-shm`. Opening such a database read-only when neither file exists makes SQLite create both, and a
//! read-only connection cannot remove them again (found 2026-10-04; other tools
//! work around the same trap). So each read is planned from the file's
//! own header and the files beside it:
//!
//! - WAL mode with its `-wal` and `-shm` present (a program has it open): opened read-only, as one more reader of
//!   that WAL, so every commit is seen and nothing new is created;
//! - WAL mode with no `-wal` (nobody has it open, and every commit is in the main file): opened `immutable`, with
//!   no lock and no side file; the file's size and time are compared before and after, and a read during which
//!   it changed is read again rather than shown;
//! - WAL mode with a `-wal` but no `-shm`: a crash left it, and only its own program should open it, so it is
//!   refused by name;
//! - a rollback journal: opened read-only, which creates nothing. A reader holds a shared lock for as long as
//!   its read lasts, which makes a writer wait, so every read here is short and bounded.
//!
//! Every connection is `query_only` and distrusts the schema's own functions, so not even a mistaken statement
//! can write.
//!
//! One race remains, and the fleet's own queue view (`tools/fleet_view.py`, on every tool call of every session)
//! runs the same one: if the last program with a WAL database open closes it between the plan and the open, SQLite
//! removes its `-wal` and `-shm`, and the read-only open makes them again, empty, and cannot remove them. They stay
//! until a program next writes the database and closes it, which removes them; nothing in the database changes.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OpenFlags};

/// How a database was opened for a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// WAL mode, read through the `-wal` a running program keeps.
    SharedWal,
    /// WAL mode with nobody writing: the main file alone, without locks.
    Immutable,
    /// A rollback-journal database, read-only.
    ReadOnly,
}

impl Access {
    pub fn words(self) -> &'static str {
        match self {
            Access::SharedWal => "read beside the program that has it open",
            Access::Immutable => "read as it lies (nobody has it open)",
            Access::ReadOnly => "read-only",
        }
    }
}

/// The journal a database declares in its header (bytes 18 and 19 of the file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Journal {
    Rollback,
    Wal,
}

/// A file beside the database: `<db>-wal`, `<db>-shm` or `<db>-journal`.
pub fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The journal `path` declares. An empty file is a database nobody has written yet.
pub fn journal(path: &Path) -> Result<Option<Journal>, String> {
    let mut file = File::open(path).map_err(|e| format!("it cannot be opened: {e}"))?;
    let mut head = [0u8; 100];
    let mut got = 0;
    while got < head.len() {
        match file.read(&mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(format!("its header cannot be read: {e}")),
        }
    }
    if got == 0 {
        return Ok(None);
    }
    if got < head.len() || &head[..16] != b"SQLite format 3\0" {
        return Err("it is not an SQLite database".into());
    }
    match (head[18], head[19]) {
        (1, 1) => Ok(Some(Journal::Rollback)),
        (2, 2) => Ok(Some(Journal::Wal)),
        (w, r) => Err(format!("its header names a journal this window does not know (versions {w} and {r})")),
    }
}

/// How `path` can be read without changing anything on disk, or why it cannot be.
pub fn plan(path: &Path) -> Result<Access, String> {
    let journal = journal(path)?;
    let wal = beside(path, "-wal").exists();
    let shm = beside(path, "-shm").exists();
    match journal {
        Some(Journal::Wal) if wal && shm => Ok(Access::SharedWal),
        Some(Journal::Wal) if wal => Err("its -wal file has no -shm beside it, as a crash leaves them; only the program that \
                                          owns it should open it now"
            .into()),
        Some(Journal::Wal) => Ok(Access::Immutable),
        Some(Journal::Rollback) | None => Ok(Access::ReadOnly),
    }
}

/// The file's size and time: what an immutable read compares before and after.
fn stamp(path: &Path) -> Result<(u64, Option<SystemTime>), String> {
    let meta = fs::metadata(path).map_err(|e| format!("it cannot be found: {e}"))?;
    Ok((meta.len(), meta.modified().ok()))
}

/// `path` as an SQLite URI with `query`, its reserved characters escaped.
pub fn uri(path: &Path, query: &str) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:");
    if text.as_bytes().get(1) == Some(&b':') {
        out.push_str("///");
    } else if text.starts_with("//") {
        out.push_str("//");
    } else if text.starts_with('/') {
        out.push_str("//");
    }
    for ch in text.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            ' ' => out.push_str("%20"),
            c if c.is_control() => out.push_str(&format!("%{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('?');
    out.push_str(query);
    out
}

/// Open `path` as `access` plans: read-only, queries only.
fn open(path: &Path, access: Access) -> Result<Connection, String> {
    let query = match access {
        Access::Immutable => "mode=ro&immutable=1",
        Access::SharedWal | Access::ReadOnly => "mode=ro",
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(uri(path, query), flags).map_err(|e| words(&e))?;
    conn.busy_timeout(Duration::from_secs(2)).map_err(|e| words(&e))?;
    conn.execute_batch("PRAGMA query_only = ON; PRAGMA trusted_schema = OFF;").map_err(|e| words(&e))?;
    Ok(conn)
}

/// An SQLite error in a sentence.
pub fn words(e: &rusqlite::Error) -> String {
    use rusqlite::ErrorCode;
    match e.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            "another program is writing it just now; it will be read again at the next refresh".into()
        }
        Some(ErrorCode::NotADatabase) => "it is not an SQLite database".into(),
        Some(ErrorCode::DatabaseCorrupt) => "SQLite reports it damaged".into(),
        Some(ErrorCode::CannotOpen) => "it cannot be opened".into(),
        Some(ErrorCode::ReadOnly) => "an interrupted write must be finished by the program that owns it first".into(),
        _ => format!("SQLite: {e}"),
    }
}

/// Read `path` with `read`, opened so that nothing on disk changes, and say how it was opened. `read` may run
/// twice: an immutable read during which the file changed is read again, as the file is then.
pub fn read<T>(path: &Path, mut query: impl FnMut(&Connection) -> rusqlite::Result<T>) -> Result<(T, Access), String> {
    for _ in 0..2 {
        let access = plan(path)?;
        let before = stamp(path)?;
        let conn = open(path, access)?;
        let out = query(&conn).map_err(|e| words(&e));
        conn.close().map_err(|(_, e)| words(&e))?;
        let out = out?;
        if access != Access::Immutable || stamp(path)? == before {
            return Ok((out, access));
        }
    }
    Err("it changed while it was being read, twice running; it will be read again at the next refresh".into())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for one test, under the system's temporary folder.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("centcom-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    fn make(path: &Path, wal: bool) {
        let conn = Connection::open(path).unwrap();
        if wal {
            conn.pragma_update(None, "journal_mode", "wal").unwrap();
        }
        conn.execute_batch("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (1), (2), (3);").unwrap();
        conn.close().unwrap();
    }

    fn count(conn: &Connection) -> rusqlite::Result<i64> {
        conn.query_row("SELECT count(*) FROM t", [], |r| r.get(0))
    }

    #[test]
    fn a_wal_database_nobody_has_open_is_read_without_making_a_side_file() {
        let dir = scratch("wal-closed");
        let db = dir.join("a.db");
        make(&db, true);
        assert_eq!(journal(&db).unwrap(), Some(Journal::Wal));
        assert_eq!(names(&dir), ["a.db"], "a clean close leaves no -wal or -shm");
        let (n, access) = read(&db, count).unwrap();
        assert_eq!((n, access), (3, Access::Immutable));
        assert_eq!(names(&dir), ["a.db"], "the read made no side file");
        // A plain read-only open (the trap) would have made both: shown here so the test proves its own point.
        let trap = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        count(&trap).unwrap();
        let during = names(&dir);
        drop(trap);
        assert!(during.iter().any(|n| n.ends_with("-shm")) || names(&dir).iter().any(|n| n.ends_with("-shm")), "{during:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wal_database_a_program_has_open_is_read_through_its_wal_and_sees_every_commit() {
        let dir = scratch("wal-open");
        let db = dir.join("b.db");
        make(&db, true);
        let writer = Connection::open(&db).unwrap();
        writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        writer.execute("INSERT INTO t VALUES (4)", []).unwrap();
        let before = names(&dir);
        assert!(before.contains(&"b.db-wal".to_string()) && before.contains(&"b.db-shm".to_string()), "{before:?}");
        let (n, access) = read(&db, count).unwrap();
        assert_eq!((n, access), (4, Access::SharedWal), "the commit still in the WAL is seen");
        assert_eq!(names(&dir), before, "nothing new beside it");
        drop(writer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rollback_database_is_read_only_and_nothing_can_write_through_the_read() {
        let dir = scratch("rollback");
        let db = dir.join("c.db");
        make(&db, false);
        assert_eq!(journal(&db).unwrap(), Some(Journal::Rollback));
        let (n, access) = read(&db, count).unwrap();
        assert_eq!((n, access), (3, Access::ReadOnly));
        let wrote = read(&db, |c| c.execute("INSERT INTO t VALUES (9)", []));
        assert!(wrote.is_err(), "a write through a read is refused");
        assert_eq!(read(&db, count).unwrap().0, 3);
        assert_eq!(names(&dir), ["c.db"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wal_without_its_index_and_a_file_that_is_not_a_database_are_refused_by_name() {
        let dir = scratch("refused");
        let db = dir.join("d.db");
        make(&db, true);
        fs::write(beside(&db, "-wal"), b"").unwrap();
        assert!(plan(&db).unwrap_err().contains("no -shm"));
        let not = dir.join("not.db");
        fs::write(&not, b"hello, this is text and not a database, long enough to fill a header of one hundred bytes......").unwrap();
        assert_eq!(read(&not, count).unwrap_err(), "it is not an SQLite database");
        let empty = dir.join("empty.db");
        fs::write(&empty, b"").unwrap();
        assert_eq!(journal(&empty).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_becomes_a_uri_with_its_reserved_characters_escaped() {
        assert_eq!(uri(Path::new(r"D:\Projects\x\a b#1?.db"), "mode=ro"), "file:///D:/Projects/x/a%20b%231%3f.db?mode=ro");
        assert_eq!(uri(Path::new("/tmp/50%.db"), "mode=ro&immutable=1"), "file:///tmp/50%25.db?mode=ro&immutable=1");
    }
}
