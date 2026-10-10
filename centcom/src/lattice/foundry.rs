//! The Lattice page's Foundry tab: what this workstation can run, and what runs alongside what, as the Python
//! desktop's Model Foundry view shows it, computed by
//! the C++ model-anatomy engine (footprint, fit, foundry and workstation description, held to Python's answers by
//! parity goldens).
//!
//! The machine is read by the engine's native probe, never by opening a device: the accelerator is the adapter
//! `VK_LOADER_DEVICE_ID_FILTER` names, found by DXGI enumeration (the RX on this computer); with the variable unset
//! or matching nothing its memory is UNMEASURED, never the first adapter's. The models are the installed GGUF files
//! (`ALELYON_MODELS_DIR`, else `~/.alelyon/models`), each read from its header only, as the Morphometry tab reads
//! them. The serving reserve is `ALELYON_HF_MEM_FRACTION`'s, read as the Python loader reads it.
//!
//! As in the Python view, nothing is measured until the tab is first shown, the reading is then kept, and a new
//! context or Re-measure takes it again, off the window's thread. Every figure carries how it was established
//! (OBSERVED, DERIVED, DECLARED or UNMEASURED), written as words, never as a colour alone.

use std::sync::Arc;

use iced::widget::{Column, column, container, pick_list, row, scrollable, space};
use iced::{Alignment, Color, Element, Length, Task, padding};

use crate::theme::{self, fonts};
use lattice_core::llama::{files, gguf};
use model_anatomy::{Description, Fit, Foundry, FoundryModel, Pairing, Reading};

use super::morphometry::grouped;
use crate::ui::{self, chip, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

/// `CONTEXT_CHOICES`: the contexts the whole machine can be evaluated at.
pub const CONTEXT_CHOICES: [(&str, i64); 5] = [
    ("2K — a single question", 2048),
    ("4K — a short exchange", 4096),
    ("8K — a working thread", 8192),
    ("32K — a long agent run", 32768),
    ("128K — a whole repository", 131072),
];
/// `DEFAULT_CONTEXT_INDEX`.
pub const DEFAULT_CONTEXT_INDEX: usize = 1;

/// The Python view lists the first 24 pairs.
const PAIRS_SHOWN: usize = 24;

/// A context choice, by its index in [`CONTEXT_CHOICES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextChoice(pub usize);

impl std::fmt::Display for ContextChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(CONTEXT_CHOICES[self.0.min(CONTEXT_CHOICES.len() - 1)].0)
    }
}

/// Everything one measurement produced.
#[derive(Clone, Debug)]
pub struct Measured {
    pub context: i64,
    pub reading: Reading,
    pub description: Result<Description, String>,
    /// Where the models were looked for.
    pub dir: String,
    pub foundry: Result<Foundry, String>,
}

pub struct State {
    pub context: usize,
    pub measuring: bool,
    pub measured: Option<Arc<Result<Measured, String>>>,
    measured_once: bool,
    /// The model being benchmarked, while one is, and each model's last benchmark.
    pub benchmarking: Option<String>,
    pub benchmarks: std::collections::BTreeMap<String, Arc<Result<Bench, String>>>,
    /// The cost inputs, as typed: none has a default (economics.py: a default is a fabricated saving).
    pub costs: Costs,
    /// The record kept between runs was read once (foundry_store), and the last save's or read's problem, if any.
    kept_read: bool,
    pub kept_problem: Option<String>,
}

/// One benchmark, as kept: the server's own rates (economics.throughput_from_llamacpp, the C++ port), the CPU
/// package's energy over the generation itself, when it was measured, and the model file it measured.
#[derive(Clone, Debug)]
pub struct Bench {
    pub throughput: model_anatomy::ThroughputReading,
    pub prompt_tokens: u64,
    /// The model's load on the wall clock (never part of a rate).
    pub load_seconds: f64,
    /// How llama.cpp was told to place the model: "--fit on" or "-ngl 999".
    pub placement: String,
    /// The CPU package's joules and average watts over the generation request, from Windows' RAPL energy counter.
    pub cpu_joules: Option<f64>,
    pub cpu_watts: Option<f64>,
    /// When it was measured (seconds since 1970), and the measured file's size and modification time.
    pub measured_at: f64,
    pub model_bytes: u64,
    pub model_modified: Option<f64>,
    /// The file under that name is no longer the one measured: shown, never priced.
    pub stale: bool,
}

/// What the person states for a cost comparison. Text as typed; read when compared.
#[derive(Clone, Debug, Default)]
pub struct Costs {
    pub gpu_watts: String,
    pub gpu_source: String,
    pub usd_per_kwh: String,
    pub tariff_source: String,
    pub tariff_as_of: String,
    pub input_tokens: String,
    pub output_tokens: String,
    pub hosted: Vec<Hosted>,
    pub draft: Hosted,
}

/// A hosted model's published price, as the person declares it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hosted {
    pub provider: String,
    pub model: String,
    pub input: String,
    pub output: String,
    pub source: String,
    pub as_of: String,
}

/// Which cost input a keystroke goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    GpuWatts,
    GpuSource,
    UsdPerKwh,
    TariffSource,
    TariffAsOf,
    InputTokens,
    OutputTokens,
    Provider,
    HostedModel,
    HostedInput,
    HostedOutput,
    HostedSource,
    HostedAsOf,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Context(ContextChoice),
    Remeasure,
    Measured(Arc<Result<Measured, String>>),
    Benchmark(String),
    Benchmarked(String, Arc<Result<Bench, String>>),
    Cost(Field, String),
    AddHosted,
    RemoveHosted(usize),
    Kept(Arc<Result<super::foundry_store::Kept, String>>),
    Saved(Result<(), String>),
}

impl Default for State {
    fn default() -> State {
        State {
            context: DEFAULT_CONTEXT_INDEX,
            measuring: false,
            measured: None,
            measured_once: false,
            benchmarking: None,
            benchmarks: Default::default(),
            costs: Costs::default(),
            kept_read: false,
            kept_problem: None,
        }
    }
}

impl State {
    pub fn busy(&self) -> bool {
        self.measuring || self.benchmarking.is_some()
    }

    pub fn context(&self) -> i64 {
        CONTEXT_CHOICES[self.context.min(CONTEXT_CHOICES.len() - 1)].1
    }

    /// The tab opened: measure the first time only (Re-measure is the explicit way to take it again).
    pub fn open(&mut self) -> Task<Msg> {
        let mut tasks = Vec::new();
        if !self.kept_read {
            self.kept_read = true;
            tasks.push(Task::perform(
                super::off_thread(|| super::foundry_store::load(&lattice_core::state::resolve())),
                |r| Msg::Kept(Arc::new(r.unwrap_or_else(|| Err("the kept record was not read".to_string())))),
            ));
        }
        if !self.measured_once {
            tasks.push(self.measure());
        }
        Task::batch(tasks)
    }

    /// Save the declarations and the benchmarks, off the window's thread.
    fn save(&self) -> Task<Msg> {
        let kept = super::foundry_store::Kept {
            costs: Costs { draft: Hosted::default(), ..self.costs.clone() },
            benchmarks: self
                .benchmarks
                .iter()
                .filter_map(|(name, b)| b.as_ref().as_ref().ok().map(|b| (name.clone(), b.clone())))
                .collect(),
        };
        Task::perform(
            super::off_thread(move || super::foundry_store::save(&lattice_core::state::resolve(), &kept)),
            |r| Msg::Saved(r.unwrap_or_else(|| Err("the save stopped before answering".to_string()))),
        )
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Context(choice) => {
                if choice.0 < CONTEXT_CHOICES.len() && choice.0 != self.context {
                    self.context = choice.0;
                    // The Python view re-measures on a new context: the question changed.
                    return self.measure();
                }
            }
            Msg::Remeasure => return self.measure(),
            Msg::Measured(result) => {
                self.measuring = false;
                self.measured = Some(result);
            }
            Msg::Benchmark(name) => {
                // One at a time, and only because a person pressed the button (bench.py: never automatic).
                if self.benchmarking.is_some() {
                    return Task::none();
                }
                self.benchmarking = Some(name.clone());
                let task = name.clone();
                return Task::perform(super::off_thread(move || benchmark(&task)), move |r| {
                    Msg::Benchmarked(name.clone(), Arc::new(r.unwrap_or_else(|| Err("the benchmark stopped before answering".to_string()))))
                });
            }
            Msg::Benchmarked(name, result) => {
                self.benchmarking = None;
                let measured = result.is_ok();
                self.benchmarks.insert(name, result);
                if measured {
                    return self.save();
                }
            }
            Msg::Kept(result) => match &*result {
                Ok(kept) => {
                    // What this run already holds wins over what was kept (a benchmark taken before the read).
                    for (name, bench) in &kept.benchmarks {
                        self.benchmarks.entry(name.clone()).or_insert_with(|| Arc::new(Ok(bench.clone())));
                    }
                    if self.costs.gpu_watts.is_empty() && self.costs.usd_per_kwh.is_empty() && self.costs.hosted.is_empty() {
                        self.costs = Costs { draft: std::mem::take(&mut self.costs.draft), ..kept.costs.clone() };
                    }
                }
                Err(why) => self.kept_problem = Some(why.clone()),
            },
            Msg::Saved(result) => self.kept_problem = result.err(),
            Msg::Cost(field, text) => {
                let c = &mut self.costs;
                let slot = match field {
                    Field::GpuWatts => &mut c.gpu_watts,
                    Field::GpuSource => &mut c.gpu_source,
                    Field::UsdPerKwh => &mut c.usd_per_kwh,
                    Field::TariffSource => &mut c.tariff_source,
                    Field::TariffAsOf => &mut c.tariff_as_of,
                    Field::InputTokens => &mut c.input_tokens,
                    Field::OutputTokens => &mut c.output_tokens,
                    Field::Provider => &mut c.draft.provider,
                    Field::HostedModel => &mut c.draft.model,
                    Field::HostedInput => &mut c.draft.input,
                    Field::HostedOutput => &mut c.draft.output,
                    Field::HostedSource => &mut c.draft.source,
                    Field::HostedAsOf => &mut c.draft.as_of,
                };
                *slot = text;
                // The draft hosted price is kept only once it is added.
                if !matches!(
                    field,
                    Field::Provider | Field::HostedModel | Field::HostedInput | Field::HostedOutput | Field::HostedSource | Field::HostedAsOf
                ) {
                    return self.save();
                }
            }
            Msg::AddHosted => {
                let draft = std::mem::take(&mut self.costs.draft);
                if !draft.provider.trim().is_empty() && !draft.model.trim().is_empty() {
                    self.costs.hosted.push(draft);
                    return self.save();
                } else {
                    self.costs.draft = draft;
                }
            }
            Msg::RemoveHosted(index) => {
                if index < self.costs.hosted.len() {
                    self.costs.hosted.remove(index);
                    return self.save();
                }
            }
        }
        Task::none()
    }

    /// One at a time; a second request while one runs is ignored, as in the Python view.
    fn measure(&mut self) -> Task<Msg> {
        if self.measuring {
            return Task::none();
        }
        self.measuring = true;
        self.measured_once = true;
        let context = self.context();
        Task::perform(super::off_thread(move || measure(context)), |r| {
            Msg::Measured(Arc::new(r.unwrap_or_else(|| Err("the measurement stopped before answering".to_string()))))
        })
    }
}

// ------------------------------------------------------------------ the work, off the window's thread

/// The machine read by the native probe, the installed models' headers, and the engine's Foundry at `context`.
pub fn measure(context: i64) -> Result<Measured, String> {
    let reading = model_anatomy::probe().map_err(|e| format!("The machine probe failed: {e}"))?;
    let paths = files::LlamaPaths::from_env(&lattice_core::ProcessEnv);
    let models: Vec<FoundryModel> = files::list_models(&paths.models_dir).iter().map(model_of).collect();
    let dir = paths.models_dir.display().to_string();
    Ok(evaluate(reading, &models, context, dir, model_anatomy::mem_fraction_from_env().as_deref()))
}

/// The CPU package's cumulative energy now, in picowatt-hours (Windows' RAPL counter, through the engine's probe).
fn cpu_energy() -> Option<i64> {
    model_anatomy::cpu_package_energy().ok()?.energy().map(|e| e.picowatt_hours)
}

/// Benchmark the installed model `name` (llama::bench, its own `--fit on` server), the CPU package's energy read
/// around the generation, and the rates read from the server's answer by the engine's `throughput_from_llamacpp`.
pub fn benchmark(name: &str) -> Result<Bench, String> {
    use lattice_core::llama::bench::{self, BenchRequest, Meter};
    let paths = files::LlamaPaths::from_env(&lattice_core::ProcessEnv);
    let model = files::list_models(&paths.models_dir)
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("{name} is no longer among the GGUF files in {}.", paths.models_dir.display()))?;
    let env: Arc<dyn lattice_core::Env> = Arc::new(lattice_core::ProcessEnv);
    let state = lattice_core::state::resolve();
    let stamp = super::foundry_store::file_stamp(&model.path);
    let mut request = BenchRequest::new(model);
    request.meter = Some(Meter(Arc::new(cpu_energy)));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("The benchmark's runtime could not start: {e}"))?;
    let reading = runtime.block_on(async {
        let config = lattice_core::llama::server::RuntimeConfig::new(env, &state).map_err(|e| format!("{e:?}"))?;
        bench::measure(&config, request).await.map_err(|e| e.to_string())
    })?;
    let response = llamacpp_response(&reading.payload);
    let parsed = model_anatomy::throughput_from_llamacpp(Some(&response), &reading.model, reading.context as i64)
        .map_err(|e| format!("The server's answer could not be read as rates: {e}"))?;
    // bench.throughput_of: the reading's own conditions replace the generic method.
    let throughput = model_anatomy::ThroughputReading { method: reading.method(), ..parsed };
    let seconds = reading.request.as_secs_f64();
    let cpu_joules = reading.metered.map(|(before, after)| (after - before) as f64 * 3.6e-9).filter(|j| *j >= 0.0);
    let cpu_watts = cpu_joules.filter(|_| seconds > 0.0).map(|j| j / seconds);
    let (model_bytes, model_modified) = stamp.unwrap_or((0, None));
    Ok(Bench {
        throughput,
        prompt_tokens: reading.timings.map_or(0, |t| t.prompt_tokens),
        load_seconds: reading.load.as_secs_f64(),
        placement: match reading.placement {
            bench::Placement::Fit => "--fit on".to_string(),
            bench::Placement::AllLayers => "-ngl 999".to_string(),
        },
        cpu_joules,
        cpu_watts,
        measured_at: crate::utc::now(),
        model_bytes,
        model_modified,
        stale: false,
    })
}

/// The server's JSON answer as the engine's `LlamacppResponse` (its `model` and `timings` values, each None, bool,
/// int, float or text; anything else is None, as Python's isinstance checks read it).
fn llamacpp_response(payload: &serde_json::Value) -> model_anatomy::LlamacppResponse {
    use model_anatomy::MetaValue;
    use serde_json::Value;
    let value = |v: &Value| match v {
        Value::Bool(b) => MetaValue::Bool(*b),
        Value::Number(n) if n.is_i64() || n.is_u64() => MetaValue::Int(n.to_string()),
        Value::Number(n) => n.as_f64().map_or(MetaValue::None, MetaValue::F64),
        Value::String(s) => MetaValue::Str(s.clone()),
        _ => MetaValue::None,
    };
    model_anatomy::LlamacppResponse {
        model: payload.get("model").map(value),
        timings: payload.get("timings").and_then(Value::as_object).map(|t| t.iter().map(|(k, v)| (k.clone(), value(v))).collect()),
    }
}

/// The engine's description of `reading` and its Foundry over `models`.
pub fn evaluate(reading: Reading, models: &[FoundryModel], context: i64, dir: String, mem_fraction: Option<&str>) -> Measured {
    let source = format!("the GGUF headers in {dir}, read when this tab measured");
    let failed = |e: model_anatomy::Error| e.to_string();
    Measured {
        context,
        description: model_anatomy::describe(&reading).map_err(failed),
        foundry: model_anatomy::foundry(&reading, models, context, mem_fraction, &source, "").map_err(failed),
        reading,
        dir,
    }
}

/// One installed model as `show(name)` answers: its header's payload, or what stopped it (which the engine names as
/// the gap `catalog_from_runtime` writes for a model the runtime did not describe).
fn model_of(model: &files::LocalModel) -> FoundryModel {
    let mut out = FoundryModel { name: model.name.clone(), ..FoundryModel::default() };
    match gguf::read_header(&model.path) {
        Err(e) => out.raised = Some(format!("its GGUF header did not read: {}", e.kind())),
        Ok(header) => match super::morphometry::payload_of(&header, &model.name) {
            Ok(payload) => out.payload = Some(payload),
            Err(e) => out.raised = Some(e.kind().to_string()),
        },
    }
    out
}

// ------------------------------------------------------------------ words, as the Python view writes them

/// `f"{bytes / GIB:.2f}"`.
pub fn gib(bytes: i64) -> String {
    format!("{:.2}", bytes as f64 / model_anatomy::GIB as f64)
}

/// The largest context that fits, as the Python view words it: zero is a real answer (the weights alone are over
/// budget), not "0 tokens".
pub fn reach(max_context: Option<i64>) -> String {
    match max_context {
        None => "UNMEASURED".to_string(),
        Some(0) => "none — the weights alone are over budget".to_string(),
        Some(n) => format!("{} tokens", grouped(n)),
    }
}

/// A pair's verdict and detail.
pub fn pair_words(pair: &Pairing) -> (&'static str, String) {
    match (pair.fits_together, pair.spare_bytes) {
        (Some(true), Some(spare)) => ("TOGETHER", format!("{} GiB spare", gib(spare))),
        (Some(false), Some(spare)) => ("NOT TOGETHER", format!("{} GiB short", gib(spare.saturating_abs()))),
        _ => ("UNDECIDABLE", "a footprint or the budget is UNMEASURED".to_string()),
    }
}

/// When nothing pairs, why (the Python view's closing sentence): the closest pair and whether a smaller context
/// would close its gap.
pub fn none_pair_note(foundry: &Foundry) -> Option<String> {
    let decided: Vec<&Pairing> = foundry.pairs.iter().filter(|p| p.fits_together.is_some()).collect();
    if decided.is_empty() || decided.iter().any(|p| p.fits_together == Some(true)) {
        return None;
    }
    // `max(..., key=spare)`: the first of the largest.
    let mut closest = decided[0];
    for pair in &decided[1..] {
        if pair.spare_bytes > closest.spare_bytes {
            closest = pair;
        }
    }
    let weight = |index: usize| foundry.catalog.get(index).and_then(|row| row.shape.weight_bytes).unwrap_or(0);
    let weights = weight(closest.left) + weight(closest.right);
    let helps = closest.budget_bytes.is_some_and(|budget| weights < budget);
    Some(format!(
        "No two of these models are resident at once in this budget. The closest pair is {} + {}, {} GiB short — and \
         their {}",
        closest.left_id,
        closest.right_id,
        gib(closest.spare_bytes.unwrap_or(0).saturating_abs()),
        if helps {
            "weights alone fit, so a smaller context would close the gap."
        } else {
            "weights alone already exceed the budget, so a smaller context will not."
        }
    ))
}

fn verdict_colour(verdict: &str) -> Color {
    match verdict {
        "RUNS" => theme::POSITIVE,
        "TIGHT" | "UNDECIDABLE" => theme::CAUTION,
        "EXCEEDS" => theme::TEXT_DIM,
        _ => theme::TEXT,
    }
}

/// A provenance word beside its value: a measurement in the text colour, anything else in caution, and the word
/// itself always written.
fn provenance_colour(provenance: &str) -> Color {
    if provenance == "OBSERVED" { theme::TEXT } else { theme::CAUTION }
}

// ------------------------------------------------------------------ the view

const PURPOSE: &str = "What this machine can run, at a chosen context, and which models are resident together. A fit \
                       is arithmetic over each model's declared shape (its GGUF header) against this machine's \
                       memory budget; no model is loaded and no device is opened to find out.";

pub fn view(state: &State, phase: f32) -> El<'_> {
    let mut col = Column::new().spacing(12).width(Length::Fill);
    col = col.push(note(PURPOSE));
    col = col.push(controls(state));
    if state.measuring {
        col = col.push(ui::working(phase, "Reading this machine and the installed models' headers…"));
    }
    match state.measured.as_deref() {
        None if !state.measuring => col = col.push(note("Nothing is measured until this tab is shown.")),
        None => {}
        Some(Err(why)) => col = col.push(ui::notice(why.as_str(), theme::CAUTION)),
        Some(Ok(m)) => {
            col = col.push(machine(m));
            col = col.push(models(m));
            col = col.push(benchmarks(state, m, phase));
            col = col.push(costs(state));
            col = col.push(pairs(m));
        }
    }
    col = col.push(footer());
    container(scrollable(col.padding(padding::right(12))).height(Length::Fill).style(theme::scrollbars)).height(Length::Fill).into()
}

fn controls(state: &State) -> El<'_> {
    let choices: Vec<ContextChoice> = (0..CONTEXT_CHOICES.len()).map(ContextChoice).collect();
    row![
        label("Evaluate at", 13.0, theme::TEXT_DIM),
        container(
            pick_list(choices, Some(ContextChoice(state.context)), Msg::Context)
                .padding(7)
                .text_size(13.0)
                .font(fonts().ui)
                .style(theme::picker)
                .menu_style(theme::picker_menu)
        )
        .width(Length::Fixed(260.0)),
        ui::primary("Re-measure", (!state.measuring).then_some(Msg::Remeasure)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn section<'a>(title: impl iced::widget::text::IntoFragment<'a>) -> El<'a> {
    strong(title, 14.0, theme::GOLD).into()
}

fn machine(m: &Measured) -> El<'_> {
    let mut col = Column::new().spacing(5).push(section("This machine"));
    match &m.description {
        Err(why) => col = col.push(ui::notice(format!("The machine could not be described: {why}"), theme::CAUTION)),
        Ok(d) => {
            for (name, value, provenance) in &d.rows {
                col = col.push(
                    row![
                        container(label(name.as_str(), 12.5, theme::TEXT_DIM)).width(230),
                        container(mono(value.as_str(), 12.5, provenance_colour(provenance))).width(260),
                        label(provenance.as_str(), 11.0, theme::TEXT_FAINT),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                );
            }
            for gap in &d.gaps {
                col = col.push(label(format!("• {gap}"), 11.5, theme::CAUTION));
            }
        }
    }
    let r = &m.reading;
    let mut probe = format!("Read by the native probe ({}): no device opened", r.probe);
    match &r.device_filter {
        Some(filter) => probe.push_str(&format!("; VK_LOADER_DEVICE_ID_FILTER = {filter}")),
        None => probe.push_str("; VK_LOADER_DEVICE_ID_FILTER unset"),
    }
    if let Some(gpu) = &r.gpu
        && let (Some(vendor), Some(device)) = (gpu.vendor_id, gpu.device_id)
    {
        probe.push_str(&format!(" (chose {vendor:04x}:{device:04x})"));
    }
    probe.push_str(&format!("; {} adapter(s) enumerated", r.adapters.len()));
    if !r.platform.is_empty() {
        probe.push_str(&format!("; {}", r.platform));
    }
    col = col.push(note(probe));
    col.into()
}

/// The one sentence that explains every verdict below it, and where the reserve came from.
fn budget(foundry: &Foundry) -> El<'_> {
    let mut col = Column::new().spacing(4);
    let first = foundry.runnable.first().map(|r| &r.fit);
    match first.and_then(|f| f.budget_bytes.map(|b| (b, f))) {
        None => {
            col = col.push(label(
                "The serving budget is UNMEASURED, so no verdict below is a refusal — they are undecided.",
                12.0,
                theme::CAUTION,
            ))
        }
        Some((bytes, fit)) => {
            col = col.push(
                row![
                    label("Budget:", 12.5, theme::TEXT_DIM),
                    strong(format!("{} GiB", gib(bytes)), 12.5, theme::TEXT),
                    label(format!("— {}. [{}]", fit.budget_basis, fit.budget_provenance), 12.0, theme::TEXT_DIM),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .wrap(),
            )
        }
    }
    col = col.push(note(format!(
        "Reserve {} [{}]: {}.",
        foundry.reserve.fraction, foundry.reserve.provenance, foundry.native.reserve_basis
    )));
    col.into()
}

fn models(m: &Measured) -> El<'_> {
    let mut col = Column::new().spacing(6).push(section("What this machine can run"));
    let foundry = match &m.foundry {
        Err(why) => return col.push(ui::notice(format!("The Foundry could not be computed: {why}"), theme::CAUTION)).into(),
        Ok(f) => f,
    };
    if !foundry.native.refusal.is_empty() {
        col = col.push(ui::notice(format!("Refused: {}", foundry.native.refusal), theme::CAUTION));
        for gap in &foundry.native.gaps {
            col = col.push(label(format!("• {gap}"), 11.5, theme::TEXT_DIM));
        }
        return col.into();
    }
    if foundry.catalog.is_empty() {
        return col
            .push(label(
                format!(
                    "No GGUF model is installed in {} (ALELYON_MODELS_DIR, else ~/.alelyon/models). Nothing here \
                     downloads one — that stays an explicit act with a visible size.",
                    m.dir
                ),
                12.5,
                theme::TEXT_DIM,
            ))
            .into();
    }
    col = col.push(budget(foundry));
    for runnable in &foundry.runnable {
        let fit: &Fit = &runnable.fit;
        let weights = &fit.footprint.weights;
        col = col.push(container(space().height(1)).width(Length::Fill).style(theme::well));
        col = col.push(
            row![
                container(strong(runnable.id.as_str(), 13.0, theme::TEXT).font(fonts().mono)).width(Length::FillPortion(3)),
                container(chip(fit.verdict.as_str(), verdict_colour(&fit.verdict))).width(Length::FillPortion(2)),
                container(label(format!("{} weights [{}]", weights.rendered, weights.provenance), 12.0, theme::TEXT_DIM))
                    .width(Length::FillPortion(3)),
                container(
                    row![
                        label("largest context that fits:", 12.0, theme::TEXT_DIM),
                        strong(reach(fit.max_context), 12.0, theme::TEXT)
                    ]
                    .spacing(4)
                )
                .width(Length::FillPortion(4)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
        if let Some(row) = foundry.catalog.get(runnable.index) {
            for n in &row.notes {
                col = col.push(label(format!("   • {} [{}]", n.claim, n.provenance), 11.5, theme::TEXT_DIM));
            }
        }
    }
    col = col.push(note(
        "EXCEEDS is a statement about the accelerator, not about usability: a runtime can still serve a model that \
         does not fit by spilling onto the CPU. UNDECIDABLE is not a refusal — it means a term of the footprint could \
         not be established.",
    ));
    col.into()
}

fn pairs(m: &Measured) -> El<'_> {
    let Ok(foundry) = &m.foundry else { return space().height(0).into() };
    if foundry.pairs.is_empty() {
        return space().height(0).into();
    }
    let mut col =
        Column::new().spacing(5).push(section(format!("What runs together, both resident at {} tokens", grouped(m.context))));
    for pair in foundry.pairs.iter().take(PAIRS_SHOWN) {
        let (verdict, detail) = pair_words(pair);
        let colour = match verdict {
            "TOGETHER" => theme::POSITIVE,
            "NOT TOGETHER" => theme::TEXT_DIM,
            _ => theme::CAUTION,
        };
        col = col.push(
            row![
                container(mono(format!("{} + {}", pair.left_id, pair.right_id), 12.0, theme::TEXT)).width(Length::FillPortion(3)),
                container(label(verdict, 12.0, colour)).width(Length::FillPortion(1)),
                label(detail, 12.0, theme::TEXT_DIM),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    if foundry.pairs.len() > PAIRS_SHOWN {
        col = col.push(note(format!("the first {PAIRS_SHOWN} of {} pairs", foundry.pairs.len())));
    }
    if let Some(words) = none_pair_note(foundry) {
        col = col.push(label(words, 12.0, theme::CAUTION));
    }
    col = col.push(note(
        "Co-residency, not sequential use — two models that cannot be held at once can still be run one after the \
         other, paying a load between them. Concurrency on one device is bounded by the KV cache rather than by the \
         number of models, which is why this answer moves with the context above it.",
    ));
    col.into()
}

/// A number as typed, or nothing (never a default).
fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn benchmarks<'a>(state: &'a State, m: &'a Measured, phase: f32) -> El<'a> {
    let mut col = Column::new().spacing(6).push(section("Benchmarks on this machine"));
    col = col.push(note(
        "One bounded generation (128 tokens of a domain-free prompt, 4K context) on a llama.cpp server started for it \
         with --fit on, so what does not fit the card stays on the CPU; stopped as soon as it answers. Rates are the \
         server's own counters, which exclude the load. It loads the model and takes the card for a minute or so, so it \
         runs only when you press Benchmark. One sample, not a property of the model.",
    ));
    let Ok(foundry) = &m.foundry else { return col.into() };
    if let Some(name) = &state.benchmarking {
        col = col.push(ui::working(phase, format!("Benchmarking {name}: loading it and generating 128 tokens…")));
    }
    for row in &foundry.catalog {
        let name = row.id.as_str();
        let run = ui::secondary("Benchmark", state.benchmarking.is_none().then(|| Msg::Benchmark(name.to_string())));
        let words: El<'_> = match state.benchmarks.get(name).map(|b| &**b) {
            None => label("not measured", 12.0, theme::TEXT_FAINT).into(),
            Some(Err(why)) => label(why.as_str(), 12.0, theme::CAUTION).into(),
            Some(Ok(b)) => {
                let rate = |v: Option<f64>| v.map_or("UNMEASURED".to_string(), |v| format!("{v:.1} tok/s"));
                let cpu = match (b.cpu_watts, b.cpu_joules) {
                    (Some(w), Some(j)) => format!("CPU package {w:.0} W ({j:.0} J) during the generation [OBSERVED]"),
                    _ => "CPU package energy UNMEASURED".to_string(),
                };
                let when = crate::utc::when(b.measured_at, crate::utc::now());
                let mut lines = column![
                    label(
                        format!(
                            "decode {} · prefill {} over {} prompt tokens [{}] · load {:.1} s · {} · measured {when}",
                            rate(b.throughput.decode_tokens_per_second),
                            rate(b.throughput.prefill_tokens_per_second),
                            b.prompt_tokens,
                            b.throughput.provenance,
                            b.load_seconds,
                            b.placement
                        ),
                        12.0,
                        if b.stale { theme::TEXT_FAINT } else { theme::TEXT }
                    ),
                    label(cpu, 11.5, theme::TEXT_DIM),
                ];
                if b.stale {
                    lines = lines.push(label(
                        "Stale: the model file under this name is not the one measured (another size or date, or gone). \
                         Benchmark it again; no cost is computed from this reading.",
                        11.5,
                        theme::CAUTION,
                    ));
                }
                lines
                .spacing(2)
                .into()
            }
        };
        col = col.push(
            row![container(mono(name, 12.5, theme::TEXT)).width(Length::FillPortion(2)), container(words).width(Length::FillPortion(5)), run]
                .spacing(10)
                .align_y(Alignment::Center),
        );
    }
    col.into()
}

fn field<'a>(placeholder: &'a str, value: &'a str, which: Field, width: f32) -> El<'a> {
    container(
        iced::widget::text_input(placeholder, value)
            .on_input(move |t| Msg::Cost(which, t))
            .padding(7)
            .size(12.5)
            .style(ui::input_style),
    )
    .width(Length::Fixed(width))
    .into()
}

fn costs(state: &State) -> El<'_> {
    use model_anatomy::{EnergyDeclaration, MonthlyVolume, ProviderPrice};
    let c = &state.costs;
    let mut col = Column::new().spacing(6).push(section("Running costs"));
    col = col.push(note(
        "A projection from four inputs, none of which has a default: a benchmark's measured rates, the electricity \
         (the GPU's board power as you declare it, plus the CPU package's power as measured during the benchmark, \
         at your tariff), a hosted model's published price, and the monthly volume you state. Missing any one refuses \
         the comparison by name.",
    ));
    col = col.push(
        row![
            field("GPU board power, W", &c.gpu_watts, Field::GpuWatts, 160.0),
            field("Where that figure is from", &c.gpu_source, Field::GpuSource, 240.0),
            field("Tariff, USD per kWh", &c.usd_per_kwh, Field::UsdPerKwh, 160.0),
            field("Tariff source", &c.tariff_source, Field::TariffSource, 200.0),
            field("As of (date)", &c.tariff_as_of, Field::TariffAsOf, 130.0),
        ]
        .spacing(6)
        .wrap(),
    );
    col = col.push(
        row![
            field("Input tokens per month", &c.input_tokens, Field::InputTokens, 200.0),
            field("Output tokens per month", &c.output_tokens, Field::OutputTokens, 200.0),
        ]
        .spacing(6),
    );
    col = col.push(strong("Hosted prices you declare", 12.5, theme::TEXT));
    for (i, h) in c.hosted.iter().enumerate() {
        col = col.push(
            row![
                label(
                    format!("{} {}: ${} in / ${} out per million · {} ({})", h.provider, h.model, h.input, h.output, h.source, h.as_of),
                    12.0,
                    theme::TEXT
                ),
                ui::secondary("Remove", Some(Msg::RemoveHosted(i))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    col = col.push(
        row![
            field("Provider", &c.draft.provider, Field::Provider, 130.0),
            field("Model", &c.draft.model, Field::HostedModel, 170.0),
            field("USD/M in", &c.draft.input, Field::HostedInput, 100.0),
            field("USD/M out", &c.draft.output, Field::HostedOutput, 100.0),
            field("Price list source", &c.draft.source, Field::HostedSource, 200.0),
            field("As of", &c.draft.as_of, Field::HostedAsOf, 110.0),
            ui::secondary("Add", Some(Msg::AddHosted)),
        ]
        .spacing(6)
        .wrap(),
    );
    if let Some(why) = &state.kept_problem {
        col = col.push(ui::notice(format!("What this tab keeps between runs: {why}"), theme::CAUTION));
    }
    let measured: Vec<(&String, &Bench)> = state
        .benchmarks
        .iter()
        .filter_map(|(n, b)| b.as_ref().as_ref().ok().filter(|b| !b.stale).map(|b| (n, b)))
        .collect();
    if measured.is_empty() {
        col = col.push(note("Benchmark a model above: a cost needs its measured rates."));
        return col.into();
    }
    let volume = MonthlyVolume {
        input_tokens: c.input_tokens.trim().parse().ok(),
        output_tokens: c.output_tokens.trim().parse().ok(),
        ..MonthlyVolume::default()
    };
    for (name, bench) in measured {
        let gpu = number(&c.gpu_watts);
        let draw = gpu.map(|g| g + bench.cpu_watts.unwrap_or(0.0));
        let source = match (gpu, bench.cpu_watts) {
            (Some(g), Some(w)) if !c.gpu_source.trim().is_empty() => {
                format!("GPU board power {g} W declared ({}) + CPU package {w:.1} W measured in the benchmark", c.gpu_source.trim())
            }
            (Some(g), None) if !c.gpu_source.trim().is_empty() => {
                format!("GPU board power {g} W declared ({}); CPU package UNMEASURED", c.gpu_source.trim())
            }
            _ => String::new(),
        };
        let energy = EnergyDeclaration {
            draw_watts: draw,
            usd_per_kwh: number(&c.usd_per_kwh),
            source: if c.tariff_source.trim().is_empty() || source.is_empty() {
                String::new()
            } else {
                format!("{source}; tariff: {}", c.tariff_source.trim())
            },
            as_of: c.tariff_as_of.trim().to_string(),
            ..EnergyDeclaration::default()
        };
        let prices: Vec<Option<ProviderPrice>> = if c.hosted.is_empty() {
            vec![None]
        } else {
            c.hosted
                .iter()
                .map(|h| {
                    Some(ProviderPrice {
                        usd_per_million_input: number(&h.input),
                        usd_per_million_output: number(&h.output),
                        source: h.source.trim().to_string(),
                        as_of: h.as_of.trim().to_string(),
                        ..ProviderPrice::new(h.provider.trim(), h.model.trim())
                    })
                })
                .collect()
        };
        for price in prices {
            let against = price.as_ref().map_or("no hosted price".to_string(), |p| format!("{} {}", p.provider, p.model));
            col = col.push(strong(format!("{name} against {against}"), 12.5, theme::GOLD));
            match model_anatomy::compare_costs(&bench.throughput, &energy, price.as_ref(), &volume) {
                Err(e) => col = col.push(ui::notice(format!("The engine refused the inputs: {e}"), theme::CAUTION)),
                Ok(cmp) => {
                    for refusal in &cmp.refusals {
                        col = col.push(label(format!("Refused: {refusal}"), 12.0, theme::CAUTION));
                    }
                    for line in &cmp.describe {
                        col = col.push(label(line.clone(), 12.0, theme::TEXT));
                    }
                    for caveat in &cmp.caveats {
                        col = col.push(label(format!("This comparison {caveat}"), 11.5, theme::TEXT_DIM));
                    }
                }
            }
        }
    }
    col.into()
}

fn footer() -> El<'static> {
    column![
        section("What this view does not establish"),
        note("• That a model will load. Fitting is arithmetic over declared shapes; the only proof of loading is loading."),
        note("• Anything about answer quality. Nothing here runs an evaluation, and a fit verdict is a memory statement."),
        note(
            "• A realised saving or a total cost of ownership: a cost here is a projection from a measured rate and \
             declared prices, without the hardware or your time, and not matched for quality."
        ),
        note(
            "• Anything about a model this machine does not hold. There is no bundled catalog: a table of model-card \
             numbers written from memory would be a fabrication wearing a provenance label."
        ),
    ]
    .spacing(4)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::morphometry::tests::{gguf_bytes, header};

    fn rx() -> Reading {
        Reading {
            probe: "dxgi".into(),
            backend: "vulkan".into(),
            gpu: Some(model_anatomy::Gpu {
                name: "AMD Radeon RX 9070 XT".into(),
                total_bytes: Some(17_095_983_104),
                free_bytes: None,
                vendor_id: Some(0x1002),
                device_id: Some(0x7550),
            }),
            ram_total_bytes: Some(64 << 30),
            ..Reading::default()
        }
    }

    /// A header with the attention dimensions declared, `blocks` deep, at `ggml_type` storage.
    fn model(name: &str, blocks: u32, width: u64) -> FoundryModel {
        let mut kv: Vec<(&str, Result<&str, u32>)> = vec![
            ("general.architecture", Ok("llama")),
            ("llama.block_count", Err(blocks)),
            ("llama.attention.head_count", Err(8)),
            ("llama.attention.head_count_kv", Err(2)),
            ("llama.attention.key_length", Err(128)),
            ("llama.context_length", Err(32768)),
            ("general.file_type", Err(1)),
        ];
        kv.push(("llama.embedding_length", Err(1024)));
        let mut tensors: Vec<(String, Vec<u64>, u32)> = vec![("token_embd.weight".into(), vec![width, 1000], 1)];
        for b in 0..blocks {
            tensors.push((format!("blk.{b}.attn_q.weight"), vec![width, width], 1));
            tensors.push((format!("blk.{b}.ffn_up.weight"), vec![width, width * 4], 1));
        }
        let borrowed: Vec<(&str, &[u64], u32)> = tensors.iter().map(|(n, d, t)| (n.as_str(), d.as_slice(), *t)).collect();
        let bytes = gguf_bytes(&kv, &borrowed);
        let payload = super::super::morphometry::payload_of(&header(&bytes), name).expect("a payload");
        FoundryModel { name: name.into(), payload: Some(payload), ..FoundryModel::default() }
    }

    #[test]
    fn the_engine_fits_installed_headers_against_the_rx_budget() {
        let models = [model("small", 4, 1024), model("wide", 8, 32768), FoundryModel {
            name: "unreadable".into(),
            raised: Some("its GGUF header did not read: Truncated".into()),
            ..FoundryModel::default()
        }];
        let m = evaluate(rx(), &models, 4096, "C:/models".into(), None);
        let foundry = m.foundry.as_ref().expect("a Foundry");
        assert_eq!(foundry.reserve.fraction, 0.15000000000000002, "local_hf's default cap, 0.85");
        let verdicts: Vec<(&str, &str)> =
            foundry.runnable.iter().map(|r| (r.id.as_str(), r.fit.verdict.as_str())).collect();
        assert_eq!(verdicts, [("small", "RUNS"), ("unreadable", "UNDECIDABLE"), ("wide", "EXCEEDS")]);
        let small = &foundry.runnable[0].fit;
        assert_eq!(small.footprint.weights.provenance, "OBSERVED", "summed over the header's inventory");
        assert_eq!(small.budget_bytes, Some((17_095_983_104f64 * (1.0 - 0.15000000000000002)) as i64));
        let unreadable = &foundry.catalog[2];
        assert!(unreadable.shape.gaps.iter().any(|g| g.contains("did not describe this model")));
        // Pairs: the readable two are decided (and do not fit), the unreadable one's are not.
        assert_eq!(foundry.pairs.len(), 3);
        assert_eq!(foundry.pairs.last().unwrap().fits_together, None, "undecidable pairs sort last");
        let rows = &m.description.as_ref().expect("described").rows;
        assert_eq!(rows[0], ("Compute backend".into(), "vulkan".into(), "OBSERVED".into()));
        assert_eq!(rows[3].2, "UNMEASURED", "free memory is never read");
    }

    #[test]
    fn without_the_filter_nothing_is_chosen_and_every_verdict_is_against_ram_or_undecided() {
        let reading = model_anatomy::probe_with(None, None).expect("a reading");
        assert!(reading.gpu.is_none() && !reading.has_accelerator());
        let m = evaluate(reading, &[model("small", 4, 1024)], 8192, "C:/models".into(), None);
        let fit = &m.foundry.as_ref().unwrap().runnable[0].fit;
        assert!(fit.budget_basis.contains("of system memory"), "{}", fit.budget_basis);
    }

    #[test]
    fn words_are_the_python_views() {
        assert_eq!(reach(None), "UNMEASURED");
        assert_eq!(reach(Some(0)), "none — the weights alone are over budget");
        assert_eq!(reach(Some(131072)), "131,072 tokens");
        assert_eq!(gib(17_095_983_104), "15.92");
        assert_eq!(ContextChoice(DEFAULT_CONTEXT_INDEX).to_string(), "4K — a short exchange");
        let pair = |fits, spare| Pairing {
            left: 0,
            left_id: "a".into(),
            right: 1,
            right_id: "b".into(),
            context: 4096,
            combined_bytes: Some(1),
            budget_bytes: Some(1),
            fits_together: fits,
            spare_bytes: spare,
        };
        assert_eq!(pair_words(&pair(Some(true), Some(1 << 30))), ("TOGETHER", "1.00 GiB spare".into()));
        assert_eq!(pair_words(&pair(Some(false), Some(-(3 << 29)))), ("NOT TOGETHER", "1.50 GiB short".into()));
        assert_eq!(pair_words(&pair(None, None)).0, "UNDECIDABLE");
    }

    #[test]
    fn the_servers_answer_reaches_the_engine_as_python_reads_it() {
        let answer = serde_json::json!({
            "model": "tiny",
            "timings": {"prompt_n": 12, "prompt_ms": 20.0, "predicted_n": 128, "predicted_ms": 1450, "extra": [1]},
        });
        let response = llamacpp_response(&answer);
        let t = model_anatomy::throughput_from_llamacpp(Some(&response), "", 4096).unwrap();
        assert_eq!(t.model, "tiny", "the answer names the model when the caller does not");
        assert_eq!(t.prefill_tokens_per_second, Some(600.0));
        assert!((t.decode_tokens_per_second.unwrap() - 128.0 / 1.45).abs() < 1e-9);
        assert!(t.complete());
        // A boolean count is no count, as Python's isinstance check reads it.
        let bad = llamacpp_response(&serde_json::json!({"timings": {"prompt_n": true, "prompt_ms": 5}}));
        let t = model_anatomy::throughput_from_llamacpp(Some(&bad), "x", 4096).unwrap();
        assert_eq!(t.prefill_tokens_per_second, None);
    }

    #[test]
    fn a_cost_with_nothing_stated_is_refused_by_name_and_never_defaulted() {
        let throughput = model_anatomy::ThroughputReading::new("tiny", 4096, Some(70.0), Some(90.0));
        let cmp = model_anatomy::compare_costs(
            &throughput,
            &model_anatomy::EnergyDeclaration::default(),
            None,
            &model_anatomy::MonthlyVolume::default(),
        )
        .unwrap();
        assert!(!cmp.established && !cmp.refusals.is_empty(), "{:?}", cmp.refusals);
        assert!(cmp.local_usd.is_none() && cmp.hosted_usd.is_none() && cmp.monthly_delta_usd.is_none());
        assert_eq!(number(" 0.31 "), Some(0.31));
        assert_eq!((number(""), number("inf"), number("cheap")), (None, None, None));
    }

    /// Run by hand: a real benchmark of the installed model `CENTCOM_BENCH_MODEL` names (ALELYON_LLAMA_SERVER names
    /// the server). It loads the model and takes the card.
    #[test]
    #[ignore = "loads a real model on this machine's card: run by hand with CENTCOM_BENCH_MODEL set"]
    fn an_installed_model_is_benchmarked_with_its_cpu_energy() {
        let name = std::env::var("CENTCOM_BENCH_MODEL").expect("CENTCOM_BENCH_MODEL");
        let b = benchmark(&name).unwrap_or_else(|why| panic!("{why}"));
        println!(
            "{}: decode {:?} prefill {:?} [{}] load {:.1} s over {} prompt tokens cpu {:?} J {:?} W",
            b.throughput.model,
            b.throughput.decode_tokens_per_second,
            b.throughput.prefill_tokens_per_second,
            b.throughput.provenance,
            b.load_seconds,
            b.prompt_tokens as f64,
            b.cpu_joules,
            b.cpu_watts
        );
        println!("{}", b.throughput.method);
        assert!(b.throughput.complete());
    }

    #[test]
    fn the_tab_measures_once_on_opening_and_a_new_context_measures_again() {
        let mut state = State::default();
        assert_eq!(state.context(), 4096);
        let _ = state.open();
        assert!(state.measuring && state.busy());
        let _ = state.update(Msg::Measured(Arc::new(Err("x".into()))));
        assert!(!state.measuring);
        let _ = state.open();
        assert!(!state.measuring, "opening again keeps the reading");
        let _ = state.update(Msg::Context(ContextChoice(4)));
        assert!(state.measuring);
        assert_eq!(state.context(), 131072);
        let _ = state.update(Msg::Remeasure);
        assert!(state.measuring, "a second request while one runs is ignored, not queued");
    }
}
