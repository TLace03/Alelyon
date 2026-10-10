// The C ABI's entry points. Each is noexcept and contains every exception:
// invalid input becomes MA_INVALID_INPUT with {"error": ...}, anything else
// (allocation failure) becomes MA_INTERNAL.
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <initializer_list>
#include <new>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "canonical_space.hpp"
#include "checked.hpp"
#include "compare.hpp"
#include "constants.hpp"
#include "dequant.hpp"
#include "economics.hpp"
#include "foundry.hpp"
#include "hierarchy_morphometry.hpp"
#include "json_writer.hpp"
#include "ma_abi.hpp"
#include "morphometry.hpp"
#include "probe.hpp"
#include "pyfmt.hpp"
#include "registration.hpp"
#include "sha256.hpp"
#include "template_hierarchy.hpp"
#include "weights.hpp"

namespace {

struct InvalidInput {
    std::string reason;
};

std::string_view view(ma_str text, const char* field) {
    if (text.ptr == nullptr) {
        if (text.len != 0) throw InvalidInput{std::string(field) + " is null with a nonzero length"};
        return std::string_view();
    }
    const std::string_view out(text.ptr, text.len);
    if (!ma::valid_utf8(out)) throw InvalidInput{std::string(field) + " is not UTF-8"};
    return out;
}

bool canonical_decimal(std::string_view text) {
    if (text.empty()) return false;
    size_t at = text[0] == '-' ? 1 : 0;
    if (at == text.size()) return false;
    if (text[at] == '0') return text.size() == at + 1 && at == 0;  // "0", never "-0" or "007"
    for (; at < text.size(); ++at) {
        if (text[at] < '0' || text[at] > '9') return false;
    }
    return true;
}

ma::Value value_of(const ma_meta& item) {
    ma::Value value;
    switch (item.tag) {
        case MA_VALUE_NONE:
            value.tag = ma::Value::Tag::None;
            break;
        case MA_VALUE_BOOL:
            if (item.boolean > 1) throw InvalidInput{"a bool value is neither 0 nor 1"};
            value.tag = ma::Value::Tag::Bool;
            value.boolean = item.boolean == 1;
            break;
        case MA_VALUE_INT:
            value.tag = ma::Value::Tag::Int;
            value.text = std::string(view(item.text, "an int value"));
            if (!canonical_decimal(value.text)) throw InvalidInput{"an int value is not canonical decimal text"};
            break;
        case MA_VALUE_F64:
            value.tag = ma::Value::Tag::F64;
            value.f64 = item.f64;
            break;
        case MA_VALUE_STR:
            value.tag = ma::Value::Tag::Str;
            value.text = std::string(view(item.text, "a str value"));
            break;
        default:
            throw InvalidInput{"a value has an unknown tag"};
    }
    return value;
}

ma::Payload convert(const ma_payload& in) {
    ma::Payload out;
    out.model = std::string(view(in.model, "model"));
    out.family = std::string(view(in.family, "family"));
    out.quantization_level = std::string(view(in.quantization_level, "quantization_level"));
    out.parent_model = std::string(view(in.parent_model, "parent_model"));
    if (in.metadata == nullptr && in.metadata_len != 0) throw InvalidInput{"metadata is null with a nonzero length"};
    for (size_t i = 0; i < in.metadata_len; ++i) {
        const ma_meta& item = in.metadata[i];
        ma::Value value;
        switch (item.tag) {
            case MA_VALUE_NONE:
                value.tag = ma::Value::Tag::None;
                break;
            case MA_VALUE_BOOL:
                if (item.boolean > 1) throw InvalidInput{"a bool value is neither 0 nor 1"};
                value.tag = ma::Value::Tag::Bool;
                value.boolean = item.boolean == 1;
                break;
            case MA_VALUE_INT:
                value.tag = ma::Value::Tag::Int;
                value.text = std::string(view(item.text, "an int value"));
                if (!canonical_decimal(value.text)) throw InvalidInput{"an int value is not canonical decimal text"};
                break;
            case MA_VALUE_F64:
                value.tag = ma::Value::Tag::F64;
                value.f64 = item.f64;
                break;
            case MA_VALUE_STR:
                value.tag = ma::Value::Tag::Str;
                value.text = std::string(view(item.text, "a str value"));
                break;
            default:
                throw InvalidInput{"a metadata value has an unknown tag"};
        }
        out.info.set(std::string(view(item.key, "a metadata key")), std::move(value));
    }
    if (in.has_tensors > 1) throw InvalidInput{"has_tensors is neither 0 nor 1"};
    out.has_tensors = in.has_tensors == 1;
    if (in.tensors == nullptr && in.tensors_len != 0) throw InvalidInput{"tensors is null with a nonzero length"};
    if (!out.has_tensors && in.tensors_len != 0) throw InvalidInput{"tensors given with has_tensors = 0"};
    out.tensors.reserve(in.tensors_len);
    for (size_t i = 0; i < in.tensors_len; ++i) {
        const ma_tensor& item = in.tensors[i];
        ma::TensorInput tensor;
        tensor.name = std::string(view(item.name, "a tensor name"));
        tensor.type_name = std::string(view(item.type_name, "a tensor type"));
        if (item.dims == nullptr && item.ndims != 0) throw InvalidInput{"a tensor's dims are null"};
        tensor.dims.assign(item.dims, item.dims + item.ndims);
        out.tensors.push_back(std::move(tensor));
    }
    return out;
}

template <typename T>
const T* items(const T* pointer, size_t len, const char* field) {
    if (pointer == nullptr && len != 0) throw InvalidInput{std::string(field) + " is null with a nonzero length"};
    return pointer;
}

bool flag(uint32_t value, const char* field) {
    if (value > 1) throw InvalidInput{std::string(field) + " is neither 0 nor 1"};
    return value == 1;
}

std::optional<std::string> optional_text(const ma_opt_str& value, const char* field) {
    if (!flag(value.present, field)) {
        if (value.text.len != 0) throw InvalidInput{std::string(field) + " is absent but has text"};
        return std::nullopt;
    }
    return std::string(view(value.text, field));
}

std::vector<std::string> texts(const ma_str* values, size_t len, const char* field) {
    items(values, len, field);
    std::vector<std::string> out;
    out.reserve(len);
    for (size_t i = 0; i < len; ++i) out.emplace_back(view(values[i], field));
    return out;
}

ma::Pairs pairs(const ma_pair* values, size_t len, const char* field) {
    items(values, len, field);
    ma::Pairs out;
    for (size_t i = 0; i < len; ++i) out.emplace_back(std::string(view(values[i].key, field)), std::string(view(values[i].value, field)));
    return out;
}

std::string member(ma_str value, const char* field, std::initializer_list<std::string_view> allowed) {
    std::string text(view(value, field));
    for (const auto option : allowed) {
        if (text == option) return text;
    }
    throw InvalidInput{std::string(field) + " is not one of its enumeration's values"};
}

ma::Space convert_space(const ma_space& in) {
    ma::Space out;
    out.space_id = std::string(view(in.space_id, "space_id"));
    out.version = std::string(view(in.version, "version"));
    out.topology = member(in.topology, "topology",
                          {"DENSE_REGULAR_GRID", "SPARSE_REGULAR_GRID", "RAGGED_LABELED_ARRAY", "RECTANGULAR_TABLE",
                           "IRREGULAR_POINT_CLOUD", "UNSTRUCTURED_MESH", "EVENT_STREAM", "DIRECTED_GRAPH",
                           "UNDIRECTED_GRAPH", "HYPERGRAPH", "PRODUCT_SPACE"});
    items(in.axes, in.axes_len, "axes");
    // The contract's MAX_AXES: a space with more is not one it constructs.
    if (in.axes_len == 0 || in.axes_len > 64) throw InvalidInput{"a space holds 1 to 64 axes"};
    for (size_t i = 0; i < in.axes_len; ++i) {
        const ma_axis& a = in.axes[i];
        ma::Axis axis;
        axis.axis_id = std::string(view(a.axis_id, "axis_id"));
        axis.semantic_id = std::string(view(a.semantic_id, "semantic_id"));
        axis.kind = member(a.kind, "kind",
                           {"CONTINUOUS", "DISCRETE_ORDINAL", "CATEGORICAL", "TEMPORAL", "SPATIAL", "ENTITY",
                            "SCENARIO", "ENSEMBLE", "FREQUENCY", "SCALE", "GRAPH_NODE", "GRAPH_EDGE", "MESH_VERTEX",
                            "MESH_CELL", "MODEL_LAYER", "MODEL_STATE", "CUSTOM_TYPED"});
        axis.scalar_type = member(a.scalar_type, "scalar_type",
                                  {"INTEGER", "RATIONAL", "DECIMAL", "FLOAT", "TIMESTAMP", "DURATION", "LABEL", "UUID",
                                   "HASH"});
        axis.ordering =
            member(a.ordering, "ordering", {"ASCENDING", "DESCENDING", "CANONICAL_LABEL_ORDER", "UNORDERED"});
        axis.unit = optional_text(a.unit, "unit");
        axis.reference_frame = optional_text(a.reference_frame, "reference_frame");
        axis.calendar = optional_text(a.calendar, "calendar");
        axis.timezone = optional_text(a.timezone, "timezone");
        axis.orientation = optional_text(a.orientation, "orientation");
        axis.origin = optional_text(a.origin, "origin");
        axis.resolution = optional_text(a.resolution, "resolution");
        if (flag(a.has_bounds, "has_bounds")) {
            axis.bounds.emplace(std::string(view(a.bounds_lower, "bounds")), std::string(view(a.bounds_upper, "bounds")));
        }
        if (flag(a.has_periodicity, "has_periodicity")) {
            axis.periodicity.emplace(std::string(view(a.period, "period")), std::string(view(a.phase, "phase")));
        }
        axis.labels_ref = optional_text(a.labels_ref, "labels_ref");
        if (flag(a.has_labels, "has_labels")) {
            axis.labels = texts(a.labels, a.labels_len, "labels");
        } else if (a.labels_len != 0) {
            throw InvalidInput{"labels given with has_labels = 0"};
        }
        axis.missingness_policy = std::string(view(a.missingness_policy, "missingness_policy"));
        axis.interpolation_policy = texts(a.interpolation_policy, a.interpolation_policy_len, "interpolation_policy");
        axis.transform_policy = texts(a.transform_policy, a.transform_policy_len, "transform_policy");
        axis.metadata = pairs(a.metadata, a.metadata_len, "axis metadata");
        out.axes.push_back(std::move(axis));
    }
    out.index_convention = std::string(view(in.index_convention, "index_convention"));
    out.unit_system = optional_text(in.unit_system, "unit_system");
    out.reference_frame = optional_text(in.reference_frame, "reference_frame");
    out.valid_domain_rule = std::string(view(in.valid_domain_rule, "valid_domain_rule"));
    out.region_atlas_refs = texts(in.region_atlas_refs, in.region_atlas_refs_len, "region_atlas_refs");
    out.metadata = pairs(in.metadata, in.metadata_len, "space metadata");
    ma::normalize(out);
    return out;
}

ma::TemplateRef convert_ref(const ma_template_ref& in) {
    return {std::string(view(in.template_id, "template_id")), std::string(view(in.version, "version"))};
}

// Converts and constructs every node (`TemplateNode(...)`), in order.
std::vector<ma::TemplateNode> convert_nodes(const ma_template_node* nodes, size_t len) {
    items(nodes, len, "nodes");
    std::vector<ma::TemplateNode> out;
    // Every node is constructed, as Python's caller constructs every node before
    // the snapshot keeps the first MAX_TEMPLATE_NODES.
    for (size_t i = 0; i < len; ++i) {
        const ma_template_node& n = nodes[i];
        ma::TemplateNode raw;
        raw.ref = convert_ref(n.ref);
        raw.tier = std::string(view(n.tier, "tier"));
        raw.label = std::string(view(n.label, "label"));
        raw.description = std::string(view(n.description, "description"));
        items(n.parent_refs, n.parent_refs_len, "parent_refs");
        for (size_t p = 0; p < n.parent_refs_len; ++p) raw.parent_refs.push_back(convert_ref(n.parent_refs[p]));
        if (n.coordinate_space != nullptr) raw.coordinate_space = convert_space(*n.coordinate_space);
        raw.transform_policy = texts(n.transform_policy, n.transform_policy_len, "transform_policy");
        raw.gaps = texts(n.gaps, n.gaps_len, "gaps");
        try {
            out.push_back(ma::construct_node(std::move(raw)));
        } catch (const ma::TemplateError& error) {
            throw InvalidInput{"template node " + std::to_string(i) + ": " + error.what};
        }
    }
    return out;
}

ma::HierarchySnapshot snapshot_of(uint32_t builtin, const ma_template_node* nodes, size_t nodes_len) {
    if (flag(builtin, "builtin")) {
        if (nodes_len != 0) throw InvalidInput{"nodes given with builtin = 1"};
        return ma::model_morphometry_hierarchy();
    }
    return ma::snapshot_template_hierarchy(convert_nodes(nodes, nodes_len));
}

std::optional<int64_t> optional_integer(const ma_opt_i64& value, const char* field) {
    if (!flag(value.present, field)) {
        if (value.value != 0) throw InvalidInput{std::string(field) + " is absent but has a value"};
        return std::nullopt;
    }
    return value.value;
}

ma::Reading convert_reading(const ma_reading& in) {
    ma::Reading out;
    out.backend = member(in.backend, "backend", {"cuda", "rocm", "mps", "cpu", "absent", "vulkan"});
    out.torch_version = std::string(view(in.torch_version, "torch_version"));
    out.probe = std::string(view(in.probe, "probe"));
    out.has_gpu = flag(in.has_gpu, "has_gpu");
    out.gpu_name = std::string(view(in.gpu_name, "gpu_name"));
    out.gpu_total_bytes = optional_integer(in.gpu_total_bytes, "gpu_total_bytes");
    out.gpu_free_bytes = optional_integer(in.gpu_free_bytes, "gpu_free_bytes");
    if (!out.has_gpu && (!out.gpu_name.empty() || out.gpu_total_bytes || out.gpu_free_bytes)) {
        throw InvalidInput{"a device's facts given with has_gpu = 0"};
    }
    out.ram_total_bytes = optional_integer(in.ram_total_bytes, "ram_total_bytes");
    out.disk_free_bytes = optional_integer(in.disk_free_bytes, "disk_free_bytes");
    switch (in.bf16) {
        case 0:
            out.bf16 = false;
            break;
        case 1:
            out.bf16 = true;
            break;
        case 2:
            break;
        default:
            throw InvalidInput{"bf16 is not 0, 1 or 2"};
    }
    out.gaps = texts(in.gaps, in.gaps_len, "gaps");
    return out;
}

ma::Record convert_record(const ma_morph_record& in) {
    ma::Record out;
    out.model = std::string(view(in.model, "model"));
    out.source = std::string(view(in.source, "source"));
    out.schema_version = std::string(view(in.schema_version, "schema_version"));
    out.refusal = std::string(view(in.refusal, "refusal"));
    out.gaps = texts(in.gaps, in.gaps_len, "gaps");
    out.native_axis_order = {std::string(view(in.native_axis_first, "native_axis_order")),
                             std::string(view(in.native_axis_second, "native_axis_order"))};
    out.expert_count = optional_integer(in.expert_count, "expert_count");
    out.expert_used_count = optional_integer(in.expert_used_count, "expert_used_count");
    items(in.cells, in.cells_len, "cells");
    out.cells.reserve(in.cells_len);
    for (size_t i = 0; i < in.cells_len; ++i) {
        const ma_cell_record& c = in.cells[i];
        ma::Cell cell;
        cell.block = optional_integer(c.block, "a cell's block");
        cell.module = std::string(view(c.module, "a cell's module"));
        cell.parameters = c.parameters;
        cell.tensors = c.tensors;
        cell.element_types = texts(c.element_types, c.element_types_len, "a cell's element types");
        cell.nominal_bytes = optional_integer(c.nominal_bytes, "a cell's nominal bytes");
        cell.routed_parameters = c.routed_parameters;
        out.cells.push_back(std::move(cell));
    }
    return out;
}

std::optional<double> optional_float(const ma_opt_f64& value, const char* field) {
    if (!flag(value.present, field)) {
        // Any bits: a NaN is not equal to 0.0, so compare the representation.
        if (value.value != 0.0 || std::signbit(value.value)) {
            throw InvalidInput{std::string(field) + " is absent but has a value"};
        }
        return std::nullopt;
    }
    return value.value;
}

ma::Throughput convert_throughput(const ma_throughput& in) {
    ma::Throughput out;
    out.model = std::string(view(in.model, "model"));
    out.context = in.context;
    out.decode_tokens_per_second = optional_float(in.decode_tokens_per_second, "decode_tokens_per_second");
    out.prefill_tokens_per_second = optional_float(in.prefill_tokens_per_second, "prefill_tokens_per_second");
    out.method = std::string(view(in.method, "method"));
    out.provenance = std::string(view(in.provenance, "provenance"));
    return out;
}

ma::Energy convert_energy(const ma_energy& in) {
    ma::Energy out;
    out.draw_watts = optional_float(in.draw_watts, "draw_watts");
    out.usd_per_kwh = optional_float(in.usd_per_kwh, "usd_per_kwh");
    out.source = std::string(view(in.source, "source"));
    out.as_of = std::string(view(in.as_of, "as_of"));
    out.provenance = std::string(view(in.provenance, "provenance"));
    return out;
}

ma::Price convert_price(const ma_price& in) {
    ma::Price out;
    out.provider = std::string(view(in.provider, "provider"));
    out.model = std::string(view(in.model, "model"));
    out.usd_per_million_input = optional_float(in.usd_per_million_input, "usd_per_million_input");
    out.usd_per_million_output = optional_float(in.usd_per_million_output, "usd_per_million_output");
    out.source = std::string(view(in.source, "source"));
    out.as_of = std::string(view(in.as_of, "as_of"));
    out.provenance = std::string(view(in.provenance, "provenance"));
    return out;
}

ma::Volume convert_volume(const ma_volume& in) {
    ma::Volume out;
    out.input_tokens = optional_integer(in.input_tokens, "input_tokens");
    out.output_tokens = optional_integer(in.output_tokens, "output_tokens");
    out.stated_by = std::string(view(in.stated_by, "stated_by"));
    return out;
}

ma::LlamacppPayload convert_llamacpp(const ma_llamacpp& in) {
    ma::LlamacppPayload out;
    out.mapping = flag(in.mapping, "mapping");
    if (in.model != nullptr) out.model = value_of(*in.model);
    out.timings_mapping = flag(in.timings_mapping, "timings_mapping");
    items(in.timings, in.timings_len, "timings");
    if (!out.timings_mapping && in.timings_len != 0) throw InvalidInput{"timings given with timings_mapping = 0"};
    for (size_t i = 0; i < in.timings_len; ++i) {
        out.timings.set(std::string(view(in.timings[i].key, "a timings key")), value_of(in.timings[i]));
    }
    return out;
}

int emit(const std::string& text, char** out, size_t* out_len) noexcept {
    if (out == nullptr || out_len == nullptr) return MA_INVALID_INPUT;
    auto* buffer = static_cast<char*>(std::malloc(text.size() + 1));
    if (buffer == nullptr) {
        *out = nullptr;
        *out_len = 0;
        return MA_INTERNAL;
    }
    std::memcpy(buffer, text.data(), text.size());
    buffer[text.size()] = '\0';
    *out = buffer;
    *out_len = text.size();
    return MA_OK;
}

int fail(int code, const std::string& reason, char** out, size_t* out_len) noexcept {
    try {
        ma::JsonWriter json;
        json.begin_object();
        json.key("error");
        json.string(reason);
        json.end_object();
        const int written = emit(json.text(), out, out_len);
        return written == MA_OK ? code : written;
    } catch (...) {
        if (out != nullptr) *out = nullptr;
        if (out_len != nullptr) *out_len = 0;
        return MA_INTERNAL;
    }
}

// Runs `body` (which returns the JSON text) inside the containment boundary.
template <typename Body>
int guarded(char** out, size_t* out_len, Body body) noexcept {
    if (out == nullptr || out_len == nullptr) return MA_INVALID_INPUT;
    *out = nullptr;
    *out_len = 0;
    try {
        return emit(body(), out, out_len);
    } catch (const InvalidInput& error) {
        return fail(MA_INVALID_INPUT, error.reason, out, out_len);
    } catch (const ma::Overflow& error) {
        // Every entry point that computes catches this itself; reaching here
        // would be a defect, reported rather than hidden.
        return fail(MA_INTERNAL, "unhandled integer overflow: " + error.what, out, out_len);
    } catch (const std::bad_alloc&) {
        return fail(MA_INTERNAL, "out of memory", out, out_len);
    } catch (const std::exception& error) {
        return fail(MA_INTERNAL, error.what(), out, out_len);
    } catch (...) {
        return fail(MA_INTERNAL, "unknown failure", out, out_len);
    }
}

std::string text_result(const std::string& text) {
    ma::JsonWriter json;
    json.begin_object();
    json.key("text");
    json.string(text);
    json.end_object();
    return json.text();
}

std::string hex(std::string_view bytes) {
    static const char* const kHex = "0123456789abcdef";
    std::string out;
    out.reserve(bytes.size() * 2);
    for (const char c : bytes) {
        const auto u = static_cast<unsigned char>(c);
        out += kHex[u >> 4];
        out += kHex[u & 15];
    }
    return out;
}

}  // namespace

extern "C" {

uint32_t ma_abi_version(void) noexcept { return MA_ABI_VERSION; }

int ma_analyze(ma_str model, const ma_payload* payload, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const std::string name(view(model, "model argument"));
        if (payload == nullptr) return ma::analysis_json(name, nullptr);
        const ma::Payload converted = convert(*payload);
        return ma::analysis_json(name, &converted);
    });
}

int ma_constants(char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [] {
        ma::JsonWriter json;
        json.begin_object();
        json.key("schema");
        json.string(ma::kMorphometrySchema);
        json.key("canonical_space_id");
        json.string(ma::kCanonicalSpaceId);
        json.key("canonical_space_version");
        json.string(ma::kCanonicalSpaceVersion);
        json.key("sources");
        json.begin_array();
        json.string(ma::kSourceTensorInventory);
        json.string(ma::kSourceDeclaredArchitecture);
        json.end_array();
        json.key("refusals");
        json.begin_array();
        json.string(ma::kRefusedNoMetadata);
        json.string(ma::kRefusedNoBlocks);
        json.string(ma::kRefusedMixtureOfExperts);
        json.string(ma::kRefusedInsufficientArchitecture);
        json.end_array();
        json.key("max_tensors");
        json.integer(ma::kMaxTensors);
        json.key("max_blocks");
        json.integer(ma::kMaxBlocks);
        json.key("families");
        json.begin_array();
        for (const auto family : ma::kFamilies) json.string(family);
        json.end_array();
        json.key("modules");
        json.begin_array();
        for (const auto& module : ma::kModules) {
            json.begin_array();
            json.string(module.id);
            json.string(module.family);
            json.integer(module.stage);
            json.string(module.label);
            json.end_array();
        }
        json.end_array();
        json.key("module_ids");
        json.begin_array();
        for (const auto& id : ma::module_ids()) json.string(id);
        json.end_array();
        json.key("stage_count");
        json.integer(ma::stage_count());
        json.key("name_to_module");
        json.begin_array();
        for (const auto& [fragment, module] : ma::kNameToModule) {
            json.begin_array();
            json.string(fragment);
            json.string(module);
            json.end_array();
        }
        json.end_array();
        json.key("routed_expert_suffix");
        json.string(ma::kRoutedExpertSuffix);
        json.key("nominal_bits_per_weight");
        json.begin_array();
        for (const auto& entry : ma::kNominalBitsPerWeight) {
            json.begin_array();
            json.string(entry.type);
            json.integer(entry.num);
            json.integer(entry.den);
            json.end_array();
        }
        json.end_array();
        json.key("foundry");
        ma::write_foundry_constants(json);
        json.end_object();
        return json.text();
    });
}

int ma_canonical_space(char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [] {
        const ma::Space space = ma::canonical_space();
        const std::string bytes = ma::coordinate_space_bytes(space);
        ma::JsonWriter json;
        json.begin_object();
        json.key("space_id");
        json.string(space.space_id);
        json.key("version");
        json.string(space.version);
        json.key("bytes_hex");
        json.string(hex(bytes));
        json.key("commitment");
        json.string("sha256:" + ma::sha256_hex(bytes));
        json.key("module_labels_ref");
        json.string(*ma::module_axis().labels_ref);
        json.key("native_block_module_ref");
        json.string(ma::coordinate_space_ref(ma::native_space("block", "module")));
        json.key("native_module_block_ref");
        json.string(ma::coordinate_space_ref(ma::native_space("module", "block")));
        json.end_object();
        return json.text();
    });
}

int ma_sha256_hex(const uint8_t* data, size_t len, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (data == nullptr && len != 0) throw InvalidInput{"data is null with a nonzero length"};
        ma::Sha256 hasher;
        if (len != 0) hasher.update(data, len);
        ma::JsonWriter json;
        json.begin_object();
        json.key("hex");
        json.string(hasher.hex_digest());
        json.end_object();
        return json.text();
    });
}

int ma_format_repr(double x, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] { return text_result(ma::py_repr(x)); });
}

int ma_format_fixed(double x, int32_t precision, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (precision < 0 || precision > 1000) throw InvalidInput{"precision is outside 0..1000"};
        return text_result(ma::py_fixed(x, precision));
    });
}

int ma_format_grouped(int64_t x, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] { return text_result(ma::py_grouped(x)); });
}

int ma_register(const ma_space* source, const ma_space* target, int64_t edge_visit_budget, char** out,
                size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (source == nullptr || target == nullptr) throw InvalidInput{"source and target spaces are required"};
        if (edge_visit_budget < 0) throw InvalidInput{"edge_visit_budget is negative"};
        const ma::Report report =
            ma::analyze_exact_compatibility(convert_space(*source), convert_space(*target), edge_visit_budget);
        ma::JsonWriter json;
        ma::write_report(json, report);
        return json.text();
    });
}

int ma_register_model(ma_str model, const ma_payload* payload, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const std::string name(view(model, "model argument"));
        std::optional<ma::Payload> converted;
        if (payload != nullptr) converted = convert(*payload);
        const ma::Morphometry morph = ma::analyze_or_refuse(name, converted ? &*converted : nullptr);
        // `morphometry.register`: the native space onto the canonical one.
        const ma::Report report = ma::analyze_exact_compatibility(
            ma::native_space(morph.native_axis_order.first, morph.native_axis_order.second), ma::canonical_space());
        ma::JsonWriter json;
        ma::write_report(json, report);
        return json.text();
    });
}

int ma_template_hierarchy(uint32_t builtin, const ma_template_node* nodes, size_t nodes_len, char** out,
                          size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const ma::HierarchySnapshot snapshot = snapshot_of(builtin, nodes, nodes_len);
        ma::JsonWriter json;
        ma::write_snapshot(json, snapshot);
        return json.text();
    });
}

int ma_measure_layers(uint32_t measure, ma_str model, const ma_payload* payload, uint32_t builtin,
                      const ma_template_node* nodes, size_t nodes_len, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const ma::HierarchySnapshot snapshot = snapshot_of(builtin, nodes, nodes_len);
        std::optional<ma::Morphometry> morph;
        if (flag(measure, "measure")) {
            const std::string name(view(model, "model argument"));
            std::optional<ma::Payload> converted;
            if (payload != nullptr) converted = convert(*payload);
            morph = ma::analyze_or_refuse(name, converted ? &*converted : nullptr);
        } else if (payload != nullptr || model.len != 0) {
            throw InvalidInput{"a model or payload given with measure = 0"};
        }
        return ma::layers_json(ma::measure_layers(morph ? &*morph : nullptr, snapshot));
    });
}

int ma_shape(ma_str model, const ma_payload* payload, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const std::string name(view(model, "model argument"));
        if (payload == nullptr) return ma::shape_json(name, nullptr);
        const ma::Payload converted = convert(*payload);
        return ma::shape_json(name, &converted);
    });
}

int ma_foundry(const ma_reading* reading, const ma_foundry_model* models, size_t models_len, int64_t context,
               ma_opt_str mem_fraction, ma_str source, ma_str as_of, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (reading == nullptr) throw InvalidInput{"a reading is required"};
        const ma::Reading converted = convert_reading(*reading);
        items(models, models_len, "models");
        std::vector<ma::FoundryModel> list;
        for (size_t i = 0; i < models_len; ++i) {
            const ma_foundry_model& in = models[i];
            ma::FoundryModel model;
            model.name = std::string(view(in.name, "a model name"));
            if (in.payload != nullptr) model.payload = convert(*in.payload);
            model.raised = optional_text(in.raised, "raised");
            if (model.raised && model.payload) throw InvalidInput{"a model with both a payload and a raised show"};
            model.requires_backends = texts(in.requires_backends, in.requires_backends_len, "requires_backends");
            list.push_back(std::move(model));
        }
        const auto env = optional_text(mem_fraction, "mem_fraction");
        return ma::foundry_json(converted, list, context, env ? &*env : nullptr,
                                std::string(view(source, "source")), std::string(view(as_of, "as_of")));
    });
}

int ma_describe(const ma_reading* reading, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (reading == nullptr) throw InvalidInput{"a reading is required"};
        return ma::describe_json(convert_reading(*reading));
    });
}

int ma_probe(ma_opt_str device_filter, ma_opt_str disk_root, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const auto filter = optional_text(device_filter, "device_filter");
        const auto root = optional_text(disk_root, "disk_root");
        return ma::probe_json(filter ? &*filter : nullptr, root ? &*root : nullptr);
    });
}

int ma_compare(ma_str left_model, const ma_payload* left_payload, ma_str right_model, const ma_payload* right_payload,
               char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const std::string left_name(view(left_model, "left model argument"));
        const std::string right_name(view(right_model, "right model argument"));
        std::optional<ma::Payload> left_converted;
        std::optional<ma::Payload> right_converted;
        if (left_payload != nullptr) left_converted = convert(*left_payload);
        if (right_payload != nullptr) right_converted = convert(*right_payload);
        // A D2 overflow is the operand's own refusal, so it compares as unmeasured.
        const ma::Morphometry left =
            ma::analyze_or_refuse(left_name, left_converted ? &*left_converted : nullptr);
        const ma::Morphometry right =
            ma::analyze_or_refuse(right_name, right_converted ? &*right_converted : nullptr);
        ma::JsonWriter json;
        ma::write_comparison(json, ma::compare(ma::record_of(left), ma::record_of(right)));
        return json.text();
    });
}

int ma_compare_records(const ma_morph_record* left, const ma_morph_record* right, char** out,
                       size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (left == nullptr) throw InvalidInput{"left must be a ModelMorphometry"};
        if (right == nullptr) throw InvalidInput{"right must be a ModelMorphometry"};
        const ma::Record left_record = convert_record(*left);
        const ma::Record right_record = convert_record(*right);
        ma::JsonWriter json;
        ma::write_comparison(json, ma::compare(left_record, right_record));
        return json.text();
    });
}

int ma_throughput_from_llamacpp(const ma_llamacpp* payload, ma_str model, int64_t context, char** out,
                                size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const std::string name(view(model, "model argument"));
        std::optional<ma::LlamacppPayload> converted;
        if (payload != nullptr) converted = convert_llamacpp(*payload);
        ma::Throughput reading;
        try {
            reading = ma::throughput_from_llamacpp(converted ? &*converted : nullptr, name, context);
        } catch (const ma::PythonError& error) {
            throw InvalidInput{error.what};
        }
        ma::JsonWriter json;
        ma::write_throughput(json, reading);
        return json.text();
    });
}

int ma_cost_compare(const ma_throughput* throughput, const ma_energy* energy, const ma_price* price,
                    const ma_volume* volume, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (throughput == nullptr || energy == nullptr || volume == nullptr) {
            throw InvalidInput{"a throughput, an energy declaration and a volume are required"};
        }
        std::optional<ma::Price> converted_price;
        if (price != nullptr) converted_price = convert_price(*price);
        const ma::Costs costs = ma::compare_costs(convert_throughput(*throughput), convert_energy(*energy),
                                                  converted_price ? &*converted_price : nullptr,
                                                  convert_volume(*volume));
        ma::JsonWriter json;
        ma::write_costs(json, costs);
        return json.text();
    });
}

int ma_energy_usd_for_seconds(const ma_energy* energy, double seconds, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (energy == nullptr) throw InvalidInput{"an energy declaration is required"};
        const ma::Energy converted = convert_energy(*energy);
        ma::JsonWriter json;
        json.begin_object();
        json.key("complete");
        json.boolean(converted.complete());
        json.key("usd");
        json.optional_py_float(converted.usd_for_seconds(seconds));
        json.end_object();
        return json.text();
    });
}

int ma_read_power(uint32_t outcome, double watts, char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        if (outcome > 3) throw InvalidInput{"outcome is not 0, 1, 2 or 3"};
        const auto [read, provenance] = ma::read_power(static_cast<int>(outcome), watts);
        ma::JsonWriter json;
        json.begin_object();
        json.key("watts");
        json.optional_py_float(read);
        json.key("provenance");
        json.string(provenance);
        json.end_object();
        return json.text();
    });
}

int ma_economics_constants(char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [] {
        ma::JsonWriter json;
        json.begin_object();
        json.key("structural_caveats");
        json.begin_array();
        for (const auto& caveat : ma::structural_caveats()) json.string(caveat);
        json.end_array();
        json.key("provenance");
        json.begin_array();
        for (const char* word : {ma::kObserved, ma::kDerived, ma::kDeclared, ma::kUnmeasured}) json.string(word);
        json.end_array();
        json.key("seconds_per_hour");
        json.integer(3600);
        json.key("watts_per_kw");
        json.integer(1000);
        json.key("million");
        json.integer(1000000);
        json.end_object();
        return json.text();
    });
}

int ma_cpu_package_energy(char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [] { return ma::cpu_package_energy_json(); });
}

int ma_quant_types(char** out, size_t* out_len) noexcept {
    return guarded(out, out_len, [] {
        size_t count = 0;
        const ma::QuantType* types = ma::quant_types(&count);
        ma::JsonWriter json;
        json.begin_array();
        for (size_t i = 0; i < count; ++i) {
            json.begin_object();
            json.key("id");
            json.integer(types[i].id);
            json.key("name");
            json.string(types[i].name);
            json.key("block_size");
            json.integer(types[i].block_size);
            json.key("type_size");
            json.integer(types[i].type_size);
            json.key("supported");
            json.boolean(types[i].supported);
            json.end_object();
        }
        json.end_array();
        return json.text();
    });
}

int ma_dequantize(uint32_t type, const uint8_t* data, size_t len, float* values, size_t values_len, char** out,
                  size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const ma::QuantType* info = ma::quant_type(type);
        if (info == nullptr) throw InvalidInput{"unknown GGML type " + std::to_string(type)};
        if (!info->supported) throw InvalidInput{"no dequantiser for " + std::string(info->name)};
        if (data == nullptr && len != 0) throw InvalidInput{"data is null with a nonzero length"};
        if (len % info->type_size != 0) {
            throw InvalidInput{"the data is not whole " + std::string(info->name) + " blocks"};
        }
        const size_t blocks = len / info->type_size;
        const size_t elements = blocks * info->block_size;
        if (values != nullptr) {
            if (values_len < elements) throw InvalidInput{"values holds fewer floats than the blocks make"};
            ma::dequantize(*info, data, blocks, values);
        } else if (values_len != 0) {
            throw InvalidInput{"values is null with a nonzero length"};
        }
        ma::JsonWriter json;
        json.begin_object();
        json.key("elements");
        json.integer(static_cast<int64_t>(elements));
        json.end_object();
        return json.text();
    });
}

int ma_weight_statistics(ma_str path, uint32_t threads, ma_progress_fn progress, void* context, char** out,
                         size_t* out_len) noexcept {
    return guarded(out, out_len, [&] {
        const std::string file(view(path, "path"));
        if (file.empty()) throw InvalidInput{"path is empty"};
        if (file.find('\0') != std::string::npos) throw InvalidInput{"path contains a NUL"};
        return ma::weight_statistics_json(file, threads, progress, context);
    });
}

void ma_free(char* p) noexcept { std::free(p); }

}  // extern "C"
