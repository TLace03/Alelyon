//! The morphometry comparison (PR 4), computed by the C++ core: a port of the
//! Python reference's `morphometry_compare.py`.
//!
//! [`compare`] is `compare(analyze(left), analyze(right))`; [`compare_records`]
//! is `compare(left, right)` on records as given, which is how the record-level
//! refusals (a schema mismatch, an invalid source, a refused registration,
//! invalid cells) are reached. Signed deltas are right minus left; the relative
//! parameter difference is the exact reduced rational `(right - left) / left`
//! ([`Ratio`]), never a float; a cell reported on one side only carries no
//! numeric delta. What is compared is declared structure: it implies nothing
//! about capability or learned behaviour.
//!
//! The input surface is typed (deviation D13 in the crate README): Python's
//! container-type checks have nothing here to fire on.

use serde::Deserialize;

use super::{Error, Morphometry, Payload, Ratio, ffi, take, with_payload};

/// `MORPHOMETRY_COMPARISON_SCHEMA`.
pub const MORPHOMETRY_COMPARISON_SCHEMA: &str = "alelyon.lattice.model-morphometry-comparison/0.1";

/// `ComparisonCode`: the outcome of an attempted comparison.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComparisonCode {
    Compared,
    InputUnmeasured,
    SchemaMismatch,
    InvalidRecord,
    InvalidCells,
    RegistrationRefused,
}

impl ComparisonCode {
    /// The Python enumeration's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compared => "COMPARED",
            Self::InputUnmeasured => "INPUT_UNMEASURED",
            Self::SchemaMismatch => "SCHEMA_MISMATCH",
            Self::InvalidRecord => "INVALID_RECORD",
            Self::InvalidCells => "INVALID_CELLS",
            Self::RegistrationRefused => "REGISTRATION_REFUSED",
        }
    }
}

/// `CellPresence`: which operand reported a canonical cell.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CellPresence {
    Both,
    LeftOnly,
    RightOnly,
}

/// `CellComparison`: one canonical cell compared as `right - left`. The deltas
/// exist only where both operands reported the cell.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CellComparison {
    /// None is the stack-external row.
    pub block: Option<i64>,
    pub module: String,
    pub presence: CellPresence,
    pub left_parameters: Option<i64>,
    pub right_parameters: Option<i64>,
    pub parameter_delta: Option<i64>,
    pub absolute_parameter_difference: Option<i64>,
    /// The exact `(right - left) / left`, reduced, denominator positive.
    pub relative_parameter_difference: Option<Ratio>,
    pub left_nominal_bytes: Option<i64>,
    pub right_nominal_bytes: Option<i64>,
    pub nominal_bytes_delta: Option<i64>,
    pub left_active_parameters: Option<i64>,
    pub right_active_parameters: Option<i64>,
    pub active_parameters_delta: Option<i64>,
    pub left_element_types: Vec<String>,
    pub right_element_types: Vec<String>,
}

impl CellComparison {
    /// `(block, module)`.
    pub fn key(&self) -> (Option<i64>, &str) {
        (self.block, &self.module)
    }
}

/// `MorphometryComparison`: a typed comparison or a typed refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MorphometryComparison {
    pub schema_version: String,
    pub code: ComparisonCode,
    pub explanation: String,
    pub left_model: String,
    pub right_model: String,
    pub left_source: String,
    pub right_source: String,
    /// `space_commitment()` when compared; empty on a refusal.
    pub space_ref: String,
    pub cells: Vec<CellComparison>,
    pub gaps: Vec<String>,
    pub left_gaps: Vec<String>,
    pub right_gaps: Vec<String>,
    pub unmeasured_sides: Vec<String>,
}

impl MorphometryComparison {
    /// `ok`: the code is COMPARED.
    pub fn ok(&self) -> bool {
        self.code == ComparisonCode::Compared
    }

    /// `complete`: every reported quantity came from gap-free shared cells.
    pub fn complete(&self) -> bool {
        self.ok()
            && self.gaps.is_empty()
            && self.left_gaps.is_empty()
            && self.right_gaps.is_empty()
            && self.cells.iter().all(|cell| {
                cell.presence == CellPresence::Both
                    && cell.parameter_delta.is_some()
                    && cell.nominal_bytes_delta.is_some()
                    && cell.active_parameters_delta.is_some()
            })
    }
}

/// The core's document: the fields, plus `ok` and `complete` as the core
/// computed them (checked against [`MorphometryComparison::ok`]/`complete`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema_version: String,
    code: ComparisonCode,
    ok: bool,
    complete: bool,
    explanation: String,
    left_model: String,
    right_model: String,
    left_source: String,
    right_source: String,
    space_ref: String,
    cells: Vec<CellComparison>,
    gaps: Vec<String>,
    left_gaps: Vec<String>,
    right_gaps: Vec<String>,
    unmeasured_sides: Vec<String>,
}

fn parse(text: &str) -> Result<MorphometryComparison, Error> {
    let wire: Wire = serde_json::from_str(text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))?;
    let out = MorphometryComparison {
        schema_version: wire.schema_version,
        code: wire.code,
        explanation: wire.explanation,
        left_model: wire.left_model,
        right_model: wire.right_model,
        left_source: wire.left_source,
        right_source: wire.right_source,
        space_ref: wire.space_ref,
        cells: wire.cells,
        gaps: wire.gaps,
        left_gaps: wire.left_gaps,
        right_gaps: wire.right_gaps,
        unmeasured_sides: wire.unmeasured_sides,
    };
    if out.ok() != wire.ok || out.complete() != wire.complete {
        return Err(Error::Internal(
            "the core's ok/complete disagree with its own fields".into(),
        ));
    }
    Ok(out)
}

/// One `morphometry.Cell`, as `compare` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordCell {
    /// None is the stack-external row.
    pub block: Option<i64>,
    pub module: String,
    pub parameters: i64,
    pub tensors: i64,
    pub element_types: Vec<String>,
    pub nominal_bytes: Option<i64>,
    pub routed_parameters: i64,
}

/// The fields of `ModelMorphometry` that `compare` reads, as given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MorphometryRecord {
    pub model: String,
    pub source: String,
    pub schema_version: String,
    pub refusal: String,
    pub gaps: Vec<String>,
    pub native_axis_order: [String; 2],
    pub expert_count: Option<i64>,
    pub expert_used_count: Option<i64>,
    pub cells: Vec<RecordCell>,
}

impl From<&Morphometry> for MorphometryRecord {
    /// The record of an [`analyze`](crate::analyze) result.
    fn from(morph: &Morphometry) -> Self {
        Self {
            model: morph.model.clone(),
            source: morph.source.clone(),
            schema_version: morph.schema_version.clone(),
            refusal: morph.refusal.clone(),
            gaps: morph.gaps.clone(),
            native_axis_order: morph.native_axis_order.clone(),
            expert_count: morph.expert_count,
            expert_used_count: morph.expert_used_count,
            cells: morph
                .cells
                .iter()
                .map(|cell| RecordCell {
                    block: cell.block,
                    module: cell.module.clone(),
                    parameters: cell.parameters,
                    tensors: cell.tensors,
                    element_types: cell.element_types.clone(),
                    nominal_bytes: cell.nominal_bytes,
                    routed_parameters: cell.routed_parameters,
                })
                .collect(),
        }
    }
}

/// `compare(analyze(left, model=left_model), analyze(right, model=right_model))`,
/// as the core's JSON. A `None` payload is `analyze(None)`.
pub fn compare_json(
    left: Option<&Payload>,
    left_model: &str,
    right: Option<&Payload>,
    right_model: &str,
) -> Result<String, Error> {
    with_payload(left, |left_raw| {
        with_payload(right, |right_raw| {
            let mut out = std::ptr::null_mut();
            let mut len = 0usize;
            // SAFETY: both models and everything both raw payloads point into
            // outlive the call (see `with_payload`); null payloads are allowed;
            // `out`/`len` are valid for writes.
            let code = unsafe {
                ffi::ma_compare(
                    ffi::MaStr::of(left_model),
                    left_raw,
                    ffi::MaStr::of(right_model),
                    right_raw,
                    &mut out,
                    &mut len,
                )
            };
            take(code, out, len)
        })
    })
}

/// [`compare_json`], deserialised.
pub fn compare(
    left: Option<&Payload>,
    left_model: &str,
    right: Option<&Payload>,
    right_model: &str,
) -> Result<MorphometryComparison, Error> {
    parse(&compare_json(left, left_model, right, right_model)?)
}

/// A record lowered to the C structs. `raw` points into the vectors this owns
/// (heap buffers, which do not move with the struct) and the record it was made
/// from, which must outlive it.
struct LoweredRecord {
    _gaps: Vec<ffi::MaStr>,
    _element_types: Vec<Vec<ffi::MaStr>>,
    _cells: Vec<ffi::MaCellRecord>,
    raw: ffi::MaMorphRecord,
}

fn strs(texts: &[String]) -> Vec<ffi::MaStr> {
    texts.iter().map(|text| ffi::MaStr::of(text)).collect()
}

fn lower_record(record: &MorphometryRecord) -> LoweredRecord {
    let gaps = strs(&record.gaps);
    let element_types: Vec<Vec<ffi::MaStr>> = record
        .cells
        .iter()
        .map(|cell| strs(&cell.element_types))
        .collect();
    let cells: Vec<ffi::MaCellRecord> = record
        .cells
        .iter()
        .zip(&element_types)
        .map(|(cell, types)| ffi::MaCellRecord {
            block: ffi::MaOptI64::of(cell.block),
            module: ffi::MaStr::of(&cell.module),
            parameters: cell.parameters,
            tensors: cell.tensors,
            element_types: types.as_ptr(),
            element_types_len: types.len(),
            nominal_bytes: ffi::MaOptI64::of(cell.nominal_bytes),
            routed_parameters: cell.routed_parameters,
        })
        .collect();
    let raw = ffi::MaMorphRecord {
        model: ffi::MaStr::of(&record.model),
        source: ffi::MaStr::of(&record.source),
        schema_version: ffi::MaStr::of(&record.schema_version),
        refusal: ffi::MaStr::of(&record.refusal),
        gaps: gaps.as_ptr(),
        gaps_len: gaps.len(),
        native_axis_first: ffi::MaStr::of(&record.native_axis_order[0]),
        native_axis_second: ffi::MaStr::of(&record.native_axis_order[1]),
        expert_count: ffi::MaOptI64::of(record.expert_count),
        expert_used_count: ffi::MaOptI64::of(record.expert_used_count),
        cells: cells.as_ptr(),
        cells_len: cells.len(),
    };
    LoweredRecord {
        _gaps: gaps,
        _element_types: element_types,
        _cells: cells,
        raw,
    }
}

/// `compare(left, right)` on records as given, as the core's JSON.
pub fn compare_records_json(
    left: &MorphometryRecord,
    right: &MorphometryRecord,
) -> Result<String, Error> {
    let left = lower_record(left);
    let right = lower_record(right);
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: every pointer in both raw records borrows from the lowered
    // vectors or the records, all alive until after the call; the core only
    // reads them. `out`/`len` are valid for writes.
    let code = unsafe { ffi::ma_compare_records(&left.raw, &right.raw, &mut out, &mut len) };
    take(code, out, len)
}

/// [`compare_records_json`], deserialised.
pub fn compare_records(
    left: &MorphometryRecord,
    right: &MorphometryRecord,
) -> Result<MorphometryComparison, Error> {
    parse(&compare_records_json(left, right)?)
}
