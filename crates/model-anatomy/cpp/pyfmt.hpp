// Python's text and number rules, as far as the ported code reaches them.
//
// Unicode is handled as ASCII only (deviation D3): `str.strip()`, `str.lower()`,
// `str.upper()`, `int(str)` and the regex `\d` act on ASCII characters, and every
// other code point passes through unchanged.
#pragma once

#include <cstdint>
#include <string>
#include <string_view>

namespace ma {

// `repr(x)` for a float: the shortest text that reads back as `x`, in Python's
// layout (fixed between 1e-4 and 1e16, else `d.ddde+XX`).
std::string py_repr(double x);

// `format(x, f".{precision}f")`: correctly rounded, like Python's.
std::string py_fixed(double x, int precision);

// `format(n, ",")`.
std::string py_grouped(int64_t n);

// `format(x, f",.{precision}f")`: `py_fixed` with the integer digits grouped.
std::string py_fixed_grouped(double x, int precision);

// `format(x, "g")` for a float (precision 6, trailing zeros removed).
std::string py_general(double x);

// Python's `a / b` of two integers: the exact quotient rounded once to the
// nearest double (ties to even). `b` must not be zero.
double py_truediv(int64_t a, int64_t b);

// Python's `i == x` for an int and a float: exact, never through a rounding.
bool py_int_equals_float(int64_t i, double x);

// Python `str.isspace()` restricted to ASCII: " \t\n\v\f\r" and \x1c-\x1f.
bool is_py_space(unsigned char c);

// `str.strip()` (ASCII whitespace as `is_py_space`).
std::string_view py_strip(std::string_view text);

std::string ascii_lower(std::string_view text);
std::string ascii_upper(std::string_view text);

enum class IntText { Ok, NotAnInteger, OutOfRange };

// `int(text)` for text already stripped by `str.strip()`: an optional sign,
// ASCII digits with single underscores between them, at most 4,300 digits
// (CPython's limit; more raises ValueError). OutOfRange is a valid Python
// integer that int64 cannot hold (D2).
IntText py_int_text(std::string_view text, int64_t& out);

// Python's `i < x` for an int and a float: exact, never through a rounding
// (CPython compares an int with a float by value). NaN compares false.
bool py_int_less_float(int64_t i, double x);

// `float(text)`: CPython's float() of a str. ASCII whitespace around (D3), an
// optional sign, then "inf", "infinity" or "nan" (any case), or decimal digits
// with an optional fraction and exponent; single underscores only between two
// digits. False is Python's ValueError. Out-of-range text is +-inf or 0 as
// Python's is.
bool py_float_text(std::string_view text, double& out);

// Text that is well-formed UTF-8 (no overlongs, no surrogates, <= U+10FFFF).
bool valid_utf8(std::string_view text);

}  // namespace ma
