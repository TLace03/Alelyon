//! Which obligations each agent session has been OBSERVED to discharge, replayed from the Action Gate's ledger:
//! the Python action gate's `derive_evidence`, rule for rule (the Python desktop's Action
//! Gate view shows the same per session).
//!
//! Every obligation is derived from records the harness wrote when a tool call happened; nothing a session declares
//! about itself is taken as held, except its falsifier, which is a declaration by nature:
//! - `P0_WORKTREE`: an observed `git status`;
//! - `P1_POLICY`: an observed read of `AGENTS.md`;
//! - `P3_BASELINE`: an observed test run (pytest, cargo test, tools/ci.py, dotnet test, npm test) WITH an exit status
//!   recorded; a run without one is noted as UNMEASURED, never a baseline;
//! - `P4_FALSIFIER`: a declared falsifier;
//! - `P5_CLAIM_SCOPE`: an observed read of `docs/cne/CLAIMS.md`;
//! - `OWNER_AUTHORITY`: an owner grant that has not expired.
//!
//! A command that moves the tree (merge, rebase, pull, reset, checkout, switch, cherry-pick, stash, apply, am) lapses
//! P0, P3 and P4: the evidence described a tree that no longer exists.
//!
//! As the Python view calls it, the current `AGENTS.md` is not compared: a read of any version holds P1 here (the
//! Python function lapses P1 when the policy text changed only when it is given that text, and the view does not give
//! it).

use std::collections::BTreeSet;

use serde_json::Value;

use super::system::Record;

/// Commands whose observation discharges P0 (`_P0_COMMANDS`).
const P0_COMMANDS: [&str; 1] = ["git status"];
/// Test runners whose observation discharges P3 (`_P3_COMMANDS`).
const P3_COMMANDS: [&str; 5] = ["pytest", "cargo test", "tools/ci.py", "dotnet test", "npm test"];
/// Commands after which an earlier P0, P3 or P4 says nothing (`_INVALIDATING`).
const INVALIDATING: [&str; 10] = [
    "git merge",
    "git rebase",
    "git pull",
    "git reset",
    "git checkout",
    "git switch",
    "git cherry-pick",
    "git stash",
    "git apply",
    "git am",
];

/// What one session has been observed to have done (`Evidence`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Evidence {
    /// The obligations held, sorted.
    pub held: Vec<String>,
    pub policy_digest_seen: String,
    pub falsifier: String,
    pub grants: Vec<String>,
    pub notes: Vec<String>,
}

/// Python's `str(x)` of a JSON value.
fn py_str(value: Option<&Value>, default: &str) -> String {
    match value {
        None => default.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "None".to_string(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.to_string(),
        Some(other) => other.to_string(),
    }
}

/// Python's `repr(s)` of a string, as `{scope!r}` writes it.
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `int(x)` for an expiry: an integer, a float truncated, or a string spelling an integer.
fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

/// The first `n` characters of `s`, as Python's `s[:n]`.
fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `derive_evidence(session, path=..., now=now)` over the ledger's well-formed records.
pub fn derive(records: &[Record], session: &str, now: i64) -> Evidence {
    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut notes = Vec::new();
    let mut falsifier = String::new();
    let mut grants = Vec::new();
    let mut policy_digest_seen = String::new();
    for rec in records.iter().filter(|r| r.session == session) {
        let payload = &rec.payload;
        let get = |key: &str| payload.as_object().and_then(|o| o.get(key));
        match rec.kind.as_str() {
            "observation" => {
                let command = py_str(get("command"), "");
                let tool = py_str(get("tool"), "");
                let low = command.to_lowercase();
                if INVALIDATING.iter().any(|inv| low.contains(inv)) {
                    for lapsed in ["P0_WORKTREE", "P3_BASELINE", "P4_FALSIFIER"] {
                        if held.remove(lapsed) {
                            notes.push(format!(
                                "{lapsed} lapsed: `{}` changed the tree the evidence described.",
                                head(command.trim(), 60)
                            ));
                        }
                    }
                    if !held.contains("P4_FALSIFIER") {
                        falsifier.clear();
                    }
                }
                if P0_COMMANDS.iter().any(|c| low.contains(c)) {
                    held.insert("P0_WORKTREE".into());
                }
                if P3_COMMANDS.iter().any(|c| low.contains(c)) {
                    if matches!(get("exit_status"), None | Some(Value::Null)) {
                        notes.push(
                            "a test command was seen but no exit status was recorded; that is UNMEASURED, not a \
                             baseline."
                                .into(),
                        );
                    } else {
                        held.insert("P3_BASELINE".into());
                    }
                }
                if tool == "Read" {
                    let read_path = py_str(get("path"), "").replace('\\', "/");
                    let seen = py_str(get("content_digest"), "");
                    if read_path.ends_with("AGENTS.md") {
                        policy_digest_seen = seen;
                        // No current policy text is given (as the view calls it), so any read holds P1.
                        held.insert("P1_POLICY".into());
                    }
                    if read_path.ends_with("docs/cne/CLAIMS.md") {
                        held.insert("P5_CLAIM_SCOPE".into());
                    }
                }
            }
            "declaration" if get("what").and_then(Value::as_str) == Some("falsifier") => {
                falsifier = py_str(get("text"), "");
                if !falsifier.trim().is_empty() {
                    held.insert("P4_FALSIFIER".into());
                }
            }
            "grant" => {
                let scope = py_str(get("scope"), "");
                if let Some(expires) = get("expires_ts").filter(|v| !v.is_null())
                    && py_int(expires).is_some_and(|e| now > e)
                {
                    notes.push(format!("owner grant for {} expired.", py_repr(&scope)));
                    continue;
                }
                grants.push(scope);
                held.insert("OWNER_AUTHORITY".into());
            }
            _ => {}
        }
    }
    Evidence { held: held.into_iter().collect(), policy_digest_seen, falsifier, grants, notes }
}

/// What an obligation means, in a line.
pub fn meaning(obligation: &str) -> &'static str {
    match obligation {
        "P0_WORKTREE" => "looked at the worktree (git status) since the tree last moved",
        "P1_POLICY" => "read AGENTS.md",
        "P3_BASELINE" => "ran the tests and its exit status was recorded",
        "P4_FALSIFIER" => "declared what would prove the change wrong",
        "P5_CLAIM_SCOPE" => "read the claim discipline (docs/cne/CLAIMS.md)",
        "OWNER_AUTHORITY" => "holds an owner grant that has not expired",
        _ => "",
    }
}

/// The sessions in the ledger, in the order each first appears.
pub fn sessions(records: &[Record]) -> Vec<String> {
    let mut seen = Vec::new();
    for r in records {
        if !seen.contains(&r.session) {
            seen.push(r.session.clone());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from the real `ledger.append` and `derive_evidence` (2026-10-07): four sessions over one ledger.
    const FIXTURE: &str = include_str!("fixtures/derive_evidence.json");

    #[test]
    fn every_session_derives_what_python_derives() {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let dir = std::env::temp_dir().join(format!("centcom-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("action_gate.jsonl");
        std::fs::write(&path, fixture["ledger"].as_str().unwrap()).unwrap();
        let ledger = super::super::system::read_ledger(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        assert!(ledger.verdict.ok, "the fixture's chain re-derives: {}", ledger.verdict.reason);
        let now = fixture["now"].as_i64().unwrap();
        for (session, want) in fixture["sessions"].as_object().unwrap() {
            let got = derive(&ledger.records, session, now);
            let strings = |v: &Value| -> Vec<String> {
                v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
            };
            assert_eq!(got.held, strings(&want["held"]), "{session}: held");
            assert_eq!(got.notes, strings(&want["notes"]), "{session}: notes");
            assert_eq!(got.grants, strings(&want["grants"]), "{session}: grants");
            assert_eq!(got.falsifier, want["falsifier"].as_str().unwrap(), "{session}: falsifier");
            assert_eq!(got.policy_digest_seen, want["policy_digest_seen"].as_str().unwrap(), "{session}");
        }
    }

    #[test]
    fn a_repr_quotes_as_python_does() {
        assert_eq!(py_repr("old grant"), "'old grant'");
        assert_eq!(py_repr("it's"), "\"it's\"");
        assert_eq!(py_repr("a\tb\\"), "'a\\tb\\\\'");
    }
}
