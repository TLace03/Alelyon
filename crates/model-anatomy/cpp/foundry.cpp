#include "foundry.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <iterator>
#include <tuple>
#include <map>
#include <string_view>
#include <utility>

#include "checked.hpp"
#include "constants.hpp"
#include "json_writer.hpp"
#include "pyfmt.hpp"

namespace ma {

namespace {

// An em dash, as Python's sources write it (U+2014).
const std::string kDash = "\xE2\x80\x94";

struct Fraction {
    int64_t num;
    int64_t den;
};

// `footprint.KV_PRECISIONS`, in its order.
struct NamedFraction {
    const char* name;
    Fraction value;
};
constexpr NamedFraction kKvPrecisions[] = {
    {"f32", {4, 1}}, {"f16", {2, 1}}, {"bf16", {2, 1}}, {"q8_0", {1, 1}}, {"q4_0", {1, 2}},
};
// `footprint.DEFAULT_KV_PRECISION`: the only precision a ported caller uses (D11).
constexpr const char* kDefaultKvPrecision = "f16";

// `footprint.DECLARED_BITS_PER_WEIGHT`, in its order, each reduced as Python's
// Fraction reduces it (485/100 is 97/20).
constexpr NamedFraction kDeclaredBitsPerWeight[] = {
    {"F32", {32, 1}},       {"FP32", {32, 1}},     {"F16", {16, 1}},      {"FP16", {16, 1}},
    {"BF16", {16, 1}},      {"Q8_0", {17, 2}},     {"Q6_K", {105, 16}},   {"Q5_K_M", {569, 100}},
    {"Q4_K_M", {97, 20}}, {"Q4_0", {9, 2}},      {"Q3_K_M", {391, 100}}, {"INT8", {8, 1}},
    {"INT4", {4, 1}},       {"MXFP4", {17, 4}},
};

// `fit.py`'s constants.
constexpr const char* kRuns = "RUNS";
constexpr const char* kTight = "TIGHT";
constexpr const char* kExceeds = "EXCEEDS";
constexpr const char* kUndecidable = "UNDECIDABLE";
constexpr double kFallbackReserveFraction = 0.15;
constexpr double kTightHeadroomFraction = 0.10;
constexpr double kCpuReserveFraction = 0.35;
constexpr int64_t kContextLadder[] = {2048, 8192, 32768, 131072};

// `local_hf._MEM_FRACTION`'s default text.
constexpr const char* kMemFractionDefault = "0.85";

// `foundry.py`'s note kinds and the one role `catalog_from_runtime` assigns.
constexpr const char* kStrength = "STRENGTH";
constexpr const char* kWeakness = "WEAKNESS";
constexpr const char* kCaveat = "CAVEAT";
constexpr const char* kRoleGeneralist = "generalist";
constexpr double kMoeActiveShare = 0.5;

std::optional<Fraction> lookup(const NamedFraction* table, size_t n, const std::string& name) {
    for (size_t i = 0; i < n; ++i) {
        if (name == table[i].name) return table[i].value;
    }
    return std::nullopt;
}

// `kv_element_bytes(precision)`: `str(precision or "").strip().lower()`.
std::optional<Fraction> kv_element_bytes(const std::string& precision) {
    return lookup(kKvPrecisions, std::size(kKvPrecisions), ascii_lower(py_strip(precision)));
}

std::optional<Fraction> declared_bits(const std::string& quantization) {
    return lookup(kDeclaredBitsPerWeight, std::size(kDeclaredBitsPerWeight), ascii_upper(quantization));
}

bool truthy(const std::optional<int64_t>& value) { return value && *value != 0; }

std::string gib_text(int64_t bytes) { return py_fixed(py_truediv(bytes, kGib), 2); }

// ── shapes ──────────────────────────────────────────────────────────────────

using Item = std::pair<std::string, Value>;

bool ends_with(std::string_view text, std::string_view suffix) {
    return text.size() >= suffix.size() && text.substr(text.size() - suffix.size()) == suffix;
}

// `_arch_value`: the direct key's value when it is not None, else the value of
// the ONE key ending in ".<suffix>" (none, or two or more, is None). Unlike
// morphometry's `_arch_key`, `general.` keys are not skipped and a second match
// is refused rather than the first taken.
const Item* arch_value(const Info& info, const std::string& architecture, const std::string& suffix) {
    if (!architecture.empty()) {
        const Item* direct = info.find(architecture + "." + suffix);
        if (direct != nullptr && direct->second.tag != Value::Tag::None) return direct;
    }
    const std::string tail = "." + suffix;
    const Item* found = nullptr;
    for (const auto& item : info.items()) {
        if (!ends_with(item.first, tail)) continue;
        if (found != nullptr) return nullptr;
        found = &item;
    }
    return found;
}

std::optional<int64_t> arch_int(const Info& info, const std::string& architecture, const std::string& suffix) {
    const Item* item = arch_value(info, architecture, suffix);
    return item == nullptr ? std::nullopt : to_int(&item->second, item->first);
}

std::string shape_name(const std::string& model, const Payload* payload) {
    if (payload == nullptr) return model;  // `str(model or "")`, unstripped
    return std::string(py_strip(!model.empty() ? model : payload->model));
}

Shape build_shape(const std::string& model, const Payload& payload) {
    const Info& info = payload.info;
    const Value* architecture_value = info.get("general.architecture");
    const std::string architecture =
        truthy(architecture_value) ? std::string(py_strip(py_str(*architecture_value))) : std::string();

    Shape shape;
    std::vector<std::string>& gaps = shape.gaps;
    std::optional<int64_t> weight_bytes;
    std::string weight_provenance = kUnmeasured;
    std::optional<int64_t> parameters = to_int(info.get("general.parameter_count"), "general.parameter_count");
    std::string quantization(py_strip(payload.quantization_level));
    std::optional<bool> moe;
    std::optional<int64_t> active;
    std::optional<int64_t> layers = arch_int(info, architecture, "block_count");

    // `MM.analyze(payload, model=model)`: the weights term is morphometry's.
    const Morphometry morph = analyze(model, &payload);
    const Derived derived = derive(morph);
    if (derived.ok) {
        weight_bytes = derived.nominal_bytes;
        if (weight_bytes) {
            weight_provenance = kObserved;
        } else {
            gaps.emplace_back(
                "the inventory contains an element type with no published bits per weight, so stored size is "
                "UNMEASURED rather than partial");
        }
        if (truthy(morph.declared_parameters)) parameters = morph.declared_parameters;
        if (!morph.quantization.empty()) quantization = morph.quantization;
        if (truthy(morph.block_count)) layers = morph.block_count;
        moe = derived.is_mixture_of_experts;
        active = derived.active_parameters;
        for (const auto& gap : morph.gaps) {
            if (std::find(gaps.begin(), gaps.end(), gap) == gaps.end()) gaps.push_back(gap);
        }
    }

    if (!weight_bytes && truthy(parameters)) {
        if (const auto bits = declared_bits(quantization)) {
            // `int(Fraction(parameters) * bits / 8)`: truncated toward zero.
            weight_bytes = mul(*parameters, bits->num, "parameters x declared bits per weight") / (bits->den * 8);
            weight_provenance = kDerived;
        } else if (!quantization.empty()) {
            gaps.push_back("no published bits per weight for quantization '" + quantization +
                           "'; stored size is UNMEASURED rather than rounded to a neighbouring format");
        }
    }

    std::optional<int64_t> kv_heads = arch_int(info, architecture, "attention.head_count_kv");
    if (!kv_heads) {
        const auto heads = arch_int(info, architecture, "attention.head_count");
        if (heads) {
            kv_heads = heads;
            gaps.emplace_back(
                "the runtime declares no separate key/value head count; the cache is sized as multi-head "
                "attention, which OVERSTATES it on a grouped-query model");
        }
    }
    std::optional<int64_t> key_length = arch_int(info, architecture, "attention.key_length");
    std::optional<int64_t> value_length = arch_int(info, architecture, "attention.value_length");
    const auto embedding = arch_int(info, architecture, "embedding_length");
    const auto heads = arch_int(info, architecture, "attention.head_count");
    if (!key_length && truthy(embedding) && truthy(heads)) {
        key_length = floordiv(*embedding, *heads, "embedding_length // head_count");
        gaps.emplace_back(
            "per-head key length was not declared; it is DERIVED as embedding_length / head_count, which is "
            "wrong on an architecture whose head dimension is not that ratio");
    }
    if (!value_length && key_length) {
        value_length = key_length;
        gaps.emplace_back(
            "per-head value length was not declared; it is assumed equal to the key length, which is false on a "
            "latent-attention architecture");
    }

    shape.model = shape_name(model, &payload);
    shape.layers = layers;
    shape.kv_heads = kv_heads;
    shape.key_length = key_length;
    shape.value_length = value_length;
    shape.context_max = arch_int(info, architecture, "context_length");
    shape.weight_bytes = weight_bytes;
    shape.weight_provenance = weight_provenance;
    shape.parameters = parameters;
    shape.quantization = quantization;
    shape.architecture = architecture;
    shape.mixture_of_experts = moe;
    shape.active_parameters = active;
    if (shape.kv_established()) {
        const char* what = "the KV cache bytes per token";
        shape.kv_bytes_per_token = mul(mul(*layers, *kv_heads, what), add(*key_length, *value_length, what), what);
    }
    return shape;
}

// ── footprints ──────────────────────────────────────────────────────────────

struct Term {
    std::string label;
    std::optional<int64_t> bytes;
    std::string provenance;
    std::string rule;
};

struct Footprint {
    int64_t context = 0;
    std::string kv_precision;
    Term weights;
    Term kv_cache;

    // No margin is ever present (D11), so these are the two terms.
    std::optional<int64_t> total_bytes() const {
        if (!weights.bytes || !kv_cache.bytes) return std::nullopt;
        return add(*weights.bytes, *kv_cache.bytes, "the footprint's total");
    }
    std::vector<std::string> missing_terms() const {
        std::vector<std::string> out;
        if (!weights.bytes) out.push_back(weights.label);
        if (!kv_cache.bytes) out.push_back(kv_cache.label);
        return out;
    }
};

// `kv_cache_bytes(shape, context, precision=...)`.
std::optional<int64_t> kv_cache_bytes(const Shape& shape, int64_t context, const std::string& precision) {
    const auto element = kv_element_bytes(precision);
    if (!shape.kv_bytes_per_token || !element || context <= 0) return std::nullopt;
    const char* what = "the KV cache at this context";
    // `int(Fraction(per_token) * context * element)`: truncated toward zero.
    return mul(mul(*shape.kv_bytes_per_token, context, what), element->num, what) / element->den;
}

// `max_context_for(shape, budget, precision=...)` with no margin (D11).
std::optional<int64_t> max_context_for(const Shape& shape, std::optional<int64_t> budget, const std::string& precision) {
    const auto element = kv_element_bytes(precision);
    if (!budget || !shape.kv_bytes_per_token || !element || !shape.weight_bytes) return std::nullopt;
    const char* what = "the largest fitting context";
    // per_token_bytes = per_token * num / den; den > 0, so its sign is per_token * num's.
    const int64_t per_token_num = mul(*shape.kv_bytes_per_token, element->num, what);
    if (per_token_num <= 0) return std::nullopt;
    // With no margin both limits are `budget - weights`.
    const int64_t spare = sub(*budget, *shape.weight_bytes, what);
    if (spare <= 0) return 0;
    // `int(spare / per_token_bytes)`, both positive: floor(spare * den / per_token_num).
    int64_t context = mul(spare, element->den, what) / per_token_num;
    if (truthy(shape.context_max)) context = std::min(context, *shape.context_max);
    return std::max<int64_t>(0, context);
}

// `footprint_at(shape, context, precision=...)`.
Footprint footprint_at(const Shape& shape, int64_t context, const std::string& precision) {
    Footprint fp;
    fp.context = context;
    fp.kv_precision = precision;
    const auto kv = kv_cache_bytes(shape, context, precision);
    const auto element = kv_element_bytes(precision);
    std::string kv_rule;
    if (shape.kv_established() && element) {
        kv_rule = py_str(shape.layers) + " layers x " + py_grouped(context) + " tokens x " + py_str(shape.kv_heads) +
                  " kv heads x (" + py_str(shape.key_length) + "+" + py_str(shape.value_length) + ") x " +
                  py_repr(py_truediv(element->num, element->den)) + "B";
    } else {
        kv_rule = "an architecture dimension or the cache precision is UNMEASURED";
    }
    std::string weights_rule;
    if (shape.weight_provenance == kObserved) {
        weights_rule = "summed over the runtime's tensor inventory";
    } else if (shape.weight_provenance == kDerived) {
        weights_rule = "parameters x declared bits per weight / 8";
    } else {
        weights_rule = "no inventory and no usable declared quantization";
    }
    fp.weights = Term{"Weights", shape.weight_bytes, shape.weight_provenance, weights_rule};
    fp.kv_cache = Term{"KV cache @ " + py_grouped(context) + " tokens", kv, kv ? kDerived : kUnmeasured, kv_rule};
    return fp;
}

// ── fits ────────────────────────────────────────────────────────────────────

struct Fit {
    std::string verdict;
    Footprint footprint;
    std::optional<int64_t> budget_bytes;
    std::string budget_basis;
    std::string budget_provenance = kUnmeasured;
    std::optional<int64_t> max_context;
    std::vector<std::string> reasons;

    std::optional<bool> fits() const {
        if (verdict == kUndecidable) return std::nullopt;
        return verdict == kRuns || verdict == kTight;
    }
    std::optional<int64_t> headroom_bytes() const {
        const auto total = footprint.total_bytes();
        if (!total || !budget_bytes) return std::nullopt;
        return sub(*budget_bytes, *total, "the headroom");
    }
};

std::string join(const std::vector<std::string>& items, std::string_view separator) {
    std::string out;
    for (size_t i = 0; i < items.size(); ++i) {
        if (i != 0) out.append(separator);
        out += items[i];
    }
    return out;
}

// `WorkstationReading.serving_budget_bytes(reserve_fraction)`.
std::optional<int64_t> serving_budget_bytes(const Reading& reading, double reserve_fraction) {
    const auto total = reading.vram_total_bytes();
    if (!total) return std::nullopt;
    // `max(0.0, min(1.0, float(reserve_fraction)))`, Python's argument order.
    const double low = reserve_fraction < 1.0 ? reserve_fraction : 1.0;
    const double keep = low > 0.0 ? low : 0.0;
    // `int(total * (1.0 - keep))`: the int made a float, the product rounded, then truncated.
    const double product = static_cast<double>(*total) * (1.0 - keep);
    if (!(product < 9223372036854775808.0) || product < -9223372036854775808.0) {
        overflow("the serving budget");
    }
    return static_cast<int64_t>(product);
}

// `_backend_gate(shape, reading)`: (admitted, reason); admitted empty is undecided.
std::pair<std::optional<bool>, std::string> backend_gate(const Shape& shape, const Reading& reading) {
    const auto& required = shape.requires_backends;
    if (required.empty()) {
        return {true,
                "no backend restriction is declared for this checkpoint; that is an absent declaration, not "
                "established portability"};
    }
    if (reading.backend == "absent") {
        return {std::nullopt, "this machine's compute backend is UNMEASURED, so a checkpoint requiring " +
                                  join(required, ", ") + " can be neither admitted nor refused"};
    }
    if (std::find(required.begin(), required.end(), reading.backend) != required.end()) {
        return {true, "this machine's backend (" + reading.backend + ") is among the " +
                          std::to_string(required.size()) + " the checkpoint declares"};
    }
    return {false, "the checkpoint declares it needs " + join(required, ", ") + "; this machine's backend is " +
                       reading.backend + ". No amount of memory changes this."};
}

// `assess(shape, reading, context=context)` with the default precision and no margin.
Fit assess(const Shape& shape, const Reading& reading, int64_t context, const Reserve& reserve) {
    Fit fit;
    fit.footprint = footprint_at(shape, context, kDefaultKvPrecision);
    const auto [admitted, backend_reason] = backend_gate(shape, reading);
    fit.reasons.push_back(backend_reason);
    if (admitted.has_value() && !*admitted) {
        fit.verdict = kExceeds;
        fit.budget_basis = "not evaluated: the backend gate refused first";
        fit.max_context = 0;
        return fit;
    }
    if (!admitted.has_value()) {
        fit.verdict = kUndecidable;
        fit.budget_basis = "not evaluated: the backend gate is undecided";
        return fit;
    }

    std::optional<int64_t> budget;
    if (reading.has_accelerator()) {
        budget = serving_budget_bytes(reading, reserve.fraction);
        fit.budget_basis = py_fixed(100 * (1 - reserve.fraction), 0) + "% of accelerator memory; " +
                           py_fixed(100 * reserve.fraction, 0) +
                           "% is reserved for everything else on the device, the desktop compositor above all";
        fit.budget_provenance = budget ? (reading.vram_total_bytes() ? kObserved : kUnmeasured) : kUnmeasured;
        if (reserve.provenance == kDeclared) {
            fit.reasons.emplace_back(
                "the loader's own memory cap could not be read; the reserve below is this module's declared "
                "fallback and may not match what the loader will do");
        }
    } else {
        if (reading.ram_total_bytes) {
            const double product = static_cast<double>(*reading.ram_total_bytes) * (1.0 - kCpuReserveFraction);
            if (!(product < 9223372036854775808.0) || product < -9223372036854775808.0) overflow("the CPU budget");
            budget = static_cast<int64_t>(product);
        }
        fit.budget_basis = py_fixed(100 * (1 - kCpuReserveFraction), 0) + "% of system memory " + kDash +
                           " there is no accelerator, so this is a CPU fit and will be far slower than any figure "
                           "on this surface suggests";
        fit.budget_provenance = budget ? kObserved : kUnmeasured;
        if (reading.backend == "cpu") {
            fit.reasons.emplace_back(
                "no accelerator is available to the runtime; the budget below is system RAM and throughput is "
                "UNMEASURED on this path");
        }
    }
    fit.budget_bytes = budget;
    fit.max_context = max_context_for(shape, budget, kDefaultKvPrecision);
    const auto total = fit.footprint.total_bytes();

    if (!total || !budget) {
        auto missing = fit.footprint.missing_terms();
        if (missing.empty()) missing.emplace_back("the memory budget");
        fit.reasons.push_back("UNMEASURED: " + join(missing, ", ") +
                              ". A footprint missing a term is not a smaller footprint, so no verdict is produced.");
        fit.verdict = kUndecidable;
        return fit;
    }

    const int64_t headroom = sub(*budget, *total, "the headroom");
    if (headroom < 0) {
        if (headroom == kI64Min) overflow("the headroom");
        std::string reason = "the footprint exceeds the budget by " + gib_text(-headroom) + " GiB at " +
                             py_grouped(context) + " tokens";
        if (truthy(fit.max_context)) {
            reason += "; it fits at " + py_grouped(*fit.max_context) + " tokens";
        } else {
            reason += "; it does not fit at any context";
        }
        fit.reasons.push_back(reason);
        fit.verdict = kExceeds;
    } else if (py_int_less_float(headroom, static_cast<double>(*budget) * kTightHeadroomFraction)) {
        // `headroom < int(budget) * TIGHT_HEADROOM_FRACTION`: an int against a float, compared exactly.
        fit.reasons.push_back("it fits with " + gib_text(headroom) + " GiB spare, under the " +
                              py_fixed(100 * kTightHeadroomFraction, 0) +
                              "% of budget this policy calls comfortable");
        fit.verdict = kTight;
    } else {
        fit.reasons.push_back("it fits with " + gib_text(headroom) + " GiB spare");
        fit.verdict = kRuns;
    }
    for (const auto& gap : shape.gaps) fit.reasons.push_back("shape gap: " + gap);
    return fit;
}

// ── capability notes ────────────────────────────────────────────────────────

struct Note {
    std::string claim;
    std::string kind;
    std::string provenance;
};

std::string billions(int64_t n) { return py_fixed(static_cast<double>(n) / 1e9, 1); }

// `derive_notes(shape, verdict)` (no measured throughput: no ported caller has one, D11).
std::vector<Note> derive_notes(const Shape& shape, const Fit* verdict) {
    std::vector<Note> notes;
    const auto& stored = shape.parameters;
    const auto& active = shape.active_parameters;
    const bool moe = shape.mixture_of_experts.value_or(false);
    if (moe && truthy(stored) && truthy(active) && *stored > 0) {
        // `active / float(stored)`: both made floats, then divided.
        const double share = static_cast<double>(*active) / static_cast<double>(*stored);
        if (share < kMoeActiveShare) {
            notes.push_back({"routed: stores " + billions(*stored) + "B parameters and activates " +
                                 billions(*active) + "B per token, so it costs a " + billions(*stored) +
                                 "B's memory at nearer a " + billions(*active) + "B's decode cost",
                             kStrength, kDerived});
        }
    } else if (moe && truthy(stored) && !active) {
        notes.push_back({"routed, but the runtime declares no usable expert partition, so the active path is "
                         "UNMEASURED and the decode cost cannot be separated from the storage cost",
                         kCaveat, kUnmeasured});
    }

    std::optional<double> observed_bits;
    if (shape.weight_provenance == kObserved && truthy(shape.weight_bytes) && truthy(shape.parameters)) {
        // `weight_bytes * 8 / float(parameters)`: float(w * 8) is float(w) * 8 exactly (a power of two).
        observed_bits = static_cast<double>(*shape.weight_bytes) * 8.0 / static_cast<double>(*shape.parameters);
    }
    const auto nominal = shape.quantization.empty() ? std::nullopt : declared_bits(shape.quantization);
    if (observed_bits && *observed_bits < 16) {
        notes.push_back({"labelled " + (shape.quantization.empty() ? std::string("quantized") : shape.quantization) +
                             " and measured at " + py_fixed(*observed_bits, 2) +
                             " bits per weight over its own tensor inventory: a lossy format. The quality cost is "
                             "UNMEASURED here " +
                             kDash + " nothing in this product has evaluated it",
                         kCaveat, kObserved});
    } else if (nominal && nominal->num < 16 * nominal->den) {
        notes.push_back({"served at " + shape.quantization + " (" +
                             py_fixed(py_truediv(nominal->num, nominal->den), 2) +
                             " bits per weight, the format's published average): a lossy format. The quality cost "
                             "is UNMEASURED here " +
                             kDash + " nothing in this product has evaluated it",
                         kCaveat, kDerived});
    }

    if (truthy(shape.context_max)) {
        notes.push_back(
            {"declares a maximum context of " + py_grouped(*shape.context_max) + " tokens", kStrength, kDerived});
    }

    if (verdict != nullptr) {
        const auto& reachable = verdict->max_context;
        if (reachable && *reachable == 0) {
            notes.push_back({"does not fit this machine's accelerator budget at any context; a runtime can still "
                             "serve it by spilling onto the CPU, which is a different performance story",
                             kWeakness, kDerived});
        } else if (!reachable) {
            notes.push_back({"the context that fits this machine is UNMEASURED; a term of the footprint could not "
                             "be established",
                             kCaveat, kUnmeasured});
        } else if (truthy(shape.context_max) && *reachable < *shape.context_max) {
            notes.push_back({"of its " + py_grouped(*shape.context_max) + " declared context, " +
                                 py_grouped(*reachable) + " tokens fit this machine's budget " + kDash +
                                 " the advertised figure is not reachable here",
                             kWeakness, kDerived});
        } else {
            notes.push_back({"its full " + py_grouped(*reachable) + "-token context fits this machine's budget",
                             kStrength, kDerived});
        }
    }
    return notes;
}

// ── the documents ───────────────────────────────────────────────────────────

void write_strings(JsonWriter& json, const std::vector<std::string>& items) {
    json.begin_array();
    for (const auto& item : items) json.string(item);
    json.end_array();
}

void write_optional_bool(JsonWriter& json, const std::optional<bool>& value) {
    if (value) {
        json.boolean(*value);
    } else {
        json.null();
    }
}

void write_shape(JsonWriter& json, const Shape& shape) {
    json.begin_object();
    json.key("model");
    json.string(shape.model);
    json.key("layers");
    json.optional_integer(shape.layers);
    json.key("kv_heads");
    json.optional_integer(shape.kv_heads);
    json.key("key_length");
    json.optional_integer(shape.key_length);
    json.key("value_length");
    json.optional_integer(shape.value_length);
    json.key("context_max");
    json.optional_integer(shape.context_max);
    json.key("weight_bytes");
    json.optional_integer(shape.weight_bytes);
    json.key("weight_provenance");
    json.string(shape.weight_provenance);
    json.key("parameters");
    json.optional_integer(shape.parameters);
    json.key("quantization");
    json.string(shape.quantization);
    json.key("architecture");
    json.string(shape.architecture);
    json.key("mixture_of_experts");
    write_optional_bool(json, shape.mixture_of_experts);
    json.key("active_parameters");
    json.optional_integer(shape.active_parameters);
    json.key("requires_backends");
    write_strings(json, shape.requires_backends);
    json.key("gaps");
    write_strings(json, shape.gaps);
    json.key("kv_established");
    json.boolean(shape.kv_established());
    json.key("kv_bytes_per_token");
    json.optional_integer(shape.kv_bytes_per_token);
    json.key("native");
    json.begin_object();
    json.key("refusal");
    json.string(shape.refusal);
    json.end_object();
    json.end_object();
}

void write_term(JsonWriter& json, const Term& term) {
    json.begin_object();
    json.key("label");
    json.string(term.label);
    json.key("bytes");
    json.optional_integer(term.bytes);
    json.key("gib");
    if (term.bytes) {
        json.number(py_truediv(*term.bytes, kGib));
    } else {
        json.null();
    }
    json.key("provenance");
    json.string(term.provenance);
    json.key("rule");
    json.string(term.rule);
    json.key("rendered");
    json.string(term.bytes ? gib_text(*term.bytes) + " GiB" : std::string(kUnmeasured));
    json.end_object();
}

void write_fit(JsonWriter& json, const Fit& fit) {
    json.begin_object();
    json.key("verdict");
    json.string(fit.verdict);
    json.key("fits");
    write_optional_bool(json, fit.fits());
    json.key("budget_bytes");
    json.optional_integer(fit.budget_bytes);
    json.key("budget_basis");
    json.string(fit.budget_basis);
    json.key("budget_provenance");
    json.string(fit.budget_provenance);
    json.key("max_context");
    json.optional_integer(fit.max_context);
    json.key("headroom_bytes");
    json.optional_integer(fit.headroom_bytes());
    json.key("reasons");
    write_strings(json, fit.reasons);
    json.key("footprint");
    json.begin_object();
    json.key("context");
    json.integer(fit.footprint.context);
    json.key("kv_precision");
    json.string(fit.footprint.kv_precision);
    json.key("weights");
    write_term(json, fit.footprint.weights);
    json.key("kv_cache");
    write_term(json, fit.footprint.kv_cache);
    json.key("margin");
    json.null();
    json.key("established_bytes");
    json.optional_integer(fit.footprint.total_bytes());
    json.key("total_bytes");
    json.optional_integer(fit.footprint.total_bytes());
    json.key("missing_terms");
    write_strings(json, fit.footprint.missing_terms());
    json.end_object();
    json.end_object();
}

struct Row {
    Shape shape;
    std::optional<Fit> verdict;
    std::vector<Note> notes;
};

struct Pairing {
    size_t left;
    size_t right;
    std::optional<int64_t> combined;
    std::optional<int64_t> budget;
    std::optional<bool> fits;
    std::optional<int64_t> spare() const {
        if (!combined || !budget) return std::nullopt;
        return sub(*budget, *combined, "a pair's spare memory");
    }
};

std::string quoted(const std::string& text) { return "\"" + text + "\""; }

}  // namespace

bool Reading::has_accelerator() const {
    return backend == "cuda" || backend == "rocm" || backend == "mps" || backend == "vulkan";
}

Shape shape_from_runtime_payload(const std::string& model, const Payload* payload) {
    if (payload == nullptr) {
        Shape shape;
        shape.model = model;
        shape.gaps.emplace_back("the runtime returned no model metadata");
        return shape;
    }
    try {
        return build_shape(model, *payload);
    } catch (const Overflow& error) {
        Shape shape;
        shape.model = shape_name(model, payload);
        shape.refusal = std::string(kRefusedIntegerOutOfRange);
        shape.gaps.push_back(error.what +
                             " does not fit the signed 64-bit integers this port computes in (deviation D9); no "
                             "figure of this shape is given rather than one approximated");
        return shape;
    }
}

Reserve serving_reserve_fraction(const std::string* env) {
    // `float(os.environ.get(NAME, "0.85") or "0.85")`, then `0.0 < cap <= 1.0`.
    const std::string text = (env == nullptr || env->empty()) ? std::string(kMemFractionDefault) : *env;
    const std::string where = env == nullptr ? std::string("ALELYON_HF_MEM_FRACTION is unset")
                              : env->empty() ? std::string("ALELYON_HF_MEM_FRACTION is empty")
                                             : "ALELYON_HF_MEM_FRACTION is " + quoted(*env);
    Reserve reserve;
    double cap = 0.0;
    if (!py_float_text(text, cap)) {
        reserve.basis = where + ", which float() refuses, so local_hf would not load: the reserve is fit.py's "
                                "declared fallback " + py_repr(kFallbackReserveFraction);
        return reserve;
    }
    if (0.0 < cap && cap <= 1.0) {
        reserve.fraction = 1.0 - cap;
        reserve.provenance = kObserved;
        reserve.basis = where + (env == nullptr || env->empty() ? ", so local_hf caps its process at its default " : "; local_hf caps its process at ") +
                        py_repr(cap) + " of accelerator memory and the reserve is 1.0 - " + py_repr(cap) + " = " +
                        py_repr(reserve.fraction);
        return reserve;
    }
    reserve.basis = where + ", a cap of " + py_repr(cap) +
                    " outside (0, 1], so the reserve is fit.py's declared fallback " +
                    py_repr(kFallbackReserveFraction);
    return reserve;
}

std::string shape_json(const std::string& model, const Payload* payload) {
    JsonWriter json;
    write_shape(json, shape_from_runtime_payload(model, payload));
    return json.text();
}

std::string foundry_json(const Reading& reading, const std::vector<FoundryModel>& models, int64_t context,
                         const std::string* mem_fraction_env, const std::string& source, const std::string& as_of) {
    const Reserve reserve = serving_reserve_fraction(mem_fraction_env);
    std::vector<Row> rows;
    std::vector<Fit> runnable;
    std::vector<size_t> runnable_order;
    std::vector<Pairing> pairs;
    std::string refusal;
    std::vector<std::string> native_gaps;
    try {
        // `catalog_from_runtime(names, show, reading)`.
        for (const auto& model : models) {
            Row row;
            row.shape = shape_from_runtime_payload(model.name, model.payload ? &*model.payload : nullptr);
            if (model.raised) {
                row.shape.gaps.push_back("the runtime did not describe this model (" + *model.raised + ")");
            }
            row.shape.requires_backends = model.requires_backends;
            row.verdict = assess(row.shape, reading, kContextLadder[1], reserve);
            row.notes = derive_notes(row.shape, &*row.verdict);
            rows.push_back(std::move(row));
        }
        // `runnable_here(models, reading, context=context)`: sorted by (verdict order,
        // -(max_context or 0), id), stably.
        for (const auto& row : rows) runnable.push_back(assess(row.shape, reading, context, reserve));
        runnable_order.resize(rows.size());
        for (size_t i = 0; i < rows.size(); ++i) runnable_order[i] = i;
        const auto order = [](const std::string& verdict) {
            if (verdict == kRuns) return 0;
            if (verdict == kTight) return 1;
            if (verdict == kUndecidable) return 2;
            return 3;
        };
        const auto negated_reach = [](const Fit& fit) {
            const int64_t reach = fit.max_context.value_or(0);
            if (reach == kI64Min) overflow("a negated context");
            return -reach;
        };
        std::vector<std::tuple<int, int64_t, std::string, size_t>> keys;
        for (size_t i = 0; i < rows.size(); ++i) {
            keys.emplace_back(order(runnable[i].verdict), negated_reach(runnable[i]), models[i].name, i);
        }
        std::stable_sort(runnable_order.begin(), runnable_order.end(), [&](size_t a, size_t b) {
            const auto& x = keys[a];
            const auto& y = keys[b];
            return std::tie(std::get<0>(x), std::get<1>(x), std::get<2>(x)) <
                   std::tie(std::get<0>(y), std::get<1>(y), std::get<2>(y));
        });

        // `coresident_pairs(models, reading, context=context)`.
        const std::optional<int64_t> budget =
            reading.has_accelerator() ? serving_budget_bytes(reading, reserve.fraction) : std::nullopt;
        // `{m.id: total for m in models}`: a repeated id keeps the LAST model's size.
        std::map<std::string, std::optional<int64_t>> sizes;
        for (size_t i = 0; i < rows.size(); ++i) {
            sizes[models[i].name] = footprint_at(rows[i].shape, context, kDefaultKvPrecision).total_bytes();
        }
        for (size_t i = 0; i < rows.size(); ++i) {
            for (size_t j = i + 1; j < rows.size(); ++j) {
                const auto a = sizes[models[i].name];
                const auto b = sizes[models[j].name];
                Pairing pair{i, j, std::nullopt, budget, std::nullopt};
                if (a && b) pair.combined = add(*a, *b, "a pair's combined footprint");
                if (pair.combined && budget) pair.fits = *pair.combined <= *budget;
                pairs.push_back(pair);
            }
        }
        std::vector<std::tuple<int, int64_t, std::string, std::string>> pair_keys;
        for (const auto& pair : pairs) {
            if (!pair.fits) {
                pair_keys.emplace_back(2, 0, models[pair.left].name, models[pair.right].name);
            } else {
                const int64_t spare = pair.spare().value_or(0);
                if (spare == kI64Min) overflow("a negated spare");
                pair_keys.emplace_back(*pair.fits ? 0 : 1, -spare, models[pair.left].name, models[pair.right].name);
            }
        }
        std::vector<size_t> pair_order(pairs.size());
        for (size_t i = 0; i < pairs.size(); ++i) pair_order[i] = i;
        std::stable_sort(pair_order.begin(), pair_order.end(),
                         [&](size_t a, size_t b) { return pair_keys[a] < pair_keys[b]; });
        std::vector<Pairing> sorted;
        for (const size_t i : pair_order) sorted.push_back(pairs[i]);
        pairs = std::move(sorted);
        // Every spare is now known to fit int64 (each was computed for its key).
    } catch (const Overflow& error) {
        refusal = std::string(kRefusedIntegerOutOfRange);
        native_gaps.push_back(error.what +
                              " does not fit the signed 64-bit integers this port computes in (deviation D9); "
                              "no verdict is given rather than one approximated");
        rows.clear();
        runnable.clear();
        runnable_order.clear();
        pairs.clear();
    }

    JsonWriter json;
    json.begin_object();
    json.key("reserve");
    json.begin_object();
    json.key("fraction");
    json.number(reserve.fraction);
    json.key("provenance");
    json.string(reserve.provenance);
    json.end_object();
    json.key("context");
    json.integer(context);
    json.key("catalog");
    json.begin_array();
    for (size_t i = 0; i < rows.size(); ++i) {
        const Row& row = rows[i];
        json.begin_object();
        json.key("id");
        json.string(models[i].name);
        json.key("label");
        json.string(models[i].name);
        json.key("pull_ref");
        json.string(models[i].name);
        json.key("license");
        json.string("");
        json.key("roles");
        write_strings(json, {kRoleGeneralist});
        json.key("source");
        json.string(source);
        json.key("as_of");
        json.string(as_of);
        json.key("provenance");
        json.string(kObserved);
        json.key("installed");
        json.boolean(true);
        json.key("shape");
        write_shape(json, row.shape);
        json.key("verdict");
        write_fit(json, *row.verdict);
        json.key("notes");
        json.begin_array();
        for (const auto& note : row.notes) {
            json.begin_object();
            json.key("claim");
            json.string(note.claim);
            json.key("kind");
            json.string(note.kind);
            json.key("provenance");
            json.string(note.provenance);
            json.key("source");
            json.string("");
            json.key("as_of");
            json.string("");
            json.key("sourced");
            json.boolean(true);  // no note here is DECLARED
            json.end_object();
        }
        json.end_array();
        json.end_object();
    }
    json.end_array();
    json.key("runnable");
    json.begin_array();
    for (const size_t i : runnable_order) {
        json.begin_object();
        json.key("index");
        json.integer(static_cast<int64_t>(i));
        json.key("id");
        json.string(models[i].name);
        json.key("fit");
        write_fit(json, runnable[i]);
        json.end_object();
    }
    json.end_array();
    json.key("pairs");
    json.begin_array();
    for (const auto& pair : pairs) {
        json.begin_object();
        json.key("left");
        json.integer(static_cast<int64_t>(pair.left));
        json.key("left_id");
        json.string(models[pair.left].name);
        json.key("right");
        json.integer(static_cast<int64_t>(pair.right));
        json.key("right_id");
        json.string(models[pair.right].name);
        json.key("context");
        json.integer(context);
        json.key("combined_bytes");
        json.optional_integer(pair.combined);
        json.key("budget_bytes");
        json.optional_integer(pair.budget);
        json.key("fits_together");
        write_optional_bool(json, pair.fits);
        json.key("spare_bytes");
        json.optional_integer(pair.spare());
        json.end_object();
    }
    json.end_array();
    json.key("native");
    json.begin_object();
    json.key("refusal");
    json.string(refusal);
    json.key("gaps");
    write_strings(json, native_gaps);
    json.key("reserve_basis");
    json.string(reserve.basis);
    json.end_object();
    json.end_object();
    return json.text();
}

std::string describe_json(const Reading& reading) {
    const auto bytes = [](const std::optional<int64_t>& value) {
        return value ? gib_text(*value) + " GiB" : std::string(kUnmeasured);
    };
    std::vector<std::array<std::string, 3>> rows;
    // The backend is OBSERVED when torch answered, or when the native probe chose it (D10).
    rows.push_back({"Compute backend", reading.backend,
                    (!reading.torch_version.empty() || !reading.probe.empty()) ? kObserved : kUnmeasured});
    rows.push_back({"Accelerator", reading.has_gpu ? reading.gpu_name : std::string(kUnmeasured),
                    reading.has_gpu ? kObserved : kUnmeasured});
    rows.push_back({"Accelerator memory", bytes(reading.vram_total_bytes()),
                    reading.vram_total_bytes() ? kObserved : kUnmeasured});
    const std::optional<int64_t> free_now = reading.has_gpu ? reading.gpu_free_bytes : std::nullopt;
    rows.push_back({"Accelerator memory free now", bytes(free_now), free_now ? kObserved : kUnmeasured});
    rows.push_back({"System memory", bytes(reading.ram_total_bytes), reading.ram_total_bytes ? kObserved : kUnmeasured});
    rows.push_back({"bfloat16",
                    !reading.bf16 ? std::string(kUnmeasured) : (*reading.bf16 ? "supported" : "not supported"),
                    reading.bf16 ? kObserved : kUnmeasured});
    rows.push_back({"Free disk", bytes(reading.disk_free_bytes), reading.disk_free_bytes ? kObserved : kUnmeasured});
    JsonWriter json;
    json.begin_object();
    json.key("rows");
    json.begin_array();
    for (const auto& row : rows) {
        json.begin_array();
        for (const auto& cell : row) json.string(cell);
        json.end_array();
    }
    json.end_array();
    json.key("gaps");
    write_strings(json, reading.gaps);
    json.end_object();
    return json.text();
}

void write_foundry_constants(JsonWriter& json) {
    json.begin_object();
    json.key("kv_precisions");
    json.begin_array();
    for (const auto& entry : kKvPrecisions) {
        json.begin_array();
        json.string(entry.name);
        json.integer(entry.value.num);
        json.integer(entry.value.den);
        json.end_array();
    }
    json.end_array();
    json.key("default_kv_precision");
    json.string(kDefaultKvPrecision);
    json.key("declared_bits_per_weight");
    json.begin_array();
    for (const auto& entry : kDeclaredBitsPerWeight) {
        json.begin_array();
        json.string(entry.name);
        json.integer(entry.value.num);
        json.integer(entry.value.den);
        json.end_array();
    }
    json.end_array();
    json.key("verdicts");
    write_strings(json, {kRuns, kTight, kExceeds, kUndecidable});
    json.key("fallback_reserve_fraction");
    json.number(kFallbackReserveFraction);
    json.key("tight_headroom_fraction");
    json.number(kTightHeadroomFraction);
    json.key("cpu_reserve_fraction");
    json.number(kCpuReserveFraction);
    json.key("context_ladder");
    json.begin_array();
    for (const auto context : kContextLadder) json.integer(context);
    json.end_array();
    json.key("gib");
    json.integer(kGib);
    json.key("provenance");
    write_strings(json, {kObserved, kDerived, kDeclared, kUnmeasured});
    json.key("roles_assigned");
    write_strings(json, {kRoleGeneralist});
    json.key("moe_active_share");
    json.number(kMoeActiveShare);
    json.end_object();
}

}  // namespace ma
