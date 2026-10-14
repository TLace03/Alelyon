//! The one-model switch: one local model as the mind, the eyes and the voice of Sinai at once, where the build offers
//! one ([`alelyon_plugin_api::OneModel`], registered with `Registry::one_model`; the public build offers none and
//! shows no switch).
//!
//! **What it changes.** On, it writes a `one-model` entry for the mind, the eyes and the voice in one write
//! (`{"kind": "one-model", "model": "<stem>"}`), and keeps the entries it replaced (and `"sinai_model"`) under
//! `"sinai_one_model"`:
//!
//! ```json
//! "sinai_one_model": {"version": 1, "model": "<stem>", "at": "<UTC>",
//!                     "previous": {"parts": {...}, "sinai_model": "..."}}
//! ```
//!
//! Off puts the kept entries back in one write and removes `"sinai_one_model"`. When nothing else changed in between,
//! the `"parts"` object and `"sinai_model"` come back exactly as they were; when a part was changed by hand while the
//! switch was on, that part keeps its new choice and only the parts still on the one model go back. Hearing and the
//! embeddings are never touched.
//!
//! **When it can be turned on.** Only when the build's server program is found and every file the build names beside
//! it and beside the model is in place ([`look`]); otherwise the switch says in plain words what is missing. The
//! program is the build's configured one, never searched for, under the same rules as the managed llama-server (an
//! absolute path to an `.exe` on a drive of this PC). The model is a GGUF file of the models folder, found by its stem
//! as every other model is.
//!
//! **What it never does.** It starts nothing. Sinai's own loop reads the entries when it starts, and starts the one
//! model's server itself when it first needs it, as it does its other servers. Old builds read a `one-model` entry as a
//! kind they do not know: they keep it as written and the part uses its default.

use std::path::{Path, PathBuf};

use alelyon_plugin_api::OneModel;
use serde_json::{Map, Value, json};

use super::{Card, Choice, Facts, Level, Part, port_state};

/// The parts the one model is: the mind, the eyes and the voice.
pub const PARTS: [Part; 3] = [Part::Mind, Part::Eyes, Part::Voice];

/// The preferences key the switch keeps what it replaced under.
pub const KEY: &str = "sinai_one_model";

/// What the switch found for the build's one model: its words, the server program and the model when they are
/// usable, and what is missing, in words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub title: String,
    pub needs: String,
    /// The model's GGUF stem: what the parts' entries name.
    pub stem: String,
    pub server: Option<PathBuf>,
    pub model: Option<PathBuf>,
    pub missing: Vec<String>,
}

impl Found {
    /// Everything is in place: the switch can be turned on.
    pub fn ready(&self) -> bool {
        !self.stem.is_empty() && self.server.is_some() && self.model.is_some() && self.missing.is_empty()
    }

    /// Why it cannot be turned on, in one sentence; None when it can.
    pub fn refusal(&self) -> Option<String> {
        (!self.ready()).then(|| {
            let what = if self.missing.is_empty() { vec!["it was not checked yet".to_string()] } else { self.missing.clone() };
            format!("The one model cannot be turned on yet: {}.", what.join("; "))
        })
    }
}

/// Look for the build's one model (`spec`, as the build found it) in `models_dir`: its server program and the files
/// beside it, the model and the files beside it. Reads only.
pub fn look(spec: &Result<OneModel, String>, models_dir: Option<&Path>) -> Found {
    use lattice_core::llama::files::{BinaryProblem, LlamaPaths, find_binary, resolve_model};
    let spec = match spec {
        Ok(spec) => spec,
        Err(why) => return Found { missing: vec![why.clone()], ..Found::default() },
    };
    let mut found = Found { title: spec.title.clone(), needs: spec.needs.clone(), stem: spec.model.clone(), ..Found::default() };
    let program = spec.server.as_ref().ok().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match &spec.server {
        Err(why) => found.missing.push(format!("the server program: {why}")),
        Ok(path) => {
            let paths = LlamaPaths {
                home: PathBuf::new(),
                llama_dir: PathBuf::new(),
                models_dir: PathBuf::new(),
                binary: path.clone(),
                binary_from_env: true,
            };
            let setting = &spec.server_setting;
            match find_binary(&paths) {
                Ok(server) => {
                    let folder = server.parent().map(Path::to_path_buf).unwrap_or_default();
                    let absent: Vec<&str> =
                        spec.beside_server.iter().map(String::as_str).filter(|rel| !folder.join(rel).is_file()).collect();
                    if absent.is_empty() {
                        found.server = Some(server);
                    } else {
                        found.missing.push(format!("{} beside the server program in {}", absent.join(", "), folder.display()));
                    }
                }
                Err(problem) => found.missing.push(match problem {
                    BinaryProblem::Missing => {
                        format!("the server program {program}, which is not at {} (put it there, or set {setting} to where it is)", path.display())
                    }
                    BinaryProblem::NotAbsolute => format!("the server program: {setting} must be a full path, and {} is not", path.display()),
                    BinaryProblem::NotExe => format!("the server program: {} is not an .exe file", path.display()),
                    BinaryProblem::NotLocal => format!("the server program: {} is not on a drive of this PC", path.display()),
                    BinaryProblem::Ollama => format!("the server program: {} is another program's copy", path.display()),
                }),
            }
        }
    }
    match models_dir {
        None => found.missing.push(format!("the model {}: the models folder is not known", spec.model)),
        Some(dir) => match resolve_model(&spec.model, dir) {
            Err(_) => found.missing.push(format!("the model {}.gguf in the models folder ({})", spec.model, dir.display())),
            Ok(m) => {
                let folder = m.path.parent().map(Path::to_path_buf).unwrap_or_default();
                let absent: Vec<&str> =
                    spec.beside_model.iter().map(String::as_str).filter(|rel| !folder.join(rel).is_file()).collect();
                if absent.is_empty() {
                    found.model = Some(m.path);
                } else {
                    found.missing.push(format!("{} beside the model in {}", absent.join(", "), folder.display()));
                }
            }
        },
    }
    found
}

fn is_on_entry(parts: Option<&Map<String, Value>>, part: Part) -> bool {
    parts.and_then(|p| p.get(part.key())).and_then(|e| e.get("kind")).and_then(Value::as_str) == Some(super::ONE_MODEL_KIND)
}

/// Whether the switch is on in `choices`: the mind, the eyes and the voice all on the one model.
pub fn is_on(choices: &super::Choices) -> bool {
    PARTS.iter().all(|p| matches!(choices.get(*p), Some(Choice::OneModel(_))))
}

/// Turn the switch on at `at`: refused unless `found` is ready; else the three entries written and the ones they
/// replace kept, in one write. Turning it on again while it is on keeps what was kept the first time.
pub fn turn_on(at: &Path, found: &Found, stamp: &str) -> Result<Vec<(Part, Option<Choice>)>, String> {
    if let Some(why) = found.refusal() {
        return Err(why);
    }
    let (parts, sinai_model) = crate::prefs::load_parts_whole(at);
    let current = parts.as_ref().and_then(Value::as_object).cloned();
    let kept = crate::prefs::load_value(at, KEY);
    let already = PARTS.iter().all(|p| is_on_entry(current.as_ref(), *p));
    let previous = match kept.as_ref().and_then(|k| k.get("previous")).filter(|_| already) {
        Some(p) => p.clone(),
        None => {
            let mut p = Map::new();
            if let Some(v) = &parts {
                p.insert("parts".into(), v.clone());
            }
            if let Some(s) = &sinai_model {
                p.insert("sinai_model".into(), json!(s));
            }
            Value::Object(p)
        }
    };
    let mut next = current.unwrap_or_default();
    let version = next.get("version").and_then(Value::as_u64).unwrap_or(1).max(crate::prefs::PARTS_VERSION);
    next.insert("version".into(), json!(version));
    let choice = Choice::OneModel(found.stem.clone());
    for part in PARTS {
        next.insert(part.key().into(), choice.entry());
    }
    let record = json!({"version": 1, "model": found.stem, "at": stamp, "previous": previous});
    crate::prefs::save_keys(at, vec![("parts", Some(Value::Object(next))), (KEY, Some(record))])?;
    Ok(PARTS.iter().map(|p| (*p, Some(choice.clone()))).collect())
}

/// The parts object without the switch's parts and its version: what must be unchanged for an exact restore.
fn others(parts: Option<&Map<String, Value>>) -> Map<String, Value> {
    let mut m = parts.cloned().unwrap_or_default();
    m.remove("version");
    for part in PARTS {
        m.remove(part.key());
    }
    m
}

/// Turn the switch off at `at`: the kept entries put back in one write (exactly, when nothing else changed), and the
/// kept record removed. The parts it changed, as now kept.
pub fn turn_off(at: &Path) -> Result<Vec<(Part, Option<Choice>)>, String> {
    let kept = crate::prefs::load_value(at, KEY);
    let (parts, sinai_model) = crate::prefs::load_parts_whole(at);
    let current = parts.as_ref().and_then(Value::as_object).cloned();
    let any_on = PARTS.iter().any(|p| is_on_entry(current.as_ref(), *p));
    if kept.is_none() && !any_on {
        return Err("The one model is not on: there is nothing to put back.".into());
    }
    let previous = kept.as_ref().and_then(|k| k.get("previous")).and_then(Value::as_object).cloned().unwrap_or_default();
    let prev_parts = previous.get("parts").filter(|v| v.is_object()).cloned();
    let prev_sinai = previous.get("sinai_model").and_then(Value::as_str).map(str::to_owned);
    let prev_map = prev_parts.as_ref().and_then(Value::as_object);
    let all_on = PARTS.iter().all(|p| is_on_entry(current.as_ref(), *p));
    let (restored, restored_sinai) = if kept.is_some() && all_on && others(current.as_ref()) == others(prev_map) {
        // nothing else changed while it was on: everything back exactly as it was
        (prev_parts.clone(), prev_sinai)
    } else {
        // a part changed by hand keeps its new choice; only the parts still on the one model go back
        let mut m = current.clone().unwrap_or_default();
        for part in PARTS {
            if is_on_entry(current.as_ref(), part) {
                match prev_map.and_then(|p| p.get(part.key())) {
                    Some(entry) => m.insert(part.key().into(), entry.clone()),
                    None => m.remove(part.key()),
                };
            }
        }
        let sinai = if is_on_entry(current.as_ref(), Part::Mind) { prev_sinai } else { sinai_model };
        (Some(Value::Object(m)), sinai)
    };
    crate::prefs::save_keys(at, vec![("parts", restored.clone()), ("sinai_model", restored_sinai.map(Value::String)), (KEY, None)])?;
    let restored_map = restored.as_ref().and_then(Value::as_object);
    Ok(PARTS.iter().map(|p| (*p, restored_map.and_then(|m| m.get(p.key())).and_then(Choice::from_entry))).collect())
}

/// The card of a part on the one model.
pub fn card(part: Part, model: &str, f: &Facts<'_>) -> Card {
    let (level, state) = if f.mind {
        port_state(
            (f.port)(8180),
            "Sinai's own loop is running: it uses the one model from its next start, and starts its server when first needed.",
            "Its loop is not running: the one model starts with it.",
        )
    } else {
        (Level::Problem, "Not in this build: only the loop that comes with the official app runs the one model.".to_string())
    };
    Card {
        part,
        model: format!("{model} (one model)"),
        runs: "on this PC (one server for the mind, eyes and voice)".into(),
        level,
        state,
        used_by: "Sinai's own loop: one server is its mind, eyes and voice. Hearing and embeddings keep their own models. \
                  Turn the one model off on Sinai's page to choose this part on its own again."
            .into(),
    }
}
