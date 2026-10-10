//! What this build carries that is not a page, as [`crate::run`] was handed it: the receipt verifier's replay
//! substrate, and where the programs the public app does not ship are found (Sinai's mind, voice enrolment's
//! tools), and a build's own rule for the speech engine's program.
//!
//! The public build carries none of them. Its Trust page checks a receipt's signature, inputs and records but does
//! not replay it (those checks say "not performed", and no receipt passes); its Sinai page says Sinai's mind is not
//! installed; and it cannot enrol a voice. It starts the speech engine by the window's built-in rule
//! (`ears::locate`), which a build's `ears` locator replaces. A build that carries them (a private official build)
//! registers them in its [`Registry`](alelyon_plugin_api::Registry), and `run` installs them here before the window
//! opens.
//!
//! Installed once per process, and read where the window needs them; the first [`install`] wins.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use alelyon_plugin_api::{Processes, Registry, ReplayKernel, VoiceTools};

struct Installed {
    replay: Option<Arc<dyn ReplayKernel>>,
    processes: Processes,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();

/// Install what `registry` carries that is not a page. `run` calls it; a wrapper's tests may too, with the registry
/// its program runs. A second call changes nothing: the first registry stays.
pub fn install(registry: &Registry) {
    let _ = INSTALLED.set(Installed { replay: registry.replay_kernel(), processes: registry.processes().clone() });
}

fn processes() -> Option<&'static Processes> {
    INSTALLED.get().map(|i| &i.processes)
}

/// The substrate the verifier replays on, or None: the receipt is checked but not replayed.
pub fn replay() -> Option<Arc<dyn ReplayKernel>> {
    INSTALLED.get().and_then(|i| i.replay.clone())
}

/// Where Sinai's mind is reached: None when this build does not carry it, else what was found or why not.
pub fn sinai_mind() -> Option<Result<String, String>> {
    processes().and_then(|p| p.sinai_mind.as_ref()).map(|locate| locate())
}

/// Where the speech engine's program is by this build's own rule: None when it registers none (the window's built-in
/// rule, `ears::locate`, applies), else what was found or why not.
pub fn ears() -> Option<Result<PathBuf, String>> {
    processes().and_then(|p| p.ears.as_ref()).map(|locate| locate())
}

/// Where voice enrolment's tools are: None when this build does not carry them, else what was found or why not.
pub fn voice() -> Option<Result<VoiceTools, String>> {
    processes().and_then(|p| p.voice.as_ref()).map(|locate| locate())
}

/// The words for something this build does not carry.
pub const SINAI_NOT_INSTALLED: &str = "Sinai's mind is not installed on this PC.";
pub const VOICE_NOT_IN_BUILD: &str = "Voice enrolment is not in this build of Alelyon.";
pub const REPLAY_NOT_IN_BUILD: &str = "Replay not performed: the replay engine is not in this build. The number, its width and its budget were not checked, so this receipt does not pass here.";

#[cfg(test)]
mod tests {
    use super::*;

    /// No centcom test installs a registry, so every one of them sees the public build.
    #[test]
    fn the_public_build_carries_nothing_that_is_not_a_page() {
        assert!(replay().is_none());
        assert!(sinai_mind().is_none() && ears().is_none() && voice().is_none());
    }
}
