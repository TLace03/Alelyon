//! `centcom`: the Alelyon window as the public build carries it, with no plug-ins. The window itself is the library
//! (`src/lib.rs`); a build with further sections runs it from its own program with its own registry.

// A release build opens no console window beside the app; see `console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    centcom::run(centcom::Registry::default())
}
