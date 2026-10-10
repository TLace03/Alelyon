//! Model morphometry's maths, computed by the C++ core in `cpp/`.
//!
//! A port of the Python reference's `morphometry.py`: [`analyze`] is
//! `morphometry.analyze(payload, model=...)` with every derived property of the
//! result, `voxel_field(result)` and the native space's commitment;
//! [`canonical_space`] is `canonical_space()`'s canonical bytes and
//! `space_commitment()`. [`gguf::show_payload`] is
//! `gguf_header.GGUFHeader.show_payload`, the payload a local GGUF model
//! answers with. The parity goldens (`tests/goldens/`, written by
//! `tools/model_anatomy_goldens.py` from the real Python) hold all of it to
//! Python's answers.
//!
//! PR 2 adds registration (identity / axis permutation only), the template
//! hierarchy, the lineage layers and the transform facts table (`frames`).
//! PR 3 adds the Foundry (`foundry`): a model's shape, its footprint and fit
//! against a machine reading, the catalog and co-resident pairs, `describe`,
//! and a native machine probe that opens no device.
//! PR 4 adds the morphometry comparison (`compare`): two measured results
//! compared cell by cell on the canonical frame, right minus left. PR 5 adds
//! the local-against-hosted economics (`economics`) and the CPU package's
//! cumulative energy counter. PR 6 adds the weights (`weights`):
//! dequantisation bit for bit as gguf-py 0.19.0 computes it, and
//! [`weight_statistics`], which reads a GGUF file's tensors and accumulates
//! per-tensor and per-cell statistics.
//!
//! What the figures are: before PR 6, declared anatomy (a tensor inventory or
//! the declared architecture), never a reading of weights or activations. PR
//! 6's statistics are a reading of the weights as stored in the file (never of
//! activations). See the crate README for the deviations from Python (D1-D17).
//!
//! The only `unsafe` in this crate is the calls into the C ABI (`ffi`).

use serde::Deserialize;

mod frames;
pub use frames::*;

mod ffi {
    //! The C ABI of `cpp/ma_abi.hpp`, declared exactly.

    use std::os::raw::c_char;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MaStr {
        pub ptr: *const c_char,
        pub len: usize,
    }

    impl MaStr {
        pub fn of(text: &str) -> Self {
            Self {
                ptr: text.as_ptr().cast(),
                len: text.len(),
            }
        }
    }

    pub const VALUE_NONE: u32 = 0;
    pub const VALUE_BOOL: u32 = 1;
    pub const VALUE_INT: u32 = 2;
    pub const VALUE_F64: u32 = 3;
    pub const VALUE_STR: u32 = 4;

    #[repr(C)]
    pub struct MaMeta {
        pub key: MaStr,
        pub tag: u32,
        pub boolean: u32,
        pub f64: f64,
        pub text: MaStr,
    }

    #[repr(C)]
    pub struct MaTensor {
        pub name: MaStr,
        pub dims: *const u64,
        pub ndims: usize,
        pub type_name: MaStr,
    }

    #[repr(C)]
    pub struct MaPayload {
        pub model: MaStr,
        pub family: MaStr,
        pub quantization_level: MaStr,
        pub parent_model: MaStr,
        pub metadata: *const MaMeta,
        pub metadata_len: usize,
        pub has_tensors: u32,
        pub tensors: *const MaTensor,
        pub tensors_len: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MaOptStr {
        pub present: u32,
        pub text: MaStr,
    }

    impl MaOptStr {
        pub fn of(text: Option<&str>) -> Self {
            match text {
                Some(text) => Self {
                    present: 1,
                    text: MaStr::of(text),
                },
                None => Self {
                    present: 0,
                    text: MaStr::of(""),
                },
            }
        }
    }

    #[repr(C)]
    pub struct MaPair {
        pub key: MaStr,
        pub value: MaStr,
    }

    #[repr(C)]
    pub struct MaAxis {
        pub axis_id: MaStr,
        pub semantic_id: MaStr,
        pub kind: MaStr,
        pub scalar_type: MaStr,
        pub ordering: MaStr,
        pub unit: MaOptStr,
        pub reference_frame: MaOptStr,
        pub calendar: MaOptStr,
        pub timezone: MaOptStr,
        pub orientation: MaOptStr,
        pub origin: MaOptStr,
        pub resolution: MaOptStr,
        pub has_bounds: u32,
        pub bounds_lower: MaStr,
        pub bounds_upper: MaStr,
        pub has_periodicity: u32,
        pub period: MaStr,
        pub phase: MaStr,
        pub labels_ref: MaOptStr,
        pub has_labels: u32,
        pub labels: *const MaStr,
        pub labels_len: usize,
        pub missingness_policy: MaStr,
        pub interpolation_policy: *const MaStr,
        pub interpolation_policy_len: usize,
        pub transform_policy: *const MaStr,
        pub transform_policy_len: usize,
        pub metadata: *const MaPair,
        pub metadata_len: usize,
    }

    #[repr(C)]
    pub struct MaSpace {
        pub space_id: MaStr,
        pub version: MaStr,
        pub topology: MaStr,
        pub axes: *const MaAxis,
        pub axes_len: usize,
        pub index_convention: MaStr,
        pub unit_system: MaOptStr,
        pub reference_frame: MaOptStr,
        pub valid_domain_rule: MaStr,
        pub region_atlas_refs: *const MaStr,
        pub region_atlas_refs_len: usize,
        pub metadata: *const MaPair,
        pub metadata_len: usize,
    }

    #[repr(C)]
    pub struct MaTemplateRef {
        pub template_id: MaStr,
        pub version: MaStr,
    }

    #[repr(C)]
    pub struct MaTemplateNode {
        pub template_ref: MaTemplateRef,
        pub tier: MaStr,
        pub label: MaStr,
        pub description: MaStr,
        pub parent_refs: *const MaTemplateRef,
        pub parent_refs_len: usize,
        pub coordinate_space: *const MaSpace,
        pub transform_policy: *const MaStr,
        pub transform_policy_len: usize,
        pub gaps: *const MaStr,
        pub gaps_len: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MaOptI64 {
        pub present: u32,
        pub value: i64,
    }

    impl MaOptI64 {
        pub fn of(value: Option<i64>) -> Self {
            Self {
                present: u32::from(value.is_some()),
                value: value.unwrap_or(0),
            }
        }
    }

    #[repr(C)]
    pub struct MaReading {
        pub backend: MaStr,
        pub torch_version: MaStr,
        pub probe: MaStr,
        pub has_gpu: u32,
        pub gpu_name: MaStr,
        pub gpu_total_bytes: MaOptI64,
        pub gpu_free_bytes: MaOptI64,
        pub ram_total_bytes: MaOptI64,
        pub disk_free_bytes: MaOptI64,
        pub bf16: u32,
        pub gaps: *const MaStr,
        pub gaps_len: usize,
    }

    #[repr(C)]
    pub struct MaFoundryModel {
        pub name: MaStr,
        pub payload: *const MaPayload,
        pub raised: MaOptStr,
        pub requires_backends: *const MaStr,
        pub requires_backends_len: usize,
    }

    #[repr(C)]
    pub struct MaCellRecord {
        pub block: MaOptI64,
        pub module: MaStr,
        pub parameters: i64,
        pub tensors: i64,
        pub element_types: *const MaStr,
        pub element_types_len: usize,
        pub nominal_bytes: MaOptI64,
        pub routed_parameters: i64,
    }

    #[repr(C)]
    pub struct MaMorphRecord {
        pub model: MaStr,
        pub source: MaStr,
        pub schema_version: MaStr,
        pub refusal: MaStr,
        pub gaps: *const MaStr,
        pub gaps_len: usize,
        pub native_axis_first: MaStr,
        pub native_axis_second: MaStr,
        pub expert_count: MaOptI64,
        pub expert_used_count: MaOptI64,
        pub cells: *const MaCellRecord,
        pub cells_len: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MaOptF64 {
        pub present: u32,
        pub value: f64,
    }

    impl MaOptF64 {
        pub fn of(value: Option<f64>) -> Self {
            Self {
                present: u32::from(value.is_some()),
                value: value.unwrap_or(0.0),
            }
        }
    }

    #[repr(C)]
    pub struct MaThroughput {
        pub model: MaStr,
        pub context: i64,
        pub decode_tokens_per_second: MaOptF64,
        pub prefill_tokens_per_second: MaOptF64,
        pub method: MaStr,
        pub provenance: MaStr,
    }

    #[repr(C)]
    pub struct MaEnergy {
        pub draw_watts: MaOptF64,
        pub usd_per_kwh: MaOptF64,
        pub source: MaStr,
        pub as_of: MaStr,
        pub provenance: MaStr,
    }

    #[repr(C)]
    pub struct MaPrice {
        pub provider: MaStr,
        pub model: MaStr,
        pub usd_per_million_input: MaOptF64,
        pub usd_per_million_output: MaOptF64,
        pub source: MaStr,
        pub as_of: MaStr,
        pub provenance: MaStr,
    }

    #[repr(C)]
    pub struct MaVolume {
        pub input_tokens: MaOptI64,
        pub output_tokens: MaOptI64,
        pub stated_by: MaStr,
    }

    #[repr(C)]
    pub struct MaLlamacpp {
        pub mapping: u32,
        pub model: *const MaMeta,
        pub timings_mapping: u32,
        pub timings: *const MaMeta,
        pub timings_len: usize,
    }

    pub const OK: i32 = 0;
    pub const INVALID_INPUT: i32 = 1;

    unsafe extern "C" {
        pub fn ma_abi_version() -> u32;
        pub fn ma_analyze(
            model: MaStr,
            payload: *const MaPayload,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_constants(out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_canonical_space(out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_sha256_hex(
            data: *const u8,
            len: usize,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_format_repr(x: f64, out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_format_fixed(
            x: f64,
            precision: i32,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_format_grouped(x: i64, out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_register(
            source: *const MaSpace,
            target: *const MaSpace,
            edge_visit_budget: i64,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_register_model(
            model: MaStr,
            payload: *const MaPayload,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_template_hierarchy(
            builtin: u32,
            nodes: *const MaTemplateNode,
            nodes_len: usize,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        #[allow(clippy::too_many_arguments)]
        pub fn ma_measure_layers(
            measure: u32,
            model: MaStr,
            payload: *const MaPayload,
            builtin: u32,
            nodes: *const MaTemplateNode,
            nodes_len: usize,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_shape(
            model: MaStr,
            payload: *const MaPayload,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        #[allow(clippy::too_many_arguments)]
        pub fn ma_foundry(
            reading: *const MaReading,
            models: *const MaFoundryModel,
            models_len: usize,
            context: i64,
            mem_fraction: MaOptStr,
            source: MaStr,
            as_of: MaStr,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_describe(
            reading: *const MaReading,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_probe(
            device_filter: MaOptStr,
            disk_root: MaOptStr,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_compare(
            left_model: MaStr,
            left_payload: *const MaPayload,
            right_model: MaStr,
            right_payload: *const MaPayload,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_compare_records(
            left: *const MaMorphRecord,
            right: *const MaMorphRecord,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_throughput_from_llamacpp(
            payload: *const MaLlamacpp,
            model: MaStr,
            context: i64,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_cost_compare(
            throughput: *const MaThroughput,
            energy: *const MaEnergy,
            price: *const MaPrice,
            volume: *const MaVolume,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_energy_usd_for_seconds(
            energy: *const MaEnergy,
            seconds: f64,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_read_power(
            outcome: u32,
            watts: f64,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_economics_constants(out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_cpu_package_energy(out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_quant_types(out: *mut *mut c_char, out_len: *mut usize) -> i32;
        pub fn ma_dequantize(
            type_id: u32,
            data: *const u8,
            len: usize,
            values: *mut f32,
            values_len: usize,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_weight_statistics(
            path: MaStr,
            threads: u32,
            progress: MaProgressFn,
            context: *mut std::ffi::c_void,
            out: *mut *mut c_char,
            out_len: *mut usize,
        ) -> i32;
        pub fn ma_free(p: *mut c_char);
    }

    /// `ma_progress_fn`: called on the calling thread only; nonzero cancels.
    pub type MaProgressFn =
        Option<unsafe extern "C" fn(context: *mut std::ffi::c_void, done: u64, total: u64) -> i32>;
}

mod foundry;
pub use foundry::*;

mod compare;
pub use compare::*;

mod economics;
pub use economics::*;

mod weights;
pub use weights::*;

/// The ABI version this wrapper was written against.
pub const ABI_VERSION: u32 = 6;

/// Why the core returned no result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The input broke an ABI rule (the core's reason).
    InvalidInput(String),
    /// The core could not finish (allocation failure), or its result was not
    /// the JSON this wrapper expects.
    Internal(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => write!(f, "invalid input: {reason}"),
            Self::Internal(reason) => write!(f, "model-anatomy core failure: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

/// Takes the core's result: copies it, frees it, and maps the return code.
fn take(code: i32, out: *mut std::os::raw::c_char, len: usize) -> Result<String, Error> {
    let text = if out.is_null() {
        None
    } else {
        // SAFETY: on every return the core either leaves `out` null or points
        // it at `len` bytes it allocated (plus a NUL) and does not touch again;
        // the bytes are read once here, before `ma_free` releases them.
        let bytes = unsafe { std::slice::from_raw_parts(out.cast::<u8>(), len) }.to_vec();
        // SAFETY: `out` came from the core's allocator and is freed exactly once.
        unsafe { ffi::ma_free(out) };
        Some(String::from_utf8(bytes).map_err(|_| Error::Internal("result is not UTF-8".into()))?)
    };
    let text = text.unwrap_or_default();
    match code {
        ffi::OK => Ok(text),
        ffi::INVALID_INPUT => Err(Error::InvalidInput(error_reason(&text))),
        _ => Err(Error::Internal(error_reason(&text))),
    }
}

fn error_reason(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("error")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "no reason given".to_owned())
}

/// A `model_info` value, as Python holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum MetaValue {
    /// Python's None (a GGUF header never produces it).
    None,
    Bool(bool),
    /// Canonical decimal text, any size: `-?(0|[1-9][0-9]*)`, never `-0`.
    Int(String),
    F64(f64),
    Str(String),
}

/// One tensor of the payload's inventory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadTensor {
    pub name: String,
    /// GGML axis order, as the GGUF header lists it.
    pub shape: Vec<u64>,
    pub type_name: String,
}

/// Python's `show`-style payload (`GGUFHeader.show_payload`'s shape). An empty
/// string stands for an absent field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Payload {
    pub model: String,
    pub family: String,
    pub quantization_level: String,
    pub parent_model: String,
    /// In GGUF file order; a key given twice keeps its first place and its
    /// last value, as a Python dict does.
    pub model_info: Vec<(String, MetaValue)>,
    /// `None` is a payload with no `tensors` list at all (not an empty one).
    pub tensors: Option<Vec<PayloadTensor>>,
}

/// `analyze(payload, model=model)` and everything derived from it, as the
/// core's JSON text. `payload` `None` is Python's `analyze(None)`.
pub fn analyze_json(payload: Option<&Payload>, model: &str) -> Result<String, Error> {
    with_payload(payload, |raw| {
        let mut out = std::ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: `model` and everything `raw` points into outlive the call (see
        // `with_payload`); a null payload is allowed; `out`/`len` are valid.
        let code = unsafe { ffi::ma_analyze(ffi::MaStr::of(model), raw, &mut out, &mut len) };
        take(code, out, len)
    })
}

/// Lowers `payload` to the C struct for the duration of `f` (null for `None`).
fn with_payload<R>(payload: Option<&Payload>, f: impl FnOnce(*const ffi::MaPayload) -> R) -> R {
    let Some(payload) = payload else {
        return f(std::ptr::null());
    };
    let lowered = lower(payload);
    // Every pointer in `lowered.raw` borrows from `payload` or from the
    // vectors `lowered` owns, all alive until `f` returns.
    f(&lowered.raw)
}

/// A payload lowered to the C struct. `raw` points into `metadata`, `tensors`
/// (their heap buffers, which do not move with the struct) and the payload it
/// was made from, which must outlive it.
struct Lowered {
    _metadata: Vec<ffi::MaMeta>,
    _tensors: Vec<ffi::MaTensor>,
    raw: ffi::MaPayload,
}

fn lower(payload: &Payload) -> Lowered {
    let metadata: Vec<ffi::MaMeta> = payload
        .model_info
        .iter()
        .map(|(key, value)| {
            let (tag, boolean, f64, text) = match value {
                MetaValue::None => (ffi::VALUE_NONE, 0, 0.0, ""),
                MetaValue::Bool(flag) => (ffi::VALUE_BOOL, u32::from(*flag), 0.0, ""),
                MetaValue::Int(digits) => (ffi::VALUE_INT, 0, 0.0, digits.as_str()),
                MetaValue::F64(number) => (ffi::VALUE_F64, 0, *number, ""),
                MetaValue::Str(text) => (ffi::VALUE_STR, 0, 0.0, text.as_str()),
            };
            ffi::MaMeta {
                key: ffi::MaStr::of(key),
                tag,
                boolean,
                f64,
                text: ffi::MaStr::of(text),
            }
        })
        .collect();
    let tensors: Vec<ffi::MaTensor> = payload
        .tensors
        .iter()
        .flatten()
        .map(|tensor| ffi::MaTensor {
            name: ffi::MaStr::of(&tensor.name),
            dims: tensor.shape.as_ptr(),
            ndims: tensor.shape.len(),
            type_name: ffi::MaStr::of(&tensor.type_name),
        })
        .collect();
    let raw = ffi::MaPayload {
        model: ffi::MaStr::of(&payload.model),
        family: ffi::MaStr::of(&payload.family),
        quantization_level: ffi::MaStr::of(&payload.quantization_level),
        parent_model: ffi::MaStr::of(&payload.parent_model),
        metadata: metadata.as_ptr(),
        metadata_len: metadata.len(),
        has_tensors: u32::from(payload.tensors.is_some()),
        tensors: tensors.as_ptr(),
        tensors_len: tensors.len(),
    };
    Lowered {
        _metadata: metadata,
        _tensors: tensors,
        raw,
    }
}

/// [`analyze_json`], deserialised.
pub fn analyze(payload: Option<&Payload>, model: &str) -> Result<Analysis, Error> {
    let text = analyze_json(payload, model)?;
    serde_json::from_str(&text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))
}

fn call0(
    f: unsafe extern "C" fn(*mut *mut std::os::raw::c_char, *mut usize) -> i32,
) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `f` is one of the core's no-input entry points; `out` and `len`
    // are valid for writes.
    let code = unsafe { f(&mut out, &mut len) };
    take(code, out, len)
}

/// The constants (module vocabulary, bit widths, budgets), as JSON.
pub fn constants_json() -> Result<String, Error> {
    call0(ffi::ma_constants)
}

/// The canonical space: its canonical bytes (hex), `space_commitment()`, the
/// module axis's `labels_ref` and both native spaces' references, as JSON.
pub fn canonical_space_json() -> Result<String, Error> {
    call0(ffi::ma_canonical_space)
}

/// [`canonical_space_json`], deserialised.
pub fn canonical_space() -> Result<CanonicalSpace, Error> {
    let text = canonical_space_json()?;
    serde_json::from_str(&text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))
}

/// The C ABI version the linked core reports.
pub fn abi_version() -> u32 {
    // SAFETY: takes nothing and returns a constant.
    unsafe { ffi::ma_abi_version() }
}

fn text_of(json: &str) -> Result<String, Error> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|value| value.get("text")?.as_str().map(str::to_owned))
        .ok_or_else(|| Error::Internal("unexpected result".into()))
}

/// The core's SHA-256 of `data`, lowercase hex.
pub fn sha256_hex(data: &[u8]) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `data` is valid for `data.len()` bytes during the call.
    let code = unsafe { ffi::ma_sha256_hex(data.as_ptr(), data.len(), &mut out, &mut len) };
    let json = take(code, out, len)?;
    serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|value| value.get("hex")?.as_str().map(str::to_owned))
        .ok_or_else(|| Error::Internal("unexpected result".into()))
}

/// Python's `repr(x)` of a float, by the core.
pub fn format_repr(x: f64) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `out` and `len` are valid for writes.
    let code = unsafe { ffi::ma_format_repr(x, &mut out, &mut len) };
    text_of(&take(code, out, len)?)
}

/// Python's `format(x, f".{precision}f")`, by the core.
pub fn format_fixed(x: f64, precision: i32) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `out` and `len` are valid for writes.
    let code = unsafe { ffi::ma_format_fixed(x, precision, &mut out, &mut len) };
    text_of(&take(code, out, len)?)
}

/// Python's `format(n, ",")`, by the core.
pub fn format_grouped(n: i64) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `out` and `len` are valid for writes.
    let code = unsafe { ffi::ma_format_grouped(n, &mut out, &mut len) };
    text_of(&take(code, out, len)?)
}

// ── the typed result ────────────────────────────────────────────────────────

/// A Python `Fraction`, reduced: `(numerator, denominator)`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct Ratio(pub i64, pub i64);

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    pub morphometry: Morphometry,
    pub voxel_field: VoxelField,
    pub native_space: NativeSpace,
}

/// `ModelMorphometry`'s fields, with its derived properties in `derived`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Morphometry {
    pub schema_version: String,
    pub model: String,
    pub source: String,
    pub architecture: String,
    pub family: String,
    pub quantization: String,
    pub declared_parameters: Option<i64>,
    pub context_length: Option<i64>,
    pub embedding_length: Option<i64>,
    pub block_count: Option<i64>,
    pub expert_count: Option<i64>,
    pub expert_used_count: Option<i64>,
    pub routing_metadata_error: String,
    /// Empty when measured; else a Python refusal code, or the native
    /// `INTEGER_OUT_OF_RANGE` (D2).
    pub refusal: String,
    pub gaps: Vec<String>,
    pub native_axis_order: [String; 2],
    pub tensors: Vec<TensorRecord>,
    pub cells: Vec<Cell>,
    pub derived: Derived,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TensorRecord {
    pub name: String,
    pub module: String,
    pub block: Option<i64>,
    pub shape: Vec<i64>,
    pub element_type: String,
    pub parameters: i64,
    pub family: String,
    pub stage: i64,
    pub bits_per_weight: Option<Ratio>,
    pub nominal_bytes: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub block: Option<i64>,
    pub module: String,
    pub parameters: i64,
    pub tensors: i64,
    pub element_types: Vec<String>,
    pub nominal_bytes: Option<i64>,
    pub routed_parameters: i64,
    pub family: String,
    pub stage: i64,
    pub label: String,
    pub mean_bits_per_weight: Option<Ratio>,
    /// `Cell.active_parameters(expert_used_count, expert_count)`. A cell cannot
    /// see the model-level gap: read [`Derived::active_path_gap`] first.
    pub active_parameters: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Derived {
    pub ok: bool,
    pub counted_parameters: i64,
    pub nominal_bytes: Option<i64>,
    pub coverage: Option<f64>,
    pub is_mixture_of_experts: bool,
    pub routed_expert_parameters: Option<i64>,
    pub always_active_parameters: Option<i64>,
    pub active_path_gap: Option<String>,
    pub active_parameters: Option<i64>,
    pub active_fraction: Option<f64>,
    pub blocks: Vec<BlockProfile>,
    pub by_family: Vec<(String, i64)>,
    pub block_dispersion: BlockDispersion,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockProfile {
    pub block: i64,
    pub parameters: i64,
    pub nominal_bytes: Option<i64>,
    pub modules: i64,
    pub mean_bits_per_weight: Option<Ratio>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockDispersion {
    pub relative: Option<f64>,
    pub outliers: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VoxelField {
    pub width: i64,
    pub height: i64,
    pub depth: i64,
    pub max_parameters: i64,
    pub has_stack_external: bool,
    pub source: String,
    pub max_active_parameters: Option<i64>,
    pub has_active_path: bool,
    pub occupied: i64,
    pub capacity: i64,
    pub voxels: Vec<Voxel>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Voxel {
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub module: String,
    pub parameters: i64,
    pub intensity: f64,
    pub bits_per_weight: Option<f64>,
    pub active_parameters: Option<i64>,
    pub active_intensity: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeSpace {
    pub axis_order: [String; 2],
    /// `coordinate_space_ref(native_space(morph))`.
    #[serde(rename = "ref")]
    pub reference: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSpace {
    pub space_id: String,
    pub version: String,
    /// `canonical.coordinate_space_bytes(canonical_space())`, hex.
    pub bytes_hex: String,
    /// `space_commitment()`.
    pub commitment: String,
    pub module_labels_ref: String,
    pub native_block_module_ref: String,
    pub native_module_block_ref: String,
}

pub mod gguf {
    //! `gguf_header.GGUFHeader.show_payload`, ported over a header's raw fields.
    //!
    //! This crate does not depend on lattice-core (its reader would bring a
    //! dozen packages into this lock), so the caller passes what
    //! `lattice_core::llama::gguf::read_header` read: the metadata in first-seen
    //! order (one entry per key) and the tensor table.

    use super::{MetaValue, Payload, PayloadTensor};

    /// A header value.
    #[derive(Clone, Debug, PartialEq)]
    pub enum GgufValue {
        Bool(bool),
        /// Canonical decimal text, any size.
        Int(String),
        Float(f64),
        Str(String),
        /// An array the reader kept (64 items or fewer), with the caller's
        /// rendering of Python's `str()` of it. `model_info` drops it.
        Other(String),
    }

    /// One tensor of the table.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct GgufTensor {
        pub name: String,
        pub shape: Vec<u64>,
        pub ggml_type: u32,
    }

    /// Why `show_payload` raised: Python's `int(general.file_type)` failed.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ShowPayloadError {
        /// `ValueError`: text that is not an integer, or NaN.
        Value,
        /// `TypeError`: an array.
        Type,
        /// `OverflowError`: an infinite float.
        Overflow,
    }

    impl ShowPayloadError {
        /// Python's exception name.
        pub fn kind(self) -> &'static str {
            match self {
                Self::Value => "ValueError",
                Self::Type => "TypeError",
                Self::Overflow => "OverflowError",
            }
        }
    }

    /// `GGML_TYPES` (gguf_header.py:53), or `"type N"` for a type it lacks.
    pub fn ggml_type_name(ggml_type: u32) -> String {
        let name = match ggml_type {
            0 => "F32",
            1 => "F16",
            2 => "Q4_0",
            3 => "Q4_1",
            6 => "Q5_0",
            7 => "Q5_1",
            8 => "Q8_0",
            9 => "Q8_1",
            10 => "Q2_K",
            11 => "Q3_K",
            12 => "Q4_K",
            13 => "Q5_K",
            14 => "Q6_K",
            15 => "Q8_K",
            16 => "IQ2_XXS",
            17 => "IQ2_XS",
            18 => "IQ3_XXS",
            19 => "IQ1_S",
            20 => "IQ4_NL",
            21 => "IQ3_S",
            22 => "IQ2_S",
            23 => "IQ4_XS",
            24 => "I8",
            25 => "I16",
            26 => "I32",
            27 => "I64",
            28 => "F64",
            29 => "IQ1_M",
            30 => "BF16",
            34 => "TQ1_0",
            35 => "TQ2_0",
            39 => "MXFP4",
            other => return format!("type {other}"),
        };
        name.to_owned()
    }

    /// `FILE_TYPES` (gguf_header.py:40).
    pub fn file_type_name(file_type: i64) -> Option<&'static str> {
        Some(match file_type {
            0 => "F32",
            1 => "F16",
            2 => "Q4_0",
            3 => "Q4_1",
            7 => "Q8_0",
            8 => "Q5_0",
            9 => "Q5_1",
            10 => "Q2_K",
            11 => "Q3_K_S",
            12 => "Q3_K_M",
            13 => "Q3_K_L",
            14 => "Q4_K_S",
            15 => "Q4_K_M",
            16 => "Q5_K_S",
            17 => "Q5_K_M",
            18 => "Q6_K",
            19 => "IQ2_XXS",
            20 => "IQ2_XS",
            21 => "Q2_K_S",
            22 => "IQ3_XS",
            23 => "IQ3_XXS",
            24 => "IQ1_S",
            25 => "IQ4_NL",
            26 => "IQ3_S",
            27 => "IQ3_M",
            28 => "IQ2_S",
            29 => "IQ2_M",
            30 => "IQ4_XS",
            31 => "IQ1_M",
            32 => "BF16",
            36 => "TQ1_0",
            37 => "TQ2_0",
            38 => "MXFP4_MOE",
            _ => return None,
        })
    }

    /// Python's `str(value)`.
    fn py_str(value: &GgufValue) -> Result<String, super::Error> {
        Ok(match value {
            GgufValue::Bool(true) => "True".to_owned(),
            GgufValue::Bool(false) => "False".to_owned(),
            GgufValue::Int(digits) => digits.clone(),
            GgufValue::Float(number) => super::format_repr(*number)?,
            GgufValue::Str(text) | GgufValue::Other(text) => text.clone(),
        })
    }

    /// `int(value)`'s decimal text, exact however large.
    fn py_int(value: &GgufValue) -> Result<String, ShowPayloadError> {
        match value {
            GgufValue::Bool(flag) => Ok(if *flag { "1" } else { "0" }.to_owned()),
            GgufValue::Int(digits) => Ok(digits.clone()),
            GgufValue::Float(number) if number.is_nan() => Err(ShowPayloadError::Value),
            GgufValue::Float(number) if number.is_infinite() => Err(ShowPayloadError::Overflow),
            GgufValue::Float(number) => Ok(float_to_integer(*number)),
            GgufValue::Str(text) => int_of_text(text),
            GgufValue::Other(_) => Err(ShowPayloadError::Type),
        }
    }

    /// `int(str)`: ASCII whitespace around (CPython's `Py_ISSPACE` for an ASCII
    /// string), a sign, ASCII digits with single underscores between them, at
    /// most 4,300 digits. Other scripts' digits and spaces are refused (D3).
    fn int_of_text(text: &str) -> Result<String, ShowPayloadError> {
        let text =
            text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'));
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let valid = !digits.is_empty()
            && !digits.starts_with('_')
            && !digits.ends_with('_')
            && !digits.contains("__")
            && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_');
        if !valid {
            return Err(ShowPayloadError::Value);
        }
        let plain = digits.replace('_', "");
        if plain.len() > 4300 {
            return Err(ShowPayloadError::Value);
        }
        let plain = plain.trim_start_matches('0');
        Ok(match (plain.is_empty(), negative) {
            (true, _) => "0".to_owned(),
            (false, true) => format!("-{plain}"),
            (false, false) => plain.to_owned(),
        })
    }

    /// `int(x)` of a finite float: toward zero, exact.
    fn float_to_integer(number: f64) -> String {
        let truncated = number.trunc();
        if truncated.abs() < 9.0e15 {
            return (truncated as i64).to_string();
        }
        // Beyond 2^53 every float is an integer: mantissa * 2^exponent.
        let bits = truncated.abs().to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i64 - 1075;
        let mantissa = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
        // Little-endian base-10^9 limbs.
        let mut limbs: Vec<u64> = vec![mantissa % 1_000_000_000, mantissa / 1_000_000_000];
        for _ in 0..exponent.max(0) {
            let mut carry = 0u64;
            for limb in &mut limbs {
                let doubled = *limb * 2 + carry;
                *limb = doubled % 1_000_000_000;
                carry = doubled / 1_000_000_000;
            }
            if carry != 0 {
                limbs.push(carry);
            }
        }
        while limbs.len() > 1 && limbs.last() == Some(&0) {
            limbs.pop();
        }
        let mut digits = limbs.last().map(u64::to_string).unwrap_or_default();
        for limb in limbs.iter().rev().skip(1) {
            digits.push_str(&format!("{limb:09}"));
        }
        if number < 0.0 {
            format!("-{digits}")
        } else {
            digits
        }
    }

    /// `GGUFHeader.quantization`.
    fn quantization(metadata: &[(String, GgufValue)]) -> Result<String, ShowPayloadError> {
        let Some((_, value)) = metadata.iter().find(|(key, _)| key == "general.file_type") else {
            return Ok(String::new());
        };
        let number = py_int(value)?;
        Ok(match number.parse::<i64>().ok().and_then(file_type_name) {
            Some(name) => name.to_owned(),
            None => format!("file_type {number}"),
        })
    }

    /// `GGUFHeader(metadata, tensors).show_payload(model)`.
    ///
    /// `metadata` is the reader's: first-seen order, one entry per key.
    /// Errors as Python raises: an unusable `general.file_type`.
    pub fn show_payload(
        model: &str,
        metadata: &[(String, GgufValue)],
        tensors: &[GgufTensor],
    ) -> Result<Payload, ShowPayloadError> {
        let architecture = match metadata
            .iter()
            .find(|(key, _)| key == "general.architecture")
        {
            // `format_repr` fails only if the core cannot allocate.
            Some((_, value)) => py_str(value).map_err(|_| ShowPayloadError::Value)?,
            None => String::new(),
        };
        let quantization_level = quantization(metadata)?;
        let model_info = metadata
            .iter()
            .filter_map(|(key, value)| {
                let kept = match value {
                    GgufValue::Bool(flag) => MetaValue::Bool(*flag),
                    GgufValue::Int(digits) => MetaValue::Int(digits.clone()),
                    GgufValue::Float(number) => MetaValue::F64(*number),
                    GgufValue::Str(text) => MetaValue::Str(text.clone()),
                    GgufValue::Other(_) => return None,
                };
                Some((key.clone(), kept))
            })
            .collect();
        Ok(Payload {
            model: model.to_owned(),
            family: architecture,
            quantization_level,
            parent_model: String::new(),
            model_info,
            tensors: Some(
                tensors
                    .iter()
                    .map(|tensor| PayloadTensor {
                        name: tensor.name.clone(),
                        shape: tensor.shape.clone(),
                        type_name: ggml_type_name(tensor.ggml_type),
                    })
                    .collect(),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_linked_core_speaks_this_abi() {
        assert_eq!(abi_version(), ABI_VERSION);
    }

    #[test]
    fn a_null_payload_is_refused_by_name() {
        let analysis = analyze(None, "x").expect("analysis");
        assert_eq!(analysis.morphometry.refusal, "NO_MODEL_METADATA");
        assert!(!analysis.morphometry.derived.ok);
    }

    #[test]
    fn a_non_canonical_int_is_invalid_input_not_a_figure() {
        let payload = Payload {
            model_info: vec![(
                "general.parameter_count".into(),
                MetaValue::Int("007".into()),
            )],
            ..Payload::default()
        };
        assert!(matches!(
            analyze_json(Some(&payload), "x"),
            Err(Error::InvalidInput(_))
        ));
    }
}
