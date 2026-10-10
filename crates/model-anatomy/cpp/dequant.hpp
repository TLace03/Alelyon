// Dequantisation of GGUF tensor blocks to float32 (PR 6), bit for bit as the
// Python package `gguf` 0.19.0 (gguf-py, part of llama.cpp, MIT License)
// computes it in `gguf.quants.dequantize`. The block layouts are ported from
// gguf-py's quants.py; the tables are in the generated `gguf_quant_tables.hpp`
// (which carries the MIT notice). See the crate README, PR 6.
#pragma once

#include <cstddef>
#include <cstdint>
#include <string_view>

namespace ma {

// One GGML tensor type, as gguf-py's GGML_QUANT_SIZES holds it.
struct QuantType {
    uint32_t id = 0;
    std::string_view name;
    uint32_t block_size = 0;  // elements per block
    uint32_t type_size = 0;   // bytes per block
    bool supported = false;   // dequantised here, bit-exact to gguf-py
};

// The type with this id, or nullptr for an id GGML's table does not have.
const QuantType* quant_type(uint32_t id);

// Every type of GGML's table, in id order.
const QuantType* quant_types(size_t* count);

// float16 bits to float32, exactly (NaN payloads kept, as numpy converts them).
float f16_to_f32(uint16_t bits);

// Dequantises `blocks` whole blocks of the supported type `type` from `src`
// (blocks * type_size bytes) into `dst` (blocks * block_size floats).
void dequantize(const QuantType& type, const uint8_t* src, size_t blocks, float* dst);

}  // namespace ma
