//! What agents keep from research, so a question already worked out costs a lookup instead of a new reading.
//!
//! The archive (`store`) holds papers as the indexes gave them. Beside it, `knowledge.sqlite` holds what agents made of
//! them, each record naming who kept it and when:
//!
//! - **Facts**: one short claim from one archived paper (a method, a result, a number, a limit, a definition), with the
//!   conditions it holds under and the concepts it is about.
//! - **Concepts and relations**: named ideas (with their other spellings) and how they relate (`is_a`, `part_of`,
//!   `uses`, `improves_on`, `contradicts`, `compares_with`, `same_as`), each relation with the paper that shows it.
//! - **Answers**: a question an agent answered, the answer, and the facts and sources it rests on; a close repeat of
//!   the question finds it.
//! - **Notes**: what an agent concluded while working, with its sources.
//! - **Gathering requests**: what an agent could not find; a new gathering spends research credits, so it waits for
//!   the person to approve it on the Research page.
//!
//! [`Knowledge::look`] answers from all of these at once. Nothing here fetches anything.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS facts (
    id INTEGER PRIMARY KEY,
    work INTEGER NOT NULL,      -- the archive's row for the paper
    paper TEXT NOT NULL,        -- its title, kept so a reading needs no archive
    subject TEXT,
    kind TEXT NOT NULL,         -- see FACT_KINDS
    text TEXT NOT NULL,
    conditions TEXT NOT NULL,
    key TEXT NOT NULL,          -- the text's words, for spotting a repeat
    by TEXT NOT NULL,
    at TEXT NOT NULL,
    UNIQUE (work, key)
);
CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(text, conditions, paper);
CREATE TABLE IF NOT EXISTS concepts (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    key TEXT NOT NULL UNIQUE,
    aliases TEXT NOT NULL       -- JSON array of other spellings
);
CREATE TABLE IF NOT EXISTS fact_concepts (
    fact INTEGER NOT NULL REFERENCES facts(id) ON DELETE CASCADE,
    concept INTEGER NOT NULL REFERENCES concepts(id) ON DELETE CASCADE,
    PRIMARY KEY (fact, concept)
);
CREATE TABLE IF NOT EXISTS relations (
    id INTEGER PRIMARY KEY,
    a INTEGER NOT NULL REFERENCES concepts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,         -- see RELATIONS
    b INTEGER NOT NULL REFERENCES concepts(id) ON DELETE CASCADE,
    work INTEGER,               -- the paper that shows it, when one does
    paper TEXT,
    note TEXT NOT NULL,
    by TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS relations_once ON relations(a, kind, b, COALESCE(work, -1));
CREATE TABLE IF NOT EXISTS answers (
    id INTEGER PRIMARY KEY,
    question TEXT NOT NULL,
    key TEXT NOT NULL,
    answer TEXT NOT NULL,
    facts TEXT NOT NULL,        -- JSON array of fact ids
    sources TEXT NOT NULL,      -- JSON array
    subject TEXT,
    by TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS answers_fts USING fts5(question, answer);
CREATE TABLE IF NOT EXISTS notes (
    id INTEGER PRIMARY KEY,
    about TEXT NOT NULL,
    text TEXT NOT NULL,
    sources TEXT NOT NULL,      -- JSON array
    subject TEXT,
    by TEXT NOT NULL,
    at TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(about, text);
CREATE TABLE IF NOT EXISTS gather_requests (
    id INTEGER PRIMARY KEY,
    query TEXT NOT NULL,
    key TEXT NOT NULL,
    subject TEXT,
    why TEXT NOT NULL,
    by TEXT NOT NULL,
    at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'waiting',  -- waiting | approved | dismissed
    decided_at TEXT
);
";

/// What a fact can be.
pub const FACT_KINDS: &[&str] = &["method", "result", "number", "limit", "definition", "claim"];
/// How two concepts can relate (`a <kind> b`).
pub const RELATIONS: &[&str] = &["is_a", "part_of", "uses", "improves_on", "contradicts", "compares_with", "same_as"];
/// A gathering request's states.
pub const REQUEST_STATES: &[&str] = &["waiting", "approved", "dismissed"];

/// The longest each field may be, in characters (and the most items in a list).
pub struct Limits {
    pub text: usize,
    pub conditions: usize,
    pub question: usize,
    pub answer: usize,
    pub about: usize,
    pub note: usize,
    pub why: usize,
    pub source: usize,
    pub sources: usize,
    pub by: usize,
    pub concept: usize,
    pub concepts: usize,
}

pub const LIMITS: Limits = Limits {
    text: 600,
    conditions: 300,
    question: 300,
    answer: 4000,
    about: 200,
    note: 2000,
    why: 600,
    source: 300,
    sources: 12,
    by: 80,
    concept: 80,
    concepts: 12,
};

/// A close repeat: this share of a question's words (of both questions' together) matches a kept one.
pub const REPEAT: f64 = 0.6;

/// `knowledge.sqlite` beside an archive file.
pub fn beside(archive: &Path) -> PathBuf {
    archive.with_file_name("knowledge.sqlite")
}

/// A paper of the archive, as a fact or relation cites it.
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    pub work: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub id: i64,
    pub paper: Paper,
    pub subject: Option<String>,
    pub kind: String,
    pub text: String,
    pub conditions: String,
    pub concepts: Vec<String>,
    pub by: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    pub id: i64,
    pub from: String,
    pub kind: String,
    pub to: String,
    pub paper: Option<Paper>,
    pub note: String,
    pub by: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Concept {
    pub id: i64,
    pub name: String,
    pub aliases: Vec<String>,
    pub relations: Vec<Relation>,
    pub facts: Vec<Fact>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub id: i64,
    pub question: String,
    pub answer: String,
    pub facts: Vec<i64>,
    pub sources: Vec<String>,
    pub subject: Option<String>,
    pub by: String,
    pub at: String,
    /// How closely it matches the question asked (1 = the same words).
    pub matched: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: i64,
    pub about: String,
    pub text: String,
    pub sources: Vec<String>,
    pub subject: Option<String>,
    pub by: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: i64,
    pub query: String,
    pub subject: Option<String>,
    pub why: String,
    pub by: String,
    pub at: String,
    pub status: String,
    pub decided_at: Option<String>,
}

/// Everything kept that bears on a question.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    /// Questions asked before whose words closely match, closest first.
    pub answers: Vec<Answer>,
    pub facts: Vec<Fact>,
    /// Concepts the question names, with their relations and facts.
    pub concepts: Vec<Concept>,
    pub notes: Vec<Note>,
}

impl Look {
    pub fn is_empty(&self) -> bool {
        self.answers.is_empty() && self.facts.is_empty() && self.concepts.is_empty() && self.notes.is_empty()
    }
}

/// The newest answers, facts and notes, newest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recent {
    pub answers: Vec<Answer>,
    pub facts: Vec<Fact>,
    pub notes: Vec<Note>,
}

/// How many of each record are kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Counts {
    pub facts: i64,
    pub concepts: i64,
    pub relations: i64,
    pub answers: i64,
    pub notes: i64,
    pub waiting: i64,
}

/// A record [`Knowledge::forget`] removes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Fact,
    Relation,
    Answer,
    Note,
}

const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "has", "have", "how", "in", "is",
    "it", "its", "of", "on", "or", "than", "that", "the", "their", "there", "this", "to", "was", "what", "when", "which",
    "who", "why", "with", "were", "will", "would", "about", "into", "any", "there", "these", "those", "vs", "versus",
];

/// The lowercase words of a text, without the commonest ones, each once, in order.
pub fn keywords(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w) && (w.chars().count() > 1 || w.chars().all(|c| c.is_ascii_digit())))
        .filter(|w| seen.insert(w.to_string()))
        .map(str::to_string)
        .collect()
}

/// A name's words, all of them, for matching a concept inside a sentence.
fn spelling(text: &str) -> String {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

fn key_of(text: &str) -> String {
    let mut words = keywords(text);
    words.sort();
    words.join(" ")
}

/// Any of the words, for the full-text index.
fn any_of(words: &[String]) -> Option<String> {
    (!words.is_empty()).then(|| words.iter().map(|w| format!("\"{}\"", w.replace('"', "\"\""))).collect::<Vec<_>>().join(" OR "))
}

/// Any of a text's [`keywords`], as a full-text query (the archive's index reads it too).
pub fn any_words(text: &str) -> Option<String> {
    any_of(&keywords(text))
}

fn overlap(a: &str, b: &str) -> f64 {
    let a: BTreeSet<&str> = a.split(' ').filter(|w| !w.is_empty()).collect();
    let b: BTreeSet<&str> = b.split(' ').filter(|w| !w.is_empty()).collect();
    let union = a.union(&b).count();
    if union == 0 { 0.0 } else { a.intersection(&b).count() as f64 / union as f64 }
}

/// One line of text: trimmed, not empty, at most `max` characters, no control characters.
pub fn line(field: &str, text: &str, max: usize) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(format!("{field} is empty"));
    }
    if t.chars().count() > max {
        return Err(format!("{field} is longer than {max} characters"));
    }
    if t.chars().any(char::is_control) {
        return Err(format!("{field} must be one line"));
    }
    Ok(t.to_string())
}

/// Text that may run over lines: as [`line`], but newlines and tabs are kept.
pub fn paragraphs(field: &str, text: &str, max: usize) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(format!("{field} is empty"));
    }
    if t.chars().count() > max {
        return Err(format!("{field} is longer than {max} characters"));
    }
    if t.chars().any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r') {
        return Err(format!("{field} has control characters"));
    }
    Ok(t.to_string())
}

fn optional(field: &str, text: Option<&str>, max: usize) -> Result<String, String> {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => line(field, t, max),
        None => Ok(String::new()),
    }
}

fn sources(list: &[String]) -> Result<Vec<String>, String> {
    if list.len() > LIMITS.sources {
        return Err(format!("at most {} sources", LIMITS.sources));
    }
    list.iter().map(|s| line("a source", s, LIMITS.source)).collect()
}

fn who(by: &str) -> Result<String, String> {
    if by.trim().is_empty() { Ok("an agent".into()) } else { line("by", by, LIMITS.by) }
}

fn sql(e: rusqlite::Error) -> String {
    format!("the knowledge store: {e}")
}

fn list_of(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json).unwrap_or_default()
}

pub struct Knowledge {
    db: Connection,
}

impl Knowledge {
    /// Open for reading and writing, made when missing.
    pub fn open(path: &Path) -> Result<Knowledge, String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("the knowledge store's folder: {e}"))?;
        }
        let db = Connection::open(path).map_err(sql)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(sql)?;
        db.execute_batch(SCHEMA).map_err(sql)?;
        Ok(Knowledge { db })
    }

    /// Open for reading only; `None` when nothing has been kept yet.
    pub fn open_read_only(path: &Path) -> Result<Option<Knowledge>, String> {
        if !path.is_file() {
            return Ok(None);
        }
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(sql)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(sql)?;
        Ok(Some(Knowledge { db }))
    }

    /// Keep a fact from a paper. A fact with the same words from the same paper is kept once: its id comes back with
    /// `false`.
    #[allow(clippy::too_many_arguments)]
    pub fn remember_fact(
        &mut self,
        paper: &Paper,
        subject: Option<&str>,
        kind: &str,
        text: &str,
        conditions: Option<&str>,
        concepts: &[String],
        by: &str,
        at: &str,
    ) -> Result<(i64, bool), String> {
        if !FACT_KINDS.contains(&kind) {
            return Err(format!("kind must be one of {}", FACT_KINDS.join(", ")));
        }
        let text = line("text", text, LIMITS.text)?;
        let conditions = optional("conditions", conditions, LIMITS.conditions)?;
        if concepts.len() > LIMITS.concepts {
            return Err(format!("at most {} concepts", LIMITS.concepts));
        }
        let names = concepts.iter().map(|c| line("a concept", c, LIMITS.concept)).collect::<Result<Vec<_>, _>>()?;
        let by = who(by)?;
        let key = key_of(&text);
        if key.is_empty() {
            return Err("text has no words".into());
        }
        let tx = self.db.transaction().map_err(sql)?;
        let had: Option<i64> =
            tx.query_row("SELECT id FROM facts WHERE work = ?1 AND key = ?2", params![paper.work, key], |r| r.get(0)).optional().map_err(sql)?;
        let (id, new) = match had {
            Some(id) => (id, false),
            None => {
                tx.execute(
                    "INSERT INTO facts (work, paper, subject, kind, text, conditions, key, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![paper.work, paper.title, subject, kind, text, conditions, key, by, at],
                )
                .map_err(sql)?;
                let id = tx.last_insert_rowid();
                tx.execute("INSERT INTO facts_fts (rowid, text, conditions, paper) VALUES (?1, ?2, ?3, ?4)", params![id, text, conditions, paper.title])
                    .map_err(sql)?;
                (id, true)
            }
        };
        for name in &names {
            let concept = concept_in(&tx, name, &[])?;
            tx.execute("INSERT OR IGNORE INTO fact_concepts (fact, concept) VALUES (?1, ?2)", params![id, concept]).map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok((id, new))
    }

    /// A concept by name, made when missing; `aliases` are added to its other spellings.
    pub fn concept_id(&mut self, name: &str, aliases: &[String]) -> Result<i64, String> {
        let tx = self.db.transaction().map_err(sql)?;
        let id = concept_in(&tx, name, aliases)?;
        tx.commit().map_err(sql)?;
        Ok(id)
    }

    /// Keep that `from <kind> to`, shown by `paper` when one does. The same relation from the same paper is kept once.
    #[allow(clippy::too_many_arguments)]
    pub fn relate(
        &mut self,
        from: &str,
        kind: &str,
        to: &str,
        paper: Option<&Paper>,
        note: Option<&str>,
        by: &str,
        at: &str,
    ) -> Result<(i64, bool), String> {
        if !RELATIONS.contains(&kind) {
            return Err(format!("relation must be one of {}", RELATIONS.join(", ")));
        }
        let note = optional("note", note, LIMITS.why)?;
        let by = who(by)?;
        let tx = self.db.transaction().map_err(sql)?;
        let a = concept_in(&tx, from, &[])?;
        let b = concept_in(&tx, to, &[])?;
        if a == b {
            return Err("a concept cannot relate to itself".into());
        }
        let work = paper.map(|p| p.work);
        let had: Option<i64> = tx
            .query_row(
                "SELECT id FROM relations WHERE a = ?1 AND kind = ?2 AND b = ?3 AND COALESCE(work, -1) = COALESCE(?4, -1)",
                params![a, kind, b, work],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        let out = match had {
            Some(id) => (id, false),
            None => {
                tx.execute(
                    "INSERT INTO relations (a, kind, b, work, paper, note, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![a, kind, b, work, paper.map(|p| p.title.as_str()), note, by, at],
                )
                .map_err(sql)?;
                (tx.last_insert_rowid(), true)
            }
        };
        tx.commit().map_err(sql)?;
        Ok(out)
    }

    /// Keep an answer to a question. It must rest on kept facts or name its sources.
    #[allow(clippy::too_many_arguments)]
    pub fn keep_answer(
        &mut self,
        question: &str,
        answer: &str,
        facts: &[i64],
        source_list: &[String],
        subject: Option<&str>,
        by: &str,
        at: &str,
    ) -> Result<i64, String> {
        let question = line("question", question, LIMITS.question)?;
        let answer = paragraphs("answer", answer, LIMITS.answer)?;
        let source_list = sources(source_list)?;
        if facts.is_empty() && source_list.is_empty() {
            return Err("an answer needs the facts it rests on or its sources".into());
        }
        for f in facts {
            let known: Option<i64> = self.db.query_row("SELECT id FROM facts WHERE id = ?1", params![f], |r| r.get(0)).optional().map_err(sql)?;
            if known.is_none() {
                return Err(format!("no fact {f} is kept"));
            }
        }
        let by = who(by)?;
        let key = key_of(&question);
        if key.is_empty() {
            return Err("question has no words".into());
        }
        let tx = self.db.transaction().map_err(sql)?;
        tx.execute(
            "INSERT INTO answers (question, key, answer, facts, sources, subject, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![question, key, answer, serde_json::to_string(facts).unwrap_or_default(), serde_json::to_string(&source_list).unwrap_or_default(), subject, by, at],
        )
        .map_err(sql)?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO answers_fts (rowid, question, answer) VALUES (?1, ?2, ?3)", params![id, question, answer]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(id)
    }

    /// Keep what an agent concluded while working.
    pub fn note(&mut self, about: &str, text: &str, source_list: &[String], subject: Option<&str>, by: &str, at: &str) -> Result<i64, String> {
        let about = line("about", about, LIMITS.about)?;
        let text = paragraphs("text", text, LIMITS.note)?;
        let source_list = sources(source_list)?;
        let by = who(by)?;
        let tx = self.db.transaction().map_err(sql)?;
        tx.execute(
            "INSERT INTO notes (about, text, sources, subject, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![about, text, serde_json::to_string(&source_list).unwrap_or_default(), subject, by, at],
        )
        .map_err(sql)?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO notes_fts (rowid, about, text) VALUES (?1, ?2, ?3)", params![id, about, text]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(id)
    }

    /// Ask the person for a new gathering. The same words still waiting are asked once: its id comes back with `false`.
    pub fn request_gathering(&mut self, query: &str, subject: Option<&str>, why: &str, by: &str, at: &str) -> Result<(i64, bool), String> {
        let query = line("query", query, LIMITS.question)?;
        let why = line("why", why, LIMITS.why)?;
        let by = who(by)?;
        let key = key_of(&query);
        if key.is_empty() {
            return Err("query has no words".into());
        }
        let had: Option<i64> = self
            .db
            .query_row("SELECT id FROM gather_requests WHERE key = ?1 AND status = 'waiting'", params![key], |r| r.get(0))
            .optional()
            .map_err(sql)?;
        if let Some(id) = had {
            return Ok((id, false));
        }
        self.db
            .execute(
                "INSERT INTO gather_requests (query, key, subject, why, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![query, key, subject, why, by, at],
            )
            .map_err(sql)?;
        Ok((self.db.last_insert_rowid(), true))
    }

    /// The gathering requests, newest first; only those in `status` when given.
    pub fn requests(&self, status: Option<&str>) -> Result<Vec<Request>, String> {
        let mut st = self
            .db
            .prepare("SELECT id, query, subject, why, by, at, status, decided_at FROM gather_requests WHERE ?1 IS NULL OR status = ?1 ORDER BY id DESC")
            .map_err(sql)?;
        let rows = st
            .query_map(params![status], |r| {
                Ok(Request {
                    id: r.get(0)?,
                    query: r.get(1)?,
                    subject: r.get(2)?,
                    why: r.get(3)?,
                    by: r.get(4)?,
                    at: r.get(5)?,
                    status: r.get(6)?,
                    decided_at: r.get(7)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// The person's decision on a waiting request.
    pub fn decide(&mut self, id: i64, status: &str, at: &str) -> Result<(), String> {
        if status == "waiting" || !REQUEST_STATES.contains(&status) {
            return Err("a request is approved or dismissed".into());
        }
        let n = self
            .db
            .execute("UPDATE gather_requests SET status = ?2, decided_at = ?3 WHERE id = ?1 AND status = 'waiting'", params![id, status, at])
            .map_err(sql)?;
        if n == 0 { Err(format!("no waiting request {id}")) } else { Ok(()) }
    }

    /// Remove one record (the person's choice on the Research page).
    pub fn forget(&mut self, kind: Kind, id: i64) -> Result<(), String> {
        let tx = self.db.transaction().map_err(sql)?;
        let n = match kind {
            Kind::Fact => {
                tx.execute("DELETE FROM facts_fts WHERE rowid = ?1", params![id]).map_err(sql)?;
                tx.execute("DELETE FROM facts WHERE id = ?1", params![id]).map_err(sql)?
            }
            Kind::Relation => tx.execute("DELETE FROM relations WHERE id = ?1", params![id]).map_err(sql)?,
            Kind::Answer => {
                tx.execute("DELETE FROM answers_fts WHERE rowid = ?1", params![id]).map_err(sql)?;
                tx.execute("DELETE FROM answers WHERE id = ?1", params![id]).map_err(sql)?
            }
            Kind::Note => {
                tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", params![id]).map_err(sql)?;
                tx.execute("DELETE FROM notes WHERE id = ?1", params![id]).map_err(sql)?
            }
        };
        tx.commit().map_err(sql)?;
        if n == 0 { Err(format!("nothing with id {id} is kept")) } else { Ok(()) }
    }

    /// The newest `limit` answers, facts and notes.
    pub fn recent(&self, limit: usize) -> Result<Recent, String> {
        let mut st = self.db.prepare("SELECT id, question, answer, facts, sources, subject, by, at FROM answers ORDER BY id DESC LIMIT ?1").map_err(sql)?;
        let answers = st
            .query_map(params![limit as i64], |r| {
                Ok(Answer {
                    id: r.get(0)?,
                    question: r.get(1)?,
                    answer: r.get(2)?,
                    facts: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                    sources: list_of(&r.get::<_, String>(4)?),
                    subject: r.get(5)?,
                    by: r.get(6)?,
                    at: r.get(7)?,
                    matched: 0.0,
                })
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let mut st = self.db.prepare("SELECT id FROM facts ORDER BY id DESC LIMIT ?1").map_err(sql)?;
        let ids = st.query_map(params![limit as i64], |r| r.get::<_, i64>(0)).map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        let mut facts = Vec::new();
        for id in ids {
            if let Some(f) = self.fact(id)? {
                facts.push(f);
            }
        }
        let mut st = self.db.prepare("SELECT id, about, text, sources, subject, by, at FROM notes ORDER BY id DESC LIMIT ?1").map_err(sql)?;
        let notes = st.query_map(params![limit as i64], note_row).map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        Ok(Recent { answers, facts, notes })
    }

    pub fn counts(&self) -> Result<Counts, String> {
        let n = |q: &str| self.db.query_row(q, [], |r| r.get::<_, i64>(0)).map_err(sql);
        Ok(Counts {
            facts: n("SELECT COUNT(*) FROM facts")?,
            concepts: n("SELECT COUNT(*) FROM concepts")?,
            relations: n("SELECT COUNT(*) FROM relations")?,
            answers: n("SELECT COUNT(*) FROM answers")?,
            notes: n("SELECT COUNT(*) FROM notes")?,
            waiting: n("SELECT COUNT(*) FROM gather_requests WHERE status = 'waiting'")?,
        })
    }

    /// Everything kept that bears on `question`, within `subject` (and what belongs to no subject) when given.
    pub fn look(&self, question: &str, subject: Option<&str>, limit: usize) -> Result<Look, String> {
        let words = keywords(question);
        let Some(any) = any_of(&words) else { return Ok(Look::default()) };
        let key = key_of(question);
        let mut look = Look::default();

        let mut st = self
            .db
            .prepare(
                "SELECT a.id, a.question, a.key, a.answer, a.facts, a.sources, a.subject, a.by, a.at FROM answers_fts f
                 JOIN answers a ON a.id = f.rowid
                 WHERE answers_fts MATCH ?1 AND (?2 IS NULL OR a.subject IS NULL OR a.subject = ?2)
                 ORDER BY f.rank LIMIT 50",
            )
            .map_err(sql)?;
        let rows = st
            .query_map(params![format!("question : ({any})"), subject], |r| {
                Ok((
                    r.get::<_, String>(2)?,
                    Answer {
                        id: r.get(0)?,
                        question: r.get(1)?,
                        answer: r.get(3)?,
                        facts: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                        sources: list_of(&r.get::<_, String>(5)?),
                        subject: r.get(6)?,
                        by: r.get(7)?,
                        at: r.get(8)?,
                        matched: 0.0,
                    },
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (k, mut a) = row.map_err(sql)?;
            a.matched = overlap(&key, &k);
            if a.matched >= REPEAT {
                look.answers.push(a);
            }
        }
        look.answers.sort_by(|x, y| y.matched.total_cmp(&x.matched).then(y.id.cmp(&x.id)));
        look.answers.truncate(limit);

        let mut st = self
            .db
            .prepare(
                "SELECT x.id FROM facts_fts f JOIN facts x ON x.id = f.rowid
                 WHERE facts_fts MATCH ?1 AND (?2 IS NULL OR x.subject IS NULL OR x.subject = ?2)
                 ORDER BY f.rank LIMIT ?3",
            )
            .map_err(sql)?;
        let ids = st.query_map(params![any, subject, limit as i64], |r| r.get::<_, i64>(0)).map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        for id in ids {
            if let Some(f) = self.fact(id)? {
                look.facts.push(f);
            }
        }

        let asked = format!(" {} ", spelling(question));
        let mut st = self.db.prepare("SELECT id, name, aliases FROM concepts").map_err(sql)?;
        let all = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let mut named: Vec<(usize, i64)> = all
            .into_iter()
            .filter_map(|(id, name, aliases)| {
                std::iter::once(name)
                    .chain(list_of(&aliases))
                    .map(|s| spelling(&s))
                    .filter(|s| !s.is_empty() && asked.contains(&format!(" {s} ")))
                    .map(|s| s.len())
                    .max()
                    .map(|len| (len, id))
            })
            .collect();
        named.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, id) in named.into_iter().take(limit) {
            if let Some(c) = self.concept_by_id(id, limit)? {
                look.concepts.push(c);
            }
        }

        let mut st = self
            .db
            .prepare(
                "SELECT n.id, n.about, n.text, n.sources, n.subject, n.by, n.at FROM notes_fts f JOIN notes n ON n.id = f.rowid
                 WHERE notes_fts MATCH ?1 AND (?2 IS NULL OR n.subject IS NULL OR n.subject = ?2)
                 ORDER BY f.rank LIMIT ?3",
            )
            .map_err(sql)?;
        let rows = st.query_map(params![any, subject, limit as i64], note_row).map_err(sql)?;
        look.notes = rows.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        Ok(look)
    }

    /// A concept by any of its spellings, with its relations and facts.
    pub fn concept(&self, name: &str, limit: usize) -> Result<Option<Concept>, String> {
        let wanted = spelling(name);
        if wanted.is_empty() {
            return Ok(None);
        }
        let mut st = self.db.prepare("SELECT id, name, aliases FROM concepts").map_err(sql)?;
        let all = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let found = all.into_iter().find(|(_, n, a)| spelling(n) == wanted || list_of(a).iter().any(|s| spelling(s) == wanted));
        match found {
            Some((id, ..)) => self.concept_by_id(id, limit),
            None => Ok(None),
        }
    }

    fn concept_by_id(&self, id: i64, limit: usize) -> Result<Option<Concept>, String> {
        let row: Option<(String, String)> =
            self.db.query_row("SELECT name, aliases FROM concepts WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(sql)?;
        let Some((name, aliases)) = row else { return Ok(None) };
        let mut st = self
            .db
            .prepare(
                "SELECT r.id, ca.name, r.kind, cb.name, r.work, r.paper, r.note, r.by, r.at FROM relations r
                 JOIN concepts ca ON ca.id = r.a JOIN concepts cb ON cb.id = r.b
                 WHERE r.a = ?1 OR r.b = ?1 ORDER BY r.id LIMIT ?2",
            )
            .map_err(sql)?;
        let relations = st
            .query_map(params![id, limit as i64], |r| {
                let work: Option<i64> = r.get(4)?;
                let title: Option<String> = r.get(5)?;
                Ok(Relation {
                    id: r.get(0)?,
                    from: r.get(1)?,
                    kind: r.get(2)?,
                    to: r.get(3)?,
                    paper: work.map(|work| Paper { work, title: title.unwrap_or_default() }),
                    note: r.get(6)?,
                    by: r.get(7)?,
                    at: r.get(8)?,
                })
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        let mut st = self.db.prepare("SELECT fact FROM fact_concepts WHERE concept = ?1 ORDER BY fact LIMIT ?2").map_err(sql)?;
        let ids = st.query_map(params![id, limit as i64], |r| r.get::<_, i64>(0)).map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        let mut facts = Vec::new();
        for f in ids {
            if let Some(f) = self.fact(f)? {
                facts.push(f);
            }
        }
        Ok(Some(Concept { id, name, aliases: list_of(&aliases), relations, facts }))
    }

    pub fn fact(&self, id: i64) -> Result<Option<Fact>, String> {
        let row = self
            .db
            .query_row("SELECT id, work, paper, subject, kind, text, conditions, by, at FROM facts WHERE id = ?1", params![id], |r| {
                Ok(Fact {
                    id: r.get(0)?,
                    paper: Paper { work: r.get(1)?, title: r.get(2)? },
                    subject: r.get(3)?,
                    kind: r.get(4)?,
                    text: r.get(5)?,
                    conditions: r.get(6)?,
                    concepts: Vec::new(),
                    by: r.get(7)?,
                    at: r.get(8)?,
                })
            })
            .optional()
            .map_err(sql)?;
        let Some(mut f) = row else { return Ok(None) };
        let mut st = self.db.prepare("SELECT c.name FROM fact_concepts fc JOIN concepts c ON c.id = fc.concept WHERE fc.fact = ?1 ORDER BY c.name").map_err(sql)?;
        f.concepts = st.query_map(params![id], |r| r.get::<_, String>(0)).map_err(sql)?.collect::<Result<Vec<_>, _>>().map_err(sql)?;
        Ok(Some(f))
    }
}

fn note_row(r: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: r.get(0)?,
        about: r.get(1)?,
        text: r.get(2)?,
        sources: list_of(&r.get::<_, String>(3)?),
        subject: r.get(4)?,
        by: r.get(5)?,
        at: r.get(6)?,
    })
}

/// A concept's row by any spelling, made when missing; new spellings are added.
fn concept_in(tx: &rusqlite::Transaction, name: &str, aliases: &[String]) -> Result<i64, String> {
    let name = line("a concept", name, LIMITS.concept)?;
    if aliases.len() > LIMITS.concepts {
        return Err(format!("at most {} other spellings", LIMITS.concepts));
    }
    let aliases = aliases.iter().map(|a| line("a spelling", a, LIMITS.concept)).collect::<Result<Vec<_>, _>>()?;
    let key = spelling(&name);
    if key.is_empty() {
        return Err("a concept's name has no words".into());
    }
    let mut st = tx.prepare("SELECT id, name, key, aliases FROM concepts").map_err(sql)?;
    let all = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    drop(st);
    let found = all.into_iter().find(|(_, _, k, a)| *k == key || list_of(a).iter().any(|s| spelling(s) == key));
    match found {
        Some((id, existing, k, kept)) => {
            let mut kept = list_of(&kept);
            for a in aliases.iter().chain(std::iter::once(&name)) {
                let s = spelling(a);
                if s != k && s != spelling(&existing) && !kept.iter().any(|x| spelling(x) == s) && kept.len() < LIMITS.concepts {
                    kept.push(a.clone());
                }
            }
            tx.execute("UPDATE concepts SET aliases = ?2 WHERE id = ?1", params![id, serde_json::to_string(&kept).unwrap_or_default()]).map_err(sql)?;
            Ok(id)
        }
        None => {
            let mut kept: Vec<String> = Vec::new();
            for a in &aliases {
                let s = spelling(a);
                if s != key && !s.is_empty() && !kept.iter().any(|x| spelling(x) == s) {
                    kept.push(a.clone());
                }
            }
            tx.execute("INSERT INTO concepts (name, key, aliases) VALUES (?1, ?2, ?3)", params![name, key, serde_json::to_string(&kept).unwrap_or_default()])
                .map_err(sql)?;
            Ok(tx.last_insert_rowid())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn store(name: &str) -> (Dir, Knowledge) {
        let dir = std::env::temp_dir().join(format!("alelyon-knowledge-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let k = Knowledge::open(&dir.join("knowledge.sqlite")).unwrap();
        (Dir(dir), k)
    }
    fn paper(work: i64) -> Paper {
        Paper { work, title: format!("Paper {work}") }
    }

    #[test]
    fn a_fact_is_kept_once_found_by_its_words_and_by_its_concepts() {
        let (_d, mut k) = store("facts");
        let concepts = vec!["LoRA".to_string(), "fine-tuning".to_string()];
        let (id, new) = k
            .remember_fact(&paper(7), Some("lora"), "result", "LoRA matches full fine-tuning on GLUE with 0.1% of the weights trained.", Some("RoBERTa-large"), &concepts, "a reader", "t1")
            .unwrap();
        assert!(new);
        let again = k.remember_fact(&paper(7), None, "result", "LoRA matches full fine-tuning on GLUE with 0.1% of the weights trained", None, &[], "", "t2").unwrap();
        assert_eq!(again, (id, false), "the same words from the same paper are kept once");
        let look = k.look("How many weights does LoRA train?", None, 10).unwrap();
        assert_eq!(look.facts.len(), 1);
        assert_eq!(look.facts[0].concepts, ["LoRA", "fine-tuning"]);
        assert_eq!(look.concepts.len(), 1, "the question names LoRA: {:?}", look.concepts);
        assert_eq!(look.concepts[0].facts[0].id, id);
        assert!(k.look("lora", Some("other subject"), 10).unwrap().facts.is_empty(), "another subject's fact stays out");
    }

    #[test]
    fn a_close_repeat_of_a_question_finds_its_answer_and_a_different_one_does_not() {
        let (_d, mut k) = store("answers");
        let (fact, _) = k.remember_fact(&paper(1), None, "number", "QMoE compresses SwitchTransformer-c2048 to 0.8 bits per parameter.", None, &[], "", "t").unwrap();
        k.keep_answer("How many bits per parameter does QMoE reach?", "About 0.8 bits per parameter (SwitchTransformer-c2048).", &[fact], &[], None, "a session", "t")
            .unwrap();
        let look = k.look("how many bits per parameter does qmoe reach", None, 5).unwrap();
        assert_eq!(look.answers.len(), 1);
        assert!((look.answers[0].matched - 1.0).abs() < 1e-9);
        assert_eq!(look.answers[0].facts, [fact]);
        assert!(k.look("Which optimizer trains binary weights?", None, 5).unwrap().answers.is_empty());
        assert!(k.look("What does QMoE compress?", None, 5).unwrap().answers.is_empty(), "sharing one word is no repeat");
    }

    #[test]
    fn relations_join_concepts_by_any_spelling() {
        let (_d, mut k) = store("relations");
        k.concept_id("Binary neural network", &["BNN".into(), "binarized network".into()]).unwrap();
        let (r, new) = k.relate("BNN", "improves_on", "XNOR-Net", Some(&paper(3)), Some("adds scaling"), "", "t").unwrap();
        assert!(new);
        assert_eq!(k.relate("binary neural network", "improves_on", "xnor net", Some(&paper(3)), None, "", "t").unwrap(), (r, false));
        let c = k.concept("binarized network", 10).unwrap().unwrap();
        assert_eq!(c.name, "Binary neural network");
        assert_eq!(c.relations.len(), 1);
        assert_eq!((c.relations[0].kind.as_str(), c.relations[0].to.as_str()), ("improves_on", "XNOR-Net"));
        let look = k.look("what improves on xnor-net?", None, 10).unwrap();
        assert_eq!(look.concepts.len(), 1);
        assert!(k.relate("BNN", "likes", "x", None, None, "", "t").is_err());
        assert!(k.relate("BNN", "is_a", "binarized network", None, None, "", "t").is_err(), "a concept is not related to itself");
    }

    #[test]
    fn gathering_requests_wait_once_for_the_persons_decision() {
        let (_d, mut k) = store("requests");
        let (id, new) = k.request_gathering("binary mixture of experts", None, "nothing kept on MoE binarization", "", "t").unwrap();
        assert!(new);
        assert_eq!(k.request_gathering("Mixture of experts, binary", None, "again", "", "t").unwrap(), (id, false));
        assert_eq!(k.requests(Some("waiting")).unwrap().len(), 1);
        k.decide(id, "approved", "t2").unwrap();
        assert!(k.decide(id, "dismissed", "t3").is_err(), "decided once");
        assert_eq!(k.counts().unwrap().waiting, 0);
        assert!(k.decide(id, "waiting", "t").is_err());
    }

    #[test]
    fn bad_records_are_refused_and_forgetting_removes_them() {
        let (_d, mut k) = store("refused");
        assert!(k.remember_fact(&paper(1), None, "rumour", "x y", None, &[], "", "t").is_err());
        assert!(k.remember_fact(&paper(1), None, "claim", "two\nlines", None, &[], "", "t").is_err());
        assert!(k.remember_fact(&paper(1), None, "claim", &"w ".repeat(400), None, &[], "", "t").is_err());
        assert!(k.keep_answer("q words here", "an answer", &[], &[], None, "", "t").is_err(), "an answer needs grounds");
        assert!(k.keep_answer("q words here", "an answer", &[99], &[], None, "", "t").is_err(), "an unknown fact");
        assert_eq!(k.recent(5).unwrap(), Recent::default());
        let n = k.note("QMoE kernels", "They decode on the GPU.\nMeasured at 0.8 bpw.", &["arXiv:2310.16795".into()], None, "", "t").unwrap();
        assert_eq!(k.look("QMoE kernels", None, 5).unwrap().notes.len(), 1);
        assert_eq!(k.recent(5).unwrap().notes[0].id, n);
        k.forget(Kind::Note, n).unwrap();
        assert!(k.look("QMoE kernels", None, 5).unwrap().is_empty());
        assert!(k.forget(Kind::Note, n).is_err());
    }

    #[test]
    fn nothing_kept_reads_as_empty_and_a_missing_store_as_none() {
        let (d, k) = store("empty");
        assert!(k.look("anything at all", None, 5).unwrap().is_empty());
        assert!(k.look("the of and", None, 5).unwrap().is_empty(), "no words left");
        assert!(Knowledge::open_read_only(&d.0.join("absent.sqlite")).unwrap().is_none());
    }
}
