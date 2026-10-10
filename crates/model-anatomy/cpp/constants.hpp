// The constants of the Python reference's `morphometry.py`, verbatim.
// `ma_constants` exports them and the goldens hold them to Python's.
#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace ma {

inline constexpr std::string_view kMorphometrySchema = "alelyon.lattice.model-morphometry/0.1";
inline constexpr std::string_view kCanonicalSpaceId = "alelyon.model.transformer.canonical";
inline constexpr std::string_view kCanonicalSpaceVersion = "0.1.0";

inline constexpr std::string_view kSourceTensorInventory = "TENSOR_INVENTORY";
inline constexpr std::string_view kSourceDeclaredArchitecture = "DECLARED_ARCHITECTURE";

inline constexpr std::string_view kRefusedNoMetadata = "NO_MODEL_METADATA";
inline constexpr std::string_view kRefusedNoBlocks = "BLOCK_COUNT_NOT_DECLARED";
inline constexpr std::string_view kRefusedMixtureOfExperts = "MIXTURE_OF_EXPERTS_WITHOUT_TENSOR_INVENTORY";
inline constexpr std::string_view kRefusedInsufficientArchitecture = "ARCHITECTURE_FIELDS_INCOMPLETE";
// Native only (deviation D2): an integer outside int64.
inline constexpr std::string_view kRefusedIntegerOutOfRange = "INTEGER_OUT_OF_RANGE";

inline constexpr int64_t kMaxTensors = 20000;
inline constexpr int64_t kMaxBlocks = 4096;

inline constexpr std::string_view kFamilyEmbedding = "embedding";
inline constexpr std::string_view kFamilyNorm = "normalisation";
inline constexpr std::string_view kFamilyAttention = "attention";
inline constexpr std::string_view kFamilyFeedForward = "feed-forward";
inline constexpr std::string_view kFamilyOutput = "output";

inline constexpr std::array<std::string_view, 5> kFamilies = {
    kFamilyEmbedding, kFamilyNorm, kFamilyAttention, kFamilyFeedForward, kFamilyOutput,
};

struct ModuleInfo {
    std::string_view id;
    std::string_view family;
    int64_t stage;
    std::string_view label;
};

// `MODULES`, in its insertion order.
inline constexpr std::array<ModuleInfo, 16> kModules = {{
    {"token_embd", kFamilyEmbedding, 0, "Token embedding"},
    {"attn_norm", kFamilyNorm, 0, "Attention norm"},
    {"attn_q_norm", kFamilyNorm, 1, "Query norm"},
    {"attn_k_norm", kFamilyNorm, 2, "Key norm"},
    {"ffn_norm", kFamilyNorm, 3, "Feed-forward norm"},
    {"output_norm", kFamilyNorm, 4, "Output norm"},
    {"attn_q", kFamilyAttention, 0, "Query projection"},
    {"attn_k", kFamilyAttention, 1, "Key projection"},
    {"attn_v", kFamilyAttention, 2, "Value projection"},
    {"attn_output", kFamilyAttention, 3, "Attention output"},
    {"ffn_gate", kFamilyFeedForward, 0, "Feed-forward gate"},
    {"ffn_up", kFamilyFeedForward, 1, "Feed-forward up"},
    {"ffn_down", kFamilyFeedForward, 2, "Feed-forward down"},
    {"ffn_expert", kFamilyFeedForward, 3, "Expert bank"},
    {"output", kFamilyOutput, 0, "Output projection"},
    {"other", kFamilyOutput, 1, "Unclassified"},
}};

// `_NAME_TO_MODULE`: the first fragment found in the lowercased name wins.
inline constexpr std::array<std::pair<std::string_view, std::string_view>, 19> kNameToModule = {{
    {"attn_q_norm", "attn_q_norm"},
    {"attn_k_norm", "attn_k_norm"},
    {"attn_norm", "attn_norm"},
    {"ffn_norm", "ffn_norm"},
    {"output_norm", "output_norm"},
    {"token_embd", "token_embd"},
    {"attn_output", "attn_output"},
    {"attn_out", "attn_output"},
    {"attn_q", "attn_q"},
    {"attn_k", "attn_k"},
    {"attn_v", "attn_v"},
    {"ffn_gate_exps", "ffn_expert"},
    {"ffn_up_exps", "ffn_expert"},
    {"ffn_down_exps", "ffn_expert"},
    {"ffn_gate_inp", "ffn_expert"},
    {"ffn_gate", "ffn_gate"},
    {"ffn_up", "ffn_up"},
    {"ffn_down", "ffn_down"},
    {"output", "output"},
}};

inline constexpr std::string_view kRoutedExpertSuffix = "_exps";

struct BitsPerWeight {
    std::string_view type;
    int64_t num;
    int64_t den;  // reduced, as Fraction holds it
};

// `NOMINAL_BITS_PER_WEIGHT`, in its insertion order.
inline constexpr std::array<BitsPerWeight, 29> kNominalBitsPerWeight = {{
    {"F64", 64, 1},     {"I64", 64, 1},      {"F32", 32, 1},     {"I32", 32, 1},
    {"F16", 16, 1},     {"BF16", 16, 1},     {"I16", 16, 1},     {"Q8_0", 17, 2},
    {"Q8_1", 9, 1},     {"Q8_K", 17, 2},     {"I8", 8, 1},       {"Q6_K", 105, 16},
    {"Q5_0", 11, 2},    {"Q5_1", 6, 1},      {"Q5_K", 11, 2},    {"Q4_0", 9, 2},
    {"Q4_1", 5, 1},     {"Q4_K", 9, 2},      {"IQ4_NL", 9, 2},   {"IQ4_XS", 17, 4},
    {"Q3_K", 55, 16},   {"IQ3_S", 55, 16},   {"IQ3_XXS", 49, 16}, {"Q2_K", 21, 8},
    {"IQ2_S", 5, 2},    {"IQ2_XS", 37, 16},  {"IQ2_XXS", 33, 16}, {"IQ1_M", 7, 4},
    {"IQ1_S", 25, 16},
}};

const ModuleInfo& module_info(std::string_view id);  // unknown -> "other"
const BitsPerWeight* nominal_bits(std::string_view type);
int64_t stage_count();
std::vector<std::string> module_ids();  // `MODULE_IDS`: sorted
size_t family_index(std::string_view family);

}  // namespace ma
