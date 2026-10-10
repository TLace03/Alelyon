#include "json_writer.hpp"

#include <cmath>

#include "pyfmt.hpp"

namespace ma {

void JsonWriter::separate() {
    if (after_key_) {
        after_key_ = false;
        return;
    }
    if (!first_.empty()) {
        if (!first_.back()) out_ += ',';
        first_.back() = false;
    }
}

void JsonWriter::begin_object() {
    separate();
    out_ += '{';
    first_.push_back(true);
}

void JsonWriter::end_object() {
    out_ += '}';
    first_.pop_back();
}

void JsonWriter::begin_array() {
    separate();
    out_ += '[';
    first_.push_back(true);
}

void JsonWriter::end_array() {
    out_ += ']';
    first_.pop_back();
}

void JsonWriter::key(std::string_view name) {
    string(name);
    out_ += ':';
    after_key_ = true;
}

void JsonWriter::string(std::string_view text) {
    static const char* const kHex = "0123456789abcdef";
    separate();
    out_ += '"';
    for (const char c : text) {
        const auto u = static_cast<unsigned char>(c);
        if (c == '"') {
            out_ += "\\\"";
        } else if (c == '\\') {
            out_ += "\\\\";
        } else if (u < 0x20) {
            out_ += "\\u00";
            out_ += kHex[u >> 4];
            out_ += kHex[u & 15];
        } else {
            out_ += c;
        }
    }
    out_ += '"';
}

void JsonWriter::integer(int64_t value) {
    separate();
    out_ += std::to_string(value);
}

void JsonWriter::number(double value) {
    if (!std::isfinite(value)) {
        null();
        return;
    }
    separate();
    out_ += py_repr(value);
}

void JsonWriter::boolean(bool value) {
    separate();
    out_ += value ? "true" : "false";
}

void JsonWriter::null() {
    separate();
    out_ += "null";
}

void JsonWriter::optional_integer(const std::optional<int64_t>& value) {
    if (value) {
        integer(*value);
    } else {
        null();
    }
}

void JsonWriter::optional_number(const std::optional<double>& value) {
    if (value) {
        number(*value);
    } else {
        null();
    }
}

void JsonWriter::py_float(double value) {
    if (std::isfinite(value)) {
        number(value);
    } else {
        string(py_repr(value));
    }
}

void JsonWriter::optional_py_float(const std::optional<double>& value) {
    if (value) {
        py_float(*value);
    } else {
        null();
    }
}

void JsonWriter::optional_string(const std::optional<std::string>& value) {
    if (value) {
        string(*value);
    } else {
        null();
    }
}

}  // namespace ma
