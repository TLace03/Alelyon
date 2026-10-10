// Coordinate spaces and their content commitment: a port of
// `morphometry.canonical_space`/`native_space`/`space_commitment`, of the
// `contracts.CoordinateAxis`/`CoordinateSpace` fields the registration subset
// reads (PR 2), and of `canonical.coordinate_space_bytes`/`axis_bytes` and
// `contracts.label_dictionary_ref`.
//
// Every field of both records is modelled, with the contracts' defaults. The
// contracts' constructors validate and normalise; the port does not re-run
// that validation (deviation D6 in the crate README): a space handed to it is
// taken as one the contract would construct. It does apply the normalisation
// the comparisons depend on (`normalize`): policies, region refs and metadata
// sorted, as `_name_set` and `_metadata` sort them.
#pragma once

#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace ma {

using Pairs = std::vector<std::pair<std::string, std::string>>;

struct Axis {
    std::string axis_id;
    std::string semantic_id;
    std::string kind;         // AxisKind's value
    std::string scalar_type;  // ScalarType's value
    std::string ordering;     // CoordinateOrdering's value
    std::optional<std::string> unit;
    std::optional<std::string> reference_frame;
    std::optional<std::string> calendar;
    std::optional<std::string> timezone;
    std::optional<std::string> orientation;
    std::optional<std::string> origin;
    std::optional<std::string> resolution;
    std::optional<std::pair<std::string, std::string>> bounds;       // (lower, upper)
    std::optional<std::pair<std::string, std::string>> periodicity;  // (period, phase)
    std::optional<std::string> labels_ref;
    std::optional<std::vector<std::string>> labels;
    std::string missingness_policy = "TYPED";
    std::vector<std::string> interpolation_policy;
    std::vector<std::string> transform_policy;
    Pairs metadata;
};

struct Space {
    std::string space_id;
    std::string version;
    std::string topology;  // TopologyType's value
    std::vector<Axis> axes;
    std::string index_convention;
    std::optional<std::string> unit_system;
    std::optional<std::string> reference_frame;
    std::string valid_domain_rule = "ALL_DECLARED_COORDINATES";
    std::vector<std::string> region_atlas_refs;
    Pairs metadata;
};

// The contracts' sorting of policies, region refs and metadata (by code point,
// which is UTF-8 byte order).
void normalize(Space& space);

// `label_dictionary_ref(labels)`: "sha256:" + hex.
std::string label_dictionary_ref(const std::vector<std::string>& labels);

Axis block_axis();
Axis module_axis();
Space canonical_space();
// `native_space` for an axis order of ("block", "module") or ("module", "block").
Space native_space(const std::string& first, const std::string& second);

std::string coordinate_space_bytes(const Space& space);
std::string coordinate_space_ref(const Space& space);

}  // namespace ma
