// Checked 64-bit integer arithmetic (deviation D2).
//
// Python's integers do not overflow; these do not either: an operation whose
// exact result is outside int64 throws `Overflow`, which the ABI layer turns
// into the named refusal INTEGER_OUT_OF_RANGE. `Overflow` is internal: it is
// caught inside every entry point and never crosses the C ABI.
#pragma once

#include <cstdint>
#include <limits>
#include <string>
#include <utility>

namespace ma {

struct Overflow {
    std::string what;  // the quantity, in words, for the refusal's gap
};

inline constexpr int64_t kI64Max = std::numeric_limits<int64_t>::max();
inline constexpr int64_t kI64Min = std::numeric_limits<int64_t>::min();

[[noreturn]] inline void overflow(std::string what) { throw Overflow{std::move(what)}; }

inline int64_t add(int64_t a, int64_t b, const char* what) {
    if ((b > 0 && a > kI64Max - b) || (b < 0 && a < kI64Min - b)) overflow(what);
    return a + b;
}

inline int64_t sub(int64_t a, int64_t b, const char* what) {
    if ((b < 0 && a > kI64Max + b) || (b > 0 && a < kI64Min + b)) overflow(what);
    return a - b;
}

inline int64_t mul(int64_t a, int64_t b, const char* what) {
    if (a == 0 || b == 0) return 0;
    if (a > 0) {
        if (b > 0) {
            if (a > kI64Max / b) overflow(what);
        } else if (b < kI64Min / a) {
            overflow(what);
        }
    } else if (b > 0) {
        if (a < kI64Min / b) overflow(what);
    } else if (b < kI64Max / a) {
        overflow(what);
    }
    return a * b;
}

// Python's `a // b` (floor division); `b` must not be zero.
inline int64_t floordiv(int64_t a, int64_t b, const char* what) {
    if (a == kI64Min && b == -1) overflow(what);
    int64_t q = a / b;
    if ((a % b != 0) && ((a < 0) != (b < 0))) --q;
    return q;
}

}  // namespace ma
