//! A harvest or update as a process of its own, so the window that starts it can freeze, crash or close without
//! taking the run with it (observed 2026-10-08: the window's graphics driver crashed 18 minutes into a run that lived
//! in the window's process, and the run was lost unsaved).
//!
//! A job is files beside the archive, in `<archive dir>/jobs/`:
//! - `<id>.json`: what to run (written by the window);
//! - `<id>.progress`: one line per round, appended by the job;
//! - `<id>.alive`: touched by the job every few seconds while it runs;
//! - `<id>.stop`: created by the window to stop it before its next request;
//! - `<id>.done.json`: the outcome, written last.
//!
//! The job saves the archive after every round (harvest.rs), so even a crash of the job loses at most one round. The
//! OpenAlex key reaches the job through its environment (`OPENALEX_API_KEY`), never its command line or these files.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(feature = "net")]
use std::sync::Arc;
#[cfg(feature = "net")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::index::Error;
use crate::snowball::Topic;
#[cfg(feature = "net")]
use crate::snowball::Policy;
#[cfg(feature = "net")]
use crate::store::Stamp;

/// How often a running job touches its `.alive` file.
pub const HEARTBEAT: Duration = Duration::from_secs(3);
/// A job whose `.alive` file is older than this is not running.
pub const STALE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A full gathering of a subject.
    Full { topic: Topic, max_calls: usize },
    /// An update of a saved subject with what is new.
    Update { subject: String, max_calls: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub id: String,
    pub db: PathBuf,
    pub kind: Kind,
}

impl Spec {
    pub fn subject(&self) -> &str {
        match &self.kind {
            Kind::Full { topic, .. } => &topic.name,
            Kind::Update { subject, .. } => subject,
        }
    }

    pub fn is_update(&self) -> bool {
        matches!(self.kind, Kind::Update { .. })
    }

    fn to_json(&self) -> Value {
        match &self.kind {
            Kind::Full { topic, max_calls } => json!({
                "id": self.id, "db": self.db, "kind": "full", "subject": topic.name,
                "queries": topic.queries, "phrases": topic.phrases, "max_calls": max_calls,
            }),
            Kind::Update { subject, max_calls } => json!({
                "id": self.id, "db": self.db, "kind": "update", "subject": subject, "max_calls": max_calls,
            }),
        }
    }

    fn from_json(v: &Value) -> Option<Spec> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let list = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>());
        let max_calls = v.get("max_calls").and_then(Value::as_u64)? as usize;
        let kind = match s("kind")?.as_str() {
            "full" => Kind::Full { topic: Topic { name: s("subject")?, queries: list("queries")?, phrases: list("phrases")? }, max_calls },
            "update" => Kind::Update { subject: s("subject")?, max_calls },
            _ => return None,
        };
        Some(Spec { id: s("id")?, db: PathBuf::from(s("db")?), kind })
    }
}

/// How a job ended, as its `.done.json` says.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    /// Stop::name of the run, when it ran.
    pub stop: Option<String>,
    pub accepted: u64,
    pub read: u64,
    pub calls: u64,
    pub halt_reason: Option<String>,
    pub error: Option<String>,
}

/// The files of one job.
#[derive(Debug, Clone, PartialEq)]
pub struct Files {
    pub spec: PathBuf,
    pub progress: PathBuf,
    pub alive: PathBuf,
    pub stop: PathBuf,
    pub done: PathBuf,
}

pub fn dir_for(db: &Path) -> PathBuf {
    db.parent().map(Path::to_path_buf).unwrap_or_default().join("jobs")
}

pub fn files(dir: &Path, id: &str) -> Files {
    Files {
        spec: dir.join(format!("{id}.json")),
        progress: dir.join(format!("{id}.progress")),
        alive: dir.join(format!("{id}.alive")),
        stop: dir.join(format!("{id}.stop")),
        done: dir.join(format!("{id}.done.json")),
    }
}

fn io(e: std::io::Error, what: &Path) -> Error {
    Error::Store(format!("{}: {e}", what.display()))
}

/// Write a job's spec (the window's side). Returns its files.
pub fn create(spec: &Spec) -> Result<Files, Error> {
    let dir = dir_for(&spec.db);
    fs::create_dir_all(&dir).map_err(|e| io(e, &dir))?;
    let f = files(&dir, &spec.id);
    fs::write(&f.spec, serde_json::to_vec_pretty(&spec.to_json()).unwrap_or_default()).map_err(|e| io(e, &f.spec))?;
    Ok(f)
}

/// A fresh job id: the clock in milliseconds and this process's id.
pub fn new_id() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{ms}-{}", std::process::id())
}

/// Ask a running job to stop before its next request.
pub fn request_stop(f: &Files) -> Result<(), Error> {
    fs::write(&f.stop, b"stop").map_err(|e| io(e, &f.stop))
}

/// The job's progress lines so far.
pub fn progress(f: &Files) -> Vec<String> {
    fs::read_to_string(&f.progress).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
}

/// The job's outcome, once it has one.
pub fn outcome(f: &Files) -> Option<Outcome> {
    let v: Value = serde_json::from_slice(&fs::read(&f.done).ok()?).ok()?;
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(Outcome { ok: v.get("ok").and_then(Value::as_bool).unwrap_or(false), stop: s("stop"), accepted: n("accepted"), read: n("read"), calls: n("calls"), halt_reason: s("halt_reason"), error: s("error") })
}

/// Whether the job touched its heartbeat within `STALE`.
pub fn alive(f: &Files) -> bool {
    fs::metadata(&f.alive).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < STALE)
}

/// Jobs in `dir` that are running now (heartbeat fresh, no outcome yet), with their specs.
pub fn running(dir: &Path) -> Vec<(Spec, Files)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_suffix(".json").filter(|id| !id.ends_with(".done")) else { continue };
        let f = files(dir, id);
        if f.done.exists() || !alive(&f) {
            continue;
        }
        if let Some(spec) = fs::read(&f.spec).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| Spec::from_json(&v)) {
            out.push((spec, f));
        }
    }
    out
}

/// Run the job whose spec is at `spec_path` (the job process's side). Always writes `.done.json`, and returns
/// whether the run produced an outcome.
#[cfg(feature = "net")]
pub fn run(spec_path: &Path) -> bool {
    use std::io::Write;
    let dir = spec_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let read = fs::read(spec_path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| Spec::from_json(&v));
    let Some(spec) = read else {
        let id = spec_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let _ = fs::write(files(&dir, &id).done, json!({"ok": false, "error": "the job's spec could not be read"}).to_string());
        return false;
    };
    let f = files(&dir, &spec.id);
    let _ = fs::write(&f.alive, b"");
    // The heartbeat, and the stop file turned into the run's cancel flag.
    let cancel = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    {
        let (cancel, finished, f) = (cancel.clone(), finished.clone(), f.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::SeqCst) {
                let _ = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&f.alive).and_then(|mut h| h.write_all(b"1"));
                if f.stop.exists() {
                    cancel.store(true, Ordering::SeqCst);
                }
                std::thread::sleep(HEARTBEAT);
            }
        });
    }
    let progress_path = f.progress.clone();
    let hook: crate::snowball::RoundHook = Box::new(move |r, calls| {
        if let Ok(mut h) = fs::OpenOptions::new().append(true).create(true).open(&progress_path) {
            let _ = writeln!(h, "{} accepted ({} new this round), {} papers read, {calls} requests", r.accepted_total, r.newly_accepted, r.archive_size);
        }
    });
    let key = std::env::var("OPENALEX_API_KEY").ok().filter(|k| !k.trim().is_empty());
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let now = Stamp { text: crate::harvest_date::date_of(secs) + &format!(" {:02}:{:02}Z", secs.rem_euclid(86_400) / 3600, secs.rem_euclid(3600) / 60), secs };
    let result = match &spec.kind {
        Kind::Full { topic, max_calls } => {
            let mut policy = Policy::for_topic(topic);
            policy.max_calls = *max_calls;
            crate::harvest::harvest(&spec.db, topic.clone(), policy, key, cancel, Some(hook), &now)
        }
        Kind::Update { subject, max_calls } => crate::harvest::update(&spec.db, subject, *max_calls, key, cancel, Some(hook), &now),
    };
    finished.store(true, Ordering::SeqCst);
    let done = match &result {
        Ok(r) => json!({
            "ok": true, "stop": r.stop.name(), "accepted": r.accepted, "read": r.accepted + r.candidates,
            "calls": r.calls, "halt_reason": r.halt_reason,
        }),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    };
    // Written to a side file and renamed, so a reader never sees half an outcome.
    let tmp = f.done.with_extension("tmp");
    let _ = fs::write(&tmp, done.to_string()).and_then(|_| fs::rename(&tmp, &f.done));
    let _ = fs::remove_file(&f.alive);
    result.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("alelyon-research-jobs-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_spec_round_trips_and_its_files_sit_beside_the_archive() {
        let d = temp();
        let spec = Spec {
            id: "1-2".into(),
            db: d.join("archive.sqlite"),
            kind: Kind::Full { topic: Topic { name: "s".into(), queries: vec!["q".into()], phrases: vec!["a|b".into()] }, max_calls: 7 },
        };
        let f = create(&spec).unwrap();
        assert_eq!(f.spec, d.join("jobs").join("1-2.json"));
        let back = Spec::from_json(&serde_json::from_slice(&fs::read(&f.spec).unwrap()).unwrap()).unwrap();
        assert_eq!(back, spec);
        let upd = Spec { id: "3".into(), db: spec.db.clone(), kind: Kind::Update { subject: "s".into(), max_calls: 300 } };
        assert_eq!(Spec::from_json(&upd.to_json()), Some(upd));
        // No key or secret is ever in the spec.
        assert!(!fs::read_to_string(&f.spec).unwrap().to_lowercase().contains("key"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn running_means_a_fresh_heartbeat_and_no_outcome() {
        let d = temp();
        let spec = Spec { id: "9".into(), db: d.join("archive.sqlite"), kind: Kind::Update { subject: "s".into(), max_calls: 1 } };
        let f = create(&spec).unwrap();
        let dir = dir_for(&spec.db);
        assert!(running(&dir).is_empty(), "no heartbeat yet");
        fs::write(&f.alive, b"1").unwrap();
        assert_eq!(running(&dir).len(), 1);
        fs::write(&f.progress, "a\nb\n").unwrap();
        assert_eq!(progress(&f), vec!["a", "b"]);
        fs::write(&f.done, r#"{"ok":true,"stop":"budget","accepted":5,"read":9,"calls":1}"#).unwrap();
        assert!(running(&dir).is_empty(), "an outcome ends it");
        let o = outcome(&f).unwrap();
        assert_eq!((o.ok, o.stop.as_deref(), o.accepted), (true, Some("budget"), 5));
        request_stop(&f).unwrap();
        assert!(f.stop.exists());
        let _ = fs::remove_dir_all(&d);
    }
}
