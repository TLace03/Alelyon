//! "Stay signed in": the refresh token sealed with Windows' DPAPI for this Windows user. The vault (and its one
//! `unsafe` block, the DPAPI calls) lives in the `alelyon-identity-client` crate, with its tests; this module keeps
//! `crate::signin::vault::*` naming it and keeps CENTCOM's setting: `CENTCOM_SESSION_FILE` names another file than
//! `~/.alelyon/alelyon/session.sealed`. The seal's entropy is unchanged, so a session kept before the move opens.
//!
//! A test build resolves no file at all, so no test can open, replace or delete the person's kept session: until
//! 2026-10-08 a sign-in test without Stay signed in deleted the one under `%USERPROFILE%`. A test that needs a kept
//! session gives `signin::State` a scratch file of its own (`session_file`).

use std::path::PathBuf;

pub use alelyon_identity_client::vault::{forget, keep, open};

/// Where the sealed session lives: `~/.alelyon/alelyon/session.sealed` (`CENTCOM_SESSION_FILE` names another).
#[cfg(not(test))]
pub fn path() -> Option<PathBuf> {
    alelyon_identity_client::vault::path_or("CENTCOM_SESSION_FILE")
}

/// In a test build: nowhere, whatever `CENTCOM_SESSION_FILE` and `USERPROFILE` say.
#[cfg(test)]
pub fn path() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No test reaches the person's kept session: the file resolves nowhere in a test build, so neither the window's
    /// start (`State::boot`, which every `App::boot` in app.rs's tests runs) nor a sign-in, sign-out or Forget can open,
    /// replace or delete the one under `%USERPROFILE%`.
    #[test]
    fn a_test_build_never_resolves_the_session_file_under_the_users_profile() {
        let at = path();
        let under_profile = match (&at, std::env::var_os("USERPROFILE")) {
            (Some(p), Some(home)) => p.starts_with(home),
            _ => false,
        };
        assert!(!under_profile, "{at:?} is under %USERPROFILE%: a test would reach the person's kept session");
        assert_eq!(at, None, "a test build keeps no session anywhere");
        let (s, _) = crate::signin::State::boot(true);
        assert_eq!((s.session_file, s.stay), (None, false), "the window's start keeps nothing and reads nothing");
    }
}
