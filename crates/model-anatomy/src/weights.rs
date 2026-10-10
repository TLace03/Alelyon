//! A GGUF model's weights, read (PR 6 of ADR-0044): dequantisation bit for bit
//! as the Python package `gguf` 0.19.0 (gguf-py, part of llama.cpp, MIT
//! License) computes it, and streaming statistics per tensor and per canonical
//! (block, module) cell, computed by the C++ core (`cpp/dequant.*`,
//! `cpp/weights.*`).
//!
//! [`weight_statistics`] opens the file read-only, parses its header, and has
//! worker threads (at most `available_parallelism() - 1`, at least one) read
//! every tensor's data in chunks of whole blocks, dequantise them a few
//! thousand blocks at a time and accumulate: no tensor is ever held whole.
//! The progress callback and the cancellation flag are consulted on the
//! calling thread only (about every 100 ms), so neither needs to be `Sync`.
//!
//! A file that is not GGUF, a header that ends early or is malformed, a file
//! that does not open and a cancelled run are `Ok` with
//! [`WeightStatistics::refusal`] set and no figures; a tensor whose type has no
//! dequantiser, whose data runs past the end of the file, whose shape is not
//! whole blocks or that could not be read is listed in
//! [`WeightStatistics::refused`] while every other tensor is still measured.
//! The crate README (PR 6) defines every figure, the histogram's bin edges and
//! the tolerance the parity test holds the sums to (D16, D17).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Error, ffi, take};

/// The histogram's bin count: |x| of the finite non-zero values on a log2
/// scale. Bin 0 holds |x| < 2^-24 (subnormals included), bin i in 1..=30
/// holds 2^(i-25) <= |x| < 2^(i-24), bin 31 holds |x| >= 2^6.
pub const HISTOGRAM_BINS: usize = 32;

/// The inclusive lower edge of histogram bin `bin` (0.0 for bin 0), or None
/// past the last bin.
pub fn histogram_bin_lower_edge(bin: usize) -> Option<f64> {
    match bin {
        0 => Some(0.0),
        1..=31 => Some(2f64.powi(bin as i32 - 25)),
        _ => None,
    }
}

/// One GGML tensor type as gguf-py 0.19.0's `GGML_QUANT_SIZES` holds it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantTypeInfo {
    pub id: u32,
    pub name: String,
    pub block_size: u32,
    pub type_size: u32,
    /// Dequantised here, bit for bit as gguf-py.
    pub supported: bool,
}

/// What is accumulated over a tensor's or a cell's values. Every figure but
/// `count` and `non_finite` is over the finite values only.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightMoments {
    pub count: u64,
    /// NaN and +-inf, excluded from everything else.
    pub non_finite: u64,
    /// Exact zeros (either sign).
    pub zeros: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Compensated (Sum2) sums in double.
    pub sum: f64,
    pub sum_abs: f64,
    pub sum_sq: f64,
    /// `sum / n`, n the finite count.
    pub mean: Option<f64>,
    /// Population: `sqrt(max(sum_sq / n - mean^2, 0))`.
    pub std: Option<f64>,
    /// `sqrt(sum_sq / n)`.
    pub rms: Option<f64>,
    /// `sum_abs / n`.
    pub mean_abs: Option<f64>,
    /// `sqrt(sum_sq)`.
    pub l2: Option<f64>,
    /// [`HISTOGRAM_BINS`] counts; they sum to `count - non_finite - zeros`.
    pub histogram: Vec<u64>,
}

/// One tensor read in full.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TensorStatistics {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub type_id: u32,
    /// GGML order (ne0 first), as the header lists them.
    pub dims: Vec<u64>,
    /// Absolute offset of the tensor's data in the file.
    pub offset: u64,
    pub bytes: u64,
    /// The canonical cell: `block_of` (None outside the stack) and `module_of`.
    pub block: Option<i64>,
    pub module: String,
    pub stats: WeightMoments,
}

/// One canonical (block, module) cell: every tensor of it that was read.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellStatistics {
    pub block: Option<i64>,
    pub module: String,
    pub tensors: u64,
    pub stats: WeightMoments,
}

/// A tensor that was not measured, and why: `UNSUPPORTED_TYPE`, `TRUNCATED`,
/// `BAD_SHAPE` or `READ_FAILED`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefusedTensor {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub type_id: u32,
    pub block: Option<i64>,
    pub module: String,
    pub code: String,
    pub reason: String,
}

/// Why a whole file was not measured: `OPEN_FAILED`, `NOT_GGUF`,
/// `UNSUPPORTED_VERSION`, `HEADER_TRUNCATED`, `MALFORMED_HEADER`,
/// `READ_FAILED` or `CANCELLED`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightRefusal {
    pub code: String,
    pub reason: String,
}

/// A GGUF file's weight statistics.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeightStatistics {
    pub path: String,
    /// Set when the file as a whole was not measured; then nothing else is.
    pub refusal: Option<WeightRefusal>,
    #[serde(default)]
    pub gguf_version: Option<u32>,
    #[serde(default)]
    pub alignment: Option<u64>,
    #[serde(default)]
    pub data_offset: Option<u64>,
    pub file_bytes: u64,
    #[serde(default)]
    pub tensor_count: Option<u64>,
    /// Worker threads used.
    #[serde(default)]
    pub threads: Option<u32>,
    /// Tensor data bytes read (the measured tensors').
    #[serde(default)]
    pub bytes_total: Option<u64>,
    /// File order.
    pub tensors: Vec<TensorStatistics>,
    /// The stack-external row first, then by block, then by module.
    pub cells: Vec<CellStatistics>,
    /// File order.
    pub refused: Vec<RefusedTensor>,
    /// No refusal and no refused tensor.
    pub complete: bool,
}

fn parse<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))
}

/// GGML's type table, with which types are dequantised here.
pub fn quant_types() -> Result<Vec<QuantTypeInfo>, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: no input; `out` and `len` are valid for writes.
    let code = unsafe { ffi::ma_quant_types(&mut out, &mut len) };
    parse(&take(code, out, len)?)
}

#[derive(Deserialize)]
struct Elements {
    elements: usize,
}

/// `gguf.quants.dequantize(data, type_id)`: `data` must be whole blocks of a
/// supported type (see [`quant_types`]).
pub fn dequantize(type_id: u32, data: &[u8]) -> Result<Vec<f32>, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `data` is valid for `data.len()` bytes; a null `values` with
    // length 0 asks only for the element count; `out`/`len` are valid.
    let code = unsafe {
        ffi::ma_dequantize(
            type_id,
            data.as_ptr(),
            data.len(),
            std::ptr::null_mut(),
            0,
            &mut out,
            &mut len,
        )
    };
    let count: Elements = parse(&take(code, out, len)?)?;
    let mut values = vec![0f32; count.elements];
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `values` is valid for writes of `values.len()` floats, which is
    // the element count the core just reported for these same bytes.
    let code = unsafe {
        ffi::ma_dequantize(
            type_id,
            data.as_ptr(),
            data.len(),
            values.as_mut_ptr(),
            values.len(),
            &mut out,
            &mut len,
        )
    };
    take(code, out, len)?;
    Ok(values)
}

/// What the trampoline reaches through the core's `context` pointer.
struct Callbacks<'a> {
    progress: Option<&'a dyn Fn(u64, u64)>,
    cancel: Option<&'a AtomicBool>,
    cancelled_by_panic: Cell<bool>,
    panic: RefCell<Option<Box<dyn Any + Send>>>,
}

unsafe extern "C" fn trampoline(context: *mut c_void, done: u64, total: u64) -> i32 {
    // SAFETY: `context` is the `&Callbacks` that `weight_statistics_json`
    // passed for the duration of its own call into the core, which calls this
    // on that same thread only; it is read through a shared reference.
    let callbacks = unsafe { &*(context as *const Callbacks<'_>) };
    if callbacks.cancelled_by_panic.get() {
        return 1;
    }
    if let Some(progress) = callbacks.progress {
        // A panic must not unwind into C++: it is caught, the run cancelled,
        // and the panic resumed once the core has returned.
        if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(|| progress(done, total))) {
            callbacks.cancelled_by_panic.set(true);
            *callbacks.panic.borrow_mut() = Some(payload);
            return 1;
        }
    }
    i32::from(
        callbacks
            .cancel
            .is_some_and(|flag| flag.load(Ordering::Relaxed)),
    )
}

/// [`weight_statistics_with_threads`]'s document, as JSON.
pub fn weight_statistics_json(
    path: &Path,
    threads: u32,
    progress: Option<&dyn Fn(u64, u64)>,
    cancel: Option<&AtomicBool>,
) -> Result<String, Error> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::InvalidInput("the path is not valid Unicode".into()))?;
    let callbacks = Callbacks {
        progress,
        cancel,
        cancelled_by_panic: Cell::new(false),
        panic: RefCell::new(None),
    };
    let hook: ffi::MaProgressFn = if progress.is_some() || cancel.is_some() {
        Some(trampoline)
    } else {
        None
    };
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `text` outlives the call; `callbacks` outlives it too and is only
    // read by `trampoline`, on this thread, while the core runs; `out`/`len`
    // are valid for writes.
    let code = unsafe {
        ffi::ma_weight_statistics(
            ffi::MaStr::of(text),
            threads,
            hook,
            std::ptr::from_ref(&callbacks).cast_mut().cast(),
            &mut out,
            &mut len,
        )
    };
    let result = take(code, out, len);
    if let Some(payload) = callbacks.panic.borrow_mut().take() {
        std::panic::resume_unwind(payload);
    }
    result
}

/// Every tensor of the GGUF file at `path` read and measured, with at most
/// `threads` workers (0: the default, `available_parallelism() - 1`, at least
/// one; a larger request is capped by that). `progress` is called with the
/// tensor bytes read so far and the total; setting `cancel` stops the run,
/// which then comes back refused as `CANCELLED`. Both are consulted on this
/// thread only.
pub fn weight_statistics_with_threads(
    path: &Path,
    threads: u32,
    progress: Option<&dyn Fn(u64, u64)>,
    cancel: Option<&AtomicBool>,
) -> Result<WeightStatistics, Error> {
    parse(&weight_statistics_json(path, threads, progress, cancel)?)
}

/// [`weight_statistics_with_threads`] with the default thread count.
pub fn weight_statistics(
    path: &Path,
    progress: Option<&dyn Fn(u64, u64)>,
    cancel: Option<&AtomicBool>,
) -> Result<WeightStatistics, Error> {
    weight_statistics_with_threads(path, 0, progress, cancel)
}
