//! Alelyon opens no console window of its own (the terminal that used to open beside the app should
//! not be visible). A release build is a Windows program rather than a console one (`windows_subsystem` in main.rs),
//! as the Angel window's is; a debug build keeps its console for whoever is developing it. Without a console, a
//! release build started from a terminal (with `--screenshot`, or a mistyped option) would say nothing; so it joins
//! that terminal's console, when there is one, and its messages appear there. Started from the desktop there is none,
//! and nothing appears. The processes it starts (the ears, the issuer, git, gh, Python) are each started without a
//! window of their own.

/// Join the console of the terminal Alelyon was started from, if any, so `eprintln!` reaches it.
#[cfg(windows)]
pub fn join_parent() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
    }
    /// ATTACH_PARENT_PROCESS
    const PARENT: u32 = u32::MAX;
    // SAFETY: AttachConsole takes a process id and returns a status; it fails harmlessly when the parent has no
    // console or this process already has one (a debug build).
    unsafe {
        AttachConsole(PARENT);
    }
}

#[cfg(not(windows))]
pub fn join_parent() {}
