//! Compiles the C++ core (`cpp/`) into a static library linked into this crate.
//!
//! C++20, no third-party code. MSVC: `/EHsc /fp:precise /W4 /permissive-`
//! (plus `/WX` and `/utf-8`), no `/arch:AVX2` and no `/fp:contract`, so every
//! float operation is the one the source names: the parity goldens compare
//! floats bit for bit. Other compilers get the equivalent:
//! `-ffp-contract=off -Wall -Wextra -Werror`.
//!
//! On Windows the native probe (`cpp/probe.cpp`, PR 3) enumerates the display
//! adapters through DXGI, so `dxgi.lib` (a Windows SDK import library) is
//! linked. The probe opens no device.
//!
//! PR 6's dequantisation (`cpp/dequant.cpp`) and weight statistics
//! (`cpp/weights.cpp`, which reads a GGUF file read-only on `std::thread`
//! workers) need nothing beyond the C++ standard library and, on Windows,
//! kernel32. `cpp/gguf_quant_tables.hpp` is generated from gguf-py (MIT) by
//! `tools/model_anatomy_goldens.py`.

const SOURCES: [&str; 16] = [
    "cpp/abi.cpp",
    "cpp/canonical_space.cpp",
    "cpp/compare.cpp",
    "cpp/constants.cpp",
    "cpp/dequant.cpp",
    "cpp/economics.cpp",
    "cpp/foundry.cpp",
    "cpp/hierarchy_morphometry.cpp",
    "cpp/json_writer.cpp",
    "cpp/morphometry.cpp",
    "cpp/probe.cpp",
    "cpp/pyfmt.cpp",
    "cpp/registration.cpp",
    "cpp/sha256.cpp",
    "cpp/template_hierarchy.cpp",
    "cpp/weights.cpp",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=cpp");
    let mut build = cc::Build::new();
    build.cpp(true).std("c++20").files(SOURCES).include("cpp");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if msvc {
        build
            .flag("/EHsc")
            .flag("/fp:precise")
            .flag("/W4")
            .flag("/WX")
            .flag("/permissive-")
            .flag("/utf-8");
    } else {
        build
            .flag("-ffp-contract=off")
            .flag("-Wall")
            .flag("-Wextra")
            .flag("-Werror");
    }
    build.compile("model_anatomy_cpp");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        println!("cargo:rustc-link-lib=dxgi");
        // The GPU Adapter Memory counter, for the chosen adapter's free memory.
        println!("cargo:rustc-link-lib=pdh");
    }
}
