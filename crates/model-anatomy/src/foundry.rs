//! The Foundry (PR 3 of ADR-0044), computed by the C++ core, and the native
//! machine probe.
//!
//! - [`shape`] is `footprint.shape_from_runtime_payload(payload, model=...)`.
//! - [`foundry`] is `foundry.catalog_from_runtime(names, show, reading)` (each
//!   row's verdict at `CONTEXT_LADDER[1]`, its derived notes), then
//!   `runnable_here` and `coresident_pairs` at the chosen context, with
//!   `fit.assess`, `footprint.footprint_at` and `max_context_for` underneath and
//!   `fit.serving_reserve_fraction()` read from `ALELYON_HF_MEM_FRACTION` as
//!   `local_hf` reads it.
//! - [`describe`] is `workstation.describe(reading)`.
//! - [`probe`] reads this machine without opening any device: DXGI enumeration
//!   for the accelerator chosen by `VK_LOADER_DEVICE_ID_FILTER`, Windows for
//!   memory, processors, disk and version (deviation D10).
//!
//! The deviations (D9-D12) are named in the crate README.

use serde::Deserialize;

use super::{Error, Lowered, Payload, ffi, lower, take};

/// The variable `local_hf` reads its process cap from.
pub const MEM_FRACTION_VAR: &str = "ALELYON_HF_MEM_FRACTION";
/// The variable that confines this machine's Vulkan work to one adapter, read
/// by the probe to choose it.
pub const DEVICE_FILTER_VAR: &str = "VK_LOADER_DEVICE_ID_FILTER";
/// `fit.CONTEXT_LADDER`.
pub const CONTEXT_LADDER: [i64; 4] = [2048, 8192, 32768, 131072];
/// `workstation.GIB`.
pub const GIB: i64 = 1 << 30;

fn parse<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))
}

fn out_call(
    f: impl FnOnce(*mut *mut std::os::raw::c_char, *mut usize) -> i32,
) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    let code = f(&mut out, &mut len);
    take(code, out, len)
}

// ── the machine reading ─────────────────────────────────────────────────────

/// The primary accelerator as a reading holds it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Gpu {
    pub name: String,
    pub total_bytes: Option<i64>,
    pub free_bytes: Option<i64>,
    /// The probe's: PCI ids as DXGI reported them (absent in a Python reading).
    #[serde(default)]
    pub vendor_id: Option<u32>,
    #[serde(default)]
    pub device_id: Option<u32>,
}

/// One adapter DXGI enumerated.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub dedicated_bytes: i64,
    pub software: bool,
}

/// `workstation.WorkstationReading` (its primary device only), plus what the
/// native probe adds: its name, the adapters it saw, the filter and the disk
/// root it read.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    /// "dxgi" for the native probe; empty for a reading built in Python.
    pub probe: String,
    /// "cuda", "rocm", "mps", "cpu", "absent", or the probe's "vulkan" (D10).
    pub backend: String,
    pub torch_version: String,
    pub gpu: Option<Gpu>,
    pub ram_total_bytes: Option<i64>,
    pub ram_available_bytes: Option<i64>,
    pub cpu_logical: Option<i64>,
    pub cpu_physical: Option<i64>,
    pub disk_free_bytes: Option<i64>,
    pub disk_root: Option<String>,
    pub bf16: Option<bool>,
    pub platform: String,
    pub device_filter: Option<String>,
    pub adapters: Vec<Adapter>,
    pub gaps: Vec<String>,
}

impl Reading {
    /// `has_accelerator`, with "vulkan" counted (D10).
    pub fn has_accelerator(&self) -> bool {
        matches!(self.backend.as_str(), "cuda" | "rocm" | "mps" | "vulkan")
    }
}

/// Lowers `reading` for the duration of `f`.
fn with_reading<R>(reading: &Reading, f: impl FnOnce(*const ffi::MaReading) -> R) -> R {
    let gaps: Vec<ffi::MaStr> = reading.gaps.iter().map(|g| ffi::MaStr::of(g)).collect();
    let gpu = reading.gpu.as_ref();
    let raw = ffi::MaReading {
        backend: ffi::MaStr::of(&reading.backend),
        torch_version: ffi::MaStr::of(&reading.torch_version),
        probe: ffi::MaStr::of(&reading.probe),
        has_gpu: u32::from(gpu.is_some()),
        gpu_name: ffi::MaStr::of(gpu.map_or("", |g| g.name.as_str())),
        gpu_total_bytes: ffi::MaOptI64::of(gpu.and_then(|g| g.total_bytes)),
        gpu_free_bytes: ffi::MaOptI64::of(gpu.and_then(|g| g.free_bytes)),
        ram_total_bytes: ffi::MaOptI64::of(reading.ram_total_bytes),
        disk_free_bytes: ffi::MaOptI64::of(reading.disk_free_bytes),
        bf16: match reading.bf16 {
            Some(false) => 0,
            Some(true) => 1,
            None => 2,
        },
        gaps: gaps.as_ptr(),
        gaps_len: gaps.len(),
    };
    // Every pointer in `raw` borrows from `reading` or `gaps`, alive until `f` returns.
    f(&raw)
}

/// The native probe's reading as JSON, given the filter's text and the disk
/// root (each `None`: absent).
pub fn probe_json_with(
    device_filter: Option<&str>,
    disk_root: Option<&str>,
) -> Result<String, Error> {
    let filter = ffi::MaOptStr::of(device_filter);
    let root = ffi::MaOptStr::of(disk_root);
    // SAFETY: both strings outlive the call; `out`/`len` are valid for writes.
    out_call(|out, len| unsafe { ffi::ma_probe(filter, root, out, len) })
}

/// [`probe_json_with`], deserialised.
pub fn probe_with(device_filter: Option<&str>, disk_root: Option<&str>) -> Result<Reading, Error> {
    parse(&probe_json_with(device_filter, disk_root)?)
}

/// This machine, read by the native probe: `VK_LOADER_DEVICE_ID_FILTER` chooses
/// the adapter, and free disk is read at the root of the home folder (as
/// Python's `Path.home().anchor`).
pub fn probe() -> Result<Reading, Error> {
    let filter = std::env::var_os(DEVICE_FILTER_VAR).map(|v| v.to_string_lossy().into_owned());
    let root = home_anchor();
    probe_with(filter.as_deref(), root.as_deref())
}

/// `str(Path.home().anchor or Path.home())` on Windows: USERPROFILE, else
/// HOMEDRIVE + HOMEPATH; the drive and root of it, or the folder itself when it
/// has none. `None` when no home folder is named.
pub fn home_anchor() -> Option<String> {
    let home = match std::env::var_os("USERPROFILE") {
        Some(profile) => profile.to_string_lossy().into_owned(),
        None => {
            let path = std::env::var_os("HOMEPATH")?;
            let drive = std::env::var_os("HOMEDRIVE").unwrap_or_default();
            format!("{}{}", drive.to_string_lossy(), path.to_string_lossy())
        }
    };
    if home.is_empty() {
        return None;
    }
    let mut anchor = std::path::PathBuf::new();
    for component in std::path::Path::new(&home).components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                anchor.push(component)
            }
            _ => break,
        }
    }
    let anchor = anchor.to_string_lossy().into_owned();
    Some(if anchor.is_empty() { home } else { anchor })
}

/// `ALELYON_HF_MEM_FRACTION`'s text, `None` when unset. Text Windows holds that
/// is not Unicode is passed as U+FFFD, which `float()` refuses as it refuses the
/// lone surrogates Python would hold.
pub fn mem_fraction_from_env() -> Option<String> {
    std::env::var_os(MEM_FRACTION_VAR).map(|v| match v.to_str() {
        Some(text) => text.to_owned(),
        None => "\u{fffd}".to_owned(),
    })
}

/// `workstation.describe(reading)` and the reading's gaps.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Description {
    /// (label, value, provenance).
    pub rows: Vec<(String, String, String)>,
    pub gaps: Vec<String>,
}

/// [`describe`] as the core's JSON.
pub fn describe_json(reading: &Reading) -> Result<String, Error> {
    with_reading(reading, |raw| {
        // SAFETY: `raw` and what it points into outlive the call.
        out_call(|out, len| unsafe { ffi::ma_describe(raw, out, len) })
    })
}

/// `workstation.describe(reading)`.
pub fn describe(reading: &Reading) -> Result<Description, Error> {
    parse(&describe_json(reading)?)
}

// ── shapes ──────────────────────────────────────────────────────────────────

/// `footprint.ModelShape`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub model: String,
    pub layers: Option<i64>,
    pub kv_heads: Option<i64>,
    pub key_length: Option<i64>,
    pub value_length: Option<i64>,
    pub context_max: Option<i64>,
    pub weight_bytes: Option<i64>,
    pub weight_provenance: String,
    pub parameters: Option<i64>,
    pub quantization: String,
    pub architecture: String,
    pub mixture_of_experts: Option<bool>,
    pub active_parameters: Option<i64>,
    pub requires_backends: Vec<String>,
    pub gaps: Vec<String>,
    pub kv_established: bool,
    pub kv_bytes_per_token: Option<i64>,
    pub native: ShapeNative,
}

/// What only the port says about a shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShapeNative {
    /// Empty, or `INTEGER_OUT_OF_RANGE` (D9).
    pub refusal: String,
}

/// [`shape`] as the core's JSON.
pub fn shape_json(payload: Option<&Payload>, model: &str) -> Result<String, Error> {
    let lowered = payload.map(lower);
    let raw = lowered
        .as_ref()
        .map_or(std::ptr::null(), |l| &l.raw as *const ffi::MaPayload);
    // SAFETY: `model` and `lowered` (with the payload it borrows) outlive the call.
    out_call(|out, len| unsafe { ffi::ma_shape(ffi::MaStr::of(model), raw, out, len) })
}

/// `shape_from_runtime_payload(payload, model=model)`; `None` is a payload that
/// is not a mapping.
pub fn shape(payload: Option<&Payload>, model: &str) -> Result<Shape, Error> {
    parse(&shape_json(payload, model)?)
}

// ── the Foundry ─────────────────────────────────────────────────────────────

/// One model a runtime lists: `show(name)`'s payload, or the exception class it
/// raised, and a declared backend restriction (none, from a runtime).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoundryModel {
    pub name: String,
    pub payload: Option<Payload>,
    pub raised: Option<String>,
    pub requires_backends: Vec<String>,
}

/// `catalog_from_runtime`, `runnable_here` and `coresident_pairs` as the core's
/// JSON. `mem_fraction` is `ALELYON_HF_MEM_FRACTION`'s text (`None`: unset).
pub fn foundry_json(
    reading: &Reading,
    models: &[FoundryModel],
    context: i64,
    mem_fraction: Option<&str>,
    source: &str,
    as_of: &str,
) -> Result<String, Error> {
    let lowered: Vec<Option<Lowered>> = models
        .iter()
        .map(|m| m.payload.as_ref().map(lower))
        .collect();
    let backends: Vec<Vec<ffi::MaStr>> = models
        .iter()
        .map(|m| {
            m.requires_backends
                .iter()
                .map(|b| ffi::MaStr::of(b))
                .collect()
        })
        .collect();
    let raw: Vec<ffi::MaFoundryModel> = models
        .iter()
        .zip(&lowered)
        .zip(&backends)
        .map(|((model, lowered), backends)| ffi::MaFoundryModel {
            name: ffi::MaStr::of(&model.name),
            payload: lowered
                .as_ref()
                .map_or(std::ptr::null(), |l| &l.raw as *const ffi::MaPayload),
            raised: ffi::MaOptStr::of(model.raised.as_deref()),
            requires_backends: backends.as_ptr(),
            requires_backends_len: backends.len(),
        })
        .collect();
    with_reading(reading, |reading| {
        // SAFETY: `raw` points into `models`, `lowered` and `backends`, which with
        // `reading`, `source` and `as_of` outlive the call.
        out_call(|out, len| unsafe {
            ffi::ma_foundry(
                reading,
                raw.as_ptr(),
                raw.len(),
                context,
                ffi::MaOptStr::of(mem_fraction),
                ffi::MaStr::of(source),
                ffi::MaStr::of(as_of),
                out,
                len,
            )
        })
    })
}

/// [`foundry_json`], deserialised.
pub fn foundry(
    reading: &Reading,
    models: &[FoundryModel],
    context: i64,
    mem_fraction: Option<&str>,
    source: &str,
    as_of: &str,
) -> Result<Foundry, Error> {
    parse(&foundry_json(
        reading,
        models,
        context,
        mem_fraction,
        source,
        as_of,
    )?)
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Foundry {
    pub reserve: Reserve,
    pub context: i64,
    pub catalog: Vec<CatalogRow>,
    /// `runnable_here`'s order: best-fitting first.
    pub runnable: Vec<Runnable>,
    /// `coresident_pairs`' order: most spare first, undecidable last.
    pub pairs: Vec<Pairing>,
    pub native: FoundryNative,
}

/// `fit.serving_reserve_fraction()`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reserve {
    pub fraction: f64,
    pub provenance: String,
}

/// What only the port says about a Foundry reading.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FoundryNative {
    /// Empty, or `INTEGER_OUT_OF_RANGE` (D9): then nothing else is given.
    pub refusal: String,
    pub gaps: Vec<String>,
    /// Where the reserve came from, in words.
    pub reserve_basis: String,
}

/// `foundry.CatalogModel`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CatalogRow {
    pub id: String,
    pub label: String,
    pub pull_ref: String,
    pub license: String,
    pub roles: Vec<String>,
    pub source: String,
    pub as_of: String,
    pub provenance: String,
    pub installed: bool,
    pub shape: Shape,
    /// `assess` at `CONTEXT_LADDER[1]`, which the notes were derived from.
    pub verdict: Option<Fit>,
    pub notes: Vec<Note>,
}

/// `foundry.CapabilityNote`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub claim: String,
    pub kind: String,
    pub provenance: String,
    pub source: String,
    pub as_of: String,
    pub sourced: bool,
}

/// One of `runnable_here`'s pairs: the catalog row (by index) and its fit.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Runnable {
    pub index: usize,
    pub id: String,
    pub fit: Fit,
}

/// `fit.Fit`, with its derived properties.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fit {
    pub verdict: String,
    pub fits: Option<bool>,
    pub budget_bytes: Option<i64>,
    pub budget_basis: String,
    pub budget_provenance: String,
    /// None is UNMEASURED; 0 means the weights alone do not fit.
    pub max_context: Option<i64>,
    pub headroom_bytes: Option<i64>,
    pub reasons: Vec<String>,
    pub footprint: Footprint,
}

/// `footprint.Footprint`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    pub context: i64,
    pub kv_precision: String,
    pub weights: Term,
    pub kv_cache: Term,
    /// Never present: no ported caller declares a margin (D11).
    pub margin: Option<Term>,
    pub established_bytes: Option<i64>,
    pub total_bytes: Option<i64>,
    pub missing_terms: Vec<String>,
}

/// `footprint.Term`, with `gib` and `rendered()`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub label: String,
    pub bytes: Option<i64>,
    pub gib: Option<f64>,
    pub provenance: String,
    pub rule: String,
    pub rendered: String,
}

/// `foundry.Pairing`, its models by catalog index.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pairing {
    pub left: usize,
    pub left_id: String,
    pub right: usize,
    pub right_id: String,
    pub context: i64,
    pub combined_bytes: Option<i64>,
    pub budget_bytes: Option<i64>,
    /// None when a footprint or the budget is UNMEASURED.
    pub fits_together: Option<bool>,
    pub spare_bytes: Option<i64>,
}
