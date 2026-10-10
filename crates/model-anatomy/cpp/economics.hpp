// A port of the Python reference's `economics.py` (PR 5): what running a
// model locally costs against a hosted price, from a complete input record or a
// refusal by name. No default price, volume or wattage exists here either.
//
// Floats follow Python's arithmetic order operation for operation (every
// operation is one IEEE double operation, as in CPython), an `int / int` is
// Python's correctly rounded true division, and an `int / float` converts the
// int once, correctly rounded, as `float.__rtruediv__` does. The records are
// typed (deviation D14 in the crate README).
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "foundry.hpp"  // `workstation.py`'s provenance words (kObserved, ...)
#include "morphometry.hpp"

namespace ma {

class JsonWriter;

// `STRUCTURAL_CAVEATS`.
const std::vector<std::string>& structural_caveats();

struct Throughput {
    std::string model;
    int64_t context = 0;
    std::optional<double> decode_tokens_per_second;
    std::optional<double> prefill_tokens_per_second;
    std::string method;
    std::string provenance = kObserved;

    bool complete() const;
};

struct Energy {
    std::optional<double> draw_watts;
    std::optional<double> usd_per_kwh;
    std::string source;
    std::string as_of;
    std::string provenance = kDeclared;

    bool complete() const;
    std::optional<double> usd_for_seconds(double seconds) const;
};

struct Price {
    std::string provider;
    std::string model;
    std::optional<double> usd_per_million_input;
    std::optional<double> usd_per_million_output;
    std::string source;
    std::string as_of;
    std::string provenance = kDeclared;

    bool complete() const;
};

struct Volume {
    std::optional<int64_t> input_tokens;
    std::optional<int64_t> output_tokens;
    std::string stated_by = "the user";

    bool complete() const;
};

struct CostLine {
    std::string label;
    std::optional<double> usd;
    std::string provenance;
    std::string rule;
};

struct Costs {
    bool throughput_complete = false;
    bool energy_complete = false;
    bool price_complete = false;  // false for an absent price, too
    bool volume_complete = false;
    std::vector<CostLine> local;
    std::vector<CostLine> hosted;
    std::vector<std::string> refusals;
    std::optional<double> local_usd;
    std::optional<double> hosted_usd;
    std::optional<double> monthly_delta_usd;
    std::optional<double> gpu_hours_per_month;
    std::vector<std::string> description;  // `describe(comparison)`
};

// A llama-server `/completion` response, as far as `throughput_from_llamacpp`
// reads it. `mapping` false is a payload that is not a Mapping.
struct LlamacppPayload {
    bool mapping = false;
    std::optional<Value> model;  // payload["model"], absent when not given
    bool timings_mapping = false;
    Info timings;
};

// Python raised: the exception's class and message.
struct PythonError {
    std::string what;
};

// `throughput_from_llamacpp(payload, model=model, context=context)`; `payload`
// null is a payload that is not a Mapping. Throws `PythonError` where Python
// raises (OverflowError, ZeroDivisionError).
Throughput throughput_from_llamacpp(const LlamacppPayload* payload, const std::string& model, int64_t context);

// `compare(throughput, energy, price, volume)` with `describe` and every
// property; `price` null is None.
Costs compare_costs(const Throughput& throughput, const Energy& energy, const Price* price, const Volume& volume);

// `read_power(reader)`: `outcome` 0 no reader, 1 the reader raised, 2 it
// returned None, 3 it returned `watts`.
std::pair<std::optional<double>, std::string> read_power(int outcome, double watts);

void write_throughput(JsonWriter& json, const Throughput& reading);
void write_costs(JsonWriter& json, const Costs& costs);

}  // namespace ma
