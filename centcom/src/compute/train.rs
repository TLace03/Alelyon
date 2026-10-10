//! Training: a trainer's live metrics stream, read as it grows, by the Training Studio's own rules.
//!
//! The one structured training record the project keeps is the stream a trainer writes with `--out <file>.jsonl`:
//! a `header` line declaring the run, a `step` line per step, then `done`, or `abort` with its
//! reason. The Python desktop's Training Studio reads it with its `StreamReader`; this
//! is that reader ported rule for rule, so the two windows agree on what a stream says:
//! - a header first, declaring a positive step budget and its expert count, its sizes whole numbers in range;
//! - steps numbered 0, 1, 2, ... with none missing or repeated and none past the budget; every number finite and not
//!   negative; expert utilisation, when given, one fraction per expert summing to one;
//! - no key twice in a record, no record over 64 KiB, at most 1 MiB read a poll, the newest 1,000 steps kept;
//! - the file read from where the last poll stopped, with the last 256 bytes read checked again, so a file replaced,
//!   truncated or rewritten under the window is refused rather than read as more of the same run;
//! - completed only with every declared step and a `done` line; the end of the file without one never means success,
//!   and an abort says the trainer's reason.
//! Anything refused makes the stream invalid, with the reason; it is not read further until chosen again.
//!
//! What it does not establish (as the Studio says): the numbers are what the file says, not evidence that a process
//! produced them; whether the trainer is still running is not known; checkpoints, GPU memory and held-out evaluation
//! are not in the stream. The streams offered are the `.jsonl` files in the folder the build names (none in the
//! public build) whose first line is a header; any other file can be typed in.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

pub const MAX_POLL_BYTES: usize = 1 << 20;
pub const MAX_LINE_BYTES: usize = 64 << 10;
pub const MAX_ROWS: usize = 1000;
const ANCHOR_BYTES: usize = 256;
/// The shortest time between two reads of a growing stream while the tab is on show; it is read only when its file changed.
pub const POLL_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Running,
    Completed,
    Incomplete,
    Aborted,
    Invalid,
}

impl Status {
    pub fn words(self) -> &'static str {
        match self {
            Status::Waiting => "waiting",
            Status::Running => "receiving",
            Status::Completed => "completed",
            Status::Incomplete => "incomplete",
            Status::Aborted => "aborted",
            Status::Invalid => "refused",
        }
    }
}

/// One step as the window draws it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    pub step: u64,
    pub loss: Option<f64>,
    pub ce: Option<f64>,
    pub grad_norm: Option<f64>,
    pub lr: Option<f64>,
    pub seconds: Option<f64>,
    pub tokens_per_s: Option<f64>,
    pub utilisation: Vec<f64>,
}

#[derive(Debug)]
pub struct Stream {
    pub path: PathBuf,
    identity: Option<(u64, u64)>,
    offset: u64,
    anchor: Vec<u8>,
    partial: Vec<u8>,
    pub header: Option<Map<String, Value>>,
    pub rows: Vec<Step>,
    steps: u64,
    terminal: Option<Status>,
    pub status: Status,
    pub message: String,
}

fn require(ok: bool, why: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(why.to_string()) }
}

/// A whole number in `lo..=hi` (JSON integers only: 3.0 is not one, as Python's `type(x) is int` says).
fn int_in(v: Option<&Value>, lo: i64, hi: i64) -> Option<i64> {
    let v = v?;
    let n = if v.is_i64() || v.is_u64() { v.as_i64()? } else { return None };
    (lo..=hi).contains(&n).then_some(n)
}

fn number(v: &Value, key: &str) -> Result<f64, String> {
    let n = v.as_f64().filter(|n| n.is_finite() && !v.is_boolean());
    let n = n.ok_or_else(|| format!("Invalid {key} measurement."))?;
    require(n >= 0.0, &format!("Invalid {key} measurement."))?;
    Ok(n)
}

fn optional(row: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => number(v, key).map(Some),
    }
}

/// Whether any object in `text` (already known to be JSON) names a key twice. serde_json keeps the last silently; the
/// Studio refuses. Keys are compared decoded, so "a" and "\u0061" are the same key, as Python's reader sees them.
fn has_duplicate_key(text: &str) -> bool {
    let b = text.as_bytes();
    // per open bracket: Some(keys, expecting a key) for an object, None for an array
    let mut stack: Vec<Option<(std::collections::HashSet<String>, bool)>> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => stack.push(Some((Default::default(), true))),
            b'[' => stack.push(None),
            b'}' | b']' => {
                stack.pop();
            }
            b',' => {
                if let Some(Some((_, expect))) = stack.last_mut() {
                    *expect = true;
                }
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                if let Some(Some((keys, expect))) = stack.last_mut() {
                    if *expect {
                        *expect = false;
                        let key: String = serde_json::from_str(&text[start..=i.min(b.len() - 1)]).unwrap_or_default();
                        if !keys.insert(key) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Whether any array in `v` is longer than the Studio allows.
fn too_long(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.len() > 4096 || a.iter().any(too_long),
        Value::Object(m) => m.values().any(too_long),
        _ => false,
    }
}

/// Parse one record by the Studio's rules: an object, no key twice, no array over 4,096 items; serde_json refuses
/// NaN and infinities as JSON does.
fn parse(line: &[u8]) -> Result<Map<String, Value>, String> {
    require(line.len() <= MAX_LINE_BYTES, "Telemetry record exceeds 64 KiB.")?;
    let malformed = || "The telemetry stream contains unreadable or malformed data.".to_string();
    let text = std::str::from_utf8(line).map_err(|_| malformed())?;
    let v: Value = serde_json::from_str(text).map_err(|_| malformed())?;
    require(!has_duplicate_key(text), "Telemetry contains a duplicate key.")?;
    require(!too_long(&v), "Telemetry array exceeds its limit.")?;
    match v {
        Value::Object(map) => Ok(map),
        _ => Err("Telemetry record must be a JSON object.".into()),
    }
}

impl Stream {
    pub fn new(path: PathBuf) -> Stream {
        Stream {
            path,
            identity: None,
            offset: 0,
            anchor: Vec::new(),
            partial: Vec::new(),
            header: None,
            rows: Vec::new(),
            steps: 0,
            terminal: None,
            status: Status::Waiting,
            message: "Waiting for the stream's first record. No trainer is started by this view.".into(),
        }
    }

    /// The steps the header declares.
    pub fn budget(&self) -> Option<u64> {
        self.header.as_ref().and_then(|h| h.get("steps")).and_then(Value::as_u64)
    }

    /// Steps seen so far (the kept rows are only the newest `MAX_ROWS`).
    pub fn seen(&self) -> u64 {
        self.steps
    }

    fn header_record(&mut self, row: Map<String, Value>) -> Result<(), String> {
        require(self.header.is_none() && self.steps == 0 && self.terminal.is_none(), "Unexpected second stream header.")?;
        require(int_in(row.get("steps"), 1, 1_000_000_000).is_some(), "Stream header must declare a positive step budget.")?;
        let experts = int_in(row.get("n_experts"), 0, 4096).ok_or("Stream header must declare its expert count.")?;
        for key in ["batch", "seq", "parameters", "n_layer", "d_model", "vocab_size", "tokens_per_step"] {
            if row.get(key).is_some_and(|v| !v.is_null()) {
                require(int_in(row.get(key), 1, 1_000_000_000_000_000).is_some(), &format!("Invalid header {key}."))?;
            }
        }
        if row.get("n_experts_active").is_some_and(|v| !v.is_null()) {
            require(int_in(row.get("n_experts_active"), 1, experts.max(2)).is_some(), "Invalid active expert count.")?;
        }
        if let Some(dense) = row.get("dense") {
            require(dense.as_bool() == Some(experts == 0), "Dense/expert declarations conflict.")?;
        }
        for key in ["lr", "bits_per_parameter", "active_fraction"] {
            optional(&row, key)?;
        }
        if let Some(f) = optional(&row, "active_fraction")? {
            require(f <= 1.0, "Invalid active expert fraction.")?;
        }
        self.header = Some(row);
        Ok(())
    }

    fn step_record(&mut self, row: Map<String, Value>) -> Result<(), String> {
        let budget = self.budget();
        require(self.header.is_some() && self.terminal.is_none(), "Step record is outside an active stream.")?;
        let n = int_in(row.get("step"), 0, i64::MAX).map(|n| n as u64);
        require(
            n == Some(self.steps) && Some(self.steps) < budget,
            "Training steps are missing, duplicated or outside the declared budget.",
        )?;
        let mut step = Step { step: self.steps, ..Step::default() };
        for key in ["loss", "ce", "cod", "balance", "grad_norm", "lr", "seconds", "tokens_per_s", "elapsed"] {
            let v = optional(&row, key)?;
            match key {
                "loss" => step.loss = v,
                "ce" => step.ce = v,
                "grad_norm" => step.grad_norm = v,
                "lr" => step.lr = v,
                "seconds" => step.seconds = v,
                "tokens_per_s" => step.tokens_per_s = v,
                _ => {}
            }
        }
        if let Some(values) = row.get("utilisation").filter(|v| !v.is_null()) {
            let experts = self.header.as_ref().and_then(|h| h.get("n_experts")).and_then(Value::as_u64).unwrap_or(0);
            let values = values.as_array().filter(|a| a.len() as u64 == experts).ok_or("Expert utilisation has the wrong size.")?;
            for v in values {
                let f = number(v, "expert utilisation")?;
                require(f <= 1.0, "Invalid expert utilisation fraction.")?;
                step.utilisation.push(f);
            }
            if !step.utilisation.is_empty() {
                let sum: f64 = step.utilisation.iter().sum();
                require(
                    (sum - 1.0).abs() <= (0.01f64).max(step.utilisation.len() as f64 * 0.0001),
                    "Expert utilisation fractions do not sum to one.",
                )?;
            }
        }
        self.steps += 1;
        self.rows.push(step);
        if self.rows.len() > MAX_ROWS {
            let cut = self.rows.len() - MAX_ROWS;
            self.rows.drain(..cut);
        }
        Ok(())
    }

    fn record(&mut self, line: &[u8]) -> Result<(), String> {
        let row = parse(line)?;
        match row.get("kind").and_then(Value::as_str) {
            Some("header") => self.header_record(row),
            Some("step") => self.step_record(row),
            Some("abort") => {
                require(self.header.is_some() && self.terminal.is_none(), "Unexpected abort record.")?;
                require(
                    int_in(row.get("step"), 0, i64::MAX).map(|n| n as u64) == Some(self.steps),
                    "Abort step does not match the observed stream.",
                )?;
                let reason = row
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|r| !r.is_empty() && r.len() <= 2048)
                    .ok_or("Invalid abort reason.")?;
                self.terminal = Some(Status::Aborted);
                self.status = Status::Aborted;
                let words: String = reason.split_whitespace().collect::<Vec<_>>().join(" ");
                self.message = format!("Trainer aborted: {}", words.chars().take(512).collect::<String>());
                Ok(())
            }
            Some("done") => {
                require(self.header.is_some() && self.terminal != Some(Status::Completed), "Unexpected completion record.")?;
                optional(&row, "elapsed")?;
                if self.terminal == Some(Status::Aborted) {
                    return Ok(()); // the trainer writes done after an abort too
                }
                let done = Some(self.steps) == self.budget();
                self.terminal = Some(if done { Status::Completed } else { Status::Incomplete });
                self.status = self.terminal.unwrap_or(Status::Incomplete);
                self.message = if done {
                    "All declared steps and the completion record were observed.".into()
                } else {
                    "The stream ended before all declared steps were observed.".into()
                };
                Ok(())
            }
            _ => Err("Unsupported telemetry record kind.".into()),
        }
    }

    /// Read what was added since the last poll.
    pub fn poll(&mut self) {
        if self.status == Status::Invalid {
            return;
        }
        if let Err(why) = self.poll_inner() {
            self.status = Status::Invalid;
            self.message = why;
        }
    }

    fn poll_inner(&mut self) -> Result<(), String> {
        let meta = match fs::symlink_metadata(&self.path) {
            Ok(m) => m,
            Err(_) => {
                require(self.identity.is_none(), "The telemetry file was removed or replaced. Choose it again.")?;
                return Ok(());
            }
        };
        require(meta.file_type().is_file(), "The telemetry path is not a plain file.")?;
        let identity = file_identity(&self.path, &meta);
        require(self.identity.is_none() || self.identity == Some(identity), "The telemetry file was replaced. Choose it again.")?;
        require(meta.len() >= self.offset, "The telemetry file was truncated. Choose it again.")?;
        let mut file = fs::File::open(&self.path).map_err(|e| format!("The telemetry file could not be opened: {e}"))?;
        if !self.anchor.is_empty() {
            file.seek(SeekFrom::Start(self.offset - self.anchor.len() as u64)).map_err(|e| e.to_string())?;
            let mut back = vec![0u8; self.anchor.len()];
            file.read_exact(&mut back).map_err(|_| "Previously read telemetry was modified.".to_string())?;
            require(back == self.anchor, "Previously read telemetry was modified.")?;
        }
        file.seek(SeekFrom::Start(self.offset)).map_err(|e| e.to_string())?;
        let mut chunk = Vec::new();
        (&mut file).take((MAX_POLL_BYTES - self.anchor.len()) as u64).read_to_end(&mut chunk).map_err(|e| e.to_string())?;
        drop(file);
        let after = fs::symlink_metadata(&self.path).map_err(|_| "Telemetry file changed while reading.".to_string())?;
        require(
            file_identity(&self.path, &after) == identity && after.len() >= self.offset + chunk.len() as u64,
            "Telemetry file changed while reading.",
        )?;
        self.identity = Some(identity);
        self.offset += chunk.len() as u64;
        let mut anchor = std::mem::take(&mut self.anchor);
        anchor.extend_from_slice(&chunk);
        self.anchor = anchor[anchor.len().saturating_sub(ANCHOR_BYTES)..].to_vec();
        let mut payload = std::mem::take(&mut self.partial);
        payload.extend_from_slice(&chunk);
        let mut lines: Vec<&[u8]> = payload.split(|b| *b == b'\n').collect();
        let partial = lines.pop().unwrap_or_default().to_vec();
        require(partial.len() <= MAX_LINE_BYTES, "Partial telemetry record exceeds 64 KiB.")?;
        for line in lines {
            if !line.iter().all(u8::is_ascii_whitespace) {
                self.record(line)?;
            }
        }
        self.partial = partial;
        match self.terminal {
            None => {
                self.status = if !chunk.is_empty() && self.header.is_some() {
                    Status::Running
                } else if self.header.is_none() {
                    Status::Waiting
                } else {
                    Status::Incomplete
                };
                self.message = if !self.partial.is_empty() {
                    "A partial record is awaiting more bytes. No completion has been established.".into()
                } else if self.status == Status::Running {
                    "New telemetry received; whether the trainer is still running is not known.".into()
                } else {
                    "Awaiting more telemetry; no completion record has been observed.".into()
                };
            }
            Some(terminal) if !self.partial.is_empty() => {
                self.status = if terminal == Status::Aborted { Status::Aborted } else { Status::Incomplete };
                self.message = "The terminal record is followed by partial data; stream integrity is incomplete.".into();
            }
            Some(_) => {}
        }
        Ok(())
    }
}

/// A file's identity across polls: the volume and file index on Windows (Python's st_dev and st_ino there), the device
/// and inode elsewhere. A file moved over this one has another.
#[cfg(windows)]
fn file_identity(path: &Path, _meta: &fs::Metadata) -> (u64, u64) {
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        attributes: u32,
        created: [u32; 2],
        accessed: [u32; 2],
        written: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: isize, info: *mut Info) -> i32;
    }
    use std::os::windows::io::AsRawHandle;
    let Ok(file) = fs::File::open(path) else { return (0, 0) };
    let mut info = Info::default();
    // SAFETY: the handle is a live, open file for the duration of the call, and `info` is the structure the call fills.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as isize, &mut info) } != 0;
    if ok { (info.volume as u64, ((info.index_high as u64) << 32) | info.index_low as u64) } else { (0, 0) }
}

#[cfg(not(windows))]
fn file_identity(_path: &Path, meta: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

/// One stream on offer.
#[derive(Clone, Debug, PartialEq)]
pub struct Offered {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: f64,
    /// The header's preset and size, for the list.
    pub what: String,
}

/// The `.jsonl` files in `results` (a trainer's results folder) whose first line is a header: one folder listed, the
/// first 64 KiB of each read. Newest first.
pub fn offered(results: &Path) -> Vec<Offered> {
    let Ok(entries) = fs::read_dir(results) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let mut head = Vec::new();
        if fs::File::open(&path).and_then(|f| f.take(MAX_LINE_BYTES as u64).read_to_end(&mut head)).is_err() {
            continue;
        }
        let first = head.split(|b| *b == b'\n').next().unwrap_or_default();
        let Ok(header) = parse(first) else { continue };
        if header.get("kind").and_then(Value::as_str) != Some("header") {
            continue;
        }
        let what = describe(&header);
        let modified =
            meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        out.push(Offered { path, bytes: meta.len(), modified, what });
    }
    out.sort_by(|a, b| b.modified.partial_cmp(&a.modified).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// A header in a few words: its preset, parameters, experts and steps.
pub fn describe(h: &Map<String, Value>) -> String {
    let mut parts = Vec::new();
    if let Some(p) = h.get("preset").and_then(Value::as_str) {
        parts.push(format!("preset {p}"));
    }
    if let Some(n) = h.get("parameters").and_then(Value::as_u64) {
        parts.push(format!("{:.1} M parameters", n as f64 / 1e6));
    }
    match (h.get("n_experts").and_then(Value::as_u64), h.get("n_experts_active").and_then(Value::as_u64)) {
        (Some(0), _) => parts.push("dense".into()),
        (Some(n), Some(a)) => parts.push(format!("{a} of {n} experts")),
        (Some(n), None) => parts.push(format!("{n} experts")),
        _ => {}
    }
    if let Some(s) = h.get("steps").and_then(Value::as_u64) {
        parts.push(format!("{s} steps"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        r#"{"kind":"header","preset":"s","dense":false,"steps":3,"batch":8,"n_experts":2,"n_experts_active":1,"lr":0.003}"#;

    fn step(n: u64) -> String {
        format!(r#"{{"kind":"step","step":{n},"loss":{},"grad_norm":1.5,"utilisation":[0.25,0.75]}}"#, 9.0 - n as f64)
    }

    fn stream_of(name: &str, text: &str) -> (Stream, PathBuf) {
        let dir = crate::sqlite_ro::tests::scratch(name);
        let path = dir.join("live.jsonl");
        fs::write(&path, text).unwrap();
        let mut s = Stream::new(path.clone());
        s.poll();
        (s, path)
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        fs::OpenOptions::new().append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn a_whole_run_is_completed_only_with_every_step_and_its_done_line() {
        let (mut s, path) = stream_of("train-whole", &format!("{HEADER}\n{}\n{}\n", step(0), step(1)));
        assert_eq!((s.status, s.seen()), (Status::Running, 2), "{}", s.message);
        assert_eq!(s.rows[1].loss, Some(8.0));
        s.poll();
        assert_eq!(s.status, Status::Incomplete, "nothing new: no completion seen");
        append(&path, &format!("{}\n{{\"kind\":\"done\",\"elapsed\":3.5}}\n", step(2)));
        s.poll();
        assert_eq!(s.status, Status::Completed, "{}", s.message);
    }

    #[test]
    fn a_done_line_before_every_step_is_incomplete_and_an_abort_says_why() {
        let (s, _) = stream_of("train-short", &format!("{HEADER}\n{}\n{{\"kind\":\"done\"}}\n", step(0)));
        assert_eq!(s.status, Status::Incomplete);
        let (s, _) = stream_of(
            "train-abort",
            &format!("{HEADER}\n{}\n{{\"kind\":\"abort\",\"step\":1,\"reason\":\"loss  went\\nNaN\"}}\n{{\"kind\":\"done\"}}\n", step(0)),
        );
        assert_eq!(s.status, Status::Aborted);
        assert_eq!(s.message, "Trainer aborted: loss went NaN");
    }

    #[test]
    fn a_partial_last_line_waits_for_its_end() {
        let (mut s, path) = stream_of("train-partial", &format!("{HEADER}\n{}", &step(0)[..20]));
        assert_eq!(s.seen(), 0);
        assert!(s.message.contains("partial record"));
        append(&path, &format!("{}\n", &step(0)[20..]));
        s.poll();
        assert_eq!(s.seen(), 1, "{}", s.message);
    }

    #[test]
    fn what_the_studio_refuses_is_refused_here() {
        let bad = [
            (format!("{}\n", step(0)), "outside an active stream"),
            (r#"{"kind":"header","steps":0,"n_experts":0}"#.to_string() + "\n", "positive step budget"),
            (format!("{HEADER}\n{}\n", step(1)), "missing, duplicated"),
            (format!("{HEADER}\n{}\n{}\n", step(0), step(0)), "missing, duplicated"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"step","step":0,"loss":-1}"#), "Invalid loss"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"step","step":0,"utilisation":[0.5,0.2]}"#), "do not sum to one"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"step","step":0,"utilisation":[1.0]}"#), "wrong size"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"step","step":0,"loss":1,"loss":2}"#), "duplicate key"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"step","step":0,"x":{"a":1,"\u0061":2}}"#), "duplicate key"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"step","step":0,"loss":1e400}"#), "Invalid loss"),
            (format!("{HEADER}\n{}\n", r#"{"kind":"weird"}"#), "Unsupported"),
            (format!("{HEADER}\n{HEADER}\n"), "second stream header"),
            (r#"{"kind":"header","steps":3,"n_experts":2,"dense":true}"#.to_string() + "\n", "conflict"),
            ("[1,2]\n".to_string(), "must be a JSON object"),
            ("{not json\n".to_string(), "malformed"),
        ];
        for (i, (text, why)) in bad.iter().enumerate() {
            let (mut s, _) = stream_of(&format!("train-bad-{i}"), text);
            assert_eq!(s.status, Status::Invalid, "case {i}: {text}");
            assert!(s.message.contains(why), "case {i}: {} lacks {why}", s.message);
            s.poll();
            assert_eq!(s.status, Status::Invalid, "a refused stream is not read again");
        }
    }

    #[test]
    fn the_same_key_in_two_objects_and_braces_inside_strings_are_not_duplicates() {
        let line = r#"{"kind":"step","step":0,"x":{"a":"\"}","b":1},"y":{"a":1}}"#;
        assert!(!has_duplicate_key(line));
        assert!(has_duplicate_key(r#"{"a":1,"b":{"c":2},"a":3}"#), "a key repeated after a nested object");
        let (s, _) = stream_of(
            "train-nested",
            &format!(
                "{HEADER}
{line}
"
            ),
        );
        assert_eq!(s.status, Status::Running, "{}", s.message);
    }

    #[test]
    fn a_file_truncated_or_rewritten_under_the_window_is_refused() {
        let (mut s, path) = stream_of("train-trunc", &format!("{HEADER}\n{}\n", step(0)));
        fs::write(&path, format!("{HEADER}\n")).unwrap();
        s.poll();
        assert_eq!(s.status, Status::Invalid);
        assert!(s.message.contains("truncated") || s.message.contains("replaced"), "{}", s.message);

        let (mut s, path) = stream_of("train-rewrite", &format!("{HEADER}\n{}\n", step(0)));
        let text = fs::read_to_string(&path).unwrap().replace("\"loss\":9", "\"loss\":7");
        let mut f = fs::OpenOptions::new().write(true).open(&path).unwrap();
        use std::io::Write;
        f.write_all(text.as_bytes()).unwrap();
        f.write_all(format!("{}\n", step(1)).as_bytes()).unwrap();
        drop(f);
        s.poll();
        assert_eq!(s.status, Status::Invalid, "{}", s.message);
        assert!(s.message.contains("modified"));
    }

    #[test]
    fn a_missing_file_waits_and_a_removed_one_is_refused() {
        let dir = crate::sqlite_ro::tests::scratch("train-missing");
        let mut s = Stream::new(dir.join("not-yet.jsonl"));
        s.poll();
        assert_eq!(s.status, Status::Waiting);
        let (mut s, path) = stream_of("train-removed", &format!("{HEADER}\n"));
        fs::remove_file(&path).unwrap();
        s.poll();
        assert_eq!(s.status, Status::Invalid);
    }

    #[test]
    fn only_the_newest_rows_are_kept_but_every_step_is_counted() {
        let mut text = r#"{"kind":"header","steps":1500,"n_experts":0,"dense":true}"#.to_string() + "\n";
        for n in 0..1200 {
            text += &format!("{{\"kind\":\"step\",\"step\":{n},\"loss\":1.0}}\n");
        }
        let (s, _) = stream_of("train-many", &text);
        assert_eq!((s.seen(), s.rows.len(), s.rows[0].step), (1200, MAX_ROWS, 200));
    }

    #[test]
    fn streams_on_offer_are_the_jsonl_files_that_start_with_a_header() {
        let repo = crate::sqlite_ro::tests::scratch("train-offer");
        let results = repo.join("results");
        fs::create_dir_all(&results).unwrap();
        fs::write(results.join("run.jsonl"), format!("{HEADER}\n{}\n", step(0))).unwrap();
        fs::write(results.join("sweep.jsonl"), "SWEEP-START\n{}\n").unwrap();
        fs::write(results.join("notes.txt"), "").unwrap();
        let o = offered(&results);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].what, "preset s, 1 of 2 experts, 3 steps");
        assert!(offered(&repo.join("nowhere")).is_empty());
    }
}
