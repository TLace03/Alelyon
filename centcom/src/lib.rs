//! `centcom`: Alelyon, Project Angel's one window (CENTCOM is its working name, since 2026-10-03).
//!
//! ```text
//! centcom [--section overview|sinai|words|lattice|fleet|data|trust|compute|account] [--tab captions|dictation|pc|files|library|<a Fleet, Compute or Lattice tab>|<a Data store>[/<table>]|appearance]
//!         [--verify <receipt.json>] [--screenshot <file.png> [--after-ms N]]
//! ```
//!
//! Since 2026-10-03 the project's applications are becoming one CENTCOM-style window on iced,
//! with a loading symbol wherever work is happening. This first slice has the rail of every section, the
//! Overview of what runs on this computer, the Transcription section wired to the speech engine (live captions,
//! dictation, the computer's own audio, audio files), Sinai's on-air state, and, for every section still to come,
//! each of its features with where it lives today.
//!
//! `--screenshot` draws the chosen page, writes it as a PNG after `--after-ms` (2500 by default) and exits:
//! the proof a change looks right, without anyone watching the screen.
//!
//! The speech engine: CENTCOM reads one that is running, and starts one (`<engine> serve`, adding
//! `CENTCOM_EARS_ARGS`, ended when CENTCOM ends) where its build says where the engine is (`installed.rs`).
//!
//! The app is this library and a thin program (`src/main.rs`) that runs it with an empty plug-in registry. A build
//! that carries further sections links this library, registers them in an [`alelyon_plugin_api::Registry`] and calls
//! [`run`] with it; see `capability.rs` for what each optional section needs. What a build carries that is not a page
//! (the verifier's replay substrate, where Sinai's mind, the speech engine and voice enrolment's tools are) is
//! installed by `run` (`installed.rs`); the public build carries none of it.

#![deny(unsafe_code)]

mod app;
// The sign-in screen's living backdrop: a carbon-fibre plain under a rolling gold fog the cursor stirs.
mod backdrop;
mod brand;
pub mod capability;
pub mod catalogue;
pub mod checkout;
mod chrome;
// Asks Windows whether the mouse button is held (GetAsyncKeyState), so it may use unsafe code.
#[allow(unsafe_code)]
mod pointer;
// Joins the starting terminal's console through Win32, so it may use unsafe code, as the job object does.
#[allow(unsafe_code)]
mod console;
pub mod compute;
mod lattice;
mod data;
mod dock;
mod ears;
mod face;
// CENTCOM_FRAME_STATS: updates, views and frames a second, and the messages behind them (a debug aid, off by default).
mod frame_stats;
mod grants;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod job;
pub mod library;
// What the build carries that is not a page: the replay substrate and where the private programs are.
pub mod installed;
// Live data: pages are told when their stores, files and folders change (Windows' change notification is unsafe code).
#[allow(unsafe_code)]
pub mod live;
mod machine;
mod memory_map;
// Models: the GGUF files and endpoints this PC can use, which one Sinai's page and Lattice's chat each talk to, and
// Sinai's page talking to the person's own model (Windows' file picker is unsafe code, allowed in its own file).
mod models;
mod pages;
// Alelyon's own preferences on this PC (preferences.json): today, whether the sign-in backdrop moves.
mod prefs;
mod probe;
// The research archive: papers on a subject, followed through citations (crates/alelyon-research).
mod research;
mod signin;
mod social;
mod sinai;
pub mod spinner;
// Captures other windows through Win32 (PrintWindow and GDI), so it may use unsafe code, as the job object does.
#[allow(unsafe_code)]
mod stage;
pub mod sqlite_ro;
// The window's surfaces, buttons and type, on lattice-app's palette (plug-ins draw with it too).
pub mod theme;
pub mod trust;
pub mod ui;
pub mod utc;
mod view;
mod voice;

// Sinai's bust, its expressions, the appearance a person gives it, and where a window keeps that appearance: the
// `sinai-face` crate (../crates/sinai-face), which the Angel window links too, so both windows draw one Sinai (no
// copies). Its first-start move of a window's settings is never called here.
use sinai_face::{appearance, body, expression, state_home};

use std::process::ExitCode;

pub use alelyon_plugin_api::Registry;
use iced::{Size, window};

/// Runs the window with the sections `registry` adds (none in the public build), and says how it ended.
pub fn run(registry: Registry) -> ExitCode {
    // `--research-run <job.json>`: one research harvest or update as a process of its own (no window), started by the
    // Research page so the run outlives the window (the research crate's jobs module).
    if std::env::args().nth(1).as_deref() == Some("--research-run") {
        let Some(spec) = std::env::args().nth(2) else { return ExitCode::from(2) };
        return if alelyon_research::jobs::run(std::path::Path::new(&spec)) { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    }
    installed::install(&registry);
    // `--research-mcp`: the research archive as a read-only MCP server on stdin and stdout, for models and agents
    // (the research crate's MCP module); `--research-mcp --allow-findings` also offers `save_finding`, as Sinai's
    // loop starts it. Checked first: no console is joined and no window opens.
    if std::env::args().nth(1).as_deref() == Some("--research-mcp") {
        let db = alelyon_research::mcp::default_db();
        let findings = std::env::args().nth(2).as_deref() == Some("--allow-findings");
        return match alelyon_research::mcp::serve_with(&db, findings, std::io::stdin().lock(), std::io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    console::join_parent();
    let options = match app::Options::parse_with(std::env::args().skip(1), &registry) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let screenshot = options.screenshot.is_some();
    prefer_opengl();
    lattice_app::theme::set_fonts(lattice_app::theme::Fonts::detect());
    theme::set_display(theme::detect_display());
    let ran = iced::application(move || app::App::boot_with(options.clone(), &registry), app::App::update, view::view)
        .title(|_: &app::App| "Alelyon".to_string())
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::theme())
        .style(|_: &app::App, t: &iced::Theme| theme::app_style(t))
        .window(window::Settings {
            size: Size::new(1440.0, 900.0),
            min_size: Some(Size::new(980.0, 640.0)),
            icon: brand::window_icon(),
            // no Windows frame: the title bar and edges are the window's own (src/chrome.rs)
            decorations: false,
            #[cfg(windows)]
            platform_specific: window::settings::PlatformSpecific { undecorated_shadow: true, ..Default::default() },
            ..window::Settings::default()
        })
        .default_font(theme::fonts().ui)
        .antialiasing(true)
        .run();
    match ran {
        Err(e) => {
            eprintln!("centcom: {e}");
            ExitCode::FAILURE
        }
        Ok(()) if screenshot && lattice_app::screenshot::failed() => ExitCode::FAILURE,
        Ok(()) => ExitCode::SUCCESS,
    }
}

/// OpenGL unless the environment chose a backend: the native Lattice window measured on this machine
/// (2026-09-30, RX 9070 XT) that under Vulkan and DirectX 12 a
/// thread of the AMD driver busy-waits a whole core for as long as a window is open, and under OpenGL the
/// same window idles at 0.0%. CENTCOM draws rectangles and text, as that window does.
#[allow(unsafe_code)]
fn prefer_opengl() {
    if let Some(backend) = lattice_app::backend_to_request(std::env::var_os("WGPU_BACKEND").as_deref()) {
        // SAFETY: `set_var` is unsound only when another thread reads or writes the environment at the same
        // time; this runs first in `run`, before iced or any thread of CENTCOM exists.
        unsafe { std::env::set_var("WGPU_BACKEND", backend) };
    }
}
