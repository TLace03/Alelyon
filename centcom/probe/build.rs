//! Compiles the activation probe's C++ (src/probe.cpp), which includes the vendored llama.cpp `ggml.h` and the
//! transcribed `llama_abi.h`. Nothing from llama.cpp is linked: the probe loads the pinned llama.dll at run time.

fn main() {
    println!("cargo:rerun-if-changed=src/probe.cpp");
    println!("cargo:rerun-if-changed=src/llama_abi.h");
    println!("cargo:rerun-if-changed=vendor/llama.cpp/ggml.h");
    let mut build = cc::Build::new();
    build.cpp(true).file("src/probe.cpp").include("src").include("vendor/llama.cpp");
    if std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc") {
        build.flag("/std:c++20").flag("/EHsc").flag("/W4").flag("/WX").flag("/permissive-").flag("/utf-8");
    } else {
        build.flag("-std=c++20").flag("-Wall").flag("-Wextra");
    }
    build.compile("lattice_probe_cpp");
}
