// Dequantisation, ported block by block from gguf-py's quants.py (the Python
// package `gguf` 0.19.0, part of llama.cpp: https://github.com/ggml-org/llama.cpp,
// MIT License, Copyright (c) 2023 Georgi Gerganov; the full notice is in
// gguf_quant_tables.hpp, which holds the tables). Each function computes what
// that file's `dequantize_blocks` computes, in float32, in its order of
// operations: numpy multiplies and adds element by element (no fused
// multiply-add), so `(d * q) - m` here is two roundings as there, and
// `d * (0.5f + s) * 0.25f` associates left to right as numpy's does. Integer
// steps (nibble and bit unpacking, int8 offsets) are exact in both. Nothing here
// depends on the host's float16 support: float16 is decoded from its bits.
#include "dequant.hpp"

#include <array>
#include <bit>
#include <cstring>

#include "gguf_quant_tables.hpp"

static_assert(std::endian::native == std::endian::little, "the GGUF readers assume a little-endian host");

namespace ma {

namespace {

namespace T = gguf_tables;

inline uint16_t rd16(const uint8_t* p) { return static_cast<uint16_t>(p[0] | (p[1] << 8)); }

inline uint32_t rd32(const uint8_t* p) {
    return static_cast<uint32_t>(p[0]) | (static_cast<uint32_t>(p[1]) << 8) | (static_cast<uint32_t>(p[2]) << 16) |
           (static_cast<uint32_t>(p[3]) << 24);
}

// numpy's half-to-float bit conversion (npy_halfbits_to_floatbits): exact,
// subnormals normalised, infinities and NaN payloads carried over unquieted.
uint32_t f16_bits_to_f32_bits(uint16_t h) {
    const uint32_t sign = static_cast<uint32_t>(h & 0x8000u) << 16;
    const uint32_t exponent = (h >> 10) & 0x1Fu;
    uint32_t mantissa = h & 0x3FFu;
    if (exponent == 0x1F) return sign | 0x7F800000u | (mantissa << 13);
    if (exponent == 0) {
        if (mantissa == 0) return sign;
        uint32_t e = 127 - 15 + 1;
        while ((mantissa & 0x400u) == 0) {
            mantissa <<= 1;
            --e;
        }
        return sign | (e << 23) | ((mantissa & 0x3FFu) << 13);
    }
    return sign | ((exponent + 112) << 23) | (mantissa << 13);
}

const std::array<float, 65536>& f16_table() {
    static const std::array<float, 65536> table = [] {
        std::array<float, 65536> out{};
        for (uint32_t h = 0; h < 65536; ++h) {
            out[h] = std::bit_cast<float>(f16_bits_to_f32_bits(static_cast<uint16_t>(h)));
        }
        return out;
    }();
    return table;
}

inline float f16(const uint8_t* p) { return f16_table()[rd16(p)]; }

inline float sign_of(uint8_t byte, int bit) { return ((byte >> bit) & 1) == 0 ? 1.0f : -1.0f; }

inline float fl(int value) { return static_cast<float>(value); }

void deq_f32(const uint8_t* b, float* y) { y[0] = std::bit_cast<float>(rd32(b)); }

void deq_f16(const uint8_t* b, float* y) { y[0] = f16(b); }

void deq_bf16(const uint8_t* b, float* y) { y[0] = std::bit_cast<float>(static_cast<uint32_t>(rd16(b)) << 16); }

void deq_q4_0(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    for (int j = 0; j < 16; ++j) {
        y[j] = d * fl((qs[j] & 0x0F) - 8);
        y[j + 16] = d * fl((qs[j] >> 4) - 8);
    }
}

void deq_q4_1(const uint8_t* b, float* y) {
    const float d = f16(b);
    const float m = f16(b + 2);
    const uint8_t* qs = b + 4;
    for (int j = 0; j < 16; ++j) {
        y[j] = d * fl(qs[j] & 0x0F) + m;
        y[j + 16] = d * fl(qs[j] >> 4) + m;
    }
}

void deq_q5_0(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint32_t qh = rd32(b + 2);
    const uint8_t* qs = b + 6;
    for (int j = 0; j < 32; ++j) {
        const int low = j < 16 ? (qs[j] & 0x0F) : (qs[j - 16] >> 4);
        const int high = static_cast<int>((qh >> j) & 1u);
        y[j] = d * fl((low | (high << 4)) - 16);
    }
}

void deq_q5_1(const uint8_t* b, float* y) {
    const float d = f16(b);
    const float m = f16(b + 2);
    const uint32_t qh = rd32(b + 4);
    const uint8_t* qs = b + 8;
    for (int j = 0; j < 32; ++j) {
        const int low = j < 16 ? (qs[j] & 0x0F) : (qs[j - 16] >> 4);
        const int high = static_cast<int>((qh >> j) & 1u);
        y[j] = d * fl(low | (high << 4)) + m;
    }
}

void deq_q8_0(const uint8_t* b, float* y) {
    const float d = f16(b);
    for (int j = 0; j < 32; ++j) y[j] = fl(static_cast<int8_t>(b[2 + j])) * d;
}

void deq_q2_k(const uint8_t* b, float* y) {
    const uint8_t* scales = b;
    const uint8_t* qs = b + 16;
    const float d = f16(b + 80);
    const float dmin = f16(b + 82);
    float dl[16];
    float ml[16];
    for (int i = 0; i < 16; ++i) {
        dl[i] = d * fl(scales[i] & 0x0F);
        ml[i] = dmin * fl(scales[i] >> 4);
    }
    for (int g = 0; g < 2; ++g) {
        for (int s = 0; s < 4; ++s) {
            for (int k = 0; k < 32; ++k) {
                const int e = g * 128 + s * 32 + k;
                const int q = (qs[g * 32 + k] >> (2 * s)) & 3;
                y[e] = dl[e / 16] * fl(q) - ml[e / 16];
            }
        }
    }
}

void deq_q3_k(const uint8_t* b, float* y) {
    const uint8_t* hmask = b;
    const uint8_t* qs = b + 32;
    const uint8_t* scales = b + 96;
    const float d = f16(b + 108);
    float dl[16];
    for (int i = 0; i < 16; ++i) {
        const int low = i < 8 ? (scales[i] & 0x0F) : ((scales[i - 8] >> 4) & 0x0F);
        const int high = (scales[8 + i % 4] >> (2 * (i / 4))) & 0x03;
        const int scale = static_cast<int8_t>(static_cast<uint8_t>(low | (high << 4))) - 32;
        dl[i] = d * fl(scale);
    }
    for (int e = 0; e < 256; ++e) {
        const int g = e / 128;
        const int s = (e % 128) / 32;
        const int k = e % 32;
        const int low = (qs[g * 32 + k] >> (2 * s)) & 3;
        const int high = ((hmask[k] >> (e / 32)) & 1) ^ 1;
        y[e] = dl[e / 16] * fl(low - (high << 2));
    }
}

// Q4_K.get_scale_min: six-bit scales and mins, unpacked from twelve bytes.
void scale_min_k4(const uint8_t* packed, int out_scale[8], int out_min[8]) {
    for (int j = 0; j < 4; ++j) {
        out_scale[j] = packed[j] & 0x3F;
        out_min[j] = packed[4 + j] & 0x3F;
        out_scale[4 + j] = (packed[8 + j] & 0x0F) | ((packed[j] >> 2) & 0x30);
        out_min[4 + j] = (packed[8 + j] >> 4) | ((packed[4 + j] >> 2) & 0x30);
    }
}

void deq_q4_k(const uint8_t* b, float* y) {
    const float d = f16(b);
    const float dmin = f16(b + 2);
    int sc[8];
    int mn[8];
    scale_min_k4(b + 4, sc, mn);
    const uint8_t* qs = b + 16;
    for (int j = 0; j < 8; ++j) {
        const float dd = d * fl(sc[j]);
        const float dm = dmin * fl(mn[j]);
        const int g = j / 2;
        const int shift = 4 * (j % 2);
        for (int k = 0; k < 32; ++k) y[j * 32 + k] = dd * fl((qs[g * 32 + k] >> shift) & 0x0F) - dm;
    }
}

void deq_q5_k(const uint8_t* b, float* y) {
    const float d = f16(b);
    const float dmin = f16(b + 2);
    int sc[8];
    int mn[8];
    scale_min_k4(b + 4, sc, mn);
    const uint8_t* qh = b + 16;
    const uint8_t* qs = b + 48;
    for (int j = 0; j < 8; ++j) {
        const float dd = d * fl(sc[j]);
        const float dm = dmin * fl(mn[j]);
        const int g = j / 2;
        const int shift = 4 * (j % 2);
        for (int k = 0; k < 32; ++k) {
            const int q = ((qs[g * 32 + k] >> shift) & 0x0F) | (((qh[k] >> j) & 1) << 4);
            y[j * 32 + k] = dd * fl(q) - dm;
        }
    }
}

void deq_q6_k(const uint8_t* b, float* y) {
    const uint8_t* ql = b;
    const uint8_t* qh = b + 128;
    const uint8_t* scales = b + 192;
    const float d = f16(b + 208);
    float ds[16];
    for (int i = 0; i < 16; ++i) ds[i] = d * fl(static_cast<int8_t>(scales[i]));
    for (int g = 0; g < 2; ++g) {
        for (int e = 0; e < 128; ++e) {
            const int low = (ql[g * 64 + e % 64] >> (4 * (e / 64))) & 0x0F;
            const int high = (qh[g * 32 + e % 32] >> (2 * (e / 32))) & 0x03;
            const int q = static_cast<int8_t>(static_cast<uint8_t>(low | (high << 4))) - 32;
            const int at = g * 128 + e;
            y[at] = ds[at / 16] * fl(q);
        }
    }
}

void deq_iq2_xxs(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    for (int ib = 0; ib < 8; ++ib) {
        const uint32_t w0 = rd32(qs + 8 * ib);
        const uint32_t w1 = rd32(qs + 8 * ib + 4);
        const float db = d * (0.5f + static_cast<float>(w1 >> 28)) * 0.25f;
        for (int l = 0; l < 4; ++l) {
            const int8_t* row = T::kGridIQ2XXS + 8 * ((w0 >> (8 * l)) & 0xFFu);
            const uint8_t signs = T::kSigns[(w1 >> (7 * l)) & 0x7Fu];
            for (int j = 0; j < 8; ++j) y[ib * 32 + l * 8 + j] = db * fl(row[j]) * sign_of(signs, j);
        }
    }
}

void iq2_scales(const uint8_t* scales, float d, float db[16]) {
    for (int i = 0; i < 8; ++i) {
        db[2 * i] = d * (0.5f + fl(scales[i] & 0x0F)) * 0.25f;
        db[2 * i + 1] = d * (0.5f + fl(scales[i] >> 4)) * 0.25f;
    }
}

void deq_iq2_xs(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    float db[16];
    iq2_scales(b + 66, d, db);
    for (int t = 0; t < 32; ++t) {
        const uint16_t q = rd16(qs + 2 * t);
        const int8_t* row = T::kGridIQ2XS + 8 * (q & 511);
        const uint8_t signs = T::kSigns[q >> 9];
        for (int j = 0; j < 8; ++j) y[t * 8 + j] = db[t / 2] * fl(row[j]) * sign_of(signs, j);
    }
}

void deq_iq2_s(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    const uint8_t* signs = b + 34;
    const uint8_t* qh = b + 66;
    float db[16];
    iq2_scales(b + 74, d, db);
    for (int t = 0; t < 32; ++t) {
        const int high = (qh[t / 4] >> (2 * (t % 4))) & 3;
        const int8_t* row = T::kGridIQ2S + 8 * (qs[t] | (high << 8));
        for (int j = 0; j < 8; ++j) y[t * 8 + j] = db[t / 2] * fl(row[j]) * sign_of(signs[t], j);
    }
}

void deq_iq3_xxs(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    const uint8_t* scales = b + 66;
    for (int ib = 0; ib < 8; ++ib) {
        const uint32_t w = rd32(scales + 4 * ib);
        const float db = d * (0.5f + static_cast<float>(w >> 28)) * 0.5f;
        for (int l = 0; l < 4; ++l) {
            const uint8_t signs = T::kSigns[(w >> (7 * l)) & 0x7Fu];
            for (int j = 0; j < 8; ++j) {
                const int8_t* row = T::kGridIQ3XXS + 4 * qs[ib * 8 + l * 2 + j / 4];
                y[ib * 32 + l * 8 + j] = db * fl(row[j % 4]) * sign_of(signs, j);
            }
        }
    }
}

void deq_iq3_s(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    const uint8_t* qh = b + 66;
    const uint8_t* signs = b + 74;
    const uint8_t* scales = b + 106;
    for (int ib = 0; ib < 8; ++ib) {
        const int scale = ib % 2 == 0 ? (scales[ib / 2] & 0x0F) : (scales[ib / 2] >> 4);
        const float db = d * fl(1 + 2 * scale);
        for (int l = 0; l < 4; ++l) {
            for (int j = 0; j < 8; ++j) {
                const int t = ib * 8 + l * 2 + j / 4;
                const int index = qs[t] | (((qh[t / 8] >> (t % 8)) & 1) << 8);
                const int8_t* row = T::kGridIQ3S + 4 * index;
                y[ib * 32 + l * 8 + j] = db * fl(row[j % 4]) * sign_of(signs[ib * 4 + l], j);
            }
        }
    }
}

void deq_iq1_s(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    const uint8_t* qh = b + 34;
    for (int ib = 0; ib < 8; ++ib) {
        const uint16_t h = rd16(qh + 2 * ib);
        const float dl = d * fl(2 * ((h >> 12) & 7) + 1);
        const float delta = (h & 0x8000u) == 0 ? 0.125f : -0.125f;
        for (int l = 0; l < 4; ++l) {
            const int8_t* row = T::kGridIQ1S + 8 * (qs[ib * 4 + l] | (((h >> (3 * l)) & 7) << 8));
            for (int j = 0; j < 8; ++j) y[ib * 32 + l * 8 + j] = dl * (fl(row[j]) + delta);
        }
    }
}

void deq_iq1_m(const uint8_t* b, float* y) {
    const uint8_t* qs = b;
    const uint8_t* qh = b + 32;
    const uint8_t* sc = b + 48;
    uint16_t s[4];
    for (int i = 0; i < 4; ++i) s[i] = rd16(sc + 2 * i);
    const uint16_t bits = static_cast<uint16_t>(((s[0] & 0xF000u) >> 12) | ((s[1] & 0xF000u) >> 8) |
                                                ((s[2] & 0xF000u) >> 4) | (s[3] & 0xF000u));
    const float d = f16_table()[bits];
    float dl[16];
    for (int i = 0; i < 16; ++i) dl[i] = d * fl(2 * ((s[i / 4] >> (3 * (i % 4))) & 7) + 1);
    for (int t = 0; t < 32; ++t) {
        const int high = (qh[t / 2] >> (4 * (t % 2))) & 0x0F;
        const int8_t* row = T::kGridIQ1S + 8 * (qs[t] | ((high & 7) << 8));
        const float delta = (high & 0x08) == 0 ? 0.125f : -0.125f;
        for (int j = 0; j < 8; ++j) y[t * 8 + j] = dl[t / 2] * (fl(row[j]) + delta);
    }
}

void deq_iq4_nl(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint8_t* qs = b + 2;
    for (int j = 0; j < 16; ++j) {
        y[j] = d * fl(T::kIq4nlValues[qs[j] & 0x0F]);
        y[j + 16] = d * fl(T::kIq4nlValues[qs[j] >> 4]);
    }
}

void deq_iq4_xs(const uint8_t* b, float* y) {
    const float d = f16(b);
    const uint16_t scales_h = rd16(b + 2);
    const uint8_t* scales_l = b + 4;
    const uint8_t* qs = b + 8;
    for (int ib = 0; ib < 8; ++ib) {
        const int low = ib % 2 == 0 ? (scales_l[ib / 2] & 0x0F) : (scales_l[ib / 2] >> 4);
        const int high = (scales_h >> (2 * ib)) & 3;
        const int scale = static_cast<int8_t>(static_cast<uint8_t>(low | (high << 4))) - 32;
        const float dl = d * fl(scale);
        for (int k = 0; k < 16; ++k) {
            y[ib * 32 + k] = dl * fl(T::kIq4nlValues[qs[ib * 16 + k] & 0x0F]);
            y[ib * 32 + 16 + k] = dl * fl(T::kIq4nlValues[qs[ib * 16 + k] >> 4]);
        }
    }
}

using BlockFn = void (*)(const uint8_t*, float*);

BlockFn block_fn(uint32_t id) {
    switch (id) {
        case 0: return deq_f32;
        case 1: return deq_f16;
        case 2: return deq_q4_0;
        case 3: return deq_q4_1;
        case 6: return deq_q5_0;
        case 7: return deq_q5_1;
        case 8: return deq_q8_0;
        case 10: return deq_q2_k;
        case 11: return deq_q3_k;
        case 12: return deq_q4_k;
        case 13: return deq_q5_k;
        case 14: return deq_q6_k;
        case 16: return deq_iq2_xxs;
        case 17: return deq_iq2_xs;
        case 18: return deq_iq3_xxs;
        case 19: return deq_iq1_s;
        case 20: return deq_iq4_nl;
        case 21: return deq_iq3_s;
        case 22: return deq_iq2_s;
        case 23: return deq_iq4_xs;
        case 29: return deq_iq1_m;
        case 30: return deq_bf16;
        default: return nullptr;
    }
}

constexpr size_t kTypeCount = sizeof(T::kGgmlTypes) / sizeof(T::kGgmlTypes[0]);

const std::array<QuantType, kTypeCount>& type_table() {
    static const std::array<QuantType, kTypeCount> table = [] {
        std::array<QuantType, kTypeCount> out{};
        for (size_t i = 0; i < kTypeCount; ++i) {
            const auto& in = T::kGgmlTypes[i];
            out[i] = QuantType{in.id, in.name, in.block_size, in.type_size, block_fn(in.id) != nullptr};
        }
        return out;
    }();
    return table;
}

}  // namespace

const QuantType* quant_type(uint32_t id) {
    for (const auto& type : type_table()) {
        if (type.id == id) return &type;
    }
    return nullptr;
}

const QuantType* quant_types(size_t* count) {
    *count = kTypeCount;
    return type_table().data();
}

float f16_to_f32(uint16_t bits) { return f16_table()[bits]; }

void dequantize(const QuantType& type, const uint8_t* src, size_t blocks, float* dst) {
    const BlockFn fn = block_fn(type.id);
    if (fn == nullptr) return;
    for (size_t i = 0; i < blocks; ++i) fn(src + i * type.type_size, dst + i * type.block_size);
}

}  // namespace ma
