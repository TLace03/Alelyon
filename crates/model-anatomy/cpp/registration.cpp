#include "registration.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <string_view>
#include <utility>

#include "json_writer.hpp"

namespace ma {

namespace {

// `INITIAL_EXACT_TOPOLOGIES`.
bool exact_topology(const std::string& topology) { return topology == "RECTANGULAR_TABLE"; }

// The loss class and invertibility both reachable transforms declare
// (`IdentityTransform`, `AxisPermutationTransform`: LOSSLESS, EXACT). The
// generated facts table holds the same two facts read from transforms.py, and
// tests/parity.rs checks the two agree.
constexpr const char* kLossless = "LOSSLESS";
constexpr const char* kExact = "EXACT";

Report refusal(std::string code, std::string explanation, std::string failing_constraint,
               std::vector<std::string> evidence = {}, bool can_retry = false) {
    Report report;
    report.code = std::move(code);
    report.explanation = std::move(explanation);
    report.failing_constraint = std::move(failing_constraint);
    report.evidence = std::move(evidence);
    report.can_retry = can_retry;
    return report;
}

std::string join(const std::vector<std::string>& items, std::string_view separator) {
    std::string out;
    for (size_t i = 0; i < items.size(); ++i) {
        if (i != 0) out += separator;
        out += items[i];
    }
    return out;
}

// `_cap_evidence`.
std::vector<std::string> cap_evidence(std::vector<std::string> items) {
    if (items.size() <= kMaxCompatibilityEvidenceItems) return items;
    const size_t omitted = items.size() - (kMaxCompatibilityEvidenceItems - 1);
    items.resize(kMaxCompatibilityEvidenceItems - 1);
    items.push_back("omitted:" + std::to_string(omitted));
    return items;
}

// `_summarize_items(items, character_budget=1024)`; lengths in code points.
std::string summarize_items(const std::vector<std::string>& items) {
    constexpr size_t kBudget = 1024;
    std::vector<std::string> shown;
    size_t used = 0;
    for (const auto& item : items) {
        const size_t added = py_len(item) + (shown.empty() ? 0 : 2);
        if (!shown.empty() && used + added > kBudget) break;
        shown.push_back(item);
        used += added;
    }
    std::string summary = join(shown, ", ");
    if (items.size() > shown.size()) {
        summary += " (+" + std::to_string(items.size() - shown.size()) + " more)";
    }
    return summary;
}

// `CoordinateAxis.unsupported_exact_features`.
std::vector<std::string> axis_unsupported(const Axis& axis) {
    std::vector<std::string> out;
    if (axis.bounds) out.emplace_back("bounds");
    if (axis.origin) out.emplace_back("origin");
    if (axis.resolution) out.emplace_back("resolution");
    if (axis.periodicity) out.emplace_back("periodicity");
    return out;
}

// `CoordinateSpace.unsupported_exact_features`.
std::vector<std::string> space_unsupported(const Space& space) {
    std::vector<std::string> out;
    if (space.valid_domain_rule != "ALL_DECLARED_COORDINATES") out.emplace_back("valid_domain_rule");
    for (const auto& axis : space.axes) {
        for (const auto& feature : axis_unsupported(axis)) out.push_back(axis.axis_id + "." + feature);
    }
    return out;
}

// `CoordinateAxis.registration_metadata_gaps`.
std::vector<std::string> axis_gaps(const Axis& axis) {
    std::vector<std::string> gaps;
    if (axis.transform_policy.empty()) gaps.emplace_back("transform_policy");
    if (axis.scalar_type == "LABEL") {
        if (!axis.labels_ref) gaps.emplace_back("labels_ref");
        if (!axis.labels) gaps.emplace_back("labels");
    }
    if (axis.kind == "TEMPORAL" || axis.scalar_type == "TIMESTAMP") {
        if (!axis.calendar) gaps.emplace_back("calendar");
        if (!axis.timezone) gaps.emplace_back("timezone");
    }
    if (axis.kind == "SPATIAL") {
        if (!axis.unit) gaps.emplace_back("unit");
        if (!axis.reference_frame) gaps.emplace_back("reference_frame");
        if (!axis.orientation) gaps.emplace_back("orientation");
    }
    if ((axis.kind == "CONTINUOUS" || axis.kind == "FREQUENCY" || axis.kind == "SCALE") && !axis.unit) {
        gaps.emplace_back("unit");
    }
    return gaps;
}

// `CoordinateSpace.registration_metadata_gaps`.
std::vector<std::string> space_gaps(const Space& space) {
    std::vector<std::string> out;
    for (const auto& axis : space.axes) {
        for (const auto& field : axis_gaps(axis)) out.push_back(axis.axis_id + "." + field);
    }
    return out;
}

// An injective encoding of a tuple of optional strings and string lists, so
// two keys are equal exactly when every field is.
class KeyBuilder {
public:
    void text(const std::string& value) {
        out_ += 'S';
        out_ += std::to_string(value.size());
        out_ += ':';
        out_ += value;
    }
    void optional(const std::optional<std::string>& value) {
        if (!value) {
            out_ += 'N';
            return;
        }
        text(*value);
    }
    void optional_pair(const std::optional<std::pair<std::string, std::string>>& value) {
        if (!value) {
            out_ += 'N';
            return;
        }
        out_ += 'P';
        text(value->first);
        text(value->second);
    }
    void list(const std::vector<std::string>& values) {
        out_ += 'L';
        out_ += std::to_string(values.size());
        out_ += ':';
        for (const auto& value : values) text(value);
    }
    void optional_list(const std::optional<std::vector<std::string>>& values) {
        if (!values) {
            out_ += 'N';
            return;
        }
        list(*values);
    }
    void pairs(const Pairs& values) {
        out_ += 'M';
        out_ += std::to_string(values.size());
        out_ += ':';
        for (const auto& [key, value] : values) {
            text(key);
            text(value);
        }
    }
    std::string take() { return std::move(out_); }

private:
    std::string out_;
};

// `CoordinateAxis.exact_semantics_key()` with every field included. The
// axis_id and the transform policy are not part of it.
std::string semantics_key(const Axis& axis) {
    KeyBuilder key;
    key.text(axis.semantic_id);
    key.text(axis.kind);
    key.text(axis.scalar_type);
    key.optional(axis.unit);
    key.optional(axis.reference_frame);
    key.optional(axis.calendar);
    key.optional(axis.timezone);
    key.optional(axis.orientation);
    key.optional(axis.origin);
    key.optional(axis.resolution);
    key.optional_pair(axis.bounds);
    key.optional_pair(axis.periodicity);
    key.text(axis.ordering);
    key.optional(axis.labels_ref);
    key.optional_list(axis.labels);
    key.text(axis.missingness_policy);
    key.list(axis.interpolation_policy);
    key.pairs(axis.metadata);
    return key.take();
}

// `CoordinateSpace.exact_space_key()`.
std::string space_key(const Space& space) {
    KeyBuilder key;
    key.text(space.topology);
    key.text(space.index_convention);
    key.optional(space.unit_system);
    key.optional(space.reference_frame);
    key.text(space.valid_domain_rule);
    key.list(space.region_atlas_refs);
    key.pairs(space.metadata);
    return key.take();
}

bool allows(const Axis& axis, std::string_view transform_type) {
    return std::find(axis.transform_policy.begin(), axis.transform_policy.end(), transform_type) !=
           axis.transform_policy.end();
}

struct BudgetExceeded {};

// `_MatchingProblem`.
struct MatchingProblem {
    std::vector<bool> source_identity, source_permutation, target_identity, target_permutation;
    std::vector<std::vector<size_t>> candidates;

    size_t axis_count() const { return candidates.size(); }

    bool allows_edge(size_t source_index, size_t target_index) const {
        if (source_index == target_index) return source_identity[source_index] && target_identity[target_index];
        return source_permutation[source_index] && target_permutation[target_index];
    }
};

// `_build_matching_problem` with nothing relaxed.
MatchingProblem build_matching_problem(const Space& source, const Space& target) {
    std::map<std::string, size_t> classes;
    const auto class_of = [&classes](const Axis& axis) {
        const auto [at, inserted] = classes.try_emplace(semantics_key(axis), classes.size());
        (void)inserted;
        return at->second;
    };
    std::vector<size_t> source_classes, target_classes;
    for (const auto& axis : source.axes) source_classes.push_back(class_of(axis));
    for (const auto& axis : target.axes) target_classes.push_back(class_of(axis));
    std::map<size_t, std::vector<size_t>> targets_by_class;
    for (size_t index = 0; index < target_classes.size(); ++index) {
        targets_by_class[target_classes[index]].push_back(index);
    }
    MatchingProblem problem;
    for (const auto& axis : source.axes) {
        problem.source_identity.push_back(allows(axis, "IDENTITY"));
        problem.source_permutation.push_back(allows(axis, "AXIS_PERMUTATION"));
    }
    for (const auto& axis : target.axes) {
        problem.target_identity.push_back(allows(axis, "IDENTITY"));
        problem.target_permutation.push_back(allows(axis, "AXIS_PERMUTATION"));
    }
    for (const size_t source_class : source_classes) {
        const auto found = targets_by_class.find(source_class);
        problem.candidates.push_back(found == targets_by_class.end() ? std::vector<size_t>{} : found->second);
    }
    return problem;
}

// `_find_policy_matching`: Kuhn's augmenting search, in Python's visiting order,
// spending one unit of `budget` per candidate edge visited.
class Matcher {
public:
    Matcher(const MatchingProblem& problem, int64_t& budget, std::optional<std::pair<size_t, size_t>> forbidden)
        : problem_(problem), budget_(budget), forbidden_(forbidden) {}

    std::optional<std::vector<int64_t>> run() {
        for (size_t source_index = 0; source_index < problem_.axis_count(); ++source_index) {
            std::set<size_t> seen;
            if (!augment(source_index, seen)) return std::nullopt;
        }
        std::vector<int64_t> order(problem_.axis_count(), -1);
        for (const auto& [target_index, source_index] : target_to_source_) {
            order[source_index] = static_cast<int64_t>(target_index);
        }
        for (const int64_t index : order) {
            if (index < 0) return std::nullopt;
        }
        return order;
    }

private:
    bool augment(size_t source_index, std::set<size_t>& seen) {
        for (const size_t target_index : problem_.candidates[source_index]) {
            budget_ -= 1;
            if (budget_ < 0) throw BudgetExceeded{};
            if (forbidden_ && forbidden_->first == source_index && forbidden_->second == target_index) continue;
            if (!problem_.allows_edge(source_index, target_index)) continue;
            if (seen.count(target_index) != 0) continue;
            seen.insert(target_index);
            const auto previous = target_to_source_.find(target_index);
            if (previous == target_to_source_.end() || augment(previous->second, seen)) {
                target_to_source_[target_index] = source_index;
                return true;
            }
        }
        return false;
    }

    const MatchingProblem& problem_;
    int64_t& budget_;
    std::optional<std::pair<size_t, size_t>> forbidden_;
    std::map<size_t, size_t> target_to_source_;
};

// `_unique_policy_matching`: (one matching or none, whether it is the only one).
std::pair<std::optional<std::vector<int64_t>>, bool> unique_policy_matching(const MatchingProblem& problem,
                                                                            int64_t budget) {
    auto first = Matcher(problem, budget, std::nullopt).run();
    if (!first) return {std::nullopt, false};
    for (size_t source_index = 0; source_index < first->size(); ++source_index) {
        const auto target_index = static_cast<size_t>((*first)[source_index]);
        if (Matcher(problem, budget, std::make_pair(source_index, target_index)).run()) return {first, false};
    }
    return {first, true};
}

// `ProhibitedTransformError(role, axis_id, transform_type)` as a refusal.
Report prohibited(const std::string& role, const std::string& axis_id, const std::string& transform_type) {
    return refusal("PROHIBITED_TRANSFORM", role + " axis " + py_repr_str(axis_id) + " does not allow " + transform_type,
                   "transform_policy", {"role:" + role, "axis:" + axis_id, "transform:" + transform_type}, true);
}

// `_require_axis_policy(transform_type, target_axis, source_axis)`: the target
// is checked first.
std::optional<Report> require_axis_policy(const std::string& transform_type, const Axis& target_axis,
                                          const Axis& source_axis) {
    if (!allows(target_axis, transform_type)) return prohibited("target", target_axis.axis_id, transform_type);
    if (!allows(source_axis, transform_type)) return prohibited("source", source_axis.axis_id, transform_type);
    return std::nullopt;
}

std::string decimal(size_t value) { return std::to_string(value); }

}  // namespace

size_t py_len(const std::string& text) {
    size_t count = 0;
    for (const char c : text) {
        if ((static_cast<unsigned char>(c) & 0xC0) != 0x80) ++count;
    }
    return count;
}

std::string py_repr_str(const std::string& text) {
    const bool has_single = text.find('\'') != std::string::npos;
    const bool has_double = text.find('"') != std::string::npos;
    const char quote = (has_single && !has_double) ? '"' : '\'';
    static const char* const kHex = "0123456789abcdef";
    std::string out(1, quote);
    for (size_t i = 0; i < text.size(); ++i) {
        const auto u = static_cast<unsigned char>(text[i]);
        if (text[i] == quote || text[i] == '\\') {
            out += '\\';
            out += text[i];
        } else if (text[i] == '\n') {
            out += "\\n";
        } else if (text[i] == '\r') {
            out += "\\r";
        } else if (text[i] == '\t') {
            out += "\\t";
        } else if (u < 0x20 || u == 0x7F) {
            out += "\\x";
            out += kHex[u >> 4];
            out += kHex[u & 15];
        } else if (u == 0xC2 && i + 1 < text.size() && static_cast<unsigned char>(text[i + 1]) >= 0x80 &&
                   static_cast<unsigned char>(text[i + 1]) <= 0x9F) {
            // A C1 control (U+0080..U+009F).
            const auto v = static_cast<unsigned char>(text[i + 1]);
            out += "\\x";
            out += kHex[v >> 4];
            out += kHex[v & 15];
            ++i;
        } else {
            out += text[i];
        }
    }
    out += quote;
    return out;
}

Report analyze_exact_compatibility(const Space& source, const Space& target, int64_t edge_visit_budget) {
    std::vector<std::string> unsupported_topologies;
    for (const auto& [role, space] : {std::pair<const char*, const Space*>{"source", &source}, {"target", &target}}) {
        if (!exact_topology(space->topology)) unsupported_topologies.push_back(std::string(role) + ":" + space->topology);
    }
    if (!unsupported_topologies.empty()) {
        return refusal("UNSUPPORTED_TOPOLOGY", "the bounded exact slice supports only rectangular tables",
                       "supported_topology", unsupported_topologies);
    }
    if (source.topology != target.topology) {
        // Unreachable while one topology is exact; ported for its order.
        return refusal("INCOMPATIBLE_TOPOLOGY",
                       "source topology " + source.topology + " does not match target topology " + target.topology,
                       "topology_identity", {"source:" + source.topology, "target:" + target.topology});
    }
    const auto source_unsupported = space_unsupported(source);
    const auto target_unsupported = space_unsupported(target);
    if (!source_unsupported.empty() || !target_unsupported.empty()) {
        std::vector<std::string> details, evidence;
        if (!source_unsupported.empty()) {
            details.push_back("source: " + summarize_items(source_unsupported));
            for (const auto& item : source_unsupported) evidence.push_back("source." + item);
        }
        if (!target_unsupported.empty()) {
            details.push_back("target: " + summarize_items(target_unsupported));
            for (const auto& item : target_unsupported) evidence.push_back("target." + item);
        }
        return refusal("UNSUPPORTED_VALUE_SEMANTICS",
                       "the bounded exact slice cannot enforce declared coordinate features (" + join(details, "; ") + ")",
                       "supported_coordinate_domain", cap_evidence(std::move(evidence)));
    }
    const auto source_gaps = space_gaps(source);
    const auto target_gaps = space_gaps(target);
    if (!source_gaps.empty() || !target_gaps.empty()) {
        std::vector<std::string> details, evidence;
        if (!source_gaps.empty()) {
            details.push_back("source: " + summarize_items(source_gaps));
            for (const auto& gap : source_gaps) evidence.push_back("source." + gap);
        }
        if (!target_gaps.empty()) {
            details.push_back("target: " + summarize_items(target_gaps));
            for (const auto& gap : target_gaps) evidence.push_back("target." + gap);
        }
        return refusal("INSUFFICIENT_METADATA",
                       "strict exact registration requires explicit metadata (" + join(details, "; ") + ")",
                       "registration_metadata", cap_evidence(std::move(evidence)), true);
    }
    if (source.axes.size() != target.axes.size()) {
        return refusal("INCOMPATIBLE_TOPOLOGY", "source and target axis counts differ", "axis_count",
                       {"source:" + decimal(source.axes.size()), "target:" + decimal(target.axes.size())});
    }
    if (space_key(source) != space_key(target)) {
        return refusal("INCOMPATIBLE_SEMANTICS", "space-wide declared coordinate metadata differs",
                       "space_metadata_identity");
    }
    std::vector<std::string> source_keys, target_keys;
    for (const auto& axis : source.axes) source_keys.push_back(semantics_key(axis));
    for (const auto& axis : target.axes) target_keys.push_back(semantics_key(axis));
    std::sort(source_keys.begin(), source_keys.end());
    std::sort(target_keys.begin(), target_keys.end());
    if (source_keys != target_keys) {
        // D7: Python tries the single-field and composed rungs here.
        return refusal(kNotPorted,
                       "the source and target axis semantics differ; Python would try its single-field and "
                       "composed registration rungs here, which this port does not include",
                       "registration_rung_not_ported");
    }
    const MatchingProblem problem = build_matching_problem(source, target);
    std::pair<std::optional<std::vector<int64_t>>, bool> matched;
    try {
        matched = unique_policy_matching(problem, edge_visit_budget);
    } catch (const BudgetExceeded&) {
        const std::string budget = std::to_string(edge_visit_budget);
        return refusal("RESOURCE_BUDGET_EXCEEDED",
                       "the bounded axis-correspondence search reached its fixed work budget of " + budget +
                           " edge visits",
                       "matching_work_budget", {"axes:" + decimal(source.axes.size()), "edge_visit_budget:" + budget},
                       true);
    }
    const auto& [order, unique] = matched;
    if (!order) {
        return refusal("PROHIBITED_TRANSFORM", "declared transform policies prohibit every complete semantic mapping",
                       "transform_policy", {}, true);
    }
    if (!unique) {
        return refusal("AMBIGUOUS_MAPPING", "more than one semantic mapping satisfies the declared policies",
                       "unique_axis_correspondence");
    }
    bool identity = true;
    for (size_t index = 0; index < order->size(); ++index) {
        if ((*order)[index] != static_cast<int64_t>(index)) identity = false;
    }
    Report report;
    if (identity) {
        // `IdentityTransform(target_space, source_space)`'s policy check.
        for (size_t index = 0; index < target.axes.size(); ++index) {
            if (auto refused = require_axis_policy("IDENTITY", target.axes[index], source.axes[index])) return *refused;
        }
        report.code = "EXACT_IDENTITY";
        report.explanation = "ordered declared coordinate metadata matches exactly";
        report.transform = std::vector<TransformStep>{{"IDENTITY", kLossless, kExact, std::nullopt}};
        return report;
    }
    // `AxisPermutationTransform(target_space, source_space, order)`'s policy check.
    for (size_t source_index = 0; source_index < order->size(); ++source_index) {
        const auto target_index = static_cast<size_t>((*order)[source_index]);
        const std::string policy = source_index == target_index ? "IDENTITY" : "AXIS_PERMUTATION";
        if (auto refused = require_axis_policy(policy, target.axes[target_index], source.axes[source_index])) {
            return *refused;
        }
    }
    report.code = "EXACT_AXIS_PERMUTATION";
    report.explanation = "one unique exact semantic axis permutation exists";
    report.transform = std::vector<TransformStep>{{"AXIS_PERMUTATION", kLossless, kExact, *order}};
    return report;
}

void write_report(JsonWriter& json, const Report& report) {
    json.begin_object();
    json.key("code");
    json.string(report.code);
    json.key("compatible");
    json.boolean(report.compatible());
    json.key("explanation");
    json.string(report.explanation);
    json.key("transform");
    if (!report.transform) {
        json.null();
    } else {
        json.begin_object();
        json.key("transforms");
        json.begin_array();
        for (const auto& step : *report.transform) {
            json.begin_object();
            json.key("transform_type");
            json.string(step.transform_type);
            json.key("loss_class");
            json.string(step.loss_class);
            json.key("invertibility");
            json.string(step.invertibility);
            json.key("source_order");
            if (!step.source_order) {
                json.null();
            } else {
                json.begin_array();
                for (const int64_t index : *step.source_order) json.integer(index);
                json.end_array();
            }
            json.end_object();
        }
        json.end_array();
        // A chain declares its weakest member's class; every reachable member
        // is LOSSLESS and EXACT, so the chain is too.
        json.key("loss_class");
        json.string(kLossless);
        json.key("invertibility");
        json.string(kExact);
        json.end_object();
    }
    json.key("failing_constraint");
    json.optional_string(report.failing_constraint);
    json.key("evidence");
    json.begin_array();
    for (const auto& item : report.evidence) json.string(item);
    json.end_array();
    json.key("can_retry_with_metadata_or_policy");
    json.boolean(report.can_retry);
    json.end_object();
}

}  // namespace ma
