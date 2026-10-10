#include "pyfmt.hpp"

#include <charconv>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <system_error>
#include <vector>

namespace ma {

namespace {

// CPython's `_PY_LONG_DEFAULT_MAX_STR_DIGITS`.
constexpr size_t kMaxIntDigits = 4300;

int bit_length(uint64_t v) {
    int n = 0;
    while (v != 0) {
        ++n;
        v >>= 1;
    }
    return n;
}

// The exact quotient a / b (b > 0) rounded once to a double, ties to even.
double unsigned_truediv(uint64_t a, uint64_t b) {
    if (a == 0) return 0.0;
    uint64_t q = a / b;
    uint64_t r = a % b;
    // Collect 54 significant bits (53 + one rounding bit) of the quotient as
    // m * 2^exp2, and whether anything nonzero lies below them.
    uint64_t m = 0;
    int exp2 = 0;
    bool sticky = false;
    if (q != 0 && bit_length(q) >= 54) {
        const int shift = bit_length(q) - 54;
        m = q >> shift;
        sticky = (shift > 0 && (q & ((uint64_t{1} << shift) - 1)) != 0) || r != 0;
        exp2 = shift;
    } else {
        m = q;
        // r < b <= 2^63, so r << 1 still fits 64 bits.
        while (bit_length(m) < 54) {
            r <<= 1;
            m <<= 1;
            if (r >= b) {
                r -= b;
                m |= 1;
            }
            --exp2;
        }
        sticky = r != 0;
    }
    const uint64_t round_bit = m & 1;
    m >>= 1;
    exp2 += 1;
    if (round_bit != 0 && (sticky || (m & 1) != 0)) m += 1;
    return std::ldexp(static_cast<double>(m), exp2);
}

uint64_t magnitude(int64_t v) {
    return v < 0 ? uint64_t{0} - static_cast<uint64_t>(v) : static_cast<uint64_t>(v);
}

}  // namespace

std::string py_repr(double x) {
    if (std::isnan(x)) return "nan";
    if (std::isinf(x)) return x > 0 ? "inf" : "-inf";
    if (x == 0.0) return std::signbit(x) ? "-0.0" : "0.0";
    char buffer[64];
    const auto result = std::to_chars(buffer, buffer + sizeof buffer, x, std::chars_format::scientific);
    std::string text(buffer, result.ptr);
    // Shortest round-trip form: [-]d[.ddd]e(+|-)XX
    std::string sign;
    size_t at = 0;
    if (text[0] == '-') {
        sign = "-";
        at = 1;
    }
    const size_t e = text.find('e');
    std::string digits;
    for (size_t i = at; i < e; ++i) {
        if (text[i] != '.') digits.push_back(text[i]);
    }
    const int exponent = std::atoi(text.c_str() + e + 1);
    const int decpt = exponent + 1;  // value = 0.DIGITS * 10^decpt
    const int ndigits = static_cast<int>(digits.size());
    std::string out = sign;
    if (decpt <= -4 || decpt > 16) {
        out += digits[0];
        if (ndigits > 1) {
            out += '.';
            out.append(digits, 1, std::string::npos);
        }
        out += 'e';
        int shown = decpt - 1;
        out += shown < 0 ? '-' : '+';
        if (shown < 0) shown = -shown;
        if (shown < 10) out += '0';
        out += std::to_string(shown);
        return out;
    }
    if (decpt <= 0) {
        out += "0.";
        out.append(static_cast<size_t>(-decpt), '0');
        out += digits;
    } else if (decpt < ndigits) {
        out.append(digits, 0, static_cast<size_t>(decpt));
        out += '.';
        out.append(digits, static_cast<size_t>(decpt), std::string::npos);
    } else {
        out += digits;
        out.append(static_cast<size_t>(decpt - ndigits), '0');
        out += ".0";
    }
    return out;
}

std::string py_fixed(double x, int precision) {
    if (precision < 0) precision = 0;
    if (std::isnan(x)) return "nan";
    if (std::isinf(x)) return x > 0 ? "inf" : "-inf";
    // The largest finite double has 309 integer digits.
    std::vector<char> buffer(static_cast<size_t>(precision) + 400);
    const auto result = std::to_chars(buffer.data(), buffer.data() + buffer.size(), x,
                                      std::chars_format::fixed, precision);
    if (result.ec != std::errc()) return std::string();
    return std::string(buffer.data(), result.ptr);
}

std::string py_fixed_grouped(double x, int precision) {
    const std::string fixed = py_fixed(x, precision);
    if (!std::isfinite(x)) return fixed;  // "nan", "inf", "-inf": no digits to group
    const size_t start = fixed[0] == '-' ? 1 : 0;
    size_t end = fixed.find('.', start);
    if (end == std::string::npos) end = fixed.size();
    std::string out(fixed, 0, start);
    const size_t digits = end - start;
    const size_t lead = digits % 3 == 0 ? 3 : digits % 3;
    out.append(fixed, start, lead);
    for (size_t i = start + lead; i < end; i += 3) {
        out += ',';
        out.append(fixed, i, 3);
    }
    out.append(fixed, end, std::string::npos);
    return out;
}

std::string py_general(double x) {
    if (std::isnan(x)) return "nan";  // Python drops a NaN's sign
    if (std::isinf(x)) return x > 0 ? "inf" : "-inf";
    // `%g` at precision 6, which `std::to_chars` general follows exactly:
    // exponent form below 1e-4 or from 1e6, trailing zeros removed, at least
    // two exponent digits; Python's `format(x, "g")` is the same rule.
    char buffer[64];
    const auto result = std::to_chars(buffer, buffer + sizeof(buffer), x, std::chars_format::general, 6);
    if (result.ec != std::errc()) return std::string();
    return std::string(buffer, result.ptr);
}

std::string py_grouped(int64_t n) {
    const std::string digits = std::to_string(magnitude(n));
    std::string out;
    if (n < 0) out += '-';
    const size_t lead = digits.size() % 3 == 0 ? 3 : digits.size() % 3;
    out.append(digits, 0, lead);
    for (size_t i = lead; i < digits.size(); i += 3) {
        out += ',';
        out.append(digits, i, 3);
    }
    return out;
}

double py_truediv(int64_t a, int64_t b) {
    const bool negative = (a < 0) != (b < 0);
    const double quotient = unsigned_truediv(magnitude(a), magnitude(b));
    return negative ? -quotient : quotient;  // Python gives 0 / -5 == -0.0 too
}

bool py_int_equals_float(int64_t i, double x) {
    if (!std::isfinite(x) || std::trunc(x) != x) return false;
    // [-2^63, 2^63) is exactly the doubles that convert to int64 without loss.
    if (x < -9223372036854775808.0 || x >= 9223372036854775808.0) return false;
    return static_cast<int64_t>(x) == i;
}

bool is_py_space(unsigned char c) {
    return c == ' ' || (c >= '\t' && c <= '\r') || (c >= 0x1c && c <= 0x1f);
}

std::string_view py_strip(std::string_view text) {
    size_t begin = 0;
    size_t end = text.size();
    while (begin < end && is_py_space(static_cast<unsigned char>(text[begin]))) ++begin;
    while (end > begin && is_py_space(static_cast<unsigned char>(text[end - 1]))) --end;
    return text.substr(begin, end - begin);
}

std::string ascii_lower(std::string_view text) {
    std::string out(text);
    for (char& c : out) {
        if (c >= 'A' && c <= 'Z') c = static_cast<char>(c - 'A' + 'a');
    }
    return out;
}

std::string ascii_upper(std::string_view text) {
    std::string out(text);
    for (char& c : out) {
        if (c >= 'a' && c <= 'z') c = static_cast<char>(c - 'a' + 'A');
    }
    return out;
}

IntText py_int_text(std::string_view text, int64_t& out) {
    bool negative = false;
    size_t at = 0;
    if (!text.empty() && (text[0] == '+' || text[0] == '-')) {
        negative = text[0] == '-';
        at = 1;
    }
    std::string digits;
    bool previous_digit = false;
    for (size_t i = at; i < text.size(); ++i) {
        const char c = text[i];
        if (c >= '0' && c <= '9') {
            digits.push_back(c);
            previous_digit = true;
        } else if (c == '_' && previous_digit && i + 1 < text.size() && text[i + 1] >= '0' &&
                   text[i + 1] <= '9') {
            previous_digit = false;
        } else {
            return IntText::NotAnInteger;
        }
    }
    if (digits.empty() || digits.size() > kMaxIntDigits) return IntText::NotAnInteger;
    // Accumulate the magnitude negatively so INT64_MIN is reachable.
    int64_t value = 0;
    for (const char c : digits) {
        const int d = c - '0';
        if (value < (INT64_MIN + d) / 10) return IntText::OutOfRange;
        value = value * 10 - d;
    }
    if (!negative) {
        if (value == INT64_MIN) return IntText::OutOfRange;
        value = -value;
    }
    out = value;
    return IntText::Ok;
}

bool py_int_less_float(int64_t i, double x) {
    if (std::isnan(x)) return false;
    if (x >= 9223372036854775808.0) return true;
    if (x < -9223372036854775808.0) return false;
    const double whole = std::floor(x);  // exact, and in [-2^63, 2^63)
    const auto floor_i = static_cast<int64_t>(whole);
    // i < x  <=>  i < x when x is whole, else i <= floor(x).
    return whole == x ? i < floor_i : i <= floor_i;
}

namespace {

bool is_digit(char c) { return c >= '0' && c <= '9'; }

bool equals_ignoring_case(std::string_view text, std::string_view lower) {
    if (text.size() != lower.size()) return false;
    for (size_t i = 0; i < text.size(); ++i) {
        char c = text[i];
        if (c >= 'A' && c <= 'Z') c = static_cast<char>(c - 'A' + 'a');
        if (c != lower[i]) return false;
    }
    return true;
}

}  // namespace

bool py_float_text(std::string_view text, double& out) {
    text = py_strip(text);
    // `_Py_string_to_number_with_underscores`: an underscore only between two digits.
    std::string plain;
    for (size_t i = 0; i < text.size(); ++i) {
        if (text[i] == '_') {
            if (i == 0 || i + 1 >= text.size() || !is_digit(text[i - 1]) || !is_digit(text[i + 1])) return false;
            continue;
        }
        plain.push_back(text[i]);
    }
    bool negative = false;
    std::string_view body(plain);
    if (!body.empty() && (body[0] == '+' || body[0] == '-')) {
        negative = body[0] == '-';
        body.remove_prefix(1);
    }
    if (equals_ignoring_case(body, "inf") || equals_ignoring_case(body, "infinity")) {
        out = negative ? -HUGE_VAL : HUGE_VAL;
        return true;
    }
    if (equals_ignoring_case(body, "nan")) {
        out = negative ? -std::nan("") : std::nan("");
        return true;
    }
    // digits [. digits] [(e|E) [sign] digits], at least one mantissa digit.
    size_t at = 0;
    size_t mantissa_digits = 0;
    while (at < body.size() && is_digit(body[at])) {
        ++at;
        ++mantissa_digits;
    }
    if (at < body.size() && body[at] == '.') {
        ++at;
        while (at < body.size() && is_digit(body[at])) {
            ++at;
            ++mantissa_digits;
        }
    }
    if (mantissa_digits == 0) return false;
    if (at < body.size() && (body[at] == 'e' || body[at] == 'E')) {
        ++at;
        if (at < body.size() && (body[at] == '+' || body[at] == '-')) ++at;
        size_t exponent_digits = 0;
        while (at < body.size() && is_digit(body[at])) {
            ++at;
            ++exponent_digits;
        }
        if (exponent_digits == 0) return false;
    }
    if (at != body.size()) return false;
    double value = 0.0;
    const auto [end, error] = std::from_chars(body.data(), body.data() + body.size(), value, std::chars_format::general);
    if (error == std::errc::result_out_of_range || end != body.data() + body.size()) {
        // Past the double range: strtod rounds to inf or to zero (or a subnormal), as CPython does.
        const std::string terminated(body);
        value = std::strtod(terminated.c_str(), nullptr);
    } else if (error != std::errc()) {
        return false;
    }
    out = negative ? -value : value;
    return true;
}

bool valid_utf8(std::string_view text) {
    const auto* s = reinterpret_cast<const unsigned char*>(text.data());
    const size_t n = text.size();
    size_t i = 0;
    while (i < n) {
        const unsigned char c = s[i];
        if (c < 0x80) {
            ++i;
            continue;
        }
        size_t extra = 0;
        uint32_t cp = 0;
        uint32_t minimum = 0;
        if ((c & 0xE0) == 0xC0) {
            extra = 1;
            cp = c & 0x1Fu;
            minimum = 0x80;
        } else if ((c & 0xF0) == 0xE0) {
            extra = 2;
            cp = c & 0x0Fu;
            minimum = 0x800;
        } else if ((c & 0xF8) == 0xF0) {
            extra = 3;
            cp = c & 0x07u;
            minimum = 0x10000;
        } else {
            return false;
        }
        if (extra >= n - i) return false;
        for (size_t k = 1; k <= extra; ++k) {
            if ((s[i + k] & 0xC0) != 0x80) return false;
            cp = (cp << 6) | (s[i + k] & 0x3Fu);
        }
        if (cp < minimum || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) return false;
        i += extra + 1;
    }
    return true;
}

}  // namespace ma
