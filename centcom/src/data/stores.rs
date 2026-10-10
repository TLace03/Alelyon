//! The databases the Data page may open (a closed list), and the reads it makes of them.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, params_from_iter};

use crate::checkout::Places;
use crate::sqlite_ro::{self, Access};

/// One database the page may open.
pub struct Store {
    pub key: &'static str,
    pub title: &'static str,
    pub what: &'static str,
    /// Its rows are a person's own words: Sinai's conversations.
    pub private: bool,
    /// The file, under the main checkout or `~/.alelyon`.
    path: fn(&Places) -> Option<PathBuf>,
}

impl Store {
    pub fn path(&self, places: &Places) -> Option<PathBuf> {
        (self.path)(places)
    }
}

fn under_alelyon(places: &Places, parts: &[&str]) -> Option<PathBuf> {
    places.alelyon.as_ref().map(|base| parts.iter().fold(base.clone(), |p, part| p.join(part)))
}

/// Every database the page may open, (chosen 2026-10-04 and 2026-10-05). Markets data and the identity
/// stores are not here, and nothing outside this list can be opened.
pub const STORES: [Store; 8] = [
    Store {
        key: "bus",
        title: "The agent bus",
        what: "Every session's posts (findings and messages), who each reached, area claims, channels, and the worktrees the fleet has seen.",
        private: false,
        path: |p| Some(p.bus.clone()),
    },
    Store {
        key: "relay",
        title: "The PR relay and its receipts",
        what: "Each branch proposed for landing in rank order, the cross-checks between them, and every verification receipt with each check's outcome.",
        private: false,
        path: |p| Some(p.relay.clone()),
    },
    Store {
        key: "intents",
        title: "Session intents",
        what: "What sessions registered they were about to do, and where, before they began (tools/preflight.py).",
        private: false,
        path: |p| Some(p.repo.join("globals").join("fleet_intent.db")),
    },
    Store {
        key: "ledger",
        title: "The model ledger",
        what: "Each model's runs by layer and kind of work, and the standings drawn from them.",
        private: false,
        path: |p| Some(p.repo.join("globals").join("fleet_ledger.db")),
    },
    Store {
        key: "universal",
        title: "The universal ledger",
        what: "A hash-chained record of the documents and changes the fleet observed, entry by entry.",
        private: false,
        path: |p| Some(p.repo.join(".git").join("alelyon-universal-ledger.db")),
    },
    Store {
        key: "workspaces",
        title: "Lattice workspaces",
        what: "The Lattice service's workspaces and what its recorder kept of them.",
        private: false,
        path: |p| Some(p.repo.join(".git").join("alelyon-lattice-workspaces.db")),
    },
    Store {
        key: "sinai-memory",
        title: "Sinai's memory",
        what: "Sinai's recall memory: each exchange it keeps (what was said to it and what it answered), and what it recalls them by.",
        private: true,
        path: |p| under_alelyon(p, &["angel", "memory.sqlite3"]),
    },
    Store {
        key: "sinai-experience",
        title: "Sinai's experience",
        what: "Sinai's experience record: each event it lived, in order, by channel and turn, with what was erased.",
        private: true,
        path: |p| under_alelyon(p, &["angel", "experience.sqlite3"]),
    },
];

/// The store `--tab` names, by its key, with a table after a slash: (its place in `STORES`, the table).
pub fn named(name: &str) -> Option<(usize, Option<String>)> {
    let (key, table) = match name.split_once('/') {
        Some((key, table)) => (key, Some(table.to_string())),
        None => (name, None),
    };
    STORES.iter().position(|s| s.key == key).map(|i| (i, table))
}

/// A column whose name says it holds a secret is never read: shown as hidden, and never selected.
pub fn hidden(column: &str) -> bool {
    let c = column.to_lowercase();
    ["password", "passwd", "salt", "secret", "private_key", "api_key", "token_sha", "key_digest", "recovery", "credential"]
        .iter()
        .any(|w| c.contains(w))
        || c == "token"
        || c.ends_with("_token")
}

/// A store's file as the page lists it before opening anything.
#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    /// How it would be read, or why it cannot be.
    pub plan: Result<Access, String>,
}

pub fn shelf(places: &Places) -> Vec<Shelf> {
    STORES
        .iter()
        .map(|store| match store.path(places) {
            None => Shelf { size: None, modified: None, plan: Err("USERPROFILE is not set, so it has no place on this PC".into()) },
            Some(path) => match fs::metadata(&path) {
                Err(_) => Shelf { size: None, modified: None, plan: Err("not on this PC".into()) },
                Ok(meta) => Shelf { size: Some(meta.len()), modified: meta.modified().ok(), plan: sqlite_ro::plan(&path) },
            },
        })
        .collect()
}

/// A table: its name, its rows, and its columns (name and declared type).
#[derive(Clone, Debug, PartialEq)]
pub struct TableInfo {
    pub name: String,
    pub rows: i64,
    pub columns: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    pub access: Option<Access>,
    pub tables: Vec<TableInfo>,
}

/// `name` quoted as an SQL identifier.
pub fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quoted(table)))?;
    stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.collect()
}

/// A store's tables with their sizes, in one read.
pub fn overview(store: &Store, places: &Places) -> Result<Overview, String> {
    let path = store.path(places).ok_or("it has no place on this PC")?;
    let (tables, access) = sqlite_ro::read(&path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
             AND sql NOT LIKE 'CREATE VIRTUAL TABLE%' ORDER BY name",
        )?;
        let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut tables = Vec::new();
        for name in names {
            let rows = conn.query_row(&format!("SELECT count(*) FROM {}", quoted(&name)), [], |r| r.get(0))?;
            tables.push(TableInfo { columns: columns(conn, &name)?, name, rows });
        }
        Ok(tables)
    })?;
    Ok(Overview { access: Some(access), tables })
}

/// One cell as read: text is kept to its first `TEXT_KEPT` characters, and a blob only by its length.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String, bool),
    Blob(usize),
    Hidden,
}

pub const TEXT_KEPT: usize = 4000;

impl Cell {
    pub fn show(&self) -> String {
        match self {
            Cell::Null => "—".into(),
            Cell::Integer(n) => n.to_string(),
            Cell::Real(x) => x.to_string(),
            Cell::Text(s, _) => s.clone(),
            Cell::Blob(n) => format!("({n} bytes of binary data)"),
            Cell::Hidden => "(hidden: it may hold a secret)".into(),
        }
    }

    /// As `show`, but a time column's seconds since 1970 read as a date too: "2026-10-05 05:13:44Z (1791177224)".
    pub fn show_in(&self, column: &str) -> String {
        let seconds = match self {
            Cell::Integer(n) => *n as f64,
            Cell::Real(x) => *x,
            _ => return self.show(),
        };
        if timed(column) && (946_684_800.0..4_102_444_800.0).contains(&seconds) {
            format!("{} ({})", crate::utc::full(seconds), self.show())
        } else {
            self.show()
        }
    }
}

/// A column whose name says it holds a time: `at`, `ts`, `*_at`, `*_ts`, `created`, `updated`, `first_seen`,
/// `last_seen`, `born`. Only numbers between 2000 and 2100 in such a column are read as dates.
pub fn timed(column: &str) -> bool {
    let c = column.to_lowercase();
    matches!(c.as_str(), "at" | "ts" | "created" | "updated" | "first_seen" | "last_seen" | "born")
        || c.ends_with("_at")
        || c.ends_with("_ts")
}

/// A page of a table's rows, newest first where the table keeps its rows' order.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub table: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub offset: i64,
    /// The rows that match the filter, when there is one.
    pub matching: Option<i64>,
    /// Newest first (by the order rows were added), or as stored.
    pub newest_first: bool,
    pub access: Access,
}

/// `words` as a LIKE pattern that matches them anywhere, with LIKE's own characters taken literally.
pub fn like(words: &str) -> String {
    let mut out = String::from("%");
    for c in words.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// A page of `table` (a name the store's overview listed), `limit` rows from `offset`, keeping only rows where
/// some shown column contains `filter`.
pub fn page(store: &Store, places: &Places, table: &str, filter: &str, offset: i64, limit: i64) -> Result<Page, String> {
    let path = store.path(places).ok_or("it has no place on this PC")?;
    let ((columns, rows, matching, newest_first), access) = sqlite_ro::read(&path, |conn| {
        let known: bool =
            conn.query_row("SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get(0))?;
        if !known {
            return Err(rusqlite::Error::InvalidParameterName(format!("no table {table}")));
        }
        let all = columns(conn, table)?;
        let names: Vec<String> = all.iter().map(|(n, _)| n.clone()).collect();
        let shown: Vec<&String> = names.iter().filter(|n| !hidden(n)).collect();
        let select = if shown.is_empty() { "NULL".to_string() } else { shown.iter().map(|n| quoted(n)).collect::<Vec<_>>().join(", ") };
        let (filter_sql, args): (String, Vec<String>) = if filter.is_empty() || shown.is_empty() {
            (String::new(), Vec::new())
        } else {
            let any = shown.iter().map(|n| format!("CAST({} AS TEXT) LIKE ?1 ESCAPE '\\'", quoted(n))).collect::<Vec<_>>().join(" OR ");
            (format!(" WHERE {any}"), vec![like(filter)])
        };
        let has_rowid = conn.prepare(&format!("SELECT rowid FROM {} LIMIT 0", quoted(table))).is_ok();
        let order = if has_rowid { " ORDER BY rowid DESC" } else { "" };
        let sql = format!("SELECT {select} FROM {}{filter_sql}{order} LIMIT {limit} OFFSET {offset}", quoted(table));
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = Vec::new();
        let mut cursor = stmt.query(params_from_iter(args.iter()))?;
        while let Some(r) = cursor.next()? {
            let mut cells = Vec::with_capacity(names.len());
            let mut k = 0;
            for name in &names {
                if hidden(name) {
                    cells.push(Cell::Hidden);
                    continue;
                }
                cells.push(match r.get_ref(k)? {
                    ValueRef::Null => Cell::Null,
                    ValueRef::Integer(n) => Cell::Integer(n),
                    ValueRef::Real(x) => Cell::Real(x),
                    ValueRef::Text(t) => {
                        let s = String::from_utf8_lossy(t);
                        let cut = s.chars().count() > TEXT_KEPT;
                        Cell::Text(if cut { s.chars().take(TEXT_KEPT).collect() } else { s.into_owned() }, cut)
                    }
                    ValueRef::Blob(b) => Cell::Blob(b.len()),
                });
                k += 1;
            }
            rows.push(cells);
        }
        let matching = if args.is_empty() {
            None
        } else {
            Some(
                conn.query_row(&format!("SELECT count(*) FROM {}{filter_sql}", quoted(table)), params_from_iter(args.iter()), |r| {
                    r.get(0)
                })?,
            )
        };
        Ok((names, rows, matching, has_rowid))
    })?;
    Ok(Page { table: table.to_string(), columns, rows, offset, matching, newest_first, access })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_ro::tests::scratch;

    fn places(dir: &std::path::Path) -> Places {
        let mut p = Places::under(dir.to_path_buf());
        p.alelyon = Some(dir.join(".alelyon"));
        p
    }

    #[test]
    fn the_list_is_closed_and_keeps_markets_and_identity_out() {
        let keys: Vec<&str> = STORES.iter().map(|s| s.key).collect();
        assert_eq!(keys, ["bus", "relay", "intents", "ledger", "universal", "workspaces", "sinai-memory", "sinai-experience"]);
        let p = places(std::path::Path::new("C:/repo"));
        for store in &STORES {
            let path = store.path(&p).unwrap().to_string_lossy().to_lowercase();
            for out in [
                "fam.db",
                "history",
                "ticks",
                "newsdesk",
                "edgar",
                "model_journal",
                "accounts",
                "directory",
                "beta_keys",
                "access_requests",
                "entitlements",
                // any key folder, the signing keys' among them
                "_keys",
            ] {
                assert!(!path.contains(out), "{path} is {out}");
            }
        }
        assert_eq!(STORES.iter().filter(|s| s.private).map(|s| s.key).collect::<Vec<_>>(), ["sinai-memory", "sinai-experience"]);
    }

    #[test]
    fn a_column_that_may_hold_a_secret_is_never_read() {
        for name in ["password_hash", "salt", "token", "session_token", "token_sha", "key_digest", "recovery_codes", "api_key", "Secret"] {
            assert!(hidden(name), "{name}");
        }
        for name in ["out_tokens", "body", "at", "kind", "head", "digest"] {
            assert!(!hidden(name), "{name}");
        }
        let dir = scratch("data-hidden");
        let p = places(&dir);
        fs::create_dir_all(dir.join("globals")).unwrap();
        let db = p.repo.join("globals").join("fleet_intent.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE registration (id TEXT, actor TEXT, password_hash TEXT, note BLOB);
             INSERT INTO registration VALUES ('r1', 'a', 'SECRET-VALUE', x'00010203');
             INSERT INTO registration VALUES ('r2', 'b', 'SECRET-TWO', NULL);",
        )
        .unwrap();
        conn.close().unwrap();
        let store = STORES.iter().find(|s| s.key == "intents").unwrap();
        let o = overview(store, &p).unwrap();
        assert_eq!((o.tables[0].name.as_str(), o.tables[0].rows), ("registration", 2));
        let page = page(store, &p, "registration", "", 0, 50).unwrap();
        assert!(page.newest_first);
        assert_eq!(page.rows[0][0], Cell::Text("r2".into(), false), "newest first");
        assert_eq!(page.rows[0][2], Cell::Hidden);
        assert_eq!(page.rows[1][3], Cell::Blob(4));
        assert!(!format!("{page:?}").contains("SECRET"), "the secret was never read");
        // A filter cannot reach a hidden column either.
        let found = super::page(store, &p, "registration", "SECRET", 0, 50).unwrap();
        assert_eq!((found.rows.len(), found.matching), (0, Some(0)));
        let found = super::page(store, &p, "registration", "b", 0, 50).unwrap();
        assert_eq!((found.rows.len(), found.matching), (1, Some(1)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_time_column_reads_as_a_date_and_nothing_else_does() {
        assert_eq!(Cell::Integer(1791177224).show_in("at"), "2026-10-05 05:13:44Z (1791177224)");
        assert_eq!(Cell::Real(1791061711.5).show_in("released_at"), "2026-10-03 21:08:31Z (1791061711.5)");
        assert_eq!(Cell::Integer(1791177224).show_in("rows"), "1791177224");
        assert_eq!(Cell::Integer(5).show_in("at"), "5", "a small number is not a date");
        assert_eq!(Cell::Text("x".into(), false).show_in("at"), "x");
    }

    #[test]
    fn a_filter_is_taken_literally_and_a_table_must_exist() {
        assert_eq!(like("50%_a\\b"), "%50\\%\\_a\\\\b%");
        let dir = scratch("data-filter");
        let p = places(&dir);
        fs::create_dir_all(dir.join("globals")).unwrap();
        let conn = Connection::open(p.repo.join("globals").join("fleet_intent.db")).unwrap();
        conn.execute_batch("CREATE TABLE t (s TEXT); INSERT INTO t VALUES ('50% off'), ('500 off'), ('a_b'), ('axb');").unwrap();
        conn.close().unwrap();
        let store = STORES.iter().find(|s| s.key == "intents").unwrap();
        assert_eq!(page(store, &p, "t", "50%", 0, 50).unwrap().matching, Some(1));
        assert_eq!(page(store, &p, "t", "a_b", 0, 50).unwrap().matching, Some(1));
        assert!(page(store, &p, "t\" ; DROP TABLE t; --", "", 0, 50).is_err());
        assert_eq!(page(store, &p, "t", "", 2, 50).unwrap().rows.len(), 2, "paging by offset");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "reads this PC's own databases; run by hand"]
    fn every_store_on_this_pc_opens_without_side_files() {
        let places = Places::find().unwrap();
        for (store, shelf) in STORES.iter().zip(shelf(&places)) {
            let path = store.path(&places).unwrap();
            let before: Vec<bool> = ["-wal", "-shm", "-journal"].iter().map(|s| sqlite_ro::beside(&path, s).exists()).collect();
            let o = overview(store, &places);
            let after: Vec<bool> = ["-wal", "-shm", "-journal"].iter().map(|s| sqlite_ro::beside(&path, s).exists()).collect();
            println!(
                "{} {:?} {:?}",
                store.key,
                shelf.plan,
                o.as_ref().map(|o| o.tables.iter().map(|t| (&t.name, t.rows)).collect::<Vec<_>>())
            );
            assert_eq!(before, after, "{}: no side file appeared or vanished", store.key);
        }
    }
}
