// The C ABI of the model-anatomy core (ADR-0044).
//
// Plain C types only, so the header reads the same from C, C++ and Rust's FFI.
// Every entry point is noexcept: a C++ exception never crosses this boundary.
// Every string is UTF-8 given as (pointer, length), not NUL-terminated; a null
// pointer with length 0 is the empty string. Every result is UTF-8 JSON written
// by the core into memory it allocates; the caller frees it with `ma_free`.
//
// Return codes: MA_OK; MA_INVALID_INPUT when the input breaks a rule below
// (the result then holds {"error": "..."}); MA_INTERNAL when the core could
// not finish (allocation failure); then the result may be null.
#ifndef ALELYON_MODEL_ANATOMY_ABI_HPP
#define ALELYON_MODEL_ANATOMY_ABI_HPP

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define MA_NOEXCEPT noexcept
extern "C" {
#else
#define MA_NOEXCEPT
#endif

// 2: registration, the template hierarchy and the lineage layers were added.
// 3: the Foundry (shape, fit, catalog, pairs, describe) and the native probe.
// 4: the morphometry comparison (`ma_compare`, `ma_compare_records`).
// 5: the local-against-hosted economics and the CPU package energy counter.
// 6: dequantisation and a GGUF file's weight statistics.
#define MA_ABI_VERSION 6u

typedef struct ma_str {
    const char* ptr;
    size_t len;
} ma_str;

// A `model_info` value, tagged as Python would hold it. NONE is Python's None
// (no GGUF header produces it; a payload built by hand can).
enum {
    MA_VALUE_NONE = 0,
    MA_VALUE_BOOL = 1,
    MA_VALUE_INT = 2,  // `text`: canonical decimal, -?(0|[1-9][0-9]*), never "-0"
    MA_VALUE_F64 = 3,  // `f64`
    MA_VALUE_STR = 4,  // `text`
};

typedef struct ma_meta {
    ma_str key;
    uint32_t tag;
    uint32_t boolean;  // MA_VALUE_BOOL: 0 or 1
    double f64;        // MA_VALUE_F64
    ma_str text;       // MA_VALUE_INT and MA_VALUE_STR
} ma_meta;

// One tensor of the inventory: GGML axis order, as the GGUF header lists it.
typedef struct ma_tensor {
    ma_str name;
    const uint64_t* dims;
    size_t ndims;
    ma_str type_name;  // gguf_header.GGML_TYPES' name, or "type N"
} ma_tensor;

// Python's `show`-style payload. `metadata` is `model_info` in GGUF FILE ORDER;
// a key given twice keeps its first place and its last value (a Python dict).
// `has_tensors` = 0 is a payload with no `tensors` list at all; 1 with
// `tensors_len` = 0 is an empty list (the two differ in Python).
// An empty string stands for an absent field (Python reads both as falsy).
typedef struct ma_payload {
    ma_str model;               // payload["model"]
    ma_str family;              // payload["details"]["family"]
    ma_str quantization_level;  // payload["details"]["quantization_level"]
    ma_str parent_model;        // payload["details"]["parent_model"]
    const ma_meta* metadata;
    size_t metadata_len;
    uint32_t has_tensors;
    const ma_tensor* tensors;
    size_t tensors_len;
} ma_payload;

enum { MA_OK = 0, MA_INVALID_INPUT = 1, MA_INTERNAL = 2 };

uint32_t ma_abi_version(void) MA_NOEXCEPT;

// `morphometry.analyze(payload, model=model)`, its derived properties,
// `voxel_field` and the native space's commitment. `payload` null is Python's
// `analyze(None)`.
int ma_analyze(ma_str model, const ma_payload* payload, char** out, size_t* out_len) MA_NOEXCEPT;

// The module vocabulary, the bit-width table and the other constants.
int ma_constants(char** out, size_t* out_len) MA_NOEXCEPT;

// `canonical_space()`'s canonical bytes (hex), `space_commitment()`, the module
// axis's `labels_ref`, and both native spaces' references.
int ma_canonical_space(char** out, size_t* out_len) MA_NOEXCEPT;

// SHA-256 of `data`, as {"hex": "..."}; exported so the digest can be tested
// against published vectors.
int ma_sha256_hex(const uint8_t* data, size_t len, char** out, size_t* out_len) MA_NOEXCEPT;

// The text formatters, exported for parity tests: Python's `repr(float)`,
// `format(x, ".Nf")` and `format(n, ",")`. Each result is {"text": "..."}.
int ma_format_repr(double x, char** out, size_t* out_len) MA_NOEXCEPT;
int ma_format_fixed(double x, int32_t precision, char** out, size_t* out_len) MA_NOEXCEPT;
int ma_format_grouped(int64_t x, char** out, size_t* out_len) MA_NOEXCEPT;

// ── registration, the template hierarchy and the lineage layers (ABI 2) ──────

// An optional string: `present` 0 is Python's None.
typedef struct ma_opt_str {
    uint32_t present;
    ma_str text;
} ma_opt_str;

typedef struct ma_pair {
    ma_str key;
    ma_str value;
} ma_pair;

// `contracts.CoordinateAxis`, every field. Enumerations are their values'
// text ("MODEL_LAYER", "INTEGER", "ASCENDING", ...).
typedef struct ma_axis {
    ma_str axis_id;
    ma_str semantic_id;
    ma_str kind;
    ma_str scalar_type;
    ma_str ordering;
    ma_opt_str unit;
    ma_opt_str reference_frame;
    ma_opt_str calendar;
    ma_opt_str timezone;
    ma_opt_str orientation;
    ma_opt_str origin;
    ma_opt_str resolution;
    uint32_t has_bounds;
    ma_str bounds_lower;
    ma_str bounds_upper;
    uint32_t has_periodicity;
    ma_str period;
    ma_str phase;
    ma_opt_str labels_ref;
    uint32_t has_labels;
    const ma_str* labels;
    size_t labels_len;
    ma_str missingness_policy;
    const ma_str* interpolation_policy;
    size_t interpolation_policy_len;
    const ma_str* transform_policy;
    size_t transform_policy_len;
    const ma_pair* metadata;
    size_t metadata_len;
} ma_axis;

// `contracts.CoordinateSpace`, every field. The core does not re-run the
// contract's validation (deviation D6): give it a space Python would construct.
typedef struct ma_space {
    ma_str space_id;
    ma_str version;
    ma_str topology;
    const ma_axis* axes;
    size_t axes_len;
    ma_str index_convention;
    ma_opt_str unit_system;
    ma_opt_str reference_frame;
    ma_str valid_domain_rule;
    const ma_str* region_atlas_refs;
    size_t region_atlas_refs_len;
    const ma_pair* metadata;
    size_t metadata_len;
} ma_space;

typedef struct ma_template_ref {
    ma_str template_id;
    ma_str version;
} ma_template_ref;

// `template_hierarchy.TemplateNode`, as given (the core normalises it as the
// record does). `coordinate_space` null is an abstract node.
typedef struct ma_template_node {
    ma_template_ref ref;
    ma_str tier;
    ma_str label;
    ma_str description;
    const ma_template_ref* parent_refs;
    size_t parent_refs_len;
    const ma_space* coordinate_space;
    const ma_str* transform_policy;
    size_t transform_policy_len;
    const ma_str* gaps;
    size_t gaps_len;
} ma_template_node;

// `registration.analyze_exact_compatibility(source_space=, target_space=)`,
// its reachable identity / axis-permutation subset, as a CompatibilityReport.
// `edge_visit_budget` is MAX_MATCHING_EDGE_VISITS (pass 1048576, Python's);
// another value exists only so the budget's refusal can be tested.
int ma_register(const ma_space* source, const ma_space* target, int64_t edge_visit_budget, char** out,
                size_t* out_len) MA_NOEXCEPT;

// `morphometry.register(analyze(payload, model=model))`.
int ma_register_model(ma_str model, const ma_payload* payload, char** out, size_t* out_len) MA_NOEXCEPT;

// `template_hierarchy.snapshot_template_hierarchy(nodes)`, or with
// `builtin` = 1 (and no nodes) `model_morphometry_hierarchy()`.
int ma_template_hierarchy(uint32_t builtin, const ma_template_node* nodes, size_t nodes_len, char** out,
                          size_t* out_len) MA_NOEXCEPT;

// `hierarchy_morphometry.measure_layers(morph, snapshot=...)`. `measure` = 0 is
// `morph=None`; else morph = `analyze(payload, model=model)`. `builtin` = 1 is
// the default snapshot (`model_morphometry_hierarchy()`); else the snapshot of
// `nodes`.
int ma_measure_layers(uint32_t measure, ma_str model, const ma_payload* payload, uint32_t builtin,
                      const ma_template_node* nodes, size_t nodes_len, char** out, size_t* out_len) MA_NOEXCEPT;

// ── the Foundry (ABI 3) ──────────────────────────────────────────────────────

// An optional integer: `present` 0 is Python's None.
typedef struct ma_opt_i64 {
    uint32_t present;
    int64_t value;
} ma_opt_i64;

// `workstation.WorkstationReading`, as far as the ported functions read it: the
// primary device (`gpus[0]`) only. `backend` is one of "cuda", "rocm", "mps",
// "cpu", "absent", or the native probe's "vulkan" (deviation D10). `probe` is
// the native probe's name ("" for a reading built in Python): the backend row
// of `describe` is OBSERVED when it or `torch_version` is non-empty (D10).
// `bf16`: 0 False, 1 True, 2 None.
typedef struct ma_reading {
    ma_str backend;
    ma_str torch_version;
    ma_str probe;
    uint32_t has_gpu;
    ma_str gpu_name;
    ma_opt_i64 gpu_total_bytes;
    ma_opt_i64 gpu_free_bytes;
    ma_opt_i64 ram_total_bytes;
    ma_opt_i64 disk_free_bytes;
    uint32_t bf16;
    const ma_str* gaps;
    size_t gaps_len;
} ma_reading;

// One model a runtime lists, for `foundry.catalog_from_runtime(names, show, ...)`:
// `payload` is what `show(name)` returned (null: None, or it raised); `raised`
// names the exception class when it raised. `requires_backends` is a declared
// restriction set on the shape (`catalog_from_runtime` itself sets none).
typedef struct ma_foundry_model {
    ma_str name;
    const ma_payload* payload;
    ma_opt_str raised;
    const ma_str* requires_backends;
    size_t requires_backends_len;
} ma_foundry_model;

// `footprint.shape_from_runtime_payload(payload, model=model)`; `payload` null
// is a payload that is not a mapping.
int ma_shape(ma_str model, const ma_payload* payload, char** out, size_t* out_len) MA_NOEXCEPT;

// `catalog_from_runtime(names, show, reading)` (each verdict at
// CONTEXT_LADDER[1]), `runnable_here(..., context=context)` and
// `coresident_pairs(..., context=context)`, with `fit.serving_reserve_fraction()`
// read from `mem_fraction` as `local_hf` reads ALELYON_HF_MEM_FRACTION
// (`present` 0: unset). `source` and `as_of` are the catalog rows' own.
int ma_foundry(const ma_reading* reading, const ma_foundry_model* models, size_t models_len, int64_t context,
               ma_opt_str mem_fraction, ma_str source, ma_str as_of, char** out, size_t* out_len) MA_NOEXCEPT;

// `workstation.describe(reading)` and the reading's gaps.
int ma_describe(const ma_reading* reading, char** out, size_t* out_len) MA_NOEXCEPT;

// The native machine probe (D10): DXGI enumeration only, never a device opened;
// the adapter whose device id is `device_filter` (VK_LOADER_DEVICE_ID_FILTER's
// text; `present` 0: unset), else accelerator memory UNMEASURED. Free disk is
// read at `disk_root` (`present` 0: no home folder named).
int ma_probe(ma_opt_str device_filter, ma_opt_str disk_root, char** out, size_t* out_len) MA_NOEXCEPT;

// ── the morphometry comparison (ABI 4) ───────────────────────────────────────

// One `morphometry.Cell`, as `morphometry_compare.compare` reads it.
typedef struct ma_cell_record {
    ma_opt_i64 block;  // `present` 0: the stack-external row (None)
    ma_str module;
    int64_t parameters;
    int64_t tensors;
    const ma_str* element_types;
    size_t element_types_len;
    ma_opt_i64 nominal_bytes;
    int64_t routed_parameters;
} ma_cell_record;

// The fields of `ModelMorphometry` that `compare` reads, typed (deviation D13:
// Python's container-type checks have no input here to fire on).
typedef struct ma_morph_record {
    ma_str model;
    ma_str source;
    ma_str schema_version;
    ma_str refusal;
    const ma_str* gaps;
    size_t gaps_len;
    ma_str native_axis_first;   // native_axis_order[0]
    ma_str native_axis_second;  // native_axis_order[1]
    ma_opt_i64 expert_count;
    ma_opt_i64 expert_used_count;
    const ma_cell_record* cells;
    size_t cells_len;
} ma_morph_record;

// `morphometry_compare.compare(analyze(left_payload, model=left_model),
// analyze(right_payload, model=right_model))`; a null payload is `analyze(None)`.
int ma_compare(ma_str left_model, const ma_payload* left_payload, ma_str right_model, const ma_payload* right_payload,
               char** out, size_t* out_len) MA_NOEXCEPT;

// `morphometry_compare.compare(left, right)` on records as given. A null record
// is MA_INVALID_INPUT (Python raises TypeError for a non-ModelMorphometry).
int ma_compare_records(const ma_morph_record* left, const ma_morph_record* right, char** out,
                       size_t* out_len) MA_NOEXCEPT;

// ── the economics and the CPU energy counter (ABI 5) ─────────────────────────
// Floats in these results are JSON numbers when finite and Python's repr text
// ("inf", "-inf", "nan") otherwise, since Python's records can hold them.

// An optional float: `present` 0 is Python's None.
typedef struct ma_opt_f64 {
    uint32_t present;
    double value;
} ma_opt_f64;

// `economics.ThroughputReading`.
typedef struct ma_throughput {
    ma_str model;
    int64_t context;
    ma_opt_f64 decode_tokens_per_second;
    ma_opt_f64 prefill_tokens_per_second;
    ma_str method;
    ma_str provenance;
} ma_throughput;

// `economics.EnergyDeclaration`.
typedef struct ma_energy {
    ma_opt_f64 draw_watts;
    ma_opt_f64 usd_per_kwh;
    ma_str source;
    ma_str as_of;
    ma_str provenance;
} ma_energy;

// `economics.ProviderPrice`.
typedef struct ma_price {
    ma_str provider;
    ma_str model;
    ma_opt_f64 usd_per_million_input;
    ma_opt_f64 usd_per_million_output;
    ma_str source;
    ma_str as_of;
    ma_str provenance;
} ma_price;

// `economics.MonthlyVolume`.
typedef struct ma_volume {
    ma_opt_i64 input_tokens;
    ma_opt_i64 output_tokens;
    ma_str stated_by;
} ma_volume;

// A llama-server `/completion` response, as `throughput_from_llamacpp` reads it.
// `mapping` 0: the payload is not a Mapping (everything else ignored). `model`
// null: no "model" key. `timings_mapping` 0: "timings" absent or not a Mapping.
// `timings` are its keys and values in order (a repeated key: last value).
typedef struct ma_llamacpp {
    uint32_t mapping;
    const ma_meta* model;
    uint32_t timings_mapping;
    const ma_meta* timings;
    size_t timings_len;
} ma_llamacpp;

// `throughput_from_llamacpp(payload, model=model, context=context)`. Where
// Python raises (OverflowError, ZeroDivisionError) the code is
// MA_INVALID_INPUT and the error is "<class>: <message>".
int ma_throughput_from_llamacpp(const ma_llamacpp* payload, ma_str model, int64_t context, char** out,
                                size_t* out_len) MA_NOEXCEPT;

// `compare(throughput, energy, price, volume)`, every property and `describe`;
// `price` null is None.
int ma_cost_compare(const ma_throughput* throughput, const ma_energy* energy, const ma_price* price,
                    const ma_volume* volume, char** out, size_t* out_len) MA_NOEXCEPT;

// `EnergyDeclaration.usd_for_seconds(seconds)` and `.complete`.
int ma_energy_usd_for_seconds(const ma_energy* energy, double seconds, char** out, size_t* out_len) MA_NOEXCEPT;

// `read_power(reader)`: `outcome` 0 no reader, 1 it raised, 2 it returned None,
// 3 it returned `watts`.
int ma_read_power(uint32_t outcome, double watts, char** out, size_t* out_len) MA_NOEXCEPT;

// STRUCTURAL_CAVEATS, the provenance words and the unit constants.
int ma_economics_constants(char** out, size_t* out_len) MA_NOEXCEPT;

// One raw sample of `\Energy Meter(RAPL_Package0_PKG)\Energy` (picowatt-hours,
// cumulative) with its timestamp; {"energy": null, "reason": ...} when the
// counter set or instance is absent. No device is opened.
int ma_cpu_package_energy(char** out, size_t* out_len) MA_NOEXCEPT;

// ── dequantisation and weight statistics (ABI 6) ─────────────────────────────

// GGML's type table as gguf-py 0.19.0 holds it: [{"id", "name", "block_size",
// "type_size", "supported"}], `supported` meaning dequantised bit for bit here.
int ma_quant_types(char** out, size_t* out_len) MA_NOEXCEPT;

// `gguf.quants.dequantize(data, type)`: `data` must be whole blocks of a
// supported type. With `values` null (and `values_len` 0) nothing is written
// and the result is {"elements": n}, the count to allocate; otherwise
// `values_len` must be at least n and the n floats are written to `values`.
int ma_dequantize(uint32_t type, const uint8_t* data, size_t len, float* values, size_t values_len, char** out,
                  size_t* out_len) MA_NOEXCEPT;

// Called on the CALLING thread only (never a worker), with the tensor bytes
// read so far and the total to read. Return nonzero to cancel the run.
typedef int (*ma_progress_fn)(void* context, uint64_t done, uint64_t total);

// Every tensor of the GGUF file at `path` (UTF-8) read, dequantised block by
// block and accumulated per tensor and per canonical (block, module) cell; the
// file is opened read-only. `threads` 0 is hardware_concurrency() - 1 (at
// least 1), and any request is capped by that. `progress` may be null. A file
// that is not GGUF, a header that ends early or is malformed, a file that does
// not open and a cancelled run are MA_OK with {"refusal": {"code", "reason"}};
// a tensor that cannot be read is listed under "refused" with its code.
int ma_weight_statistics(ma_str path, uint32_t threads, ma_progress_fn progress, void* context, char** out,
                         size_t* out_len) MA_NOEXCEPT;

void ma_free(char* p) MA_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#endif
