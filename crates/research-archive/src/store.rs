//! The archive on disk: one SQLite file holding every paper gathered for every subject, the subjects themselves,
//! why and how each paper belongs to each, every run's outcome, and a full-text index over titles and abstracts.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Statement, params};

use std::collections::{HashMap, HashSet};

use crate::gaps;
use crate::graph::{self, Community, Role};
use crate::ident::Id;
use crate::index::Error;
use crate::snowball::{Found, Reason, Report, Snowball, Stop};
use crate::work::{Source, Work};

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS works (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    abstract TEXT,
    year INTEGER,
    authors TEXT NOT NULL,      -- JSON array of names
    venue TEXT,
    open_url TEXT,
    cited_by_count INTEGER
);
CREATE TABLE IF NOT EXISTS ids (
    scheme TEXT NOT NULL,
    value TEXT NOT NULL,
    work INTEGER NOT NULL REFERENCES works(id),
    PRIMARY KEY (scheme, value)
);
CREATE INDEX IF NOT EXISTS ids_by_work ON ids(work);
CREATE TABLE IF NOT EXISTS refs (
    work INTEGER NOT NULL REFERENCES works(id),
    scheme TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (work, scheme, value)
);
CREATE TABLE IF NOT EXISTS seen (
    work INTEGER NOT NULL REFERENCES works(id),
    source TEXT NOT NULL,
    PRIMARY KEY (work, source)
);
CREATE TABLE IF NOT EXISTS topics (
    name TEXT PRIMARY KEY,
    queries TEXT NOT NULL,      -- JSON array
    phrases TEXT NOT NULL,      -- JSON array, one concept per entry, spellings separated by |
    watch TEXT NOT NULL DEFAULT 'off',  -- off | daily | weekly
    watched_at INTEGER          -- Unix seconds of the subject's last run of either kind
);
CREATE TABLE IF NOT EXISTS members (
    topic TEXT NOT NULL REFERENCES topics(name) ON DELETE CASCADE,
    work INTEGER NOT NULL REFERENCES works(id),
    reason TEXT NOT NULL,       -- text:N | cites:L:N | cited_by:L
    via TEXT NOT NULL,          -- search | citation | both | none
    added_at INTEGER,           -- Unix seconds of the run that first accepted it (absent for archives older than this column)
    PRIMARY KEY (topic, work)
);
CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY,
    topic TEXT NOT NULL REFERENCES topics(name) ON DELETE CASCADE,
    finished_at TEXT NOT NULL,  -- as the caller's clock gave it
    stop TEXT NOT NULL,         -- see Stop::name
    halt_reason TEXT,
    calls INTEGER NOT NULL,
    accepted INTEGER NOT NULL,
    archived INTEGER NOT NULL,
    by_search INTEGER NOT NULL,
    by_citation INTEGER NOT NULL,
    by_both INTEGER NOT NULL,
    missing TEXT NOT NULL,      -- the estimate as words, UNMEASURED with its reason when there is none
    kind TEXT NOT NULL DEFAULT 'full',  -- full | update
    finished_secs INTEGER
);
CREATE VIRTUAL TABLE IF NOT EXISTS works_fts USING fts5(title, abstract);
-- Each subject's knowledge graph (graph.rs), rebuilt from the archive after every run.
CREATE TABLE IF NOT EXISTS graph_nodes (
    topic TEXT NOT NULL REFERENCES topics(name) ON DELETE CASCADE,
    work INTEGER NOT NULL REFERENCES works(id),
    community INTEGER NOT NULL,
    pagerank REAL NOT NULL,
    role TEXT NOT NULL,         -- foundation | bridge | frontier | member
    cites INTEGER NOT NULL,
    cited_by INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    PRIMARY KEY (topic, work)
);
CREATE TABLE IF NOT EXISTS graph_edges (
    topic TEXT NOT NULL REFERENCES topics(name) ON DELETE CASCADE,
    a INTEGER NOT NULL,
    b INTEGER NOT NULL,
    kind TEXT NOT NULL,         -- cites (a cites b) | similar
    weight REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS graph_edges_a ON graph_edges(topic, a);
CREATE INDEX IF NOT EXISTS graph_edges_b ON graph_edges(topic, b);
-- Each subject's gaps (gaps.rs), found with its graph.
CREATE TABLE IF NOT EXISTS research_gaps (
    topic TEXT NOT NULL REFERENCES topics(name) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    rank INTEGER NOT NULL,
    score REAL NOT NULL,
    headline TEXT NOT NULL,
    detail TEXT NOT NULL,
    terms TEXT NOT NULL,        -- JSON array
    works TEXT NOT NULL,        -- JSON array of work ids
    threads TEXT NOT NULL,      -- JSON array
    PRIMARY KEY (topic, kind, rank)
);
-- What the person did on the Research page, for the hub. Local only.
CREATE TABLE IF NOT EXISTS activity (
    id INTEGER PRIMARY KEY,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    subject TEXT,
    work INTEGER,
    text TEXT
);
-- What an agent concluded about a gap after working on it (the MCP server's save_finding). Appended, never edited.
CREATE TABLE IF NOT EXISTS gap_findings (
    id INTEGER PRIMARY KEY,
    subject TEXT NOT NULL,
    headline TEXT NOT NULL,
    at TEXT NOT NULL,
    by TEXT NOT NULL,
    verdict TEXT NOT NULL,      -- real | closed | unclear
    why TEXT NOT NULL,
    next_step TEXT NOT NULL,
    sources TEXT NOT NULL       -- JSON array of strings
);
CREATE TABLE IF NOT EXISTS graph_communities (
    topic TEXT NOT NULL REFERENCES topics(name) ON DELETE CASCADE,
    id INTEGER NOT NULL,
    label TEXT NOT NULL,
    size INTEGER NOT NULL,
    year_min INTEGER,
    year_max INTEGER,
    PRIMARY KEY (topic, id)
);
";

pub struct Store {
    db: Connection,
    /// The tables (`"activity"`) and columns (`"topics.watch"`) this archive has. An archive opened read-only is not
    /// migrated, so one made by an older build lacks what came later; its readers fall back instead of failing.
    has: HashSet<String>,
}

/// The tables and columns of `db` that the readers ask about.
fn schema_of(db: &Connection) -> Result<HashSet<String>, Error> {
    let mut has = HashSet::new();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>())
        .map_err(store)?;
    for t in tables {
        if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let cols: Vec<String> = db
            .prepare(&format!("PRAGMA table_info({t})"))
            .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>())
            .map_err(store)?;
        for c in cols {
            has.insert(format!("{t}.{c}"));
        }
        has.insert(t);
    }
    Ok(has)
}

/// One full-text hit.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub work: i64,
    pub title: String,
    pub year: Option<i32>,
    pub ids: Vec<Id>,
    pub open_url: Option<String>,
}

/// A moment, as words for people and as Unix seconds for comparisons. The caller's clock gives both.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    pub text: String,
    pub secs: i64,
}

/// How often a subject is updated with what is new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watch {
    Off,
    Daily,
    Weekly,
}

impl Watch {
    pub const ALL: [Watch; 3] = [Watch::Off, Watch::Daily, Watch::Weekly];

    pub fn name(self) -> &'static str {
        match self {
            Watch::Off => "off",
            Watch::Daily => "daily",
            Watch::Weekly => "weekly",
        }
    }

    pub fn parse(s: &str) -> Watch {
        match s {
            "daily" => Watch::Daily,
            "weekly" => Watch::Weekly,
            _ => Watch::Off,
        }
    }

    pub fn every_secs(self) -> Option<i64> {
        match self {
            Watch::Off => None,
            Watch::Daily => Some(86_400),
            Watch::Weekly => Some(7 * 86_400),
        }
    }
}

/// A paper a subject accepted after its first run.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPaper {
    pub subject: String,
    pub paper: PaperRef,
    pub added_at: i64,
    pub reason: String,
}

/// One run's outcome, as saved.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// full | update
    pub kind: String,
    pub finished_at: String,
    pub stop: String,
    pub halt_reason: Option<String>,
    pub calls: u64,
    pub accepted: u64,
    pub archived: u64,
    pub by_search: u64,
    pub by_citation: u64,
    pub by_both: u64,
    pub missing: String,
}

/// A saved subject and its latest run.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicRow {
    pub name: String,
    pub queries: Vec<String>,
    pub phrases: Vec<String>,
    pub members: u64,
    pub last: Option<RunRow>,
    /// The latest full run (an update run reads only what is new and makes no estimate of what is missing).
    pub last_full: Option<RunRow>,
    pub watch: Watch,
    /// Unix seconds of the last run of either kind.
    pub watched_at: Option<i64>,
}

/// One accepted paper of a subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// The archive's row id.
    pub id: i64,
    pub work: Work,
    pub reason: String,
    pub via: String,
}

fn store(e: rusqlite::Error) -> Error {
    Error::Store(e.to_string())
}

pub fn reason_text(r: Reason) -> String {
    match r {
        Reason::Text { phrases } => format!("text:{phrases}"),
        Reason::Cites { links, phrases } => format!("cites:{links}:{phrases}"),
        Reason::CitedBy { links } => format!("cited_by:{links}"),
        Reason::Peripheral { links } => format!("peripheral:{links}"),
    }
}

fn via_text(found: Option<&std::collections::BTreeSet<Found>>) -> &'static str {
    let Some(f) = found else { return "none" };
    let s = f.iter().any(|x| matches!(x, Found::Search(_)));
    let c = f.iter().any(|x| matches!(x, Found::Reference | Found::Citation));
    match (s, c) {
        (true, true) => "both",
        (true, false) => "search",
        (false, true) => "citation",
        (false, false) => "none",
    }
}

/// Plain words to an FTS5 query that matches papers containing every word (each quoted, so punctuation and FTS5's
/// operators in the user's text are taken literally). `None` when there are no words.
pub fn words_query(text: &str) -> Option<String> {
    let words: Vec<String> = text.split_whitespace().map(|w| format!("\"{}\"", w.replace('"', "\"\""))).collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// The prepared statements one save reuses for every paper.
struct Writes<'t> {
    find: Statement<'t>,
    update: Statement<'t>,
    insert: Statement<'t>,
    fts_drop: Statement<'t>,
    fts_add: Statement<'t>,
    read_back: Statement<'t>,
    id: Statement<'t>,
    reference: Statement<'t>,
    seen: Statement<'t>,
}

impl<'t> Writes<'t> {
    fn prepare(tx: &'t rusqlite::Transaction) -> Result<Writes<'t>, Error> {
        Ok(Writes {
            find: tx.prepare("SELECT work FROM ids WHERE scheme = ?1 AND value = ?2").map_err(store)?,
            update: tx
                .prepare(
                    "UPDATE works SET title = ?2, abstract = COALESCE(?3, abstract), year = COALESCE(?4, year), authors = ?5,
                     venue = COALESCE(?6, venue), open_url = COALESCE(?7, open_url), cited_by_count = COALESCE(?8, cited_by_count) WHERE id = ?1",
                )
                .map_err(store)?,
            insert: tx
                .prepare("INSERT INTO works (title, abstract, year, authors, venue, open_url, cited_by_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
                .map_err(store)?,
            fts_drop: tx.prepare("DELETE FROM works_fts WHERE rowid = ?1").map_err(store)?,
            fts_add: tx.prepare("INSERT INTO works_fts (rowid, title, abstract) VALUES (?1, ?2, ?3)").map_err(store)?,
            read_back: tx.prepare("SELECT title, abstract FROM works WHERE id = ?1").map_err(store)?,
            id: tx.prepare("INSERT OR IGNORE INTO ids (scheme, value, work) VALUES (?1, ?2, ?3)").map_err(store)?,
            reference: tx.prepare("INSERT OR IGNORE INTO refs (work, scheme, value) VALUES (?1, ?2, ?3)").map_err(store)?,
            seen: tx.prepare("INSERT OR IGNORE INTO seen (work, source) VALUES (?1, ?2)").map_err(store)?,
        })
    }
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Store(format!("{}: {e}", dir.display())))?;
        }
        Store::init(Connection::open(path).map_err(store)?)
    }

    /// Open an existing archive for reading only: nothing is created, migrated or written (the MCP server's mode).
    pub fn open_read_only(path: &Path) -> Result<Store, Error> {
        if !path.is_file() {
            return Err(Error::Store(format!("{} does not exist", path.display())));
        }
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(store)?;
        let has = schema_of(&db)?;
        Ok(Store { db, has })
    }

    /// One archived paper, whole.
    pub fn work(&self, id: i64) -> Result<Option<Work>, Error> {
        Ok(self.works_where("WHERE id = ?1", params![id])?.into_iter().next().map(|(_, w)| w))
    }

    pub fn in_memory() -> Result<Store, Error> {
        Store::init(Connection::open_in_memory().map_err(store)?)
    }

    fn init(db: Connection) -> Result<Store, Error> {
        db.execute_batch(SCHEMA).map_err(store)?;
        // Columns added after the first archives were made: added in place, so an older archive keeps its papers.
        for (table, column, decl) in [
            ("topics", "watch", "TEXT NOT NULL DEFAULT 'off'"),
            ("topics", "watched_at", "INTEGER"),
            ("members", "added_at", "INTEGER"),
            ("runs", "kind", "TEXT NOT NULL DEFAULT 'full'"),
            ("runs", "finished_secs", "INTEGER"),
        ] {
            let has: bool = db
                .prepare(&format!("PRAGMA table_info({table})"))
                .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<Vec<_>, _>>())
                .map_err(store)?
                .iter()
                .any(|c| c == column);
            if !has {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl};")).map_err(store)?;
            }
        }
        let has = schema_of(&db)?;
        Ok(Store { db, has })
    }

    /// Write a finished run: every paper the run holds (accepted or not, so a later run with a wider policy need
    /// not fetch them again), the subject's membership (replacing its previous one) and the run's outcome.
    pub fn save(&mut self, run: &Snowball, report: &Report, now: &Stamp) -> Result<(), Error> {
        let tx = self.db.transaction().map_err(store)?;
        let mut row_of = Vec::with_capacity(run.archive.len());
        {
            let mut w = Writes::prepare(&tx)?;
            for (_, work) in run.archive.works() {
                row_of.push(upsert(&mut w, &tx, work)?);
            }
        }
        let json = |v: &[String]| serde_json::to_string(v).unwrap_or_default();
        let name = &run.topic.name;
        tx.execute(
            "INSERT INTO topics (name, queries, phrases, watched_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET queries = ?2, phrases = ?3, watched_at = ?4",
            params![name, json(&run.topic.queries), json(&run.topic.phrases), now.secs],
        )
        .map_err(store)?;
        // When each member joined is kept across runs; a paper new to the subject joins now.
        let joined: HashMap<i64, Option<i64>> = {
            let mut st = tx.prepare("SELECT work, added_at FROM members WHERE topic = ?1").map_err(store)?;
            st.query_map(params![name], |r| Ok((r.get(0)?, r.get(1)?))).map_err(store)?.collect::<Result<_, _>>().map_err(store)?
        };
        tx.execute("DELETE FROM members WHERE topic = ?1", params![name]).map_err(store)?;
        {
            let mut add = tx
                .prepare("INSERT OR REPLACE INTO members (topic, work, reason, via, added_at) VALUES (?1, ?2, ?3, ?4, ?5)")
                .map_err(store)?;
            for (k, _, r) in run.accepted() {
                // A member from before joining times were kept counts as joined at the beginning (0), so what an
                // update adds to an old archive still reads as new.
                let added = match joined.get(&row_of[k]) {
                    Some(at) => Some(at.unwrap_or(0)),
                    None => Some(now.secs),
                };
                add.execute(params![name, row_of[k], reason_text(r), via_text(run.found(k)), added]).map_err(store)?;
            }
        }
        // A run keeps one row while it goes (its latest checkpoint), replaced by the next checkpoint or its outcome.
        tx.execute("DELETE FROM runs WHERE topic = ?1 AND stop = 'in_progress'", params![name]).map_err(store)?;
        tx.execute(
            "INSERT INTO runs (topic, finished_at, stop, halt_reason, calls, accepted, archived, by_search, by_citation, by_both, missing, kind, finished_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                name,
                now.text,
                report.stop.name(),
                report.halt_reason,
                report.calls as i64,
                report.accepted as i64,
                (report.accepted + report.candidates) as i64,
                report.by_search as i64,
                report.by_citation as i64,
                report.by_both as i64,
                report.coverage.summary(),
                if report.update { "update" } else { "full" },
                now.secs,
            ],
        )
        .map_err(store)?;
        tx.commit().map_err(store)
    }

    /// Copy a subject from another archive: its member papers (merged with papers this archive already holds), its
    /// searches and concepts, the reasons and routes of its members, and its runs; then its graph and gaps are built
    /// here. Refuses a name this archive already uses. Returns the number of papers copied.
    pub fn import_subject(&mut self, from: &Store, name: &str, now: &Stamp) -> Result<usize, Error> {
        if self.topics()?.iter().any(|t| t.name == name) {
            return Err(Error::Store(format!("this archive already has a subject named {name:?}")));
        }
        let topic = from.topics()?.into_iter().find(|t| t.name == name).ok_or_else(|| Error::Store(format!("no subject named {name:?} to copy")))?;
        let members = from.members(name)?;
        let mut runs = from.runs(name, 1_000)?;
        runs.reverse();
        let json = |v: &[String]| serde_json::to_string(v).unwrap_or_default();
        let tx = self.db.transaction().map_err(store)?;
        let mut rows = Vec::with_capacity(members.len());
        {
            let mut w = Writes::prepare(&tx)?;
            for m in &members {
                rows.push(upsert(&mut w, &tx, &m.work)?);
            }
        }
        tx.execute("INSERT INTO topics (name, queries, phrases, watched_at) VALUES (?1, ?2, ?3, ?4)", params![name, json(&topic.queries), json(&topic.phrases), now.secs])
            .map_err(store)?;
        {
            let mut add = tx.prepare("INSERT OR REPLACE INTO members (topic, work, reason, via, added_at) VALUES (?1, ?2, ?3, ?4, ?5)").map_err(store)?;
            for (m, row) in members.iter().zip(&rows) {
                add.execute(params![name, row, m.reason, m.via, now.secs]).map_err(store)?;
            }
            let mut run = tx
                .prepare(
                    "INSERT INTO runs (topic, finished_at, stop, halt_reason, calls, accepted, archived, by_search, by_citation, by_both, missing, kind, finished_secs)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                )
                .map_err(store)?;
            for r in &runs {
                run.execute(params![
                    name,
                    r.finished_at,
                    r.stop,
                    r.halt_reason,
                    r.calls as i64,
                    r.accepted as i64,
                    r.archived as i64,
                    r.by_search as i64,
                    r.by_citation as i64,
                    r.by_both as i64,
                    r.missing,
                    r.kind,
                    now.secs
                ])
                .map_err(store)?;
            }
        }
        tx.commit().map_err(store)?;
        self.rebuild_graph(name)?;
        Ok(members.len())
    }

    /// Keep a finding beside one of a subject's saved gaps (named by its headline, as `gaps` gives it). Refuses a gap
    /// the archive does not hold, a verdict outside `VERDICTS`, an empty reason, and any part over `FINDING_LIMITS`.
    pub fn save_finding(&mut self, subject: &str, headline: &str, f: &Finding) -> Result<(), Error> {
        let refuse = |why: String| Err(Error::Store(why));
        let held: bool = self
            .db
            .query_row("SELECT EXISTS(SELECT 1 FROM research_gaps WHERE topic = ?1 AND headline = ?2)", params![subject, headline], |r| r.get(0))
            .map_err(store)?;
        if !held {
            return refuse(format!("subject {subject:?} has no gap with that headline; use the headline exactly as research_gaps gives it"));
        }
        if !VERDICTS.contains(&f.verdict.as_str()) {
            return refuse(format!("verdict must be one of {VERDICTS:?}"));
        }
        let l = &FINDING_LIMITS;
        let chars = |s: &str| s.chars().count();
        if f.why.trim().is_empty() {
            return refuse("why is empty".into());
        }
        for (name, text, max) in [("by", &f.by, l.by), ("why", &f.why, l.why), ("next_step", &f.next_step, l.next_step)] {
            if chars(text) > max {
                return refuse(format!("{name} is over {max} characters"));
            }
        }
        if f.sources.len() > l.sources || f.sources.iter().any(|s| chars(s) > l.source) {
            return refuse(format!("at most {} sources of at most {} characters each", l.sources, l.source));
        }
        self.db
            .execute(
                "INSERT INTO gap_findings (subject, headline, at, by, verdict, why, next_step, sources) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![subject, headline, f.at, f.by, f.verdict, f.why, f.next_step, serde_json::to_string(&f.sources).unwrap_or_default()],
            )
            .map_err(store)?;
        Ok(())
    }

    /// Every gap's findings, newest first; none from an archive made before findings were kept.
    fn findings(&self) -> Result<HashMap<(String, String), Vec<Finding>>, Error> {
        let mut out: HashMap<(String, String), Vec<Finding>> = HashMap::new();
        if !self.has.contains("gap_findings") {
            return Ok(out);
        }
        let mut st = self
            .db
            .prepare("SELECT subject, headline, at, by, verdict, why, next_step, sources FROM gap_findings ORDER BY id DESC")
            .map_err(store)?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                    Finding {
                        at: r.get(2)?,
                        by: r.get(3)?,
                        verdict: r.get(4)?,
                        why: r.get(5)?,
                        next_step: r.get(6)?,
                        sources: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                    },
                ))
            })
            .map_err(store)?;
        for row in rows {
            let (key, f) = row.map_err(store)?;
            out.entry(key).or_default().push(f);
        }
        Ok(out)
    }

    /// Set how often a subject is updated with what is new.
    pub fn set_watch(&mut self, topic: &str, watch: Watch) -> Result<(), Error> {
        self.db.execute("UPDATE topics SET watch = ?2 WHERE name = ?1", params![topic, watch.name()]).map_err(store)?;
        Ok(())
    }

    /// Watched subjects whose update is due at `now` (Unix seconds), the longest overdue first.
    pub fn due(&self, now: i64) -> Result<Vec<String>, Error> {
        let mut due: Vec<(i64, String)> = Vec::new();
        for t in self.topics()? {
            if let Some(every) = t.watch.every_secs() {
                let next = t.watched_at.map_or(i64::MIN, |at| at + every);
                if next <= now {
                    due.push((next, t.name));
                }
            }
        }
        due.sort();
        Ok(due.into_iter().map(|(_, n)| n).collect())
    }

    /// Papers accepted after each subject's first run, since `since` (Unix seconds), newest first: what an update
    /// brought. Papers of an archive older than the joining column are never reported as new.
    pub fn new_papers(&self, topic: Option<&str>, since: i64, limit: usize) -> Result<Vec<NewPaper>, Error> {
        // An archive from before joining times were kept has nothing to call new.
        if !self.has.contains("members.added_at") {
            return Ok(Vec::new());
        }
        let mut st = self
            .db
            .prepare(
                "SELECT m.topic, m.work, m.added_at, m.reason FROM members m
                 WHERE (?1 IS NULL OR m.topic = ?1) AND m.added_at IS NOT NULL AND m.added_at >= ?2
                   AND m.added_at > (SELECT MIN(f.added_at) FROM members f WHERE f.topic = m.topic)
                 ORDER BY m.added_at DESC, m.work LIMIT ?3",
            )
            .map_err(store)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows: Vec<(String, i64, i64, String)> =
            st.query_map(params![topic, since, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(store)?.collect::<Result<_, _>>().map_err(store)?;
        let mut out = Vec::new();
        for (subject, work, added_at, reason) in rows {
            if let Some(paper) = self.paper_ref(work)? {
                out.push(NewPaper { subject, paper, added_at, reason });
            }
        }
        Ok(out)
    }

    /// Every saved subject, by name, with its latest run.
    pub fn topics(&self) -> Result<Vec<TopicRow>, Error> {
        let watch = if self.has.contains("topics.watch") { "t.watch" } else { "'off'" };
        let watched_at = if self.has.contains("topics.watched_at") { "t.watched_at" } else { "NULL" };
        let mut st = self
            .db
            .prepare(&format!(
                "SELECT t.name, t.queries, t.phrases, (SELECT COUNT(*) FROM members m WHERE m.topic = t.name), {watch}, {watched_at}
                 FROM topics t ORDER BY t.name"
            ))
            .map_err(store)?;
        let rows = st
            .query_map([], |r| {
                Ok(TopicRow {
                    name: r.get(0)?,
                    queries: serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default(),
                    phrases: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                    members: r.get::<_, i64>(3)? as u64,
                    last: None,
                    last_full: None,
                    watch: Watch::parse(&r.get::<_, String>(4)?),
                    watched_at: r.get(5)?,
                })
            })
            .map_err(store)?;
        let mut out = rows.collect::<Result<Vec<_>, _>>().map_err(store)?;
        for t in &mut out {
            let runs = self.runs(&t.name, 50)?;
            t.last = runs.first().cloned();
            t.last_full = runs.into_iter().find(|r| r.kind == "full");
        }
        Ok(out)
    }

    /// A subject's runs, newest first.
    pub fn runs(&self, topic: &str, limit: usize) -> Result<Vec<RunRow>, Error> {
        let mut st = self
            .db
            .prepare(&format!(
                "SELECT finished_at, stop, halt_reason, calls, accepted, archived, by_search, by_citation, by_both, missing, {}
                 FROM runs WHERE topic = ?1 ORDER BY id DESC LIMIT ?2",
                // Before updates existed every run was a full one.
                if self.has.contains("runs.kind") { "kind" } else { "'full'" }
            ))
            .map_err(store)?;
        let rows = st
            .query_map(params![topic, limit as i64], |r| {
                let n = |i: usize| r.get::<_, i64>(i).map(|v| v as u64);
                Ok(RunRow {
                    kind: r.get(10)?,
                    finished_at: r.get(0)?,
                    stop: r.get(1)?,
                    halt_reason: r.get(2)?,
                    calls: n(3)?,
                    accepted: n(4)?,
                    archived: n(5)?,
                    by_search: n(6)?,
                    by_citation: n(7)?,
                    by_both: n(8)?,
                    missing: r.get(9)?,
                })
            })
            .map_err(store)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(store)
    }

    /// Forget a subject: its membership and runs. The papers stay archived (other subjects may hold them).
    pub fn delete_topic(&mut self, topic: &str) -> Result<(), Error> {
        let tx = self.db.transaction().map_err(store)?;
        for table in ["graph_nodes", "graph_edges", "graph_communities", "research_gaps"] {
            tx.execute(&format!("DELETE FROM {table} WHERE topic = ?1"), params![topic]).map_err(store)?;
        }
        tx.execute("DELETE FROM members WHERE topic = ?1", params![topic]).map_err(store)?;
        tx.execute("DELETE FROM runs WHERE topic = ?1", params![topic]).map_err(store)?;
        tx.execute("DELETE FROM topics WHERE name = ?1", params![topic]).map_err(store)?;
        tx.commit().map_err(store)
    }

    /// Full-text search over every archived paper, or over one subject's accepted papers. `query` is FTS5 syntax;
    /// [`words_query`] makes one from plain words.
    pub fn search(&self, query: &str, topic: Option<&str>, limit: usize) -> Result<Vec<Hit>, Error> {
        let mut st = self
            .db
            .prepare(
                "SELECT w.id, w.title, w.year, w.open_url FROM works_fts f JOIN works w ON w.id = f.rowid
                 WHERE works_fts MATCH ?1
                   AND (?2 IS NULL OR EXISTS (SELECT 1 FROM members m WHERE m.topic = ?2 AND m.work = w.id))
                 ORDER BY f.rank LIMIT ?3",
            )
            .map_err(store)?;
        let rows = st
            .query_map(params![query, topic, limit as i64], |r| {
                Ok(Hit { work: r.get(0)?, title: r.get(1)?, year: r.get(2)?, ids: Vec::new(), open_url: r.get(3)? })
            })
            .map_err(store)?;
        let mut hits = rows.collect::<Result<Vec<_>, _>>().map_err(store)?;
        for h in &mut hits {
            h.ids = self.ids_of(h.work)?;
        }
        Ok(hits)
    }

    fn ids_of(&self, work: i64) -> Result<Vec<Id>, Error> {
        let mut st = self.db.prepare("SELECT scheme, value FROM ids WHERE work = ?1 ORDER BY scheme").map_err(store)?;
        let rows = st.query_map(params![work], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(store)?;
        Ok(rows.filter_map(Result::ok).filter_map(|(s, v)| Id::from_parts(&s, &v)).collect())
    }

    /// Every archived paper, accepted or not, so a changed policy can re-judge them without fetching again.
    pub fn all_works(&self) -> Result<Vec<Work>, Error> {
        Ok(self.works_where("", params![])?.into_iter().map(|(_, w)| w).collect())
    }

    /// A subject's accepted papers with their reasons and how they were found, oldest first.
    pub fn members(&self, topic: &str) -> Result<Vec<Member>, Error> {
        let mut tags: std::collections::HashMap<i64, (String, String)> = std::collections::HashMap::new();
        {
            let mut st = self.db.prepare("SELECT work, reason, via FROM members WHERE topic = ?1").map_err(store)?;
            let rows = st.query_map(params![topic], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).map_err(store)?;
            for row in rows {
                let (w, reason, via) = row.map_err(store)?;
                tags.insert(w, (reason, via));
            }
        }
        let works = self.works_where("WHERE id IN (SELECT work FROM members WHERE topic = ?1)", params![topic])?;
        let mut out: Vec<Member> = works
            .into_iter()
            .filter_map(|(id, work)| tags.remove(&id).map(|(reason, via)| Member { id, work, reason, via }))
            .collect();
        out.sort_by(|a, b| a.work.year.unwrap_or(i32::MAX).cmp(&b.work.year.unwrap_or(i32::MAX)).then_with(|| a.work.title.cmp(&b.work.title)));
        Ok(out)
    }

    /// Papers matching `filter` (a WHERE clause over `works`, or empty), with their identifiers, references and
    /// sources, read in four scans rather than per paper.
    fn works_where(&self, filter: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<(i64, Work)>, Error> {
        use std::collections::HashMap;
        let mut works: HashMap<i64, Work> = HashMap::new();
        let mut order = Vec::new();
        let sql = format!("SELECT id, title, abstract, year, authors, venue, open_url, cited_by_count FROM works {filter} ORDER BY id");
        let mut st = self.db.prepare(&sql).map_err(store)?;
        let rows = st
            .query_map(args, |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Work {
                        title: r.get(1)?,
                        abstract_text: r.get(2)?,
                        year: r.get(3)?,
                        authors: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                        venue: r.get(5)?,
                        open_url: r.get(6)?,
                        cited_by_count: r.get::<_, Option<i64>>(7)?.map(|c| c as u64),
                        ..Work::default()
                    },
                ))
            })
            .map_err(store)?;
        for row in rows {
            let (id, w) = row.map_err(store)?;
            order.push(id);
            works.insert(id, w);
        }
        let sub = |table_sql: &str| -> Result<Vec<(i64, String, String)>, Error> {
            let sql = if filter.is_empty() { table_sql.to_string() } else { format!("{table_sql} WHERE work IN (SELECT id FROM works {filter})") };
            let mut st = self.db.prepare(&sql).map_err(store)?;
            let rows = st.query_map(args, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(store)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(store)
        };
        for (w, sch, v) in sub("SELECT work, scheme, value FROM ids")? {
            if let (Some(work), Some(id)) = (works.get_mut(&w), Id::from_parts(&sch, &v)) {
                work.ids.insert(id);
            }
        }
        for (w, sch, v) in sub("SELECT work, scheme, value FROM refs")? {
            if let (Some(work), Some(id)) = (works.get_mut(&w), Id::from_parts(&sch, &v)) {
                work.references.insert(id);
            }
        }
        for (w, src, _) in sub("SELECT work, source, '' FROM seen")? {
            if let (Some(work), Some(s)) = (works.get_mut(&w), Source::parse(&src)) {
                work.seen_in.insert(s);
            }
        }
        Ok(order.into_iter().filter_map(|id| works.remove(&id).map(|w| (id, w))).collect())
    }
}

/// A paper as the graph's readers see it.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperRef {
    pub work: i64,
    pub title: String,
    pub year: Option<i32>,
    /// Its identifiers as text ("doi:10.1/x", "openalex:W1", "arxiv:2106.09685").
    pub ids: Vec<String>,
}

/// One paper's place in a subject's graph.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub paper: PaperRef,
    pub community: usize,
    pub pagerank: f64,
    pub role: Role,
    pub cites: u64,
    pub cited_by: u64,
    pub x: f32,
    pub y: f32,
}

/// A subject's whole graph, for drawing or export.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoredGraph {
    pub nodes: Vec<GraphNode>,
    /// (work a, work b, kind, weight): `cites` means a cites b; `similar` is undirected.
    pub edges: Vec<(i64, i64, String, f64)>,
    pub communities: Vec<Community>,
}

/// A paper's connections within a subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbourhood {
    pub node: GraphNode,
    pub community: Option<Community>,
    /// Its rank by influence in the subject, 1 = most influential, of `of`.
    pub rank: usize,
    pub of: usize,
    pub cites: Vec<PaperRef>,
    pub cited_by: Vec<PaperRef>,
    pub similar: Vec<(PaperRef, f64)>,
}

impl Store {
    /// Rebuild a subject's knowledge graph from the archive and replace the stored one. Returns its size
    /// (nodes, edges, threads).
    pub fn rebuild_graph(&mut self, topic: &str) -> Result<(usize, usize, usize), Error> {
        // Papers on the edge stay members but not in the map: they would add threads of other fields.
        let all = self.members(topic)?;
        let members: Vec<Member> = all.into_iter().filter(|m| !m.reason.starts_with("peripheral:")).collect();
        let inputs: Vec<graph::Input> = members
            .iter()
            .map(|m| graph::Input {
                work: m.id,
                title: m.work.title.clone(),
                year: m.work.year,
                ids: m.work.ids.iter().cloned().collect(),
                references: m.work.references.iter().cloned().collect(),
            })
            .collect();
        let g = graph::build(&inputs);
        let tx = self.db.transaction().map_err(store)?;
        for table in ["graph_nodes", "graph_edges", "graph_communities"] {
            tx.execute(&format!("DELETE FROM {table} WHERE topic = ?1"), params![topic]).map_err(store)?;
        }
        {
            let mut node = tx
                .prepare("INSERT INTO graph_nodes (topic, work, community, pagerank, role, cites, cited_by, x, y) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")
                .map_err(store)?;
            for n in &g.nodes {
                node.execute(params![topic, n.work, n.community as i64, n.pagerank, n.role.name(), n.cites as i64, n.cited_by as i64, n.x as f64, n.y as f64])
                    .map_err(store)?;
            }
            let mut edge = tx.prepare("INSERT INTO graph_edges (topic, a, b, kind, weight) VALUES (?1, ?2, ?3, ?4, ?5)").map_err(store)?;
            for e in &g.edges {
                edge.execute(params![topic, g.nodes[e.a].work, g.nodes[e.b].work, e.kind.name(), e.weight]).map_err(store)?;
            }
            let mut comm = tx
                .prepare("INSERT INTO graph_communities (topic, id, label, size, year_min, year_max) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .map_err(store)?;
            for c in &g.communities {
                comm.execute(params![topic, c.id as i64, c.label, c.size as i64, c.years.map(|y| y.0), c.years.map(|y| y.1)]).map_err(store)?;
            }
        }
        tx.commit().map_err(store)?;
        self.rebuild_gaps(topic, &members, &g)?;
        Ok((g.nodes.len(), g.edges.len(), g.communities.len()))
    }

    fn paper_ref(&self, work: i64) -> Result<Option<PaperRef>, Error> {
        let row = self
            .db
            .query_row("SELECT title, year FROM works WHERE id = ?1", params![work], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i32>>(1)?)))
            .optional()
            .map_err(store)?;
        let Some((title, year)) = row else { return Ok(None) };
        Ok(Some(PaperRef { work, title, year, ids: self.ids_of(work)?.iter().map(|i| i.to_string()).collect() }))
    }

    /// A subject's threads, largest first.
    pub fn communities(&self, topic: &str) -> Result<Vec<Community>, Error> {
        if !self.has.contains("graph_communities") {
            return Ok(Vec::new());
        }
        let mut st = self
            .db
            .prepare("SELECT id, label, size, year_min, year_max FROM graph_communities WHERE topic = ?1 ORDER BY id")
            .map_err(store)?;
        let rows = st
            .query_map(params![topic], |r| {
                let (lo, hi): (Option<i32>, Option<i32>) = (r.get(3)?, r.get(4)?);
                Ok(Community { id: r.get::<_, i64>(0)? as usize, label: r.get(1)?, size: r.get::<_, i64>(2)? as usize, years: lo.zip(hi) })
            })
            .map_err(store)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(store)
    }

    /// A subject's graph nodes, most influential first, optionally one thread's or one role's.
    pub fn graph_nodes(&self, topic: &str, community: Option<usize>, role: Option<Role>, limit: usize) -> Result<Vec<GraphNode>, Error> {
        if !self.has.contains("graph_nodes") {
            return Ok(Vec::new());
        }
        let mut st = self
            .db
            .prepare(
                "SELECT n.work, w.title, w.year, n.community, n.pagerank, n.role, n.cites, n.cited_by, n.x, n.y
                 FROM graph_nodes n JOIN works w ON w.id = n.work
                 WHERE n.topic = ?1 AND (?2 IS NULL OR n.community = ?2) AND (?3 IS NULL OR n.role = ?3)
                 ORDER BY n.pagerank DESC, n.work LIMIT ?4",
            )
            .map_err(store)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = st
            .query_map(params![topic, community.map(|c| c as i64), role.map(|r| r.name()), limit], |r| {
                Ok(GraphNode {
                    paper: PaperRef { work: r.get(0)?, title: r.get(1)?, year: r.get(2)?, ids: Vec::new() },
                    community: r.get::<_, i64>(3)? as usize,
                    pagerank: r.get(4)?,
                    role: Role::parse(&r.get::<_, String>(5)?),
                    cites: r.get::<_, i64>(6)? as u64,
                    cited_by: r.get::<_, i64>(7)? as u64,
                    x: r.get::<_, f64>(8)? as f32,
                    y: r.get::<_, f64>(9)? as f32,
                })
            })
            .map_err(store)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(store)
    }

    /// The whole graph, for drawing or export. Identifiers are not filled in (`neighbourhood` gives one paper's).
    pub fn graph(&self, topic: &str) -> Result<StoredGraph, Error> {
        if !self.has.contains("graph_edges") {
            return Ok(StoredGraph::default());
        }
        let nodes = self.graph_nodes(topic, None, None, usize::MAX)?;
        let mut st = self.db.prepare("SELECT a, b, kind, weight FROM graph_edges WHERE topic = ?1").map_err(store)?;
        let edges = st
            .query_map(params![topic], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(store)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(store)?;
        Ok(StoredGraph { nodes, edges, communities: self.communities(topic)? })
    }

    /// One paper's connections within a subject, or `None` when it is not in the subject's graph.
    pub fn neighbourhood(&self, topic: &str, work: i64) -> Result<Option<Neighbourhood>, Error> {
        if !self.has.contains("graph_nodes") {
            return Ok(None);
        }
        let mut st = self
            .db
            .prepare(
                "SELECT n.community, n.pagerank, n.role, n.cites, n.cited_by, n.x, n.y,
                        (SELECT COUNT(*) FROM graph_nodes o WHERE o.topic = n.topic AND o.pagerank > n.pagerank) + 1,
                        (SELECT COUNT(*) FROM graph_nodes o WHERE o.topic = n.topic)
                 FROM graph_nodes n WHERE n.topic = ?1 AND n.work = ?2",
            )
            .map_err(store)?;
        let row = st
            .query_row(params![topic, work], |r| {
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, f64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? as u64,
                    r.get::<_, i64>(4)? as u64,
                    r.get::<_, f64>(5)? as f32,
                    r.get::<_, f64>(6)? as f32,
                    r.get::<_, i64>(7)? as usize,
                    r.get::<_, i64>(8)? as usize,
                ))
            })
            .optional()
            .map_err(store)?;
        let Some((community, pagerank, role, cites, cited_by, x, y, rank, of)) = row else { return Ok(None) };
        let Some(paper) = self.paper_ref(work)? else { return Ok(None) };
        let list = |sql: &str| -> Result<Vec<(i64, f64)>, Error> {
            let mut st = self.db.prepare(sql).map_err(store)?;
            let rows = st.query_map(params![topic, work], |r| Ok((r.get(0)?, r.get(1)?))).map_err(store)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(store)
        };
        let refs = |pairs: Vec<(i64, f64)>| -> Result<Vec<(PaperRef, f64)>, Error> {
            let mut out = Vec::new();
            for (w, x) in pairs {
                if let Some(p) = self.paper_ref(w)? {
                    out.push((p, x));
                }
            }
            Ok(out)
        };
        let cites_list = refs(list(
            "SELECT e.b, 0.0 FROM graph_edges e WHERE e.topic = ?1 AND e.a = ?2 AND e.kind = 'cites'
             ORDER BY (SELECT pagerank FROM graph_nodes g WHERE g.topic = ?1 AND g.work = e.b) DESC",
        )?)?;
        let cited_list = refs(list(
            "SELECT e.a, 0.0 FROM graph_edges e WHERE e.topic = ?1 AND e.b = ?2 AND e.kind = 'cites'
             ORDER BY (SELECT pagerank FROM graph_nodes g WHERE g.topic = ?1 AND g.work = e.a) DESC",
        )?)?;
        let similar = refs(list(
            "SELECT CASE WHEN e.a = ?2 THEN e.b ELSE e.a END, e.weight FROM graph_edges e
             WHERE e.topic = ?1 AND (e.a = ?2 OR e.b = ?2) AND e.kind = 'similar' ORDER BY e.weight DESC",
        )?)?;
        let community_row = self.communities(topic)?.into_iter().find(|c| c.id == community);
        Ok(Some(Neighbourhood {
            node: GraphNode { paper, community, pagerank, role: Role::parse(&role), cites, cited_by, x, y },
            community: community_row,
            rank,
            of,
            cites: cites_list.into_iter().map(|(p, _)| p).collect(),
            cited_by: cited_list.into_iter().map(|(p, _)| p).collect(),
            similar,
        }))
    }

    /// The shortest chain of citations (either direction) linking two papers of a subject. Each step after the first
    /// says how it relates to the step before: "cites" (the earlier step cites it) or "is cited by".
    pub fn connection(&self, topic: &str, a: i64, b: i64) -> Result<Option<Vec<(PaperRef, &'static str)>>, Error> {
        use std::collections::{HashMap, VecDeque, hash_map::Entry};
        if !self.has.contains("graph_edges") {
            return Ok(None);
        }
        let mut st = self.db.prepare("SELECT a, b FROM graph_edges WHERE topic = ?1 AND kind = 'cites'").map_err(store)?;
        let edges: Vec<(i64, i64)> = st.query_map(params![topic], |r| Ok((r.get(0)?, r.get(1)?))).map_err(store)?.collect::<Result<_, _>>().map_err(store)?;
        let directed: std::collections::HashSet<(i64, i64)> = edges.iter().copied().collect();
        let mut next: HashMap<i64, Vec<i64>> = HashMap::new();
        for &(x, y) in &edges {
            next.entry(x).or_default().push(y);
            next.entry(y).or_default().push(x);
        }
        let mut prev: HashMap<i64, i64> = HashMap::from([(a, a)]);
        let mut queue = VecDeque::from([a]);
        while let Some(u) = queue.pop_front() {
            if u == b {
                let mut chain = vec![b];
                let mut at = b;
                while at != a {
                    at = prev[&at];
                    chain.push(at);
                }
                chain.reverse();
                let mut out = Vec::new();
                for (k, &w) in chain.iter().enumerate() {
                    let how = match k {
                        0 => "",
                        _ if directed.contains(&(chain[k - 1], w)) => "cites",
                        _ => "is cited by",
                    };
                    out.extend(self.paper_ref(w)?.map(|p| (p, how)));
                }
                return Ok(Some(out));
            }
            let mut around = next.get(&u).cloned().unwrap_or_default();
            around.sort_unstable();
            for v in around {
                if let Entry::Vacant(e) = prev.entry(v) {
                    e.insert(u);
                    queue.push_back(v);
                }
            }
        }
        Ok(None)
    }

    /// The archive's row for an identifier as a person or model writes it: "W2741809807", "openalex:W…",
    /// "https://openalex.org/W…", "10.48550/arXiv.2106.09685", "doi:…", "https://doi.org/…", "arXiv:2106.09685",
    /// "2106.09685", or a row number.
    pub fn find_work(&self, text: &str) -> Result<Option<i64>, Error> {
        let t = text.trim();
        let t = match t.split_once(':') {
            Some((s, rest)) if s.eq_ignore_ascii_case("doi") || s.eq_ignore_ascii_case("openalex") => rest.trim(),
            _ => t,
        };
        for id in [Id::openalex(t), Id::doi(t), Id::arxiv(t)].into_iter().flatten() {
            let row = self
                .db
                .query_row("SELECT work FROM ids WHERE scheme = ?1 AND value = ?2", params![id.scheme(), id.value()], |r| r.get::<_, i64>(0))
                .optional()
                .map_err(store)?;
            if row.is_some() {
                return Ok(row);
            }
        }
        if let Ok(n) = t.parse::<i64>() {
            return self.db.query_row("SELECT id FROM works WHERE id = ?1", params![n], |r| r.get(0)).optional().map_err(store);
        }
        Ok(None)
    }

    /// A subject's graph as node-link JSON (networkx's `node_link_graph`, d3 and most graph tools read it).
    pub fn graph_json(&self, topic: &str) -> Result<serde_json::Value, Error> {
        let g = self.graph(topic)?;
        let labels: std::collections::HashMap<usize, &str> = g.communities.iter().map(|c| (c.id, c.label.as_str())).collect();
        let nodes: Vec<serde_json::Value> = g
            .nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.paper.work, "title": n.paper.title, "year": n.paper.year, "community": n.community,
                    "thread": labels.get(&n.community), "pagerank": n.pagerank, "role": n.role.name(), "x": n.x, "y": n.y,
                })
            })
            .collect();
        let links: Vec<serde_json::Value> =
            g.edges.iter().map(|(a, b, kind, w)| serde_json::json!({"source": a, "target": b, "kind": kind, "weight": w})).collect();
        let threads: Vec<serde_json::Value> = g
            .communities
            .iter()
            .map(|c| serde_json::json!({"id": c.id, "label": c.label, "size": c.size, "years": c.years.map(|y| [y.0, y.1])}))
            .collect();
        Ok(serde_json::json!({"directed": true, "multigraph": true, "graph": {"subject": topic, "threads": threads}, "nodes": nodes, "links": links}))
    }

    /// A subject's graph as GraphML (Gephi, Cytoscape, yEd).
    pub fn graph_ml(&self, topic: &str) -> Result<String, Error> {
        let g = self.graph(topic)?;
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let mut out = String::from(concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#, "\n",
            r#"<graphml xmlns="http://graphml.graphdrawing.org/xmlns">"#, "\n",
            r#"<key id="title" for="node" attr.name="title" attr.type="string"/>"#, "\n",
            r#"<key id="year" for="node" attr.name="year" attr.type="int"/>"#, "\n",
            r#"<key id="community" for="node" attr.name="community" attr.type="int"/>"#, "\n",
            r#"<key id="pagerank" for="node" attr.name="pagerank" attr.type="double"/>"#, "\n",
            r#"<key id="role" for="node" attr.name="role" attr.type="string"/>"#, "\n",
            r#"<key id="kind" for="edge" attr.name="kind" attr.type="string"/>"#, "\n",
            r#"<key id="weight" for="edge" attr.name="weight" attr.type="double"/>"#, "\n",
        ));
        out.push_str(&format!("<graph id=\"{}\" edgedefault=\"directed\">\n", esc(topic)));
        for n in &g.nodes {
            let year = n.paper.year.map(|y| format!(r#"<data key="year">{y}</data>"#)).unwrap_or_default();
            out.push_str(&format!(
                r#"<node id="n{}"><data key="title">{}</data>{year}<data key="community">{}</data><data key="pagerank">{}</data><data key="role">{}</data></node>"#,
                n.paper.work,
                esc(&n.paper.title),
                n.community,
                n.pagerank,
                n.role.name()
            ));
            out.push('\n');
        }
        for (a, b, kind, w) in &g.edges {
            let directed = if kind == "cites" { "true" } else { "false" };
            out.push_str(&format!(
                r#"<edge source="n{a}" target="n{b}" directed="{directed}"><data key="kind">{kind}</data><data key="weight">{w}</data></edge>"#
            ));
            out.push('\n');
        }
        out.push_str("</graph>\n</graphml>\n");
        Ok(out)
    }
}

/// research_gaps as read: topic, kind, rank, score, headline, detail, terms, works, threads.
type GapRow = (String, String, i64, f64, String, String, String, String, String);

/// A gap as saved, with its papers.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredGap {
    pub subject: String,
    pub kind: gaps::Kind,
    /// Its place within its kind for the subject, 0 = strongest.
    pub rank: usize,
    pub score: f64,
    pub headline: String,
    pub detail: String,
    pub terms: Vec<String>,
    pub works: Vec<PaperRef>,
    pub threads: Vec<usize>,
    pub dismissed: bool,
    /// When the person last handed this gap to Sinai to work on (the `pursue_gap` activity's time), if ever.
    pub pursued: Option<String>,
    /// What agents concluded about it, newest first.
    pub findings: Vec<Finding>,
}

/// What an agent concluded about a gap after working on it: whether it is real, already closed by work it found, or
/// unclear; why; the next step; and its sources. Written by the MCP server's `save_finding`, never edited.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub at: String,
    /// Who concluded it ("Sinai").
    pub by: String,
    /// One of `VERDICTS`.
    pub verdict: String,
    pub why: String,
    pub next_step: String,
    pub sources: Vec<String>,
}

/// The verdicts a finding may give.
pub const VERDICTS: [&str; 3] = ["real", "closed", "unclear"];
/// The longest a finding's parts may be, in characters (a finding over them is refused, not cut).
pub const FINDING_LIMITS: FindingLimits = FindingLimits { by: 60, why: 2_000, next_step: 1_000, sources: 12, source: 400 };

pub struct FindingLimits {
    pub by: usize,
    pub why: usize,
    pub next_step: usize,
    pub sources: usize,
    pub source: usize,
}

/// One thing the person did on the Research page, kept on this PC to personalise the hub.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub at: String,
    /// search | open | save | unsave | dismiss_gap | pursue_gap | gather
    pub kind: String,
    pub subject: Option<String>,
    pub work: Option<i64>,
    pub text: Option<String>,
}

impl Store {
    /// Find a subject's gaps from its graph and papers and replace the saved ones (called by `rebuild_graph`).
    fn rebuild_gaps(&mut self, topic: &str, members: &[Member], g: &graph::Graph) -> Result<usize, Error> {
        let texts: Vec<gaps::Text> =
            members.iter().map(|m| gaps::Text { title: m.work.title.clone(), abstract_text: m.work.abstract_text.clone(), year: m.work.year }).collect();
        let mut found = gaps::find(&texts, g);
        found.extend(self.blind_spots(topic, members)?);
        let tx = self.db.transaction().map_err(store)?;
        tx.execute("DELETE FROM research_gaps WHERE topic = ?1", params![topic]).map_err(store)?;
        {
            let mut add = tx
                .prepare(
                    "INSERT INTO research_gaps (topic, kind, rank, score, headline, detail, terms, works, threads)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )
                .map_err(store)?;
            let mut rank: HashMap<&str, usize> = HashMap::new();
            for gap in &found {
                let r = rank.entry(gap.kind.name()).or_default();
                add.execute(params![
                    topic,
                    gap.kind.name(),
                    *r as i64,
                    gap.score,
                    gap.headline,
                    gap.detail,
                    serde_json::to_string(&gap.terms).unwrap_or_default(),
                    serde_json::to_string(&gap.works).unwrap_or_default(),
                    serde_json::to_string(&gap.threads).unwrap_or_default(),
                ])
                .map_err(store)?;
                *r += 1;
            }
        }
        tx.commit().map_err(store)?;
        Ok(found.len())
    }

    /// Works many of the subject's papers cite that are not in the subject: a blind spot of the gathering (an index
    /// that lacks them, or a paper the gate did not accept), worth a look before trusting the subject is complete.
    fn blind_spots(&self, topic: &str, members: &[Member]) -> Result<Vec<gaps::Gap>, Error> {
        // A work the archive never read is a blind spot only once a run has closed (read every accepted paper's
        // references); before that it is simply not fetched yet.
        let closed = self.runs(topic, 1)?.first().is_some_and(|r| r.stop == "closed");
        let mine: HashSet<&Id> = members.iter().flat_map(|m| m.work.ids.iter()).collect();
        let mut counts: HashMap<&Id, usize> = HashMap::new();
        for m in members {
            for r in &m.work.references {
                if !mine.contains(r) {
                    *counts.entry(r).or_default() += 1;
                }
            }
        }
        let floor = (members.len() / 50).max(3);
        let mut top: Vec<(&Id, usize)> = counts.into_iter().filter(|(_, c)| *c >= floor).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut out = Vec::new();
        for (id, count) in top {
            if out.len() == 8 {
                break;
            }
            let row: Option<(i64, Option<i64>)> = self
                .db
                .query_row(
                    "SELECT i.work, w.cited_by_count FROM ids i JOIN works w ON w.id = i.work WHERE i.scheme = ?1 AND i.value = ?2",
                    params![id.scheme(), id.value()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(store)?;
            // A classic every field cites (a standard architecture, a benchmark) is missing on purpose: the subject's
            // papers are a vanishing share of its citers. The gate's own specificity test (5%) decides.
            if let Some((_, Some(total))) = row
                && (count as f64) < 0.05 * total as f64
            {
                continue;
            }
            if row.is_none() && !closed {
                continue;
            }
            let row = row.map(|r| r.0);
            let (headline, works) = match row.map(|w| self.paper_ref(w)).transpose()?.flatten() {
                Some(p) => (
                    format!("{}: \"{}\" is cited by {count} of the subject's papers but is not in it", p.year.map_or("?".into(), |y| y.to_string()), p.title),
                    vec![p.work],
                ),
                None => (format!("{id} is cited by {count} of the subject's papers and no index returned its record"), Vec::new()),
            };
            out.push(gaps::Gap {
                kind: gaps::Kind::BlindSpot,
                score: count as f64,
                headline,
                detail: format!(
                    "{count} of {} papers cite it. Either an index lacks its record, or its text did not match the subject's concepts; if it \
                     belongs, add its words as a concept and gather again.",
                    members.len()
                ),
                terms: Vec::new(),
                works,
                threads: Vec::new(),
            });
        }
        Ok(out)
    }

    /// A subject's saved gaps (or every subject's), strongest first within each kind.
    pub fn gaps(&self, topic: Option<&str>) -> Result<Vec<StoredGap>, Error> {
        if !self.has.contains("research_gaps") {
            return Ok(Vec::new());
        }
        let dismissed: HashSet<(String, String)> = if !self.has.contains("activity") {
            HashSet::new()
        } else {
            let mut st = self.db.prepare("SELECT COALESCE(subject, ''), COALESCE(text, '') FROM activity WHERE kind = 'dismiss_gap'").map_err(store)?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(store)?.collect::<Result<_, _>>().map_err(store)?
        };
        let findings = self.findings()?;
        // The latest hand-over wins (rows in the order they were logged).
        let pursued: HashMap<(String, String), String> = if !self.has.contains("activity") {
            HashMap::new()
        } else {
            let mut st = self
                .db
                .prepare("SELECT COALESCE(subject, ''), COALESCE(text, ''), at FROM activity WHERE kind = 'pursue_gap' ORDER BY rowid")
                .map_err(store)?;
            st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?))).map_err(store)?.collect::<Result<_, _>>().map_err(store)?
        };
        let mut st = self
            .db
            .prepare(
                "SELECT topic, kind, rank, score, headline, detail, terms, works, threads FROM research_gaps
                 WHERE ?1 IS NULL OR topic = ?1 ORDER BY topic, kind, rank",
            )
            .map_err(store)?;
        let rows: Vec<GapRow> = st
            .query_map(params![topic], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))
            .map_err(store)?
            .collect::<Result<_, _>>()
            .map_err(store)?;
        let mut out: Vec<StoredGap> = Vec::new();
        for (subject, kind, rank, score, headline, detail, terms, works, threads) in rows {
            let Some(kind) = gaps::Kind::parse(&kind) else { continue };
            let ids: Vec<i64> = serde_json::from_str(&works).unwrap_or_default();
            let mut papers = Vec::new();
            for w in ids {
                papers.extend(self.paper_ref(w)?);
            }
            let dismissed = dismissed.contains(&(subject.clone(), headline.clone()));
            let pursued = pursued.get(&(subject.clone(), headline.clone())).cloned();
            let findings = findings.get(&(subject.clone(), headline.clone())).cloned().unwrap_or_default();
            out.push(StoredGap {
                subject,
                kind,
                rank: rank as usize,
                score,
                headline,
                detail,
                terms: serde_json::from_str(&terms).unwrap_or_default(),
                works: papers,
                threads: serde_json::from_str(&threads).unwrap_or_default(),
                dismissed,
                pursued,
                findings,
            });
        }
        // Kinds in the order a researcher would act on them.
        let order = |k: gaps::Kind| [gaps::Kind::Unbridged, gaps::Kind::Combination, gaps::Kind::Forgotten, gaps::Kind::Young, gaps::Kind::Stalled, gaps::Kind::BlindSpot]
            .iter()
            .position(|x| *x == k)
            .unwrap_or(9);
        out.sort_by(|a, b| a.subject.cmp(&b.subject).then(order(a.kind).cmp(&order(b.kind))).then(a.rank.cmp(&b.rank)));
        Ok(out)
    }

    /// Keep one thing the person did. Local only; nothing reads it but the hub.
    pub fn log(&mut self, a: &Activity) -> Result<(), Error> {
        self.db
            .execute(
                "INSERT INTO activity (at, kind, subject, work, text) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![a.at, a.kind, a.subject, a.work, a.text],
            )
            .map_err(store)?;
        Ok(())
    }

    /// The newest activity first.
    pub fn activity(&self, limit: usize) -> Result<Vec<Activity>, Error> {
        if !self.has.contains("activity") {
            return Ok(Vec::new());
        }
        let mut st = self.db.prepare("SELECT at, kind, subject, work, text FROM activity ORDER BY id DESC LIMIT ?1").map_err(store)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        st.query_map(params![limit], |r| Ok(Activity { at: r.get(0)?, kind: r.get(1)?, subject: r.get(2)?, work: r.get(3)?, text: r.get(4)? }))
            .map_err(store)?
            .collect::<Result<_, _>>()
            .map_err(store)
    }

    #[cfg(test)]
    pub(crate) fn raw(&self, sql: &str) {
        self.db.execute_batch(sql).unwrap();
    }

    /// Papers the person saved and has not unsaved, newest save first.
    pub fn saved(&self) -> Result<Vec<PaperRef>, Error> {
        if !self.has.contains("activity") {
            return Ok(Vec::new());
        }
        let mut st = self.db.prepare("SELECT work, kind FROM activity WHERE kind IN ('save', 'unsave') AND work IS NOT NULL ORDER BY id").map_err(store)?;
        let events: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(store)?.collect::<Result<_, _>>().map_err(store)?;
        let mut order: Vec<i64> = Vec::new();
        for (w, kind) in events {
            order.retain(|&x| x != w);
            if kind == "save" {
                order.push(w);
            }
        }
        let mut out = Vec::new();
        for w in order.into_iter().rev() {
            out.extend(self.paper_ref(w)?);
        }
        Ok(out)
    }

    /// Papers the person opened or saved: the hub does not suggest them again.
    pub fn seen_works(&self) -> Result<HashSet<i64>, Error> {
        if !self.has.contains("activity") {
            return Ok(HashSet::new());
        }
        let mut st = self.db.prepare("SELECT DISTINCT work FROM activity WHERE kind IN ('open', 'save') AND work IS NOT NULL").map_err(store)?;
        st.query_map([], |r| r.get(0)).map_err(store)?.collect::<Result<_, _>>().map_err(store)
    }
}

/// Insert a paper, or update the row that already holds any of its identifiers. Returns the row id.
fn upsert(s: &mut Writes, tx: &rusqlite::Transaction, w: &Work) -> Result<i64, Error> {
    let mut existing = None;
    for id in &w.ids {
        existing = s.find.query_row(params![id.scheme(), id.value()], |r| r.get::<_, i64>(0)).optional().map_err(store)?;
        if existing.is_some() {
            break;
        }
    }
    let authors = serde_json::to_string(&w.authors).unwrap_or_else(|_| "[]".into());
    let cited = w.cited_by_count.map(|c| c as i64);
    let row = match existing {
        Some(row) => {
            s.update.execute(params![row, w.title, w.abstract_text, w.year, authors, w.venue, w.open_url, cited]).map_err(store)?;
            s.fts_drop.execute(params![row]).map_err(store)?;
            row
        }
        None => {
            s.insert.execute(params![w.title, w.abstract_text, w.year, authors, w.venue, w.open_url, cited]).map_err(store)?;
            tx.last_insert_rowid()
        }
    };
    let (title, abs): (String, Option<String>) = s.read_back.query_row(params![row], |r| Ok((r.get(0)?, r.get(1)?))).map_err(store)?;
    s.fts_add.execute(params![row, title, abs]).map_err(store)?;
    for id in &w.ids {
        s.id.execute(params![id.scheme(), id.value(), row]).map_err(store)?;
    }
    for id in &w.references {
        s.reference.execute(params![row, id.scheme(), id.value()]).map_err(store)?;
    }
    for src in &w.seen_in {
        s.seen.execute(params![row, src.name()]).map_err(store)?;
    }
    Ok(row)
}

impl Stop {
    pub fn name(self) -> &'static str {
        match self {
            Stop::Closed => "closed",
            Stop::Budget => "budget",
            Stop::Rounds => "rounds",
            Stop::Throttled => "throttled",
            Stop::KeyRefused => "key_refused",
            Stop::Cancelled => "cancelled",
            Stop::InProgress => "in_progress",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snowball::{Policy, Topic};

    fn run_with(works: Vec<Work>) -> (Snowball, Report) {
        let topic = Topic { name: "lora".into(), queries: vec!["q".into()], phrases: vec!["low rank".into(), "adapter|adapters".into()] };
        let mut s = Snowball::new(topic.clone(), Policy::for_topic(&topic));
        for w in works {
            s.archive.insert(w);
        }
        // No index: evaluate the inserted papers only.
        let r = s.run(&mut []);
        (s, r)
    }

    fn at(text: &str, secs: i64) -> Stamp {
        Stamp { text: text.into(), secs }
    }

    fn paper(id: &str, title: &str, abs: &str) -> Work {
        Work {
            ids: [Id::openalex(id).unwrap()].into(),
            title: title.into(),
            abstract_text: Some(abs.into()),
            year: Some(2021),
            authors: vec!["A. Author".into()],
            references: [Id::OpenAlex("W99".into())].into(),
            seen_in: [Source::OpenAlex].into(),
            ..Work::default()
        }
    }

    #[test]
    fn save_search_and_read_back() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter"), paper("W2", "Weather", "rain and wind")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("2026-10-08T00:00:00Z", 100)).unwrap();

        let hits = st.search("adapter", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].ids, vec![Id::OpenAlex("W1".into())]);
        // The unaccepted paper is archived and searchable, but not a member of the subject.
        assert_eq!(st.search("rain", None, 10).unwrap().len(), 1);
        assert_eq!(st.search("rain", Some("lora"), 10).unwrap().len(), 0);

        let members = st.members("lora").unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].reason, "text:2");
        // Put in directly, so found by neither method.
        assert_eq!(members[0].via, "none");
        assert_eq!(members[0].work.references.len(), 1);
        assert_eq!(members[0].work.seen_in.len(), 1);

        let topics = st.topics().unwrap();
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].members, 1);
        assert_eq!(topics[0].phrases, vec!["low rank", "adapter|adapters"]);
        let last = topics[0].last.as_ref().unwrap();
        assert_eq!(last.accepted, 1);
        assert_eq!(last.archived, 2);
        assert!(last.missing.starts_with("UNMEASURED"), "{}", last.missing);
    }

    #[test]
    fn all_works_round_trips_every_paper() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter"), paper("W2", "Weather", "rain")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("t", 100)).unwrap();
        let all = st.all_works().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].ids, [Id::OpenAlex("W2".into())].into());
        assert_eq!(all[1].references.len(), 1);
        assert_eq!(all[0].seen_in, [Source::OpenAlex].into());
    }

    #[test]
    fn saving_twice_updates_rather_than_duplicates_and_keeps_both_runs() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("t1", 100)).unwrap();
        st.save(&run, &report, &at("t2", 200)).unwrap();
        assert_eq!(st.search("adapter", None, 10).unwrap().len(), 1);
        let n: i64 = st.db.query_row("SELECT COUNT(*) FROM works", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let runs = st.runs("lora", 10).unwrap();
        assert_eq!(runs.iter().map(|r| r.finished_at.as_str()).collect::<Vec<_>>(), vec!["t2", "t1"]);
    }

    #[test]
    fn deleting_a_subject_keeps_its_papers() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("t", 100)).unwrap();
        st.delete_topic("lora").unwrap();
        assert!(st.topics().unwrap().is_empty());
        assert!(st.runs("lora", 10).unwrap().is_empty());
        assert_eq!(st.search("adapter", None, 10).unwrap().len(), 1);
    }

    #[test]
    fn the_graph_is_saved_queried_exported_and_forgotten() {
        let mut a = paper("W1", "Low-rank adapter tuning", "a low rank adapter");
        a.references.clear();
        let mut b = paper("W2", "Low-rank adapter merging", "a low rank adapter");
        b.references = [Id::OpenAlex("W1".into())].into();
        let mut c = paper("W3", "Low-rank adapter pruning", "a low rank adapter");
        c.references = [Id::OpenAlex("W2".into())].into();
        let (run, report) = run_with(vec![a, b, c]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("t", 100)).unwrap();
        let (nodes, edges, _) = st.rebuild_graph("lora").unwrap();
        assert_eq!(nodes, 3);
        assert!(edges >= 2);
        let w = |id: &str| st.find_work(id).unwrap().unwrap();
        assert_eq!(st.find_work("openalex:W2").unwrap(), Some(w("W2")));
        assert_eq!(st.find_work("https://openalex.org/W3").unwrap(), Some(w("W3")));
        assert_eq!(st.find_work("W404").unwrap(), None);
        let n = st.neighbourhood("lora", w("W2")).unwrap().unwrap();
        assert_eq!(n.cites.iter().map(|p| p.work).collect::<Vec<_>>(), vec![w("W1")]);
        assert_eq!(n.cited_by.iter().map(|p| p.work).collect::<Vec<_>>(), vec![w("W3")]);
        assert_eq!(n.of, 3);
        let chain = st.connection("lora", w("W3"), w("W1")).unwrap().unwrap();
        assert_eq!(chain.iter().map(|(p, _)| p.work).collect::<Vec<_>>(), vec![w("W3"), w("W2"), w("W1")]);
        assert_eq!(chain.iter().map(|(_, how)| *how).collect::<Vec<_>>(), vec!["", "cites", "cites"]);
        let back = st.connection("lora", w("W1"), w("W3")).unwrap().unwrap();
        assert_eq!(back[1].1, "is cited by");
        let json = st.graph_json("lora").unwrap();
        assert_eq!(json["nodes"].as_array().unwrap().len(), 3);
        assert!(st.graph_ml("lora").unwrap().contains("<edge source="));
        // Rebuilding replaces rather than adds.
        st.rebuild_graph("lora").unwrap();
        assert_eq!(st.graph("lora").unwrap().nodes.len(), 3);
        st.delete_topic("lora").unwrap();
        assert!(st.graph("lora").unwrap().nodes.is_empty());
    }

    #[test]
    fn activity_is_kept_and_saves_can_be_undone() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter"), paper("W2", "Low-rank adapter merging", "a low rank adapter")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("t", 100)).unwrap();
        let (a, b) = (st.find_work("W1").unwrap().unwrap(), st.find_work("W2").unwrap().unwrap());
        let act = |kind: &str, work: Option<i64>| Activity { at: "t".into(), kind: kind.into(), subject: Some("lora".into()), work, text: None };
        st.log(&act("save", Some(a))).unwrap();
        st.log(&act("save", Some(b))).unwrap();
        st.log(&act("unsave", Some(a))).unwrap();
        assert_eq!(st.saved().unwrap().iter().map(|p| p.work).collect::<Vec<_>>(), vec![b]);
        assert!(st.seen_works().unwrap().contains(&a), "a paper once saved was seen");
        assert_eq!(st.activity(10).unwrap().len(), 3);
        // A dismissed gap stays saved but is marked.
        st.db.execute("INSERT INTO research_gaps VALUES ('lora','young',0,1.0,'h','d','[]','[]','[]')", []).unwrap();
        st.log(&Activity { at: "t".into(), kind: "dismiss_gap".into(), subject: Some("lora".into()), work: None, text: Some("h".into()) }).unwrap();
        assert!(st.gaps(Some("lora")).unwrap()[0].dismissed);
        // A gap handed to Sinai is marked with the latest hand-over's time.
        assert_eq!(st.gaps(Some("lora")).unwrap()[0].pursued, None);
        for at in ["t1", "t2"] {
            st.log(&Activity { at: at.into(), kind: "pursue_gap".into(), subject: Some("lora".into()), work: None, text: Some("h".into()) }).unwrap();
        }
        assert_eq!(st.gaps(Some("lora")).unwrap()[0].pursued.as_deref(), Some("t2"));
        // A finding is kept beside its gap, newest first; one for a gap the archive does not hold is refused.
        let f = |at: &str, verdict: &str| Finding {
            at: at.into(),
            by: "Sinai".into(),
            verdict: verdict.into(),
            why: "w".into(),
            next_step: "n".into(),
            sources: vec!["https://doi.org/10.1/x".into()],
        };
        st.save_finding("lora", "h", &f("t3", "unclear")).unwrap();
        st.save_finding("lora", "h", &f("t4", "real")).unwrap();
        let g = &st.gaps(Some("lora")).unwrap()[0];
        assert_eq!(g.findings.iter().map(|x| x.at.as_str()).collect::<Vec<_>>(), ["t4", "t3"]);
        assert_eq!(g.findings[0].sources, ["https://doi.org/10.1/x"]);
        assert!(st.save_finding("lora", "no such gap", &f("t5", "real")).is_err());
        assert!(st.save_finding("lora", "h", &f("t5", "maybe")).is_err(), "a verdict outside the list");
        let mut long = f("t5", "real");
        long.why = "x".repeat(FINDING_LIMITS.why + 1);
        assert!(st.save_finding("lora", "h", &long).is_err(), "over the limit is refused, not cut");
        let mut empty = f("t5", "real");
        empty.why = " ".into();
        assert!(st.save_finding("lora", "h", &empty).is_err());
        assert_eq!(st.gaps(Some("lora")).unwrap()[0].findings.len(), 2, "nothing refused was kept");
    }

    #[test]
    fn members_keep_when_they_joined_and_watched_subjects_fall_due() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("first", 1_000)).unwrap();
        // A later run accepts one more paper; the first keeps its joining time.
        let (run2, report2) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter"), paper("W2", "Low-rank adapter merging", "a low rank adapter")]);
        st.save(&run2, &report2, &at("second", 5_000)).unwrap();
        let fresh = st.new_papers(Some("lora"), 0, 10).unwrap();
        assert_eq!(fresh.iter().map(|n| n.paper.title.as_str()).collect::<Vec<_>>(), vec!["Low-rank adapter merging"]);
        assert_eq!(fresh[0].added_at, 5_000);
        assert!(st.new_papers(Some("lora"), 6_000, 10).unwrap().is_empty());

        assert!(st.due(10_000_000).unwrap().is_empty(), "a subject is not watched until the person says so");
        st.set_watch("lora", Watch::Daily).unwrap();
        assert!(st.due(5_000 + 86_399).unwrap().is_empty());
        assert_eq!(st.due(5_000 + 86_400).unwrap(), vec!["lora"]);
        let t = &st.topics().unwrap()[0];
        assert_eq!((t.watch, t.watched_at), (Watch::Daily, Some(5_000)));
        assert_eq!(t.last_full.as_ref().map(|r| r.kind.as_str()), Some("full"));
    }

    #[test]
    fn an_archive_from_before_these_columns_is_migrated_in_place() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE topics (name TEXT PRIMARY KEY, queries TEXT NOT NULL, phrases TEXT NOT NULL);
             INSERT INTO topics VALUES ('old', '[]', '[]');
             CREATE TABLE members (topic TEXT NOT NULL, work INTEGER NOT NULL, reason TEXT NOT NULL, via TEXT NOT NULL, PRIMARY KEY (topic, work));
             CREATE TABLE runs (id INTEGER PRIMARY KEY, topic TEXT NOT NULL, finished_at TEXT NOT NULL, stop TEXT NOT NULL, halt_reason TEXT,
               calls INTEGER NOT NULL, accepted INTEGER NOT NULL, archived INTEGER NOT NULL, by_search INTEGER NOT NULL,
               by_citation INTEGER NOT NULL, by_both INTEGER NOT NULL, missing TEXT NOT NULL);",
        )
        .unwrap();
        let st = Store::init(db).unwrap();
        let t = &st.topics().unwrap()[0];
        assert_eq!((t.name.as_str(), t.watch, t.watched_at), ("old", Watch::Off, None));
    }

    #[test]
    fn checkpoints_keep_one_row_that_the_outcome_replaces() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter")]);
        let mut st = Store::in_memory().unwrap();
        let progress = run.report(Stop::InProgress);
        st.save(&run, &progress, &at("cp1", 10)).unwrap();
        st.save(&run, &progress, &at("cp2", 20)).unwrap();
        let runs = st.runs("lora", 10).unwrap();
        assert_eq!(runs.iter().map(|r| (r.finished_at.as_str(), r.stop.as_str())).collect::<Vec<_>>(), vec![("cp2", "in_progress")]);
        st.save(&run, &report, &at("done", 30)).unwrap();
        let runs = st.runs("lora", 10).unwrap();
        assert_eq!(runs.iter().map(|r| r.stop.as_str()).collect::<Vec<_>>(), vec!["closed"]);
        assert_eq!(st.members("lora").unwrap().len(), 1);
    }

    #[test]
    fn an_archive_from_before_watch_mode_reads_read_only_without_migration() {
        // Seen live 2026-10-09: the MCP server opened an archive made before watch mode's columns read-only, and
        // every list failed with "no such column: t.watch".
        let dir = std::env::temp_dir().join(format!("alelyon-research-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("archive.sqlite");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE works (id INTEGER PRIMARY KEY, title TEXT NOT NULL, abstract TEXT, year INTEGER, authors TEXT NOT NULL,
                   venue TEXT, open_url TEXT, cited_by_count INTEGER);
                 CREATE TABLE ids (scheme TEXT NOT NULL, value TEXT NOT NULL, work INTEGER NOT NULL, PRIMARY KEY (scheme, value));
                 CREATE TABLE topics (name TEXT PRIMARY KEY, queries TEXT NOT NULL, phrases TEXT NOT NULL);
                 INSERT INTO topics VALUES ('old', '[\"q\"]', '[\"c\"]');
                 CREATE TABLE members (topic TEXT NOT NULL, work INTEGER NOT NULL, reason TEXT NOT NULL, via TEXT NOT NULL, PRIMARY KEY (topic, work));
                 CREATE TABLE runs (id INTEGER PRIMARY KEY, topic TEXT NOT NULL, finished_at TEXT NOT NULL, stop TEXT NOT NULL, halt_reason TEXT,
                   calls INTEGER NOT NULL, accepted INTEGER NOT NULL, archived INTEGER NOT NULL, by_search INTEGER NOT NULL,
                   by_citation INTEGER NOT NULL, by_both INTEGER NOT NULL, missing TEXT NOT NULL);
                 INSERT INTO runs (topic, finished_at, stop, halt_reason, calls, accepted, archived, by_search, by_citation, by_both, missing)
                   VALUES ('old', 't', 'budget', NULL, 5, 1, 2, 1, 0, 0, 'UNMEASURED');",
            )
            .unwrap();
        }
        let st = Store::open_read_only(&path).unwrap();
        let t = &st.topics().unwrap()[0];
        assert_eq!((t.name.as_str(), t.watch, t.watched_at), ("old", Watch::Off, None));
        assert_eq!(t.last.as_ref().map(|r| r.kind.as_str()), Some("full"));
        assert!(st.new_papers(None, 0, 10).unwrap().is_empty());
        assert!(st.gaps(None).unwrap().is_empty());
        assert!(st.activity(5).unwrap().is_empty() && st.saved().unwrap().is_empty() && st.seen_works().unwrap().is_empty());
        assert!(st.graph("old").unwrap().nodes.is_empty() && st.communities("old").unwrap().is_empty());
        assert_eq!(st.neighbourhood("old", 1).unwrap(), None);
        assert!(crate::hub::build(&st, 0).is_ok(), "the hub reads an old archive too");
        drop(st);
        // Read-only means read-only: the archive was not migrated.
        let db = Connection::open(&path).unwrap();
        let cols: Vec<String> = db.prepare("PRAGMA table_info(topics)").unwrap().query_map([], |r| r.get(1)).unwrap().map(Result::unwrap).collect();
        assert!(!cols.contains(&"watch".to_string()));
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_subject_is_copied_whole_and_merges_with_papers_already_held() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter"), paper("W2", "Low-rank adapter merging", "a low rank adapter")]);
        let mut from = Store::in_memory().unwrap();
        from.save(&run, &report, &at("then", 10)).unwrap();
        let mut to = Store::in_memory().unwrap();
        // The destination already holds W1 (under another subject): it is merged, not duplicated.
        let mut t2 = Topic { name: "other".into(), queries: vec![], phrases: vec!["low rank".into(), "adapter".into()] };
        t2.queries.push("q".into());
        let mut other = Snowball::new(t2.clone(), Policy::for_topic(&t2));
        other.archive.insert(paper("W1", "Low-rank adapter tuning", "a low rank adapter"));
        let r2 = other.run(&mut []);
        to.save(&other, &r2, &at("x", 5)).unwrap();
        assert_eq!(to.import_subject(&from, "lora", &at("now", 20)).unwrap(), 2);
        let n: i64 = to.db.query_row("SELECT COUNT(*) FROM works", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "W1 merged with the copy already held");
        let t = to.topics().unwrap().into_iter().find(|t| t.name == "lora").unwrap();
        assert_eq!((t.members, t.queries.clone()), (2, vec!["q".to_string()]));
        assert_eq!(t.last.as_ref().map(|r| r.stop.as_str()), Some("closed"));
        assert!(!to.graph("lora").unwrap().nodes.is_empty(), "its graph is built here");
        assert!(to.import_subject(&from, "lora", &at("again", 30)).is_err(), "an existing name is refused");
    }

    #[test]
    fn plain_words_cannot_inject_fts_syntax() {
        let (run, report) = run_with(vec![paper("W1", "Low-rank adapter tuning", "a low rank adapter")]);
        let mut st = Store::in_memory().unwrap();
        st.save(&run, &report, &at("t", 100)).unwrap();
        assert_eq!(words_query("  "), None);
        // Raw, these are FTS5 syntax errors; quoted, they are just words.
        for text in ["low-rank", "adapter OR", "\"adapter", "adapter NEAR(", "title:adapter"] {
            assert!(st.search(&words_query(text).unwrap(), None, 10).is_ok(), "{text}");
        }
        assert_eq!(st.search(&words_query("low-rank adapter").unwrap(), None, 10).unwrap().len(), 1);
    }
}
