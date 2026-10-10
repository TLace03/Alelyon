#include "morphometry.hpp"

#include <algorithm>
#include <cmath>
#include <map>
#include <set>
#include <string_view>
#include <tuple>

#include "canonical_space.hpp"
#include "checked.hpp"
#include "constants.hpp"
#include "json_writer.hpp"
#include "pyfmt.hpp"

namespace ma {

// ── model_info as a Python dict ─────────────────────────────────────────────
void Info::set(std::string key, Value value) {
    const auto at = index_.find(key);
    if (at != index_.end()) {
        items_[at->second].second = std::move(value);  // first place, last value
        return;
    }
    index_.emplace(key, items_.size());
    items_.emplace_back(std::move(key), std::move(value));
}

const std::pair<std::string, Value>* Info::find(const std::string& key) const {
    const auto at = index_.find(key);
    return at == index_.end() ? nullptr : &items_[at->second];
}

namespace {

// An em dash, as Python's sources write it (U+2014).
const std::string kDash = "\xE2\x80\x94";

bool starts_with(std::string_view text, std::string_view prefix) {
    return text.size() >= prefix.size() && text.substr(0, prefix.size()) == prefix;
}

bool ends_with(std::string_view text, std::string_view suffix) {
    return text.size() >= suffix.size() && text.substr(text.size() - suffix.size()) == suffix;
}

std::string join(const std::vector<std::string>& items, std::string_view separator) {
    std::string out;
    for (size_t i = 0; i < items.size(); ++i) {
        if (i != 0) out.append(separator);
        out += items[i];
    }
    return out;
}

}  // namespace

// The value rules below are shared with the Foundry port (morphometry.hpp).

// Python's truth value of a `model_info` value (None when absent).
bool truthy(const Value* value) {
    if (value == nullptr) return false;
    switch (value->tag) {
        case Value::Tag::None:
            return false;
        case Value::Tag::Bool:
            return value->boolean;
        case Value::Tag::Int:
            return value->text != "0";
        case Value::Tag::F64:
            return value->f64 != 0.0;  // NaN is truthy, as in Python
        case Value::Tag::Str:
            return !value->text.empty();
    }
    return false;
}

// Python's `str(value)`.
std::string py_str(const Value& value) {
    switch (value.tag) {
        case Value::Tag::None:
            return "None";
        case Value::Tag::Bool:
            return value.boolean ? "True" : "False";
        case Value::Tag::Int:
        case Value::Tag::Str:
            return value.text;
        case Value::Tag::F64:
            return py_repr(value.f64);
    }
    return std::string();
}

// An f-string's `{x}` of `int | None`.
std::string py_str(const std::optional<int64_t>& value) {
    return value ? std::to_string(*value) : std::string("None");
}

// `_int(value)`. A valid Python integer outside int64 is D2.
std::optional<int64_t> to_int(const Value* value, const std::string& key) {
    if (value == nullptr) return std::nullopt;
    int64_t out = 0;
    switch (value->tag) {
        case Value::Tag::None:
        case Value::Tag::Bool:
            return std::nullopt;
        case Value::Tag::Int: {
            const IntText parsed = py_int_text(value->text, out);
            if (parsed == IntText::OutOfRange) overflow("the integer value of " + key);
            if (parsed == IntText::NotAnInteger) return std::nullopt;  // unreachable: validated
            return out;
        }
        case Value::Tag::F64: {
            const double x = value->f64;
            if (!std::isfinite(x) || std::trunc(x) != x) return std::nullopt;
            if (x < -9223372036854775808.0 || x >= 9223372036854775808.0) {
                overflow("the integer value of " + key);
            }
            return static_cast<int64_t>(x);
        }
        case Value::Tag::Str: {
            const IntText parsed = py_int_text(py_strip(value->text), out);
            if (parsed == IntText::OutOfRange) overflow("the integer value of " + key);
            if (parsed == IntText::NotAnInteger) return std::nullopt;
            return out;
        }
    }
    return std::nullopt;
}

namespace {

using Item = std::pair<std::string, Value>;

// `_arch_key`: the direct key when it holds something other than None, else
// the FIRST key in insertion order ending in ".<suffix>" outside "general.".
const Item* arch_key(const Info& info, const std::string& architecture, const std::string& suffix) {
    if (!architecture.empty()) {
        const Item* direct = info.find(architecture + "." + suffix);
        if (direct != nullptr && direct->second.tag != Value::Tag::None) return direct;
    }
    const std::string tail = "." + suffix;
    for (const auto& item : info.items()) {
        if (ends_with(item.first, tail) && !starts_with(item.first, "general.")) return &item;
    }
    return nullptr;
}

std::optional<int64_t> arch_int(const Info& info, const std::string& architecture, const std::string& suffix) {
    const Item* item = arch_key(info, architecture, suffix);
    return item == nullptr ? std::nullopt : to_int(&item->second, item->first);
}

// `_routing_declaration`: present even when the value is None.
const Item* routing_declaration(const Info& info, const std::string& architecture, const std::string& suffix) {
    if (!architecture.empty()) {
        const Item* direct = info.find(architecture + "." + suffix);
        if (direct != nullptr) return direct;
    }
    const std::string tail = "." + suffix;
    for (const auto& item : info.items()) {
        if (ends_with(item.first, tail) && !starts_with(item.first, "general.")) return &item;
    }
    return nullptr;
}

// `module_of`: the first fragment found in the lowercased name.
std::string module_of(std::string_view name) {
    const std::string lower = ascii_lower(name);
    for (const auto& [fragment, module] : kNameToModule) {
        if (lower.find(fragment) != std::string::npos) return std::string(module);
    }
    return "other";
}

// `block_of`: `(?:^|\.)blk\.(\d{1,6})\.`, the leftmost match.
std::optional<int64_t> block_of(std::string_view name) {
    for (size_t start = 0; start < name.size(); ++start) {
        size_t at = 0;
        if (start == 0 && starts_with(name, "blk.")) {
            at = 4;
        } else if (name[start] == '.' && starts_with(name.substr(start + 1), "blk.")) {
            at = start + 5;
        } else {
            continue;
        }
        size_t end = at;
        while (end < name.size() && name[end] >= '0' && name[end] <= '9' && end - at < 7) ++end;
        const size_t digits = end - at;
        if (digits >= 1 && digits <= 6 && end < name.size() && name[end] == '.') {
            int64_t value = 0;
            for (size_t i = at; i < end; ++i) value = value * 10 + (name[i] - '0');
            return value;
        }
    }
    return std::nullopt;
}

bool is_routed_expert(std::string_view name) {
    size_t begin = 0;
    while (true) {
        const size_t dot = name.find('.', begin);
        const std::string_view segment =
            name.substr(begin, dot == std::string_view::npos ? std::string_view::npos : dot - begin);
        if (ends_with(segment, kRoutedExpertSuffix)) return true;
        if (dot == std::string_view::npos) return false;
        begin = dot + 1;
    }
}

// `parse_tensor_inventory`.
void parse_tensor_inventory(const Payload& payload, std::vector<TensorRecord>& records,
                            std::vector<std::string>& gaps) {
    if (!payload.has_tensors) {
        gaps.emplace_back("the runtime published no tensor inventory");
        return;
    }
    int64_t unusable = 0;
    std::set<std::string> unknown_types;
    const bool truncated = static_cast<int64_t>(payload.tensors.size()) > kMaxTensors;
    const size_t limit = std::min(payload.tensors.size(), static_cast<size_t>(kMaxTensors));
    for (size_t i = 0; i < limit; ++i) {
        const TensorInput& item = payload.tensors[i];
        const std::string name(py_strip(item.name));
        if (name.empty() || item.dims.empty()) {
            ++unusable;
            continue;
        }
        // Python stops at the first dimension <= 0 and drops the tensor; a zero
        // anywhere drops it before a too-large dimension could matter.
        if (std::find(item.dims.begin(), item.dims.end(), uint64_t{0}) != item.dims.end()) {
            ++unusable;
            continue;
        }
        std::vector<int64_t> dims;
        int64_t parameters = 1;
        for (const uint64_t dim : item.dims) {
            if (dim > static_cast<uint64_t>(kI64Max)) overflow("a dimension of tensor " + name);
            dims.push_back(static_cast<int64_t>(dim));
            parameters = mul(parameters, static_cast<int64_t>(dim), "a tensor's parameter count");
        }
        const std::string element_type = ascii_upper(py_strip(item.type_name));
        if (!element_type.empty() && nominal_bits(element_type) == nullptr) {
            unknown_types.insert(element_type);
        }
        TensorRecord record;
        record.name = name;
        record.module = module_of(name);
        record.block = block_of(name);
        record.shape = std::move(dims);
        record.element_type = element_type;
        record.parameters = parameters;
        records.push_back(std::move(record));
    }
    if (truncated) {
        gaps.push_back("the inventory was truncated at " + py_grouped(kMaxTensors) +
                       " tensors; totals below are for the truncated set");
    }
    if (unusable != 0) {
        gaps.push_back(std::to_string(unusable) +
                       " tensor entries had no usable name or shape and were dropped rather than "
                       "counted as zero");
    }
    if (!unknown_types.empty()) {
        gaps.push_back("no published bits-per-weight for element type(s) " +
                       join(std::vector<std::string>(unknown_types.begin(), unknown_types.end()), ", ") +
                       " " + kDash + " storage for those cells is UNMEASURED");
    }
}

// `_active_path_gap`.
std::optional<std::string> active_path_gap_of(const std::vector<TensorRecord>& tensors,
                                              std::optional<int64_t> used, std::optional<int64_t> total) {
    std::vector<const TensorRecord*> routed;
    for (const auto& tensor : tensors) {
        if (is_routed_expert(tensor.name)) routed.push_back(&tensor);
    }
    const std::string declared = "(expert_count=" + py_str(total) + ", expert_used_count=" + py_str(used) + ")";
    if (routed.empty()) {
        if (used || total) {
            return "the runtime declares routing metadata " + declared +
                   ", but its tensor inventory contains no recognized routed _exps expert bank. "
                   "Stored inventory counts remain measured; the active-parameter figure is UNMEASURED.";
        }
        return std::nullopt;
    }
    if (!used || !total) {
        return "this model routes part of its feed-forward stack, and the runtime does not declare a "
               "complete expert_count/expert_used_count pair " +
               declared +
               ". Parameter share is exact; the active-parameter figure is UNMEASURED and the totals "
               "here must not be read as the cost of one token.";
    }
    if (*used <= 0 || *total <= 0) {
        return "the runtime declares non-positive routing counts " + declared +
               ". Parameter share is exact; the active-parameter figure is UNMEASURED.";
    }
    if (*used > *total) {
        return "the runtime declares expert_used_count=" + std::to_string(*used) +
               ", greater than expert_count=" + std::to_string(*total) +
               ". Parameter share is exact; the active-parameter figure is UNMEASURED.";
    }
    int64_t mismatched = 0;
    for (const auto* tensor : routed) {
        if (tensor->shape.empty() || tensor->shape.back() != *total) ++mismatched;
    }
    if (mismatched != 0) {
        return "the runtime declares expert_count=" + std::to_string(*total) + ", but " +
               std::to_string(mismatched) + " of " + std::to_string(routed.size()) +
               " routed tensor shape(s) do not end in an expert axis of length " + std::to_string(*total) +
               ". Stored parameter counts still follow the inventory; the active-parameter figure is "
               "UNMEASURED.";
    }
    return std::nullopt;
}

// `_cells_from_tensors`: buckets ordered as Python sorts them, with the
// stack-external (block None) rows first.
std::vector<Cell> cells_from_tensors(const std::vector<TensorRecord>& tensors) {
    struct Entry {
        int64_t parameters = 0;
        int64_t count = 0;
        std::set<std::string> types;
        int64_t nominal = 0;
        bool known = true;
        int64_t routed = 0;
    };
    using Key = std::tuple<bool, int64_t, std::string>;
    std::map<Key, Entry> buckets;
    for (const auto& tensor : tensors) {
        Entry& entry = buckets[Key{tensor.block.has_value(), tensor.block.value_or(0), tensor.module}];
        entry.parameters = add(entry.parameters, tensor.parameters, "a cell's parameter total");
        entry.count += 1;
        entry.types.insert(tensor.element_type);
        const auto nominal = tensor_nominal_bytes(tensor);
        if (!nominal) {
            entry.known = false;
        } else {
            entry.nominal = add(entry.nominal, *nominal, "a cell's nominal bytes");
        }
        if (is_routed_expert(tensor.name)) {
            entry.routed = add(entry.routed, tensor.parameters, "a cell's routed parameters");
        }
    }
    std::vector<Cell> cells;
    for (auto& [key, entry] : buckets) {
        Cell cell;
        if (std::get<0>(key)) cell.block = std::get<1>(key);
        cell.module = std::get<2>(key);
        cell.parameters = entry.parameters;
        cell.tensors = entry.count;
        cell.element_types.assign(entry.types.begin(), entry.types.end());
        if (entry.known) cell.nominal_bytes = entry.nominal;
        cell.routed_parameters = entry.routed;
        cells.push_back(std::move(cell));
    }
    return cells;
}

// `_native_axis_order`.
std::pair<std::string, std::string> native_axis_order_of(const std::vector<TensorRecord>& tensors) {
    int64_t block_changes = 0;
    int64_t module_changes = 0;
    const TensorRecord* previous = nullptr;
    for (const auto& tensor : tensors) {
        if (!tensor.block) continue;
        if (previous != nullptr) {
            if (tensor.block != previous->block) ++block_changes;
            if (tensor.module != previous->module) ++module_changes;
        }
        previous = &tensor;
    }
    if (block_changes > module_changes) return {"module", "block"};
    return {"block", "module"};
}

Cell dense_cell(std::optional<int64_t> block, std::string_view module, int64_t parameters) {
    Cell cell;
    cell.block = block;
    cell.module = std::string(module);
    cell.parameters = parameters;
    cell.tensors = 1;
    return cell;
}

// `_dense_decoder_cells`.
std::vector<Cell> dense_decoder_cells(int64_t blocks, int64_t embedding, int64_t feed_forward, int64_t heads,
                                      int64_t kv_heads, int64_t head_dim, std::optional<int64_t> vocab) {
    const char* what = "the declared-architecture arithmetic";
    const int64_t q_out = mul(heads, head_dim, what);
    const int64_t kv_out = mul(kv_heads, head_dim, what);
    const std::vector<std::pair<std::string_view, int64_t>> per_block = {
        {"attn_norm", embedding},
        {"attn_q", mul(embedding, q_out, what)},
        {"attn_k", mul(embedding, kv_out, what)},
        {"attn_v", mul(embedding, kv_out, what)},
        {"attn_output", mul(q_out, embedding, what)},
        {"ffn_norm", embedding},
        {"ffn_gate", mul(embedding, feed_forward, what)},
        {"ffn_up", mul(embedding, feed_forward, what)},
        {"ffn_down", mul(feed_forward, embedding, what)},
    };
    std::vector<Cell> cells;
    if (vocab && *vocab != 0) {
        const int64_t table = mul(*vocab, embedding, what);
        cells.push_back(dense_cell(std::nullopt, "token_embd", table));
        cells.push_back(dense_cell(std::nullopt, "output", table));
    }
    cells.push_back(dense_cell(std::nullopt, "output_norm", embedding));
    for (int64_t index = 0; index < blocks; ++index) {
        for (const auto& [module, parameters] : per_block) cells.push_back(dense_cell(index, module, parameters));
    }
    return cells;
}

bool contains(const std::vector<std::string>& items, const std::string& item) {
    return std::find(items.begin(), items.end(), item) != items.end();
}

// `_median` of an already sorted list.
double median_of(const std::vector<double>& values) {
    const size_t count = values.size();
    if (count == 0) return 0.0;
    const size_t middle = count / 2;
    if (count % 2 != 0) return values[middle];
    return (values[middle - 1] + values[middle]) / 2.0;
}

int64_t gcd(int64_t a, int64_t b) {
    a = a < 0 ? -a : a;
    b = b < 0 ? -b : b;
    while (b != 0) {
        const int64_t t = a % b;
        a = b;
        b = t;
    }
    return a;
}

void write_rational(JsonWriter& json, const std::optional<Rational>& value) {
    if (!value) {
        json.null();
        return;
    }
    json.begin_array();
    json.integer(value->num);
    json.integer(value->den);
    json.end_array();
}

void write_optional_block(JsonWriter& json, const std::optional<int64_t>& block) {
    json.optional_integer(block);
}

}  // namespace

std::string tensor_module(std::string_view name) { return module_of(py_strip(name)); }

std::optional<int64_t> tensor_block(std::string_view name) { return block_of(py_strip(name)); }

// ── per-tensor and per-cell properties ──────────────────────────────────────
std::optional<Rational> tensor_bits_per_weight(const TensorRecord& tensor) {
    const BitsPerWeight* bits = nominal_bits(ascii_upper(tensor.element_type));
    if (bits == nullptr) return std::nullopt;
    return Rational{bits->num, bits->den};
}

std::optional<int64_t> tensor_nominal_bytes(const TensorRecord& tensor) {
    const auto bits = tensor_bits_per_weight(tensor);
    if (!bits) return std::nullopt;
    // int(parameters * num / (den * 8)), exactly: split so no product exceeds
    // what the quotient needs. parameters > 0, so int() is the floor.
    const int64_t divisor = bits->den * 8;
    const int64_t whole = tensor.parameters / divisor;
    const int64_t part = tensor.parameters % divisor;
    const char* what = "a tensor's nominal bytes";
    return add(mul(whole, bits->num, what), mul(part, bits->num, what) / divisor, what);
}

std::optional<Rational> mean_bits_per_weight(std::optional<int64_t> nominal_bytes, int64_t parameters) {
    if (!nominal_bytes || parameters <= 0) return std::nullopt;
    const int64_t num = mul(*nominal_bytes, 8, "a mean bits-per-weight numerator");
    const int64_t divisor = gcd(num, parameters);
    // Fraction normalises the sign onto the numerator; parameters > 0 already.
    return Rational{num / divisor, parameters / divisor};
}

std::optional<int64_t> cell_active_parameters(const Cell& cell, std::optional<int64_t> used,
                                              std::optional<int64_t> total) {
    if (cell.routed_parameters == 0) return cell.parameters;
    if (!used || !total || *used <= 0 || *total <= 0 || *used > *total ||
        cell.routed_parameters % *total != 0) {
        return std::nullopt;
    }
    const char* what = "a cell's active parameters";
    const int64_t dense = sub(cell.parameters, cell.routed_parameters, what);
    return add(dense, mul(cell.routed_parameters, *used, what) / *total, what);
}

// ── the entry point ─────────────────────────────────────────────────────────
std::string resolved_name(const std::string& model, const Payload* payload) {
    if (payload == nullptr) return model;
    const std::string& chosen = !model.empty()            ? model
                                : !payload->model.empty() ? payload->model
                                                          : payload->parent_model;
    return std::string(py_strip(chosen));
}

Morphometry overflow_refusal(const std::string& name, const std::string& what) {
    Morphometry morph;
    morph.model = name;
    morph.refusal = std::string(kRefusedIntegerOutOfRange);
    morph.gaps.push_back(what +
                         " does not fit the signed 64-bit integers this port computes in "
                         "(deviation D2); nothing is measured rather than a figure approximated");
    return morph;
}

Morphometry analyze(const std::string& model, const Payload* payload) {
    Morphometry morph;
    if (payload == nullptr) {
        morph.model = model;
        morph.refusal = std::string(kRefusedNoMetadata);
        morph.gaps.emplace_back("the runtime returned no model metadata");
        return morph;
    }
    const Info& info = payload->info;
    morph.model = resolved_name(model, payload);
    const Value* architecture_value = info.get("general.architecture");
    const std::string architecture =
        truthy(architecture_value) ? std::string(py_strip(py_str(*architecture_value))) : std::string();
    morph.architecture = architecture;
    morph.family = std::string(py_strip(payload->family));
    morph.quantization = std::string(py_strip(payload->quantization_level));
    morph.declared_parameters = to_int(info.get("general.parameter_count"), "general.parameter_count");
    const std::optional<int64_t> blocks = arch_int(info, architecture, "block_count");
    const std::optional<int64_t> embedding = arch_int(info, architecture, "embedding_length");
    morph.context_length = arch_int(info, architecture, "context_length");
    morph.block_count = blocks;
    morph.embedding_length = embedding;

    const Item* experts_item = routing_declaration(info, architecture, "expert_count");
    const Item* used_item = routing_declaration(info, architecture, "expert_used_count");
    const std::optional<int64_t> experts =
        experts_item == nullptr ? std::nullopt : to_int(&experts_item->second, experts_item->first);
    const std::optional<int64_t> experts_used =
        used_item == nullptr ? std::nullopt : to_int(&used_item->second, used_item->first);
    morph.expert_count = experts;
    morph.expert_used_count = experts_used;
    std::vector<std::string> invalid;
    if (experts_item != nullptr && !experts) invalid.emplace_back("expert_count");
    if (used_item != nullptr && !experts_used) invalid.emplace_back("expert_used_count");
    if (!invalid.empty()) {
        morph.routing_metadata_error =
            "the runtime declares unusable routing metadata for " + join(invalid, ", ") +
            "; each value must be an integer. The expert partition is not established, so the "
            "active-parameter figure is UNMEASURED.";
    }

    std::vector<TensorRecord> tensors;
    std::vector<std::string> gaps;
    parse_tensor_inventory(*payload, tensors, gaps);
    if (!tensors.empty()) {
        std::vector<Cell> cells = cells_from_tensors(tensors);
        std::set<int64_t> observed;
        for (const auto& cell : cells) {
            if (cell.block) observed.insert(*cell.block);
        }
        morph.source = std::string(kSourceTensorInventory);
        if (static_cast<int64_t>(observed.size()) > kMaxBlocks) {
            morph.refusal = std::string(kRefusedNoBlocks);
            gaps.push_back("the inventory declares more than " + py_grouped(kMaxBlocks) +
                           " blocks, beyond this view's budget");
            morph.gaps = std::move(gaps);
            return morph;
        }
        if (blocks && !observed.empty() && static_cast<int64_t>(observed.size()) != *blocks) {
            gaps.push_back("the runtime declares " + std::to_string(*blocks) + " blocks; the inventory contains " +
                           std::to_string(observed.size()) + " " + kDash + " the breakdown follows the inventory");
        }
        const auto routing_gap = active_path_gap_of(tensors, experts_used, experts);
        for (const std::string& gap : {morph.routing_metadata_error, routing_gap.value_or(std::string())}) {
            if (!gap.empty() && !contains(gaps, gap)) gaps.push_back(gap);
        }
        morph.native_axis_order = native_axis_order_of(tensors);
        morph.tensors = std::move(tensors);
        morph.cells = std::move(cells);
        morph.gaps = std::move(gaps);
        return morph;
    }

    // No inventory: arithmetic over declared dimensions, for the dense shape only.
    morph.source = std::string(kSourceDeclaredArchitecture);
    if (experts_item != nullptr || used_item != nullptr) {
        const std::string refusal_gap =
            !morph.routing_metadata_error.empty()
                ? morph.routing_metadata_error
                : "this model declares routing metadata (expert_count=" + py_str(experts) +
                      ", expert_used_count=" + py_str(experts_used) +
                      "). The dense decoder arithmetic below could understate it, so no breakdown is "
                      "produced. A runtime that publishes a tensor inventory can establish the expert bank.";
        morph.refusal = std::string(kRefusedMixtureOfExperts);
        gaps.push_back(refusal_gap);
        morph.gaps = std::move(gaps);
        return morph;
    }
    if (!blocks || *blocks == 0) {
        morph.refusal = std::string(kRefusedNoBlocks);
        gaps.emplace_back("the runtime declares no block count, so there is no stack to arrange");
        morph.gaps = std::move(gaps);
        return morph;
    }
    const std::optional<int64_t> feed_forward = arch_int(info, architecture, "feed_forward_length");
    const std::optional<int64_t> heads = arch_int(info, architecture, "attention.head_count");
    std::optional<int64_t> kv_heads = arch_int(info, architecture, "attention.head_count_kv");
    if (!kv_heads || *kv_heads == 0) kv_heads = heads;
    std::vector<std::string> missing;
    if (!embedding || *embedding == 0) missing.emplace_back("embedding_length");
    if (!feed_forward || *feed_forward == 0) missing.emplace_back("feed_forward_length");
    if (!heads || *heads == 0) missing.emplace_back("attention.head_count");
    if (!missing.empty()) {
        morph.refusal = std::string(kRefusedInsufficientArchitecture);
        gaps.push_back("the runtime declares no " + join(missing, ", ") + " " + kDash +
                       " the per-cell arithmetic needs them");
        morph.gaps = std::move(gaps);
        return morph;
    }
    const std::optional<int64_t> key_length = arch_int(info, architecture, "attention.key_length");
    const int64_t head_dim = (key_length && *key_length != 0)
                                 ? *key_length
                                 : floordiv(*embedding, *heads, "the declared head dimension");
    const std::optional<int64_t> vocab = arch_int(info, architecture, "vocab_size");
    // D4: a declared depth past the inventory path's own block budget is
    // refused here too, rather than materialising one cell per declared block.
    if (*blocks > kMaxBlocks) {
        morph.refusal = std::string(kRefusedNoBlocks);
        gaps.push_back("the runtime declares " + std::to_string(*blocks) + " blocks, more than this view's budget of " +
                       py_grouped(kMaxBlocks) + " (native bound, deviation D4)");
        morph.gaps = std::move(gaps);
        return morph;
    }
    morph.cells = dense_decoder_cells(*blocks, *embedding, *feed_forward, *heads, *kv_heads, head_dim, vocab);
    gaps.emplace_back(
        "counted from declared dimensions, not from a tensor inventory: the figures are the dense-decoder "
        "shape this runtime declares, and any tensor the architecture adds beyond it is not represented");
    gaps.push_back("per-cell storage is UNMEASURED without an inventory " + kDash +
                   " a file's headline quantisation is not applied uniformly across tensors");
    if (!vocab || *vocab == 0) {
        gaps.emplace_back(
            "the runtime declares no vocabulary size, so the embedding and output projections are absent "
            "from the breakdown");
    }
    if (!key_length || *key_length == 0) {
        gaps.emplace_back(
            "no declared attention.key_length; head dimension taken as embedding_length / head_count");
    }
    morph.gaps = std::move(gaps);
    return morph;
}

// ── the derived properties ──────────────────────────────────────────────────
Derived derive(const Morphometry& morph) {
    Derived d;
    d.ok = morph.refusal.empty() && !morph.cells.empty();
    for (const auto& cell : morph.cells) {
        d.counted_parameters = add(d.counted_parameters, cell.parameters, "the counted parameter total");
    }
    {
        int64_t total = 0;
        bool known = true;
        for (const auto& cell : morph.cells) {
            if (!cell.nominal_bytes) {
                known = false;
                break;
            }
            total = add(total, *cell.nominal_bytes, "the nominal byte total");
        }
        if (known) d.nominal_bytes = total;
    }
    if (morph.declared_parameters && *morph.declared_parameters > 0) {
        d.coverage = static_cast<double>(d.counted_parameters) / static_cast<double>(*morph.declared_parameters);
    }
    // active_path_gap
    if (!morph.routing_metadata_error.empty()) {
        d.active_path_gap = morph.routing_metadata_error;
    } else {
        d.active_path_gap = active_path_gap_of(morph.tensors, morph.expert_used_count, morph.expert_count);
    }
    // routed_expert_parameters
    if (!morph.tensors.empty()) {
        int64_t routed = 0;
        for (const auto& tensor : morph.tensors) {
            if (is_routed_expert(tensor.name)) {
                routed = add(routed, tensor.parameters, "the routed expert parameter total");
            }
        }
        if (!(routed == 0 && d.active_path_gap)) d.routed_expert_parameters = routed;
    }
    // always_active_parameters
    if (d.routed_expert_parameters && !(*d.routed_expert_parameters == 0 && d.active_path_gap)) {
        d.always_active_parameters =
            sub(d.counted_parameters, *d.routed_expert_parameters, "the always-active parameters");
    }
    // active_parameters
    if (d.routed_expert_parameters && !d.active_path_gap) {
        const int64_t routed = *d.routed_expert_parameters;
        if (routed == 0) {
            d.active_parameters = d.counted_parameters;
        } else {
            const char* what = "the active parameters";
            d.active_parameters = add(sub(d.counted_parameters, routed, what),
                                      mul(routed, *morph.expert_used_count, what) / *morph.expert_count, what);
        }
    }
    // is_mixture_of_experts
    d.is_mixture_of_experts = (d.routed_expert_parameters && *d.routed_expert_parameters != 0) ||
                              d.active_path_gap.has_value() ||
                              (morph.expert_count && *morph.expert_count != 0 && *morph.expert_count > 1);
    // active_fraction
    if (d.active_parameters && d.counted_parameters > 0) {
        d.active_fraction = static_cast<double>(*d.active_parameters) / static_cast<double>(d.counted_parameters);
    }
    // blocks
    {
        struct Entry {
            int64_t parameters = 0;
            int64_t bytes = 0;
            int64_t modules = 0;
            bool known = true;
        };
        std::map<int64_t, Entry> totals;
        for (const auto& cell : morph.cells) {
            if (!cell.block) continue;
            Entry& entry = totals[*cell.block];
            entry.parameters = add(entry.parameters, cell.parameters, "a block's parameter total");
            entry.bytes = add(entry.bytes, cell.nominal_bytes.value_or(0), "a block's nominal bytes");
            entry.modules += 1;
            if (!cell.nominal_bytes) entry.known = false;
        }
        for (const auto& [index, entry] : totals) {
            BlockProfile profile;
            profile.block = index;
            profile.parameters = entry.parameters;
            if (entry.known) profile.nominal_bytes = entry.bytes;
            profile.modules = entry.modules;
            d.blocks.push_back(profile);
        }
    }
    // by_family
    for (const auto family : kFamilies) {
        int64_t total = 0;
        for (const auto& cell : morph.cells) {
            if (module_info(cell.module).family == family) {
                total = add(total, cell.parameters, "a family's parameter total");
            }
        }
        d.by_family.emplace_back(std::string(family), total);
    }
    // block_dispersion
    if (d.blocks.size() >= 3) {
        // Python sorts the ints and `_median` converts the middle one(s).
        std::vector<int64_t> ints;
        for (const auto& profile : d.blocks) ints.push_back(profile.parameters);
        std::sort(ints.begin(), ints.end());
        std::vector<double> values;
        for (const int64_t v : ints) values.push_back(static_cast<double>(v));
        const double median = median_of(values);
        if (median > 0) {
            std::vector<double> deviations;
            for (const auto& profile : d.blocks) {
                deviations.push_back(std::fabs(static_cast<double>(profile.parameters) - median));
            }
            std::sort(deviations.begin(), deviations.end());
            const double mad = median_of(deviations);
            d.dispersion_relative = mad / median;
            for (const auto& profile : d.blocks) {
                const bool outlier = mad <= 0
                                         ? !py_int_equals_float(profile.parameters, median)
                                         : std::fabs(static_cast<double>(profile.parameters) - median) > 3 * mad;
                if (outlier) d.dispersion_outliers.push_back(profile.block);
            }
        }
    }
    return d;
}

// ── the voxel field ─────────────────────────────────────────────────────────
VoxelField voxel_field(const Morphometry& morph, const Derived& derived) {
    VoxelField field;
    field.height = static_cast<int64_t>(kFamilies.size());
    field.depth = stage_count();
    field.source = morph.source;
    if (morph.cells.empty()) return field;
    int64_t maximum = morph.cells.front().parameters;
    for (const auto& cell : morph.cells) maximum = std::max(maximum, cell.parameters);
    std::set<int64_t> blocks;
    for (const auto& cell : morph.cells) {
        if (cell.block) blocks.insert(*cell.block);
    }
    std::map<int64_t, int64_t> rank;
    for (const int64_t block : blocks) {
        const auto position = static_cast<int64_t>(rank.size());
        rank.emplace(block, position);
    }
    std::vector<std::optional<int64_t>> active_by_cell;
    for (const auto& cell : morph.cells) {
        active_by_cell.push_back(derived.active_path_gap
                                     ? std::nullopt
                                     : cell_active_parameters(cell, morph.expert_used_count, morph.expert_count));
    }
    const bool active_known =
        std::all_of(active_by_cell.begin(), active_by_cell.end(), [](const auto& v) { return v.has_value(); });
    std::optional<int64_t> max_active;
    if (active_known) {
        max_active = *active_by_cell.front();
        for (const auto& value : active_by_cell) max_active = std::max(*max_active, *value);
    }
    for (size_t i = 0; i < morph.cells.size(); ++i) {
        const Cell& cell = morph.cells[i];
        const ModuleInfo& info = module_info(cell.module);
        Voxel voxel;
        voxel.x = cell.block ? rank.at(*cell.block) : -1;
        voxel.y = static_cast<int64_t>(family_index(info.family));
        voxel.z = std::min(info.stage, field.depth - 1);
        voxel.module = cell.module;
        voxel.parameters = cell.parameters;
        voxel.intensity = maximum > 0 ? py_truediv(cell.parameters, maximum) : 0.0;
        const auto bits = mean_bits_per_weight(cell.nominal_bytes, cell.parameters);
        if (bits) voxel.bits_per_weight = py_truediv(bits->num, bits->den);
        if (active_known) {
            voxel.active_parameters = active_by_cell[i];
            if (*max_active != 0) voxel.active_intensity = py_truediv(*active_by_cell[i], *max_active);
        }
        field.voxels.push_back(std::move(voxel));
    }
    std::sort(field.voxels.begin(), field.voxels.end(), [](const Voxel& a, const Voxel& b) {
        return std::tie(a.x, a.y, a.z, a.module) < std::tie(b.x, b.y, b.z, b.module);
    });
    field.width = static_cast<int64_t>(blocks.size());
    field.max_parameters = maximum;
    field.has_stack_external =
        std::any_of(field.voxels.begin(), field.voxels.end(), [](const Voxel& v) { return v.x < 0; });
    field.max_active_parameters = max_active;
    return field;
}

// ── JSON ────────────────────────────────────────────────────────────────────
namespace {

void write_morphometry(JsonWriter& json, const Morphometry& morph, const Derived& d) {
    json.begin_object();
    json.key("schema_version");
    json.string(kMorphometrySchema);
    json.key("model");
    json.string(morph.model);
    json.key("source");
    json.string(morph.source);
    json.key("architecture");
    json.string(morph.architecture);
    json.key("family");
    json.string(morph.family);
    json.key("quantization");
    json.string(morph.quantization);
    json.key("declared_parameters");
    json.optional_integer(morph.declared_parameters);
    json.key("context_length");
    json.optional_integer(morph.context_length);
    json.key("embedding_length");
    json.optional_integer(morph.embedding_length);
    json.key("block_count");
    json.optional_integer(morph.block_count);
    json.key("expert_count");
    json.optional_integer(morph.expert_count);
    json.key("expert_used_count");
    json.optional_integer(morph.expert_used_count);
    json.key("routing_metadata_error");
    json.string(morph.routing_metadata_error);
    json.key("refusal");
    json.string(morph.refusal);
    json.key("gaps");
    json.begin_array();
    for (const auto& gap : morph.gaps) json.string(gap);
    json.end_array();
    json.key("native_axis_order");
    json.begin_array();
    json.string(morph.native_axis_order.first);
    json.string(morph.native_axis_order.second);
    json.end_array();

    json.key("tensors");
    json.begin_array();
    for (const auto& tensor : morph.tensors) {
        const ModuleInfo& info = module_info(tensor.module);
        json.begin_object();
        json.key("name");
        json.string(tensor.name);
        json.key("module");
        json.string(tensor.module);
        json.key("block");
        write_optional_block(json, tensor.block);
        json.key("shape");
        json.begin_array();
        for (const int64_t dim : tensor.shape) json.integer(dim);
        json.end_array();
        json.key("element_type");
        json.string(tensor.element_type);
        json.key("parameters");
        json.integer(tensor.parameters);
        json.key("family");
        json.string(info.family);
        json.key("stage");
        json.integer(info.stage);
        json.key("bits_per_weight");
        write_rational(json, tensor_bits_per_weight(tensor));
        json.key("nominal_bytes");
        json.optional_integer(tensor_nominal_bytes(tensor));
        json.end_object();
    }
    json.end_array();

    json.key("cells");
    json.begin_array();
    for (const auto& cell : morph.cells) {
        const ModuleInfo& info = module_info(cell.module);
        json.begin_object();
        json.key("block");
        write_optional_block(json, cell.block);
        json.key("module");
        json.string(cell.module);
        json.key("parameters");
        json.integer(cell.parameters);
        json.key("tensors");
        json.integer(cell.tensors);
        json.key("element_types");
        json.begin_array();
        for (const auto& type : cell.element_types) json.string(type);
        json.end_array();
        json.key("nominal_bytes");
        json.optional_integer(cell.nominal_bytes);
        json.key("routed_parameters");
        json.integer(cell.routed_parameters);
        json.key("family");
        json.string(info.family);
        json.key("stage");
        json.integer(info.stage);
        json.key("label");
        json.string(info.label);
        json.key("mean_bits_per_weight");
        write_rational(json, mean_bits_per_weight(cell.nominal_bytes, cell.parameters));
        json.key("active_parameters");
        json.optional_integer(cell_active_parameters(cell, morph.expert_used_count, morph.expert_count));
        json.end_object();
    }
    json.end_array();

    json.key("derived");
    json.begin_object();
    json.key("ok");
    json.boolean(d.ok);
    json.key("counted_parameters");
    json.integer(d.counted_parameters);
    json.key("nominal_bytes");
    json.optional_integer(d.nominal_bytes);
    json.key("coverage");
    json.optional_number(d.coverage);
    json.key("is_mixture_of_experts");
    json.boolean(d.is_mixture_of_experts);
    json.key("routed_expert_parameters");
    json.optional_integer(d.routed_expert_parameters);
    json.key("always_active_parameters");
    json.optional_integer(d.always_active_parameters);
    json.key("active_path_gap");
    json.optional_string(d.active_path_gap);
    json.key("active_parameters");
    json.optional_integer(d.active_parameters);
    json.key("active_fraction");
    json.optional_number(d.active_fraction);
    json.key("blocks");
    json.begin_array();
    for (const auto& profile : d.blocks) {
        json.begin_object();
        json.key("block");
        json.integer(profile.block);
        json.key("parameters");
        json.integer(profile.parameters);
        json.key("nominal_bytes");
        json.optional_integer(profile.nominal_bytes);
        json.key("modules");
        json.integer(profile.modules);
        json.key("mean_bits_per_weight");
        write_rational(json, mean_bits_per_weight(profile.nominal_bytes, profile.parameters));
        json.end_object();
    }
    json.end_array();
    json.key("by_family");
    json.begin_array();
    for (const auto& [family, total] : d.by_family) {
        json.begin_array();
        json.string(family);
        json.integer(total);
        json.end_array();
    }
    json.end_array();
    json.key("block_dispersion");
    json.begin_object();
    json.key("relative");
    json.optional_number(d.dispersion_relative);
    json.key("outliers");
    json.begin_array();
    for (const int64_t block : d.dispersion_outliers) json.integer(block);
    json.end_array();
    json.end_object();
    json.end_object();
    json.end_object();
}

void write_voxel_field(JsonWriter& json, const VoxelField& field) {
    json.begin_object();
    json.key("width");
    json.integer(field.width);
    json.key("height");
    json.integer(field.height);
    json.key("depth");
    json.integer(field.depth);
    json.key("max_parameters");
    json.integer(field.max_parameters);
    json.key("has_stack_external");
    json.boolean(field.has_stack_external);
    json.key("source");
    json.string(field.source);
    json.key("max_active_parameters");
    json.optional_integer(field.max_active_parameters);
    json.key("has_active_path");
    json.boolean(field.max_active_parameters.has_value());
    json.key("occupied");
    json.integer(static_cast<int64_t>(field.voxels.size()));
    json.key("capacity");
    json.integer(mul(mul(std::max<int64_t>(0, field.width), field.height, "the voxel capacity"), field.depth,
                     "the voxel capacity"));
    json.key("voxels");
    json.begin_array();
    for (const auto& voxel : field.voxels) {
        json.begin_object();
        json.key("x");
        json.integer(voxel.x);
        json.key("y");
        json.integer(voxel.y);
        json.key("z");
        json.integer(voxel.z);
        json.key("module");
        json.string(voxel.module);
        json.key("parameters");
        json.integer(voxel.parameters);
        json.key("intensity");
        json.number(voxel.intensity);
        json.key("bits_per_weight");
        json.optional_number(voxel.bits_per_weight);
        json.key("active_parameters");
        json.optional_integer(voxel.active_parameters);
        json.key("active_intensity");
        json.optional_number(voxel.active_intensity);
        json.end_object();
    }
    json.end_array();
    json.end_object();
}

std::string render(const Morphometry& morph) {
    const Derived derived = derive(morph);
    const VoxelField field = voxel_field(morph, derived);
    JsonWriter json;
    json.begin_object();
    json.key("morphometry");
    write_morphometry(json, morph, derived);
    json.key("voxel_field");
    write_voxel_field(json, field);
    json.key("native_space");
    json.begin_object();
    json.key("axis_order");
    json.begin_array();
    json.string(morph.native_axis_order.first);
    json.string(morph.native_axis_order.second);
    json.end_array();
    json.key("ref");
    json.string(coordinate_space_ref(native_space(morph.native_axis_order.first, morph.native_axis_order.second)));
    json.end_object();
    json.end_object();
    return json.text();
}

}  // namespace

void write_voxel_field_json(JsonWriter& json, const VoxelField& field) { write_voxel_field(json, field); }

Morphometry analyze_or_refuse(const std::string& model, const Payload* payload) {
    const std::string name = resolved_name(model, payload);
    try {
        return analyze(model, payload);
    } catch (const Overflow& error) {
        return overflow_refusal(name, error.what);
    }
}

std::string analysis_json(const std::string& model, const Payload* payload) {
    const std::string name = resolved_name(model, payload);
    try {
        return render(analyze(model, payload));
    } catch (const Overflow& error) {
        // The refusal carries no integer, so rendering it cannot overflow.
        return render(overflow_refusal(name, error.what));
    }
}

}  // namespace ma
