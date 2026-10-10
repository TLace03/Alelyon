// A port of the Python reference's morphometry comparison (PR 4):
// two measured morphometry results compared cell by cell on the canonical
// (block, module) frame. Signed deltas are right - left; the relative
// parameter difference is the exact reduced rational (right - left) / left; a
// cell reported on one side only gets no numeric delta. Pure and
// deterministic, like the Python.
//
// The input is the record `compare` reads (`Record`): the fields of
// `ModelMorphometry` the comparison touches, typed. Python's checks of a
// field's container type (non-text metadata, a cells value that is not a
// tuple, a bool or float count, a malformed axis-order tuple) have no input
// here to fire on (deviation D13 in the crate README).
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "morphometry.hpp"

namespace ma {

class JsonWriter;

// `MORPHOMETRY_COMPARISON_SCHEMA`.
inline constexpr const char* kComparisonSchema = "alelyon.lattice.model-morphometry-comparison/0.1";

// The part of `ModelMorphometry` that `compare` reads, as given.
struct Record {
    std::string model;
    std::string source;
    std::string schema_version;
    std::string refusal;
    std::vector<std::string> gaps;
    std::pair<std::string, std::string> native_axis_order{"block", "module"};
    std::optional<int64_t> expert_count;
    std::optional<int64_t> expert_used_count;
    std::vector<Cell> cells;
};

// The record of an `analyze` result (its schema version is the current one).
Record record_of(const Morphometry& morph);

struct CellComparison {
    std::optional<int64_t> block;
    std::string module;
    std::string presence;  // "BOTH", "LEFT_ONLY" or "RIGHT_ONLY"
    std::optional<int64_t> left_parameters;
    std::optional<int64_t> right_parameters;
    std::optional<int64_t> parameter_delta;
    std::optional<int64_t> absolute_parameter_difference;
    std::optional<Rational> relative_parameter_difference;  // reduced, den > 0
    std::optional<int64_t> left_nominal_bytes;
    std::optional<int64_t> right_nominal_bytes;
    std::optional<int64_t> nominal_bytes_delta;
    std::optional<int64_t> left_active_parameters;
    std::optional<int64_t> right_active_parameters;
    std::optional<int64_t> active_parameters_delta;
    std::vector<std::string> left_element_types;
    std::vector<std::string> right_element_types;
};

// `MorphometryComparison`.
struct Comparison {
    std::string code;  // a `ComparisonCode` value
    std::string explanation;
    std::string left_model;
    std::string right_model;
    std::string left_source;
    std::string right_source;
    std::string space_ref;
    std::vector<CellComparison> cells;
    std::vector<std::string> gaps;
    std::vector<std::string> left_gaps;
    std::vector<std::string> right_gaps;
    std::vector<std::string> unmeasured_sides;

    bool ok() const { return code == "COMPARED"; }
    // `MorphometryComparison.complete`.
    bool complete() const;
};

// `morphometry_compare.compare(left, right)`.
Comparison compare(const Record& left, const Record& right);

// The comparison's document, as tools/model_anatomy_goldens.py records it.
void write_comparison(JsonWriter& json, const Comparison& comparison);

}  // namespace ma
