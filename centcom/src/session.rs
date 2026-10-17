//! Where the person left the window, so it opens there again: the page and its tab, the window's size, Lattice's tab,
//! the folder and files open in its editor with the panes as sized, the chat that was open with its model and mode, and
//! what the command palette ran lately.
//!
//! Kept in the preferences file (`prefs`) under `"session"`, written a second after it changes, never more often.
//! Nothing here is a secret: a chat is named by its id, a file by its path in the folder. A command line that names a
//! page, a folder or a receipt wins over it, and a photograph (`--screenshot`) neither reads nor writes it. Whatever
//! no longer exists (a folder moved, a chat deleted, a page renamed) is passed over quietly: the window opens as it
//! would have without it.

use std::path::Path;

use serde_json::{Map, Value, json};

/// The key in the preferences file.
pub const KEY: &str = "session";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    /// The page, by its title, and the Transcription page's tab, by its title.
    pub section: Option<String>,
    pub words_tab: Option<String>,
    /// The window's size when it was last not maximised, and whether it was maximised.
    pub size: Option<(f32, f32)>,
    pub maximised: bool,
    pub lattice: LatticePlace,
    /// The command palette's latest runs, latest first.
    pub recent: Vec<String>,
}

/// Lattice as it was left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LatticePlace {
    /// Lattice's tab, as `--tab` names it.
    pub tab: Option<String>,
    /// The editor's folder, its files' tabs in order (paths in the folder), and the file on show.
    pub folder: Option<String>,
    pub files: Vec<String>,
    pub active: Option<String>,
    /// The side bar's view (`explorer`, `search`, `chats`, `changes`, `tools`), whether it and the bottom panel were open, and the panes' sizes.
    pub side: Option<String>,
    pub side_open: Option<bool>,
    pub panel_open: Option<bool>,
    pub side_width: Option<f32>,
    pub agent_width: Option<f32>,
    pub panel_height: Option<f32>,
    /// Whether the editor beside the chat was on show, its width, and whether the editor followed the agents.
    pub editor_open: Option<bool>,
    pub editor_width: Option<f32>,
    pub follow: Option<bool>,
    /// The chat that was open (its id), the model chosen and the mode (`ask` or `agent`).
    pub chat: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn number(map: &Map<String, Value>, key: &str) -> Option<f32> {
    map.get(key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n > 0.0).map(|n| n as f32)
}

fn texts(map: &Map<String, Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

impl Session {
    /// The session in `value`; whatever is missing or of the wrong kind is left out.
    pub fn from_json(value: &Value) -> Session {
        let Some(map) = value.as_object() else { return Session::default() };
        let size = match (number(map, "width"), number(map, "height")) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => None,
        };
        let empty = Map::new();
        let l = map.get("lattice").and_then(Value::as_object).unwrap_or(&empty);
        Session {
            section: text(map, "section"),
            words_tab: text(map, "words_tab"),
            size,
            maximised: map.get("maximised").and_then(Value::as_bool).unwrap_or(false),
            lattice: LatticePlace {
                tab: text(l, "tab"),
                folder: text(l, "folder"),
                files: texts(l, "files"),
                active: text(l, "active"),
                side: text(l, "side"),
                side_open: l.get("side_open").and_then(Value::as_bool),
                panel_open: l.get("panel_open").and_then(Value::as_bool),
                side_width: number(l, "side_width"),
                agent_width: number(l, "agent_width"),
                panel_height: number(l, "panel_height"),
                editor_open: l.get("editor_open").and_then(Value::as_bool),
                editor_width: number(l, "editor_width"),
                follow: l.get("follow").and_then(Value::as_bool),
                chat: text(l, "chat"),
                model: text(l, "model"),
                mode: text(l, "mode"),
            },
            recent: texts(map, "recent"),
        }
    }

    pub fn to_json(&self) -> Value {
        let l = &self.lattice;
        let mut map = Map::new();
        let put = |map: &mut Map<String, Value>, key: &str, value: Value| {
            if !value.is_null() {
                map.insert(key.into(), value);
            }
        };
        put(&mut map, "section", json!(self.section));
        put(&mut map, "words_tab", json!(self.words_tab));
        if let Some((w, h)) = self.size {
            put(&mut map, "width", json!(w.round()));
            put(&mut map, "height", json!(h.round()));
        }
        put(&mut map, "maximised", json!(self.maximised));
        let mut lattice = Map::new();
        put(&mut lattice, "tab", json!(l.tab));
        put(&mut lattice, "folder", json!(l.folder));
        if !l.files.is_empty() {
            put(&mut lattice, "files", json!(l.files));
        }
        put(&mut lattice, "active", json!(l.active));
        put(&mut lattice, "side", json!(l.side));
        put(&mut lattice, "side_open", json!(l.side_open));
        put(&mut lattice, "panel_open", json!(l.panel_open));
        put(&mut lattice, "side_width", json!(l.side_width.map(f32::round)));
        put(&mut lattice, "agent_width", json!(l.agent_width.map(f32::round)));
        put(&mut lattice, "panel_height", json!(l.panel_height.map(f32::round)));
        put(&mut lattice, "editor_open", json!(l.editor_open));
        put(&mut lattice, "editor_width", json!(l.editor_width.map(f32::round)));
        put(&mut lattice, "follow", json!(l.follow));
        put(&mut lattice, "chat", json!(l.chat));
        put(&mut lattice, "model", json!(l.model));
        put(&mut lattice, "mode", json!(l.mode));
        map.insert("lattice".into(), Value::Object(lattice));
        if !self.recent.is_empty() {
            map.insert("recent".into(), json!(self.recent));
        }
        Value::Object(map)
    }
}

/// The session kept at `at`: none when there is no file or no session in it.
pub fn load(at: &Path) -> Session {
    crate::prefs::load_value(at, KEY).map(|v| Session::from_json(&v)).unwrap_or_default()
}

/// Keep `session` at `at`, with whatever else the file held.
pub fn save(at: &Path, session: &Session) -> Result<(), String> {
    crate::prefs::save_value(at, KEY, session.to_json())
}

/// Where the session is kept, for a window that may keep one: never in a test, never for a photograph.
pub fn path(photograph: bool) -> Option<std::path::PathBuf> {
    if cfg!(test) || photograph { None } else { crate::prefs::path() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Session {
        Session {
            section: Some("Lattice".into()),
            words_tab: Some("Library".into()),
            size: Some((1600.0, 1000.0)),
            maximised: true,
            lattice: LatticePlace {
                tab: Some("chat".into()),
                folder: Some(r"C:\src\proj".into()),
                files: vec!["src/main.rs".into(), "README.md".into()],
                active: Some("README.md".into()),
                side: Some("search".into()),
                side_open: Some(false),
                panel_open: Some(true),
                side_width: Some(300.0),
                agent_width: Some(420.0),
                panel_height: Some(250.0),
                editor_open: Some(true),
                editor_width: Some(700.0),
                follow: Some(false),
                chat: Some("c-1".into()),
                model: Some("hosted:claude".into()),
                mode: Some("agent".into()),
            },
            recent: vec!["go:lattice".into(), "editor:Save".into()],
        }
    }

    #[test]
    fn a_session_comes_back_as_it_was_kept_beside_the_other_preferences() {
        let dir = crate::sqlite_ro::tests::scratch("session-roundtrip");
        let at = dir.join("preferences.json");
        assert_eq!(load(&at), Session::default(), "no file is no session");
        crate::prefs::save_web_pages(&at, "yours").unwrap();
        save(&at, &full()).unwrap();
        assert_eq!(load(&at), full());
        assert_eq!(crate::prefs::load_web_pages(&at).as_deref(), Some("yours"), "the other preferences are kept");
        save(&at, &Session::default()).unwrap();
        assert_eq!(load(&at), Session::default());
    }

    #[test]
    fn what_is_missing_or_of_the_wrong_kind_is_left_out() {
        let odd = json!({
            "section": 7, "width": -5, "height": 900, "maximised": "yes",
            "lattice": {"files": ["a.rs", 3, ""], "side_width": "wide", "chat": ""},
            "recent": "go:overview"
        });
        let s = Session::from_json(&odd);
        assert_eq!(s.section, None);
        assert_eq!(s.size, None, "a size needs both, positive");
        assert!(!s.maximised);
        assert_eq!(s.lattice.files, ["a.rs"]);
        assert_eq!(s.lattice.side_width, None);
        assert_eq!(s.lattice.chat, None);
        assert!(s.recent.is_empty());
        assert_eq!(Session::from_json(&json!([1, 2])), Session::default());
    }

    #[test]
    fn tests_and_photographs_never_touch_the_persons_file() {
        assert_eq!(path(false), None);
        assert_eq!(path(true), None);
    }
}
