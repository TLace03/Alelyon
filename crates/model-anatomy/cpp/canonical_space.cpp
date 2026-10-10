#include "canonical_space.hpp"

#include <algorithm>
#include <cstdint>

#include "constants.hpp"
#include "sha256.hpp"

namespace ma {

namespace {

constexpr std::string_view kCoordinateSpaceSchema = "alelyon.lattice.coordinate-space/0.1";
constexpr std::string_view kLabelDictionarySchema = "alelyon.lattice.label-dictionary/0.1";
constexpr std::string_view kAxisDomain = "alelyon.lattice.canonical.axis/0.1";
constexpr std::string_view kSpaceDomain = "alelyon.lattice.canonical.coordinate-space/0.1";
constexpr char kAbsent = '\x00';
constexpr char kPresent = '\x01';

void put_u32(std::string& out, uint32_t value) {
    out += static_cast<char>((value >> 24) & 0xFF);
    out += static_cast<char>((value >> 16) & 0xFF);
    out += static_cast<char>((value >> 8) & 0xFF);
    out += static_cast<char>(value & 0xFF);
}

// `_domain`: ASCII identifier and a NUL.
void put_domain(std::string& out, std::string_view identifier) {
    out.append(identifier);
    out += '\x00';
}

// `_string`: u32 big-endian byte length, then the UTF-8 bytes.
void put_string(std::string& out, std::string_view text) {
    put_u32(out, static_cast<uint32_t>(text.size()));
    out.append(text);
}

void put_optional_string(std::string& out, const std::optional<std::string>& text) {
    if (!text) {
        out += kAbsent;
        return;
    }
    out += kPresent;
    put_string(out, *text);
}

void put_strings(std::string& out, const std::vector<std::string>& items) {
    put_u32(out, static_cast<uint32_t>(items.size()));
    for (const auto& item : items) put_string(out, item);
}

void put_pairs(std::string& out, const Pairs& items) {
    put_u32(out, static_cast<uint32_t>(items.size()));
    for (const auto& [key, value] : items) {
        put_string(out, key);
        put_string(out, value);
    }
}

void put_optional_pair(std::string& out, const std::optional<std::pair<std::string, std::string>>& pair) {
    if (!pair) {
        out += kAbsent;
        return;
    }
    out += kPresent;
    put_string(out, pair->first);
    put_string(out, pair->second);
}

std::vector<std::string> sorted(std::vector<std::string> items) {
    std::sort(items.begin(), items.end());
    return items;
}

Pairs sorted(Pairs items) {
    std::sort(items.begin(), items.end());
    return items;
}

// `canonical.axis_bytes`: every field in declared order.
std::string axis_bytes(const Axis& axis) {
    std::string out;
    put_domain(out, kAxisDomain);
    put_string(out, axis.axis_id);
    put_string(out, axis.semantic_id);
    put_string(out, axis.kind);
    put_string(out, axis.scalar_type);
    put_string(out, axis.ordering);
    put_optional_string(out, axis.unit);
    put_optional_string(out, axis.reference_frame);
    put_optional_string(out, axis.calendar);
    put_optional_string(out, axis.timezone);
    put_optional_string(out, axis.orientation);
    put_optional_string(out, axis.origin);
    put_optional_string(out, axis.resolution);
    put_optional_pair(out, axis.bounds);
    put_optional_pair(out, axis.periodicity);
    put_optional_string(out, axis.labels_ref);
    if (!axis.labels) {
        out += kAbsent;
    } else {
        out += kPresent;
        put_strings(out, *axis.labels);
    }
    put_string(out, axis.missingness_policy);
    put_strings(out, sorted(axis.interpolation_policy));
    put_strings(out, sorted(axis.transform_policy));
    put_pairs(out, sorted(axis.metadata));
    return out;
}

}  // namespace

std::string label_dictionary_ref(const std::vector<std::string>& labels) {
    Sha256 digest;
    std::string head(kLabelDictionarySchema);
    head += '\x00';
    digest.update(head);
    for (const auto& label : labels) {
        const uint64_t length = label.size();
        uint8_t prefix[8];
        for (int i = 0; i < 8; ++i) prefix[i] = static_cast<uint8_t>(length >> (56 - 8 * i));
        digest.update(prefix, 8);
        digest.update(label);
    }
    return "sha256:" + digest.hex_digest();
}

Axis block_axis() {
    Axis axis;
    axis.axis_id = "block";
    axis.semantic_id = "alelyon:model.transformer.block";
    axis.kind = "MODEL_LAYER";
    axis.scalar_type = "INTEGER";
    axis.ordering = "ASCENDING";
    axis.transform_policy = {"AXIS_PERMUTATION", "IDENTITY"};  // sorted, as `_name_set` keeps it
    axis.metadata = {{"stack_external", "block index is absent for embedding and output tensors"}};
    return axis;
}

Axis module_axis() {
    Axis axis;
    axis.axis_id = "module";
    axis.semantic_id = "alelyon:model.transformer.module";
    axis.kind = "MODEL_STATE";
    axis.scalar_type = "LABEL";
    axis.ordering = "CANONICAL_LABEL_ORDER";
    const std::vector<std::string> ids = module_ids();
    axis.labels_ref = label_dictionary_ref(ids);
    axis.labels = ids;
    axis.transform_policy = {"AXIS_PERMUTATION", "IDENTITY", "LABEL_REINDEX"};
    return axis;
}

Space canonical_space() {
    Space space;
    space.space_id = std::string(kCanonicalSpaceId);
    space.version = std::string(kCanonicalSpaceVersion);
    space.topology = "RECTANGULAR_TABLE";
    space.axes = {block_axis(), module_axis()};
    space.index_convention = "ZERO_BASED";
    space.metadata = {{"schema", std::string(kMorphometrySchema)}};
    return space;
}

Space native_space(const std::string& first, const std::string& second) {
    Space space = canonical_space();
    space.space_id = std::string(kCanonicalSpaceId) + ".native";
    const auto axis_named = [](const std::string& name) {
        return name == "module" ? module_axis() : block_axis();
    };
    space.axes = {axis_named(first), axis_named(second)};
    return space;
}

std::string coordinate_space_bytes(const Space& space) {
    std::string out;
    put_domain(out, kSpaceDomain);
    put_string(out, kCoordinateSpaceSchema);
    put_string(out, space.space_id);
    put_string(out, space.version);
    put_string(out, space.topology);
    put_u32(out, static_cast<uint32_t>(space.axes.size()));
    for (const auto& axis : space.axes) out += axis_bytes(axis);
    put_string(out, space.index_convention);
    put_optional_string(out, space.unit_system);
    put_optional_string(out, space.reference_frame);
    put_string(out, space.valid_domain_rule);
    put_strings(out, sorted(space.region_atlas_refs));
    put_pairs(out, sorted(space.metadata));
    return out;
}

void normalize(Space& space) {
    for (auto& axis : space.axes) {
        axis.interpolation_policy = sorted(std::move(axis.interpolation_policy));
        axis.transform_policy = sorted(std::move(axis.transform_policy));
        axis.metadata = sorted(std::move(axis.metadata));
    }
    space.region_atlas_refs = sorted(std::move(space.region_atlas_refs));
    space.metadata = sorted(std::move(space.metadata));
}

std::string coordinate_space_ref(const Space& space) {
    return "sha256:" + sha256_hex(coordinate_space_bytes(space));
}

}  // namespace ma
