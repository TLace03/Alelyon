// A port of the Foundry's arithmetic (PR 3):
// the Python reference's `footprint.py` (`shape_from_runtime_payload`,
// `kv_cache_bytes`, `max_context_for`, `footprint_at`), `fit.py`
// (`serving_reserve_fraction`, `assess`), `foundry.py` (`derive_notes`,
// `catalog_from_runtime`, `runnable_here`, `coresident_pairs`) and
// `workstation.py` (`WorkstationReading`'s budget and `describe`).
//
// Integers are int64 with checked arithmetic (D2, D9); text rules are ASCII-only
// (D3). The declared serving margin is not ported (D11): no ported caller passes
// one. See the crate README's DEVIATIONS.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "morphometry.hpp"

namespace ma {

class JsonWriter;

inline constexpr const char* kObserved = "OBSERVED";
inline constexpr const char* kDerived = "DERIVED";
inline constexpr const char* kDeclared = "DECLARED";
inline constexpr const char* kUnmeasured = "UNMEASURED";

// `workstation.GIB`.
inline constexpr int64_t kGib = int64_t{1} << 30;

// `footprint.ModelShape`, plus the native refusal (D9).
struct Shape {
    std::string model;
    std::optional<int64_t> layers;
    std::optional<int64_t> kv_heads;
    std::optional<int64_t> key_length;
    std::optional<int64_t> value_length;
    std::optional<int64_t> context_max;
    std::optional<int64_t> weight_bytes;
    std::string weight_provenance = kUnmeasured;
    std::optional<int64_t> parameters;
    std::string quantization;
    std::string architecture;
    std::optional<bool> mixture_of_experts;
    std::optional<int64_t> active_parameters;
    std::vector<std::string> requires_backends;
    std::vector<std::string> gaps;
    // `kv_bytes_per_token`, computed once with the shape (D9: a product past
    // int64 refuses the shape).
    std::optional<int64_t> kv_bytes_per_token;
    // Empty, or INTEGER_OUT_OF_RANGE (D9).
    std::string refusal;

    bool kv_established() const { return layers && kv_heads && key_length && value_length; }
};

// `shape_from_runtime_payload(payload, model=model)`; `payload` null is a
// payload that is not a mapping.
Shape shape_from_runtime_payload(const std::string& model, const Payload* payload);

// The reading the ported functions take: `WorkstationReading`, its primary
// device only (the only one any ported function reads).
struct Reading {
    std::string backend = "absent";
    std::string torch_version;
    // The native probe's name; empty for a reading built in Python (D10).
    std::string probe;
    bool has_gpu = false;
    std::string gpu_name;
    std::optional<int64_t> gpu_total_bytes;
    std::optional<int64_t> gpu_free_bytes;
    std::optional<int64_t> ram_total_bytes;
    std::optional<int64_t> disk_free_bytes;
    std::optional<bool> bf16;
    std::vector<std::string> gaps;

    // `has_accelerator`; "vulkan" counts (D10, decided 2026-10-07).
    bool has_accelerator() const;
    std::optional<int64_t> vram_total_bytes() const { return has_gpu ? gpu_total_bytes : std::nullopt; }
};

// `fit.serving_reserve_fraction()` with `ALELYON_HF_MEM_FRACTION` given as its
// text (`env` null: unset), read as `local_hf` reads it.
struct Reserve {
    double fraction = 0.15;
    std::string provenance = kDeclared;
    std::string basis;  // native: where the figure came from, in words
};
Reserve serving_reserve_fraction(const std::string* env);

// One model the runtime lists (`catalog_from_runtime`'s `names` and `show`).
struct FoundryModel {
    std::string name;
    std::optional<Payload> payload;      // empty: `show` returned None or raised
    std::optional<std::string> raised;   // the exception class `show` raised
    std::vector<std::string> requires_backends;
};

// `catalog_from_runtime(names, show, reading)`, then `runnable_here(models,
// reading, context=context)` and `coresident_pairs(models, reading,
// context=context)`, as one JSON document.
std::string foundry_json(const Reading& reading, const std::vector<FoundryModel>& models, int64_t context,
                         const std::string* mem_fraction_env, const std::string& source, const std::string& as_of);

// `workstation.describe(reading)` and the reading's gaps.
std::string describe_json(const Reading& reading);

// `ModelShape` as JSON.
std::string shape_json(const std::string& model, const Payload* payload);

// The Foundry's constants, written into `ma_constants`' document.
void write_foundry_constants(JsonWriter& json);

}  // namespace ma
