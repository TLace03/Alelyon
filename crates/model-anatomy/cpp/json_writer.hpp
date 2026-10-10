// A small JSON writer. Strings are written as given (UTF-8, validated at the
// ABI boundary) with `"`, `\` and the C0 controls escaped; floats are written
// as Python's `repr`, which is what `json.dumps` writes for a finite float.
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace ma {

class JsonWriter {
public:
    void begin_object();
    void end_object();
    void begin_array();
    void end_array();
    void key(std::string_view name);
    void string(std::string_view text);
    void integer(int64_t value);
    // A non-finite float is written as null: JSON has no spelling for it
    // (Python writes `Infinity`/`NaN`, which no JSON reader accepts).
    void number(double value);
    void boolean(bool value);
    void null();

    void optional_integer(const std::optional<int64_t>& value);
    void optional_number(const std::optional<double>& value);
    void optional_string(const std::optional<std::string>& value);
    // A float a Python result can hold non-finite (the economics, PR 5): a
    // finite value as a number, else Python's repr as text ("inf", "-inf",
    // "nan"), so nothing Python holds is written as an absent null.
    void py_float(double value);
    void optional_py_float(const std::optional<double>& value);

    const std::string& text() const { return out_; }

private:
    void separate();
    std::string out_;
    std::vector<bool> first_;  // per open container: nothing written yet
    bool after_key_ = false;
};

}  // namespace ma
