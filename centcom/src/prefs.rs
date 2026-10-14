//! Alelyon's own preferences on this PC: a small JSON object in `~/.alelyon/alelyon/preferences.json`
//! (`CENTCOM_PREFERENCES_FILE` names another file). Today it holds where the window was left (`"session"`, `session`)
//! and four choices: whether the sign-in backdrop moves
//! (`"backdrop_motion": true | false`), where the account panel's web pages open (`"web_pages": "lattice" |
//! "yours"`, `signin::web`), the model of the person's own that Sinai's page talks to (`"sinai_model":
//! "gguf:<name>" | "endpoint:<id>"`, `models`; never a key, which is Credential Manager's), and the model chosen for
//! each of Sinai's parts (`"parts"`, a versioned object whose shape `sinai_parts` documents), and Auto configure's
//! dials, paid allowances and previous setup (`"sinai_auto"`, `sinai_parts::auto`); each is absent when the person
//! never chose, and each is written on its own, keeping the others.
//!
//! Read once at start. A missing, unreadable or malformed file is no preference at all (never an error on screen);
//! keys this version does not know are kept when it writes. Each write is a whole new file moved over the old one,
//! so a crash mid-write leaves the old file, never half of the new one.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The setting that names another file.
pub const FILE_VAR: &str = "CENTCOM_PREFERENCES_FILE";

const MOTION: &str = "backdrop_motion";
const WEB_PAGES: &str = "web_pages";
const SINAI_MODEL: &str = "sinai_model";
const PARTS: &str = "parts";
const AUTO: &str = "sinai_auto";

/// The version of the `"parts"` object this build writes. A file from a newer build keeps its higher number and every
/// entry this build does not know; an object without a number is read as version 1.
pub const PARTS_VERSION: u64 = 1;

/// The preferences this version reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// Whether the sign-in backdrop moves; None when the person never chose.
    pub motion: Option<bool>,
}

/// Where the preferences live: the file `CENTCOM_PREFERENCES_FILE` names, else `~/.alelyon/alelyon/preferences.json`;
/// None without a home folder.
pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(FILE_VAR).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".alelyon").join("alelyon").join("preferences.json"))
}

/// The file's object, or an empty one when it is missing, unreadable or not a JSON object.
fn object(at: &Path) -> Map<String, Value> {
    match fs::read_to_string(at).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// The preferences kept at `at`: none where there is no file or it cannot be made sense of.
pub fn load(at: &Path) -> Prefs {
    Prefs { motion: object(at).get(MOTION).and_then(Value::as_bool) }
}

/// Keep `prefs` at `at`, with whatever else the file held.
pub fn save(at: &Path, prefs: &Prefs) -> Result<(), String> {
    let mut map = object(at);
    match prefs.motion {
        Some(on) => map.insert(MOTION.into(), Value::Bool(on)),
        None => map.remove(MOTION),
    };
    write(at, map)
}

/// Where the account panel's web pages open, as kept at `at` (`"lattice"` or `"yours"`); None when never chosen.
pub fn load_web_pages(at: &Path) -> Option<String> {
    object(at).get(WEB_PAGES).and_then(Value::as_str).map(str::to_owned)
}

/// Keep where the account panel's web pages open (`key`) at `at`, with whatever else the file held.
pub fn save_web_pages(at: &Path, key: &str) -> Result<(), String> {
    let mut map = object(at);
    map.insert(WEB_PAGES.into(), Value::String(key.into()));
    write(at, map)
}

/// The model Sinai's page talks to when Sinai's mind is not the one answering (`models`, the bring-your-own path):
/// `"gguf:<name>"` or `"endpoint:<id>"`, as kept at `at`; None when never chosen.
pub fn load_sinai_model(at: &Path) -> Option<String> {
    object(at).get(SINAI_MODEL).and_then(Value::as_str).map(str::to_owned)
}

/// Keep the model Sinai's page talks to (`key`, see [`load_sinai_model`]) at `at`, with whatever else the file held.
pub fn save_sinai_model(at: &Path, key: &str) -> Result<(), String> {
    let mut map = object(at);
    map.insert(SINAI_MODEL.into(), Value::String(key.into()));
    write(at, map)
}

/// The choices kept for Sinai's parts at `at`: the `"parts"` object without its `"version"`, each entry by the part's
/// key (`"mind"`, `"eyes"`, `"ears"`, `"embeddings"`, `"voice"`), as written; empty when none is kept or the file
/// cannot be made sense of. What an entry means is `sinai_parts`' to say.
pub fn load_parts(at: &Path) -> Map<String, Value> {
    match object(at).remove(PARTS) {
        Some(Value::Object(mut parts)) => {
            parts.remove("version");
            parts
        }
        _ => Map::new(),
    }
}

/// Keep one part's choice (`entry`; None forgets it) at `at`, with every other part's and whatever else the file held.
/// `sinai_model`, when given, is written in the same write: the mind's own model is also the key older builds read.
pub fn save_part(at: &Path, part: &str, entry: Option<Value>, sinai_model: Option<&str>) -> Result<(), String> {
    let mut map = object(at);
    let mut parts = match map.remove(PARTS) {
        Some(Value::Object(parts)) => parts,
        _ => Map::new(),
    };
    let version = parts.get("version").and_then(Value::as_u64).unwrap_or(1).max(PARTS_VERSION);
    parts.insert("version".into(), Value::from(version));
    match entry {
        Some(entry) => parts.insert(part.into(), entry),
        None => parts.remove(part),
    };
    map.insert(PARTS.into(), Value::Object(parts));
    if let Some(key) = sinai_model {
        map.insert(SINAI_MODEL.into(), Value::String(key.into()));
    }
    write(at, map)
}

/// Auto configure's settings kept at `at` (`"sinai_auto"`, an object whose shape `sinai_parts::auto` documents); empty
/// when none are kept or the file cannot be made sense of.
pub fn load_auto(at: &Path) -> Map<String, Value> {
    match object(at).remove(AUTO) {
        Some(Value::Object(auto)) => auto,
        _ => Map::new(),
    }
}

/// Keep Auto configure's settings at `at`, with whatever else the file held.
pub fn save_auto(at: &Path, auto: Map<String, Value>) -> Result<(), String> {
    let mut map = object(at);
    map.insert(AUTO.into(), Value::Object(auto));
    write(at, map)
}

/// The parts' choices exactly as kept (the whole `"parts"` value and `"sinai_model"`), for putting back later.
pub fn load_parts_whole(at: &Path) -> (Option<Value>, Option<String>) {
    let map = object(at);
    (map.get(PARTS).cloned(), map.get(SINAI_MODEL).and_then(Value::as_str).map(str::to_owned))
}

/// Put back the parts' choices and Auto configure's settings in one write (None removes the entry), with whatever
/// else the file held.
pub fn restore_parts(at: &Path, parts: Option<Value>, sinai_model: Option<String>, auto: Map<String, Value>) -> Result<(), String> {
    let mut map = object(at);
    match parts {
        Some(v) => map.insert(PARTS.into(), v),
        None => map.remove(PARTS),
    };
    match sinai_model {
        Some(v) => map.insert(SINAI_MODEL.into(), Value::String(v)),
        None => map.remove(SINAI_MODEL),
    };
    map.insert(AUTO.into(), Value::Object(auto));
    write(at, map)
}

/// The value kept under `key` at `at`; None when there is none or no file to make sense of.
pub fn load_value(at: &Path, key: &str) -> Option<Value> {
    object(at).remove(key)
}

/// Keep `value` under `key` at `at`, with whatever else the file held.
pub fn save_value(at: &Path, key: &str, value: Value) -> Result<(), String> {
    let mut map = object(at);
    map.insert(key.into(), value);
    write(at, map)
}

/// `map` as the whole new file at `at`, moved over the old one.
fn write(at: &Path, map: Map<String, Value>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
    if let Some(dir) = at.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    }
    let partial = at.with_extension("json.saving");
    fs::write(&partial, text.as_bytes()).map_err(|e| format!("the preferences could not be written: {e}"))?;
    fs::rename(&partial, at).map_err(|e| {
        let _ = fs::remove_file(&partial);
        format!("the preferences could not be kept: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_is_no_preference_and_a_saved_choice_comes_back() {
        let dir = crate::sqlite_ro::tests::scratch("prefs-roundtrip");
        let at = dir.join("nested").join("preferences.json");
        assert_eq!(load(&at), Prefs::default());
        save(&at, &Prefs { motion: Some(false) }).unwrap();
        assert_eq!(load(&at), Prefs { motion: Some(false) });
        save(&at, &Prefs { motion: Some(true) }).unwrap();
        assert_eq!(load(&at), Prefs { motion: Some(true) });
        save(&at, &Prefs { motion: None }).unwrap();
        assert_eq!(load(&at), Prefs::default(), "forgotten");
        let names: Vec<String> =
            fs::read_dir(at.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["preferences.json"], "nothing half-written is left beside it");
    }

    #[test]
    fn a_corrupt_file_is_no_preference_and_is_replaced_whole() {
        let dir = crate::sqlite_ro::tests::scratch("prefs-corrupt");
        let at = dir.join("preferences.json");
        for junk in ["", "{", "not json", "[true]", "{\"backdrop_motion\": \"yes\"}", "\u{feff}garbage"] {
            fs::write(&at, junk).unwrap();
            assert_eq!(load(&at), Prefs::default(), "{junk:?}");
        }
        save(&at, &Prefs { motion: Some(false) }).unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&at).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"backdrop_motion": false}));
    }

    /// The web pages' choice and the backdrop's motion are written each on its own, and each keeps the other.
    #[test]
    fn the_web_pages_choice_and_the_motion_keep_each_other() {
        let dir = crate::sqlite_ro::tests::scratch("prefs-web-pages");
        let at = dir.join("preferences.json");
        assert_eq!(load_web_pages(&at), None);
        save(&at, &Prefs { motion: Some(false) }).unwrap();
        save_web_pages(&at, "yours").unwrap();
        assert_eq!(load_web_pages(&at).as_deref(), Some("yours"));
        assert_eq!(load(&at).motion, Some(false));
        save(&at, &Prefs { motion: Some(true) }).unwrap();
        assert_eq!(load_web_pages(&at).as_deref(), Some("yours"), "the motion's write keeps it");
        save_web_pages(&at, "lattice").unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&at).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"backdrop_motion": true, "web_pages": "lattice"}));
    }

    /// Each part's choice is written on its own, in a versioned object, beside the other preferences; a newer build's
    /// version and entries are kept, and the mind's own model goes to `sinai_model` in the same write.
    #[test]
    fn a_parts_choice_is_kept_beside_the_others_and_a_newer_version_is_kept() {
        let dir = crate::sqlite_ro::tests::scratch("prefs-parts");
        let at = dir.join("preferences.json");
        assert!(load_parts(&at).is_empty());
        save_web_pages(&at, "yours").unwrap();
        save_part(&at, "eyes", Some(serde_json::json!({"kind": "gguf", "name": "vision"})), None).unwrap();
        save_part(&at, "mind", Some(serde_json::json!({"kind": "gguf", "name": "qwen"})), Some("gguf:qwen")).unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&at).unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"web_pages": "yours", "sinai_model": "gguf:qwen", "parts": {"version": 1,
                "eyes": {"kind": "gguf", "name": "vision"}, "mind": {"kind": "gguf", "name": "qwen"}}})
        );
        assert_eq!(load_sinai_model(&at).as_deref(), Some("gguf:qwen"), "older builds read the mind's model here");
        assert_eq!(load_parts(&at).len(), 2, "the version is not a part");
        save_part(&at, "eyes", None, None).unwrap();
        assert!(!load_parts(&at).contains_key("eyes"), "forgotten");
        fs::write(&at, r#"{"parts": {"version": 7, "smell": {"kind": "nose"}}, "sinai_model": "endpoint:x"}"#).unwrap();
        save_part(&at, "voice", Some(serde_json::json!({"kind": "endpoint", "id": "tts"})), None).unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&at).unwrap()).unwrap();
        assert_eq!(value["parts"]["version"], 7, "a newer build's version is kept");
        assert_eq!(value["parts"]["smell"]["kind"], "nose", "an entry this build does not know is kept");
        assert_eq!(value["sinai_model"], "endpoint:x", "untouched when not given");
        fs::write(&at, r#"{"parts": "garbage"}"#).unwrap();
        assert!(load_parts(&at).is_empty(), "a parts value that is not an object is no choice");
    }

    #[test]
    fn keys_it_does_not_know_are_kept() {
        let dir = crate::sqlite_ro::tests::scratch("prefs-unknown");
        let at = dir.join("preferences.json");
        fs::write(&at, r#"{"theme": "dark", "backdrop_motion": true}"#).unwrap();
        save(&at, &Prefs { motion: Some(false) }).unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&at).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"theme": "dark", "backdrop_motion": false}));
    }

    #[test]
    fn a_write_that_cannot_land_says_so_and_leaves_the_old_file() {
        let dir = crate::sqlite_ro::tests::scratch("prefs-refused");
        // the target is a folder: the new file cannot be moved over it
        let at = dir.join("preferences.json");
        fs::create_dir_all(&at).unwrap();
        let err = save(&at, &Prefs { motion: Some(true) }).unwrap_err();
        assert!(err.contains("could not be kept"), "{err}");
        assert!(at.is_dir() && !dir.join("preferences.json.saving").exists());
    }
}
