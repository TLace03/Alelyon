// The reachable subset of `registration.analyze_exact_compatibility`
// (the Python reference's registration.py:1602-1841), decided on 2026-10-07:
// only the identity and axis-permutation paths.
//
// Morphometry registers a model's native space onto the canonical one, and the
// two differ at most in axis ORDER (`morphometry._native_axis_order`), so only
// these checks are reached, and they run here in Python's order: unsupported
// topology, topology identity, unsupported coordinate features, registration
// metadata gaps, axis count, the space-wide key (`exact_space_key`, which leaves
// out `space_id` and `version`), the axis-key multiset, the policy matching
// (`_build_matching_problem` :670, `_unique_policy_matching` :764, with its
// edge-visit budget), and the transform constructor's policy check.
//
// Not ported (deviation D7): when the axis-key multisets differ, Python goes on
// to the single-field rungs (registration.py:1480-1600) and the composed rung
// (`_composed_rung` :1149). Here that case is the native refusal NOT_PORTED,
// never a guess at what those rungs would have answered.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "canonical_space.hpp"

namespace ma {

class JsonWriter;

// `MAX_MATCHING_EDGE_VISITS`.
inline constexpr int64_t kMaxMatchingEdgeVisits = int64_t{1} << 20;
// `MAX_COMPATIBILITY_EVIDENCE_ITEMS`.
inline constexpr size_t kMaxCompatibilityEvidenceItems = 128;

// The native code for the unported rungs (D7).
inline constexpr const char* kNotPorted = "NOT_PORTED";

struct TransformStep {
    std::string transform_type;  // "IDENTITY" or "AXIS_PERMUTATION"
    std::string loss_class;
    std::string invertibility;
    std::optional<std::vector<int64_t>> source_order;  // AXIS_PERMUTATION only
};

// `CompatibilityReport`.
struct Report {
    std::string code;
    std::string explanation;
    std::optional<std::vector<TransformStep>> transform;  // the chain
    std::optional<std::string> failing_constraint;
    std::vector<std::string> evidence;
    bool can_retry = false;

    bool compatible() const { return transform.has_value(); }
};

// `analyze_exact_compatibility(source_space=, target_space=)` with no
// declarations, under an edge-visit budget (`kMaxMatchingEdgeVisits` is
// Python's; another value is for the budget's own test).
Report analyze_exact_compatibility(const Space& source, const Space& target,
                                   int64_t edge_visit_budget = kMaxMatchingEdgeVisits);

void write_report(JsonWriter& json, const Report& report);

// Python's `repr(str)`, as far as the ported messages reach it: the quote
// Python picks, `\\`, the quote, `\n`, `\r`, `\t` and other C0/DEL/C1 controls
// escaped; every other character as is (D3: Python also escapes a few
// non-ASCII non-printables, which no contract identifier can hold).
std::string py_repr_str(const std::string& text);

// Python's `len(str)`: code points of valid UTF-8.
size_t py_len(const std::string& text);

}  // namespace ma
