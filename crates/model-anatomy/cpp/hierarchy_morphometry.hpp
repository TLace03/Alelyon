// A port of `hierarchy_morphometry.measure_layers` (the Python reference's
// hierarchy_morphometry.py:334-416) with `_frames_for` (:258) and
// `_layer_state` (:306).
//
// The volume is built ONCE and every frame that earns it holds the same
// object (a shared pointer), so `volumes_built` (:224) is a count of distinct
// objects, as Python counts `id(frame.field)`, never a stored figure.
#pragma once

#include <memory>
#include <string>
#include <vector>

#include "morphometry.hpp"
#include "template_hierarchy.hpp"

namespace ma {

inline constexpr const char* kHierarchyMorphometrySchema = "alelyon.lattice.hierarchy-morphometry/0.1";
inline constexpr size_t kMaxLayerFrames = 64;

struct LayerFrame {
    std::string template_id;
    std::string template_version;
    std::string label;
    std::string space_id;
    std::string space_version;
    std::string space_commitment;
    std::string state;
    std::string compatibility;
    std::string explanation;
    std::vector<std::string> transform_policy;
    std::shared_ptr<const VoxelField> field;
};

struct HierarchyLayer {
    std::string tier;
    int order = 0;
    std::string label;
    int64_t nodes = 0;
    int64_t abstract_nodes = 0;
    std::string state;
    std::string reason;
    std::vector<LayerFrame> frames;
    std::vector<std::string> gaps;

    // `HierarchyLayer.field`: the first drawn frame's volume.
    const VoxelField* field() const;
};

struct HierarchyMorphometry {
    std::string model;
    std::string hierarchy_state;
    std::vector<HierarchyLayer> layers;
    std::vector<std::string> findings;
    std::vector<std::string> gaps;
    bool model_measured = false;

    // Distinct volume objects across every drawn frame.
    int64_t volumes_built() const;
};

// `measure_layers(morph, snapshot=snapshot)`. `morph` null is `morph=None`.
HierarchyMorphometry measure_layers(const Morphometry* morph, const HierarchySnapshot& snapshot);

std::string layers_json(const HierarchyMorphometry& result);

}  // namespace ma
