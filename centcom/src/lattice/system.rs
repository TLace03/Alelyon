//! The Lattice page's System tab, read-only: the verification stack this build can reach, and the agents' Action
//! Gate ledger with its hash chain re-derived here (the Python desktop's Stack, Action Gate and Ledgers views).
//!
//! The ledger is `ALELYON_ACTION_GATE_LEDGER`, else `<globals>/action_gate.jsonl`, as
//! the Python action gate places it. Each link is re-derived from its own content: SHA-256
//! over the canonical JSON of `{"v":1,"seq","ts","session","kind","payload","prev_hash"}` (sorted keys, no spaces,
//! non-ASCII as itself, no floating point), the rule `canonical.py` freezes. A chain that verifies says no record was
//! edited or removed from the middle; it cannot say the end was not cut off (`head_marker`'s caveat), and an empty
//! ledger verifies as nothing recorded, which is also what a deleted one looks like.
//!
//! The Python Ledgers view also counts the trading gate's decisions (`gate_decisions.jsonl`) and the intent ledger
//! (`fam.db`): both are the Financial Markets platform's, which Alelyon leaves out for now (that work was paused
//! on 2026-10-07 and `fam.db` was kept out on 2026-10-05), so this tab names them and does not read them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// How well a stack row's claim is evidenced: the Python view's closed vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// This build can reach it and it declared itself.
    Present,
    /// It exists, but its claim depends on something outside this build.
    Gated,
    /// Not in this build.
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackRow {
    pub name: &'static str,
    pub level: Level,
    pub headline: String,
    pub detail: &'static str,
}

/// What this stack does not establish, as the Python view prints it.
pub const STANDING_GAPS: [&str; 4] = [
    "No external verification is recorded: no party outside this organisation has verified an Alelyon envelope.",
    "No genuinely external witness is operating. Same-organisation infrastructure is staging, not independence.",
    "No owner-approved public deterministic-kernel distribution exists for substrate-sensitive nonzero widths.",
    "Nothing in this stack checks the model's REASONING. It checks where figures came from and whether a computation \
     replays.",
];

/// The stack's rows, for a window running from `checkout` (if any).
pub fn stack(checkout: Option<&Path>) -> Vec<StackRow> {
    let vectors = checkout.map(|root| count_vectors(&root.join("alelyon").join("verify").join("vectors")));
    let vector_note = match vectors {
        Some(Some(n)) => format!("{n} conformance vectors in this checkout"),
        _ => "conformance vectors UNMEASURED in this build".to_string(),
    };
    let accs = checkout.is_some_and(|root| root.join("research").join("experiments").join("accs").is_dir());
    vec![
        StackRow {
            name: "CNE — Certified Number Envelope",
            level: Level::Present,
            headline: format!(
                "spec {}; envelope type {}; kernel {}",
                alelyon_verify::SPEC_VERSION,
                alelyon_verify::ENVELOPE_TYPE,
                crate::installed::replay()
                    .map(|k| k.id().to_owned())
                    .unwrap_or_else(|| "none in this build (receipts are checked, not replayed)".to_owned())
            ),
            detail: "A figure travels with its inputs' digest, its computation and its error terms. Successful \
                     verification detects revision of committed inputs; it does not establish that a producer's inputs \
                     were true.",
        },
        StackRow {
            name: "Open Verifier (alelyon-verify)",
            level: Level::Present,
            headline: format!("the Rust verifier, linked into this window; {vector_note}"),
            detail: "A third party can check a receipt without this application and without trusting it. What is \
                     linked here is the SOURCE verifier; whether a matching package is published, and whether anyone \
                     outside this organisation has ever run it against a real envelope, are separate facts this screen \
                     cannot establish.",
        },
        if accs {
            StackRow {
                name: "ACCS — Asynchronous Certified Conclusion Search",
                level: Level::Gated,
                headline: "present in this checkout as a research experiment".to_string(),
                detail: "Proves that a selected path is the utility maximiser over a SUPPLIED, committed finite tree. It \
                         says nothing about conclusions left out of that tree, its certificate is not a CNE and is not \
                         authenticated, and it is not wired into this product's answer path.",
            }
        } else {
            StackRow {
                name: "ACCS — Asynchronous Certified Conclusion Search",
                level: Level::Absent,
                headline: "not included in this build".to_string(),
                detail: "The research tree is excluded from distributed artifacts.",
            }
        },
        morphometry_row(),
    ]
}

/// Model Morphometry: the linked C++ model-anatomy engine states its canonical space
/// and that space's commitment, as the Python Stack view does from `morphometry.space_commitment()`. The commitment is
/// computed by the engine on every read, not a figure copied here; the Python view's certificate claim is not made,
/// because this window issues none.
fn morphometry_row() -> StackRow {
    match model_anatomy::canonical_space() {
        Ok(space) => StackRow {
            name: "Model Morphometry",
            level: Level::Present,
            headline: format!("template {} v{} · {}", space.space_id, space.version, space.commitment),
            detail: "Arranges a model's declared anatomy on an immutable canonical coordinate space and registers it there \
                     with an exact transform (the C++ port, linked into this window; the Lattice page's Morphometry tab). \
                     The commitment above covers the FRAME: it binds none of the measured figures, the measurements \
                     describe declared structure, not learned behaviour, and this window issues no registration \
                     certificate.",
        },
        Err(e) => StackRow {
            name: "Model Morphometry",
            level: Level::Absent,
            headline: format!("the linked C++ engine did not answer: {e}"),
            detail: "",
        },
    }
}

/// How many published vectors a folder holds (`*.json` but the manifest), or `None` when it cannot be read.
fn count_vectors(dir: &Path) -> Option<usize> {
    let entries = std::fs::read_dir(dir).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".json") && name != "manifest.json"
            })
            .count(),
    )
}

// ------------------------------------------------------------------ the action gate's ledger

/// The first link's predecessor (`ledger.GENESIS`).
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// The ledger's link rule version (`ledger.LEDGER_VERSION`).
const LEDGER_VERSION: i64 = 1;
/// The most records the tab lists, newest first.
pub const SHOWN: usize = 200;

/// Where the ledger lives: `ALELYON_ACTION_GATE_LEDGER`, else `<globals>/action_gate.jsonl`.
pub fn ledger_path(globals: &Path) -> PathBuf {
    match std::env::var_os("ALELYON_ACTION_GATE_LEDGER").filter(|v| !v.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => globals.join("action_gate.jsonl"),
    }
}

/// One well-formed record (`ledger.Record`).
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub seq: i64,
    pub ts: i64,
    pub session: String,
    pub kind: String,
    pub payload: Value,
    pub prev_hash: String,
    pub leaf_hash: String,
}

/// Whether the chain re-derives (`ledger.ChainVerdict`): `ok` is never inferred from absence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub ok: bool,
    pub checked: usize,
    pub reason: String,
    pub first_bad_seq: Option<i64>,
    pub unparseable: usize,
}

/// The ledger as the tab shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ledger {
    pub path: PathBuf,
    /// The file is absent: nothing was recorded, or it was deleted (the two look alike).
    pub absent: bool,
    pub verdict: Verdict,
    /// Every well-formed record, in file order (the tab shows the newest [`SHOWN`]).
    pub records: Vec<Record>,
    /// Records per kind and per session.
    pub kinds: BTreeMap<String, usize>,
    pub sessions: BTreeMap<String, usize>,
}

impl Ledger {
    /// The head to keep somewhere this session cannot reach (`head_marker`): the count, the last leaf, its seq.
    pub fn head(&self) -> (usize, &str, i64) {
        match self.records.last() {
            Some(last) => (self.records.len(), last.leaf_hash.as_str(), last.seq),
            None => (0, GENESIS, -1),
        }
    }
}

/// Read and check the ledger at `path`. Never writes.
pub fn read_ledger(path: &Path) -> Result<Ledger, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Ledger {
                path: path.to_path_buf(),
                absent: true,
                verdict: Verdict { ok: true, checked: 0, reason: String::new(), first_bad_seq: None, unparseable: 0 },
                records: Vec::new(),
                kinds: BTreeMap::new(),
                sessions: BTreeMap::new(),
            });
        }
        Err(e) => return Err(format!("The action gate's ledger could not be read: {e}")),
    };
    let text = String::from_utf8(bytes).map_err(|_| "The action gate's ledger is not UTF-8 text.".to_string())?;
    // `_read_raw`: each non-blank line, stripped, parsed or kept as unparseable.
    let rows: Vec<Option<Value>> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();
    let verdict = verify(&rows);
    let records: Vec<Record> = rows.iter().flatten().filter_map(record_of).collect();
    let mut kinds = BTreeMap::new();
    let mut sessions = BTreeMap::new();
    for r in &records {
        *kinds.entry(r.kind.clone()).or_insert(0) += 1;
        *sessions.entry(r.session.clone()).or_insert(0) += 1;
    }
    Ok(Ledger { path: path.to_path_buf(), absent: false, verdict, records, kinds, sessions })
}

/// Python's `int(x)` on what JSON gives it: an integer, or a string spelling one (a float or bool is refused here,
/// where Python would truncate a float; a ledger written by `append` carries integers only).
fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Python's `str(x)`, for the fields the ledger stores as strings.
fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn fields(row: &Value) -> Option<(i64, i64, String, String, Value, String, String)> {
    let o = row.as_object()?;
    // `row.get("payload") or {}`: any falsy value (absent, null, false, 0, "", [], {}) is an empty object.
    let payload = match o.get("payload") {
        Some(v) if truthy(v) => v.clone(),
        _ => Value::Object(Default::default()),
    };
    Some((
        int_of(o.get("seq")?)?,
        int_of(o.get("ts")?)?,
        str_of(o.get("session")?)?,
        str_of(o.get("kind")?)?,
        payload,
        str_of(o.get("prev_hash")?)?,
        str_of(o.get("leaf_hash")?)?,
    ))
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn record_of(row: &Value) -> Option<Record> {
    let (seq, ts, session, kind, payload, prev_hash, leaf_hash) = fields(row)?;
    Some(Record { seq, ts, session, kind, payload, prev_hash, leaf_hash })
}

/// `verify_chain`, rule for rule.
fn verify(rows: &[Option<Value>]) -> Verdict {
    let unparseable = rows.iter().filter(|r| r.is_none()).count();
    let mut prev = GENESIS.to_string();
    let mut checked = 0usize;
    let bad = |checked, reason: String, seq| Verdict { ok: false, checked, reason, first_bad_seq: seq, unparseable };
    for (i, row) in rows.iter().enumerate() {
        let Some(row) = row else {
            return bad(checked, format!("line {} is not valid JSON; the chain cannot be continued past it", i + 1), None);
        };
        let Some((seq, ts, session, kind, payload, stored_prev, stored_leaf)) = fields(row) else {
            return bad(checked, format!("record {i} is missing chain fields"), None);
        };
        if seq != checked as i64 {
            return bad(
                checked,
                format!(
                    "sequence jumped: expected {checked}, found {seq}. Records were removed or two sessions appended at \
                     once (a FORK)."
                ),
                Some(seq),
            );
        }
        if stored_prev != prev {
            return bad(
                checked,
                format!(
                    "record {seq} does not follow record {}: its prev_hash is not the previous leaf. Something between \
                     them was edited or removed.",
                    seq - 1
                ),
                Some(seq),
            );
        }
        let link = serde_json::json!({
            "v": LEDGER_VERSION, "seq": seq, "ts": ts, "session": session, "kind": kind, "payload": payload,
            "prev_hash": stored_prev,
        });
        let recomputed = match canonical(&link) {
            Ok(bytes) => lattice_core::sha::sha256_hex(&bytes),
            Err(why) => {
                return bad(checked, format!("record {seq} has no canonical encoding ({why}), so its link cannot be re-derived"), Some(seq));
            }
        };
        if recomputed != stored_leaf {
            return bad(
                checked,
                format!("record {seq} was edited after it was written: its content does not hash to its stored leaf_hash."),
                Some(seq),
            );
        }
        prev = stored_leaf;
        checked += 1;
    }
    Verdict { ok: true, checked, reason: String::new(), first_bad_seq: None, unparseable }
}

/// `canonical.canonical`: sorted keys, `,` and `:` with no spaces, non-ASCII as itself, integers within int64 only
/// (no floating point, no NaN).
pub fn canonical(value: &Value) -> Result<Vec<u8>, String> {
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out.into_bytes())
}

fn write(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            // The integer's own text, kept exactly (arbitrary_precision); anything else is a float.
            let text = n.to_string();
            if n.as_i64().is_none() || text.contains(['.', 'e', 'E']) {
                return Err(format!("{text} is not an int64 integer"));
            }
            out.push_str(&text);
        }
        Value::String(s) => string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            // Python sorts by code point, which is UTF-8 byte order.
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(key, out);
                out.push(':');
                write(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// A string as `json.dumps(..., ensure_ascii=False)` spells it.
fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two records written by the real `ledger.append` (2026-10-07), with a payload that exercises the encoding:
    /// non-ASCII, quotes, U+2028, a tab, a control character, nested lists and objects, null and true.
    const PYTHON_LEDGER: &str = "{\"kind\":\"decision\",\"leaf_hash\":\"2444566890dcc965cbfc6ed0a6731c91b08c60e7d07c89ff9075fef77b24b729\",\"payload\":{\"axis\":\"write\",\"n\":[1,-2,{\"a\":null,\"b\":true}],\"note\":\"caf\u{e9} \\\"q\\\" \u{2028} tab\\t\\u0001\",\"verdict\":\"allow\"},\"prev_hash\":\"0000000000000000000000000000000000000000000000000000000000000000\",\"seq\":0,\"session\":\"sess-1\",\"ts\":1786000000}\n{\"kind\":\"observation\",\"leaf_hash\":\"250dfd7a4e4016029ce2acc5fc3b17dd138b606330dc9d6744a553addeb76c5a\",\"payload\":{\"command\":\"git status\",\"exit_status\":0,\"tool\":\"Bash\"},\"prev_hash\":\"2444566890dcc965cbfc6ed0a6731c91b08c60e7d07c89ff9075fef77b24b729\",\"seq\":1,\"session\":\"sess-2\",\"ts\":1786000005}\n";

    fn ledger(text: &str) -> Ledger {
        let dir = std::env::temp_dir().join(format!("centcom-gate-{}-{}", std::process::id(), text.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("action_gate.jsonl");
        std::fs::write(&path, text).unwrap();
        let read = read_ledger(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        read
    }

    #[test]
    fn a_ledger_python_wrote_re_derives_link_for_link() {
        let l = ledger(PYTHON_LEDGER);
        assert_eq!(l.verdict, Verdict { ok: true, checked: 2, reason: String::new(), first_bad_seq: None, unparseable: 0 });
        assert_eq!(l.head(), (2, "250dfd7a4e4016029ce2acc5fc3b17dd138b606330dc9d6744a553addeb76c5a", 1));
        assert_eq!(l.kinds.get("decision"), Some(&1));
        assert_eq!(l.sessions.len(), 2);
    }

    #[test]
    fn an_edited_payload_breaks_its_link() {
        let l = ledger(&PYTHON_LEDGER.replace("\"allow\"", "\"refuse\""));
        assert!(!l.verdict.ok);
        assert_eq!(l.verdict.first_bad_seq, Some(0));
        assert!(l.verdict.reason.contains("edited after it was written"), "{}", l.verdict.reason);
    }

    #[test]
    fn a_removed_record_is_a_jump_and_a_torn_line_stops_the_chain() {
        let second = PYTHON_LEDGER.lines().nth(1).unwrap();
        let l = ledger(&format!("{second}\n"));
        assert!(!l.verdict.ok && l.verdict.reason.contains("sequence jumped"));
        let l = ledger(&format!("{PYTHON_LEDGER}{{\"seq\":2,\"kin"));
        assert!(!l.verdict.ok);
        assert_eq!((l.verdict.checked, l.verdict.unparseable), (2, 1));
        assert!(l.verdict.reason.starts_with("line 3 is not valid JSON"));
    }

    #[test]
    fn an_absent_ledger_is_nothing_recorded_never_a_clean_bill() {
        let l = read_ledger(Path::new("C:/definitely/not/here/action_gate.jsonl")).unwrap();
        assert!(l.absent && l.records.is_empty());
        assert_eq!(l.head(), (0, GENESIS, -1));
    }

    #[test]
    fn the_canonical_form_refuses_floating_point_and_sorts_keys() {
        let v: Value = serde_json::from_str("{\"b\":1,\"a\":[true,null]}").unwrap();
        assert_eq!(canonical(&v).unwrap(), b"{\"a\":[true,null],\"b\":1}");
        let f: Value = serde_json::from_str("{\"x\":1.5}").unwrap();
        assert!(canonical(&f).is_err());
    }

    #[test]
    fn the_stack_reports_the_linked_verifier_and_the_linked_morphometry_engine() {
        let rows = stack(None);
        assert_eq!(rows.len(), 4);
        assert!(rows[0].headline.contains(alelyon_verify::SPEC_VERSION));
        assert!(rows[1].headline.contains("UNMEASURED"));
        assert_eq!(rows[2].level, Level::Absent);
        // PRESENT only because the linked engine answered, and its figures are the engine's own.
        let space = model_anatomy::canonical_space().expect("the linked engine answers");
        assert_eq!(rows[3].level, Level::Present);
        assert!(rows[3].headline.contains(&space.commitment) && space.commitment.starts_with("sha256:"));
        assert!(rows[3].headline.contains(&format!("{} v{}", space.space_id, space.version)));
        assert!(rows[3].detail.contains("issues no registration certificate"), "no certificate claim");
    }
}
