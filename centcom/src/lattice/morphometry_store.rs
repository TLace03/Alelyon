//! What the Morphometry tab keeps between runs: each model's last weight statistics and last activation probe, as the
//! engine and the probe answered them (their own JSON, kept whole, so a restored answer is the answer, not a summary of
//! it), in `<globals>/lattice_native/morphometry/<model>.json`, one file per model.
//!
//! Reading a model's weights takes a pass over its whole file (15 GB for qwen3-coder-30b) and a probe loads the model,
//! so without this the Reading lost everything past the header whenever the window closed. Each kept answer carries
//! when it was taken and the size and modification time of the model file it read: when that file has changed or is
//! gone, the answer is STALE, is never put into the Reading, and the tab says it was set aside. A file is rewritten in
//! place (no new file per save), off the window's thread, and only by one writer at a time.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

use super::foundry_store::file_stamp;

/// The record's own version.
pub const SCHEMA: i64 = 1;

/// One writer at a time, so a weight answer and a probe answer finishing together cannot lose each other.
static WRITING: Mutex<()> = Mutex::new(());

/// The two answers a model can have kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Weights,
    Activations,
}

impl Slot {
    fn key(self) -> &'static str {
        match self {
            Slot::Weights => "weights",
            Slot::Activations => "activations",
        }
    }
}

/// One kept answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// The engine's or the probe's JSON, as it answered.
    pub text: String,
    /// When it was taken (seconds since 1970).
    pub at: f64,
    /// For a probe: the prompt and the layers on the card.
    pub prompt: Option<String>,
    pub gpu_layers: Option<i64>,
    /// The model file under this name is not the one it read.
    pub stale: bool,
}

/// What a model has kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kept {
    pub weights: Option<Answer>,
    pub activations: Option<Answer>,
}

/// Where a model's record lives: its name with anything but letters, digits, `.`, `_` and `-` written as `_` (the
/// record names its model in full, and a record naming another is not read as this one's).
pub fn path(state: &lattice_core::StateRoot, model: &str) -> PathBuf {
    let safe: String = model.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    state.globals.join("lattice_native").join("morphometry").join(format!("{safe}.json"))
}

/// Keep `answer` (JSON text) as `model`'s `slot`, stamped with `model_path`'s size and modification time now.
pub fn remember(
    file: &Path,
    model: &str,
    model_path: &Path,
    slot: Slot,
    answer: &str,
    prompt: Option<&str>,
    gpu_layers: Option<i64>,
) -> Result<(), String> {
    let answer: Value = serde_json::from_str(answer).map_err(|e| format!("the answer is not JSON ({e}), so it was not kept"))?;
    let _one = WRITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut record = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(|v| v.get("schema").and_then(Value::as_i64) == Some(SCHEMA) && v.get("model").and_then(Value::as_str) == Some(model))
        .unwrap_or_else(|| json!({"schema": SCHEMA, "model": model}));
    let (bytes, modified) = file_stamp(model_path).ok_or_else(|| format!("{} could not be read, so nothing was kept", model_path.display()))?;
    record[slot.key()] = json!({
        "answer": answer,
        "at": crate::utc::now(),
        "model_bytes": bytes,
        "model_modified": modified,
        "prompt": prompt,
        "gpu_layers": gpu_layers,
    });
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    std::fs::write(file, record.to_string()).map_err(|e| format!("{} could not be written: {e}", file.display()))
}

/// What `model` has kept, each answer marked stale when `model_path` is no longer the file it read. A missing record
/// is nothing kept; one that cannot be read, or names another model, is named.
pub fn recall(file: &Path, model: &str, model_path: &Path) -> Result<Kept, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Kept::default()),
        Err(e) => return Err(format!("{} could not be read: {e}", file.display())),
    };
    let record: Value =
        serde_json::from_str(&text).map_err(|e| format!("{} is not JSON ({e}); the next probe replaces it", file.display()))?;
    if record.get("schema").and_then(Value::as_i64) != Some(SCHEMA) {
        return Err(format!("{} is not a version {SCHEMA} record; the next probe replaces it", file.display()));
    }
    if record.get("model").and_then(Value::as_str) != Some(model) {
        return Err(format!("{} belongs to another model; the next probe replaces it", file.display()));
    }
    let now = file_stamp(model_path);
    let answer = |slot: Slot| -> Option<Answer> {
        let entry = record.get(slot.key())?;
        let kept = entry.get("answer").filter(|a| a.is_object())?;
        let stamp = (
            entry.get("model_bytes").and_then(Value::as_u64)?,
            entry.get("model_modified").and_then(Value::as_f64),
        );
        Some(Answer {
            text: kept.to_string(),
            at: entry.get("at").and_then(Value::as_f64).unwrap_or(0.0),
            prompt: entry.get("prompt").and_then(Value::as_str).map(str::to_string),
            gpu_layers: entry.get("gpu_layers").and_then(Value::as_i64),
            stale: now != Some(stamp),
        })
    };
    Ok(Kept { weights: answer(Slot::Weights), activations: answer(Slot::Activations) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_kept_whole_per_model_and_a_changed_model_file_makes_them_stale() {
        let dir = std::env::temp_dir().join(format!("centcom-morpho-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("tiny.gguf");
        std::fs::write(&model, b"GGUF one").unwrap();
        let file = dir.join("kept").join("tiny.json");
        assert_eq!(recall(&file, "tiny", &model), Ok(Kept::default()), "nothing kept yet");

        // A number the engine wrote is kept as written (arbitrary precision: not rounded through an f64).
        let weights = r#"{"path":"tiny.gguf","cells":[{"rms":0.0123456789012345678}],"complete":true}"#;
        remember(&file, "tiny", &model, Slot::Weights, weights, None, None).unwrap();
        remember(&file, "tiny", &model, Slot::Activations, r#"{"ok":true,"layers":2}"#, Some("Hello."), Some(43)).unwrap();
        let kept = recall(&file, "tiny", &model).unwrap();
        let w = kept.weights.expect("weights kept beside the probe");
        assert!(w.text.contains("0.0123456789012345678") && !w.stale, "{}", w.text);
        let a = kept.activations.unwrap();
        assert_eq!((a.prompt.as_deref(), a.gpu_layers, a.stale), (Some("Hello."), Some(43), false));
        assert!(a.at > 1.7e9);

        // Another file under the name: both answers are kept but stale.
        std::fs::write(&model, b"GGUF two, longer").unwrap();
        let kept = recall(&file, "tiny", &model).unwrap();
        assert!(kept.weights.unwrap().stale && kept.activations.unwrap().stale);

        // A record of another model, or not JSON, is named rather than read as this one's.
        assert!(recall(&file, "other", &model).unwrap_err().contains("another model"));
        std::fs::write(&file, b"not json").unwrap();
        assert!(recall(&file, "tiny", &model).unwrap_err().contains("not JSON"));
        // ... and the next answer replaces it.
        remember(&file, "tiny", &model, Slot::Weights, weights, None, None).unwrap();
        assert!(recall(&file, "tiny", &model).unwrap().weights.is_some());
        assert!(remember(&file, "tiny", &model, Slot::Weights, "not json", None, None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_model_name_becomes_a_safe_file_name() {
        let state = lattice_core::StateRoot { root: PathBuf::from("."), globals: PathBuf::from("g"), installed: false };
        assert_eq!(path(&state, "qwen3-coder-30b"), PathBuf::from("g/lattice_native/morphometry/qwen3-coder-30b.json"));
        assert_eq!(path(&state, "a/b:c d").file_name().unwrap(), "a_b_c_d.json");
    }
}
