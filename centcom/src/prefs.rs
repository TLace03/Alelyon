//! Alelyon's own preferences on this PC: a small JSON object in `~/.alelyon/alelyon/preferences.json`
//! (`CENTCOM_PREFERENCES_FILE` names another file). Today it holds three choices: whether the sign-in backdrop moves
//! (`"backdrop_motion": true | false`), where the account panel's web pages open (`"web_pages": "lattice" |
//! "yours"`, `signin::web`), and the model of the person's own that Sinai's page talks to (`"sinai_model":
//! "gguf:<name>" | "endpoint:<id>"`, `models`; never a key, which is Credential Manager's); each is absent when the
//! person never chose, and each is written on its own, keeping the others.
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
