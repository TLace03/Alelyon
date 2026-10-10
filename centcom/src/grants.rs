//! Grants: what Sinai may do beyond this computer, written by a person in the file Sinai's loop reads
//! (the companion program's authority rules), edited here instead of in Notepad.
//!
//! Since 2026-10-05 the page edits grants, with anything that adds authority asking first. The file stays the
//! person's: the loop only reads it, and the CHARTER's rule holds that a grant cannot authorise work on grants. So:
//!
//! - **The loop's file, by its own word.** The path is the one the loop names (`standing.path`); without it there is
//!   nothing to edit.
//! - **The loader's rules, before saving.** Every field the loop requires is required here (`issued_by`, `until`,
//!   `recipients`, `subjects`, `spend_limit`), recipients are exact (no `*` or `?`), and a little more: a name, a time
//!   above zero, a limit of zero or more. Fields this window does not know are kept as they were.
//! - **Every save is confirmed, with what it changes.** What adds authority (a new grant, a recipient, any subject, a
//!   higher limit, a longer time) is listed apart from what reduces it. Two effects of the file itself are always
//!   said: `until` counts from when the loop reads the file, so saving restarts every grant's clock; and the loop keeps
//!   a grant's spending under its name and note, so changing either starts its budget again.
//! - **Nothing over a changed file.** The file's size and time are taken when it is read; if either differs at
//!   saving, nothing is written. The write is a whole new file moved over the old, so the loop never reads half of
//!   one (a file it cannot read drops every grant).
//! - **Not while Sinai's hands are armed.** Sinai acts on this computer only through the window its hands are armed
//!   on; the grants are written only while they are not armed anywhere, so this window cannot be the way Sinai grants
//!   itself anything (checked by the window, `app.rs`).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Map, Value, json};

/// One grant as the person edits it: each field as typed, and what this window does not know, kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grant {
    pub issued_by: String,
    /// How long it lasts, in days (the file keeps seconds).
    pub days: String,
    /// Exact addresses, one per comma or line.
    pub recipients: String,
    /// Keywords; none means any subject.
    pub subjects: String,
    pub spend_limit: String,
    pub currency: String,
    pub note: String,
    /// The grant's place in the file as read (None: added here), and the fields this window does not edit.
    pub from: Option<usize>,
    pub rest: Map<String, Value>,
}

/// Which field of a grant is being typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    IssuedBy,
    Days,
    Recipients,
    Subjects,
    SpendLimit,
    Currency,
    Note,
}

impl Grant {
    pub fn set(&mut self, field: Field, value: String) {
        match field {
            Field::IssuedBy => self.issued_by = value,
            Field::Days => self.days = value,
            Field::Recipients => self.recipients = value,
            Field::Subjects => self.subjects = value,
            Field::SpendLimit => self.spend_limit = value,
            Field::Currency => self.currency = value,
            Field::Note => self.note = value,
        }
    }

    /// A blank grant, as the Angel window's template: a week, nothing to spend.
    pub fn blank() -> Grant {
        Grant { days: "7".into(), spend_limit: "0".into(), currency: "USD".into(), ..Grant::default() }
    }

    fn read(at: usize, v: &Value) -> Grant {
        let text = |key: &str| match v.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let list = |key: &str| {
            v.get(key).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(", ")).unwrap_or_default()
        };
        let days = v.get("until").and_then(Value::as_f64).map(days_of).unwrap_or_default();
        let mut rest = v.as_object().cloned().unwrap_or_default();
        for key in KNOWN {
            rest.remove(key);
        }
        Grant {
            issued_by: text("issued_by"),
            days,
            recipients: list("recipients"),
            subjects: list("subjects"),
            spend_limit: text("spend_limit"),
            currency: text("currency"),
            note: text("note"),
            from: Some(at),
            rest,
        }
    }
}

const KNOWN: [&str; 7] = ["issued_by", "until", "recipients", "subjects", "spend_limit", "currency", "note"];

/// Seconds as days, plainly: 604800 is "7", 43200 is "0.5".
fn days_of(seconds: f64) -> String {
    let days = seconds / 86_400.0;
    let text = format!("{days:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn items(text: &str) -> Vec<String> {
    text.split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The grants file, as read for editing: where it is, its size and time when read (None: there was no file), the
/// list as it stood, and the grants as they are being edited.
#[derive(Clone, Debug)]
pub struct Draft {
    pub path: PathBuf,
    pub stamp: Option<(u64, Option<SystemTime>)>,
    pub read: Vec<Value>,
    pub grants: Vec<Grant>,
}

/// The most of the grants file that is read; a person's list of grants is far smaller.
const MAX_FILE: u64 = 1 << 20;

fn stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| (m.len(), m.modified().ok()))
}

/// Read the grants file at `path` for editing. A missing file is an empty list (saving creates it); a file the loop
/// itself cannot read is refused here too, to be put right where it was written.
pub fn open(path: &Path) -> Result<Draft, String> {
    let stamp = stamp(path);
    let read = match stamp {
        None => Vec::new(),
        Some((size, _)) if size > MAX_FILE => return Err(format!("{} is larger than 1 MiB.", path.display())),
        Some(_) => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} is not readable as JSON ({e}); Sinai's loop loads none of it. Put it right in Notepad first.", path.display()))?;
            match v {
                Value::Array(list) => list,
                _ => return Err(format!("{} should hold a list of grants; Sinai's loop loads none of it.", path.display())),
            }
        }
    };
    let grants = read.iter().enumerate().map(|(i, v)| Grant::read(i, v)).collect();
    Ok(Draft { path: path.to_path_buf(), stamp, read, grants })
}

/// The grants as the file will hold them, or what is wrong with each, named by its place ("Grant 2: ...").
pub fn written(grants: &[Grant]) -> Result<Vec<Value>, Vec<String>> {
    let mut out = Vec::new();
    let mut wrong = Vec::new();
    for (i, g) in grants.iter().enumerate() {
        let n = i + 1;
        let mut bad = |why: String| wrong.push(format!("Grant {n}: {why}"));
        let issued_by = g.issued_by.trim();
        if issued_by.is_empty() {
            bad("it needs the name of the person who grants it (a person, not a role).".into());
        }
        let seconds = match g.days.trim().parse::<f64>() {
            Ok(days) if days.is_finite() && days > 0.0 => (days * 86_400.0).round(),
            _ => {
                bad("how long it lasts must be a number of days above zero.".into());
                0.0
            }
        };
        if seconds < 1.0 && g.days.trim().parse::<f64>().is_ok_and(|d| d > 0.0) {
            bad("it must last at least a second.".into());
        }
        let recipients = items(&g.recipients);
        if recipients.is_empty() {
            bad("it needs at least one recipient, by exact address.".into());
        }
        for r in &recipients {
            if r.contains(['*', '?']) {
                bad(format!("{r:?} has a wildcard; Sinai's loop refuses patterns, so name each recipient."));
            }
        }
        let spend = match g.spend_limit.trim().parse::<f64>() {
            Ok(limit) if limit.is_finite() && limit >= 0.0 => limit,
            _ => {
                bad("the spending limit must be a number, zero or more.".into());
                0.0
            }
        };
        let currency = if g.currency.trim().is_empty() { "USD".to_string() } else { g.currency.trim().to_string() };
        let mut grant = g.rest.clone();
        grant.insert("issued_by".into(), json!(issued_by));
        grant.insert("until".into(), json!(seconds as u64));
        grant.insert("recipients".into(), json!(recipients));
        grant.insert("subjects".into(), json!(items(&g.subjects)));
        grant.insert("spend_limit".into(), if spend.fract() == 0.0 { json!(spend as u64) } else { json!(spend) });
        grant.insert("currency".into(), json!(currency));
        if !g.note.trim().is_empty() {
            grant.insert("note".into(), json!(g.note.trim()));
        } else {
            grant.remove("note");
        }
        out.push(Value::Object(grant));
    }
    if wrong.is_empty() { Ok(out) } else { Err(wrong) }
}

/// What a save changes, in words: what adds authority, apart from what reduces it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    pub adds: Vec<String>,
    pub reduces: Vec<String>,
}

/// Always true of saving the grants file, from the loop's own rules.
pub const ALWAYS: [&str; 2] = [
    "Saving restarts every grant's clock: each lasts its full time again, counted from when Sinai's loop next reads the file.",
    "Sinai's loop keeps what a grant has spent under its name and note: a grant whose name or note changes starts its budget again.",
];

/// What turning the grants `read` into `written` changes. `grants` says which grant came from which place in `read`.
pub fn changes(read: &[Value], grants: &[Grant], written: &[Value]) -> Changes {
    let mut c = Changes::default();
    let name = |v: &Value| -> String {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let note = s("note");
        if note.is_empty() { format!("{}'s grant", s("issued_by")) } else { format!("{}'s grant \"{note}\"", s("issued_by")) }
    };
    let list = |v: &Value, k: &str| -> Vec<String> {
        v.get(k).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default()
    };
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let kept: Vec<usize> = grants.iter().filter_map(|g| g.from).collect();
    for (i, old) in read.iter().enumerate() {
        if !kept.contains(&i) {
            c.reduces.push(format!("{} is removed.", name(old)));
        }
    }
    for (g, new) in grants.iter().zip(written) {
        let Some(old) = g.from.and_then(|i| read.get(i)) else {
            let subjects = list(new, "subjects");
            c.adds.push(format!(
                "{} is new: {} recipient(s), {}, up to {} {}, for {} day(s).",
                name(new),
                list(new, "recipients").len(),
                if subjects.is_empty() { "any subject".to_string() } else { format!("subjects {}", subjects.join(", ")) },
                num(new, "spend_limit"),
                new.get("currency").and_then(Value::as_str).unwrap_or("USD"),
                days_of(num(new, "until"))
            ));
            continue;
        };
        let who = name(new);
        let (was, now) = (list(old, "recipients"), list(new, "recipients"));
        for r in now.iter().filter(|r| !was.contains(r)) {
            c.adds.push(format!("{who} may now reach {r}."));
        }
        for r in was.iter().filter(|r| !now.contains(r)) {
            c.reduces.push(format!("{who} may no longer reach {r}."));
        }
        let (was, now) = (list(old, "subjects"), list(new, "subjects"));
        if !was.is_empty() && now.is_empty() {
            c.adds.push(format!("{who} now covers any subject (it was limited to {}).", was.join(", ")));
        } else if was.is_empty() && !now.is_empty() {
            c.reduces.push(format!("{who} is now limited to {} (it covered any subject).", now.join(", ")));
        } else {
            for s in now.iter().filter(|s| !was.contains(s)) {
                c.adds.push(format!("{who} now covers the subject {s}."));
            }
            for s in was.iter().filter(|s| !now.contains(s)) {
                c.reduces.push(format!("{who} no longer covers the subject {s}."));
            }
        }
        let (was, now) = (num(old, "spend_limit"), num(new, "spend_limit"));
        if now > was {
            c.adds.push(format!("{who} may spend up to {now} instead of {was}."));
        } else if now < was {
            c.reduces.push(format!("{who} may spend up to {now} instead of {was}."));
        }
        let currency = |v: &Value| v.get("currency").and_then(Value::as_str).unwrap_or("USD").to_string();
        if currency(old) != currency(new) {
            c.adds.push(format!("{who} now counts its limit in {} instead of {}.", currency(new), currency(old)));
        }
        let (was, now) = (num(old, "until"), num(new, "until"));
        if now > was {
            c.adds.push(format!("{who} lasts {} day(s) instead of {}.", days_of(now), days_of(was)));
        } else if now < was {
            c.reduces.push(format!("{who} lasts {} day(s) instead of {}.", days_of(now), days_of(was)));
        }
        if name(old) != who {
            c.adds.push(format!("{who} was {}: its spending starts again from nothing.", name(old)));
        }
    }
    c
}

/// What keeps a save from being written.
pub const CHANGED: &str = "The grants file changed since it was opened here, so nothing was written. Open it again to see what changed.";

/// Write `list` to the draft's file: only if the file is as it was when read, as a whole new file moved over the old.
pub fn write(draft: &Draft, list: &[Value]) -> Result<(), String> {
    if stamp(&draft.path) != draft.stamp {
        return Err(CHANGED.into());
    }
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())? + "\n";
    let mut temp = draft.path.clone().into_os_string();
    temp.push(".centcom-new");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text.as_bytes()).map_err(|e| format!("{}: {e}", temp.display()))?;
    if let Err(e) = std::fs::rename(&temp, &draft.path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {e}", draft.path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("centcom-grants-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The example in AUTHORITY.md, with a field this window does not know.
    fn press_grant() -> Value {
        json!({"issued_by": "Thomas Lacy", "until": 604800, "recipients": ["press@example.com", "editor@example.com"],
            "subjects": ["launch", "availability"], "spend_limit": 0, "currency": "USD",
            "note": "launch announcement, press list only", "reviewed": "2026-10-05"})
    }

    #[test]
    fn a_grants_file_is_read_and_written_back_as_it_was() {
        let dir = scratch("roundtrip");
        let path = dir.join("authority.json");
        std::fs::write(&path, serde_json::to_string_pretty(&json!([press_grant()])).unwrap()).unwrap();
        let draft = open(&path).unwrap();
        let g = &draft.grants[0];
        assert_eq!((g.days.as_str(), g.recipients.as_str(), g.subjects.as_str()), ("7", "press@example.com, editor@example.com", "launch, availability"));
        let list = written(&draft.grants).unwrap();
        assert_eq!(list, [press_grant()], "unchanged, a field this window does not know included");
        assert_eq!(changes(&draft.read, &draft.grants, &list), Changes::default());
        write(&draft, &list).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(), json!([press_grant()]));
        assert!(!dir.join("authority.json.centcom-new").exists(), "the new file was moved over the old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_an_empty_list_and_a_broken_one_is_refused() {
        let dir = scratch("missing");
        let path = dir.join("authority.json");
        let draft = open(&path).unwrap();
        assert!(draft.read.is_empty() && draft.stamp.is_none());
        std::fs::write(&path, "{ not json").unwrap();
        assert!(open(&path).unwrap_err().contains("loads none of it"));
        std::fs::write(&path, "{\"issued_by\": \"Tom\"}").unwrap();
        assert!(open(&path).unwrap_err().contains("list of grants"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_loaders_rules_hold_before_anything_is_written() {
        let ok = Grant { issued_by: "Tom".into(), recipients: "a@example.com".into(), ..Grant::blank() };
        assert!(written(std::slice::from_ref(&ok)).is_ok());
        let cases = [
            (Grant { issued_by: " ".into(), ..ok.clone() }, "name of the person"),
            (Grant { days: "0".into(), ..ok.clone() }, "above zero"),
            (Grant { days: "soon".into(), ..ok.clone() }, "above zero"),
            (Grant { recipients: "".into(), ..ok.clone() }, "at least one recipient"),
            (Grant { recipients: "*@alelyon.com".into(), ..ok.clone() }, "wildcard"),
            (Grant { recipients: "a@example.com, b?@example.com".into(), ..ok.clone() }, "wildcard"),
            (Grant { spend_limit: "-1".into(), ..ok.clone() }, "zero or more"),
        ];
        for (grant, said) in cases {
            let wrong = written(&[ok.clone(), grant.clone()]).unwrap_err();
            assert!(wrong.iter().any(|w| w.starts_with("Grant 2: ") && w.contains(said)), "{grant:?}: {wrong:?}");
        }
    }

    #[test]
    fn what_adds_authority_is_said_apart_from_what_reduces_it() {
        let read = vec![press_grant(), json!({"issued_by": "Thomas Lacy", "until": 3600, "recipients": ["a@example.com"],
            "subjects": [], "spend_limit": 50, "currency": "USD", "note": "groceries"})];
        let draft = Draft { path: PathBuf::new(), stamp: None, grants: read.iter().enumerate().map(|(i, v)| Grant::read(i, v)).collect(), read: read.clone() };
        let mut grants = draft.grants.clone();
        // the press grant: one more recipient, one fewer, every subject, a higher limit, longer
        grants[0].recipients = "press@example.com, desk@example.com".into();
        grants[0].subjects = "".into();
        grants[0].spend_limit = "10".into();
        grants[0].days = "14".into();
        // the groceries grant is removed; a new one is added
        grants.remove(1);
        grants.push(Grant { issued_by: "Thomas Lacy".into(), recipients: "shop@example.com".into(), note: "flowers".into(), ..Grant::blank() });
        let list = written(&grants).unwrap();
        let c = changes(&draft.read, &grants, &list);
        let has = |list: &[String], words: &str| list.iter().any(|s| s.contains(words));
        assert!(has(&c.adds, "may now reach desk@example.com"), "{c:?}");
        assert!(has(&c.reduces, "may no longer reach editor@example.com"), "{c:?}");
        assert!(has(&c.adds, "now covers any subject"), "emptying the subjects widens a grant: {c:?}");
        assert!(has(&c.adds, "may spend up to 10 instead of 0"), "{c:?}");
        assert!(has(&c.adds, "lasts 14 day(s) instead of 7"), "{c:?}");
        assert!(has(&c.reduces, "\"groceries\" is removed"), "{c:?}");
        assert!(has(&c.adds, "\"flowers\" is new"), "{c:?}");
        // a renamed note starts its budget again: that adds authority too
        let mut renamed = draft.grants.clone();
        renamed[1].note = "food".into();
        let c = changes(&draft.read, &renamed, &written(&renamed).unwrap());
        assert!(has(&c.adds, "spending starts again"), "{c:?}");
    }

    #[test]
    fn nothing_is_written_over_a_file_that_changed_since_it_was_read() {
        let dir = scratch("changed");
        let path = dir.join("authority.json");
        std::fs::write(&path, "[]").unwrap();
        let draft = open(&path).unwrap();
        std::fs::write(&path, "[ ]  ").unwrap();
        assert_eq!(write(&draft, &[]).unwrap_err(), CHANGED);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[ ]  ", "the person's file is as they left it");
        // and a file that appeared where there was none is not overwritten either
        let absent = dir.join("other.json");
        let draft = open(&absent).unwrap();
        std::fs::write(&absent, "[]").unwrap();
        assert_eq!(write(&draft, &[]).unwrap_err(), CHANGED);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
