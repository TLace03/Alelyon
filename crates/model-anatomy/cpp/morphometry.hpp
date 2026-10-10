// A port of the Python reference's `morphometry.py`: `analyze` and its
// helpers, every derived property of `ModelMorphometry`, `Cell` and
// `BlockProfile`, and `voxel_field`. Integers are int64 with checked arithmetic
// (D2); text rules are ASCII-only (D3). See the crate README's DEVIATIONS.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

namespace ma {

// A `model_info` value as Python holds it.
struct Value {
    enum class Tag { None, Bool, Int, F64, Str } tag = Tag::None;
    bool boolean = false;
    double f64 = 0.0;
    std::string text;  // Int: canonical decimal; Str: UTF-8
};

// `model_info`: a Python dict built in file order (first place, last value).
class Info {
public:
    void set(std::string key, Value value);
    const std::pair<std::string, Value>* find(const std::string& key) const;
    const Value* get(const std::string& key) const {
        const auto* item = find(key);
        return item == nullptr ? nullptr : &item->second;
    }
    const std::vector<std::pair<std::string, Value>>& items() const { return items_; }

private:
    std::vector<std::pair<std::string, Value>> items_;
    std::unordered_map<std::string, size_t> index_;
};

struct TensorInput {
    std::string name;
    std::vector<uint64_t> dims;
    std::string type_name;
};

struct Payload {
    std::string model;
    std::string family;
    std::string quantization_level;
    std::string parent_model;
    Info info;
    bool has_tensors = false;
    std::vector<TensorInput> tensors;
};

struct Rational {
    int64_t num = 0;
    int64_t den = 1;
};

struct TensorRecord {
    std::string name;
    std::string module;
    std::optional<int64_t> block;
    std::vector<int64_t> shape;
    std::string element_type;
    int64_t parameters = 0;
};

struct Cell {
    std::optional<int64_t> block;
    std::string module;
    int64_t parameters = 0;
    int64_t tensors = 0;
    std::vector<std::string> element_types;
    std::optional<int64_t> nominal_bytes;
    int64_t routed_parameters = 0;
};

struct Morphometry {
    std::string model;
    std::string source;
    std::string architecture;
    std::string family;
    std::string quantization;
    std::optional<int64_t> declared_parameters;
    std::optional<int64_t> context_length;
    std::optional<int64_t> embedding_length;
    std::optional<int64_t> block_count;
    std::optional<int64_t> expert_count;
    std::optional<int64_t> expert_used_count;
    std::string routing_metadata_error;
    std::vector<TensorRecord> tensors;
    std::vector<Cell> cells;
    std::vector<std::string> gaps;
    std::string refusal;
    std::pair<std::string, std::string> native_axis_order{"block", "module"};
};

struct BlockProfile {
    int64_t block = 0;
    int64_t parameters = 0;
    std::optional<int64_t> nominal_bytes;
    int64_t modules = 0;
};

struct Derived {
    bool ok = false;
    int64_t counted_parameters = 0;
    std::optional<int64_t> nominal_bytes;
    std::optional<double> coverage;
    bool is_mixture_of_experts = false;
    std::optional<int64_t> routed_expert_parameters;
    std::optional<int64_t> always_active_parameters;
    std::optional<std::string> active_path_gap;
    std::optional<int64_t> active_parameters;
    std::optional<double> active_fraction;
    std::vector<BlockProfile> blocks;
    std::vector<std::pair<std::string, int64_t>> by_family;
    std::optional<double> dispersion_relative;
    std::vector<int64_t> dispersion_outliers;
};

struct Voxel {
    int64_t x = 0;
    int64_t y = 0;
    int64_t z = 0;
    std::string module;
    int64_t parameters = 0;
    double intensity = 0.0;
    std::optional<double> bits_per_weight;
    std::optional<int64_t> active_parameters;
    std::optional<double> active_intensity;
};

struct VoxelField {
    int64_t width = 0;
    int64_t height = 0;
    int64_t depth = 0;
    std::vector<Voxel> voxels;
    int64_t max_parameters = 0;
    bool has_stack_external = false;
    std::string source;
    std::optional<int64_t> max_active_parameters;
};

// Python's truth value of a `model_info` value (null: absent).
bool truthy(const Value* value);
// Python's `str(value)`, and an f-string's `{x}` of `int | None`.
std::string py_str(const Value& value);
std::string py_str(const std::optional<int64_t>& value);
// `_int(value)` (morphometry's and footprint's, which are the same rule): None
// for a bool, None, a non-integral float or text `int()` refuses. Throws
// `Overflow` (D2) for a valid integer outside int64; `key` names it.
std::optional<int64_t> to_int(const Value* value, const std::string& key);

// `analyze(payload, model=model)`; `payload` null is `analyze(None)`.
// Throws `Overflow` (D2); the caller turns it into `overflow_refusal`.
Morphometry analyze(const std::string& model, const Payload* payload);

// The name `analyze` would give the model (computed before any integer).
std::string resolved_name(const std::string& model, const Payload* payload);

// The D2 refusal: what `analyze` returns in place of a figure int64 cannot hold.
Morphometry overflow_refusal(const std::string& name, const std::string& what);

Derived derive(const Morphometry& morph);
VoxelField voxel_field(const Morphometry& morph, const Derived& derived);

// A tensor name's canonical cell, as `parse_tensor_inventory` assigns it:
// `module_of` and `block_of` of the name after `strip()` (D3). PR 6's weight
// statistics accumulate per cell by these.
std::string tensor_module(std::string_view name);
std::optional<int64_t> tensor_block(std::string_view name);

// Per-tensor and per-cell properties.
std::optional<Rational> tensor_bits_per_weight(const TensorRecord& tensor);
std::optional<int64_t> tensor_nominal_bytes(const TensorRecord& tensor);
std::optional<Rational> mean_bits_per_weight(std::optional<int64_t> nominal_bytes, int64_t parameters);
std::optional<int64_t> cell_active_parameters(const Cell& cell, std::optional<int64_t> used,
                                              std::optional<int64_t> total);

// The whole result as JSON (the shape tools/model_anatomy_goldens.py records).
std::string analysis_json(const std::string& model, const Payload* payload);

// `analyze`, with a D2 overflow turned into its refusal.
Morphometry analyze_or_refuse(const std::string& model, const Payload* payload);

class JsonWriter;
// `voxel_field`'s document, as `analysis_json` writes it.
void write_voxel_field_json(JsonWriter& json, const VoxelField& field);

}  // namespace ma
