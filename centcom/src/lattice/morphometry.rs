//! The Lattice page's Morphometry tab: an installed GGUF model's declared anatomy, measured by the C++ engine
//! (the model-anatomy engine) and shown as the Python desktop shows it (the Model Morphometry,
//! Lineage Morphometry, Template Hierarchy and Registration views).
//!
//! A model is read from its GGUF header only (`lattice-core`'s `gguf::read_header`, the reader the Models tab and
//! the managed server use), turned into Python's `show` payload (`GGUFHeader.show_payload`, ported in the crate), and
//! measured: `analyze`, `voxel_field`, `register` against the canonical space, and `measure_layers` over the template
//! hierarchy. No weight is read and no forward pass is run, so every figure is declared structure and storage
//! precision, never what the model has learned. All of it runs off the window's thread.
//!
//! The volume drawn is the engine's canonical voxel field, unchanged: the space commitment covers that frame, so this
//! view neither resamples it nor re-arranges it per layer (a lineage layer that draws draws the same field).
//!
//! Beside the volume, the Reading (lattice/digest.rs) says what all of it means in a few plain sentences, each tagged
//! with how it is known and opening onto its figures and what it cannot say; a finding about particular cells lights
//! them on the volume. The figures themselves stay one tab away (Structure, Probes, Compare, Frames).
//!
//! The installed models are listed when the tab opens and the first (the managed server's chosen model, when it is
//! installed) is measured then; Measure measures the picked one again. The Python view measured on a button press
//! because asking its runtime was an HTTP call; here it is one header read of a local file.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::canvas::{self, Canvas, Frame, Path, Stroke, Text};
use iced::widget::{Column, Row, button, column, container, pick_list, row, scrollable, space};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Task, Theme, mouse, padding};

use crate::theme::{self, fonts};
use lattice_core::chat::pyjson::{PyValue, py_str};
use lattice_core::llama::{files, gguf};
use model_anatomy::gguf::{GgufTensor, GgufValue, show_payload};
use model_anatomy::{
    Analysis, CanonicalSpace, CompatibilityReport, HierarchyMorphometry, HierarchySnapshot, Morphometry, TransformFacts,
    Voxel, VoxelField,
};

use super::digest::{self, Basis, Focus, Further, Reading, Tone};
use crate::ui::{self, chip, fact, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

// ------------------------------------------------------------------ state

/// The installed models, where they were looked for, and the managed server's chosen one.
#[derive(Clone, Debug, Default)]
pub struct Installed {
    pub dir: String,
    pub models: Vec<files::LocalModel>,
    pub selected: String,
}

/// What the tab shows before (and beside) any model: the frame's lineage, the transform families' facts, the
/// canonical space, the module vocabulary and the lineage with no model on it.
#[derive(Clone, Debug)]
pub struct Reference {
    pub hierarchy: Result<HierarchySnapshot, String>,
    pub facts: Result<TransformFacts, String>,
    pub canonical: Result<CanonicalSpace, String>,
    pub unmeasured: Result<HierarchyMorphometry, String>,
    /// `MM.FAMILIES`, in the order a voxel's `y` indexes.
    pub families: Vec<String>,
    /// `MM.MODULES[module][2]`: a module's human label.
    pub module_labels: HashMap<String, String>,
}

/// One model, measured.
#[derive(Clone, Debug)]
pub struct Measured {
    pub model: String,
    pub path: String,
    pub analysis: Analysis,
    /// `MM.register(morph)`; only for a measured model, as the Python view registers only then.
    pub registration: Option<CompatibilityReport>,
    pub layers: HierarchyMorphometry,
}

/// What a cube's colour says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colour {
    Parameters,
    Precision,
    Active,
}

impl std::fmt::Display for Colour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Colour::Parameters => "Colour: parameter share",
            Colour::Precision => "Colour: storage precision",
            Colour::Active => "Colour: active share (per token)",
        })
    }
}

/// A family filter choice: `None` is all of them, else the index a voxel's `y` holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyChoice(pub Option<i64>, pub String);

impl std::fmt::Display for FamilyChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// A lineage layer choice: its index in `measure_layers`' layers, and its words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerChoice(pub usize, pub String);

impl std::fmt::Display for LayerChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// The right-hand column's views: the Reading first, the figures behind it after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Reading,
    Structure,
    Probes,
    Compare,
    Frames,
}

impl Pane {
    pub const ALL: [Pane; 5] = [Pane::Reading, Pane::Structure, Pane::Probes, Pane::Compare, Pane::Frames];

    fn title(self) -> String {
        match self {
            Pane::Reading => "Reading",
            Pane::Structure => "Structure",
            Pane::Probes => "Probes",
            Pane::Compare => "Compare",
            Pane::Frames => "Frames",
        }
        .to_string()
    }
}

pub struct State {
    pub installed: Option<Installed>,
    pub listing: bool,
    pub reference: Option<Reference>,
    pub chosen: Option<String>,
    /// The model being measured, while it is.
    pub measuring: Option<String>,
    pub measured: Option<Result<Arc<Measured>, String>>,
    pub colour: Colour,
    pub family: Option<i64>,
    pub layer: Option<usize>,
    pub fact: Option<usize>,
    measured_once: bool,
    /// Draw the measured cells as the stylised brain instead of the canonical frame (a display mapping only).
    pub brain: bool,
    /// The brain laid out once per measurement, never per frame.
    pub brain_layout: Option<Arc<super::brain::Brain>>,
    /// Show the brain inside Sinai's head too, which turns glass to hold it (since 2026-10-07).
    pub in_sinai: bool,
    /// The model to compare the chosen one with, and the comparison (`morphometry_compare.compare`, the C++ port):
    /// the chosen model is the left operand, deltas are right minus left.
    pub compare_with: Option<String>,
    pub comparing: bool,
    pub comparison: Option<Result<Arc<model_anatomy::MorphometryComparison>, String>>,
    /// The probes (lattice/probes.rs): weight statistics of the chosen model's file, and one forward pass of a prompt.
    pub weights_running: Option<Arc<super::probes::Running>>,
    pub weights: Option<Result<Arc<model_anatomy::WeightStatistics>, String>>,
    pub probe_prompt: String,
    pub probe_layers: String,
    pub probe_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub activations: Option<Result<Arc<super::probes::Activations>, String>>,
    /// The last layer suggestion's words (DERIVED), or why there is none.
    pub layers_hint: Option<String>,
    /// The model each probe's reading belongs to (a reading of another model is never mixed into this one's).
    pub weights_for: Option<String>,
    pub activations_for: Option<String>,
    /// When each probe's answer was taken (seconds since 1970), and the probe's prompt.
    pub weights_at: Option<f64>,
    pub activations_at: Option<f64>,
    pub activations_prompt: Option<String>,
    /// What keeping or restoring the answers said, when it was not simply done (morphometry_store).
    pub kept_note: Option<String>,
    /// The right-hand column's view, the Reading of the measured model, the finding opened, and the one lit on the
    /// volume.
    pub pane: Pane,
    pub reading: Option<Arc<Reading>>,
    pub opened: Option<&'static str>,
    pub lit: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Arc<(Installed, Reference)>),
    Pick(String),
    Measure,
    Measured(String, Arc<Result<Measured, String>>),
    Colour(Colour),
    Family(FamilyChoice),
    Layer(LayerChoice),
    Fact(usize),
    Brain(bool),
    InSinai(bool),
    CompareWith(String),
    Compare,
    Compared(Arc<Result<model_anatomy::MorphometryComparison, String>>),
    ProbeWeights,
    StopWeights,
    /// The weight statistics, and what keeping them said when it failed.
    WeightsRead(Arc<Result<model_anatomy::WeightStatistics, String>>, Option<String>),
    ProbePrompt(String),
    ProbeLayers(String),
    ProbeActivations,
    StopActivations,
    /// The probe's answer, and what keeping it said when it failed.
    ActivationsRead(Arc<Result<super::probes::Activations, String>>, Option<String>),
    /// What the model measured last kept between runs.
    Kept(String, Arc<Result<super::morphometry_store::Kept, String>>),
    SuggestLayers,
    Suggested(Result<(u64, String), String>),
    Pane(Pane),
    Open(&'static str),
    Light(Option<&'static str>),
    Further(Further),
}

impl Default for State {
    fn default() -> State {
        State {
            installed: None,
            listing: false,
            reference: None,
            chosen: None,
            measuring: None,
            measured: None,
            colour: Colour::Parameters,
            family: None,
            layer: None,
            fact: None,
            measured_once: false,
            brain: false,
            brain_layout: None,
            in_sinai: true,
            compare_with: None,
            comparing: false,
            comparison: None,
            weights_running: None,
            weights: None,
            probe_prompt: "The capital of France is Paris. Write a Python function that reverses a list.".to_string(),
            probe_layers: "0".to_string(),
            probe_stop: None,
            activations: None,
            layers_hint: None,
            weights_for: None,
            activations_for: None,
            weights_at: None,
            activations_at: None,
            activations_prompt: None,
            kept_note: None,
            pane: Pane::Reading,
            reading: None,
            opened: None,
            lit: None,
        }
    }
}

impl State {
    pub fn busy(&self) -> bool {
        self.listing || self.measuring.is_some()
    }

    /// The tab opened: list the installed models (and, the first time, read the lineage and the facts).
    pub fn open(&mut self, state_root: Option<lattice_core::StateRoot>) -> Task<Msg> {
        if self.listing {
            return Task::none();
        }
        self.listing = true;
        Task::perform(super::off_thread(move || read(state_root.as_ref())), |r| match r {
            Some(read) => Msg::Read(Arc::new(read)),
            None => Msg::Read(Arc::new((
                Installed::default(),
                Reference::failed("the reader stopped before answering".to_string()),
            ))),
        })
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let rebuild =
            matches!(msg, Msg::Read(_) | Msg::Measured(..) | Msg::WeightsRead(..) | Msg::ActivationsRead(..) | Msg::Compared(_) | Msg::Kept(..));
        let task = self.apply(msg);
        if rebuild {
            self.reread();
        }
        task
    }

    /// Rebuild the Reading from what is known about the measured model now.
    fn reread(&mut self) {
        let Some(m) = self.current() else {
            self.reading = None;
            return;
        };
        let empty = HashMap::new();
        let labels = self.reference.as_ref().map(|r| &r.module_labels).unwrap_or(&empty);
        let mine = |of: &Option<String>| of.as_deref() == Some(m.model.as_str());
        let weights = match &self.weights {
            Some(Ok(w)) if mine(&self.weights_for) => Some(&**w),
            _ => None,
        };
        let activations = match &self.activations {
            Some(Ok(a)) if mine(&self.activations_for) => Some(&**a),
            _ => None,
        };
        let comparison = match &self.comparison {
            Some(Ok(c)) if c.left_model == m.model => Some(&**c),
            _ => None,
        };
        let now = crate::utc::now();
        let when = |at: Option<f64>| at.map(|at| crate::utc::when(at, now));
        let reading = digest::read(&digest::Inputs {
            measured: m,
            labels,
            weights,
            activations,
            comparison,
            weights_when: weights.and(when(self.weights_at)),
            activations_when: activations.and(when(self.activations_at)),
            prompt: activations.and(self.activations_prompt.as_deref()),
        });
        if self.reading.as_ref().is_some_and(|r| r.model != reading.model) {
            self.opened = None;
            self.lit = None;
        }
        self.reading = Some(Arc::new(reading));
    }

    /// The finding lit on the volume, while the reading still has it.
    pub fn lit_finding(&self) -> Option<&digest::Finding> {
        let id = self.lit?;
        self.reading.as_ref()?.findings.iter().find(|f| f.id == id && f.focus.is_some())
    }

    fn apply(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Read(read) => {
                self.listing = false;
                let (installed, reference) = (*read).clone();
                if self.chosen.as_ref().is_none_or(|c| !installed.models.iter().any(|m| &m.name == c)) {
                    self.chosen = installed
                        .models
                        .iter()
                        .find(|m| m.name == installed.selected)
                        .or(installed.models.first())
                        .map(|m| m.name.clone());
                }
                self.installed = Some(installed);
                self.reference = Some(reference);
                if !self.measured_once && self.chosen.is_some() {
                    self.measured_once = true;
                    return self.measure();
                }
            }
            Msg::Pick(name) => self.chosen = Some(name),
            Msg::Measure => return self.measure(),
            Msg::Measured(name, result) => {
                if self.measuring.as_deref() == Some(name.as_str()) {
                    self.measuring = None;
                }
                let result = match &*result {
                    Ok(measured) => Ok(Arc::new(measured.clone())),
                    Err(why) => Err(why.clone()),
                };
                self.brain_layout = None;
                if let Ok(m) = &result {
                    self.brain_layout = Some(Arc::new(super::brain::build(
                        &m.analysis.morphometry.cells,
                        super::brain::PREVIEW_RESOLUTION,
                    )));
                    // The deepest layer that draws, as the Python view defaults to it.
                    self.layer = m.layers.layers.iter().rposition(|l| l.drawn);
                    if self.colour == Colour::Active && !m.analysis.voxel_field.has_active_path {
                        self.colour = Colour::Parameters;
                    }
                }
                let restore = match (&result, self.chosen_model()) {
                    (Ok(m), Some(model)) if model.name == m.model => Some(model),
                    _ => None,
                };
                self.measured = Some(result);
                if let Some(model) = restore {
                    let name = model.name.clone();
                    return Task::perform(
                        super::off_thread(move || {
                            let file = super::morphometry_store::path(&lattice_core::state::resolve(), &model.name);
                            super::morphometry_store::recall(&file, &model.name, &model.path)
                        }),
                        move |r| Msg::Kept(name.clone(), Arc::new(r.unwrap_or_else(|| Err("the kept answers were not read".to_string())))),
                    );
                }
            }
            Msg::Colour(colour) => self.colour = colour,
            Msg::Brain(on) => self.brain = on,
            Msg::InSinai(on) => self.in_sinai = on,
            Msg::CompareWith(name) => self.compare_with = Some(name),
            Msg::Compare => {
                self.pane = Pane::Compare;
                return self.compare();
            }
            Msg::ProbeWeights => {
                let Some(model) = self.chosen_model() else { return Task::none() };
                if self.weights_running.is_some() {
                    return Task::none();
                }
                let running = Arc::new(super::probes::Running::default());
                self.weights_running = Some(running.clone());
                self.weights_for = Some(model.name.clone());
                self.weights_at = None;
                return Task::perform(
                    super::off_thread(move || {
                        let (read, text) = super::probes::weights(&model.path, running)?;
                        // Only a whole answer is kept: one stopped part way would stand in for the file.
                        let kept = if read.complete && read.refusal.is_none() {
                            let file = super::morphometry_store::path(&lattice_core::state::resolve(), &model.name);
                            let slot = super::morphometry_store::Slot::Weights;
                            super::morphometry_store::remember(&file, &model.name, &model.path, slot, &text, None, None).err()
                        } else {
                            None
                        };
                        Ok((read, kept))
                    }),
                    |r| {
                        let (result, kept) = match r {
                            Some(Ok((read, kept))) => (Ok(read), kept),
                            Some(Err(why)) => (Err(why), None),
                            None => (Err("the weight statistics stopped before answering".to_string()), None),
                        };
                        Msg::WeightsRead(Arc::new(result), kept)
                    },
                );
            }
            Msg::StopWeights => {
                if let Some(running) = &self.weights_running {
                    running.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Msg::WeightsRead(result, kept) => {
                self.weights_running = None;
                if result.is_ok() {
                    self.weights_at = Some(crate::utc::now());
                }
                self.weights = Some(match &*result {
                    Ok(w) => Ok(Arc::new(w.clone())),
                    Err(why) => Err(why.clone()),
                });
                if let Some(why) = kept {
                    self.kept_note = Some(format!("The weight statistics were not kept for next time: {why}"));
                }
            }
            Msg::Kept(name, result) => {
                if self.current().is_none_or(|m| m.model != name) {
                    return Task::none();
                }
                let mut set_aside = Vec::new();
                match &*result {
                    Err(why) => self.kept_note = Some(why.clone()),
                    Ok(kept) => {
                        self.kept_note = None;
                        // An answer this run already holds for the model, or is taking, wins over a kept one.
                        if let Some(answer) = &kept.weights {
                            let holding = self.weights_running.is_some()
                                || (self.weights_for.as_deref() == Some(name.as_str()) && self.weights.is_some());
                            if answer.stale {
                                set_aside.push("weight statistics");
                            } else if !holding {
                                match super::probes::parse_weights(&answer.text) {
                                    Ok(w) => {
                                        self.weights = Some(Ok(Arc::new(w)));
                                        self.weights_for = Some(name.clone());
                                        self.weights_at = Some(answer.at);
                                    }
                                    Err(why) => self.kept_note = Some(why),
                                }
                            }
                        }
                        if let Some(answer) = &kept.activations {
                            let holding = self.probe_stop.is_some()
                                || (self.activations_for.as_deref() == Some(name.as_str()) && self.activations.is_some());
                            if answer.stale {
                                set_aside.push("activation probe");
                            } else if !holding {
                                match super::probes::Activations::parse(&answer.text) {
                                    Some(a) => {
                                        self.activations = Some(Ok(Arc::new(a)));
                                        self.activations_for = Some(name.clone());
                                        self.activations_at = Some(answer.at);
                                        self.activations_prompt = answer.prompt.clone();
                                        if let Some(prompt) = &answer.prompt {
                                            self.probe_prompt = prompt.clone();
                                        }
                                        if let Some(layers) = answer.gpu_layers {
                                            self.probe_layers = layers.to_string();
                                        }
                                    }
                                    None => self.kept_note = Some("the kept probe answer could not be read".to_string()),
                                }
                            }
                        }
                    }
                }
                if !set_aside.is_empty() {
                    self.kept_note = Some(format!(
                        "The kept {} {} set aside: the model file under this name has changed since, so {} describe another file. Run {} again.",
                        set_aside.join(" and "),
                        if set_aside.len() == 1 { "was" } else { "were" },
                        if set_aside.len() == 1 { "it would" } else { "they would" },
                        if set_aside.len() == 1 { "it" } else { "them" }
                    ));
                }
            }
            Msg::SuggestLayers => {
                let Some(model) = self.chosen_model() else { return Task::none() };
                // The layer count from the measured model (its highest block + 1), when the chosen model is measured.
                let layers = self
                    .current()
                    .filter(|m| m.model == model.name)
                    .and_then(|m| m.analysis.morphometry.cells.iter().filter_map(|c| c.block).max())
                    .map(|b| (b + 1) as u64);
                let Some(layers) = layers else {
                    self.layers_hint = Some("Measure the chosen model first: its layer count comes from its header.".to_string());
                    return Task::none();
                };
                let bytes = model.size;
                return Task::perform(
                    super::off_thread(move || {
                        let free = super::probes::free_card_memory()?;
                        let n = super::probes::suggested_layers(free, bytes, layers);
                        let gib = |b: u64| b as f64 / (1u64 << 30) as f64;
                        Ok((
                            n,
                            format!(
                                "About {n} of {layers} layers [DERIVED]: {:.2} GiB free on the card, less {:.1} GiB for \
                                 llama.cpp's working buffers, at {:.2} GiB a layer (the file's {:.2} GiB over its layers).",
                                gib(free),
                                gib(super::probes::CARD_MARGIN),
                                gib(bytes) / layers as f64,
                                gib(bytes)
                            ),
                        ))
                    }),
                    |r| Msg::Suggested(r.unwrap_or_else(|| Err("the suggestion stopped before answering".to_string()))),
                );
            }
            Msg::Suggested(result) => match result {
                Ok((n, words)) => {
                    self.probe_layers = n.to_string();
                    self.layers_hint = Some(words);
                }
                Err(why) => self.layers_hint = Some(why),
            },
            Msg::ProbePrompt(text) => self.probe_prompt = text,
            Msg::ProbeLayers(text) => self.probe_layers = text,
            Msg::ProbeActivations => {
                let Some(model) = self.chosen_model() else { return Task::none() };
                let Ok(layers) = self.probe_layers.trim().parse::<u32>() else { return Task::none() };
                if self.probe_stop.is_some() || self.probe_prompt.trim().is_empty() {
                    return Task::none();
                }
                let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.probe_stop = Some(stop.clone());
                self.activations_for = Some(model.name.clone());
                let prompt = self.probe_prompt.clone();
                self.activations_at = None;
                self.activations_prompt = Some(prompt.clone());
                return Task::perform(
                    super::off_thread(move || {
                        let (read, text) = super::probes::activations(&model.path, &prompt, layers, stop)?;
                        let file = super::morphometry_store::path(&lattice_core::state::resolve(), &model.name);
                        let slot = super::morphometry_store::Slot::Activations;
                        let kept = super::morphometry_store::remember(
                            &file,
                            &model.name,
                            &model.path,
                            slot,
                            &text,
                            Some(&prompt),
                            Some(i64::from(layers)),
                        )
                        .err();
                        Ok((read, kept))
                    }),
                    |r| {
                        let (result, kept) = match r {
                            Some(Ok((read, kept))) => (Ok(read), kept),
                            Some(Err(why)) => (Err(why), None),
                            None => (Err("the probe stopped before answering".to_string()), None),
                        };
                        Msg::ActivationsRead(Arc::new(result), kept)
                    },
                );
            }
            Msg::StopActivations => {
                if let Some(stop) = &self.probe_stop {
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Msg::ActivationsRead(result, kept) => {
                self.probe_stop = None;
                if result.is_ok() {
                    self.activations_at = Some(crate::utc::now());
                }
                self.activations = Some(match &*result {
                    Ok(a) => Ok(Arc::new(a.clone())),
                    Err(why) => Err(why.clone()),
                });
                if let Some(why) = kept {
                    self.kept_note = Some(format!("The probe's answer was not kept for next time: {why}"));
                }
            }
            Msg::Compared(result) => {
                self.comparing = false;
                self.comparison = Some(match &*result {
                    Ok(comparison) => Ok(Arc::new(comparison.clone())),
                    Err(why) => Err(why.clone()),
                });
            }
            Msg::Family(choice) => self.family = choice.0,
            Msg::Layer(choice) => self.layer = Some(choice.0),
            Msg::Fact(index) => self.fact = if self.fact == Some(index) { None } else { Some(index) },
            Msg::Pane(pane) => self.pane = pane,
            Msg::Open(id) => self.opened = if self.opened == Some(id) { None } else { Some(id) },
            Msg::Light(id) => {
                self.lit = id;
                // The stylised brain does not draw cells one by one, so the frame shows what is lit.
                if id.is_some() {
                    self.brain = false;
                }
            }
            Msg::Further(Further::Weights) => return self.apply(Msg::ProbeWeights),
            Msg::Further(Further::Activations) => self.pane = Pane::Probes,
            Msg::Further(Further::Compare) => self.pane = Pane::Compare,
        }
        Task::none()
    }

    fn measure(&mut self) -> Task<Msg> {
        let Some(name) = self.chosen.clone() else { return Task::none() };
        if self.measuring.is_some() {
            return Task::none();
        }
        let Some(model) = self.installed.as_ref().and_then(|i| i.models.iter().find(|m| m.name == name)).cloned() else {
            return Task::none();
        };
        self.measuring = Some(name.clone());
        Task::perform(super::off_thread(move || measure_file(&model.path, &model.name)), move |r| {
            Msg::Measured(
                name.clone(),
                Arc::new(r.unwrap_or_else(|| Err("the measurement stopped before answering".to_string()))),
            )
        })
    }

    /// Compare the chosen model (left) with `compare_with` (right), off the window's thread.
    fn compare(&mut self) -> Task<Msg> {
        if self.comparing {
            return Task::none();
        }
        let find = |name: &Option<String>| {
            let name = name.as_ref()?;
            self.installed.as_ref()?.models.iter().find(|m| &m.name == name).cloned()
        };
        let (Some(left), Some(right)) = (find(&self.chosen), find(&self.compare_with)) else { return Task::none() };
        self.comparing = true;
        Task::perform(
            super::off_thread(move || compare_files(&left.path, &left.name, &right.path, &right.name)),
            |r| Msg::Compared(Arc::new(r.unwrap_or_else(|| Err("the comparison stopped before answering".to_string())))),
        )
    }

    /// The installed model chosen in the picker.
    fn chosen_model(&self) -> Option<lattice_core::llama::files::LocalModel> {
        let name = self.chosen.as_ref()?;
        self.installed.as_ref()?.models.iter().find(|m| &m.name == name).cloned()
    }

    /// The measured model, when the last measurement succeeded.
    fn current(&self) -> Option<&Measured> {
        match &self.measured {
            Some(Ok(m)) => Some(m),
            _ => None,
        }
    }
}

impl Reference {
    fn failed(why: String) -> Reference {
        Reference {
            hierarchy: Err(why.clone()),
            facts: Err(why.clone()),
            canonical: Err(why.clone()),
            unmeasured: Err(why),
            families: Vec::new(),
            module_labels: HashMap::new(),
        }
    }
}

// ------------------------------------------------------------------ the work, off the window's thread

/// The installed models (`ALELYON_MODELS_DIR`, else `~/.alelyon/models`, as lattice-core finds them) and the reference
/// readings.
pub fn read(state_root: Option<&lattice_core::StateRoot>) -> (Installed, Reference) {
    let paths = files::LlamaPaths::from_env(&lattice_core::ProcessEnv);
    let installed = Installed {
        dir: paths.models_dir.display().to_string(),
        models: files::list_models(&paths.models_dir),
        selected: state_root.map(files::selected_model).unwrap_or_default(),
    };
    let text = |e: model_anatomy::Error| e.to_string();
    let (families, module_labels) = vocabulary().unwrap_or_default();
    let reference = Reference {
        hierarchy: model_anatomy::template_hierarchy(None).map_err(text),
        facts: model_anatomy::transform_facts().map_err(text),
        canonical: model_anatomy::canonical_space().map_err(text),
        unmeasured: model_anatomy::measure_layers(None, None).map_err(text),
        families,
        module_labels,
    };
    (installed, reference)
}

/// `MM.FAMILIES` and each module's label, from the engine's constants.
fn vocabulary() -> Option<(Vec<String>, HashMap<String, String>)> {
    let constants: serde_json::Value = serde_json::from_str(&model_anatomy::constants_json().ok()?).ok()?;
    let families = constants.get("families")?.as_array()?.iter().filter_map(|f| f.as_str().map(str::to_owned)).collect();
    let mut labels = HashMap::new();
    for module in constants.get("modules")?.as_array()? {
        if let (Some(id), Some(label)) = (module.get(0).and_then(|v| v.as_str()), module.get(3).and_then(|v| v.as_str())) {
            labels.insert(id.to_owned(), label.to_owned());
        }
    }
    Some((families, labels))
}

/// Two GGUF files compared on the canonical frame: each header read and turned into Python's `show` payload, then
/// `morphometry_compare.compare` (the C++ port). Deltas are right minus left.
pub fn compare_files(
    left: &std::path::Path,
    left_name: &str,
    right: &std::path::Path,
    right_name: &str,
) -> Result<model_anatomy::MorphometryComparison, String> {
    let payload = |path: &std::path::Path, name: &str| {
        let header = gguf::read_header(path)
            .map_err(|e| format!("{} could not be read as a GGUF header ({}).", path.display(), e.kind()))?;
        payload_of(&header, name).map_err(|e| {
            format!("{name}'s general.file_type cannot be read as an integer ({}, as Python's show_payload raises).", e.kind())
        })
    };
    let (l, r) = (payload(left, left_name)?, payload(right, right_name)?);
    model_anatomy::compare(Some(&l), left_name, Some(&r), right_name).map_err(|e| e.to_string())
}

/// A GGUF file, measured: its header read, then [`measure_header`].
pub fn measure_file(path: &std::path::Path, name: &str) -> Result<Measured, String> {
    let header = gguf::read_header(path)
        .map_err(|e| format!("{} could not be read as a GGUF header ({}).", path.display(), e.kind()))?;
    measure_header(&header, name, path.to_path_buf())
}

/// A header's metadata value, as the crate's `show_payload` takes it. A list (64 items or fewer; the reader leaves
/// longer ones out) keeps Python's `str()` of it; `model_info` drops it, as Python's does.
fn gguf_value(value: &PyValue) -> GgufValue {
    match value {
        PyValue::Bool(flag) => GgufValue::Bool(*flag),
        PyValue::Int(digits) => GgufValue::Int(digits.clone()),
        PyValue::Float(number) => GgufValue::Float(*number),
        PyValue::Str(text) => GgufValue::Str(text.clone()),
        other => GgufValue::Other(py_str(other)),
    }
}

/// Python's `show` payload of one header (`GGUFHeader.show_payload`, ported in the crate); the Foundry tab reads it
/// too. An error is Python's exception: `general.file_type` that `int()` refuses.
pub fn payload_of(header: &gguf::Header, name: &str) -> Result<model_anatomy::Payload, model_anatomy::gguf::ShowPayloadError> {
    let metadata: Vec<(String, GgufValue)> = header.metadata.iter().map(|(k, v)| (k.clone(), gguf_value(v))).collect();
    let tensors: Vec<GgufTensor> = header
        .tensors
        .iter()
        .map(|t| GgufTensor { name: t.name.clone(), shape: t.shape.clone(), ggml_type: t.ggml_type })
        .collect();
    show_payload(name, &metadata, &tensors)
}

/// `show_payload`, `analyze`, `register` and `measure_layers` over one header.
pub fn measure_header(header: &gguf::Header, name: &str, path: PathBuf) -> Result<Measured, String> {
    let payload = payload_of(header, name).map_err(|e| {
        format!("The header's general.file_type cannot be read as an integer ({}, as Python's show_payload raises).", e.kind())
    })?;
    let failed = |e: model_anatomy::Error| e.to_string();
    let analysis = model_anatomy::analyze(Some(&payload), name).map_err(failed)?;
    let registration = if analysis.morphometry.derived.ok {
        Some(model_anatomy::register_model(Some(&payload), name).map_err(failed)?)
    } else {
        None
    };
    let layers = model_anatomy::measure_layers(Some((Some(&payload), name)), None).map_err(failed)?;
    Ok(Measured { model: name.to_owned(), path: path.display().to_string(), analysis, registration, layers })
}

/// A float to four significant figures, or a dash.
fn small(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |v| format!("{v:.4}"))
}

/// The probes: what they read, their controls, and what they found.
fn probes_card(state: &State) -> El<'_> {
    use iced::widget::text_input;
    let mut col = Column::new().spacing(6);
    col = col.push(strong("Probes: past the header", 14.0, theme::GOLD));
    col = col.push(note(
        "Weight statistics read the chosen model's whole file and dequantise every tensor (bit for bit as gguf-py), \
         summarising each (block, module) cell; the file is only read. The activation probe runs one forward pass of \
         your prompt on the pinned llama.cpp build in a process of its own, with the layers you choose on the card, and \
         records each layer's activations and the experts its router chose. Both run only when pressed; each model's \
         last answers are kept (globals/lattice_native/morphometry) and come back when it is measured again, unless \
         its file has changed.",
    ));
    let chosen = state.chosen.is_some();
    let weights_button = match &state.weights_running {
        Some(running) => row![
            label(format!("Reading the weights: {:.0}%", running.share() * 100.0), 12.5, theme::TEXT),
            ui::secondary("Stop", Some(Msg::StopWeights)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        None => row![ui::secondary("Probe weights", chosen.then_some(Msg::ProbeWeights))],
    };
    col = col.push(weights_button);
    let layers_ok = state.probe_layers.trim().parse::<u32>().is_ok();
    col = col.push(
        text_input("A prompt to run through the model", &state.probe_prompt)
            .on_input(Msg::ProbePrompt)
            .padding(7)
            .size(12.5)
            .style(ui::input_style),
    );
    let activation_button = if state.probe_stop.is_some() {
        row![label("Running the prompt through the model…", 12.5, theme::TEXT), ui::secondary("Stop", Some(Msg::StopActivations))]
    } else {
        row![
            label("Layers on the card", 12.0, theme::TEXT_DIM),
            container(
                text_input("0", &state.probe_layers).on_input(Msg::ProbeLayers).padding(6).size(12.5).style(ui::input_style)
            )
            .width(Length::Fixed(70.0)),
            ui::secondary("Suggest", chosen.then_some(Msg::SuggestLayers)),
            ui::secondary("Probe activations", (chosen && layers_ok && !state.probe_prompt.trim().is_empty()).then_some(Msg::ProbeActivations)),
        ]
    };
    col = col.push(activation_button.spacing(8).align_y(Alignment::Center));
    col = col.push(note(
        "0 layers on the card runs the probe on the CPU. Suggest estimates how many fit from the card's free memory now; \
         the more layers on the card, the faster the pass.",
    ));
    if let Some(hint) = &state.layers_hint {
        col = col.push(label(hint.as_str(), 11.5, theme::TEXT_DIM));
    }
    match &state.weights {
        Some(Err(why)) => col = col.push(ui::notice(why.as_str(), theme::CAUTION)),
        Some(Ok(w)) => col = col.push(weights_report(w)),
        None => {}
    }
    match &state.activations {
        Some(Err(why)) => col = col.push(ui::notice(why.as_str(), theme::CAUTION)),
        Some(Ok(a)) => col = col.push(activations_report(a)),
        None => {}
    }
    container(col).padding(10).style(theme::panel).into()
}

fn weights_report(w: &model_anatomy::WeightStatistics) -> El<'_> {
    let mut col = Column::new().spacing(3);
    if let Some(r) = &w.refusal {
        return ui::notice(format!("Weight statistics refused ({}): {}", r.code, r.reason), theme::CAUTION);
    }
    col = col.push(strong(
        format!("Weights: {} tensors read, {} refused, {} bytes{}", w.tensors.len(), w.refused.len(), grouped(w.file_bytes as i64),
            if w.complete { "" } else { " (incomplete)" }),
        12.5,
        theme::TEXT,
    ));
    for r in &w.refused {
        col = col.push(label(format!("Refused {} ({}): {}", r.name, r.type_name, r.reason), 11.5, theme::CAUTION));
    }
    col = col.push(row![
        container(label("Cell", 11.0, theme::TEXT_FAINT)).width(150),
        container(label("RMS", 11.0, theme::TEXT_FAINT)).width(70),
        container(label("Mean", 11.0, theme::TEXT_FAINT)).width(80),
        container(label("Largest |x|", 11.0, theme::TEXT_FAINT)).width(80),
        label("Zeros", 11.0, theme::TEXT_FAINT),
    ].spacing(4));
    for cell in &w.cells {
        let place = match cell.block {
            Some(b) => format!("blk {b} · {}", cell.module),
            None => cell.module.clone(),
        };
        let s = &cell.stats;
        let largest = s.min.zip(s.max).map(|(lo, hi)| lo.abs().max(hi.abs()));
        let zeros = if s.count > 0 { format!("{:.2}%", s.zeros as f64 / s.count as f64 * 100.0) } else { "-".to_string() };
        col = col.push(row![
            container(mono(place, 11.0, theme::TEXT)).width(150),
            container(label(small(s.rms), 11.0, theme::TEXT)).width(70),
            container(label(small(s.mean), 11.0, theme::TEXT_DIM)).width(80),
            container(label(small(largest), 11.0, theme::TEXT_DIM)).width(80),
            label(zeros, 11.0, theme::TEXT_DIM),
        ].spacing(4));
    }
    col.into()
}

fn activations_report(a: &super::probes::Activations) -> El<'_> {
    let mut col = Column::new().spacing(3);
    col = col.push(strong(
        format!(
            "Activations: {} ({} layers{}), {} prompt tokens, {} layers on the card, load {:.1} s, pass {:.2} s · {}",
            a.architecture,
            a.layers,
            a.expert_count.map_or(String::new(), |e| format!(", {e} experts")),
            a.tokens,
            a.gpu_layers,
            a.load_ms.unwrap_or(0.0) / 1e3,
            a.decode_ms.unwrap_or(0.0) / 1e3,
            a.build
        ),
        12.5,
        theme::TEXT,
    ));
    col = col.push(note(
        "One pass of one prompt: what this model computed for it, not a property of the model. The last layer is computed \
         only for the token it outputs. Expert choices can differ between the CPU and the card: near ties in a router \
         fall either way with the backends' arithmetic.",
    ));
    col = col.push(row![
        container(label("Layer", 11.0, theme::TEXT_FAINT)).width(50),
        container(label("Output RMS", 11.0, theme::TEXT_FAINT)).width(80),
        container(label("Attn norm RMS", 11.0, theme::TEXT_FAINT)).width(90),
        container(label("Experts chosen", 11.0, theme::TEXT_FAINT)).width(100),
        label("Most chosen", 11.0, theme::TEXT_FAINT),
    ].spacing(4));
    for layer in 0..a.layers {
        let rms = |op: &str| small(a.at(op, layer).and_then(|t| t.rms));
        let routing = a.experts.iter().find(|r| r.layer == layer);
        let (chosen, top) = match routing {
            Some(r) => (format!("{} distinct, {}/token", r.counts.len(), r.used_per_token), super::probes::top_experts(r, 4)),
            None => ("-".to_string(), String::new()),
        };
        col = col.push(row![
            container(label(layer.to_string(), 11.0, theme::TEXT)).width(50),
            container(label(rms("l_out"), 11.0, theme::TEXT)).width(80),
            container(label(rms("attn_norm"), 11.0, theme::TEXT_DIM)).width(90),
            container(label(chosen, 11.0, theme::TEXT_DIM)).width(100),
            label(top, 11.0, theme::TEXT_DIM),
        ].spacing(4));
    }
    let nonfinite: Vec<String> =
        a.tensors.iter().filter(|t| t.nonfinite > 0).map(|t| format!("{} ({})", t.name, t.nonfinite)).collect();
    if !nonfinite.is_empty() {
        col = col.push(label(format!("Non-finite values: {}", nonfinite.join(", ")), 11.0, theme::CAUTION));
    }
    let skipped: Vec<&str> = a.tensors.iter().filter(|t| t.skipped.is_some()).map(|t| t.name.as_str()).collect();
    if !skipped.is_empty() {
        col = col.push(label(format!("Not read: {}", skipped.join(", ")), 11.0, theme::CAUTION));
    }
    col.into()
}

/// A count, or UNMEASURED.
fn count(value: Option<i64>) -> String {
    value.map_or_else(|| "UNMEASURED".to_string(), grouped)
}

/// A signed delta, or a dash where it has none (a side-only cell, an unknown storage).
fn delta(value: Option<i64>) -> String {
    match value {
        Some(d) if d > 0 => format!("+{}", grouped(d)),
        Some(d) => grouped(d),
        None => "-".to_string(),
    }
}

/// The comparison of two models (`morphometry_compare`): what it is and is not, its totals, and each cell.
fn comparison_report(c: &model_anatomy::MorphometryComparison) -> El<'_> {
    use model_anatomy::CellPresence;
    let mut col = Column::new().spacing(6);
    col = col.push(strong(format!("Compared: {} (left) with {} (right)", c.left_model, c.right_model), 14.0, theme::GOLD));
    let code = if c.ok() { theme::POSITIVE } else { theme::CAUTION };
    col = col.push(row![chip(c.code.as_str(), code), label(c.explanation.as_str(), 12.5, theme::TEXT)].spacing(8).align_y(Alignment::Center));
    col = col.push(note(
        "Descriptive only: declared structural differences on the canonical (block, module) frame, right minus left. It \
         says nothing about quality, capability or learned behaviour. A cell one side alone reports gets no delta; an \
         unknown storage or active count stays UNMEASURED.",
    ));
    col = col.push(fact("Sources", label(format!("{} · {}", c.left_source, c.right_source), 12.5, theme::TEXT)));
    if !c.ok() {
        for side in &c.unmeasured_sides {
            col = col.push(label(format!("UNMEASURED: {side}"), 12.0, theme::CAUTION));
        }
        return col.into();
    }
    let both = c.cells.iter().filter(|x| x.presence == CellPresence::Both).count();
    let left = c.cells.iter().filter(|x| x.presence == CellPresence::LeftOnly).count();
    let right = c.cells.iter().filter(|x| x.presence == CellPresence::RightOnly).count();
    col = col.push(fact("Cells", label(format!("{both} in both · {left} only left · {right} only right"), 12.5, theme::TEXT)));
    col = col.push(fact(
        "Complete",
        label(if c.complete() { "yes: every figure from gap-free shared cells" } else { "no: see the gaps and the dashes" }, 12.5, theme::TEXT),
    ));
    // A sum over the cells both sides report; None (UNMEASURED) if any is unknown or the sum overflows.
    let shared = |f: fn(&model_anatomy::CellComparison) -> Option<i64>| {
        c.cells.iter().filter(|x| x.presence == CellPresence::Both).map(f).try_fold(0i64, |a, v| v.and_then(|v| a.checked_add(v)))
    };
    col = col.push(fact(
        "Parameters, shared cells",
        label(
            format!(
                "{} -> {} ({})",
                count(shared(|x| x.left_parameters)),
                count(shared(|x| x.right_parameters)),
                delta(shared(|x| x.parameter_delta)),
            ),
            12.5,
            theme::TEXT,
        ),
    ));
    for gap in c.gaps.iter().chain(&c.left_gaps).chain(&c.right_gaps) {
        col = col.push(label(format!("Gap: {gap}"), 12.0, theme::CAUTION));
    }
    col = col.push(
        row![
            container(label("Cell", 11.5, theme::TEXT_FAINT)).width(170),
            container(label("Where", 11.5, theme::TEXT_FAINT)).width(80),
            container(label("Parameters", 11.5, theme::TEXT_FAINT)).width(230),
            container(label("Change", 11.5, theme::TEXT_FAINT)).width(150),
            container(label("Stored bytes", 11.5, theme::TEXT_FAINT)).width(120),
            label("Active", 11.5, theme::TEXT_FAINT),
        ]
        .spacing(6),
    );
    for cell in &c.cells {
        let place = match cell.block {
            Some(b) => format!("blk {b} · {}", cell.module),
            None => cell.module.clone(),
        };
        let presence = match cell.presence {
            CellPresence::Both => "both",
            CellPresence::LeftOnly => "left only",
            CellPresence::RightOnly => "right only",
        };
        let relative = cell.relative_parameter_difference.as_ref().map_or(String::new(), |r| {
            let percent = r.0 as f64 / r.1 as f64 * 100.0;
            format!(" ({percent:+.1}%)")
        });
        col = col.push(
            row![
                container(mono(place, 11.5, theme::TEXT)).width(170),
                container(label(presence, 11.5, theme::TEXT_DIM)).width(80),
                container(label(format!("{} -> {}", count(cell.left_parameters), count(cell.right_parameters)), 11.5, theme::TEXT)).width(230),
                container(label(format!("{}{relative}", delta(cell.parameter_delta)), 11.5, theme::TEXT)).width(150),
                container(label(delta(cell.nominal_bytes_delta), 11.5, theme::TEXT_DIM)).width(120),
                label(delta(cell.active_parameters_delta), 11.5, theme::TEXT_DIM),
            ]
            .spacing(6),
        );
    }
    col.into()
}

// ------------------------------------------------------------------ words, as the Python view writes them

/// `{:,}` of an integer.
pub fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `{:,.Nf}` of a float (a negative value keeps its sign, as Python's does).
fn grouped_fixed(x: f64, places: usize) -> String {
    let text = format!("{:.*}", places, x.abs());
    let (whole, fraction) = text.split_once('.').unwrap_or((text.as_str(), ""));
    let sign = if x.is_sign_negative() && !x.is_nan() { "-" } else { "" };
    let whole = whole.parse::<i64>().map(grouped).unwrap_or_else(|_| whole.to_string());
    if fraction.is_empty() { format!("{sign}{whole}") } else { format!("{sign}{whole}.{fraction}") }
}

/// `_si`: parameter counts in the units people say them in.
pub fn si(value: Option<i64>) -> String {
    let Some(value) = value else { return "—".to_string() };
    for (scale, suffix) in [(1_000_000_000i64, "B"), (1_000_000, "M"), (1_000, "K")] {
        if value.unsigned_abs() >= scale as u64 {
            return format!("{}{suffix}", grouped_fixed(value as f64 / scale as f64, 2));
        }
    }
    grouped(value)
}

/// `_bytes`: nominal storage, or UNMEASURED.
pub fn byte_size(value: Option<i64>) -> String {
    let Some(value) = value else { return "UNMEASURED".to_string() };
    let mut size = value as f64;
    for suffix in ["B", "KiB", "MiB", "GiB", "TiB"] {
        if size < 1024.0 || suffix == "TiB" {
            return format!("{} {suffix}", grouped_fixed(size, 1));
        }
        size /= 1024.0;
    }
    format!("{} TiB", grouped_fixed(size, 1))
}

fn ratio_float(r: &model_anatomy::Ratio) -> f64 {
    r.0 as f64 / r.1 as f64
}

// ------------------------------------------------------------------ the volume

/// `isometric.Z_RISE / Z_RUN`: how far a cube rises per stage (an exact 18/25, so no two cubes coincide).
const Z_RISE: i64 = 18;
const Z_RUN: i64 = 25;
const Z_SCALE: f32 = 18.0 / 25.0;
const CUBE_FILL: f32 = 0.82;
const MARGIN_X: f32 = 40.0;
const MARGIN_Y: f32 = 56.0;
const MIN_CELL: f32 = 3.0;

/// `voxel_view._RAMP`: dark to gold, one hue.
const RAMP: [(f32, [u8; 3]); 5] = [
    (0.00, [0x14, 0x14, 0x14]),
    (0.25, [0x4a, 0x3a, 0x18]),
    (0.55, [0x8f, 0x7a, 0x3e]),
    (0.80, [0xc9, 0xa3, 0x4a]),
    (1.00, [0xf0, 0xd4, 0x89]),
];
/// `voxel_view._BIT_BANDS`: a precision is a format, not a point on a line.
pub const BIT_BANDS: [(f64, [u8; 3], &str); 6] = [
    (3.0, [0x7b, 0x3b, 0x2e], "≤3 bit"),
    (5.0, [0xa8, 0x6a, 0x2c], "4–5 bit"),
    (7.0, [0xc9, 0xa3, 0x4a], "6–7 bit"),
    (9.0, [0x7f, 0xa8, 0x6a], "8–9 bit"),
    (17.0, [0x5a, 0x8f, 0xc7], "16 bit"),
    (99.0, [0x9a, 0x86, 0xc9], "32 bit +"),
];
/// `viz_palette.BASELINE`: a cell whose precision is UNMEASURED, and it must look it.
const UNMEASURED: [u8; 3] = [0x38, 0x38, 0x35];

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb8(r, g, b)
}

pub fn ramp(t: f64) -> Color {
    let t = t.clamp(0.0, 1.0) as f32;
    let mut previous = RAMP[0];
    for stop in &RAMP[1..] {
        if t <= stop.0 {
            let span = stop.0 - previous.0;
            let f = if span <= 0.0 { 0.0 } else { (t - previous.0) / span };
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f) as u8;
            return Color::from_rgb8(
                mix(previous.1[0], stop.1[0]),
                mix(previous.1[1], stop.1[1]),
                mix(previous.1[2], stop.1[2]),
            );
        }
        previous = *stop;
    }
    rgb(RAMP[4].1)
}

pub fn band(bits: Option<f64>) -> Color {
    let Some(bits) = bits else { return rgb(UNMEASURED) };
    for (ceiling, colour, _) in BIT_BANDS {
        if bits < ceiling {
            return rgb(colour);
        }
    }
    rgb(BIT_BANDS[5].1)
}

fn scaled(c: Color, k: f32) -> Color {
    Color::from_rgb((c.r * k).min(1.0), (c.g * k).min(1.0), (c.b * k).min(1.0))
}

/// `isometric.depth_key`: back to front, ascending.
fn depth_key(xi: i64, yi: i64, zi: i64) -> i64 {
    (xi + yi) * Z_RISE + zi * Z_RUN
}

/// The grid a voxel list is drawn on: each axis's distinct values, in order, as indices.
struct Grid {
    xs: Vec<i64>,
    ys: Vec<i64>,
    zs: Vec<i64>,
}

impl Grid {
    fn of(voxels: &[&Voxel]) -> Grid {
        let distinct = |f: fn(&Voxel) -> i64| {
            let mut v: Vec<i64> = voxels.iter().map(|x| f(x)).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        Grid { xs: distinct(|v| v.x), ys: distinct(|v| v.y), zs: distinct(|v| v.z) }
    }

    fn index(values: &[i64], value: i64) -> i64 {
        values.binary_search(&value).map(|i| i as i64).unwrap_or(0)
    }

    fn at(&self, v: &Voxel) -> (i64, i64, i64) {
        (Self::index(&self.xs, v.x), Self::index(&self.ys, v.y), Self::index(&self.zs, v.z))
    }

    /// `isometric.cell_size`: one cell's edge so the volume fits.
    fn cell(&self, size: Size) -> f32 {
        let (cx, cy, cz) = (self.xs.len() as f32, self.ys.len() as f32, self.zs.len() as f32);
        let span_w = if cx + cy > 0.0 { cx + cy } else { 1.0 };
        let span_h = (cx + cy) * 0.5 + cz * Z_SCALE + 2.0;
        ((size.width - MARGIN_X) / span_w).min((size.height - MARGIN_Y) / span_h.max(1.0)).max(MIN_CELL)
    }
}

/// `isometric.project`: y grows into the screen, z upward at `Z_SCALE`.
fn project(x: f32, y: f32, z: f32, cell: f32, origin: Point) -> Point {
    Point::new(origin.x + (x - y) * cell, origin.y + (x + y) * cell * 0.5 - z * cell * Z_SCALE)
}

/// `isometric.cube_faces`: (top, left, right).
fn cube_faces(xi: i64, yi: i64, zi: i64, cell: f32, origin: Point) -> [[Point; 4]; 3] {
    let inset = (1.0 - CUBE_FILL) * 0.5;
    let (x, y, z) = (xi as f32, yi as f32, zi as f32);
    let (low, high, left, right, back, front) = (z + inset, z + 1.0 - inset, x + inset, x + 1.0 - inset, y + inset, y + 1.0 - inset);
    let p = |a, b, c| project(a, b, c, cell, origin);
    let (p100, p110, p010) = (p(right, back, low), p(right, front, low), p(left, front, low));
    let (p001, p101, p111, p011) = (p(left, back, high), p(right, back, high), p(right, front, high), p(left, front, high));
    [[p001, p101, p111, p011], [p011, p010, p110, p111], [p101, p100, p110, p111]]
}

fn polygon(points: &[Point; 4]) -> Path {
    Path::new(|b| {
        b.move_to(points[0]);
        for q in &points[1..] {
            b.line_to(*q);
        }
        b.close();
    })
}

/// Whether `p` is inside the convex quad `q` (its points in order).
fn inside(q: &[Point; 4], p: Point) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross.abs() < f32::EPSILON {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// The canonical voxel field, drawn isometrically: blocks across (x, the stack-external plane at x = -1 leading),
/// families into the screen (y), stages upward (z).
pub struct Volume<'a> {
    pub field: &'a VoxelField,
    pub colour: Colour,
    pub family: Option<i64>,
    pub labels: &'a HashMap<String, String>,
    pub title: String,
    /// The cells a lit finding is about: drawn as they are, every other cell dimmed.
    pub focus: Option<&'a Focus>,
}

impl Volume<'_> {
    fn visible(&self) -> Vec<&Voxel> {
        self.field.voxels.iter().filter(|v| self.family.is_none_or(|f| v.y == f)).collect()
    }

    fn fill(&self, v: &Voxel) -> Color {
        match self.colour {
            Colour::Precision => band(v.bits_per_weight),
            // An unknown active share falls back to the stored ramp rather than to zero (a dark cell would read as
            // "a token barely touches this", a measurement nobody made).
            Colour::Active => ramp(v.active_intensity.unwrap_or(v.intensity)),
            Colour::Parameters => ramp(v.intensity),
        }
    }

    /// The hover line, as the Python view writes it.
    fn describe(&self, v: &Voxel) -> String {
        let label = self.labels.get(&v.module).or_else(|| self.labels.get("other")).cloned().unwrap_or_else(|| v.module.clone());
        let place = if v.x < 0 { "outside the stack".to_string() } else { format!("block {}", v.x) };
        let bits = match v.bits_per_weight {
            Some(b) => format!("{b:.2} bits/weight nominal"),
            None => "precision UNMEASURED".to_string(),
        };
        let routed = match v.active_parameters {
            Some(a) if a != v.parameters => format!(" · {} reached per token", grouped(a)),
            _ => String::new(),
        };
        format!("{label} · {place} · {} parameters{routed} · {bits}", grouped(v.parameters))
    }
}

impl<Message> canvas::Program<Message> for Volume<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::SURFACE);
        let caption = |frame: &mut Frame, content: String, y: f32, color: Color| {
            frame.fill_text(Text { content, position: Point::new(8.0, y), color, size: 11.5.into(), font: fonts().ui, ..Text::default() });
        };
        let voxels = self.visible();
        caption(&mut frame, self.title.clone(), 6.0, theme::GOLD);
        if voxels.is_empty() {
            let words = if self.family.is_some() {
                "This model has no cells in the selected family."
            } else {
                "The model inventory produced no cells — see the named gaps beside the figures."
            };
            caption(&mut frame, words.to_string(), bounds.height / 2.0, theme::TEXT_DIM);
            return vec![frame.into_geometry()];
        }
        let grid = Grid::of(&voxels);
        let cell = grid.cell(bounds.size());
        let origin = Point::new(20.0 + grid.ys.len() as f32 * cell, 40.0 + grid.zs.len() as f32 * cell * Z_SCALE);
        let mut ordered: Vec<(&Voxel, (i64, i64, i64))> = voxels.iter().map(|v| (*v, grid.at(v))).collect();
        ordered.sort_by_key(|(_, (x, y, z))| depth_key(*x, *y, *z));
        let pointer = cursor.position_in(bounds);
        let mut hovered = None;
        let edge = Stroke::default().with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.43)).with_width(0.7);
        for (voxel, (x, y, z)) in &ordered {
            let mut base = self.fill(voxel);
            if self.focus.is_some_and(|f| !f.covers((voxel.x >= 0).then_some(voxel.x), &voxel.module)) {
                base = Color { a: 1.0, ..scaled(base, 0.22) };
            }
            let [top, left, right] = cube_faces(*x, *y, *z, cell, origin);
            for (face, shade) in [(right, 1.0 / 1.4), (left, 1.0), (top, 1.28)] {
                let path = polygon(&face);
                frame.fill(&path, scaled(base, shade));
                frame.stroke(&path, edge);
            }
            // The last top face under the pointer is the nearest: cubes are drawn back to front.
            if pointer.is_some_and(|p| inside(&top, p)) {
                hovered = Some(*voxel);
            }
        }
        caption(
            &mut frame,
            format!(
                // Not the Python view's "↘": the window's UI font draws it as an emoji.
                "depth → {} block plane(s)   ·   families (into the page) {}   ·   stage ↑ {}   ·   {} occupied cells",
                grid.xs.len(),
                grid.ys.len(),
                grid.zs.len(),
                self.field.occupied
            ),
            22.0,
            theme::TEXT_DIM,
        );
        let line = match hovered {
            Some(v) => self.describe(v),
            None => "Hover a cell for its module, block and size.".to_string(),
        };
        caption(&mut frame, line, bounds.height - 18.0, if hovered.is_some() { theme::TEXT } else { theme::TEXT_FAINT });
        vec![frame.into_geometry()]
    }
}

// ------------------------------------------------------------------ the view

const PURPOSE: &str = "The volume is measured from what the model's GGUF header declares: its tensor inventory, or the \
                       architecture fields when it lists none, so it shows structure and storage precision, never what the \
                       model has learned. The Reading beside it says what is known and how; the weights and a prompt are \
                       read only when you ask (Probes).";

pub fn view(state: &State, phase: f32) -> El<'_> {
    let mut left = Column::new().spacing(10).width(Length::FillPortion(3)).height(Length::Fill);
    left = left.push(note(PURPOSE));
    left = left.push(controls(state));
    if let Some(name) = &state.measuring {
        left = left.push(ui::working(phase, format!("Measuring {name}: reading its GGUF header and arranging it on the canonical space…")));
    } else if state.listing && state.installed.is_none() {
        left = left.push(ui::working(phase, "Reading the installed models…"));
    }
    left = left.push(volumes(state));
    let right = container(scrollable(report(state).padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars))
        .width(Length::FillPortion(2))
        .height(Length::Fill);
    row![left, right].spacing(14).height(Length::Fill).into()
}

fn picker<'a, T: ToString + PartialEq + Clone + 'a>(
    options: Vec<T>,
    selected: Option<T>,
    on: impl Fn(T) -> Msg + 'a,
    placeholder: &'a str,
) -> El<'a> {
    pick_list(options, selected, on)
        .placeholder(placeholder)
        .padding(7)
        .text_size(13.0)
        .font(fonts().ui)
        .style(theme::picker)
        .menu_style(theme::picker_menu)
        .into()
}

fn controls(state: &State) -> El<'_> {
    let models: Vec<String> = state.installed.as_ref().map(|i| i.models.iter().map(|m| m.name.clone()).collect()).unwrap_or_default();
    let can_measure = state.chosen.is_some() && state.measuring.is_none();
    let mut colours = vec![Colour::Parameters, Colour::Precision];
    // Offered only for a model that has an active path: on any other it would be a third name for stored size.
    if state.current().is_some_and(|m| m.analysis.voxel_field.has_active_path) {
        colours.push(Colour::Active);
    }
    let families = state.reference.as_ref().map(|r| r.families.clone()).unwrap_or_default();
    let mut family_choices = vec![FamilyChoice(None, "All families".to_string())];
    for (i, name) in families.iter().enumerate() {
        let mut words = name.clone();
        if let Some(first) = words.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        family_choices.push(FamilyChoice(Some(i as i64), words));
    }
    let family_selected = family_choices.iter().find(|c| c.0 == state.family).cloned();
    let layers = layers_of(state);
    let layer_choices: Vec<LayerChoice> = layers
        .map(|l| {
            l.layers
                .iter()
                .enumerate()
                .map(|(i, layer)| LayerChoice(i, format!("Layer: {}{}", layer.label, if layer.drawn { "" } else { "  ·  no volume" })))
                .collect()
        })
        .unwrap_or_default();
    let layer_selected = state.layer.and_then(|i| layer_choices.get(i).cloned());
    row![
        container(picker(models.clone(), state.chosen.clone(), Msg::Pick, "No installed model")).width(Length::Fixed(240.0)),
        ui::primary("Measure", can_measure.then_some(Msg::Measure)),
        picker(colours, Some(state.colour), Msg::Colour, "Colour"),
        picker(family_choices, family_selected, Msg::Family, "Family"),
        picker(layer_choices, layer_selected, Msg::Layer, "Lineage layer"),
        ui::tabs(&[false, true], state.brain, |b| if b { "Brain".to_string() } else { "Frame".to_string() }, Msg::Brain),
        container(picker(
            models.iter().filter(|m| Some(*m) != state.chosen.as_ref()).cloned().collect(),
            state.compare_with.clone().filter(|c| Some(c) != state.chosen.as_ref()),
            Msg::CompareWith,
            "Compare with…",
        ))
        .width(Length::Fixed(220.0)),
        ui::secondary(
            "Compare",
            (!state.comparing && state.chosen.is_some() && state.compare_with.is_some() && state.compare_with != state.chosen)
                .then_some(Msg::Compare)
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .wrap()
    .into()
}

/// The lineage as measured for this model, else as it stands with none.
fn layers_of(state: &State) -> Option<&HierarchyMorphometry> {
    match state.current() {
        Some(m) => Some(&m.layers),
        None => state.reference.as_ref().and_then(|r| r.unmeasured.as_ref().ok()),
    }
}

fn volumes(state: &State) -> El<'_> {
    let installed = state.installed.as_ref();
    let empty_labels = &EMPTY_LABELS;
    let labels = state.reference.as_ref().map(|r| &r.module_labels).unwrap_or(empty_labels);
    let body: El<'_> = match (&state.measured, state.current()) {
        (_, Some(m)) if state.brain && state.brain_layout.is_some() => {
            let layout = state.brain_layout.as_ref().expect("checked");
            let picture = Canvas::new(super::brain::BrainView {
                brain: layout,
                title: format!("{} · as a stylised brain (cells laid on regions)", m.model),
            })
            .width(Length::FillPortion(3))
            .height(Length::Fill);
            let mut legend = Column::new().spacing(6).width(Length::Fill);
            legend = legend.push(
                ui::tabs(
                    &[true, false],
                    state.in_sinai,
                    |b| if b { "In Sinai's glass head".to_string() } else { "Here only".to_string() },
                    Msg::InSinai,
                ),
            );
            legend = legend.push(strong("Regions", 13.0, theme::GOLD));
            for share in &layout.regions {
                let [r, g, b] = share.region.colour();
                let line = row![ui::dot(Color::from_rgb(r, g, b)), label(share.region.name(), 12.5, theme::TEXT)]
                    .spacing(6)
                    .align_y(Alignment::Center);
                let what = if share.cells == 0 {
                    format!("{} · this model has none, so it is drawn faint", share.region.stands_for())
                } else {
                    let mut w = format!(
                        "{} · {} cell(s), {} parameters",
                        share.region.stands_for(),
                        share.cells,
                        grouped(share.parameters)
                    );
                    if share.unshown_cells > 0 {
                        w.push_str(&format!(" · {} too small for a cube at this size", share.unshown_cells));
                    }
                    w
                };
                legend = legend.push(column![line, label(what, 11.5, theme::TEXT_DIM)].spacing(2));
            }
            legend = legend.push(note(
                "Each region's cubes are shared among its cells in proportion to their parameters, blocks running from \
                 the back of the head to the front. The canonical frame and its commitment are unchanged; this is a \
                 picture drawn on top of them.",
            ));
            row![
                picture,
                container(scrollable(legend).height(Length::Fill))
                    .padding(10)
                    .width(Length::FillPortion(2))
                    .height(Length::Fill)
                    .style(theme::panel)
            ]
            .spacing(10)
            .height(Length::Fill)
            .into()
        }
        (_, Some(m)) => {
            let field = &m.analysis.voxel_field;
            let own = Canvas::new(Volume {
                field,
                colour: state.colour,
                family: state.family,
                labels,
                title: format!("{} · canonical (block, module) frame", m.model),
                focus: state.lit_finding().and_then(|f| f.focus.as_ref()),
            })
            .width(Length::FillPortion(3))
            .height(Length::Fill);
            let layer = state.layer.and_then(|i| m.layers.layers.get(i));
            let companion: El<'_> = match (layer, m.layers.field.as_ref()) {
                (Some(l), Some(shared)) if l.drawn => Canvas::new(Volume {
                    field: shared,
                    colour: state.colour,
                    family: state.family,
                    labels,
                    title: format!("Layer: {} · the same volume, registered onto its frame", l.label),
                    focus: state.lit_finding().and_then(|f| f.focus.as_ref()),
                })
                .width(Length::FillPortion(2))
                .height(Length::Fill)
                .into(),
                (Some(l), _) => container(
                    column![
                        strong(format!("Layer: {}", l.label), 13.0, theme::GOLD),
                        chip(l.state.as_str(), theme::TEXT_FAINT),
                        label(l.reason.as_str(), 12.0, theme::TEXT_DIM),
                        note("No volume is drawn here: an absence is shown beside the thing it is not."),
                    ]
                    .spacing(8),
                )
                .padding(14)
                .width(Length::FillPortion(2))
                .height(Length::Fill)
                .style(theme::panel)
                .into(),
                (None, _) => space().width(Length::FillPortion(2)).into(),
            };
            let mut under = column![legend(state, field)].spacing(6);
            if let Some(f) = state.lit_finding() {
                under = under.push(
                    row![
                        ui::dot(tone_colour(f.tone)),
                        label(format!("Lit: {} (every other cell dimmed)", f.headline), 11.5, theme::TEXT),
                        ui::secondary("Clear", Some(Msg::Light(None))),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .wrap(),
                );
            }
            column![row![own, companion].spacing(10).height(Length::Fill), under].spacing(6).height(Length::Fill).into()
        }
        (Some(Err(why)), None) => ui::notice(why.as_str(), theme::CAUTION),
        (None, None) => match installed {
            Some(i) if i.models.is_empty() => ui::notice(
                format!(
                    "No GGUF model is installed in {} (ALELYON_MODELS_DIR, else ~/.alelyon/models), so there is no volume \
                     to draw. A model added there appears here the next time this tab opens; the lineage, the frame and \
                     the transform families beside this do not depend on one.",
                    i.dir
                ),
                theme::GOLD_DIM,
            ),
            Some(_) => note("Pick a model and press Measure."),
            None => note("Reading the installed models…"),
        },
        (Some(Ok(_)), None) => note("Pick a model and press Measure."),
    };
    container(body).height(Length::Fill).into()
}

static EMPTY_LABELS: std::sync::LazyLock<HashMap<String, String>> = std::sync::LazyLock::new(HashMap::new);

fn legend(state: &State, field: &VoxelField) -> El<'static> {
    let mut line = Row::new().spacing(10).align_y(Alignment::Center);
    match state.colour {
        Colour::Precision => {
            for (_, colour, words) in BIT_BANDS {
                line = line.push(row![swatch(rgb(colour)), label(words, 11.5, theme::TEXT_DIM)].spacing(4).align_y(Alignment::Center));
            }
            line = line.push(row![swatch(rgb(UNMEASURED)), label("UNMEASURED", 11.5, theme::TEXT_DIM)].spacing(4).align_y(Alignment::Center));
        }
        colour => {
            for t in [0.0, 0.25, 0.55, 0.8, 1.0] {
                line = line.push(swatch(ramp(t)));
            }
            let maximum = if colour == Colour::Active { field.max_active_parameters.unwrap_or(0) } else { field.max_parameters };
            let what = if colour == Colour::Active { "parameters reached per token" } else { "parameters stored" };
            line = line.push(label(format!("dark → gold: 0 → {} {what} per cell", grouped(maximum)), 11.5, theme::TEXT_DIM));
        }
    }
    line.into()
}

fn swatch(colour: Color) -> El<'static> {
    container(space().width(12).height(10))
        .style(move |_: &Theme| container::Style { background: Some(colour.into()), ..container::Style::default() })
        .into()
}

fn section<'a>(title: &'a str) -> El<'a> {
    strong(title, 14.0, theme::GOLD).into()
}

fn report(state: &State) -> Column<'_, Msg> {
    let mut col = Column::new().spacing(12);
    col = col.push(ui::tabs(&Pane::ALL, state.pane, Pane::title, Msg::Pane));
    match state.pane {
        Pane::Reading => col = col.push(reading_view(state)),
        Pane::Structure => match state.current() {
            Some(m) => col = col.push(model_report(m)),
            None => col = col.push(note("Pick a model and press Measure.")),
        },
        Pane::Probes => col = col.push(probes_card(state)),
        Pane::Compare => {
            if state.comparing {
                col = col.push(note("Comparing the two models' headers on the canonical frame…"));
            }
            match &state.comparison {
                Some(Ok(c)) => col = col.push(comparison_report(c)),
                Some(Err(why)) => col = col.push(ui::notice(why.as_str(), theme::CAUTION)),
                None if !state.comparing => col = col.push(note(
                    "Choose a second model under \"Compare with…\" above and press Compare: the two headers are set side \
                     by side on the canonical frame, cell by cell, right minus left.",
                )),
                None => {}
            }
        }
        Pane::Frames => col = frames(state, col),
    }
    col
}

fn tone_colour(tone: Tone) -> Color {
    match tone {
        Tone::Plain => theme::GOLD_DIM,
        Tone::Notable => theme::GOLD,
        Tone::Caution => theme::CAUTION,
    }
}

fn basis_colour(basis: Basis) -> Color {
    match basis {
        Basis::Unmeasured => theme::CAUTION,
        Basis::Observed => theme::TEXT_DIM,
        _ => theme::GOLD_DIM,
    }
}

/// The Reading: the model at a glance, then each finding (opened one at a time), then what could be read next.
fn reading_view(state: &State) -> El<'_> {
    let Some(r) = state.reading.as_deref() else {
        return match &state.measured {
            Some(Err(why)) => ui::notice(why.as_str(), theme::CAUTION),
            _ if state.measuring.is_some() => note("Reading the model…"),
            _ => note("Pick a model and press Measure: a plain reading of it appears here."),
        };
    };
    let mut col = Column::new().spacing(10);
    if let Some(why) = &state.kept_note {
        col = col.push(ui::notice(why.as_str(), theme::CAUTION));
    }
    let mut glance = Column::new().spacing(6).push(strong("At a glance", 13.0, theme::GOLD));
    for line in &r.glance {
        glance = glance.push(label(line.as_str(), 14.0, theme::TEXT));
    }
    col = col.push(container(glance).padding(12).width(Length::Fill).style(theme::panel));
    let mut key = Row::new().spacing(10);
    for b in Basis::ALL {
        key = key.push(row![chip(b.word(), basis_colour(b)), label(b.meaning(), 11.0, theme::TEXT_FAINT)].spacing(4).align_y(Alignment::Center));
    }
    col = col.push(key.wrap());
    col = col.push(note("Open a finding for the figures behind it and what it does not tell you."));
    for f in &r.findings {
        let open = state.opened == Some(f.id);
        let head = button(
            row![
                ui::dot(tone_colour(f.tone)),
                column![label(f.topic, 11.0, theme::TEXT_FAINT), label(f.headline.as_str(), 13.0, theme::TEXT)]
                    .spacing(2)
                    .width(Length::Fill),
                chip(f.basis.word(), basis_colour(f.basis)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([7.0, 9.0])
        .style(theme::list_row(open))
        .on_press(Msg::Open(f.id));
        col = col.push(head);
        if open {
            let mut body = Column::new().spacing(4);
            for line in &f.figures {
                body = body.push(label(format!("• {line}"), 12.0, theme::TEXT_DIM));
            }
            if !f.caveat.is_empty() {
                body = body.push(
                    row![strong("What it does not tell you:", 11.5, theme::TEXT_DIM), label(f.caveat.as_str(), 11.5, theme::TEXT_FAINT)]
                        .spacing(6)
                        .wrap(),
                );
            }
            if f.focus.is_some() {
                let lit = state.lit == Some(f.id);
                body = body.push(row![ui::secondary(
                    if lit { "Stop showing on the volume" } else { "Show on the volume" },
                    Some(Msg::Light(if lit { None } else { Some(f.id) }))
                )]);
            }
            col = col.push(container(body).padding([8.0, 14.0]).width(Length::Fill).style(theme::well));
        }
    }
    if !r.further.is_empty() || state.weights_running.is_some() {
        col = col.push(strong("Read further", 13.0, theme::GOLD));
        if let Some(running) = &state.weights_running {
            col = col.push(
                row![
                    label(format!("Reading the weights: {:.0}%", running.share() * 100.0), 12.5, theme::TEXT),
                    ui::secondary("Stop", Some(Msg::StopWeights)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        for f in &r.further {
            if *f == Further::Weights && state.weights_running.is_some() {
                continue;
            }
            col = col.push(
                button(label(f.offer(), 12.5, theme::TEXT))
                    .width(Length::Fill)
                    .padding([7.0, 9.0])
                    .style(theme::list_row(false))
                    .on_press(Msg::Further(*f)),
            );
        }
    }
    col.into()
}

/// The frame the model is laid on: its registration, the lineage, the template hierarchy and the transform facts.
fn frames<'a>(state: &'a State, mut col: Column<'a, Msg>) -> Column<'a, Msg> {
    match state.current() {
        Some(m) if m.analysis.morphometry.derived.ok => {
            col = col.push(registration(state, m));
            col = col.push(section("Lineage layers"));
            col = col.push(lineage_rows(&m.layers));
            col = col.push(note(
                "The volume is measured once and shared by every frame it registers onto, so these are FRAMES rather than \
                 five measurements of this model. A tier marked COORDINATE_SPACE_ABSENT is abstract by design and no axes \
                 were invented for it.",
            ));
        }
        Some(_) => {}
        None => {
            col = col.push(note(
                "Pick a model and press Measure. Its header is arranged on the canonical (block, module) coordinate \
                 space and registered against it by the exact-transform core. A header that lists its tensors is \
                 measured cell by cell; one that does not is computed from its declared dimensions, which is \
                 arithmetic for a dense decoder and is refused by name for a mixture-of-experts model.",
            ));
            if let Some(Ok(layers)) = state.reference.as_ref().map(|r| &r.unmeasured) {
                col = col.push(ui::card("Lineage layers (no model measured)", lineage_rows(layers)));
            }
        }
    }
    if let Some(r) = &state.reference {
        col = col.push(hierarchy_card(r));
        col = col.push(facts_card(state, r));
    }
    col
}

/// How the model's frame is registered on the canonical space.
fn registration<'a>(state: &'a State, m: &'a Measured) -> El<'a> {
    let morph = &m.analysis.morphometry;
    let mut col = Column::new().spacing(8).push(section("Registration"));
    let canonical = state.reference.as_ref().and_then(|r| r.canonical.as_ref().ok());
    col = col.push(fact(
        "Canonical space",
        mono(canonical.map(|c| format!("{} v{}", c.space_id, c.version)).unwrap_or_else(|| "UNREAD".into()), 12.0, theme::TEXT),
    ));
    col = col.push(fact("Space commitment", mono(canonical.map(|c| c.commitment.clone()).unwrap_or_else(|| "UNREAD".into()), 11.0, theme::TEXT)));
    col = col.push(fact("Native axis order", mono(morph.native_axis_order.join(" → "), 12.0, theme::TEXT)));
    if let Some(r) = &m.registration {
        col = col.push(fact("Result", chip(r.code.as_str(), if r.compatible { theme::POSITIVE } else { theme::CAUTION })));
        col = col.push(fact("What that proves", label(r.explanation.as_str(), 12.0, theme::TEXT)));
        if let Some(chain) = &r.transform {
            let steps: Vec<String> = chain
                .transforms
                .iter()
                .map(|s| match &s.source_order {
                    Some(order) => format!("{} source_order {:?}", s.transform_type, order),
                    None => s.transform_type.clone(),
                })
                .collect();
            col = col.push(fact(
                "Transform",
                mono(format!("{} ({}, invertibility {})", steps.join(" → "), chain.loss_class, chain.invertibility), 11.5, theme::TEXT),
            ));
        }
        if let Some(failing) = &r.failing_constraint {
            col = col.push(fact("Failing constraint", mono(failing.as_str(), 11.5, theme::TEXT)));
        }
        for item in &r.evidence {
            col = col.push(label(format!("• {item}"), 11.5, theme::TEXT_DIM));
        }
    }
    col = col.push(note(
        "The commitment covers the coordinate FRAME, not the model: it binds none of the figures below. The Python \
         desktop can sign a Registration Certificate for this registration; this window signs nothing, and nothing \
         here has been verified by an external party.",
    ));
    col.into()
}

fn model_report(m: &Measured) -> El<'_> {
    let morph = &m.analysis.morphometry;
    let d = &morph.derived;
    let mut col = Column::new().spacing(8);
    col = col.push(
        row![
            strong(if morph.model.is_empty() { "model" } else { morph.model.as_str() }, 16.0, theme::TEXT),
            label(
                format!(
                    "· {}",
                    if !morph.family.is_empty() {
                        morph.family.as_str()
                    } else if !morph.architecture.is_empty() {
                        morph.architecture.as_str()
                    } else {
                        "unknown family"
                    }
                ),
                13.0,
                theme::TEXT_DIM
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    col = col.push(mono(m.path.as_str(), 11.0, theme::TEXT_FAINT));
    if !d.ok {
        col = col.push(ui::notice(
            format!("Refused: {}", if morph.refusal.is_empty() { "no cells produced" } else { morph.refusal.as_str() }),
            theme::CAUTION,
        ));
        col = col.push(gaps(morph));
        return col.into();
    }
    // Inventory.
    col = col.push(section("Inventory"));
    let source = if morph.source == "TENSOR_INVENTORY" { "counted tensor inventory" } else { "derived from declared architecture" };
    col = col.push(fact("Source", label(source, 12.0, theme::TEXT)));
    col = col.push(fact("Architecture", label(if morph.architecture.is_empty() { "—" } else { &morph.architecture }, 12.0, theme::TEXT)));
    col = col.push(fact("Quantisation", label(if morph.quantization.is_empty() { "—" } else { &morph.quantization }, 12.0, theme::TEXT)));
    col = col.push(fact("Blocks", label(d.blocks.len().to_string(), 12.0, theme::TEXT)));
    col = col.push(fact("Tensors", label(if morph.tensors.is_empty() { "—".to_string() } else { grouped(morph.tensors.len() as i64) }, 12.0, theme::TEXT)));
    col = col.push(fact("Counted parameters", mono(si(Some(d.counted_parameters)), 12.0, theme::TEXT)));
    col = col.push(fact("Runtime declares", mono(si(morph.declared_parameters), 12.0, theme::TEXT)));
    col = col.push(fact(
        "Coverage",
        label(
            match d.coverage {
                Some(c) => format!("{:.2}%", c * 100.0),
                None => "UNMEASURED — the runtime declared no count".to_string(),
            },
            12.0,
            theme::TEXT,
        ),
    ));
    col = col.push(fact("Nominal storage", mono(byte_size(d.nominal_bytes), 12.0, theme::TEXT)));
    col = col.push(fact(
        "Context length",
        label(match morph.context_length {
            Some(n) if n != 0 => grouped(n),
            _ => "—".to_string(),
        }, 12.0, theme::TEXT),
    ));
    // Routing, only for a routed model.
    if d.is_mixture_of_experts {
        col = col.push(routing(morph));
    }
    // Families and depth.
    col = col.push(section("Where the parameters sit"));
    let total = if d.counted_parameters == 0 { 1 } else { d.counted_parameters };
    for (name, count) in &d.by_family {
        let share = 100.0 * *count as f64 / total as f64;
        col = col.push(
            row![
                container(label(name.as_str(), 12.0, theme::TEXT)).width(120),
                container(mono(si(Some(*count)), 12.0, theme::TEXT)).width(90),
                container(label(format!("{share:.1}%"), 12.0, theme::TEXT_DIM)).width(60),
                container(space().width(Length::Fixed((share.clamp(0.0, 100.0) * 0.9) as f32)).height(7))
                    .style(|_: &Theme| container::Style { background: Some(theme::GOLD_DIM.into()), ..container::Style::default() }),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    col = col.push(depth(morph));
    col = col.push(gaps(morph));
    col.into()
}

fn routing(morph: &Morphometry) -> El<'_> {
    let d = &morph.derived;
    let mut col = Column::new().spacing(6).push(section("Routing"));
    match d.active_parameters {
        None => {
            let declared = |value: Option<i64>, field: &str| match value {
                Some(v) => grouped(v),
                None if morph.routing_metadata_error.contains(field) => "INVALID".to_string(),
                None => "NOT DECLARED".to_string(),
            };
            col = col
                .push(fact("Routed expert parameters", mono(d.routed_expert_parameters.map(|r| si(Some(r))).unwrap_or_else(|| "UNMEASURED".into()), 12.0, theme::TEXT)))
                .push(fact("Experts per block", label(declared(morph.expert_count, "expert_count"), 12.0, theme::TEXT)))
                .push(fact("Experts used per token", label(declared(morph.expert_used_count, "expert_used_count"), 12.0, theme::TEXT)))
                .push(fact("Active parameters", label("UNMEASURED", 12.0, theme::TEXT)))
                .push(note(d.active_path_gap.clone().unwrap_or_else(|| {
                    "This model routes part of its feed-forward stack, but the runtime did not establish a usable expert \
                     partition."
                        .to_string()
                })));
        }
        Some(active) => {
            let share = d.active_fraction.unwrap_or(0.0);
            let (experts, used) = (morph.expert_count.unwrap_or(0), morph.expert_used_count.unwrap_or(0));
            col = col
                .push(fact("Experts per block", label(grouped(experts), 12.0, theme::TEXT)))
                .push(fact("Experts used per token", label(format!("{} of {}", grouped(used), grouped(experts)), 12.0, theme::TEXT)))
                .push(fact("Routed expert parameters", mono(si(d.routed_expert_parameters), 12.0, theme::TEXT)))
                .push(fact("Always active", mono(si(d.always_active_parameters), 12.0, theme::TEXT)))
                .push(fact("Active per token", mono(format!("{}  ({:.2}% of stored)", si(Some(active)), share * 100.0), 12.0, theme::TEXT)))
                .push(note(format!(
                    "Parameter share is where the weights are STORED. On this model a token reaches {:.1}% of them, so \
                     the expert bank's share is not its share of the work. Declared routing, not an observed one: \
                     nothing here watches which experts fire.",
                    share * 100.0
                )));
        }
    }
    col.into()
}

fn depth(morph: &Morphometry) -> El<'_> {
    let d = &morph.derived;
    let mut col = Column::new().spacing(4);
    if d.blocks.is_empty() {
        return col.into();
    }
    col = col.push(section("Depth profile"));
    let outliers = &d.block_dispersion.outliers;
    let mut spread = match d.block_dispersion.relative {
        None => "dispersion UNMEASURED — fewer than three blocks".to_string(),
        Some(r) => format!("blocks differ from their median by {:.2}% (median absolute deviation)", r * 100.0),
    };
    if !outliers.is_empty() {
        let shown: Vec<String> = outliers.iter().take(8).map(i64::to_string).collect();
        spread.push_str(&format!("; {} block(s) beyond three deviations: {}", outliers.len(), shown.join(", ")));
    }
    col = col.push(note(spread));
    let cells = |a: String, b: String, c: String, e: String, color: Color| {
        row![
            container(label(a, 11.5, color)).width(70),
            container(label(b, 11.5, color)).width(90),
            container(label(c, 11.5, color)).width(70),
            container(label(e, 11.5, color)).width(80),
        ]
        .spacing(6)
    };
    col = col.push(cells("Block".into(), "Parameters".into(), "Modules".into(), "Bits/weight".into(), theme::TEXT_DIM));
    let shown: Vec<_> = if d.blocks.len() <= 12 {
        d.blocks.iter().collect()
    } else {
        d.blocks.iter().take(6).chain(d.blocks.iter().skip(d.blocks.len() - 6)).collect()
    };
    for b in shown {
        let flag = if outliers.contains(&b.block) { " ⚠" } else { "" };
        let bits = b.mean_bits_per_weight.as_ref().map(|r| format!("{:.2}", ratio_float(r))).unwrap_or_else(|| "—".into());
        col = col.push(cells(format!("{}{flag}", b.block), si(Some(b.parameters)), b.modules.to_string(), bits, theme::TEXT));
    }
    if d.blocks.len() > 12 {
        col = col.push(note(format!(
            "first and last six of {} blocks shown; the volume on the left draws all of them",
            d.blocks.len()
        )));
    }
    col.into()
}

fn gaps(morph: &Morphometry) -> El<'_> {
    if morph.gaps.is_empty() {
        return note("No named gaps: every figure above came from a declared value.");
    }
    let mut col = Column::new().spacing(4).push(section("Named gaps"));
    for gap in &morph.gaps {
        col = col.push(label(format!("• {gap}"), 11.5, theme::TEXT_DIM));
    }
    col.into()
}

fn lineage_rows(layers: &HierarchyMorphometry) -> Column<'_, Msg> {
    let mut col = Column::new().spacing(5);
    for layer in &layers.layers {
        let detail = if layer.drawn { format!("{} occupied cells", grouped(layer.occupied)) } else { layer.state.clone() };
        col = col.push(
            row![
                container(label(layer.label.as_str(), 12.0, theme::TEXT)).width(Length::Fill),
                label(detail, 12.0, if layer.drawn { theme::TEXT } else { theme::TEXT_FAINT }),
            ]
            .spacing(8),
        );
        for frame in &layer.frames {
            col = col.push(mono(
                format!(
                    "   {}@{} · {} v{} · {} · {}",
                    frame.template_id,
                    frame.template_version,
                    frame.space_id,
                    frame.space_version,
                    if frame.compatibility.is_empty() { "not registered" } else { &frame.compatibility },
                    frame.space_commitment
                ),
                10.5,
                theme::TEXT_DIM,
            ));
        }
    }
    col = col.push(fact("Volumes built", label(layers.volumes_built.to_string(), 12.0, theme::TEXT)));
    col
}

fn hierarchy_card(r: &Reference) -> El<'_> {
    let mut col = Column::new().spacing(6);
    match &r.hierarchy {
        Err(why) => col = col.push(ui::notice(format!("The hierarchy could not be read: {why}"), theme::CAUTION)),
        Ok(h) => {
            let mut tiers: Vec<&str> = h.nodes.iter().map(|n| n.tier.as_str()).collect();
            tiers.dedup();
            col = col
                .push(fact("State", chip(h.state.as_str(), if h.state == "AVAILABLE" { theme::POSITIVE } else { theme::CAUTION })))
                .push(fact("Declared templates", label(h.nodes.len().to_string(), 12.0, theme::TEXT)))
                .push(fact("Root declarations", label(h.roots.len().to_string(), 12.0, theme::TEXT)))
                .push(fact("Tiers represented", label(format!("{} of 5", tiers.len()), 12.0, theme::TEXT)))
                .push(fact("Storage", label("in memory only", 12.0, theme::TEXT)))
                .push(note("Base contract → domain family → application → optional study/cohort → resolution variant."));
            for node in &h.nodes {
                let frame = match &node.coordinate_space {
                    Some(s) => format!("frame {} v{} ({}) · {}", s.space_id, s.version, s.axes.join(", "), s.reference),
                    None => "abstract: no coordinate space".to_string(),
                };
                col = col.push(
                    column![
                        row![strong(node.label.as_str(), 12.5, theme::TEXT), chip(node.tier.as_str(), theme::GOLD_DIM)]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        mono(node.template_ref.key(), 11.0, theme::TEXT_DIM),
                        label(node.description.as_str(), 11.5, theme::TEXT_DIM),
                        mono(frame, 10.5, theme::TEXT_FAINT),
                    ]
                    .spacing(2),
                );
            }
            if !h.findings.is_empty() {
                col = col.push(strong("Validation findings", 12.5, theme::CAUTION));
                for f in &h.findings {
                    col = col.push(label(format!("• {}: {}", f.code, f.detail), 11.5, theme::TEXT_DIM));
                }
            }
            col = col.push(strong("Named gaps", 12.5, theme::TEXT));
            for gap in &h.gaps {
                col = col.push(label(format!("• {gap}"), 11.5, theme::TEXT_DIM));
            }
        }
    }
    ui::card("Template hierarchy", col)
}

fn facts_card<'a>(state: &'a State, r: &'a Reference) -> El<'a> {
    let mut col = Column::new().spacing(6);
    match &r.facts {
        Err(why) => col = col.push(ui::notice(format!("The transform facts could not be read: {why}"), theme::CAUTION)),
        Ok(facts) => {
            col = col.push(note(format!(
                "Read from {} by the goldens tool and checked for drift; facts, not logic. This window builds only \
                 IDENTITY and AXIS_PERMUTATION, the two a model's frame can reach.",
                facts.source
            )));
            let ladder: Vec<String> = facts.loss_class_rank.iter().map(|(c, r)| format!("{c} {r}")).collect();
            col = col.push(fact("Loss ladder", mono(ladder.join(" < "), 10.5, theme::TEXT_DIM)));
            col = col.push(note(
                "Ordered from strongest to weakest; a CHAIN declares the weakest class any member declares. A loss \
                 class is a statement about the MAP, never about the claim.",
            ));
            for (i, family) in facts.families.iter().enumerate() {
                let open = state.fact == Some(i);
                col = col.push(
                    button(
                        row![
                            container(label(family.class_name.as_str(), 12.0, if open { theme::GOLD } else { theme::TEXT })).width(Length::Fill),
                            chip(family.loss_class.as_str(), theme::GOLD_DIM),
                            chip(family.invertibility.as_str(), theme::TEXT_FAINT),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([5.0, 8.0])
                    .style(theme::list_row(open))
                    .on_press(Msg::Fact(i)),
                );
                if open {
                    col = col.push(fact("Transform type", mono(family.transform_type.as_str(), 11.5, theme::TEXT)));
                    col = col.push(note("What the engine says about it, verbatim (its docstring):"));
                    col = col.push(container(mono(family.doc.as_str(), 11.0, theme::TEXT_DIM)).padding(8).style(theme::well));
                }
            }
        }
    }
    ui::card("Transform families", col)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A GGUF v3 header: `kv` as (key, string or u32), tensors as (name, dims, ggml type).
    pub(crate) fn gguf_bytes(kv: &[(&str, Result<&str, u32>)], tensors: &[(&str, &[u64], u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let string = |out: &mut Vec<u8>, s: &str| {
            out.extend((s.len() as u64).to_le_bytes());
            out.extend(s.as_bytes());
        };
        out.extend(b"GGUF");
        out.extend(3u32.to_le_bytes());
        out.extend((tensors.len() as u64).to_le_bytes());
        out.extend((kv.len() as u64).to_le_bytes());
        for (key, value) in kv {
            string(&mut out, key);
            match value {
                Ok(text) => {
                    out.extend(8u32.to_le_bytes());
                    string(&mut out, text);
                }
                Err(n) => {
                    out.extend(4u32.to_le_bytes());
                    out.extend(n.to_le_bytes());
                }
            }
        }
        for (name, dims, ggml_type) in tensors {
            string(&mut out, name);
            out.extend((dims.len() as u32).to_le_bytes());
            for d in *dims {
                out.extend(d.to_le_bytes());
            }
            out.extend(ggml_type.to_le_bytes());
            out.extend(0u64.to_le_bytes());
        }
        out.resize(out.len().div_ceil(32) * 32 + 64, 0);
        out
    }

    pub(crate) fn header(bytes: &[u8]) -> gguf::Header {
        gguf::read_from(Cursor::new(bytes.to_vec()), bytes.len() as u64).expect("a header")
    }

    fn tiny() -> Vec<u8> {
        gguf_bytes(
            &[("general.architecture", Ok("llama")), ("llama.block_count", Err(2)), ("general.file_type", Err(15))],
            &[
                ("token_embd.weight", &[8, 100], 12),
                ("blk.0.attn_q.weight", &[8, 8], 12),
                ("blk.0.ffn_up.weight", &[8, 16], 14),
                ("blk.1.attn_q.weight", &[8, 8], 12),
                ("blk.1.ffn_up.weight", &[8, 16], 14),
                ("output.weight", &[8, 100], 14),
            ],
        )
    }

    #[test]
    fn a_header_is_measured_registered_and_laid_on_the_lineage_by_the_engine() {
        let m = measure_header(&header(&tiny()), "tiny", PathBuf::from("tiny.gguf")).expect("measured");
        let morph = &m.analysis.morphometry;
        assert!(morph.derived.ok, "{:?}", morph.refusal);
        assert_eq!(morph.quantization, "Q4_K_M");
        assert_eq!(morph.source, "TENSOR_INVENTORY");
        let report = m.registration.as_ref().expect("a measured model is registered");
        assert_eq!(report.code, "EXACT_IDENTITY");
        assert_eq!(m.layers.volumes_built, 1);
        // The lineage draws the canonical field itself, unchanged.
        assert_eq!(m.layers.field.as_ref(), Some(&m.analysis.voxel_field));
    }

    #[test]
    fn two_files_are_compared_right_minus_left_with_side_only_cells_left_undelta() {
        let dir = std::env::temp_dir().join(format!("centcom-compare-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let three = gguf_bytes(
            &[("general.architecture", Ok("llama")), ("llama.block_count", Err(3)), ("general.file_type", Err(15))],
            &[
                ("token_embd.weight", &[8, 100], 12),
                ("blk.0.attn_q.weight", &[8, 8], 12),
                ("blk.0.ffn_up.weight", &[8, 16], 14),
                ("blk.1.attn_q.weight", &[8, 8], 12),
                ("blk.1.ffn_up.weight", &[8, 32], 14),
                ("blk.2.attn_q.weight", &[8, 8], 12),
                ("output.weight", &[8, 100], 14),
            ],
        );
        let (left, right) = (dir.join("tiny.gguf"), dir.join("three.gguf"));
        std::fs::write(&left, tiny()).unwrap();
        std::fs::write(&right, three).unwrap();
        let c = compare_files(&left, "tiny", &right, "three").expect("compared");
        assert!(c.ok(), "{}: {}", c.code.as_str(), c.explanation);
        assert_eq!((c.left_model.as_str(), c.right_model.as_str()), ("tiny", "three"));
        let cell = |block: Option<i64>, module: &str| c.cells.iter().find(|x| x.key() == (block, module)).expect(module);
        let same = cell(Some(0), "attn_q");
        assert_eq!((same.presence, same.parameter_delta), (model_anatomy::CellPresence::Both, Some(0)));
        let grown = cell(Some(1), "ffn_up");
        assert_eq!((grown.parameter_delta, grown.relative_parameter_difference), (Some(128), Some(model_anatomy::Ratio(1, 1))));
        let new = cell(Some(2), "attn_q");
        assert_eq!((new.presence, new.parameter_delta, new.left_parameters), (model_anatomy::CellPresence::RightOnly, None, None));
        // Nothing to compare with: a missing file is named, not compared.
        assert!(compare_files(&dir.join("absent.gguf"), "absent", &right, "three").unwrap_err().contains("could not be read"));
        let _ = std::fs::remove_file(&left);
        let _ = std::fs::remove_file(&right);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_header_python_would_raise_on_is_named_not_measured() {
        let bad = gguf_bytes(&[("general.file_type", Ok("Q4"))], &[("blk.0.attn_q.weight", &[8, 8], 12)]);
        let why = measure_header(&header(&bad), "bad", PathBuf::from("bad.gguf")).unwrap_err();
        assert!(why.contains("ValueError"), "{why}");
        assert!(measure_file(std::path::Path::new("C:/definitely/not/here.gguf"), "x").is_err());
    }

    #[test]
    fn a_refused_model_is_not_registered_and_draws_no_layer() {
        let refused = gguf_bytes(&[("general.architecture", Ok("llama"))], &[]);
        let m = measure_header(&header(&refused), "empty", PathBuf::from("e.gguf")).expect("an answer");
        assert!(!m.analysis.morphometry.derived.ok);
        assert!(m.registration.is_none());
        assert_eq!(m.layers.volumes_built, 0);
        assert!(m.layers.layers.iter().all(|l| !l.drawn));
    }

    #[test]
    fn words_are_written_as_the_python_view_writes_them() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(-1000), "-1,000");
        assert_eq!(si(None), "—");
        assert_eq!(si(Some(999)), "999");
        assert_eq!(si(Some(8_030_261_248)), "8.03B");
        assert_eq!(si(Some(1_500)), "1.50K");
        assert_eq!(si(Some(1_234_567_890_123)), "1,234.57B");
        assert_eq!(byte_size(None), "UNMEASURED");
        assert_eq!(byte_size(Some(512)), "512.0 B");
        assert_eq!(byte_size(Some(1536)), "1.5 KiB");
    }

    #[test]
    fn the_colour_scales_are_the_python_views() {
        assert_eq!(ramp(0.0), Color::from_rgb8(0x14, 0x14, 0x14));
        assert_eq!(ramp(1.0), Color::from_rgb8(0xf0, 0xd4, 0x89));
        assert_eq!(band(None), Color::from_rgb8(0x38, 0x38, 0x35));
        assert_eq!(band(Some(4.5)), Color::from_rgb8(0xa8, 0x6a, 0x2c));
        assert_eq!(band(Some(16.0)), Color::from_rgb8(0x5a, 0x8f, 0xc7));
        assert_eq!(band(Some(32.0)), Color::from_rgb8(0x9a, 0x86, 0xc9));
    }

    #[test]
    fn cubes_are_painted_back_to_front_and_the_pointer_finds_the_nearest() {
        // The camera sits at +(1, 1, 1): larger coordinates are nearer and sort later.
        assert!(depth_key(0, 0, 0) < depth_key(1, 1, 1));
        assert!(depth_key(0, 0, 0) < depth_key(0, 0, 1));
        let [top, ..] = cube_faces(0, 0, 0, 20.0, Point::new(100.0, 100.0));
        let centre = Point::new(top.iter().map(|p| p.x).sum::<f32>() / 4.0, top.iter().map(|p| p.y).sum::<f32>() / 4.0);
        assert!(inside(&top, centre));
        assert!(!inside(&top, Point::new(-50.0, -50.0)));
    }

    #[test]
    fn the_first_listing_measures_the_chosen_model_once() {
        let mut state = State::default();
        let model = files::LocalModel { name: "b".into(), path: PathBuf::from("C:/no/b.gguf"), size: 1 };
        let other = files::LocalModel { name: "a".into(), path: PathBuf::from("C:/no/a.gguf"), size: 1 };
        let installed = Installed { dir: "C:/no".into(), models: vec![other, model], selected: "b".into() };
        let _ = state.update(Msg::Read(Arc::new((installed.clone(), Reference::failed("test".into())))));
        assert_eq!(state.chosen.as_deref(), Some("b"), "the managed server's chosen model first");
        assert_eq!(state.measuring.as_deref(), Some("b"));
        let _ = state.update(Msg::Measured("b".into(), Arc::new(Err("x".into()))));
        let _ = state.update(Msg::Read(Arc::new((installed, Reference::failed("test".into())))));
        assert!(state.measuring.is_none(), "a later listing does not measure again by itself");
    }
}
