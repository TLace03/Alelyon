// The parts of llama.cpp's public C interface the activation probe uses, transcribed field for field from
// `include/llama.h` at commit fb27a525d28381a16a4bb038858a10e4927381ca of ggml-org/llama.cpp (MIT licence:
// ../vendor/llama.cpp/LICENSE; Copyright (c) 2023-2026 The ggml authors). `llama.h` itself is not vendored because it
// includes headers that were not downloaded (see ../vendor/llama.cpp/README.md). Enums are int-sized and opaque
// types are pointers, as in the original; the probe refuses any build other than the pinned one.
//
// Function pointer types only: the probe loads llama.dll at run time and links nothing from llama.cpp.

#pragma once

#include <stddef.h>
#include <stdint.h>

#include "ggml.h"

extern "C" {

typedef int32_t llama_pos;
typedef int32_t llama_token;
typedef int32_t llama_seq_id;

struct llama_model;
struct llama_context;
struct llama_vocab;
struct llama_sampler_seq_config;
struct llama_model_tensor_buft_override;
struct llama_model_kv_override;
typedef struct ggml_backend_device * ggml_backend_dev_t;

typedef bool (*llama_progress_callback)(float progress, void * user_data);
// From ggml-backend.h at the same commit: called with ask=true to learn whether the tensor's data is wanted, then with
// ask=false once it is computed; returning false stops the graph.
typedef bool (*ggml_backend_sched_eval_callback)(struct ggml_tensor * t, bool ask, void * user_data);

// `struct llama_model_params`.
struct llama_model_params {
    ggml_backend_dev_t * devices;
    const struct llama_model_tensor_buft_override * tensor_buft_overrides;
    int32_t n_gpu_layers;
    int32_t split_mode;  // enum llama_split_mode
    int32_t load_mode;   // enum llama_load_mode
    int32_t lazy_mode;   // enum llama_lazy_mode
    int32_t main_gpu;
    const float * tensor_split;
    llama_progress_callback progress_callback;
    void * progress_callback_user_data;
    const struct llama_model_kv_override * kv_overrides;
    bool vocab_only;
    bool check_tensors;
    bool use_extra_bufts;
    bool no_host;
    bool no_alloc;
    bool load_mtp;
};

// `struct llama_context_params`.
struct llama_context_params {
    uint32_t n_ctx;
    uint32_t n_batch;
    uint32_t n_ubatch;
    uint32_t n_seq_max;
    uint32_t n_rs_seq;
    uint32_t n_outputs_max;
    uint32_t n_outputs_max_per_seq;
    int32_t n_threads;
    int32_t n_threads_batch;
    int32_t ctx_type;           // enum llama_context_type
    int32_t rope_scaling_type;  // enum llama_rope_scaling_type
    int32_t pooling_type;       // enum llama_pooling_type
    int32_t attention_type;     // enum llama_attention_type
    int32_t flash_attn_type;    // enum llama_flash_attn_type
    float rope_freq_base;
    float rope_freq_scale;
    float yarn_ext_factor;
    float yarn_attn_factor;
    float yarn_beta_fast;
    float yarn_beta_slow;
    uint32_t yarn_orig_ctx;
    float defrag_thold;
    ggml_backend_sched_eval_callback cb_eval;
    void * cb_eval_user_data;
    enum ggml_type type_k;
    enum ggml_type type_v;
    ggml_abort_callback abort_callback;
    void * abort_callback_data;
    bool embeddings;
    bool offload_kqv;
    bool no_perf;
    bool op_offload;
    bool swa_full;
    bool kv_unified;
    struct llama_sampler_seq_config * samplers;
    size_t n_samplers;
    struct llama_context * ctx_other;
};

// `struct llama_batch`.
struct llama_batch {
    int32_t n_tokens;
    llama_token * token;
    float * embd;
    llama_pos * pos;
    int32_t * n_seq_id;
    llama_seq_id ** seq_id;
    int8_t * logits;
};

typedef void (*fn_llama_backend_init)(void);
typedef struct llama_model_params (*fn_llama_model_default_params)(void);
typedef struct llama_context_params (*fn_llama_context_default_params)(void);
typedef struct llama_model * (*fn_llama_model_load_from_file)(const char * path_model, struct llama_model_params params);
typedef void (*fn_llama_model_free)(struct llama_model * model);
typedef struct llama_context * (*fn_llama_init_from_model)(struct llama_model * model, struct llama_context_params params);
typedef void (*fn_llama_free)(struct llama_context * ctx);
typedef const struct llama_vocab * (*fn_llama_model_get_vocab)(const struct llama_model * model);
typedef int32_t (*fn_llama_tokenize)(const struct llama_vocab * vocab, const char * text, int32_t text_len,
                                     llama_token * tokens, int32_t n_tokens_max, bool add_special, bool parse_special);
typedef struct llama_batch (*fn_llama_batch_get_one)(llama_token * tokens, int32_t n_tokens);
typedef int32_t (*fn_llama_decode)(struct llama_context * ctx, struct llama_batch batch);
typedef int32_t (*fn_llama_model_n_layer)(const struct llama_model * model);
typedef int32_t (*fn_llama_model_meta_val_str)(const struct llama_model * model, const char * key, char * buf,
                                               size_t buf_size);

// ggml-backend.h / ggml.h functions, exported by ggml.dll and ggml-base.dll.
typedef void (*fn_ggml_backend_load_all_from_path)(const char * dir_path);
typedef void (*fn_ggml_backend_tensor_get)(const struct ggml_tensor * tensor, void * data, size_t offset, size_t size);
typedef size_t (*fn_ggml_nbytes)(const struct ggml_tensor * tensor);
typedef int64_t (*fn_ggml_nelements)(const struct ggml_tensor * tensor);

}  // extern "C"
