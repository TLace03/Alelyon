//! `lattice-probe`: the activation probe, run by CENTCOM's Morphometry tab as a process of its own (src/probe.cpp
//! says what it does and does not do). This file only hands the command line to it.

use std::ffi::{CString, c_char, c_int};

unsafe extern "C" {
    fn lattice_probe_main(argc: c_int, argv: *const *const c_char) -> c_int;
}

fn main() {
    let args: Vec<CString> = std::env::args()
        .map(|a| CString::new(a).unwrap_or_else(|_| CString::new("?").expect("no NUL")))
        .collect();
    let pointers: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
    // SAFETY: `pointers` holds `args.len()` valid NUL-terminated strings that outlive the call.
    let code = unsafe { lattice_probe_main(pointers.len() as c_int, pointers.as_ptr()) };
    std::process::exit(code);
}
