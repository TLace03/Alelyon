// `economics.py`, operation for operation. Every string Python builds is built
// here from the same pieces, with Python's own formatting rules (`pyfmt`).
#include "economics.hpp"

#include <charconv>
#include <cmath>
#include <system_error>

#include "json_writer.hpp"
#include "pyfmt.hpp"

namespace ma {

namespace {

constexpr double kSecondsPerHour = 3600;  // SECONDS_PER_HOUR (an int; exact as a double)
constexpr double kWattsPerKw = 1000;      // WATTS_PER_KW
constexpr int64_t kMillion = 1000000;     // MILLION

// Python's `float(n)` of an int given as canonical decimal text: correctly
// rounded (`std::from_chars` rounds to nearest, ties to even, as
// `PyLong_AsDouble` does); past the double range, Python's OverflowError.
double int_text_to_double(const std::string& text) {
    double out = 0.0;
    const auto result = std::from_chars(text.data(), text.data() + text.size(), out);
    if (result.ec == std::errc::result_out_of_range) {
        throw PythonError{"OverflowError: int too large to convert to float"};
    }
    return out;
}

bool int_text_positive(const std::string& text) { return !text.empty() && text[0] != '-' && text != "0"; }

// `_rate(count_key, millis_key)`.
std::optional<double> rate(const Info& timings, const char* count_key, const char* millis_key) {
    const Value* count = timings.get(count_key);
    const Value* millis = timings.get(millis_key);
    // `isinstance(count, bool) or not isinstance(count, int)`.
    if (count == nullptr || count->tag != Value::Tag::Int) return std::nullopt;
    if (millis == nullptr || (millis->tag != Value::Tag::Int && millis->tag != Value::Tag::F64)) return std::nullopt;
    // `count <= 0 or millis <= 0` (a NaN duration is not <= 0, as in Python).
    if (!int_text_positive(count->text)) return std::nullopt;
    if (millis->tag == Value::Tag::Int ? !int_text_positive(millis->text) : millis->f64 <= 0) return std::nullopt;
    // `count / (float(millis) / 1e3)`: float(millis) first, then the int count is
    // converted by float.__rtruediv__, then the zero check of float division.
    const double seconds =
        (millis->tag == Value::Tag::Int ? int_text_to_double(millis->text) : millis->f64) / 1e3;
    const double numerator = int_text_to_double(count->text);
    if (seconds == 0.0) throw PythonError{"ZeroDivisionError: float division by zero"};
    return numerator / seconds;
}

// `_sum_lines`: `sum(float(line.usd) for line in lines)` starts from the int 0
// (0.0 once a float arrives). CPython 3.12 sums floats with a compensation
// term; for the two lines every comparison has, the compensated result is the
// plain one (the term is the exact error of the second addition, which rounds
// back to that sum), so the plain additions are Python's answer.
std::optional<double> sum_lines(const std::vector<CostLine>& lines) {
    if (lines.empty()) return std::nullopt;
    double total = 0.0;
    for (const auto& line : lines) {
        if (!line.usd) return std::nullopt;
        total += *line.usd;
    }
    return total;
}

// `_local_seconds`.
std::optional<double> local_seconds(const Throughput& throughput, const Volume& volume) {
    if (!throughput.complete() || !volume.complete()) return std::nullopt;
    return static_cast<double>(*volume.input_tokens) / *throughput.prefill_tokens_per_second +
           static_cast<double>(*volume.output_tokens) / *throughput.decode_tokens_per_second;
}

std::string energy_rule(int64_t tokens, double rate, const Energy& energy) {
    return py_grouped(tokens) + " tokens / " + py_fixed_grouped(rate, 1) + " tok/s x " +
           py_general(*energy.draw_watts) + " W x $" + py_general(*energy.usd_per_kwh) + "/kWh";
}

std::string price_rule(double usd_per_million, const Price& price) {
    return "$" + py_general(usd_per_million) + "/M declared by " + price.source + " as of " + price.as_of;
}

void write_texts(JsonWriter& json, const std::vector<std::string>& texts) {
    json.begin_array();
    for (const auto& text : texts) json.string(text);
    json.end_array();
}

void write_lines(JsonWriter& json, const std::vector<CostLine>& lines) {
    json.begin_array();
    for (const auto& line : lines) {
        json.begin_object();
        json.key("label");
        json.string(line.label);
        json.key("usd");
        json.optional_py_float(line.usd);
        json.key("provenance");
        json.string(line.provenance);
        json.key("rule");
        json.string(line.rule);
        json.end_object();
    }
    json.end_array();
}

std::vector<std::string> describe(const Costs& costs, const Price* price) {
    std::vector<std::string> lines;
    if (!costs.refusals.empty()) {
        for (const auto& reason : costs.refusals) lines.push_back("UNMEASURED: " + reason);
        return lines;
    }
    lines.push_back("Local energy: $" + py_fixed_grouped(*costs.local_usd, 2) + "/month");
    lines.push_back(price->provider + ": $" + py_fixed_grouped(*costs.hosted_usd, 2) + "/month");
    if (costs.monthly_delta_usd) {
        const double delta = *costs.monthly_delta_usd;
        const char* direction = delta > 0 ? "less" : "more";
        lines.push_back("Running locally costs $" + py_fixed_grouped(std::fabs(delta), 2) + " " + direction +
                        " per month in energy at this volume — a projection, not an observed bill.");
    }
    if (costs.gpu_hours_per_month) {
        const double hours = *costs.gpu_hours_per_month;
        lines.push_back("That volume occupies the accelerator for " + py_fixed_grouped(hours, 1) +
                        " hours of the month" +
                        (hours > 730 ? " — more than a month contains, so it does not fit on one device at this "
                                       "rate."
                                     : "."));
    }
    for (const auto& caveat : structural_caveats()) lines.push_back("This comparison " + caveat);
    return lines;
}

}  // namespace

const std::vector<std::string>& structural_caveats() {
    static const std::vector<std::string> kCaveats = {
        "excludes the hardware. For most users the accelerator is the largest cost in this comparison and it does "
        "not appear in it at all.",
        "excludes the operator's time — running a local model is work a hosted API does for you.",
        "is not quality-matched. A local model and a frontier API are different products; nothing here has "
        "compared what they produce.",
        "is a projection from declared prices and measured rates, not an observed bill.",
    };
    return kCaveats;
}

bool Throughput::complete() const {
    return decode_tokens_per_second && prefill_tokens_per_second && *decode_tokens_per_second > 0 &&
           *prefill_tokens_per_second > 0;
}

bool Energy::complete() const {
    return draw_watts && usd_per_kwh && *draw_watts > 0 && *usd_per_kwh >= 0 && !source.empty() && !as_of.empty();
}

std::optional<double> Energy::usd_for_seconds(double seconds) const {
    if (!complete()) return std::nullopt;
    const double kwh = (*draw_watts / kWattsPerKw) * (seconds / kSecondsPerHour);
    return kwh * *usd_per_kwh;
}

bool Price::complete() const {
    return usd_per_million_input && usd_per_million_output && !source.empty() && !as_of.empty();
}

bool Volume::complete() const { return input_tokens && output_tokens && *input_tokens >= 0 && *output_tokens >= 0; }

Throughput throughput_from_llamacpp(const LlamacppPayload* payload, const std::string& model, int64_t context) {
    static const Info kEmpty;
    const bool mapping = payload != nullptr && payload->mapping;
    const Info& timings = mapping && payload->timings_mapping ? payload->timings : kEmpty;
    Throughput out;
    // `str(model or name or "")`.
    if (!model.empty()) {
        out.model = model;
    } else if (mapping && payload->model && truthy(&*payload->model)) {
        out.model = py_str(*payload->model);
    }
    out.context = context;
    out.decode_tokens_per_second = rate(timings, "predicted_n", "predicted_ms");
    out.prefill_tokens_per_second = rate(timings, "prompt_n", "prompt_ms");
    out.method = "the runtime's own timings, excluding model load time";
    return out;
}

Costs compare_costs(const Throughput& throughput, const Energy& energy, const Price* price, const Volume& volume) {
    Costs out;
    out.throughput_complete = throughput.complete();
    out.energy_complete = energy.complete();
    out.price_complete = price != nullptr && price->complete();
    out.volume_complete = volume.complete();

    if (!out.throughput_complete) {
        std::string missing;
        if (!throughput.decode_tokens_per_second) missing = "decode rate";
        if (!throughput.prefill_tokens_per_second) missing += missing.empty() ? "prefill rate" : ", prefill rate";
        out.refusals.push_back("throughput is UNMEASURED (" + (missing.empty() ? std::string("both rates") : missing) +
                               "). Generate once with this model and read the runtime's own counters; a rate "
                               "quoted from a specification is not this machine's rate.");
    }
    if (!out.energy_complete) {
        out.refusals.push_back(
            "the energy declaration is incomplete — board draw, tariff, source and date are all required. "
            "There is no default: a typical price chosen here would be the number doing the work in the result.");
    }
    if (!out.volume_complete) {
        out.refusals.push_back(
            "no monthly token volume was stated. Nobody but the operator knows it, and a typical figure would "
            "decide the answer.");
    }
    if (!out.price_complete) {
        out.refusals.push_back(
            "no hosted price with a source and an as-of date was supplied, so there is nothing to compare against. "
            "Published prices go stale; one hard-coded here would look authoritative while being wrong.");
    }

    if (out.refusals.empty()) {
        const int64_t input = *volume.input_tokens;
        const int64_t output = *volume.output_tokens;
        const double prefill = *throughput.prefill_tokens_per_second;
        const double decode = *throughput.decode_tokens_per_second;
        const double prefill_seconds = static_cast<double>(input) / prefill;
        const double decode_seconds = static_cast<double>(output) / decode;
        out.local = {
            {"Energy, reading prompts", energy.usd_for_seconds(prefill_seconds), kDerived,
             energy_rule(input, prefill, energy)},
            {"Energy, writing answers", energy.usd_for_seconds(decode_seconds), kDerived,
             energy_rule(output, decode, energy)},
        };
        // `int(tokens) / MILLION * float(rate)`: a correctly rounded int / int, then one product.
        out.hosted = {
            {price->provider + " input", py_truediv(input, kMillion) * *price->usd_per_million_input, kDerived,
             price_rule(*price->usd_per_million_input, *price)},
            {price->provider + " output", py_truediv(output, kMillion) * *price->usd_per_million_output, kDerived,
             price_rule(*price->usd_per_million_output, *price)},
        };
    }
    out.local_usd = sum_lines(out.local);
    out.hosted_usd = sum_lines(out.hosted);
    if (out.local_usd && out.hosted_usd) out.monthly_delta_usd = *out.hosted_usd - *out.local_usd;
    const auto seconds = local_seconds(throughput, volume);
    if (seconds) out.gpu_hours_per_month = *seconds / kSecondsPerHour;
    out.description = describe(out, price);
    return out;
}

std::pair<std::optional<double>, std::string> read_power(int outcome, double watts) {
    // No reader, a reader that raised, or one that returned None: a gap.
    if (outcome != 3) return {std::nullopt, kUnmeasured};
    // `watts <= 0` (a NaN reading is not <= 0, so Python keeps it, OBSERVED).
    if (watts <= 0) return {std::nullopt, kUnmeasured};
    return {watts, kObserved};
}

void write_throughput(JsonWriter& json, const Throughput& reading) {
    json.begin_object();
    json.key("model");
    json.string(reading.model);
    json.key("context");
    json.integer(reading.context);
    json.key("decode_tokens_per_second");
    json.optional_py_float(reading.decode_tokens_per_second);
    json.key("prefill_tokens_per_second");
    json.optional_py_float(reading.prefill_tokens_per_second);
    json.key("method");
    json.string(reading.method);
    json.key("provenance");
    json.string(reading.provenance);
    json.key("complete");
    json.boolean(reading.complete());
    json.end_object();
}

void write_costs(JsonWriter& json, const Costs& costs) {
    json.begin_object();
    json.key("inputs_complete");
    json.begin_object();
    json.key("throughput");
    json.boolean(costs.throughput_complete);
    json.key("energy");
    json.boolean(costs.energy_complete);
    json.key("price");
    json.boolean(costs.price_complete);
    json.key("volume");
    json.boolean(costs.volume_complete);
    json.end_object();
    json.key("established");
    json.boolean(costs.refusals.empty());
    json.key("refusals");
    write_texts(json, costs.refusals);
    json.key("local");
    write_lines(json, costs.local);
    json.key("hosted");
    write_lines(json, costs.hosted);
    json.key("caveats");
    write_texts(json, structural_caveats());
    json.key("local_usd");
    json.optional_py_float(costs.local_usd);
    json.key("hosted_usd");
    json.optional_py_float(costs.hosted_usd);
    json.key("monthly_delta_usd");
    json.optional_py_float(costs.monthly_delta_usd);
    json.key("gpu_hours_per_month");
    json.optional_py_float(costs.gpu_hours_per_month);
    json.key("describe");
    write_texts(json, costs.description);
    json.end_object();
}

}  // namespace ma
