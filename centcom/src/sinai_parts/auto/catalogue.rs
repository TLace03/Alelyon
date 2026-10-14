//! The quality catalogue that ships with the app: candidate models for each of Sinai's parts, each with a quality
//! rank and the published source that rank rests on (`catalogue.json`, beside this file, versioned).
//!
//! **Shape (version 1).** `scales` names each part's one quality scale and its source (`{"kind": "benchmark", "url":
//! "https://..."}` or `{"kind": "measurement", "file": "<path in this crate>"}`). Each candidate has an `id`, its
//! `part`, a `name`, its size in billions of parameters (`params_b`), its `modalities` (`in`, `out`), its `licence`
//! (`id`, whether its weights are open, a `url`), whether it thinks before answering (`thinking`), the effort levels it
//! suits (`efforts`: `light`, `balanced`, `deep`), its `quality` (`scale`, `rank`, `basis`: what in the source places it
//! there), and where it runs: `local` (a Hugging Face `repo`, `revision` and `files`, each a `path` and its `role`,
//! `model` or `projector`) and or `cloud` (`provider`, `model`). `todo` lists models that are not candidates yet and what
//! each needs.
//!
//! **What the validator holds to.** Every rank has a source: a candidate's scale must be declared, belong to its part,
//! and carry an `https://` benchmark address or a measurement file; ranks compare only within one part's one scale
//! (a second scale for a part is refused, so a family is added only with a source that ranks it against the others);
//! ids are unique; every candidate runs somewhere; every local file is a plain Hugging Face path; providers are ones the
//! app knows; and no candidate is an Alelyon model (no Alelyon model is offered until a flagship exists).

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::sinai_parts::Part;

/// The catalogue as it ships.
pub const SHIPPED: &str = include_str!("catalogue.json");
/// The version this build reads.
pub const VERSION: u64 = 1;

/// The effort levels the dial offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effort {
    Light,
    Balanced,
    Deep,
}

impl Effort {
    pub const ALL: [Effort; 3] = [Effort::Light, Effort::Balanced, Effort::Deep];

    pub fn key(self) -> &'static str {
        match self {
            Effort::Light => "light",
            Effort::Balanced => "balanced",
            Effort::Deep => "deep",
        }
    }

    pub fn of(key: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.key() == key)
    }
}

/// Where a rank comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A published benchmark, by its address.
    Benchmark(String),
    /// A measurement of our own, by its file in the source tree.
    Measurement(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scale {
    pub id: String,
    pub part: Part,
    pub title: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Licence {
    pub id: String,
    pub open_weights: bool,
    pub url: String,
}

/// One file of a local candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalFile {
    pub path: String,
    /// `model`, or `projector` (an eyes model's vision projector, kept beside it).
    pub role: String,
}

impl LocalFile {
    /// The name it is kept under: the path's last segment.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub repo: String,
    pub revision: String,
    pub files: Vec<LocalFile>,
}

impl Local {
    /// The model file (the first of role `model`).
    pub fn model(&self) -> Option<&LocalFile> {
        self.files.iter().find(|f| f.role == "model")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudModel {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub part: Part,
    pub name: String,
    pub params_b: f64,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub licence: Licence,
    pub thinking: bool,
    pub efforts: Vec<Effort>,
    pub scale: String,
    pub rank: u32,
    pub basis: String,
    pub local: Option<Local>,
    pub cloud: Vec<CloudModel>,
}

/// A model that is not a candidate yet, and what it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Todo {
    pub part: String,
    pub name: String,
    pub needs: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Catalogue {
    pub version: u64,
    pub as_of: String,
    pub scales: HashMap<String, Scale>,
    pub candidates: Vec<Candidate>,
    pub todo: Vec<Todo>,
}

fn part_of(key: &str) -> Option<Part> {
    Part::ALL.into_iter().find(|p| p.key() == key)
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
}

/// The providers a cloud candidate may name: those Sinai's parts can run on.
fn provider_known(id: &str) -> bool {
    lattice_core::hosted::parts::cloud(id).is_some()
}

/// Whether text names an Alelyon model (or Sinai's own mind, which is one).
fn names_alelyon(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("alelyon") || t.contains("sinai")
}

fn source_of(v: &Value) -> Result<Source, String> {
    match (text(v, "kind").as_deref(), text(v, "url"), text(v, "file")) {
        (Some("benchmark"), Some(url), _) if url.starts_with("https://") && !url.contains(char::is_whitespace) => Ok(Source::Benchmark(url)),
        (Some("measurement"), _, Some(file))
            if !file.starts_with('/') && !file.contains('\\') && !file.contains(':') && file.split('/').all(|s| !s.is_empty() && s != "..") =>
        {
            Ok(Source::Measurement(file))
        }
        _ => Err("a source is a benchmark with an https:// address, or a measurement file in this crate".into()),
    }
}

/// Read and check a catalogue. Every problem found is named; one is enough to refuse it whole.
pub fn parse(text_in: &str) -> Result<Catalogue, Vec<String>> {
    let value: Value = serde_json::from_str(text_in).map_err(|e| vec![format!("not JSON: {e}")])?;
    let mut wrong = Vec::new();
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(0);
    if value.get("schema").and_then(Value::as_str) != Some("sinai-parts-catalogue") {
        wrong.push("its schema is not sinai-parts-catalogue".to_string());
    }
    if version != VERSION {
        wrong.push(format!("version {version} is not the version this build reads ({VERSION})"));
    }
    let as_of = text(&value, "as_of").unwrap_or_default();
    if as_of.len() != 10 {
        wrong.push("as_of is not a date (YYYY-MM-DD)".into());
    }
    let mut scales = HashMap::new();
    for (id, s) in value.get("scales").and_then(Value::as_object).into_iter().flatten() {
        let part = text(s, "part").as_deref().and_then(part_of);
        let source = s.get("source").map(source_of);
        match (part, source, text(s, "title")) {
            (Some(part), Some(Ok(source)), Some(title)) => {
                scales.insert(id.clone(), Scale { id: id.clone(), part, title, source });
            }
            (_, Some(Err(why)), _) => wrong.push(format!("scale {id}: {why}")),
            _ => wrong.push(format!("scale {id} needs a part, a title and a source")),
        }
    }
    let mut parts_scale: HashMap<Part, String> = HashMap::new();
    for s in scales.values() {
        if let Some(other) = parts_scale.insert(s.part, s.id.clone()) {
            wrong.push(format!("the {} has two scales ({other}, {}): ranks compare on one scale per part", s.part.key(), s.id));
        }
    }
    let mut candidates = Vec::new();
    let mut ids = HashSet::new();
    let mut ranks: HashSet<(String, u32)> = HashSet::new();
    for c in value.get("candidates").and_then(Value::as_array).into_iter().flatten() {
        let id = text(c, "id").unwrap_or_default();
        let at = if id.is_empty() { "a candidate".to_string() } else { format!("candidate {id}") };
        let mut bad = |why: String| wrong.push(format!("{at}: {why}"));
        if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '.')) {
            bad("its id is lower-case letters, digits, '-' and '.'".into());
        }
        if !ids.insert(id.clone()) {
            bad("its id is used twice".into());
        }
        let Some(part) = text(c, "part").as_deref().and_then(part_of) else {
            bad("its part is not one of Sinai's parts".into());
            continue;
        };
        let name = text(c, "name").unwrap_or_default();
        if name.is_empty() {
            bad("it has no name".into());
        }
        let params_b = c.get("params_b").and_then(Value::as_f64).unwrap_or(f64::NAN);
        if !(params_b.is_finite() && params_b > 0.0) {
            bad("params_b is not a positive number".into());
        }
        let modalities = c.get("modalities");
        let inputs = strings(modalities.and_then(|m| m.get("in")));
        let outputs = strings(modalities.and_then(|m| m.get("out")));
        if inputs.is_empty() || outputs.is_empty() {
            bad("its modalities need an in and an out".into());
        }
        let licence = c.get("licence").map(|l| Licence {
            id: text(l, "id").unwrap_or_default(),
            open_weights: l.get("open_weights").and_then(Value::as_bool).unwrap_or(false),
            url: text(l, "url").unwrap_or_default(),
        });
        let licence = match licence {
            Some(l) if !l.id.is_empty() && l.url.starts_with("https://") => l,
            _ => {
                bad("its licence needs an id and an https:// address".into());
                Licence { id: String::new(), open_weights: false, url: String::new() }
            }
        };
        let efforts: Vec<Effort> = strings(c.get("efforts")).iter().filter_map(|e| Effort::of(e)).collect();
        if efforts.is_empty() || efforts.len() != strings(c.get("efforts")).len() {
            bad("its efforts are light, balanced or deep, at least one".into());
        }
        let quality = c.get("quality");
        let scale = quality.and_then(|q| text(q, "scale")).unwrap_or_default();
        let rank = quality.and_then(|q| q.get("rank")).and_then(Value::as_u64).unwrap_or(0) as u32;
        let basis = quality.and_then(|q| text(q, "basis")).unwrap_or_default();
        match scales.get(&scale) {
            None => bad(format!("its rank has no source: scale {scale:?} is not declared")),
            Some(s) if s.part != part => bad(format!("its scale {scale} belongs to the {}", s.part.key())),
            Some(_) => {}
        }
        if rank == 0 {
            bad("its rank is a whole number from 1".into());
        } else if !ranks.insert((scale.clone(), rank)) {
            bad(format!("rank {rank} on {scale} is given twice: ranks order candidates, they do not tie"));
        }
        if basis.is_empty() {
            bad("its rank needs a basis: what in the source places it there".into());
        }
        let local = c.get("local").map(|l| Local {
            repo: text(l, "repo").unwrap_or_default(),
            revision: text(l, "revision").unwrap_or_default(),
            files: l
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|f| LocalFile { path: text(f, "path").unwrap_or_default(), role: text(f, "role").unwrap_or_default() })
                .collect(),
        });
        if let Some(l) = &local {
            use lattice_core::fetch;
            if !fetch::repo_ok(&l.repo) || !fetch::name_ok(&l.revision) {
                bad("its local repo or revision is not a Hugging Face name".into());
            }
            if l.files.is_empty() || l.files.iter().any(|f| !fetch::path_ok(&f.path) || !fetch::name_ok(f.name()) || !matches!(f.role.as_str(), "model" | "projector")) {
                bad("its local files are plain paths with the role model or projector".into());
            }
            if l.model().is_none() {
                bad("its local files have no model".into());
            }
            if part == Part::Eyes && !l.files.iter().any(|f| f.role == "projector") {
                bad("an eyes model on this PC needs its vision projector".into());
            }
        }
        let cloud: Vec<CloudModel> = c
            .get("cloud")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|m| CloudModel { provider: text(m, "provider").unwrap_or_default(), model: text(m, "model").unwrap_or_default() })
            .collect();
        for m in &cloud {
            if !provider_known(&m.provider) || m.model.is_empty() {
                bad(format!("its cloud provider {:?} is not one Sinai's parts run on, or names no model", m.provider));
            }
        }
        if local.is_none() && cloud.is_empty() {
            bad("it runs nowhere: give it local files or a cloud model".into());
        }
        let mut named = vec![id.clone(), name.clone()];
        named.extend(local.iter().flat_map(|l| std::iter::once(l.repo.clone()).chain(l.files.iter().map(|f| f.path.clone()))));
        named.extend(cloud.iter().map(|m| m.model.clone()));
        if named.iter().any(|n| names_alelyon(n)) {
            bad("it is an Alelyon model: none is offered until a flagship exists".into());
        }
        candidates.push(Candidate {
            id,
            part,
            name,
            params_b,
            inputs,
            outputs,
            licence,
            thinking: c.get("thinking").and_then(Value::as_bool).unwrap_or(false),
            efforts,
            scale,
            rank,
            basis,
            local,
            cloud,
        });
    }
    let todo = value
        .get("todo")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|t| Todo { part: text(t, "part").unwrap_or_default(), name: text(t, "name").unwrap_or_default(), needs: text(t, "needs").unwrap_or_default() })
        .collect();
    if wrong.is_empty() { Ok(Catalogue { version, as_of, scales, candidates, todo }) } else { Err(wrong) }
}

/// The catalogue this build ships, checked; its tests hold it valid.
pub fn shipped() -> Result<Catalogue, Vec<String>> {
    parse(SHIPPED)
}

impl Catalogue {
    /// `part`'s candidates.
    pub fn of(&self, part: Part) -> impl Iterator<Item = &Candidate> {
        self.candidates.iter().filter(move |c| c.part == part)
    }
}
