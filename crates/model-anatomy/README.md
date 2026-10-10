# model_anatomy — model morphometry's maths in C++

Alelyon's measurement engine, in C++ behind a C ABI with a safe Rust wrapper. It ports
the maths behind the Python Lattice desktop's Model Morphometry view so the Rust iced app
CENTCOM can show it without Python. Licensed Apache-2.0; `NOTICE` names the third-party
material in `cpp/` (tables and dequantisation derived from llama.cpp's gguf-py, MIT).

What PR 6 adds (the Morphometry tab's probe reads a GGUF model's actual weights; this is
the engine part only, CENTCOM does not show it yet):

- **Dequantisation** (`cpp/dequant.*`) of 22 GGUF tensor types: F32, F16, BF16, Q4_0,
  Q4_1, Q5_0, Q5_1, Q8_0, Q2_K, Q3_K, Q4_K, Q5_K, Q6_K, IQ4_NL, IQ4_XS, IQ3_S, IQ3_XXS,
  IQ2_XXS, IQ2_XS, IQ2_S, IQ1_S, IQ1_M. The reference is the Python package `gguf` 0.19.0
  (gguf-py, part of llama.cpp, https://github.com/ggml-org/llama.cpp, **MIT License,
  Copyright (c) 2023 Georgi Gerganov**): `gguf.quants.dequantize`. The block layouts are
  ported from its `quants.py`; its tables (GGML's type table, the IQ sign table, the
  IQ4_NL values and the IQ grids decoded from `grid_hex` as `init_grid` decodes them) are
  written into `cpp/gguf_quant_tables.hpp` by `tools/model_anatomy_goldens.py` from the
  installed package, with the MIT notice in its header comment, and `--check` fails when
  they drift. Parity is **bit for bit** on the float32 output: each type computes in
  float32 in gguf-py's order of operations (numpy multiplies and adds element by element,
  so `d * q - m` is two roundings, never a fused multiply-add; `d * (0.5 + s) * 0.25`
  associates left to right), and float16 is decoded from its bits exactly as numpy's
  `npy_halfbits_to_floatbits` does (subnormals normalised, NaN payloads kept unquieted;
  on this PC numpy's float16 cast agreed with that rule on all 65,536 patterns, measured
  2026-10-07). No type is refused for want of exactness. Every other GGML type (Q8_1,
  Q8_K, TQ1_0, TQ2_0, MXFP4, NVFP4, Q1_0, the integer types, F64) has no dequantiser
  here and is refused by name.
- **Weight statistics** (`cpp/weights.*`): `weight_statistics(path, progress, cancel)`
  opens a GGUF file read-only, parses its header in C++ (D17), and for each tensor reads
  its data at `align_up(end of header, general.alignment) + offset` (alignment default
  32), in reads of at most 4 MiB of whole blocks, dequantised 2^18 floats at a time:
  no tensor is materialised. Worker threads (`std::thread`, at most
  `hardware_concurrency() - 1`, at least 1; a request is capped by that) take tensors
  largest first from a shared queue, each with its own read-only handle. Accumulated per
  tensor and per canonical (block, module) cell (`tensor_block`/`tensor_module`, the
  morphometry analysis's own `block_of`/`module_of` after `strip()`):
  - `count` (every element), `non_finite` (NaN, +-inf), `zeros` (exact zeros, either
    sign); every other figure is over the n = count - non_finite finite values;
  - `min`, `max` (by value; the sign of a zero extreme is not specified);
  - `sum`, `sum_abs`, `sum_sq` (of x*x, exact in double for a float32 x), each a
    compensated sum in double (Ogita-Rump-Oishi Sum2: TwoSum per term, the errors summed;
    four interleaved partials per chunk merged in a fixed order);
  - derived: `mean = sum / n`, population `std = sqrt(max(sum_sq / n - mean^2, 0))`,
    `rms = sqrt(sum_sq / n)`, `mean_abs = sum_abs / n`, `l2 = sqrt(sum_sq)` (null when
    n = 0);
  - `histogram`: 32 bins of |x| over the finite NON-ZERO values on a log2 scale. Bin 0
    holds |x| < 2^-24 (float32 subnormals included); bin i for i = 1..30 holds
    2^(i-25) <= |x| < 2^(i-24); bin 31 holds |x| >= 2^6. Computed from the float32's
    exponent bits (bin = clamp(biased exponent - 102, 0, 31), 0 for a subnormal), so the
    bins sum to count - non_finite - zeros.
  Cells are combined from the tensors' accumulators in file order, so the figures do not
  depend on the thread count or the scheduling (the parity test runs one worker and the
  default and requires identical results). The progress callback (bytes of tensor data
  read, total) and the cancellation flag are consulted on the CALLING thread only, about
  every 100 ms; a cancelled run is refused `CANCELLED`. A panic in the Rust callback is
  caught at the boundary, cancels the run, and is resumed after the core returns.
- **Refusals by name.** The whole file: `OPEN_FAILED`, `NOT_GGUF`, `UNSUPPORTED_VERSION`
  (only GGUF 2 and 3 are read), `HEADER_TRUNCATED`, `MALFORMED_HEADER`, `READ_FAILED`,
  `CANCELLED`: `Ok` with `refusal` set and no figures. One tensor, listed in `refused`
  while every other tensor is still measured: `UNSUPPORTED_TYPE`, `TRUNCATED` (the file
  ends before the tensor's data does), `BAD_SHAPE` (ne0 not whole blocks, or a size past
  2^64), `READ_FAILED`. `complete` is true only with no refusal of either kind.
- **Goldens without model weights** (`tests/goldens/dequant/`, 22 files;
  `tests/goldens/weights/`, 3 files): seeded blocks of each type (24 random blocks with
  modest finite float16 scales, then an all-zero block, zero scales, the float16
  extremes +-65504, the smallest subnormals +-2^-24, unit scales and an all-0xFF block;
  for F32/F16/BF16 random values and the special bit patterns, NaN payloads included),
  dequantised by gguf-py, recorded as the input hex, the SHA-256 of the little-endian
  float32 output and sample values by their bits; and two small synthetic GGUF files
  written by gguf-py's `GGUFWriter` (raw quantized bytes with `raw_dtype`; every
  supported type once, a Q8_1 tensor to be refused, an F32 tensor with zeros, +-inf and
  NaN, one file at alignment 64) plus a truncated copy, with the statistics computed by
  gguf-py's `GGUFReader` and `dequantize`, numpy float64 and `math.fsum`.
- **The parity tolerance.** Dequantisation: the hashes must match EXACTLY. Statistics:
  counts, zeros, the histogram, min and max exactly; each float sum within 1e-12 of its
  scale (`sum` against `sum_abs`, its condition; `sum_abs` and `sum_sq` against
  themselves); `mean`, `mean_abs`, `rms` and `l2` within 1e-12 relative (the mean
  against `mean_abs`); `std` on its square against rms^2, since it is the root of a
  difference of two near quantities. Why 1e-12 and not exact: `math.fsum` is the
  correctly rounded sum, and Sum2's error bound is about u|S| + n u^2 sum|x| (u = 2^-53),
  far inside 1e-12 for any tensor here but not zero. In every case measured so far the
  sums were in fact bit-identical to `fsum` (the synthetic goldens and the four real
  tensors below).

Measured by hand on 2026-10-07 (`weight_statistics_of_the_file_named_by_the_environment`,
release build, a 32-logical-CPU workstation so 31 workers) on a local
`qwen3-coder-30b.gguf` (15,386,884,896 bytes; 579 tensors: 241 F32, 241 IQ4_XS, 48 Q5_K,
48 IQ3_S, 1 Q6_K): all 579 measured, none refused, 15,380,912,128 bytes of tensor data
in 7.93 s = 1.94 GB/s; peak working set 171.5 MiB, peak private bytes 225.1 MiB (the test
process, `K32GetProcessMemoryInfo`). How much of the file was already in the OS file cache
was not measured (a llama.cpp benchmark had loaded the same model about 30 minutes
earlier), so this is not a cold-disk figure. Four of its tensors (output_norm F32,
blk.0.attn_v Q5_K, blk.0.attn_q IQ4_XS, blk.0.ffn_gate_exps IQ3_S, 201,326,592 values)
were cross-checked against gguf-py's dequantize with numpy and `math.fsum` in a scratch
script: counts, zeros, min, max and histograms equal, and every sum bit-identical.

The C ABI (version 6) adds `ma_quant_types`, `ma_dequantize` and `ma_weight_statistics`
(with `ma_progress_fn`); the Rust side is `src/weights.rs`: `quant_types()`,
`dequantize(type_id, &[u8]) -> Vec<f32>`, `weight_statistics(path, progress:
Option<&dyn Fn(u64, u64)>, cancel: Option<&AtomicBool>) -> Result<WeightStatistics,
Error>` (`weight_statistics_with_threads` and `weight_statistics_json` take a thread
count too), `histogram_bin_lower_edge`.

What PR 5 adds (the economics, and the CPU package's energy counter):

- the Python reference's `economics.py` (`cpp/economics.*`):
  `throughput_from_llamacpp` (the runtime's own `predicted_n`/`predicted_ms` and
  `prompt_n`/`prompt_ms` counters, never a wall clock; where Python raises,
  `OverflowError` for an integer past the double range or `ZeroDivisionError` for a
  duration that divides to 0.0, the core returns that exception's class and message),
  `ThroughputReading`/`EnergyDeclaration`/`ProviderPrice`/`MonthlyVolume` and each one's
  `complete`, `EnergyDeclaration.usd_for_seconds`, `read_power` (the meter is called on
  the Rust side and its outcome handed to the core), `compare` with every
  `CostComparison` property, `describe`, and `STRUCTURAL_CAVEATS`. Every refusal,
  caveat, rule and `describe` sentence is Python's byte for byte (the em dashes
  included); a comparison takes a complete input record or refuses by name, with no
  default price, volume or wattage.
- **The arithmetic is Python's operation for operation**, so the goldens compare
  every float by its bits: `count / (float(millis) / 1e3)` with the int count converted
  once, correctly rounded (`std::from_chars`, as `PyLong_AsDouble` rounds);
  `(watts / 1000) * (seconds / 3600) * tariff`; `int(tokens) / MILLION` as Python's
  correctly rounded int / int (`py_truediv`), then one product; `tokens / rate` with the
  int64 converted once. `_sum_lines` is CPython 3.12's compensated `sum()`, which for the
  two lines every comparison has equals the plain sum (its correction term is the exact
  error of the second addition, which rounds back to that sum), so the port adds plainly;
  the goldens include sums with `inf` and `nan` lines. The `:g`, `:,`, `:,.1f` and `:,.2f`
  formats are `pyfmt`'s (`py_general`, `py_grouped`, `py_fixed_grouped`).
- **Non-finite floats travel.** Python's records can hold `inf` and `nan` (a NaN duration,
  an infinite price, a rate that overflows), so these results write a non-finite float
  as Python's repr text (`"inf"`, `"-inf"`, `"nan"`) rather than D5's `null`, and the Rust
  wrapper reads it back as that float. The goldens record them the same way.
- **The CPU package's cumulative energy** (`cpu_package_energy`, `cpp/probe.cpp`), D15.

The C ABI (version 5) adds `ma_throughput_from_llamacpp`, `ma_cost_compare`,
`ma_energy_usd_for_seconds`, `ma_read_power`, `ma_economics_constants` and
`ma_cpu_package_energy`; the Rust side is `src/economics.rs`. The goldens
(`tests/goldens/economics/`, 77 cases, and `economics_constants.json`) follow
`tests/oracle/test_model_foundry_economics.py`: the deepseek counters, each refusal by
name and all four together, each rate alone, zero and NaN rates, unsourced and undated
declarations, a zero and a negative tariff, fast and slow prefill (both rates used),
a volume past a month, local dearer than hosted, large-number formats, volumes past
2^53, non-finite prices and rates, every `throughput_from_llamacpp` malformation
(bool, float, text and None values, a non-mapping payload or timings, a repeated key,
the model name's precedence) and both of its exceptions, and every `read_power` outcome.
CENTCOM does not show it yet.

What PR 4 adds (the morphometry comparison):

- `morphometry_compare.compare(left, right)` (`cpp/compare.*`), in Python's order of
  checks: two measured operands (else `INPUT_UNMEASURED`, naming each side's refusal or
  "no measured cells"), the current schema version (`SCHEMA_MISMATCH`), a current producer
  source (`INVALID_RECORD`), each native axis order and its registration onto the
  canonical frame (`REGISTRATION_REFUSED`), then each side's cells (`INVALID_CELLS`: a
  negative block, a module outside the frozen `MODULE_IDS`, a non-positive parameter or
  tensor count, negative storage, routed parameters outside `[0, parameters]` or on a
  module other than `ffn_expert`, a duplicate coordinate). Compared cells are the union of
  both sides' coordinates, the stack-external row first, then by block and module. Signed
  deltas are right minus left; the relative parameter difference is the exact reduced
  rational `(right - left) / left` (int64 numerator and denominator, never a float); a cell
  on one side only (`LEFT_ONLY`/`RIGHT_ONLY`) carries no numeric delta, and a gap names
  how many there were. Active parameters keep `_active_parameters`' rule: incoherent
  routing is None (UNMEASURED). `ok` and `complete` are computed as the Python properties.
- No int64 deviation is needed: every compared count is validated non-negative (and
  parameters positive) before any arithmetic, so each delta, magnitude and reduced ratio
  fits in int64, and the active path computes `routed * used // total` as
  `(routed / total) * used` (exact, since `routed % total == 0`), which is at most
  `routed`. An operand `analyze` refused under D2 compares as `INPUT_UNMEASURED`.

The C ABI (version 4) adds `ma_compare` (two payloads, analysed by the core, then
compared) and `ma_compare_records` (two `ma_morph_record`s as given, which is how the
record-level refusals are reached); the Rust side is `src/compare.rs` (`compare`,
`compare_records`, `MorphometryRecord::from(&Morphometry)`). The goldens
(`tests/goldens/compare/`) hold 41 pairs, each side's record included: 15 from payloads
(identical models, dense of different depths, dense against MoE so the expert cells are
side-only, declared architecture against an inventory, module-major order, past-2^53
cells, unmeasured operands) and 26 records edited from those analyses to reach every
other code. CENTCOM does not show it yet.

What PR 3 adds (the Foundry, with the decisions of 2026-10-07):

- `footprint.shape_from_runtime_payload` (with `_int` and `_arch_value`, whose suffix
  match must be UNIQUE, unlike morphometry's first match), `kv_cache_bytes`,
  `max_context_for`, `footprint_at` and the `KV_PRECISIONS` / `DECLARED_BITS_PER_WEIGHT`
  tables, in exact int64 rationals (`cpp/foundry.*`);
- `fit.serving_reserve_fraction` (read from `ALELYON_HF_MEM_FRACTION` as `local_hf` reads
  it: unset, the reserve is `1.0 - 0.85` = 0.15000000000000002, OBSERVED; a text `float()`
  refuses or a cap outside (0, 1] is the DECLARED fallback 0.15), `assess` with
  `_backend_gate`, including the exact int-against-float `headroom < int(budget) * 0.10`;
- `foundry.derive_notes`, `catalog_from_runtime` (each verdict at `CONTEXT_LADDER[1]` =
  8,192), `runnable_here` (sorted by `(order, -(max_context or 0), id)`, stably) and
  `coresident_pairs` (a repeated id keeps the last model's size, as Python's dict does);
- `WorkstationReading`'s budget and `has_accelerator`, and `workstation.describe`;
- a native machine probe (`cpp/probe.*`, D10) that opens no device.

The C ABI (version 3) adds `ma_shape`, `ma_foundry`, `ma_describe` and `ma_probe`; the
Rust side is `src/foundry.rs`. CENTCOM's Lattice page shows it as its Foundry tab.

What PR 2 adds (decided 2026-10-07: only the reachable registration
path, plus a drift-checked facts table, not the whole transforms/registration modules):

- `registration.analyze_exact_compatibility`, its identity / axis-permutation subset
  (`cpp/registration.*`): in Python's order, the unsupported-topology, topology,
  unsupported-feature, metadata-gap, axis-count, space-key (`exact_space_key`, which
  leaves out `space_id`), axis-key multiset, policy matching (`_build_matching_problem`,
  `_unique_policy_matching`, with the edge-visit budget) and transform-constructor
  policy checks, returning the `CompatibilityReport` with Python's exact strings;
  `morphometry.register(morph)` on top. Morphometry's native and canonical spaces differ
  at most in axis order, so nothing else is reachable from it;
- `template_hierarchy.snapshot_template_hierarchy` and `model_morphometry_hierarchy`
  (`cpp/template_hierarchy.*`, Kahn's algorithm and every structural finding);
- `hierarchy_morphometry.measure_layers` (`cpp/hierarchy_morphometry.*`): one volume,
  shared by every frame that registers onto it, so `volumes_built` counts objects;
- the nine transform families' facts (`transform_type`, loss class, invertibility,
  docstring, `LOSS_CLASS_RANK`) as `generated/transform_facts.json`, written by the
  goldens tool from transforms.py and embedded by `src/frames.rs`: facts, not logic.

The full field set of `CoordinateAxis`/`CoordinateSpace` is now modelled and encoded
(`canonical.axis_bytes`), so any frame's commitment is computed, not only the model's.

What PR 1 ported (registration is PR 2, Foundry PR 3):

- the Python reference's `morphometry.py`: `analyze` and its helpers
  (`parse_tensor_inventory`, `_cells_from_tensors`, `_native_axis_order`,
  `_dense_decoder_cells`, `_active_path_gap`, `_arch_key`, `_routing_declaration`,
  `module_of`, `block_of`), every derived property of `ModelMorphometry`, `Cell` and
  `BlockProfile`, `voxel_field`, the constants, `canonical_space`, `native_space`
  and `space_commitment`;
- the part of `canonical.coordinate_space_bytes`/`axis_bytes` and
  `contracts.label_dictionary_ref` those reach, over a SHA-256 written here;
- `gguf_header.GGUFHeader.show_payload` (in Rust, `src/lib.rs`'s `gguf` module),
  taking the raw fields `lattice_core::llama::gguf::read_header` returns.

What the figures are: through PR 5, declared anatomy (a tensor inventory, or the declared
architecture), never a reading of weights or activations, same as the Python. PR 6's
weight statistics are a reading of the weights as stored in the file, never of
activations.

## Layout

| Path | What |
|---|---|
| `cpp/ma_abi.hpp` | the C ABI: plain structs in, UTF-8 JSON out, freed by `ma_free`; every entry point `noexcept` |
| `cpp/morphometry.*` | `analyze`, the derived properties, `voxel_field`, the result's JSON |
| `cpp/canonical_space.*` | coordinate spaces (every field), the canonical/native spaces, canonical bytes and references |
| `cpp/registration.*` | the reachable registration subset and its report |
| `cpp/template_hierarchy.*`, `cpp/hierarchy_morphometry.*` | the template hierarchy snapshot and the lineage layers |
| `cpp/foundry.*` | (PR 3) the shape, the footprint, the fit, the catalog, the pairs and `describe` |
| `cpp/probe.*` | (PR 3) the native machine probe: DXGI enumeration, memory, processors, disk, version |
| `src/foundry.rs` | the wrapper for those, the probe, and the environment it reads |
| `cpp/compare.*` | (PR 4) the morphometry comparison and its document |
| `src/compare.rs` | the wrapper for it: typed records in, `MorphometryComparison` out |
| `cpp/economics.*` | (PR 5) local against hosted cost, its refusals, caveats and sentences |
| `src/economics.rs` | the wrapper for it, and for the CPU package energy counter |
| `cpp/dequant.*` | (PR 6) dequantisation of 22 GGUF types, bit for bit as gguf-py 0.19.0 |
| `cpp/gguf_quant_tables.hpp` | (PR 6) GGML's type table and the IQ tables, generated from gguf-py (MIT notice inside); never edited by hand |
| `cpp/weights.*` | (PR 6) the C++ GGUF header reader and the streaming per-tensor and per-cell statistics |
| `src/weights.rs` | the wrapper for those: `dequantize`, `quant_types`, `weight_statistics` |
| `src/frames.rs` | the wrapper for those, and the embedded transform facts table |
| `generated/transform_facts.json` | written by `tools/model_anatomy_goldens.py` from transforms.py; never edited by hand |
| `cpp/pyfmt.*` | Python's `repr(float)`, `:.Nf`, `{:,}`, int/int true division, `int(str)` |
| `cpp/checked.hpp` | int64 arithmetic that refuses overflow (D2) |
| `cpp/sha256.*`, `cpp/json_writer.*`, `cpp/constants.*` | as named |
| `build.rs` | `cc` with C++20, MSVC `/EHsc /fp:precise /W4 /WX /permissive- /utf-8` (no `/arch:AVX2`, no fp-contract); links `dxgi.lib` on Windows |
| `src/lib.rs` | the safe Rust wrapper (the only `unsafe` is the FFI calls) and typed results |
| `tests/parity.rs` | the core against the Python goldens, structurally (floats by bits) |
| `tests/native.rs` | SHA-256 vectors (FIPS 180-2) and each deviation below |
| `tests/goldens/` | written by `tools/model_anatomy_goldens.py` from the real Python |

## Commands

```powershell
cargo test --offline --locked
# in the source repository, with its Python reference:
python tools/model_anatomy_goldens.py --check   # or without --check to rewrite
python -m pytest -q tests/frontend/test_model_anatomy_goldens.py
```

The probe, on this machine (no device is opened; it prints what it read):

```powershell
$env:VK_LOADER_DEVICE_ID_FILTER = "0x7550"
cargo test --offline --locked --test native the_probe_reads_this_machine -- --ignored --nocapture
```

The CPU package's energy counter (D15), read twice 1 s apart; it prints the joules and
the seconds between the counter's own timestamps. On this PC on 2026-10-07 it read
23,761,841,666 pWh = 85.543 J over 1.0149 s (84.29 W, the package at that moment):

```powershell
cargo test --offline --locked --test native the_cpu_package_energy_counter_increases -- --ignored --nocapture
```

PR 6's weight statistics on a whole model file, read-only (release build; prints the
elapsed time, bytes, GB/s, peak memory and a few cells; `MODEL_ANATOMY_WEIGHTS_JSON`
optionally names a file to keep the whole document in):

```powershell
$env:MODEL_ANATOMY_WEIGHTS_FILE = "$HOME\.alelyon\models\qwen3-coder-30b.gguf"
cargo test --release --offline --locked --test native weight_statistics_of_the_file_named_by_the_environment -- --ignored --nocapture
```

The accelerator's board power is not read by any probe: it stays DECLARED in an
`EnergyDeclaration`, by design.

`local_ci.py` runs the first as `model-anatomy` and the last as `model-anatomy-parity`.
A change to a ported Python rule fails the pytest until the goldens are regenerated,
and regenerating them fails `tests/parity.rs` until the C++ follows.

## DEVIATIONS

Each is a place where Python's answer is NOT the port's. None is recorded in a
golden (Python's answer would be the wrong expectation); `tests/native.rs` pins each.

- **D1 — the input surface.** The ABI carries what a GGUF header produces: `model_info`
  values tagged None/bool/int (as canonical decimal text)/f64/str, and tensors as a name,
  `u64` dimensions and a type name. Python's `analyze` also accepts payloads no header
  produces (a tensor entry that is not a mapping, a dimension given as text or a float,
  a `dtype` key, a negative dimension); those are not representable here.
- **D2 — integers are int64, checked.** Python's integers do not overflow. Here an
  integer input or intermediate outside int64 (a metadata integer, a dimension past
  2^63-1, a parameter product, a sum, nominal bytes, a declared-architecture product)
  makes the whole result the native refusal `INTEGER_OUT_OF_RANGE`, with one gap naming
  the quantity. Nothing is wrapped or approximated. Integers are parsed only where
  Python parses them, in Python's order. Rationals are int64 numerator/denominator.
- **D3 — text rules are ASCII-only.** `str.strip()` strips ASCII whitespace (with
  `\x1c`-`\x1f`); `lower()`/`upper()` change ASCII letters only; `int(str)` and the
  block regex's `\d` accept ASCII digits only. Python also strips U+00A0 and other
  Unicode spaces, lowercases U+212A KELVIN SIGN to `k`, and reads Arabic-Indic digits.
  The same rule as the Rust GGUF port's (lattice-core `gguf.rs`). Non-ASCII text passes
  through unchanged.
- **D4 — the declared-architecture path shares the block budget.** Python's fallback
  arithmetic makes one cell per declared block with no bound. Here a declared
  `block_count` above `MAX_BLOCKS` (4,096) is refused as `BLOCK_COUNT_NOT_DECLARED` with
  a gap naming D4, as the inventory path already refuses more than 4,096 blocks.
- **D5 — a non-finite float in the result is written `null`.** JSON has no spelling for
  it (Python writes `Infinity`/`NaN`). No reachable result carries one: coverage and
  active fraction divide by a positive count, intensities by a positive maximum.

- **D6 — a coordinate space is taken as the contract would construct it.** The ABI
  carries every field of `CoordinateAxis`/`CoordinateSpace`; the core checks that the
  text is UTF-8, that each enumeration is one of its values and that a space has 1 to 64
  axes, and sorts policies, region refs and metadata as the constructors do. It does not
  re-run the rest of the contract's validation (NFC, outer whitespace, label commitments,
  rational syntax, unique axis ids, size budgets): a space Python would refuse to
  construct gets an answer here instead of a `ValueError`.
- **D7 — the unreached registration rungs are not ported.** When the two spaces'
  axis-key multisets differ, Python tries the single-field rungs (ordering, orientation,
  label reindex, calendar, timezone, reference basis, unit; registration.py:1480-1600)
  and the composed rung (`_composed_rung`, :1149). Here that case is the native refusal
  `NOT_PORTED` (failing constraint `registration_rung_not_ported`), never a guess. No
  space morphometry builds reaches it.
- **D8 — template text is not NFC-normalised.** `template_hierarchy._text` applies
  `unicodedata.normalize("NFC", ...)`; here that step is a no-op (and `strip()` is
  ASCII-only, D3). A node built with a decomposed character keeps it; the built-in
  hierarchy is ASCII.

- **D9 — the Foundry's integers are int64, checked (PR 3).** As D2: a value or product
  outside int64 while building a shape (a metadata integer, `parameters x bits`, the KV
  bytes per token) refuses that shape as `INTEGER_OUT_OF_RANGE` (every figure None, one
  gap naming D9), so its fit is UNDECIDABLE; one later (the cache at the chosen context, a
  total, a pair's sum) refuses the whole Foundry document, which then holds no rows. A
  shape refuses even where Python would later have replaced the value it could not hold.
- **D10 — the native probe, and its "vulkan" backend (PR 3, decision D1 of
  2026-10-07).** Python reads the machine through torch, psutil, shutil and `platform`;
  the probe reads Windows instead and never opens a device. The accelerator is the adapter
  whose PCI device id is `VK_LOADER_DEVICE_ID_FILTER`'s (hex; 0x7550, AMD 1002, on this
  machine), found by DXGI enumeration (`CreateDXGIFactory1`, `EnumAdapters1`,
  `DXGI_ADAPTER_DESC1`: name, `DedicatedVideoMemory`, vendor and device ids). The variable
  unset, unparsable, matching no adapter or two leaves accelerator memory UNMEASURED with
  a gap: never the first adapter. Its backend is `"vulkan"`, which `has_accelerator` counts
  (Python's vocabulary is cuda/rocm/mps). Free accelerator memory is the adapter's
  `DedicatedVideoMemory` less what every process holds on it when read: the counter Task
  Manager reads, `\GPU Adapter Memory(luid_<high>_<low>_phys_0)\Dedicated Usage`, one PDH
  sample keyed by the adapter's LUID (Python asks torch's `mem_get_info`, also a
  whole-device figure; DXGI's `QueryVideoMemoryInfo` is a process budget, a different
  quantity, and is not read). A counter that does not read, or reads more than the adapter
  holds, leaves it UNMEASURED with a gap. The architecture, the compute units and bfloat16
  are UNMEASURED. RAM is `GlobalMemoryStatusEx`; CPU
  counts are `GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)` and the
  `RelationProcessorCore` records of `GetLogicalProcessorInformationEx` (psutil's own
  sources); free disk is `GetDiskFreeSpaceExW`'s total free bytes at the root of the home
  folder (the value CPython's `nt._getdiskusage` passes `shutil.disk_usage`, by its
  source, rather than the bytes available to the caller; on this PC, 2026-10-07, the two
  were equal, so that run could not tell them apart); the platform is `RtlGetVersion`'s `Windows major.minor.build`,
  not `platform.platform()`'s text. `describe` writes the backend row OBSERVED when the
  reading names the probe that chose it (Python: only when torch reported a version).
  Python's goldens keep Python's vocabulary; `tests/native.rs` pins all of this.
- **D11 — the declared margin is not ported (PR 3).** No ported caller
  (`catalog_from_runtime`, `runnable_here`, `coresident_pairs`, the panel) passes a
  `ServingMargin`, a KV precision other than the default `f16`, or a measured throughput, so
  `footprint.margin` is always None, the cache is always 16-bit and no throughput note is
  written. `max_context_for`'s fractional-margin branch (an exact `Fraction` of a float) is
  therefore not reached.
- **D12 — the cap is read per call (PR 3).** `local_hf` reads `ALELYON_HF_MEM_FRACTION`
  once, when it is first imported; the port reads the text it is given each time (the
  Rust side reads the variable at each measurement). `float()`'s text rules are ASCII
  (D3): `" 0.5"` is 0.5 to Python and the declared fallback here.

- **D13 — the comparison's input is a typed record (PR 4).** `compare` reads a
  `ModelMorphometry`, whose fields Python does not type-check at construction, so it
  checks them itself: `_record_problem` (non-text `model`/`source`/`refusal`/
  `schema_version`, cells or gaps that are not a tuple, a gap that is not text), a cell
  that is not a `Cell`, a block, count or storage that is a `bool` or a float, element
  types that are not a tuple of text, an axis order that is not a pair of text, and the
  `KeyError`/`TypeError`/`ValueError` registration path. The ABI carries text as text,
  counts as int64 and the axis order as two strings, so none of those inputs exists here
  and that `INVALID_RECORD` reason is never produced (the invalid-source one is). A null
  record pointer is `MA_INVALID_INPUT`, where Python raises `TypeError` for an operand
  that is not a `ModelMorphometry`. The types are the pin: safe Rust cannot build those
  inputs, so `tests/native.rs` has no case for D13; it pins instead that a D2-refused
  operand compares as unmeasured and that the int64 extremes compare exactly.

- **D14 — the economics' records are typed (PR 5).** Python's dataclasses do not check
  their fields' types. Here every rate, wattage, tariff and price is an `f64` (an int
  given to Python is used as `float(x)` and formatted by `:g` through a float, so the
  answer is the same except for an int past the double range, where Python's
  `float()` raises and this cannot be given); token volumes and the context are int64
  (Python's are unbounded, and a float volume, which Python would format as `1,000.0`,
  cannot be given). A llama-server response's `model` and `timings` values are the
  tagged None/bool/int/float/str of D1: a nested list or mapping as a timing is not
  representable (Python reads it as "not an int", the same None a string gets). The
  meter of `read_power` is a Rust closure whose `Err` stands for Python's raised
  exception. A record built field by field (`CostComparison(...)` with lines it did
  not compute) is not ported: the properties exist on `compare`'s own result. No golden
  holds a case outside these types.
- **D15 — the CPU package's energy counter (PR 5, no Python counterpart).**
  `cpu_package_energy()` reads one raw sample of Windows' counter
  `\Energy Meter(RAPL_Package0_PKG)\Energy` through PDH (`PdhAddEnglishCounterW`,
  `PdhGetRawCounterValue`): the package's cumulative energy in picowatt-hours with the
  sample's own FILETIME. It opens no device. The counter is cumulative from no stated
  origin, so only a difference of two samples is an energy (1 pWh = 3.6e-9 J). An
  absent counter set, instance or counter, or an invalid sample, is `Absent` with the
  reason, never a zero. It is not wired into `read_power` or `EnergyDeclaration`: it
  measures the CPU package only, not the accelerator, whose board power stays DECLARED
  by design. `tests/native.rs`'s `the_cpu_package_energy_counter_increases`
  (ignored; run by hand) reads it twice, 1 s apart.

- **D16 — the weight statistics have no Python counterpart in the tree (PR 6).** Their
  expectation is computed by the goldens tool itself (gguf-py's dequantize, numpy float64,
  `math.fsum`, morphometry's `block_of`/`module_of`), and the float sums are held to the
  1e-12 tolerance above rather than to bits: the core's compensated double sums are not
  `fsum`'s correctly rounded sum by construction, only (so far, measured) equal to it.
  Dequantisation itself is held bit for bit and is not a deviation.
- **D17 — the GGUF header is read in C++ (PR 6), and stricter than gguf-py's reader.**
  The crate had no C++ reader (PR 1 takes lattice-core's header fields). Here: only GGUF
  versions 2 and 3, little-endian (a big-endian file reads as an unsupported version);
  `general.alignment` must be a uint32 power of two (gguf-py also requires uint32);
  at most 4 dimensions (GGML_MAX_DIMS); metadata arrays nested at most 8 deep; tensor
  names must be UTF-8; counts and lengths are bounded by the bytes left in the file
  before anything is allocated. A tensor's offset is not required to be aligned. Any
  metadata value other than the alignment is skipped, not read. `tests/native.rs` pins
  each whole-file refusal.

Exactness, not a deviation: every Python `int / int` (a voxel's intensity, a float of a
`Fraction`) is the exact quotient rounded once to the nearest double, and the
dispersion rule's `int != float` is an exact comparison, as in Python; the
`huge_parameters` golden holds cells past 2^53 where a naive `double / double` differs.
The Foundry's `headroom < int(budget) * 0.10` and its `int(total * (1.0 - keep))` keep
Python's mixed arithmetic the same way: the int made a double, one product, an exact
int-against-double comparison, a truncation; `Fraction`s are reduced int64 pairs
(`Q4_K_M`'s 485/100 is 97/20, as Python holds it).
