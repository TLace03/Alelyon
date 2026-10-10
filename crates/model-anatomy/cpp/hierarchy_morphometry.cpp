#include "hierarchy_morphometry.hpp"

#include <algorithm>
#include <set>
#include <tuple>
#include <utility>

#include "canonical_space.hpp"
#include "json_writer.hpp"
#include "registration.hpp"

namespace ma {

namespace {

constexpr const char* kTierOrder[] = {"BASE_CONTRACT", "DOMAIN_FAMILY", "APPLICATION", "STUDY_COHORT",
                                      "RESOLUTION_VARIANT"};

// `TIER_LABELS`.
const char* tier_label(int order) {
    static const char* const kLabels[] = {"Base CQCS contract", "Domain template family", "Institution / application",
                                          "Study / cohort", "Resolution variant"};
    return kLabels[order];
}

// `DRAWABLE_CODES`.
bool drawable(const std::string& code) { return code == "EXACT_IDENTITY" || code == "EXACT_AXIS_PERMUTATION"; }

// `_frames_for`: (frames, truncated).
std::pair<std::vector<LayerFrame>, bool> frames_for(const std::vector<const TemplateNode*>& members,
                                                   bool model_measured, const Space* source_space,
                                                   const std::shared_ptr<const VoxelField>& volume) {
    std::vector<const TemplateNode*> concrete;
    for (const auto* node : members) {
        if (node->coordinate_space) concrete.push_back(node);
    }
    const bool truncated = concrete.size() > kMaxLayerFrames;
    if (truncated) concrete.resize(kMaxLayerFrames);
    std::vector<LayerFrame> frames;
    for (const auto* node : concrete) {
        const Space& space = *node->coordinate_space;
        LayerFrame frame;
        frame.template_id = node->ref.template_id;
        frame.template_version = node->ref.version;
        frame.label = node->label;
        frame.space_id = space.space_id;
        frame.space_version = space.version;
        frame.space_commitment = coordinate_space_ref(space);
        frame.transform_policy = node->transform_policy;
        if (!model_measured || source_space == nullptr) {
            frame.state = "MODEL_NOT_MEASURED";
            frame.explanation = "no measured model was supplied, so nothing was registered onto this frame";
            frames.push_back(std::move(frame));
            continue;
        }
        const Report report = analyze_exact_compatibility(*source_space, space);
        frame.compatibility = report.code;
        frame.explanation = report.explanation;
        if (drawable(report.code) && volume) {
            frame.state = "MEASURED";
            frame.field = volume;  // the one shared volume
        } else {
            frame.state = "REGISTRATION_REFUSED";
        }
        frames.push_back(std::move(frame));
    }
    std::stable_sort(frames.begin(), frames.end(), [](const LayerFrame& a, const LayerFrame& b) {
        return std::tie(a.template_id, a.template_version, a.space_id) <
               std::tie(b.template_id, b.template_version, b.space_id);
    });
    return {std::move(frames), truncated};
}

// `_layer_state`.
std::pair<std::string, std::string> layer_state(size_t nodes, size_t concrete, const std::vector<LayerFrame>& frames,
                                                bool model_measured) {
    if (nodes == 0) return {"TIER_UNPOPULATED", "the declared hierarchy holds no template at this tier"};
    if (concrete == 0) {
        return {"COORDINATE_SPACE_ABSENT",
                "this is an abstract lineage tier: it constrains meaning and declares no coordinate space, and no "
                "axes were invented for it"};
    }
    for (const auto& frame : frames) {
        if (frame.field) return {"MEASURED", ""};
    }
    if (!model_measured) {
        return {"MODEL_NOT_MEASURED", "this tier declares a frame; no measured model has been registered onto it"};
    }
    std::set<std::string> codes;
    for (const auto& frame : frames) {
        if (!frame.compatibility.empty()) codes.insert(frame.compatibility);
    }
    std::string joined;
    for (const auto& code : codes) {
        if (!joined.empty()) joined += ", ";
        joined += code;
    }
    if (joined.empty()) joined = "no code returned";
    return {"REGISTRATION_REFUSED", "the model does not register exactly onto this tier's frame (" + joined +
                                        "), so no volume is drawn under it"};
}

}  // namespace

const VoxelField* HierarchyLayer::field() const {
    for (const auto& frame : frames) {
        if (frame.field) return frame.field.get();
    }
    return nullptr;
}

int64_t HierarchyMorphometry::volumes_built() const {
    std::set<const VoxelField*> distinct;
    for (const auto& layer : layers) {
        for (const auto& frame : layer.frames) {
            if (frame.field) distinct.insert(frame.field.get());
        }
    }
    return static_cast<int64_t>(distinct.size());
}

HierarchyMorphometry measure_layers(const Morphometry* morph, const HierarchySnapshot& snapshot) {
    HierarchyMorphometry result;
    std::optional<Derived> derived;
    if (morph != nullptr) derived = derive(*morph);
    result.model_measured = derived && derived->ok;
    // Built ONCE and shared by every frame that earns it.
    std::shared_ptr<const VoxelField> volume;
    std::optional<Space> source_space;
    if (result.model_measured) {
        volume = std::make_shared<const VoxelField>(voxel_field(*morph, *derived));
        source_space = native_space(morph->native_axis_order.first, morph->native_axis_order.second);
    }
    std::vector<std::string> truncated_tiers;
    for (int order = 0; order < 5; ++order) {
        const std::string tier = kTierOrder[order];
        std::vector<const TemplateNode*> members;
        size_t concrete = 0;
        std::set<std::string> gaps;
        for (const auto& node : snapshot.nodes) {
            if (node.tier != tier) continue;
            members.push_back(&node);
            if (node.coordinate_space) ++concrete;
            gaps.insert(node.gaps.begin(), node.gaps.end());
        }
        auto [frames, truncated] =
            frames_for(members, result.model_measured, source_space ? &*source_space : nullptr, volume);
        if (truncated) truncated_tiers.push_back(tier);
        auto [state, reason] = layer_state(members.size(), concrete, frames, result.model_measured);
        HierarchyLayer layer;
        layer.tier = tier;
        layer.order = order;
        layer.label = tier_label(order);
        layer.nodes = static_cast<int64_t>(members.size());
        layer.abstract_nodes = static_cast<int64_t>(members.size() - concrete);
        layer.state = std::move(state);
        layer.reason = std::move(reason);
        layer.frames = std::move(frames);
        layer.gaps.assign(gaps.begin(), gaps.end());
        result.layers.push_back(std::move(layer));
    }
    result.gaps = {
        "the volume is measured ONCE from the model's own inventory and shared by every frame it registers onto: "
        "these layers are frames, not independent measurements of the model",
        "a layer draws only where the registration core returned an exact identity or axis permutation. No layer "
        "here resamples, remaps or aggregates the model's cells onto a different frame",
    };
    if (!result.model_measured) {
        if (morph == nullptr) {
            result.gaps.emplace_back("no model has been measured, so every frame below is UNMEASURED rather than empty");
        } else {
            result.gaps.push_back("the supplied model was not measured (" +
                                  (morph->refusal.empty() ? std::string("no cells") : morph->refusal) +
                                  "), so no frame carries a volume");
        }
    }
    for (const auto& tier : truncated_tiers) {
        result.gaps.push_back("tier " + tier + " declares more than " + std::to_string(kMaxLayerFrames) +
                              " concrete frames; the layer below is the truncated set");
    }
    result.gaps.insert(result.gaps.end(), snapshot.gaps.begin(), snapshot.gaps.end());
    for (const auto& finding : snapshot.findings) result.findings.push_back(finding.code + ": " + finding.detail);
    result.hierarchy_state = snapshot.state;
    result.model = morph != nullptr ? morph->model : "";
    return result;
}

std::string layers_json(const HierarchyMorphometry& result) {
    JsonWriter json;
    const auto strings = [&json](const std::vector<std::string>& items) {
        json.begin_array();
        for (const auto& item : items) json.string(item);
        json.end_array();
    };
    json.begin_object();
    json.key("schema_version");
    json.string(kHierarchyMorphometrySchema);
    json.key("model");
    json.string(result.model);
    json.key("hierarchy_state");
    json.string(result.hierarchy_state);
    json.key("model_measured");
    json.boolean(result.model_measured);
    json.key("volumes_built");
    json.integer(result.volumes_built());
    json.key("layers");
    json.begin_array();
    const VoxelField* shared = nullptr;
    for (const auto& layer : result.layers) {
        const VoxelField* field = layer.field();
        if (shared == nullptr) shared = field;
        json.begin_object();
        json.key("tier");
        json.string(layer.tier);
        json.key("order");
        json.integer(layer.order);
        json.key("label");
        json.string(layer.label);
        json.key("nodes");
        json.integer(layer.nodes);
        json.key("abstract_nodes");
        json.integer(layer.abstract_nodes);
        json.key("state");
        json.string(layer.state);
        json.key("reason");
        json.string(layer.reason);
        json.key("drawn");
        json.boolean(field != nullptr);
        json.key("occupied");
        json.integer(field == nullptr ? 0 : static_cast<int64_t>(field->voxels.size()));
        json.key("frames");
        json.begin_array();
        for (const auto& frame : layer.frames) {
            json.begin_object();
            json.key("template_id");
            json.string(frame.template_id);
            json.key("template_version");
            json.string(frame.template_version);
            json.key("label");
            json.string(frame.label);
            json.key("space_id");
            json.string(frame.space_id);
            json.key("space_version");
            json.string(frame.space_version);
            json.key("space_commitment");
            json.string(frame.space_commitment);
            json.key("state");
            json.string(frame.state);
            json.key("compatibility");
            json.string(frame.compatibility);
            json.key("explanation");
            json.string(frame.explanation);
            json.key("transform_policy");
            strings(frame.transform_policy);
            json.key("drawn");
            json.boolean(frame.field != nullptr);
            json.key("occupied");
            json.integer(frame.field == nullptr ? 0 : static_cast<int64_t>(frame.field->voxels.size()));
            json.end_object();
        }
        json.end_array();
        json.key("gaps");
        strings(layer.gaps);
        json.end_object();
    }
    json.end_array();
    json.key("findings");
    strings(result.findings);
    json.key("gaps");
    strings(result.gaps);
    json.key("field");
    if (shared == nullptr) {
        json.null();
    } else {
        write_voxel_field_json(json, *shared);
    }
    json.end_object();
    return json.text();
}

}  // namespace ma
