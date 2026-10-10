# llama.cpp headers for the activation probe

Two files from [ggml-org/llama.cpp](https://github.com/ggml-org/llama.cpp) at commit
`fb27a525d28381a16a4bb038858a10e4927381ca` (2026-09-16), the commit the pinned llama.cpp build that
Alelyon's installer ships (`llama-server --version`: "0.4.1-dev (build 1, commit fb27a52)") was made from.
They are MIT-licensed; the licence is `LICENSE` here, unchanged, and the probe's `NOTICE` lists them. Downloaded on
2026-10-07: exactly these two headers.

| File | Source path at that commit | SHA-256 |
|---|---|---|
| `ggml.h` | `ggml/include/ggml.h` | `12ee71f99db7db9b353bc02b1fbb57c344ee17c01ac5fb7952b41a637a747ea9` |
| `LICENSE` | `LICENSE` | `94f29bbed6a22c35b992c5c6ebf0e7c92f13b836b90f36f461c9cf2f0f1d010d` |

`ggml.h` is used as it is (for `struct ggml_tensor`, `enum ggml_type` and its constants). `llama.h` (84,829 bytes)
includes four headers that were not downloaded (`ggml-cpu.h`, `ggml-backend.h`, `ggml-opt.h`, `gguf.h`), so it is not
vendored: the three structs and the functions the probe needs are transcribed from it, field for field, in
the probe's `src/llama_abi.h`, which carries the same notice. The probe refuses any `llama.dll` or `ggml-base.dll` whose
SHA-256 is not the pinned build's, because a transcribed layout is right only for the build it was taken from.
