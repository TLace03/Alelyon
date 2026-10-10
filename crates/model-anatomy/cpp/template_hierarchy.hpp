// A port of the Python reference's `template_hierarchy.py`:
// `snapshot_template_hierarchy` (:298-411, the structural findings and Kahn's
// algorithm), the records' normalisation, and `model_morphometry_hierarchy`
// (:419-495, a constant input).
//
// Text is normalised as `_text` does, except that Unicode NFC normalisation is
// a no-op and `strip()` is ASCII-only (deviations D8 and D3): a node built by
// hand with a decomposed character or a non-ASCII space keeps it here.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "canonical_space.hpp"

namespace ma {

class JsonWriter;

inline constexpr const char* kTemplateHierarchySchema = "alelyon.lattice.template-hierarchy/0.1";
inline constexpr size_t kMaxTemplateNodes = 1024;
inline constexpr size_t kMaxParentRefs = 16;

struct TemplateRef {
    std::string template_id;
    std::string version;
    std::string key() const { return template_id + "@" + version; }
    auto operator<=>(const TemplateRef&) const = default;
};

struct TemplateNode {
    TemplateRef ref;
    std::string tier;  // TemplateTier's value
    std::string label;
    std::string description;
    std::vector<TemplateRef> parent_refs;
    std::optional<Space> coordinate_space;
    std::vector<std::string> transform_policy;
    std::vector<std::string> gaps;
};

struct HierarchyFinding {
    std::string code;
    std::string detail;
    std::optional<TemplateRef> node_ref;
    std::optional<TemplateRef> parent_ref;
    auto operator<=>(const HierarchyFinding&) const = default;
};

struct HierarchySnapshot {
    std::string state;  // EMPTY, INVALID or AVAILABLE
    std::vector<TemplateNode> nodes;
    std::vector<HierarchyFinding> findings;
    std::vector<std::string> gaps;  // LIFECYCLE_GAPS
};

// Why a node could not be constructed (Python's TypeError/ValueError message).
struct TemplateError {
    std::string what;
};

// `TIER_ORDER[tier]`; throws TemplateError for an unknown tier.
int tier_order(const std::string& tier);

// `TemplateNode(...)`'s normalisation and checks; throws TemplateError.
TemplateNode construct_node(TemplateNode raw);

// `snapshot_template_hierarchy(nodes)` over constructed nodes.
HierarchySnapshot snapshot_template_hierarchy(std::vector<TemplateNode> nodes);

// `model_morphometry_hierarchy()`.
HierarchySnapshot model_morphometry_hierarchy();

void write_snapshot(JsonWriter& json, const HierarchySnapshot& snapshot);
void write_ref(JsonWriter& json, const std::optional<TemplateRef>& ref);

}  // namespace ma
