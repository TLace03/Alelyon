// Weight statistics (PR 6). The GGUF header is parsed here (the crate had no
// C++ reader: the morphometry path takes a header lattice-core already read);
// tensor data is read by worker threads, each with its own read-only handle,
// in chunks of whole blocks, dequantised a sub-chunk at a time and accumulated.
// The file is opened for reading only and never written.
#include "weights.hpp"

#include <algorithm>
#include <atomic>
#include <bit>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <initializer_list>
#include <cstring>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string_view>
#include <thread>
#include <vector>

#include "dequant.hpp"
#include "json_writer.hpp"
#include "morphometry.hpp"
#include "pyfmt.hpp"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

namespace ma {

// ── accumulation ─────────────────────────────────────────────────────────────

void Moments::accumulate(const float* values, size_t n) {
    // Four interleaved partial sums per quantity shorten the dependency chain;
    // they are merged in a fixed order, so the result does not depend on how
    // the work was scheduled.
    Sum2 sums[4];
    Sum2 abs_sums[4];
    Sum2 squares[4];
    uint64_t non_finite_here = 0;
    uint64_t zeros_here = 0;
    float lo = std::numeric_limits<float>::infinity();
    float hi = -std::numeric_limits<float>::infinity();
    for (size_t i = 0; i < n; ++i) {
        const float x = values[i];
        const uint32_t bits = std::bit_cast<uint32_t>(x);
        const uint32_t exponent = (bits >> 23) & 0xFFu;
        if (exponent == 0xFFu) {
            ++non_finite_here;
            continue;
        }
        if (x < lo) lo = x;
        if (x > hi) hi = x;
        if ((bits & 0x7FFFFFFFu) == 0) {
            ++zeros_here;
            continue;
        }
        // |x| in [2^(k-25), 2^(k-24)) for bins 1..30; bin 0 below 2^-24
        // (subnormals included); bin 31 at and above 2^6.
        const int bin = exponent == 0 ? 0 : std::clamp(static_cast<int>(exponent) - 102, 0, kHistogramBins - 1);
        ++histogram[static_cast<size_t>(bin)];
        const double d = static_cast<double>(x);
        const size_t lane = i & 3u;
        sums[lane].add(d);
        abs_sums[lane].add(std::fabs(d));
        squares[lane].add(d * d);
    }
    count += n;
    non_finite += non_finite_here;
    zeros += zeros_here;
    if (lo <= hi) {
        if (!has_finite) {
            min = lo;
            max = hi;
            has_finite = true;
        } else {
            if (lo < min) min = lo;
            if (hi > max) max = hi;
        }
    }
    for (int lane = 0; lane < 4; ++lane) {
        sum.merge(sums[lane]);
        sum_abs.merge(abs_sums[lane]);
        sum_sq.merge(squares[lane]);
    }
}

void Moments::merge(const Moments& other) {
    count += other.count;
    non_finite += other.non_finite;
    zeros += other.zeros;
    if (other.has_finite) {
        if (!has_finite) {
            min = other.min;
            max = other.max;
            has_finite = true;
        } else {
            if (other.min < min) min = other.min;
            if (other.max > max) max = other.max;
        }
    }
    sum.merge(other.sum);
    sum_abs.merge(other.sum_abs);
    sum_sq.merge(other.sum_sq);
    for (int i = 0; i < kHistogramBins; ++i) histogram[static_cast<size_t>(i)] += other.histogram[static_cast<size_t>(i)];
}

namespace {

// ── a read-only file ─────────────────────────────────────────────────────────

class ReadOnlyFile {
public:
    ReadOnlyFile() = default;
    ReadOnlyFile(const ReadOnlyFile&) = delete;
    ReadOnlyFile& operator=(const ReadOnlyFile&) = delete;
    ~ReadOnlyFile() { close(); }

    // False with `error` set when the file cannot be opened for reading.
    bool open(const std::string& path, std::string& error) {
#ifdef _WIN32
        const int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path.data(), static_cast<int>(path.size()),
                                             nullptr, 0);
        if (size <= 0 && !path.empty()) {
            error = "the path is not convertible to UTF-16";
            return false;
        }
        std::wstring wide(static_cast<size_t>(size), L'\0');
        if (size > 0) {
            MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path.data(), static_cast<int>(path.size()), wide.data(),
                                size);
        }
        // Shared for reading, writing and deletion so no other process is blocked.
        handle_ = CreateFileW(wide.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                              nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
        if (handle_ == INVALID_HANDLE_VALUE) {
            error = "CreateFileW failed with Windows error " + std::to_string(GetLastError());
            return false;
        }
        LARGE_INTEGER length;
        if (!GetFileSizeEx(handle_, &length)) {
            error = "GetFileSizeEx failed with Windows error " + std::to_string(GetLastError());
            close();
            return false;
        }
        BY_HANDLE_FILE_INFORMATION info;
        if (GetFileInformationByHandle(handle_, &info) && (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
            error = "the path is a directory";
            close();
            return false;
        }
        size_ = static_cast<uint64_t>(length.QuadPart);
        return true;
#else
        fd_ = ::open(path.c_str(), O_RDONLY | O_CLOEXEC);
        if (fd_ < 0) {
            error = "open failed";
            return false;
        }
        struct stat st;
        if (::fstat(fd_, &st) != 0 || !S_ISREG(st.st_mode)) {
            error = "not a regular file";
            close();
            return false;
        }
        size_ = static_cast<uint64_t>(st.st_size);
        return true;
#endif
    }

    uint64_t size() const { return size_; }

    // Reads exactly `len` bytes at `offset`; false on an error or end of file.
    bool read_at(uint64_t offset, void* buffer, size_t len) {
        auto* out = static_cast<uint8_t*>(buffer);
        while (len > 0) {
#ifdef _WIN32
            const DWORD want = static_cast<DWORD>(std::min<size_t>(len, size_t{1} << 30));
            OVERLAPPED at{};
            at.Offset = static_cast<DWORD>(offset & 0xFFFFFFFFu);
            at.OffsetHigh = static_cast<DWORD>(offset >> 32);
            DWORD got = 0;
            if (!ReadFile(handle_, out, want, &got, &at) || got == 0) return false;
#else
            const ssize_t got = ::pread(fd_, out, len, static_cast<off_t>(offset));
            if (got <= 0) return false;
#endif
            out += got;
            offset += static_cast<uint64_t>(got);
            len -= static_cast<size_t>(got);
        }
        return true;
    }

private:
    void close() {
#ifdef _WIN32
        if (handle_ != INVALID_HANDLE_VALUE) CloseHandle(handle_);
        handle_ = INVALID_HANDLE_VALUE;
#else
        if (fd_ >= 0) ::close(fd_);
        fd_ = -1;
#endif
    }
#ifdef _WIN32
    HANDLE handle_ = INVALID_HANDLE_VALUE;
#else
    int fd_ = -1;
#endif
    uint64_t size_ = 0;
};

// ── the GGUF header ──────────────────────────────────────────────────────────

struct Refusal {
    std::string code;
    std::string reason;
};

struct HeaderRefusal {
    Refusal refusal;
};

struct TensorInfo {
    std::string name;
    std::vector<uint64_t> dims;
    uint32_t type_id = 0;
    uint64_t offset = 0;  // relative to the data section
};

struct Header {
    uint32_t version = 0;
    uint64_t alignment = 32;
    uint64_t data_offset = 0;
    std::vector<TensorInfo> tensors;
};

enum GgufValueType : uint32_t {
    kU8 = 0, kI8 = 1, kU16 = 2, kI16 = 3, kU32 = 4, kI32 = 5, kF32 = 6, kBool = 7,
    kString = 8, kArray = 9, kU64 = 10, kI64 = 11, kF64 = 12,
};

constexpr uint32_t kMaxDims = 4;     // GGML_MAX_DIMS
constexpr int kMaxArrayDepth = 8;    // nested arrays; no reader writes more than one level

// Sequential reads of the header through a buffer.
class HeaderReader {
public:
    explicit HeaderReader(ReadOnlyFile& file) : file_(file) {}

    uint64_t position() const { return pos_; }
    uint64_t remaining() const { return file_.size() - pos_; }

    void bytes(void* out, uint64_t len, const char* what) {
        if (len > remaining()) truncated(what);
        auto* dst = static_cast<uint8_t*>(out);
        while (len > 0) {
            if (pos_ < buffer_start_ || pos_ >= buffer_start_ + buffer_.size()) fill();
            const uint64_t at = pos_ - buffer_start_;
            const uint64_t take = std::min<uint64_t>(len, buffer_.size() - at);
            std::memcpy(dst, buffer_.data() + at, static_cast<size_t>(take));
            dst += take;
            pos_ += take;
            len -= take;
        }
    }

    void skip(uint64_t len, const char* what) {
        if (len > remaining()) truncated(what);
        pos_ += len;
    }

    uint32_t u32(const char* what) {
        uint8_t b[4];
        bytes(b, 4, what);
        return static_cast<uint32_t>(b[0]) | (static_cast<uint32_t>(b[1]) << 8) | (static_cast<uint32_t>(b[2]) << 16) |
               (static_cast<uint32_t>(b[3]) << 24);
    }

    uint64_t u64(const char* what) {
        const uint64_t low = u32(what);
        const uint64_t high = u32(what);
        return low | (high << 32);
    }

    std::string string(const char* what) {
        const uint64_t len = u64(what);
        if (len > remaining()) truncated(what);
        std::string out(static_cast<size_t>(len), '\0');
        bytes(out.data(), len, what);
        return out;
    }

    [[noreturn]] void truncated(const char* what) {
        throw HeaderRefusal{{"HEADER_TRUNCATED", std::string("the file ends inside the GGUF header (") + what + ")"}};
    }

private:
    void fill() {
        const uint64_t want = std::min<uint64_t>(kBuffer, file_.size() - pos_);
        buffer_.resize(static_cast<size_t>(want));
        if (!file_.read_at(pos_, buffer_.data(), buffer_.size())) {
            throw HeaderRefusal{{"READ_FAILED", "the GGUF header could not be read"}};
        }
        buffer_start_ = pos_;
    }

    static constexpr uint64_t kBuffer = uint64_t{1} << 20;
    ReadOnlyFile& file_;
    uint64_t pos_ = 0;
    uint64_t buffer_start_ = 0;
    std::vector<uint8_t> buffer_;
};

[[noreturn]] void malformed(const std::string& reason) { throw HeaderRefusal{{"MALFORMED_HEADER", reason}}; }

uint64_t fixed_size(uint32_t type) {
    switch (type) {
        case kU8: case kI8: case kBool: return 1;
        case kU16: case kI16: return 2;
        case kU32: case kI32: case kF32: return 4;
        case kU64: case kI64: case kF64: return 8;
        default: return 0;
    }
}

void skip_value(HeaderReader& in, uint32_t type, int depth) {
    if (const uint64_t size = fixed_size(type); size != 0) {
        in.skip(size, "a metadata value");
        return;
    }
    if (type == kString) {
        const uint64_t len = in.u64("a metadata string's length");
        in.skip(len, "a metadata string");
        return;
    }
    if (type != kArray) malformed("a metadata value has the unknown type " + std::to_string(type));
    if (depth >= kMaxArrayDepth) malformed("metadata arrays are nested too deeply");
    const uint32_t element = in.u32("a metadata array's element type");
    const uint64_t count = in.u64("a metadata array's length");
    if (const uint64_t size = fixed_size(element); size != 0) {
        if (count > in.remaining() / size) in.truncated("a metadata array");
        in.skip(count * size, "a metadata array");
        return;
    }
    if (element != kString && element != kArray) {
        malformed("a metadata array has the unknown element type " + std::to_string(element));
    }
    // Every string or array element takes at least eight bytes.
    if (count > in.remaining() / 8) in.truncated("a metadata array");
    for (uint64_t i = 0; i < count; ++i) skip_value(in, element, depth + 1);
}

Header parse_header(ReadOnlyFile& file) {
    HeaderReader in(file);
    Header header;
    if (file.size() < 4) throw HeaderRefusal{{"NOT_GGUF", "the file is shorter than the GGUF magic"}};
    char magic[4];
    in.bytes(magic, 4, "the magic");
    if (std::memcmp(magic, "GGUF", 4) != 0) throw HeaderRefusal{{"NOT_GGUF", "the file does not begin with GGUF"}};
    header.version = in.u32("the version");
    if (header.version != 2 && header.version != 3) {
        throw HeaderRefusal{{"UNSUPPORTED_VERSION", "GGUF version " + std::to_string(header.version) +
                                                         " (versions 2 and 3 are read)"}};
    }
    const uint64_t tensor_count = in.u64("the tensor count");
    const uint64_t kv_count = in.u64("the metadata count");
    // A metadata entry takes at least 12 bytes, a tensor entry at least 24.
    if (kv_count > in.remaining() / 12) in.truncated("the metadata");
    for (uint64_t i = 0; i < kv_count; ++i) {
        const std::string key = in.string("a metadata key");
        const uint32_t type = in.u32("a metadata value's type");
        if (key == "general.alignment") {
            if (type != kU32) malformed("general.alignment is not a uint32");
            const uint32_t alignment = in.u32("general.alignment");
            if (alignment == 0 || (alignment & (alignment - 1)) != 0) {
                malformed("general.alignment " + std::to_string(alignment) + " is not a power of two");
            }
            header.alignment = alignment;
        } else {
            skip_value(in, type, 0);
        }
    }
    if (tensor_count > in.remaining() / 24) in.truncated("the tensor table");
    header.tensors.reserve(static_cast<size_t>(tensor_count));
    for (uint64_t i = 0; i < tensor_count; ++i) {
        TensorInfo info;
        info.name = in.string("a tensor name");
        if (!valid_utf8(info.name)) malformed("tensor " + std::to_string(i) + "'s name is not UTF-8");
        const uint32_t n_dims = in.u32("a tensor's dimension count");
        if (n_dims > kMaxDims) malformed("tensor " + info.name + " has " + std::to_string(n_dims) + " dimensions");
        for (uint32_t d = 0; d < n_dims; ++d) info.dims.push_back(in.u64("a tensor dimension"));
        info.type_id = in.u32("a tensor type");
        info.offset = in.u64("a tensor offset");
        header.tensors.push_back(std::move(info));
    }
    const uint64_t end = in.position();
    const uint64_t pad = (header.alignment - end % header.alignment) % header.alignment;
    header.data_offset = end + pad;
    return header;
}

// ── the plan and the workers ─────────────────────────────────────────────────

struct Outcome {
    // A tensor read in full has `ok`; otherwise `refusal` says why not.
    bool ok = false;
    std::optional<Refusal> refusal;
    std::string type_name;
    uint64_t elements = 0;
    uint64_t absolute = 0;
    uint64_t bytes = 0;
    const QuantType* type = nullptr;
    Moments moments;
};

struct Shared {
    std::atomic<size_t> next{0};
    std::atomic<uint64_t> done{0};
    std::atomic<bool> cancel{false};
    std::mutex mutex;
    std::condition_variable wake;
    size_t running = 0;
    std::string failure;  // an exception a worker could not contain
};

constexpr uint64_t kReadBytes = uint64_t{4} << 20;  // raw bytes per read, whole blocks
constexpr uint64_t kFloats = uint64_t{1} << 18;     // floats per dequantised sub-chunk

void process(ReadOnlyFile& file, Outcome& outcome, Shared& shared, std::vector<uint8_t>& raw,
             std::vector<float>& values) {
    const QuantType& type = *outcome.type;
    const uint64_t blocks = outcome.bytes / type.type_size;
    const uint64_t blocks_per_read = std::max<uint64_t>(1, kReadBytes / type.type_size);
    const uint64_t blocks_per_step = std::max<uint64_t>(1, kFloats / type.block_size);
    raw.resize(static_cast<size_t>(std::min(blocks, blocks_per_read) * type.type_size));
    values.resize(static_cast<size_t>(std::min(blocks_per_step, blocks_per_read) * type.block_size));
    uint64_t at = 0;
    while (at < blocks) {
        if (shared.cancel.load(std::memory_order_relaxed)) return;
        const uint64_t take = std::min(blocks_per_read, blocks - at);
        const uint64_t bytes = take * type.type_size;
        if (!file.read_at(outcome.absolute + at * type.type_size, raw.data(), static_cast<size_t>(bytes))) {
            outcome.refusal = Refusal{"READ_FAILED", "the tensor's data could not be read in full"};
            return;
        }
        for (uint64_t step = 0; step < take; step += blocks_per_step) {
            const uint64_t n = std::min(blocks_per_step, take - step);
            dequantize(type, raw.data() + step * type.type_size, static_cast<size_t>(n), values.data());
            outcome.moments.accumulate(values.data(), static_cast<size_t>(n * type.block_size));
        }
        at += take;
        shared.done.fetch_add(bytes, std::memory_order_relaxed);
    }
    outcome.ok = true;
}

void worker(const std::string& path, std::vector<Outcome>& outcomes, const std::vector<size_t>& order,
            Shared& shared) {
    try {
        ReadOnlyFile file;
        std::string error;
        const bool opened = file.open(path, error);
        std::vector<uint8_t> raw;
        std::vector<float> values;
        while (!shared.cancel.load(std::memory_order_relaxed)) {
            const size_t slot = shared.next.fetch_add(1);
            if (slot >= order.size()) break;
            Outcome& outcome = outcomes[order[slot]];
            if (!opened) {
                outcome.refusal = Refusal{"READ_FAILED", "a reader could not open the file: " + error};
                continue;
            }
            process(file, outcome, shared, raw, values);
        }
    } catch (const std::bad_alloc&) {
        std::lock_guard<std::mutex> lock(shared.mutex);
        if (shared.failure.empty()) shared.failure = "out of memory";
        shared.cancel = true;
    } catch (const std::exception& error) {
        std::lock_guard<std::mutex> lock(shared.mutex);
        if (shared.failure.empty()) shared.failure = error.what();
        shared.cancel = true;
    }
    std::lock_guard<std::mutex> lock(shared.mutex);
    --shared.running;
    shared.wake.notify_all();
}

// ── the document ─────────────────────────────────────────────────────────────

void write_moments(JsonWriter& json, const Moments& m) {
    json.begin_object();
    json.key("count");
    json.integer(static_cast<int64_t>(m.count));
    json.key("non_finite");
    json.integer(static_cast<int64_t>(m.non_finite));
    json.key("zeros");
    json.integer(static_cast<int64_t>(m.zeros));
    json.key("min");
    if (m.has_finite) {
        json.number(static_cast<double>(m.min));
    } else {
        json.null();
    }
    json.key("max");
    if (m.has_finite) {
        json.number(static_cast<double>(m.max));
    } else {
        json.null();
    }
    const double total = m.sum.value();
    const double total_abs = m.sum_abs.value();
    const double total_sq = m.sum_sq.value();
    json.key("sum");
    json.number(total);
    json.key("sum_abs");
    json.number(total_abs);
    json.key("sum_sq");
    json.number(total_sq);
    const uint64_t n = m.count - m.non_finite;
    if (n > 0) {
        const double count = static_cast<double>(n);
        const double mean = total / count;
        json.key("mean");
        json.number(mean);
        json.key("std");
        json.number(std::sqrt(std::max(total_sq / count - mean * mean, 0.0)));
        json.key("rms");
        json.number(std::sqrt(total_sq / count));
        json.key("mean_abs");
        json.number(total_abs / count);
        json.key("l2");
        json.number(std::sqrt(total_sq));
    } else {
        for (const char* name : {"mean", "std", "rms", "mean_abs", "l2"}) {
            json.key(name);
            json.null();
        }
    }
    json.key("histogram");
    json.begin_array();
    for (const uint64_t bin : m.histogram) json.integer(static_cast<int64_t>(bin));
    json.end_array();
    json.end_object();
}

void write_cell(JsonWriter& json, const std::optional<int64_t>& block, const std::string& module) {
    json.key("block");
    json.optional_integer(block);
    json.key("module");
    json.string(module);
}

std::string refusal_document(const std::string& path, const Refusal& refusal, uint64_t file_bytes) {
    JsonWriter json;
    json.begin_object();
    json.key("path");
    json.string(path);
    json.key("refusal");
    json.begin_object();
    json.key("code");
    json.string(refusal.code);
    json.key("reason");
    json.string(refusal.reason);
    json.end_object();
    json.key("file_bytes");
    json.integer(static_cast<int64_t>(file_bytes));
    for (const char* name : {"tensors", "cells", "refused"}) {
        json.key(name);
        json.begin_array();
        json.end_array();
    }
    json.key("complete");
    json.boolean(false);
    json.end_object();
    return json.text();
}

uint32_t thread_count(uint32_t requested, size_t jobs) {
    const unsigned hardware = std::thread::hardware_concurrency();
    const uint32_t ceiling = hardware > 1 ? static_cast<uint32_t>(hardware - 1) : 1u;
    uint32_t count = requested == 0 ? ceiling : std::min(requested, ceiling);
    if (jobs < count) count = static_cast<uint32_t>(std::max<size_t>(jobs, 1));
    return count;
}

bool checked_mul(uint64_t a, uint64_t b, uint64_t& out) {
    if (a != 0 && b > std::numeric_limits<uint64_t>::max() / a) return false;
    out = a * b;
    return true;
}

}  // namespace

std::string weight_statistics_json(const std::string& path, uint32_t threads, ProgressFn progress, void* context) {
    ReadOnlyFile file;
    std::string error;
    if (!file.open(path, error)) return refusal_document(path, Refusal{"OPEN_FAILED", error}, 0);
    Header header;
    try {
        header = parse_header(file);
    } catch (const HeaderRefusal& refusal) {
        return refusal_document(path, refusal.refusal, file.size());
    }

    // The plan: each tensor's type, size and byte range, or its refusal.
    std::vector<Outcome> outcomes(header.tensors.size());
    std::vector<size_t> order;
    uint64_t total = 0;
    for (size_t i = 0; i < header.tensors.size(); ++i) {
        const TensorInfo& info = header.tensors[i];
        Outcome& outcome = outcomes[i];
        const QuantType* type = quant_type(info.type_id);
        outcome.type_name = type != nullptr ? std::string(type->name) : "type " + std::to_string(info.type_id);
        uint64_t elements = 1;
        bool shaped = true;
        for (const uint64_t dim : info.dims) shaped = shaped && checked_mul(elements, dim, elements);
        outcome.elements = elements;
        if (type == nullptr || !type->supported) {
            outcome.refusal = Refusal{"UNSUPPORTED_TYPE", "no dequantiser for " + outcome.type_name};
            continue;
        }
        outcome.type = type;
        const uint64_t ne0 = info.dims.empty() ? 1 : info.dims[0];
        uint64_t bytes = 0;
        if (!shaped || ne0 % type->block_size != 0 ||
            !checked_mul(elements / type->block_size, type->type_size, bytes)) {
            outcome.refusal = Refusal{"BAD_SHAPE", "the shape is not whole " + outcome.type_name + " blocks per row"};
            continue;
        }
        outcome.bytes = bytes;
        outcome.absolute = header.data_offset + info.offset;
        if (outcome.absolute < header.data_offset || outcome.absolute > file.size() ||
            bytes > file.size() - outcome.absolute) {
            outcome.refusal = Refusal{"TRUNCATED", "the file ends before the tensor's data does"};
            continue;
        }
        order.push_back(i);
        total += bytes;
    }
    // Largest first, so one big tensor does not start last; ties by file order.
    std::stable_sort(order.begin(), order.end(),
                     [&](size_t a, size_t b) { return outcomes[a].bytes > outcomes[b].bytes; });

    Shared shared;
    const uint32_t count = thread_count(threads, order.size());
    shared.running = count;
    std::vector<std::thread> pool;
    pool.reserve(count);
    if (progress != nullptr && progress(context, 0, total) != 0) shared.cancel = true;
    for (uint32_t t = 0; t < count; ++t) {
        try {
            pool.emplace_back(worker, std::cref(path), std::ref(outcomes), std::cref(order), std::ref(shared));
        } catch (...) {
            std::lock_guard<std::mutex> lock(shared.mutex);
            shared.running -= count - t;
            shared.cancel = true;
            if (shared.failure.empty()) shared.failure = "a worker thread could not be started";
            break;
        }
    }
    {
        std::unique_lock<std::mutex> lock(shared.mutex);
        while (shared.running > 0) {
            shared.wake.wait_for(lock, std::chrono::milliseconds(100));
            if (progress != nullptr && shared.running > 0) {
                lock.unlock();
                if (progress(context, shared.done.load(), total) != 0) shared.cancel = true;
                lock.lock();
            }
        }
    }
    for (auto& thread : pool) thread.join();
    if (!shared.failure.empty()) throw std::runtime_error(shared.failure);
    if (shared.cancel) {
        return refusal_document(path, Refusal{"CANCELLED", "the caller cancelled the run"}, file.size());
    }
    if (progress != nullptr) progress(context, shared.done.load(), total);

    // Cells, combined in file order (deterministic whatever the scheduling).
    struct CellKey {
        std::optional<int64_t> block;
        std::string module;
    };
    struct CellAccum {
        CellKey key;
        uint64_t tensors = 0;
        Moments moments;
    };
    std::vector<CellAccum> cells;
    JsonWriter json;
    json.begin_object();
    json.key("path");
    json.string(path);
    json.key("refusal");
    json.null();
    json.key("gguf_version");
    json.integer(header.version);
    json.key("alignment");
    json.integer(static_cast<int64_t>(header.alignment));
    json.key("data_offset");
    json.integer(static_cast<int64_t>(header.data_offset));
    json.key("file_bytes");
    json.integer(static_cast<int64_t>(file.size()));
    json.key("tensor_count");
    json.integer(static_cast<int64_t>(header.tensors.size()));
    json.key("threads");
    json.integer(count);
    uint64_t read_total = 0;
    json.key("tensors");
    json.begin_array();
    for (size_t i = 0; i < header.tensors.size(); ++i) {
        const Outcome& outcome = outcomes[i];
        if (!outcome.ok) continue;
        const TensorInfo& info = header.tensors[i];
        const std::string module = tensor_module(info.name);
        const std::optional<int64_t> block = tensor_block(info.name);
        read_total += outcome.bytes;
        json.begin_object();
        json.key("name");
        json.string(info.name);
        json.key("type");
        json.string(outcome.type_name);
        json.key("type_id");
        json.integer(info.type_id);
        json.key("dims");
        json.begin_array();
        for (const uint64_t dim : info.dims) json.integer(static_cast<int64_t>(dim));
        json.end_array();
        json.key("offset");
        json.integer(static_cast<int64_t>(outcome.absolute));
        json.key("bytes");
        json.integer(static_cast<int64_t>(outcome.bytes));
        write_cell(json, block, module);
        json.key("stats");
        write_moments(json, outcome.moments);
        json.end_object();
        auto found = std::find_if(cells.begin(), cells.end(), [&](const CellAccum& cell) {
            return cell.key.block == block && cell.key.module == module;
        });
        if (found == cells.end()) {
            cells.push_back(CellAccum{CellKey{block, module}, 0, Moments{}});
            found = cells.end() - 1;
        }
        found->tensors += 1;
        found->moments.merge(outcome.moments);
    }
    json.end_array();
    std::stable_sort(cells.begin(), cells.end(), [](const CellAccum& a, const CellAccum& b) {
        if (a.key.block.has_value() != b.key.block.has_value()) return !a.key.block.has_value();
        if (a.key.block != b.key.block) return *a.key.block < *b.key.block;
        return a.key.module < b.key.module;
    });
    json.key("cells");
    json.begin_array();
    for (const CellAccum& cell : cells) {
        json.begin_object();
        write_cell(json, cell.key.block, cell.key.module);
        json.key("tensors");
        json.integer(static_cast<int64_t>(cell.tensors));
        json.key("stats");
        write_moments(json, cell.moments);
        json.end_object();
    }
    json.end_array();
    json.key("refused");
    json.begin_array();
    size_t refused = 0;
    for (size_t i = 0; i < header.tensors.size(); ++i) {
        const Outcome& outcome = outcomes[i];
        if (outcome.ok) continue;
        ++refused;
        const TensorInfo& info = header.tensors[i];
        json.begin_object();
        json.key("name");
        json.string(info.name);
        json.key("type");
        json.string(outcome.type_name);
        json.key("type_id");
        json.integer(info.type_id);
        write_cell(json, tensor_block(info.name), tensor_module(info.name));
        json.key("code");
        json.string(outcome.refusal ? outcome.refusal->code : "READ_FAILED");
        json.key("reason");
        json.string(outcome.refusal ? outcome.refusal->reason : "the tensor was not read");
        json.end_object();
    }
    json.end_array();
    json.key("bytes_total");
    json.integer(static_cast<int64_t>(read_total));
    json.key("complete");
    json.boolean(refused == 0);
    json.end_object();
    return json.text();
}

}  // namespace ma
