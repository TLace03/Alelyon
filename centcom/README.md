# CENTCOM: the Alelyon window

The desktop app's window, built here with an empty plug-in registry: the open app. It is
an [iced](https://iced.rs) application drawn over wgpu (OpenGL unless `WGPU_BACKEND` says
otherwise), black and gold, redrawn only when something changes.

| Section | What it is |
|---|---|
| Overview | What runs on this PC, and the features of every section with where they stand. |
| Sinai | Sinai's face and page. Choose a model of your own (a GGUF file run on this PC, or an endpoint you use) and Sinai answers with it. |
| Words | Transcription: live captions, dictation and files. It starts the speech engine (`angel-ears.exe` beside the window, or the one `CENTCOM_EARS` names) and says what to put in place first when its model or recogniser is missing. |
| Lattice | The chat and coding agent with its IDE, its runs, the measurement engine's Morphometry and Foundry tabs, and the Training Studio's data. |
| Data | The project's SQLite databases, read-only, as tables to browse, when they are on this PC. |
| Research | The research archive: subjects, their papers, the citation map and the gaps worth pursuing, kept on this PC. |
| Trust | The receipt verifier: drop a receipt and its inputs, pin the issuer's key, read every check. |
| Compute | The simulator, stepped in real time on the processor and drawn by its CPU ray caster. |
| Account | Signing in (optional: *Use Alelyon offline* opens everything that runs on this PC). |

A section whose part this build does not carry (the Fleet page, the receipt replay's
deterministic kernel, Sinai's mind, voice enrolment) says so, what the part is and how to
get it, instead of showing an error.

## Build and run

Windows, Rust 1.97 or later, and the C++ build tools (the measurement engine and the
activation probe compile C++ with the platform compiler). Every dependency is pinned by
`Cargo.lock`; nothing outside this repository is needed.

```bash
cargo build --release --locked
target/release/centcom.exe
```

`centcom --section compute` opens a section directly; `centcom --help` lists the options.
The simulator's scenes are read from `sim/crates/sim-scene/tests/fixtures/mujoco/` of the
checkout the program runs from.

```bash
cargo test --locked
```

Set `ALELYON_NO_CREDENTIALS=1` for test or screenshot runs: the window then never reads
or writes a key kept in Windows Credential Manager.
