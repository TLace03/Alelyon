// Weight statistics (PR 6): a GGUF file's tensors read from disk, dequantised
// block by block (dequant.hpp) and accumulated per tensor and per canonical
// (block, module) cell, without materialising a tensor. See the crate README,
// PR 6, for every definition (the sums, the derived figures, the histogram's
// bin edges) and each refusal.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>

namespace ma {

inline constexpr int kHistogramBins = 32;

// A sum of doubles carried with the exact error of each addition (Ogita, Rump
// and Oishi's Sum2: TwoSum per term, the errors summed). `value()` is as
// accurate as summing in twice the working precision, then rounding.
struct Sum2 {
    double s = 0.0;
    double c = 0.0;
    void add(double x) {
        const double t = s + x;
        const double bp = t - s;
        c += (s - (t - bp)) + (x - bp);
        s = t;
    }
    void merge(const Sum2& other) {
        add(other.s);
        c += other.c;
    }
    double value() const { return s + c; }
};

// What is accumulated over one tensor's or one cell's values.
struct Moments {
    uint64_t count = 0;       // every element
    uint64_t non_finite = 0;  // NaN and +-inf: excluded from everything below
    uint64_t zeros = 0;       // exact zeros (either sign)
    float min = 0.0f;         // over finite values; meaningful when count > non_finite
    float max = 0.0f;
    bool has_finite = false;
    Sum2 sum;
    Sum2 sum_abs;
    Sum2 sum_sq;  // of x*x in double, which is exact for a float32 x
    std::array<uint64_t, kHistogramBins> histogram{};

    void accumulate(const float* values, size_t n);
    void merge(const Moments& other);
};

// Called on the calling thread only, with the bytes of tensor data read so far
// and the total to read; a nonzero return cancels the run.
using ProgressFn = int (*)(void* context, uint64_t done, uint64_t total);

// The whole result as JSON (README, PR 6). `threads` 0 is the default:
// hardware_concurrency() - 1, at least 1; any request is capped by that.
std::string weight_statistics_json(const std::string& path, uint32_t threads, ProgressFn progress,
                                   void* context);

}  // namespace ma
