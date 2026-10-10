//! What running a model locally costs against a hosted price (PR 5),
//! computed by the C++ core: a port of the Python reference's
//! `economics.py`, and the CPU package's
//! cumulative energy counter read by the native probe.
//!
//! [`compare_costs`] takes a complete input record or refuses by name: there
//! is no default price, volume or wattage, and every comparison carries the
//! structural caveats. The floats are Python's to the bit (the parity goldens
//! compare them by their bits); a float Python can hold non-finite comes back
//! non-finite here too. The records are typed (deviation D14 in the crate
//! README); the energy counter is D15.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

use super::{Error, MetaValue, ffi, take};

/// `OBSERVED`, `DERIVED`, `DECLARED`, `UNMEASURED` (workstation.py).
pub const OBSERVED: &str = "OBSERVED";
pub const DERIVED: &str = "DERIVED";
pub const DECLARED: &str = "DECLARED";
pub const UNMEASURED: &str = "UNMEASURED";

/// A float the core writes as a number, or as Python's repr text when it is
/// not finite ("inf", "-inf", "nan").
///
/// Read through `serde_json::Value`, not an untagged enum: a program that links
/// this crate may turn on serde_json's `arbitrary_precision` (CENTCOM does,
/// through the verifier), and an untagged enum cannot read a number then. The
/// core writes the shortest text that reads back as the same double, so
/// `as_f64` is exact either way.
fn py_float<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("not a float: {n}"))),
        Some(serde_json::Value::String(text)) => match text.as_str() {
            "inf" => Ok(Some(f64::INFINITY)),
            "-inf" => Ok(Some(f64::NEG_INFINITY)),
            "nan" => Ok(Some(f64::NAN)),
            other => Err(D::Error::custom(format!("not a float: {other}"))),
        },
        Some(other) => Err(D::Error::custom(format!("not a float: {other}"))),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))
}

fn call0(
    f: unsafe extern "C" fn(*mut *mut std::os::raw::c_char, *mut usize) -> i32,
) -> Result<String, Error> {
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `f` takes no input; `out` and `len` are valid for writes.
    let code = unsafe { f(&mut out, &mut len) };
    take(code, out, len)
}

// ── the records ─────────────────────────────────────────────────────────────

/// `ThroughputReading`: generation rates measured on this machine.
#[derive(Clone, Debug, PartialEq)]
pub struct ThroughputReading {
    pub model: String,
    pub context: i64,
    pub decode_tokens_per_second: Option<f64>,
    pub prefill_tokens_per_second: Option<f64>,
    pub method: String,
    pub provenance: String,
}

/// The core's reading, with `complete` as it computed it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThroughputWire {
    model: String,
    context: i64,
    #[serde(deserialize_with = "py_float")]
    decode_tokens_per_second: Option<f64>,
    #[serde(deserialize_with = "py_float")]
    prefill_tokens_per_second: Option<f64>,
    method: String,
    provenance: String,
    complete: bool,
}

impl ThroughputReading {
    /// A reading with no method and OBSERVED provenance (Python's defaults).
    pub fn new(model: &str, context: i64, decode: Option<f64>, prefill: Option<f64>) -> Self {
        Self {
            model: model.into(),
            context,
            decode_tokens_per_second: decode,
            prefill_tokens_per_second: prefill,
            method: String::new(),
            provenance: OBSERVED.into(),
        }
    }

    /// `complete`: both rates present and positive (a NaN rate is not).
    pub fn complete(&self) -> bool {
        matches!((self.decode_tokens_per_second, self.prefill_tokens_per_second),
                 (Some(d), Some(p)) if d > 0.0 && p > 0.0)
    }
}

/// `EnergyDeclaration`: board draw and tariff, DECLARED with a source and date.
#[derive(Clone, Debug, PartialEq)]
pub struct EnergyDeclaration {
    pub draw_watts: Option<f64>,
    pub usd_per_kwh: Option<f64>,
    pub source: String,
    pub as_of: String,
    pub provenance: String,
}

impl Default for EnergyDeclaration {
    fn default() -> Self {
        Self {
            draw_watts: None,
            usd_per_kwh: None,
            source: String::new(),
            as_of: String::new(),
            provenance: DECLARED.into(),
        }
    }
}

/// `ProviderPrice`: a hosted provider's published price, DECLARED.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderPrice {
    pub provider: String,
    pub model: String,
    pub usd_per_million_input: Option<f64>,
    pub usd_per_million_output: Option<f64>,
    pub source: String,
    pub as_of: String,
    pub provenance: String,
}

impl ProviderPrice {
    /// A price with no rates, source or date (Python's defaults).
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            usd_per_million_input: None,
            usd_per_million_output: None,
            source: String::new(),
            as_of: String::new(),
            provenance: DECLARED.into(),
        }
    }
}

/// `MonthlyVolume`: tokens per month, stated by the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthlyVolume {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub stated_by: String,
}

impl Default for MonthlyVolume {
    fn default() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            stated_by: "the user".into(),
        }
    }
}

/// `CostLine`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CostLine {
    pub label: String,
    #[serde(deserialize_with = "py_float")]
    pub usd: Option<f64>,
    pub provenance: String,
    pub rule: String,
}

/// Each input record's `complete`, as `compare` read it (`price` is false for
/// an absent price).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InputsComplete {
    pub throughput: bool,
    pub energy: bool,
    pub price: bool,
    pub volume: bool,
}

/// `CostComparison`, with its properties and `describe(comparison)`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CostComparison {
    pub inputs_complete: InputsComplete,
    /// `established`: no refusal, so the figures below exist.
    pub established: bool,
    pub refusals: Vec<String>,
    pub local: Vec<CostLine>,
    pub hosted: Vec<CostLine>,
    /// `STRUCTURAL_CAVEATS`, attached to every comparison.
    pub caveats: Vec<String>,
    #[serde(deserialize_with = "py_float")]
    pub local_usd: Option<f64>,
    #[serde(deserialize_with = "py_float")]
    pub hosted_usd: Option<f64>,
    #[serde(deserialize_with = "py_float")]
    pub monthly_delta_usd: Option<f64>,
    #[serde(deserialize_with = "py_float")]
    pub gpu_hours_per_month: Option<f64>,
    /// `describe(comparison)`: refusals first, the caveats beside the figure.
    pub describe: Vec<String>,
}

/// `read_power`'s result: `(watts, provenance)`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PowerReading {
    #[serde(deserialize_with = "py_float")]
    pub watts: Option<f64>,
    pub provenance: String,
}

/// What an injected meter did, for [`read_power`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeterOutcome {
    /// No meter was given (Python's `reader=None`).
    NoMeter,
    /// The meter failed (Python: it raised).
    Failed,
    /// The meter answered (Python: it returned this, None included).
    Read(Option<f64>),
}

impl MeterOutcome {
    /// Calls `reader` once: an `Err` is [`MeterOutcome::Failed`].
    pub fn of<E>(reader: impl FnOnce() -> Result<Option<f64>, E>) -> Self {
        match reader() {
            Ok(watts) => Self::Read(watts),
            Err(_) => Self::Failed,
        }
    }
}

/// A llama-server `/completion` response, as `throughput_from_llamacpp` reads
/// it: `model` is `payload["model"]` (None: no such key); `timings` is
/// `payload["timings"]` in order (None: absent, or not a mapping).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LlamacppResponse {
    pub model: Option<MetaValue>,
    pub timings: Option<Vec<(String, MetaValue)>>,
}

// ── lowering ────────────────────────────────────────────────────────────────

fn meta(key: &str, value: &MetaValue) -> ffi::MaMeta {
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
}

fn lower_throughput(reading: &ThroughputReading) -> ffi::MaThroughput {
    ffi::MaThroughput {
        model: ffi::MaStr::of(&reading.model),
        context: reading.context,
        decode_tokens_per_second: ffi::MaOptF64::of(reading.decode_tokens_per_second),
        prefill_tokens_per_second: ffi::MaOptF64::of(reading.prefill_tokens_per_second),
        method: ffi::MaStr::of(&reading.method),
        provenance: ffi::MaStr::of(&reading.provenance),
    }
}

fn lower_energy(energy: &EnergyDeclaration) -> ffi::MaEnergy {
    ffi::MaEnergy {
        draw_watts: ffi::MaOptF64::of(energy.draw_watts),
        usd_per_kwh: ffi::MaOptF64::of(energy.usd_per_kwh),
        source: ffi::MaStr::of(&energy.source),
        as_of: ffi::MaStr::of(&energy.as_of),
        provenance: ffi::MaStr::of(&energy.provenance),
    }
}

fn lower_price(price: &ProviderPrice) -> ffi::MaPrice {
    ffi::MaPrice {
        provider: ffi::MaStr::of(&price.provider),
        model: ffi::MaStr::of(&price.model),
        usd_per_million_input: ffi::MaOptF64::of(price.usd_per_million_input),
        usd_per_million_output: ffi::MaOptF64::of(price.usd_per_million_output),
        source: ffi::MaStr::of(&price.source),
        as_of: ffi::MaStr::of(&price.as_of),
        provenance: ffi::MaStr::of(&price.provenance),
    }
}

fn lower_volume(volume: &MonthlyVolume) -> ffi::MaVolume {
    ffi::MaVolume {
        input_tokens: ffi::MaOptI64::of(volume.input_tokens),
        output_tokens: ffi::MaOptI64::of(volume.output_tokens),
        stated_by: ffi::MaStr::of(&volume.stated_by),
    }
}

// ── the entry points ────────────────────────────────────────────────────────

/// `throughput_from_llamacpp(payload, model=model, context=context)`, as JSON.
/// `payload` None is a payload that is not a mapping. Where Python raises
/// (OverflowError, ZeroDivisionError) this is `Error::InvalidInput` with the
/// exception's class and message.
pub fn throughput_from_llamacpp_json(
    payload: Option<&LlamacppResponse>,
    model: &str,
    context: i64,
) -> Result<String, Error> {
    let model_meta = payload
        .and_then(|p| p.model.as_ref())
        .map(|value| meta("model", value));
    let timings: Vec<ffi::MaMeta> = payload
        .and_then(|p| p.timings.as_ref())
        .map(|items| items.iter().map(|(key, value)| meta(key, value)).collect())
        .unwrap_or_default();
    let raw = payload.map(|p| ffi::MaLlamacpp {
        mapping: 1,
        model: model_meta
            .as_ref()
            .map_or(std::ptr::null(), |m| m as *const ffi::MaMeta),
        timings_mapping: u32::from(p.timings.is_some()),
        timings: timings.as_ptr(),
        timings_len: timings.len(),
    });
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `raw` points into `model_meta`, `timings` and `payload`, all alive
    // until after the call; a null payload is allowed; `out`/`len` are valid.
    let code = unsafe {
        ffi::ma_throughput_from_llamacpp(
            raw.as_ref()
                .map_or(std::ptr::null(), |r| r as *const ffi::MaLlamacpp),
            ffi::MaStr::of(model),
            context,
            &mut out,
            &mut len,
        )
    };
    take(code, out, len)
}

/// [`throughput_from_llamacpp_json`], deserialised.
pub fn throughput_from_llamacpp(
    payload: Option<&LlamacppResponse>,
    model: &str,
    context: i64,
) -> Result<ThroughputReading, Error> {
    let wire: ThroughputWire = parse(&throughput_from_llamacpp_json(payload, model, context)?)?;
    let reading = ThroughputReading {
        model: wire.model,
        context: wire.context,
        decode_tokens_per_second: wire.decode_tokens_per_second,
        prefill_tokens_per_second: wire.prefill_tokens_per_second,
        method: wire.method,
        provenance: wire.provenance,
    };
    if reading.complete() != wire.complete {
        return Err(Error::Internal(
            "the core's `complete` disagrees with its own rates".into(),
        ));
    }
    Ok(reading)
}

/// `compare(throughput, energy, price, volume)` with every property and
/// `describe`, as JSON. `price` None is Python's None.
pub fn compare_costs_json(
    throughput: &ThroughputReading,
    energy: &EnergyDeclaration,
    price: Option<&ProviderPrice>,
    volume: &MonthlyVolume,
) -> Result<String, Error> {
    let throughput = lower_throughput(throughput);
    let energy = lower_energy(energy);
    let price = price.map(lower_price);
    let volume = lower_volume(volume);
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: the lowered records borrow from the caller's records, alive until
    // after the call; a null price is allowed; `out`/`len` are valid.
    let code = unsafe {
        ffi::ma_cost_compare(
            &throughput,
            &energy,
            price
                .as_ref()
                .map_or(std::ptr::null(), |p| p as *const ffi::MaPrice),
            &volume,
            &mut out,
            &mut len,
        )
    };
    take(code, out, len)
}

/// [`compare_costs_json`], deserialised.
pub fn compare_costs(
    throughput: &ThroughputReading,
    energy: &EnergyDeclaration,
    price: Option<&ProviderPrice>,
    volume: &MonthlyVolume,
) -> Result<CostComparison, Error> {
    parse(&compare_costs_json(throughput, energy, price, volume)?)
}

/// `EnergyDeclaration.usd_for_seconds(seconds)` and `.complete`, as JSON.
pub fn usd_for_seconds_json(energy: &EnergyDeclaration, seconds: f64) -> Result<String, Error> {
    let energy = lower_energy(energy);
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `energy` borrows from the caller's record for the call.
    let code = unsafe { ffi::ma_energy_usd_for_seconds(&energy, seconds, &mut out, &mut len) };
    take(code, out, len)
}

/// `EnergyDeclaration.usd_for_seconds(seconds)`: None unless the declaration
/// is complete.
pub fn usd_for_seconds(energy: &EnergyDeclaration, seconds: f64) -> Result<Option<f64>, Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Usd {
        #[allow(dead_code)]
        complete: bool,
        #[serde(deserialize_with = "py_float")]
        usd: Option<f64>,
    }
    Ok(parse::<Usd>(&usd_for_seconds_json(energy, seconds)?)?.usd)
}

/// `read_power(reader)`, as JSON.
pub fn read_power_json(meter: MeterOutcome) -> Result<String, Error> {
    let (outcome, watts) = match meter {
        MeterOutcome::NoMeter => (0, 0.0),
        MeterOutcome::Failed => (1, 0.0),
        MeterOutcome::Read(None) => (2, 0.0),
        MeterOutcome::Read(Some(watts)) => (3, watts),
    };
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: plain values in; `out`/`len` are valid for writes.
    let code = unsafe { ffi::ma_read_power(outcome, watts, &mut out, &mut len) };
    take(code, out, len)
}

/// `read_power(reader)`: OBSERVED watts from a meter, else UNMEASURED.
pub fn read_power(meter: MeterOutcome) -> Result<PowerReading, Error> {
    parse(&read_power_json(meter)?)
}

/// `STRUCTURAL_CAVEATS`, the provenance words and the unit constants, as JSON.
pub fn economics_constants_json() -> Result<String, Error> {
    call0(ffi::ma_economics_constants)
}

/// `STRUCTURAL_CAVEATS`.
pub fn structural_caveats() -> Result<Vec<String>, Error> {
    #[derive(Deserialize)]
    struct Constants {
        structural_caveats: Vec<String>,
    }
    Ok(parse::<Constants>(&economics_constants_json()?)?.structural_caveats)
}

// ── the CPU package's energy counter (D15) ──────────────────────────────────

/// One raw sample of `\Energy Meter(RAPL_Package0_PKG)\Energy`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CpuEnergy {
    /// Cumulative energy in picowatt-hours (1 pWh = 3.6e-9 J), as the counter
    /// holds it: it starts at no particular moment, so only a difference
    /// between two samples is an energy.
    pub picowatt_hours: i64,
    pub counter_set: String,
    pub instance: String,
    pub counter: String,
    /// The sample's own timestamp: a FILETIME, 100 ns units since 1601 (UTC).
    pub filetime_100ns: i64,
}

impl CpuEnergy {
    /// The energy in joules.
    pub fn joules(&self) -> f64 {
        self.picowatt_hours as f64 * 3.6e-9
    }
}

/// The counter's reading, or why there is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CpuPackageEnergy {
    Read(CpuEnergy),
    /// The counter set or instance is absent (or did not read): the reason.
    Absent(String),
}

impl CpuPackageEnergy {
    pub fn energy(&self) -> Option<&CpuEnergy> {
        match self {
            Self::Read(energy) => Some(energy),
            Self::Absent(_) => None,
        }
    }
}

/// The CPU package's cumulative energy, as JSON (`{"energy", "reason"}`).
pub fn cpu_package_energy_json() -> Result<String, Error> {
    call0(ffi::ma_cpu_package_energy)
}

/// One sample of the CPU package's cumulative energy counter. No device is
/// opened. The accelerator's board power is not read: it stays DECLARED.
pub fn cpu_package_energy() -> Result<CpuPackageEnergy, Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        energy: Option<CpuEnergy>,
        reason: String,
    }
    let wire: Wire = parse(&cpu_package_energy_json()?)?;
    Ok(match wire.energy {
        Some(energy) => CpuPackageEnergy::Read(energy),
        None => CpuPackageEnergy::Absent(wire.reason),
    })
}
