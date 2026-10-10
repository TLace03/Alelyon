// The activation probe: one forward pass of a prompt through a GGUF model on the pinned llama.cpp build, recording
// statistics of the activations the graph computes on the way (each layer's output, its attention and feed-forward
// results, the norms) and, for a mixture-of-experts model, which experts each layer's router chose for each token.
//
// It runs as its own process (CENTCOM starts it, as it starts llama-server), so a fault in the GPU driver or in
// llama.cpp ends this process, never the window. llama.dll is loaded at run time from the folder given, and only
// when its SHA-256 and ggml-base.dll's are the pinned build's (the layouts in llama_abi.h were taken from that build's
// commit and are right only for it). Nothing is written but the JSON on stdout; the model file is only read.
//
// Statistics are accumulated in double precision with compensated sums, over every element of a tensor each time
// the graph computes it (once per micro-batch), and say how many elements they cover. A tensor that is not
// contiguous, or of a type other than F32, F16 or I32, is listed with the reason it was not read.

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <bcrypt.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include "llama_abi.h"

#pragma comment(lib, "bcrypt.lib")

namespace {

// The pinned build: llama.cpp commit fb27a52, the build Alelyon's installer ships (2026-10-07).
constexpr const char* kLlamaSha256 = "bf5af913ee1c7d75a615d0de8f7036ab8ed0e887f50df9f14e2de90ed72d33e2";
constexpr const char* kGgmlBaseSha256 = "b0d6699a516ac89e133df135b4ba7d9df6d4cdf62bf1f351ab79ade269362425";

// The activations recorded by default: the names llama.cpp's graph builders give them, before the "-<layer>".
const char* const kDefaultNames[] = {"l_out", "attn_out", "ffn_out", "attn_norm", "ffn_norm", "ffn_moe_topk",
                                     "ffn_moe_probs", "ffn_moe_weights", "result_norm", "result_output"};

std::wstring wide(const std::string& text) {
    if (text.empty()) return std::wstring();
    const int size = MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0);
    std::wstring out(static_cast<size_t>(size), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), out.data(), size);
    return out;
}

// SHA-256 of a file as lowercase hex, through Windows' CNG; empty when it cannot be read.
std::string sha256_file(const std::wstring& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) return std::string();
    BCRYPT_ALG_HANDLE alg = nullptr;
    BCRYPT_HASH_HANDLE hash = nullptr;
    if (BCryptOpenAlgorithmProvider(&alg, BCRYPT_SHA256_ALGORITHM, nullptr, 0) != 0) return std::string();
    std::string out;
    if (BCryptCreateHash(alg, &hash, nullptr, 0, nullptr, 0, 0) == 0) {
        std::vector<char> buffer(1 << 20);
        bool ok = true;
        while (in) {
            in.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
            const auto got = in.gcount();
            if (got > 0 && BCryptHashData(hash, reinterpret_cast<PUCHAR>(buffer.data()), static_cast<ULONG>(got), 0) != 0) {
                ok = false;
                break;
            }
        }
        unsigned char digest[32];
        if (ok && BCryptFinishHash(hash, digest, sizeof(digest), 0) == 0) {
            static const char* hex = "0123456789abcdef";
            for (unsigned char b : digest) {
                out += hex[b >> 4];
                out += hex[b & 15];
            }
        }
        BCryptDestroyHash(hash);
    }
    BCryptCloseAlgorithmProvider(alg, 0);
    return out;
}

void json_string(std::ostringstream& o, const std::string& s) {
    o << '"';
    for (unsigned char c : s) {
        switch (c) {
            case '"': o << "\\\""; break;
            case '\\': o << "\\\\"; break;
            case '\n': o << "\\n"; break;
            case '\r': o << "\\r"; break;
            case '\t': o << "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof(buf), "\\u%04x", c);
                    o << buf;
                } else {
                    o << static_cast<char>(c);
                }
        }
    }
    o << '"';
}

void json_number(std::ostringstream& o, double v) {
    if (!std::isfinite(v)) {
        o << "null";
        return;
    }
    char buf[40];
    std::snprintf(buf, sizeof(buf), "%.17g", v);
    o << buf;
}

// A compensated (Neumaier) sum.
struct Sum {
    double sum = 0.0;
    double carry = 0.0;
    void add(double x) {
        const double t = sum + x;
        if (std::fabs(sum) >= std::fabs(x)) {
            carry += (sum - t) + x;
        } else {
            carry += (x - t) + sum;
        }
        sum = t;
    }
    double value() const { return sum + carry; }
};

struct Stats {
    std::string type;
    std::vector<int64_t> shape;
    int64_t times = 0;
    int64_t count = 0;
    int64_t nonfinite = 0;
    int64_t zeros = 0;
    double min = INFINITY;
    double max = -INFINITY;
    Sum total, absolute, squares;
    std::string skipped;
};

struct Experts {
    int64_t used_per_token = 0;
    std::map<int64_t, int64_t> counts;
};

struct Probe {
    fn_ggml_backend_tensor_get tensor_get = nullptr;
    fn_ggml_nbytes nbytes = nullptr;
    fn_ggml_nelements nelements = nullptr;
    bool all = false;
    std::map<std::string, Stats> tensors;
    std::map<int64_t, Experts> experts;
    std::vector<unsigned char> buffer;
};

// "l_out-12" -> ("l_out", 12); a name with no layer suffix -> (name, -1).
std::pair<std::string, int64_t> split_name(const char* name) {
    const std::string s(name);
    const auto dash = s.rfind('-');
    if (dash != std::string::npos && dash + 1 < s.size() &&
        std::all_of(s.begin() + static_cast<std::ptrdiff_t>(dash) + 1, s.end(), [](char c) { return c >= '0' && c <= '9'; })) {
        return {s.substr(0, dash), std::stoll(s.substr(dash + 1))};
    }
    return {s, -1};
}

bool wanted(const Probe& p, const char* name) {
    if (p.all) return name[0] != '\0';
    const auto base = split_name(name).first;
    for (const char* w : kDefaultNames) {
        if (base == w) return true;
    }
    return false;
}

bool contiguous(const ggml_tensor* t, size_t element) {
    if (t->nb[0] != element) return false;
    for (int i = 1; i < GGML_MAX_DIMS; ++i) {
        if (t->nb[i] != t->nb[i - 1] * static_cast<size_t>(t->ne[i - 1])) return false;
    }
    return true;
}

float half_to_float(uint16_t h) {
    const uint32_t sign = static_cast<uint32_t>(h & 0x8000) << 16;
    uint32_t exponent = (h >> 10) & 0x1f;
    uint32_t mantissa = h & 0x3ff;
    uint32_t bits;
    if (exponent == 0) {
        if (mantissa == 0) {
            bits = sign;
        } else {
            exponent = 127 - 15 + 1;
            while ((mantissa & 0x400) == 0) {
                mantissa <<= 1;
                --exponent;
            }
            mantissa &= 0x3ff;
            bits = sign | (exponent << 23) | (mantissa << 13);
        }
    } else if (exponent == 31) {
        bits = sign | 0x7f800000u | (mantissa << 13);
    } else {
        bits = sign | ((exponent + 127 - 15) << 23) | (mantissa << 13);
    }
    float f;
    std::memcpy(&f, &bits, sizeof(f));
    return f;
}

bool on_tensor(ggml_tensor* t, bool ask, void* user_data) {
    auto& p = *static_cast<Probe*>(user_data);
    if (ask) return wanted(p, t->name);
    if (!wanted(p, t->name)) return true;
    Stats& s = p.tensors[t->name];
    s.times += 1;
    if (s.shape.empty()) {
        for (int i = 0; i < GGML_MAX_DIMS; ++i) s.shape.push_back(t->ne[i]);
    }
    const auto [base, layer] = split_name(t->name);
    size_t element = 0;
    switch (t->type) {
        case GGML_TYPE_F32: s.type = "f32"; element = 4; break;
        case GGML_TYPE_F16: s.type = "f16"; element = 2; break;
        case GGML_TYPE_I32: s.type = "i32"; element = 4; break;
        default:
            s.type = "other";
            s.skipped = "its type is not F32, F16 or I32";
            return true;
    }
    const int64_t n = p.nelements(t);
    if (contiguous(t, element)) {
        const size_t bytes = p.nbytes(t);
        p.buffer.resize(bytes);
        p.tensor_get(t, p.buffer.data(), 0, bytes);
    } else if (t->nb[0] == element) {
        // A view whose rows are contiguous (the router's top-k is a slice of a sort): read it row by row at its
        // strides, into one packed buffer.
        const size_t row = static_cast<size_t>(t->ne[0]) * element;
        p.buffer.resize(static_cast<size_t>(n) * element);
        size_t at = 0;
        for (int64_t i3 = 0; i3 < t->ne[3]; ++i3) {
            for (int64_t i2 = 0; i2 < t->ne[2]; ++i2) {
                for (int64_t i1 = 0; i1 < t->ne[1]; ++i1) {
                    const size_t offset = static_cast<size_t>(i1) * t->nb[1] + static_cast<size_t>(i2) * t->nb[2] +
                                          static_cast<size_t>(i3) * t->nb[3];
                    p.tensor_get(t, p.buffer.data() + at, offset, row);
                    at += row;
                }
            }
        }
    } else {
        s.skipped = "its elements are not contiguous within a row";
        return true;
    }
    if (t->type == GGML_TYPE_I32) {
        // The router's choice: expert ids, n_expert_used per token.
        const auto* ids = reinterpret_cast<const int32_t*>(p.buffer.data());
        if (base == "ffn_moe_topk") {
            Experts& e = p.experts[layer];
            e.used_per_token = t->ne[0];
            for (int64_t i = 0; i < n; ++i) e.counts[ids[i]] += 1;
        }
        for (int64_t i = 0; i < n; ++i) {
            const double v = ids[i];
            s.count += 1;
            s.min = std::min(s.min, v);
            s.max = std::max(s.max, v);
            s.total.add(v);
            s.absolute.add(std::fabs(v));
            s.squares.add(v * v);
            if (v == 0.0) s.zeros += 1;
        }
        return true;
    }
    for (int64_t i = 0; i < n; ++i) {
        double v;
        if (t->type == GGML_TYPE_F32) {
            float f;
            std::memcpy(&f, p.buffer.data() + static_cast<size_t>(i) * 4, 4);
            v = f;
        } else {
            uint16_t h;
            std::memcpy(&h, p.buffer.data() + static_cast<size_t>(i) * 2, 2);
            v = half_to_float(h);
        }
        if (!std::isfinite(v)) {
            s.nonfinite += 1;
            continue;
        }
        s.count += 1;
        if (v == 0.0) s.zeros += 1;
        s.min = std::min(s.min, v);
        s.max = std::max(s.max, v);
        s.total.add(v);
        s.absolute.add(std::fabs(v));
        s.squares.add(v * v);
    }
    return true;
}

struct Args {
    std::string dir, model, prompt, prompt_file, error;
    int32_t gpu_layers = 0;
    uint32_t ctx = 2048;
    bool all = false;
};

Args parse(int argc, const char** argv) {
    Args a;
    for (int i = 1; i < argc; ++i) {
        const std::string k = argv[i];
        auto next = [&](std::string& out) {
            if (i + 1 >= argc) {
                a.error = k + " needs a value";
                return;
            }
            out = argv[++i];
        };
        std::string v;
        if (k == "--dir") next(a.dir);
        else if (k == "--model") next(a.model);
        else if (k == "--prompt") next(a.prompt);
        else if (k == "--prompt-file") next(a.prompt_file);
        else if (k == "--gpu-layers") { next(v); a.gpu_layers = std::atoi(v.c_str()); }
        else if (k == "--ctx") { next(v); a.ctx = static_cast<uint32_t>(std::max(256, std::atoi(v.c_str()))); }
        else if (k == "--all") a.all = true;
        else a.error = "unknown argument " + k;
    }
    if (a.error.empty() && (a.dir.empty() || a.model.empty())) a.error = "--dir and --model are required";
    return a;
}

int refuse(const std::string& why) {
    std::ostringstream o;
    o << "{\"ok\":false,\"refusal\":";
    json_string(o, why);
    o << "}\n";
    std::fputs(o.str().c_str(), stdout);
    std::fflush(stdout);
    return 2;
}

template <typename F>
F symbol(HMODULE module, const char* name) {
    return reinterpret_cast<F>(reinterpret_cast<void*>(GetProcAddress(module, name)));
}

}  // namespace

extern "C" int lattice_probe_main(int argc, const char** argv) {
    Args args = parse(argc, argv);
    if (!args.error.empty()) return refuse(args.error);
    if (!args.prompt_file.empty()) {
        std::ifstream in(wide(args.prompt_file), std::ios::binary);
        if (!in) return refuse("the prompt file could not be read");
        std::ostringstream text;
        text << in.rdbuf();
        args.prompt = text.str();
    }
    if (args.prompt.empty()) return refuse("no prompt was given");

    const std::wstring dir = wide(args.dir);
    struct Pinned {
        const wchar_t* file;
        const char* name;
        const char* sha256;
    };
    for (const Pinned& pin : {Pinned{L"\\llama.dll", "llama.dll", kLlamaSha256},
                              Pinned{L"\\ggml-base.dll", "ggml-base.dll", kGgmlBaseSha256}}) {
        const std::string got = sha256_file(dir + pin.file);
        const char* want = pin.sha256;
        if (got != want) {
            return refuse(std::string(pin.name) + " is not the pinned llama.cpp build (fb27a52): its SHA-256 is " +
                          (got.empty() ? std::string("unreadable") : got) + ", the probe's layouts were taken from " +
                          want);
        }
    }
    SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS);
    AddDllDirectory(dir.c_str());
    const DWORD flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
    HMODULE llama = LoadLibraryExW((dir + L"\\llama.dll").c_str(), nullptr, flags);
    HMODULE ggml = LoadLibraryExW((dir + L"\\ggml.dll").c_str(), nullptr, flags);
    HMODULE base = LoadLibraryExW((dir + L"\\ggml-base.dll").c_str(), nullptr, flags);
    if (!llama || !ggml || !base) return refuse("the llama.cpp libraries could not be loaded from the folder given");

    auto backend_init = symbol<fn_llama_backend_init>(llama, "llama_backend_init");
    auto model_defaults = symbol<fn_llama_model_default_params>(llama, "llama_model_default_params");
    auto context_defaults = symbol<fn_llama_context_default_params>(llama, "llama_context_default_params");
    auto load_model = symbol<fn_llama_model_load_from_file>(llama, "llama_model_load_from_file");
    auto free_model = symbol<fn_llama_model_free>(llama, "llama_model_free");
    auto init_context = symbol<fn_llama_init_from_model>(llama, "llama_init_from_model");
    auto free_context = symbol<fn_llama_free>(llama, "llama_free");
    auto get_vocab = symbol<fn_llama_model_get_vocab>(llama, "llama_model_get_vocab");
    auto tokenize = symbol<fn_llama_tokenize>(llama, "llama_tokenize");
    auto batch_one = symbol<fn_llama_batch_get_one>(llama, "llama_batch_get_one");
    auto decode = symbol<fn_llama_decode>(llama, "llama_decode");
    auto n_layer = symbol<fn_llama_model_n_layer>(llama, "llama_model_n_layer");
    auto meta = symbol<fn_llama_model_meta_val_str>(llama, "llama_model_meta_val_str");
    auto load_all = symbol<fn_ggml_backend_load_all_from_path>(ggml, "ggml_backend_load_all_from_path");
    Probe probe;
    probe.tensor_get = symbol<fn_ggml_backend_tensor_get>(base, "ggml_backend_tensor_get");
    probe.nbytes = symbol<fn_ggml_nbytes>(base, "ggml_nbytes");
    probe.nelements = symbol<fn_ggml_nelements>(base, "ggml_nelements");
    probe.all = args.all;
    if (!backend_init || !model_defaults || !context_defaults || !load_model || !free_model || !init_context ||
        !free_context || !get_vocab || !tokenize || !batch_one || !decode || !n_layer || !meta || !load_all ||
        !probe.tensor_get || !probe.nbytes || !probe.nelements) {
        return refuse("a llama.cpp function the probe needs is missing from the build");
    }

    // The backends (CPU, Vulkan) are separate libraries in this build, loaded from its folder as llama-server does.
    load_all(args.dir.c_str());
    backend_init();
    using clock = std::chrono::steady_clock;
    const auto t0 = clock::now();
    llama_model_params mp = model_defaults();
    mp.n_gpu_layers = args.gpu_layers;
    llama_model* model = load_model(args.model.c_str(), mp);
    if (!model) return refuse("llama.cpp could not load the model (too large for the layers asked of the card, or not a model it reads)");
    const auto t1 = clock::now();

    const llama_vocab* vocab = get_vocab(model);
    std::vector<llama_token> tokens(args.prompt.size() + 16);
    int32_t n = tokenize(vocab, args.prompt.data(), static_cast<int32_t>(args.prompt.size()), tokens.data(),
                         static_cast<int32_t>(tokens.size()), true, false);
    if (n < 0) {
        tokens.resize(static_cast<size_t>(-n));
        n = tokenize(vocab, args.prompt.data(), static_cast<int32_t>(args.prompt.size()), tokens.data(),
                     static_cast<int32_t>(tokens.size()), true, false);
    }
    if (n <= 0) {
        free_model(model);
        return refuse("the prompt gave no tokens");
    }
    tokens.resize(static_cast<size_t>(n));
    if (static_cast<uint32_t>(n) > args.ctx) {
        free_model(model);
        return refuse("the prompt is longer than the context asked for");
    }

    llama_context_params cp = context_defaults();
    cp.n_ctx = args.ctx;
    cp.n_batch = args.ctx;
    cp.n_ubatch = std::min<uint32_t>(args.ctx, 512);
    cp.cb_eval = on_tensor;
    cp.cb_eval_user_data = &probe;
    cp.no_perf = true;
    llama_context* ctx = init_context(model, cp);
    if (!ctx) {
        free_model(model);
        return refuse("llama.cpp could not make a context for the model");
    }
    const auto t2 = clock::now();
    const int32_t rc = decode(ctx, batch_one(tokens.data(), n));
    const auto t3 = clock::now();

    char arch[128] = {0};
    char expert_count[64] = {0};
    meta(model, "general.architecture", arch, sizeof(arch));
    if (arch[0]) {
        const std::string key = std::string(arch) + ".expert_count";
        meta(model, key.c_str(), expert_count, sizeof(expert_count));
    }
    const int32_t layers = n_layer(model);
    free_context(ctx);
    free_model(model);
    if (rc != 0) return refuse("llama.cpp's decode of the prompt failed (code " + std::to_string(rc) + ")");

    const auto ms = [](auto a, auto b) { return std::chrono::duration<double, std::milli>(b - a).count(); };
    std::ostringstream o;
    o << "{\"ok\":true,\"build\":\"llama.cpp fb27a52\",\"architecture\":";
    json_string(o, arch);
    o << ",\"layers\":" << layers << ",\"expert_count\":" << (expert_count[0] ? expert_count : "null")
      << ",\"tokens\":" << n << ",\"token_ids\":[";
    for (int32_t i = 0; i < n; ++i) o << (i ? "," : "") << tokens[static_cast<size_t>(i)];
    o << "],\"gpu_layers\":" << args.gpu_layers << ",\"context\":" << args.ctx
      << ",\"load_ms\":";
    json_number(o, ms(t0, t1));
    o << ",\"decode_ms\":";
    json_number(o, ms(t2, t3));
    o << ",\"tensors\":[";
    bool first = true;
    for (const auto& [name, s] : probe.tensors) {
        if (!first) o << ',';
        first = false;
        const auto [base_name, layer] = split_name(name.c_str());
        o << "{\"name\":";
        json_string(o, name);
        o << ",\"op\":";
        json_string(o, base_name);
        o << ",\"layer\":" << layer << ",\"type\":";
        json_string(o, s.type);
        o << ",\"shape\":[";
        for (size_t i = 0; i < s.shape.size(); ++i) o << (i ? "," : "") << s.shape[i];
        o << "],\"times\":" << s.times << ",\"count\":" << s.count << ",\"nonfinite\":" << s.nonfinite
          << ",\"zeros\":" << s.zeros;
        if (!s.skipped.empty()) {
            o << ",\"skipped\":";
            json_string(o, s.skipped);
        }
        if (s.count > 0) {
            const double c = static_cast<double>(s.count);
            const double mean = s.total.value() / c;
            const double var = std::max(0.0, s.squares.value() / c - mean * mean);
            o << ",\"min\":";
            json_number(o, s.min);
            o << ",\"max\":";
            json_number(o, s.max);
            o << ",\"mean\":";
            json_number(o, mean);
            o << ",\"std\":";
            json_number(o, std::sqrt(var));
            o << ",\"rms\":";
            json_number(o, std::sqrt(s.squares.value() / c));
            o << ",\"mean_abs\":";
            json_number(o, s.absolute.value() / c);
        }
        o << '}';
    }
    o << "],\"experts\":[";
    first = true;
    for (const auto& [layer, e] : probe.experts) {
        if (!first) o << ',';
        first = false;
        o << "{\"layer\":" << layer << ",\"used_per_token\":" << e.used_per_token << ",\"counts\":{";
        bool f2 = true;
        for (const auto& [id, count] : e.counts) {
            o << (f2 ? "" : ",") << '"' << id << "\":" << count;
            f2 = false;
        }
        o << "}}";
    }
    o << "]}\n";
    std::fputs(o.str().c_str(), stdout);
    std::fflush(stdout);
    return 0;
}
