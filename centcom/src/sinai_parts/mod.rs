//! Sinai's parts and the model each one uses: the part picker.
//!
//! Sinai has five parts that run a model: its **mind** (thinks and answers), its **eyes** (understand images and the
//! screen), its **ears** (turn speech into text), its **embeddings** (turn text into vectors for memory and search) and
//! its **voice** (speaks its answers). Its loop is a service, not a model, and has no picker. Each part's card (on the
//! Overview and in the Machine list on Sinai's page) is a button; a press opens that part's picker, with the part's
//! model, where it runs and its state at the top, then the models it can use.
//!
//! **What a part can use, in this version**: a model on this PC (a GGUF file of the models folder; for the ears, a
//! speech model file of the speech engine's models folder), an OpenAI-compatible endpoint the person set up in the
//! Models panel, or a free cloud model on the person's own provider account ([`Kind::Cloud`], [`cloud`]: offered,
//! connected and agreed to part by part before anything is sent); for the mind, also Sinai's own mind where the build
//! carries it. Which kinds each part offers is [`offered`].
//!
//! **What a choice changes**:
//! - the mind: who answers on Sinai's page (`models::answering`). Without Sinai's mind, the chosen model answers.
//!   Where the build carries Sinai's mind, it answers unless the person picks another model here, and that pick lasts
//!   until Alelyon closes; picking Sinai's mind again gives the page back to it;
//! - the ears: the speech model the speech engine starts with when Alelyon starts it (`--model`, see
//!   [`ears_model_args`]); an engine already running keeps its model until it next starts;
//! - the ears in the cloud: the speech engine starts with `--cloud <provider> --cloud-model <model>` and the
//!   provider's key in its environment, and reads with the provider's model instead of starting whisper-server;
//! - the eyes, embeddings and voice: kept for Sinai's own loop. Nothing in this window runs them; each card says so.
//!
//! **Where choices are kept**: Alelyon's preferences (`prefs`), one entry per part in a versioned object:
//!
//! ```json
//! "parts": {
//!   "version": 1,
//!   "mind":       {"kind": "sinai-mind"} | {"kind": "gguf", "name": "<stem>"} | {"kind": "endpoint", "id": "<id>"},
//!   "eyes":       {"kind": "gguf", "name": "<stem>"} | {"kind": "endpoint", "id": "<id>"},
//!   "ears":       {"kind": "speech-file", "file": "<file name>"},
//!   "embeddings": {"kind": "gguf", "name": "<stem>"} | {"kind": "endpoint", "id": "<id>"},
//!   "voice":      {"kind": "endpoint", "id": "<id>"}
//!   any part:     {"kind": "cloud", "provider": "<id>", "model": "<id>", "agreed": "<UTC time>"}
//! }
//! ```
//!
//! A part without an entry uses its default: the eyes are off; the ears use the speech engine's own model rule; the
//! embeddings and voice are Sinai's loop's own; the mind follows the models panel's rule. A cloud entry is used only
//! with its `"agreed"` stamp, which only the Agree step writes, for a provider the part offers and a free model; one
//! without it is kept and not used. An entry of a kind this version does not know is kept as written and the part
//! uses its default. Never a key: keys stay in Windows Credential Manager. The mind's own model is also written to
//! `"sinai_model"`, the key older versions read (a cloud model as `cloud:<provider>/<model>`, which they read as no
//! model); when the two disagree (an older version chose after this one), `"sinai_model"` wins.
//!
//! One action can fill several parts at once ([`Choices::set`] per part, one write each): the store holds a choice per
//! part, never one shared model, so "one model for every part" is a later action over the same entries.
//!
//! **Live**: while a card or the picker is on show, the preferences file and the speech models folder are watched
//! (`live`), and the window is told only when one changes; the models folder and the registry are the Models panel's
//! watch. There is no Check again.

pub mod cloud;
pub mod view;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::{Subscription, Task};
use serde_json::{Value, json};

use crate::live::{Freshness, Seen, Source, Watch};
use crate::models::{Answering, Catalog, Pick, Ready, host_port};

// ------------------------------------------------------------------- the parts

/// One of Sinai's parts that runs a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Mind,
    Eyes,
    Ears,
    Embeddings,
    Voice,
}

impl Part {
    pub const ALL: [Part; 5] = [Part::Mind, Part::Eyes, Part::Ears, Part::Embeddings, Part::Voice];

    /// Its entry's key in the preferences.
    pub fn key(self) -> &'static str {
        match self {
            Part::Mind => "mind",
            Part::Eyes => "eyes",
            Part::Ears => "ears",
            Part::Embeddings => "embeddings",
            Part::Voice => "voice",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Part::Mind => "Mind",
            Part::Eyes => "Eyes",
            Part::Ears => "Ears",
            Part::Embeddings => "Embeddings",
            Part::Voice => "Voice",
        }
    }

    /// What it does, in a line.
    pub fn does(self) -> &'static str {
        match self {
            Part::Mind => "Thinks and answers.",
            Part::Eyes => "Understands images and the screen.",
            Part::Ears => "Turns speech into text.",
            Part::Embeddings => "Turns text into vectors for memory and search.",
            Part::Voice => "Speaks Sinai's answers.",
        }
    }

    /// The part a local service's card stands for, by its port; None for a service that is not a part (Sinai's loop,
    /// the speech engine's own socket).
    pub fn of_port(port: u16) -> Option<Part> {
        match port {
            8179 => Some(Part::Mind),
            8178 | 8187 => Some(Part::Ears),
            8181 => Some(Part::Eyes),
            8182 => Some(Part::Embeddings),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------- what a choice is

/// The kinds of model a part can be given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Sinai's own mind, where the build carries it.
    SinaiMind,
    /// A GGUF file of the models folder.
    Gguf,
    /// An OpenAI-compatible endpoint of the model registry.
    Endpoint,
    /// A speech model file of the speech engine's models folder.
    SpeechFile,
    /// A free model of a hosted provider, on the person's own account, agreed to part by part ([`cloud`]).
    Cloud,
}

/// Whether hosted providers' free models are offered in the pickers (each still needs the person's Connect and Agree).
pub const CLOUD_OFFERED: bool = true;

/// The kinds `part`'s picker offers, in order, in a build that carries Sinai's mind (`mind`) or not.
pub fn offered(part: Part, mind: bool) -> Vec<Kind> {
    let mut kinds = match part {
        Part::Mind if mind => vec![Kind::SinaiMind, Kind::Gguf, Kind::Endpoint],
        Part::Mind | Part::Eyes | Part::Embeddings => vec![Kind::Gguf, Kind::Endpoint],
        Part::Ears => vec![Kind::SpeechFile],
        // a voice model on this PC is not a GGUF file, and its files are not listed yet
        Part::Voice => vec![Kind::Endpoint],
    };
    if CLOUD_OFFERED {
        kinds.push(Kind::Cloud);
    }
    kinds
}

/// A part's model, as chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    SinaiMind,
    Gguf(String),
    Endpoint(String),
    /// A speech model file, by its file name, in the speech engine's models folder.
    SpeechFile(String),
    /// A free model of a hosted provider; used only once agreed (`agreed`: when, in UTC).
    Cloud { provider: String, model: String, agreed: Option<String> },
    /// An entry of a kind this version does not know, kept as written.
    Unknown(Value),
}

impl Choice {
    pub fn kind(&self) -> Option<Kind> {
        Some(match self {
            Choice::SinaiMind => Kind::SinaiMind,
            Choice::Gguf(_) => Kind::Gguf,
            Choice::Endpoint(_) => Kind::Endpoint,
            Choice::SpeechFile(_) => Kind::SpeechFile,
            Choice::Cloud { .. } => Kind::Cloud,
            Choice::Unknown(_) => return None,
        })
    }

    /// As the preferences keep it.
    pub fn entry(&self) -> Value {
        match self {
            Choice::SinaiMind => json!({"kind": "sinai-mind"}),
            Choice::Gguf(name) => json!({"kind": "gguf", "name": name}),
            Choice::Endpoint(id) => json!({"kind": "endpoint", "id": id}),
            Choice::SpeechFile(file) => json!({"kind": "speech-file", "file": file}),
            Choice::Cloud { provider, model, agreed } => {
                let mut v = json!({"kind": "cloud", "provider": provider, "model": model});
                if let Some(at) = agreed {
                    v["agreed"] = json!(at);
                }
                v
            }
            Choice::Unknown(v) => v.clone(),
        }
    }

    /// An entry read back: a known kind with its fields, else [`Choice::Unknown`]; None for what is not an entry at all.
    pub fn from_entry(v: &Value) -> Option<Choice> {
        let field = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let kind = v.get("kind").and_then(Value::as_str)?;
        Some(match kind {
            "sinai-mind" => Choice::SinaiMind,
            "gguf" => Choice::Gguf(field("name")?),
            "endpoint" => Choice::Endpoint(field("id")?),
            "speech-file" => {
                let file = field("file")?;
                // a file name, never a path: the file is looked for in the speech models folder only
                if !plain_file_name(&file) {
                    return None;
                }
                Choice::SpeechFile(file)
            }
            "cloud" => Choice::Cloud { provider: field("provider")?, model: field("model")?, agreed: field("agreed") },
            _ => Choice::Unknown(v.clone()),
        })
    }

    /// The mind's own model, as the models panel names it.
    pub fn pick(&self) -> Option<Pick> {
        match self {
            Choice::Gguf(name) => Some(Pick::Gguf(name.clone())),
            Choice::Endpoint(id) => Some(Pick::Endpoint(id.clone())),
            _ => None,
        }
    }

    pub fn from_pick(pick: &Pick) -> Choice {
        match pick {
            Pick::Gguf(name) => Choice::Gguf(name.clone()),
            Pick::Endpoint(id) => Choice::Endpoint(id.clone()),
        }
    }
}

fn plain_file_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':']) && name != "." && name != ".."
}

/// Every part's choice, as the preferences keep them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    chosen: HashMap<Part, Choice>,
}

impl Choices {
    /// From the preferences' `"parts"` object and `"sinai_model"`: the mind's own model is `"sinai_model"` when the two
    /// disagree or only it is kept.
    pub fn from_prefs(parts: &serde_json::Map<String, Value>, sinai_model: Option<&str>) -> Choices {
        let mut chosen = HashMap::new();
        for part in Part::ALL {
            if let Some(c) = parts.get(part.key()).and_then(Choice::from_entry) {
                chosen.insert(part, c);
            }
        }
        if let Some(pick) = sinai_model.and_then(Pick::parse) {
            match chosen.get(&Part::Mind) {
                // Sinai's mind chosen: the own model kept beside it is the models panel's, not the part's
                Some(Choice::SinaiMind) => {}
                _ => {
                    chosen.insert(Part::Mind, Choice::from_pick(&pick));
                }
            }
        }
        Choices { chosen }
    }

    pub fn read(at: Option<&Path>) -> Choices {
        match at {
            Some(at) => Choices::from_prefs(&crate::prefs::load_parts(at), crate::prefs::load_sinai_model(at).as_deref()),
            None => Choices::default(),
        }
    }

    pub fn get(&self, part: Part) -> Option<&Choice> {
        self.chosen.get(&part)
    }

    /// The choice in effect: an unknown kind is no choice, and a cloud model is one only once agreed, for a provider
    /// the part offers and a free model.
    pub fn used(&self, part: Part) -> Option<&Choice> {
        self.get(part).filter(|c| match c {
            Choice::Cloud { provider, model, agreed } => CLOUD_OFFERED && agreed.is_some() && cloud::allowed(part, provider, model),
            other => other.kind().is_some(),
        })
    }

    /// Change one part's choice here (the write is [`save`]).
    pub fn set(&mut self, part: Part, choice: Option<Choice>) {
        match choice {
            Some(c) => self.chosen.insert(part, c),
            None => self.chosen.remove(&part),
        };
    }
}

/// Keep `part`'s choice (None: its default) in the preferences at `at`; the mind's own model goes to `"sinai_model"` too.
pub fn save(at: &Path, part: Part, choice: Option<&Choice>) -> Result<(), String> {
    let sinai_model = match (part, choice) {
        // older versions read this as no model; this version reads the part's own entry
        (Part::Mind, Some(Choice::Cloud { provider, model, .. })) => Some(format!("cloud:{provider}/{model}")),
        (Part::Mind, Some(c)) => c.pick().map(|p| p.key()),
        _ => None,
    };
    crate::prefs::save_part(at, part.key(), choice.map(Choice::entry), sinai_model.as_deref())
}

// ------------------------------------------------------------------- speech model files

/// A speech model file of the speech engine's models folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// whisper.cpp's model files start with its GGML magic, `0x67676d6c` written little-endian.
const GGML_MAGIC: &[u8; 4] = b"lmgg";

/// The speech engine's models folder (`~/.alelyon/angel/models`, the engine's own rule), or None without a home folder.
pub fn speech_dir() -> Option<PathBuf> {
    let home = ["USERPROFILE", "HOME"].iter().find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))?;
    Some(alelyon_ears::setup::models_dir(&home))
}

/// The speech model files in `dir` (not below it): `.bin` files that start as whisper.cpp's model files do, by name.
pub fn speech_files(dir: &Path) -> Vec<SpeechFile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<SpeechFile> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.to_lowercase().ends_with(".bin") {
                return None;
            }
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let mut magic = [0u8; 4];
            let mut file = std::fs::File::open(e.path()).ok()?;
            std::io::Read::read_exact(&mut file, &mut magic).ok()?;
            (&magic == GGML_MAGIC).then(|| SpeechFile { name, path: e.path(), size: meta.len() })
        })
        .collect();
    found.sort_by_key(|f| f.name.to_lowercase());
    found
}

/// The speech engine's model by its own rule (`ANGEL_EARS_MODEL`, else its approved file in its folder), by file name.
pub fn default_speech_model() -> String {
    alelyon_ears::setup::Given::from_env(None)
        .model_path()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| alelyon_ears::setup::MODEL_FILE.to_string())
}

/// What the speech engine is started with for the ears' choice: `--model <file in dir>` for a speech model file kept,
/// nothing for the default. `CENTCOM_EARS_ARGS`, read after these, still has the last word.
pub fn ears_model_args(choices: &Choices, dir: Option<&Path>) -> Vec<String> {
    match (choices.used(Part::Ears), dir) {
        (Some(Choice::SpeechFile(file)), Some(dir)) => vec!["--model".into(), dir.join(file).display().to_string()],
        (c @ Some(Choice::Cloud { .. }), _) => cloud::ears_args(c),
        _ => Vec::new(),
    }
}

/// [`ears_model_args`] for this process, from the preferences as they are now (a test build keeps none).
pub fn chosen_ears_args() -> Vec<String> {
    let prefs = if cfg!(test) { None } else { crate::prefs::path() };
    ears_model_args(&Choices::read(prefs.as_deref()), speech_dir().as_deref())
}

/// The key the speech engine is started with when the ears' agreed choice is a cloud model (`cloud::ears_env`).
pub fn chosen_ears_env() -> Result<Option<(String, lattice_core::keys::SecretString)>, String> {
    let prefs = if cfg!(test) { None } else { crate::prefs::path() };
    cloud::ears_env(Choices::read(prefs.as_deref()).used(Part::Ears))
}

// ------------------------------------------------------------------- a part's card

/// How a card's state reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Running, or ready to use.
    Up,
    /// Not known yet.
    Waiting,
    /// Off, or not running.
    Off,
    /// Chosen but not usable as it stands.
    Problem,
}

/// What a part's card and its picker's head say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub part: Part,
    /// The model, by name.
    pub model: String,
    /// Where it runs: "on this PC (llama.cpp)", "off this PC (host)", or empty when nothing runs.
    pub runs: String,
    pub level: Level,
    pub state: String,
    /// What uses this part's choice in this build.
    pub used_by: String,
}

/// What the speech engine says now, for the ears' card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EarsNow {
    Looking,
    Starting,
    /// Connected: the recogniser's state ("ready" or what it is doing) and the model it runs.
    Running { recognizer: String, model: String },
    NotRunning,
}

/// What the window knows that a card needs.
pub struct Facts<'a> {
    /// The build carries Sinai's mind.
    pub mind: bool,
    pub answering: &'a Answering,
    pub catalog: Option<&'a Catalog>,
    pub knocked: &'a HashMap<String, bool>,
    /// A local service's port: Some(true) when it answers, None before the first look.
    pub port: &'a dyn Fn(u16) -> Option<bool>,
    pub ears: EarsNow,
}

/// Where a model runs, in words.
pub fn runs_at(pick: &Pick, catalog: Option<&Catalog>) -> String {
    match pick {
        Pick::Gguf(_) => "on this PC (llama.cpp)".into(),
        Pick::Endpoint(id) => match catalog.and_then(|c| c.endpoint(id)) {
            Some(e) if e.local => format!("on this PC ({})", e.knock.as_ref().map(|(h, p)| format!("{h}:{p}")).unwrap_or_else(|| e.address.clone())),
            Some(e) => format!("off this PC ({})", host_port(&e.address).map(|(h, _)| h).unwrap_or_else(|| e.address.clone())),
            None => "an endpoint that is not set up".into(),
        },
    }
}

fn ready_level(ready: &Ready) -> Level {
    match ready {
        Ready::Yes(_) => Level::Up,
        Ready::Asking(_) => Level::Waiting,
        Ready::No(_) => Level::Problem,
    }
}

fn port_state(up: Option<bool>, running: &str, not: &str) -> (Level, String) {
    match up {
        Some(true) => (Level::Up, running.to_string()),
        Some(false) => (Level::Off, not.to_string()),
        None => (Level::Waiting, "Checking".to_string()),
    }
}

/// A model of the person's own, as a card shows it: its name, where it runs, whether it can be used now.
fn own(pick: &Pick, f: &Facts<'_>) -> (String, String, Level, String) {
    let name = f.catalog.map(|c| c.label(pick)).unwrap_or_else(|| pick.key());
    let (level, state) = match f.catalog {
        Some(c) => {
            let ready = c.ready(pick, f.knocked);
            (ready_level(&ready), ready.words().to_string())
        }
        None => (Level::Waiting, "Reading the models folder and the model registry".into()),
    };
    (name, runs_at(pick, f.catalog), level, state)
}

/// `part`'s card: its model, where it runs, its state, and what uses the choice in this build.
pub fn card(part: Part, choices: &Choices, f: &Facts<'_>) -> Card {
    if let Some(card) = cloud_card(part, choices, f) {
        return card;
    }
    let loop_note = if f.mind {
        "Kept for Sinai's own loop, which does not read this choice yet: it runs a model of its own."
    } else {
        "Nothing in this build uses it yet: Sinai's own loop comes with the official app. The choice is kept for it."
    };
    let chosen = |part| choices.used(part).and_then(Choice::pick);
    let (model, runs, level, state, used_by) = match part {
        Part::Mind => {
            let used_by = if f.mind {
                "Sinai's page. Sinai's mind answers there unless you choose another model here; that choice lasts until \
                 Alelyon closes."
            } else {
                "Sinai's page: the model chosen here answers there."
            };
            match f.answering {
                Answering::Mind => {
                    let (level, state) = port_state((f.port)(8179), "Running", "Not running: it starts with Sinai's loop.");
                    ("Sinai's mind".to_string(), "on this PC".to_string(), level, state, used_by)
                }
                Answering::Own(pick) => {
                    let (name, runs, level, state) = own(pick, f);
                    (name, runs, level, state, used_by)
                }
                Answering::Cloud(pick) => {
                    let (name, runs, level, state) = cloud::card_lines(Part::Mind, &pick.provider, &pick.model, cloud::now());
                    (name, runs, level, state, used_by)
                }
                Answering::Nobody => (
                    "None chosen".to_string(),
                    String::new(),
                    Level::Off,
                    "Choose a model on this PC or an endpoint you set up.".to_string(),
                    used_by,
                ),
            }
        }
        Part::Ears => {
            let file = match choices.used(Part::Ears) {
                Some(Choice::SpeechFile(file)) => file.clone(),
                _ => default_speech_model(),
            };
            let (level, state) = match &f.ears {
                EarsNow::Looking => (Level::Waiting, "Looking for the speech engine".to_string()),
                EarsNow::Starting => (Level::Waiting, "Starting the speech engine".to_string()),
                EarsNow::NotRunning => (Level::Off, "Not running. It starts with this model.".to_string()),
                EarsNow::Running { recognizer, model } => {
                    let loading = !recognizer.is_empty() && recognizer != "ready";
                    let now = if loading { format!("Loading {model}") } else { format!("Running {model}") };
                    if !model.is_empty() && *model != file {
                        (Level::Up, format!("{now}. This choice applies when the speech engine next starts from Alelyon."))
                    } else {
                        (if loading { Level::Waiting } else { Level::Up }, now)
                    }
                }
            };
            let used_by = if f.mind {
                "The speech engine (captions, dictation and files), when Alelyon starts it. Sinai's own hearing runs a \
                 model of its own and does not read this choice yet."
            } else {
                "The speech engine (captions, dictation and files), when Alelyon starts it."
            };
            (file, "on this PC (the speech engine)".to_string(), level, state, used_by)
        }
        Part::Eyes | Part::Embeddings | Part::Voice => {
            let (model, runs, level, mut state) = match (chosen(part), part) {
                (Some(pick), _) => own(&pick, f),
                (None, Part::Eyes) => {
                    ("Off".to_string(), String::new(), Level::Off, "Off until you choose a vision model.".to_string())
                }
                (None, _) => ("Sinai's loop's own".to_string(), String::new(), Level::Off, String::new()),
            };
            // what Sinai's own loop is doing, where the build carries it
            if f.mind {
                let loop_now = match part {
                    Part::Eyes => port_state((f.port)(8181), "Its own vision model is open.", "Its own vision model opens when it looks.").1,
                    Part::Embeddings => port_state((f.port)(8182), "Its own embedding model is running.", "Its own embedding model is not running.").1,
                    _ => port_state((f.port)(8180), "It speaks through its loop, which is running.", "Its loop is not running.").1,
                };
                state = if state.is_empty() { loop_now } else { format!("{state} {loop_now}") };
            } else if state.is_empty() {
                state = "Not in this build.".to_string();
            }
            (model, runs, level, state, loop_note)
        }
    };
    Card { part, model, runs, level, state, used_by: used_by.to_string() }
}

/// The card of a part on a cloud model: the mind while its cloud model answers, another part while its agreed cloud
/// choice is in effect. None otherwise.
fn cloud_card(part: Part, choices: &Choices, f: &Facts<'_>) -> Option<Card> {
    let now = cloud::now();
    let (provider, model) = match (part, f.answering) {
        (Part::Mind, Answering::Cloud(pick)) => (pick.provider.clone(), pick.model.clone()),
        (Part::Mind, _) => return None,
        _ => match choices.used(part) {
            Some(Choice::Cloud { provider, model, .. }) => (provider.clone(), model.clone()),
            _ => return None,
        },
    };
    let (model, runs, mut level, mut state) = cloud::card_lines(part, &provider, &model, now);
    let label = cloud::provider_label(&provider).to_string();
    let used_by = match part {
        Part::Mind => "Sinai's page: this cloud model answers there.".to_string(),
        Part::Ears => format!(
            "The speech engine (captions, dictation and files), when Alelyon starts it: it reads with {label} instead of a model on this PC."
        ),
        _ if f.mind => format!("Sinai's own loop, which sends to {label} only when it uses this part."),
        _ => "Nothing in this build uses it yet: Sinai's own loop comes with the official app. The choice is kept for it.".to_string(),
    };
    if part == Part::Ears && level == Level::Up {
        (level, state) = match &f.ears {
            EarsNow::Looking => (Level::Waiting, "Looking for the speech engine".to_string()),
            EarsNow::Starting => (Level::Waiting, "Starting the speech engine".to_string()),
            EarsNow::NotRunning => (Level::Off, format!("Not running. It starts reading with {label}.")),
            EarsNow::Running { model: running, .. } if !running.contains(&label) => (
                Level::Up,
                "The speech engine runs on this PC now; this choice applies when it next starts from Alelyon.".to_string(),
            ),
            EarsNow::Running { .. } => (Level::Up, state),
        };
    }
    Some(Card { part, model, runs, level, state, used_by })
}

// ------------------------------------------------------------------- the picker's state

/// What the picker reads: the choices, and the speech models folder with its files.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub choices: Choices,
    pub speech_dir: Option<PathBuf>,
    pub speech: Vec<SpeechFile>,
}

/// Read what the cards and the picker show. Reads only.
pub fn snapshot(prefs: Option<&Path>, speech_dir: Option<PathBuf>) -> Snapshot {
    let speech = speech_dir.as_deref().map(speech_files).unwrap_or_default();
    Snapshot { choices: Choices::read(prefs), speech_dir, speech }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Open(Part),
    Close,
    Read(Box<Snapshot>),
    Live(Seen),
    /// Use this model for the part (None: its default).
    Choose(Part, Option<Choice>),
    Chosen(Part, Option<Choice>, Result<(), String>),
    /// Open the Models panel, to add a GGUF file or an endpoint.
    AddModels,
    /// A step of using a free cloud model: connect, choose, agree ([`cloud`]).
    Cloud(cloud::Step),
}

/// What the window does after a message, beyond the picker's own state.
pub enum Then {
    Nothing,
    Task(Task<Msg>),
    /// A choice was kept: the window applies it (the mind's who-answers, the models panel's catalog).
    Chosen(Part, Option<Choice>),
    /// The picker closed for the Models panel.
    OpenModels,
}

/// The picker, and the choices the cards show.
pub struct State {
    /// The part whose picker is open.
    pub open: Option<Part>,
    pub snapshot: Option<Snapshot>,
    reading: bool,
    pub fresh: Freshness,
    pub working: bool,
    pub said: Option<Result<String, String>>,
    /// The steps of using a free cloud model, open in the picker.
    pub flow: Option<cloud::Flow>,
    /// The preferences file (None: nothing is kept) and the speech models folder.
    prefs: Option<PathBuf>,
    speech_dir: Option<PathBuf>,
}

impl Default for State {
    fn default() -> State {
        State {
            open: None,
            snapshot: None,
            reading: false,
            fresh: Freshness::waiting(),
            working: false,
            said: None,
            flow: None,
            prefs: if cfg!(test) { None } else { crate::prefs::path() },
            speech_dir: if cfg!(test) { None } else { speech_dir() },
        }
    }
}

impl State {
    /// Use these places (a test's).
    #[cfg(test)]
    pub fn at(prefs: Option<PathBuf>, speech_dir: Option<PathBuf>) -> State {
        State { prefs, speech_dir, ..State::default() }
    }

    pub fn choices(&self) -> Choices {
        self.snapshot.as_ref().map(|s| s.choices.clone()).unwrap_or_default()
    }

    pub fn busy(&self) -> bool {
        self.working || (self.reading && self.snapshot.is_none()) || self.flow.as_ref().is_some_and(|f| f.busy)
    }

    pub fn read(&mut self) -> Task<Msg> {
        if self.reading {
            return Task::none();
        }
        self.reading = true;
        let (prefs, dir) = (self.prefs.clone(), self.speech_dir.clone());
        Task::perform(off_thread(move || snapshot(prefs.as_deref(), dir)), |s| Msg::Read(Box::new(s.unwrap_or_default())))
    }

    /// Why `choice` cannot be used for `part`, if it cannot: a kind the part does not offer, a model not there.
    pub fn cannot_choose(&self, part: Part, choice: &Choice, catalog: Option<&Catalog>, mind: bool) -> Option<String> {
        let kind = choice.kind()?;
        if !offered(part, mind).contains(&kind) {
            return Some(format!("the {} cannot use that kind of model in this version.", part.title().to_lowercase()));
        }
        match choice {
            Choice::Gguf(_) | Choice::Endpoint(_) => catalog.and_then(|c| c.cannot_choose(&choice.pick()?)),
            Choice::SpeechFile(file) => {
                let there = self.snapshot.as_ref().is_some_and(|s| s.speech.iter().any(|f| f.name == *file));
                (!there).then(|| "that speech model file is not in the speech models folder.".to_string())
            }
            Choice::Cloud { provider, model, agreed } => {
                if !cloud::allowed(part, provider, model) {
                    Some(format!("the {} offers no free model of that kind there.", part.title().to_lowercase()))
                } else if agreed.is_none() {
                    Some("nothing is sent before you agree: use a free cloud model and agree on its card.".to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn update(&mut self, msg: Msg, catalog: Option<&Catalog>, mind: bool) -> Then {
        match msg {
            Msg::Open(part) => {
                self.open = Some(part);
                self.said = None;
                self.flow = None;
                if self.snapshot.is_none() {
                    return Then::Task(self.read());
                }
            }
            Msg::Close => {
                self.open = None;
                self.said = None;
                let _ = cloud::update(&mut self.flow, cloud::Step::Close);
            }
            Msg::Cloud(step) => {
                if matches!(step, cloud::Step::Start(..)) {
                    self.said = None;
                }
                match cloud::update(&mut self.flow, step) {
                    cloud::Out::Nothing => {}
                    cloud::Out::Task(task) => return Then::Task(task.map(Msg::Cloud)),
                    cloud::Out::Choose(part, choice) => return self.update(Msg::Choose(part, Some(choice)), catalog, mind),
                }
            }
            Msg::AddModels => {
                self.open = None;
                self.said = None;
                return Then::OpenModels;
            }
            Msg::Read(snapshot) => {
                self.reading = false;
                self.snapshot = Some(*snapshot);
            }
            Msg::Live(seen) => {
                self.fresh.saw(&seen);
                if !seen.first {
                    return Then::Task(self.read());
                }
            }
            Msg::Choose(part, choice) => {
                if self.working {
                    return Then::Nothing;
                }
                if let Some(why) = choice.as_ref().and_then(|c| self.cannot_choose(part, c, catalog, mind)) {
                    self.said = Some(Err(format!("That cannot be chosen: {why}")));
                    return Then::Nothing;
                }
                let Some(at) = self.prefs.clone() else {
                    self.said = Some(Err("This window keeps no preferences, so the choice was not kept.".into()));
                    return Then::Nothing;
                };
                self.working = true;
                self.said = None;
                return Then::Task(Task::perform(
                    off_thread(move || {
                        let done = save(&at, part, choice.as_ref());
                        (part, choice, done)
                    }),
                    |r| match r {
                        Some((part, choice, done)) => Msg::Chosen(part, choice, done),
                        None => Msg::Close,
                    },
                ));
            }
            Msg::Chosen(part, choice, result) => {
                self.working = false;
                match result {
                    Ok(()) => {
                        if let Some(s) = &mut self.snapshot {
                            s.choices.set(part, choice.clone());
                        }
                        self.said = Some(Ok(match (part, &choice) {
                            (_, Some(Choice::Cloud { provider, .. })) => {
                                format!("Agreed and kept: the {} runs on {} now.", part.title().to_lowercase(), cloud::provider_label(provider))
                            }
                            (Part::Ears, Some(_)) => "Kept. The speech engine uses it the next time Alelyon starts it.".to_string(),
                            (Part::Mind, _) => "Kept. Sinai's page uses it now.".to_string(),
                            (_, None) => "Kept: the default.".to_string(),
                            _ => "Kept.".to_string(),
                        }));
                        return Then::Chosen(part, choice);
                    }
                    Err(why) => self.said = Some(Err(why)),
                }
            }
        }
        Then::Nothing
    }

    /// While a card or the picker is on show: the watch over the preferences file and the speech models folder.
    pub fn subscription(&self, on_show: bool) -> Subscription<Msg> {
        if !(on_show || self.open.is_some()) {
            return Subscription::none();
        }
        let mut sources = Vec::new();
        if let Some(prefs) = &self.prefs {
            sources.push(("Alelyon's preferences".to_string(), Source::File(prefs.clone())));
        }
        if let Some(dir) = &self.speech_dir {
            sources.push(("the speech models folder".to_string(), Source::Folder { path: dir.clone(), subtree: false }));
        }
        if sources.is_empty() {
            return Subscription::none();
        }
        Watch::new("parts", sources).subscription().map(Msg::Live)
    }
}

async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = std::thread::Builder::new().name("centcom-parts".into()).spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.ok()
}

#[cfg(test)]
mod tests;
