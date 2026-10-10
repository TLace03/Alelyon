#include "template_hierarchy.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <tuple>
#include <utility>

#include "constants.hpp"
#include "json_writer.hpp"
#include "pyfmt.hpp"
#include "registration.hpp"

namespace ma {

namespace {

constexpr const char* kTiers[] = {"BASE_CONTRACT", "DOMAIN_FAMILY", "APPLICATION", "STUDY_COHORT",
                                  "RESOLUTION_VARIANT"};

// `LIFECYCLE_GAPS`.
const std::vector<std::string>& lifecycle_gaps() {
    static const std::vector<std::string> gaps = {
        "TEMPLATE_MANIFEST_ABSENT: nodes are read descriptors; no CanonicalTemplate manifest protocol exists",
        "TEMPLATE_PERSISTENCE_ABSENT: the hierarchy is rebuilt in memory and writes no store",
        "TEMPLATE_PUBLICATION_ABSENT: AVAILABLE means structurally renderable, not published or approved",
        "TEMPLATE_SIGNING_ABSENT: no key or signature authenticates this hierarchy",
        "TEMPLATE_MIGRATION_ABSENT: no migration between template versions is represented",
        "TEMPLATE_SEMANTIC_NARROWING_UNMEASURED: structural parent, tier, and cycle validation does not prove "
        "that child constraints narrow parent semantics",
    };
    return gaps;
}

// A Unicode Cc code point at `text[i]` (C0, DEL, or C1 as U+0080..U+009F).
bool control_at(const std::string& text, size_t i) {
    const auto u = static_cast<unsigned char>(text[i]);
    if (u < 0x20 || u == 0x7F) return true;
    return u == 0xC2 && i + 1 < text.size() && static_cast<unsigned char>(text[i + 1]) >= 0x80 &&
           static_cast<unsigned char>(text[i + 1]) <= 0x9F;
}

// `template_hierarchy._text`: NFC (a no-op here, D8), strip (ASCII, D3),
// non-empty, at most `maximum` code points, no Cc character.
std::string clean(const std::string& value, const std::string& field, size_t maximum) {
    std::string normalized(py_strip(value));
    if (normalized.empty()) throw TemplateError{field + " must not be empty"};
    if (py_len(normalized) > maximum) {
        throw TemplateError{field + " exceeds the " + std::to_string(maximum) + "-character limit"};
    }
    for (size_t i = 0; i < normalized.size(); ++i) {
        if (control_at(normalized, i)) throw TemplateError{field + " must not contain control characters"};
    }
    return normalized;
}

template <typename T>
std::vector<T> sorted_unique(std::vector<T> items) {
    std::sort(items.begin(), items.end());
    items.erase(std::unique(items.begin(), items.end()), items.end());
    return items;
}

// `_text_tuple`.
std::vector<std::string> text_tuple(const std::vector<std::string>& values, const std::string& field,
                                    size_t maximum_items) {
    if (values.size() > maximum_items) {
        throw TemplateError{field + " exceeds the " + std::to_string(maximum_items) + "-item limit"};
    }
    std::vector<std::string> out;
    for (const auto& value : values) out.push_back(clean(value, field + " item", 2048));
    return sorted_unique(std::move(out));
}

TemplateRef construct_ref(const TemplateRef& raw) {
    return {clean(raw.template_id, "template_id", 512), clean(raw.version, "version", 128)};
}

using SpaceKey = std::optional<std::tuple<std::string, std::string, std::string, std::vector<std::string>>>;

SpaceKey space_key(const TemplateNode& node) {
    if (!node.coordinate_space) return std::nullopt;
    const Space& space = *node.coordinate_space;
    std::vector<std::string> axes;
    for (const auto& axis : space.axes) axes.push_back(axis.axis_id);
    return std::make_tuple(space.space_id, space.version, space.topology, axes);
}

// `_node_sort_key`. An absent space sorts first, as Python's `()` does.
bool node_less(const TemplateNode& a, const TemplateNode& b) {
    return std::forward_as_tuple(tier_order(a.tier), a.ref, a.label, a.description, a.parent_refs) <
               std::forward_as_tuple(tier_order(b.tier), b.ref, b.label, b.description, b.parent_refs) ||
           (std::forward_as_tuple(tier_order(a.tier), a.ref, a.label, a.description, a.parent_refs) ==
                std::forward_as_tuple(tier_order(b.tier), b.ref, b.label, b.description, b.parent_refs) &&
            std::make_tuple(space_key(a), a.transform_policy, a.gaps) <
                std::make_tuple(space_key(b), b.transform_policy, b.gaps));
}

// `_finding_sort_key`.
std::tuple<std::string, std::string, std::string, std::string> finding_key(const HierarchyFinding& f) {
    return {f.code, f.node_ref ? f.node_ref->key() : "", f.parent_ref ? f.parent_ref->key() : "", f.detail};
}

void sort_findings(std::vector<HierarchyFinding>& findings) {
    std::stable_sort(findings.begin(), findings.end(), [](const auto& a, const auto& b) {
        return finding_key(a) < finding_key(b);
    });
}

HierarchyFinding finding(std::string code, std::string detail, std::optional<TemplateRef> node_ref = std::nullopt,
                         std::optional<TemplateRef> parent_ref = std::nullopt) {
    return {clean(code, "code", 128), clean(detail, "detail", 4096), std::move(node_ref), std::move(parent_ref)};
}

}  // namespace

int tier_order(const std::string& tier) {
    for (int i = 0; i < 5; ++i) {
        if (tier == kTiers[i]) return i;
    }
    throw TemplateError{"tier must be a TemplateTier"};
}

TemplateNode construct_node(TemplateNode raw) {
    TemplateNode node;
    node.ref = construct_ref(raw.ref);
    tier_order(raw.tier);
    node.tier = raw.tier;
    node.label = clean(raw.label, "label", 512);
    node.description = clean(raw.description, "description", 4096);
    if (raw.parent_refs.size() > kMaxParentRefs) {
        throw TemplateError{"parent_refs exceeds the " + std::to_string(kMaxParentRefs) + "-item limit"};
    }
    for (const auto& parent : raw.parent_refs) node.parent_refs.push_back(construct_ref(parent));
    node.parent_refs = sorted_unique(std::move(node.parent_refs));
    if (raw.coordinate_space) {
        node.coordinate_space = std::move(raw.coordinate_space);
        normalize(*node.coordinate_space);
    }
    node.transform_policy = text_tuple(raw.transform_policy, "transform_policy", 128);
    node.gaps = text_tuple(raw.gaps, "gaps", 128);
    return node;
}

HierarchySnapshot snapshot_template_hierarchy(std::vector<TemplateNode> nodes) {
    HierarchySnapshot snapshot;
    snapshot.gaps = lifecycle_gaps();
    std::vector<HierarchyFinding> findings;
    if (nodes.size() > kMaxTemplateNodes) {
        findings.push_back(finding("NODE_BUDGET_EXCEEDED",
                                   "hierarchy exceeds the " + std::to_string(kMaxTemplateNodes) +
                                       "-node inspection budget"));
        nodes.resize(kMaxTemplateNodes);
    }
    if (nodes.empty()) {
        sort_findings(findings);
        snapshot.state = "EMPTY";
        snapshot.findings = std::move(findings);
        return snapshot;
    }
    std::stable_sort(nodes.begin(), nodes.end(), node_less);
    std::map<TemplateRef, size_t> by_ref;  // into `unique`
    std::vector<TemplateNode> unique;
    for (auto& node : nodes) {
        if (by_ref.count(node.ref) != 0) {
            findings.push_back(finding("DUPLICATE_TEMPLATE_REF", "more than one node declares " + node.ref.key(),
                                       node.ref));
            continue;
        }
        by_ref.emplace(node.ref, unique.size());
        unique.push_back(std::move(node));
    }
    // Already in `_node_sort_key` order: the first of each ref, in sorted order.
    for (const auto& node : unique) {
        if (node.tier == "BASE_CONTRACT" && !node.parent_refs.empty()) {
            findings.push_back(finding("BASE_CONTRACT_HAS_PARENT",
                                       "a BASE_CONTRACT is a hierarchy root and cannot name a parent", node.ref));
        } else if (node.tier != "BASE_CONTRACT" && node.parent_refs.empty()) {
            findings.push_back(
                finding("DERIVED_TEMPLATE_HAS_NO_PARENT", node.tier + " must name at least one parent", node.ref));
        }
        for (const auto& parent_ref : node.parent_refs) {
            const auto parent = by_ref.find(parent_ref);
            if (parent == by_ref.end()) {
                findings.push_back(finding("UNKNOWN_PARENT",
                                           node.ref.key() + " names parent " + parent_ref.key() + ", which is absent",
                                           node.ref, parent_ref));
                continue;
            }
            const TemplateNode& parent_node = unique[parent->second];
            if (tier_order(parent_node.tier) >= tier_order(node.tier)) {
                findings.push_back(finding("TIER_ORDER_INVALID",
                                           "parent " + parent_node.tier + " must sit above child " + node.tier,
                                           node.ref, parent_ref));
            }
        }
    }
    // Kahn's algorithm. Which order the ready refs are taken in does not change
    // which refs are reached, so a sorted set stands in for Python's heap.
    std::map<TemplateRef, int64_t> indegree;
    std::map<TemplateRef, std::vector<TemplateRef>> children;
    for (const auto& [ref, index] : by_ref) {
        (void)index;
        indegree[ref] = 0;
        children[ref];
    }
    for (const auto& node : unique) {
        for (const auto& parent_ref : node.parent_refs) {
            if (by_ref.count(parent_ref) == 0) continue;
            indegree[node.ref] += 1;
            children[parent_ref].push_back(node.ref);
        }
    }
    std::set<TemplateRef> ready;
    for (const auto& [ref, count] : indegree) {
        if (count == 0) ready.insert(ref);
    }
    std::set<TemplateRef> visited;
    while (!ready.empty()) {
        const TemplateRef ref = *ready.begin();
        ready.erase(ready.begin());
        visited.insert(ref);
        auto kids = children[ref];
        std::sort(kids.begin(), kids.end());
        for (const auto& child : kids) {
            if (--indegree[child] == 0) ready.insert(child);
        }
    }
    std::vector<TemplateRef> cyclic;
    for (const auto& [ref, index] : by_ref) {
        (void)index;
        if (visited.count(ref) == 0) cyclic.push_back(ref);  // `by_ref` is a sorted map
    }
    if (!cyclic.empty()) {
        std::string preview;
        for (size_t i = 0; i < cyclic.size() && i < 8; ++i) {
            if (i != 0) preview += ", ";
            preview += cyclic[i].key();
        }
        findings.push_back(finding("CYCLE_DETECTED", "parent graph contains a cycle involving " + preview, cyclic[0]));
    }
    findings = sorted_unique(std::move(findings));
    sort_findings(findings);
    snapshot.state = findings.empty() ? "AVAILABLE" : "INVALID";
    snapshot.nodes = std::move(unique);
    snapshot.findings = std::move(findings);
    return snapshot;
}

HierarchySnapshot model_morphometry_hierarchy() {
    const std::vector<std::string> abstract_gap = {
        "COORDINATE_SPACE_ABSENT: this is an abstract lineage tier; no axes were invented for it"};
    const TemplateRef base{"alelyon.cqcs.contract", "0.1.0"};
    const TemplateRef domain{"alelyon.model.template-family", "0.1.0"};
    const TemplateRef application{"alelyon.model.transformer", "0.1.0"};
    const TemplateRef study{"alelyon.model.transformer.declared-anatomy", "0.1.0"};
    const TemplateRef leaf{std::string(kCanonicalSpaceId), std::string(kCanonicalSpaceVersion)};
    const Space space = canonical_space();
    std::vector<std::string> policy;
    for (const auto& axis : space.axes) policy.insert(policy.end(), axis.transform_policy.begin(), axis.transform_policy.end());
    policy = sorted_unique(std::move(policy));
    std::vector<std::string> study_gaps = abstract_gap;
    study_gaps.emplace_back("COHORT_CONSTRUCTION_ABSENT: no population-derived template is claimed");
    std::vector<TemplateNode> nodes = {
        {base, "BASE_CONTRACT", "CQCS base contract",
         "Universal coordinate-contract semantics; abstract rather than a fabricated universal axis grid.", {},
         std::nullopt, {}, abstract_gap},
        {domain, "DOMAIN_FAMILY", "Model domain family", "The model-structure branch of the CQCS hierarchy.", {base},
         std::nullopt, {}, abstract_gap},
        {application, "APPLICATION", "Transformer application", "Transformer block and module conventions.",
         {domain}, std::nullopt, {}, abstract_gap},
        {study, "STUDY_COHORT", "Declared-anatomy profile",
         "The declaration-only model anatomy profile; it is not a population-derived cohort template.",
         {application}, std::nullopt, {}, study_gaps},
        {leaf, "RESOLUTION_VARIANT", "Canonical (block, module) frame",
         "The immutable coordinate space used by the current declared model-morphometry analysis.", {study}, space,
         policy,
         {"TEMPLATE_MANIFEST_ABSENT: this leaf is an existing CoordinateSpace, not a published template manifest"}},
    };
    std::vector<TemplateNode> constructed;
    for (auto& node : nodes) constructed.push_back(construct_node(std::move(node)));
    return snapshot_template_hierarchy(std::move(constructed));
}

void write_ref(JsonWriter& json, const std::optional<TemplateRef>& ref) {
    if (!ref) {
        json.null();
        return;
    }
    json.begin_object();
    json.key("template_id");
    json.string(ref->template_id);
    json.key("version");
    json.string(ref->version);
    json.end_object();
}

void write_snapshot(JsonWriter& json, const HierarchySnapshot& snapshot) {
    const auto strings = [&json](const std::vector<std::string>& items) {
        json.begin_array();
        for (const auto& item : items) json.string(item);
        json.end_array();
    };
    json.begin_object();
    json.key("schema_version");
    json.string(kTemplateHierarchySchema);
    json.key("state");
    json.string(snapshot.state);
    json.key("nodes");
    json.begin_array();
    for (const auto& node : snapshot.nodes) {
        json.begin_object();
        json.key("ref");
        write_ref(json, node.ref);
        json.key("tier");
        json.string(node.tier);
        json.key("label");
        json.string(node.label);
        json.key("description");
        json.string(node.description);
        json.key("parent_refs");
        json.begin_array();
        for (const auto& parent : node.parent_refs) write_ref(json, parent);
        json.end_array();
        json.key("coordinate_space");
        if (!node.coordinate_space) {
            json.null();
        } else {
            const Space& space = *node.coordinate_space;
            json.begin_object();
            json.key("space_id");
            json.string(space.space_id);
            json.key("version");
            json.string(space.version);
            json.key("topology");
            json.string(space.topology);
            json.key("axes");
            json.begin_array();
            for (const auto& axis : space.axes) json.string(axis.axis_id);
            json.end_array();
            json.key("index_convention");
            json.string(space.index_convention);
            json.key("ref");
            json.string(coordinate_space_ref(space));
            json.end_object();
        }
        json.key("transform_policy");
        strings(node.transform_policy);
        json.key("gaps");
        strings(node.gaps);
        json.end_object();
    }
    json.end_array();
    json.key("findings");
    json.begin_array();
    for (const auto& f : snapshot.findings) {
        json.begin_object();
        json.key("code");
        json.string(f.code);
        json.key("detail");
        json.string(f.detail);
        json.key("node_ref");
        write_ref(json, f.node_ref);
        json.key("parent_ref");
        write_ref(json, f.parent_ref);
        json.end_object();
    }
    json.end_array();
    json.key("gaps");
    strings(snapshot.gaps);
    json.key("roots");
    json.begin_array();
    for (const auto& node : snapshot.nodes) {
        if (node.parent_refs.empty()) write_ref(json, node.ref);
    }
    json.end_array();
    json.end_object();
}

}  // namespace ma
