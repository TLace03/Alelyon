// `morphometry_compare.compare`, in Python's order of checks: each record's
// container types (D13: unreachable here), the measured operands, the schema
// version, the producer source, the registration onto the canonical frame,
// then each side's cells; then the union of coordinates, sorted with the
// stack-external row first, compared right - left.
#include "compare.hpp"

#include <algorithm>
#include <map>
#include <numeric>
#include <string_view>

#include "canonical_space.hpp"
#include "checked.hpp"
#include "constants.hpp"
#include "json_writer.hpp"
#include "registration.hpp"

namespace ma {

namespace {

using Key = std::pair<std::optional<int64_t>, std::string>;

// `_sort_key`: (block is not None, -1 if block is None else block, module).
// Modules compare by code point, which is UTF-8 byte order.
struct KeyLess {
    bool operator()(const Key& a, const Key& b) const {
        const bool a_has = a.first.has_value();
        const bool b_has = b.first.has_value();
        if (a_has != b_has) return !a_has;
        const int64_t a_block = a_has ? *a.first : -1;
        const int64_t b_block = b_has ? *b.first : -1;
        if (a_block != b_block) return a_block < b_block;
        return a.second < b.second;
    }
};

// Python's `repr((block, module))`.
std::string key_repr(const Key& key) {
    return "(" + (key.first ? std::to_string(*key.first) : std::string("None")) + ", " +
           py_repr_str(key.second) + ")";
}

Comparison refusal(std::string code, std::string explanation, const Record& left, const Record& right,
                   std::vector<std::string> unmeasured_sides = {}) {
    // `_refusal`: `safe_text` and `safe_gaps` keep only text; every field here
    // is already text (D13).
    Comparison out;
    out.code = std::move(code);
    out.explanation = std::move(explanation);
    out.left_model = left.model;
    out.right_model = right.model;
    out.left_source = left.source;
    out.right_source = right.source;
    out.left_gaps = left.gaps;
    out.right_gaps = right.gaps;
    out.unmeasured_sides = std::move(unmeasured_sides);
    return out;
}

bool measured(const Record& record) { return record.refusal.empty() && !record.cells.empty(); }

// `_registration_problem`.
std::string registration_problem(const Record& record, const char* side) {
    const auto& [first, second] = record.native_axis_order;
    const bool valid = (first == "block" && second == "module") || (first == "module" && second == "block");
    if (!valid) {
        return std::string(side) + " has invalid native axis order (" + py_repr_str(first) + ", " +
               py_repr_str(second) + ")";
    }
    const Report report = analyze_exact_compatibility(native_space(first, second), canonical_space());
    if (!report.compatible()) {
        return std::string(side) + " registration refused with " + report.code + ": " + report.explanation;
    }
    return "";
}

// `_cell_map`: the cells validated in order, keyed by coordinate. On a problem
// the map is empty and `problem` names it.
std::map<Key, const Cell*, KeyLess> cell_map(const Record& record, const char* side, std::string& problem) {
    static const std::vector<std::string> kIds = module_ids();
    std::map<Key, const Cell*, KeyLess> cells;
    for (size_t index = 0; index < record.cells.size(); ++index) {
        const Cell& cell = record.cells[index];
        const std::string at = std::string(side) + " cell " + std::to_string(index);
        if (cell.block && *cell.block < 0) {
            problem = at + " has invalid block " + std::to_string(*cell.block);
            return {};
        }
        // Validated against the labels frozen into the canonical commitment.
        if (std::find(kIds.begin(), kIds.end(), cell.module) == kIds.end()) {
            problem = at + " has unknown module " + py_repr_str(cell.module);
            return {};
        }
        if (cell.parameters <= 0) {
            problem = at + " has invalid parameter count " + std::to_string(cell.parameters);
            return {};
        }
        if (cell.tensors <= 0) {
            problem = at + " has invalid tensor count " + std::to_string(cell.tensors);
            return {};
        }
        if (cell.nominal_bytes && *cell.nominal_bytes < 0) {
            problem = at + " has invalid nominal storage " + std::to_string(*cell.nominal_bytes);
            return {};
        }
        if (cell.routed_parameters < 0 || cell.routed_parameters > cell.parameters) {
            problem = at + " has invalid routed parameter count " + std::to_string(cell.routed_parameters);
            return {};
        }
        if (cell.routed_parameters != 0 && cell.module != "ffn_expert") {
            problem = at + " assigns routed expert parameters to non-expert module " + py_repr_str(cell.module);
            return {};
        }
        Key key{cell.block, cell.module};
        if (cells.count(key) != 0) {
            problem = std::string(side) + " has duplicate canonical cell " + key_repr(key) +
                      "; neither measurement was selected";
            return {};
        }
        cells.emplace(std::move(key), &cell);
    }
    return cells;
}

// `_active_parameters`: incoherent routing stays UNMEASURED. The cell was
// validated (0 <= routed <= parameters), so with `routed % total == 0` and
// `used <= total`, `routed * used // total` is `(routed / total) * used`,
// which is at most `routed`: no intermediate leaves int64.
std::optional<int64_t> active_parameters(const Cell* cell, const Record& record) {
    if (cell == nullptr) return std::nullopt;
    if (cell->routed_parameters == 0) return cell->parameters;
    const auto used = record.expert_used_count;
    const auto total = record.expert_count;
    if (!used || !total || *used <= 0 || *total <= 0 || *used > *total) return std::nullopt;
    if (cell->routed_parameters % *total != 0) return std::nullopt;
    const char* what = "a compared cell's active parameters";
    const int64_t dense = sub(cell->parameters, cell->routed_parameters, what);
    return add(dense, mul(cell->routed_parameters / *total, *used, what), what);
}

CellComparison compare_cell(const Key& key, const Cell* left, const Cell* right, const Record& left_record,
                            const Record& right_record) {
    CellComparison out;
    out.block = key.first;
    out.module = key.second;
    out.presence = left == nullptr ? "RIGHT_ONLY" : right == nullptr ? "LEFT_ONLY" : "BOTH";
    out.left_active_parameters = active_parameters(left, left_record);
    out.right_active_parameters = active_parameters(right, right_record);
    if (left != nullptr && right != nullptr) {
        // Both counts are positive (validated), so neither the difference nor
        // its magnitude leaves int64.
        const int64_t delta = right->parameters - left->parameters;
        out.parameter_delta = delta;
        out.absolute_parameter_difference = delta < 0 ? -delta : delta;
        const int64_t divisor = std::gcd(delta, left->parameters);  // > 0: left->parameters > 0
        out.relative_parameter_difference = Rational{delta / divisor, left->parameters / divisor};
        if (left->nominal_bytes && right->nominal_bytes) {
            // Both non-negative (validated).
            out.nominal_bytes_delta = *right->nominal_bytes - *left->nominal_bytes;
        }
        if (out.left_active_parameters && out.right_active_parameters) {
            // Both non-negative: dense + a share of routed, within parameters.
            out.active_parameters_delta = *out.right_active_parameters - *out.left_active_parameters;
        }
    }
    if (left != nullptr) {
        out.left_parameters = left->parameters;
        out.left_nominal_bytes = left->nominal_bytes;
        out.left_element_types = left->element_types;
    }
    if (right != nullptr) {
        out.right_parameters = right->parameters;
        out.right_nominal_bytes = right->nominal_bytes;
        out.right_element_types = right->element_types;
    }
    return out;
}

void write_texts(JsonWriter& json, const std::vector<std::string>& texts) {
    json.begin_array();
    for (const auto& text : texts) json.string(text);
    json.end_array();
}

}  // namespace

Record record_of(const Morphometry& morph) {
    Record out;
    out.model = morph.model;
    out.source = morph.source;
    out.schema_version = std::string(kMorphometrySchema);
    out.refusal = morph.refusal;
    out.gaps = morph.gaps;
    out.native_axis_order = morph.native_axis_order;
    out.expert_count = morph.expert_count;
    out.expert_used_count = morph.expert_used_count;
    out.cells = morph.cells;
    return out;
}

bool Comparison::complete() const {
    if (!ok() || !gaps.empty() || !left_gaps.empty() || !right_gaps.empty()) return false;
    return std::all_of(cells.begin(), cells.end(), [](const CellComparison& cell) {
        return cell.presence == "BOTH" && cell.parameter_delta && cell.nominal_bytes_delta &&
               cell.active_parameters_delta;
    });
}

Comparison compare(const Record& left, const Record& right) {
    // `_record_problem` (non-text metadata, non-tuple cells or gaps) has no
    // representable input here (D13).

    std::vector<std::string> unmeasured;
    if (!measured(left)) unmeasured.emplace_back("left");
    if (!measured(right)) unmeasured.emplace_back("right");
    if (!unmeasured.empty()) {
        std::string reasons;
        for (const auto* side : {&left, &right}) {
            if (measured(*side)) continue;
            if (!reasons.empty()) reasons += "; ";
            reasons += side == &left ? "left: " : "right: ";
            reasons += side->refusal.empty() ? std::string("no measured cells") : side->refusal;
        }
        return refusal("INPUT_UNMEASURED", "comparison requires two measured operands (" + reasons + ")", left,
                       right, std::move(unmeasured));
    }

    const std::string schema(kMorphometrySchema);
    if (left.schema_version != schema || right.schema_version != schema) {
        return refusal("SCHEMA_MISMATCH",
                       "comparison requires both operands on current schema " + py_repr_str(schema) +
                           "; received left=" + py_repr_str(left.schema_version) +
                           ", right=" + py_repr_str(right.schema_version) + ". The current frame was not assumed.",
                       left, right);
    }

    std::string invalid_sources;
    for (const auto* side : {&left, &right}) {
        if (side->source == kSourceTensorInventory || side->source == kSourceDeclaredArchitecture) continue;
        if (!invalid_sources.empty()) invalid_sources += ", ";
        invalid_sources += side == &left ? "left" : "right";
    }
    if (!invalid_sources.empty()) {
        return refusal("INVALID_RECORD",
                       "measured operands require a current producer source; invalid side(s): " + invalid_sources,
                       left, right);
    }

    for (const auto* side : {&left, &right}) {
        const std::string problem = registration_problem(*side, side == &left ? "left" : "right");
        if (!problem.empty()) return refusal("REGISTRATION_REFUSED", problem, left, right);
    }

    std::string problem;
    const auto left_cells = cell_map(left, "left", problem);
    if (!problem.empty()) return refusal("INVALID_CELLS", problem, left, right);
    const auto right_cells = cell_map(right, "right", problem);
    if (!problem.empty()) return refusal("INVALID_CELLS", problem, left, right);

    std::map<Key, std::pair<const Cell*, const Cell*>, KeyLess> keys;
    for (const auto& [key, cell] : left_cells) keys[key].first = cell;
    for (const auto& [key, cell] : right_cells) keys[key].second = cell;

    Comparison out;
    out.code = "COMPARED";
    out.explanation =
        "declared structure compared cell by cell on the current canonical frame; signed deltas are right minus "
        "left and imply no capability or learned-behaviour ranking";
    out.left_model = left.model;
    out.right_model = right.model;
    out.left_source = left.source;
    out.right_source = right.source;
    out.space_ref = coordinate_space_ref(canonical_space());
    int64_t left_only = 0;
    int64_t right_only = 0;
    for (const auto& [key, pair] : keys) {
        out.cells.push_back(compare_cell(key, pair.first, pair.second, left, right));
        if (pair.second == nullptr) ++left_only;
        if (pair.first == nullptr) ++right_only;
    }
    if (left_only != 0) {
        out.gaps.push_back(std::to_string(left_only) +
                           " canonical cell(s) were reported on the left only; absence is not measured zero");
    }
    if (right_only != 0) {
        out.gaps.push_back(std::to_string(right_only) +
                           " canonical cell(s) were reported on the right only; absence is not measured zero");
    }
    out.left_gaps = left.gaps;
    out.right_gaps = right.gaps;
    return out;
}

void write_comparison(JsonWriter& json, const Comparison& c) {
    json.begin_object();
    json.key("schema_version");
    json.string(kComparisonSchema);
    json.key("code");
    json.string(c.code);
    json.key("ok");
    json.boolean(c.ok());
    json.key("complete");
    json.boolean(c.complete());
    json.key("explanation");
    json.string(c.explanation);
    json.key("left_model");
    json.string(c.left_model);
    json.key("right_model");
    json.string(c.right_model);
    json.key("left_source");
    json.string(c.left_source);
    json.key("right_source");
    json.string(c.right_source);
    json.key("space_ref");
    json.string(c.space_ref);
    json.key("cells");
    json.begin_array();
    for (const auto& cell : c.cells) {
        json.begin_object();
        json.key("block");
        json.optional_integer(cell.block);
        json.key("module");
        json.string(cell.module);
        json.key("presence");
        json.string(cell.presence);
        json.key("left_parameters");
        json.optional_integer(cell.left_parameters);
        json.key("right_parameters");
        json.optional_integer(cell.right_parameters);
        json.key("parameter_delta");
        json.optional_integer(cell.parameter_delta);
        json.key("absolute_parameter_difference");
        json.optional_integer(cell.absolute_parameter_difference);
        json.key("relative_parameter_difference");
        if (cell.relative_parameter_difference) {
            json.begin_array();
            json.integer(cell.relative_parameter_difference->num);
            json.integer(cell.relative_parameter_difference->den);
            json.end_array();
        } else {
            json.null();
        }
        json.key("left_nominal_bytes");
        json.optional_integer(cell.left_nominal_bytes);
        json.key("right_nominal_bytes");
        json.optional_integer(cell.right_nominal_bytes);
        json.key("nominal_bytes_delta");
        json.optional_integer(cell.nominal_bytes_delta);
        json.key("left_active_parameters");
        json.optional_integer(cell.left_active_parameters);
        json.key("right_active_parameters");
        json.optional_integer(cell.right_active_parameters);
        json.key("active_parameters_delta");
        json.optional_integer(cell.active_parameters_delta);
        json.key("left_element_types");
        write_texts(json, cell.left_element_types);
        json.key("right_element_types");
        write_texts(json, cell.right_element_types);
        json.end_object();
    }
    json.end_array();
    json.key("gaps");
    write_texts(json, c.gaps);
    json.key("left_gaps");
    write_texts(json, c.left_gaps);
    json.key("right_gaps");
    write_texts(json, c.right_gaps);
    json.key("unmeasured_sides");
    write_texts(json, c.unmeasured_sides);
    json.end_object();
}

}  // namespace ma
