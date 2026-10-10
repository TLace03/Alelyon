//! What the window can do, and what unlocks each: the one list.
//!
//! Some of the window is there for everyone (`Always`); some only while a person is signed in (`SignedIn`); some
//! only in a build whose plug-in registry carries a named plug-in (`Plugin`); and some only when Alelyon runs from a
//! checkout of the repository (`DevCheckout`). The public build carries no plug-in, so what a plug-in unlocks is
//! absent from it. A page asks [`available`] rather than deciding for itself.

use alelyon_plugin_api::Registry;

/// Something the window offers that not every person or build has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Verify a receipt (the Trust page).
    VerifyReceipts,
    /// Friends, presence and chat (the rail's friends button and panel).
    Friends,
    /// Pages: public profiles, organizations and posts (the Pages page).
    Pages,
    /// The Compute page's emulator tab.
    ComputeEmulator,
    /// The Compute page's tab of recorded GPU jobs.
    ComputeKitJobs,
    /// Issuing signed receipts on the Trust page.
    IssueReceipts,
    /// Data trust on the Trust page.
    DataTrust,
    /// The repository's own tools read from its checkout (the fleet's sources, Sinai's voice tools).
    CheckoutTools,
    /// Replaying a receipt's number on the deterministic kernel (the Trust page's number, width, budget and tier
    /// checks). Without it a receipt is checked but not replayed, and does not pass.
    ReplayReceipts,
    /// Sinai's mind: the loop the Sinai page talks to. Without it the page says the mind is not installed.
    SinaiMind,
    /// A model of the person's own (a GGUF file run on this PC by llama.cpp, or an OpenAI-compatible endpoint), chosen
    /// in the Models panel: Sinai's page talks to it when the build carries no mind, or when the person picks it over
    /// the mind; Lattice's chat can use it too. Every build has it: it needs no server and no closed part.
    OwnModel,
    /// Starting the speech engine from the window (its program beside the window's, or `CENTCOM_EARS`). Every build
    /// has it: the engine is open source. Its model is a file the person puts in place.
    StartEars,
    /// Voice enrolment: showing, re-recording and erasing Sinai's voiceprint of its person.
    VoiceEnrolment,
    /// The simulator's scenes on the Compute page: its own test scenes, read from the source checkout.
    SimulatorScenes,
}

/// What unlocks a capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlock {
    Always,
    SignedIn,
    /// A plug-in with this id is in the build's registry.
    Plugin(&'static str),
    /// Alelyon runs from a checkout of the repository.
    DevCheckout,
}

/// Every capability, what it is, and what unlocks it.
pub const CAPABILITIES: [(Capability, &str, Unlock); 14] = [
    (Capability::VerifyReceipts, "Verify a receipt", Unlock::Always),
    (Capability::Friends, "Friends, presence and chat", Unlock::SignedIn),
    (Capability::Pages, "Pages, organizations and posts", Unlock::SignedIn),
    (Capability::ComputeEmulator, "The emulator tab", Unlock::Plugin("emulator")),
    (Capability::ComputeKitJobs, "Kit jobs", Unlock::Plugin("kit")),
    (Capability::IssueReceipts, "Issue receipts", Unlock::Plugin("issue")),
    (Capability::DataTrust, "Data trust", Unlock::Plugin("datatrust")),
    (Capability::CheckoutTools, "The repository's own tools", Unlock::DevCheckout),
    (Capability::ReplayReceipts, "Replay a receipt's number", Unlock::Plugin(alelyon_plugin_api::REPLAY)),
    (Capability::SinaiMind, "Sinai's mind", Unlock::Plugin(alelyon_plugin_api::SINAI_MIND)),
    (Capability::OwnModel, "Bring your own model (a GGUF file or an OpenAI-compatible endpoint)", Unlock::Always),
    (Capability::StartEars, "Start the speech engine", Unlock::Always),
    (Capability::VoiceEnrolment, "Voice enrolment", Unlock::Plugin(alelyon_plugin_api::VOICE)),
    (Capability::SimulatorScenes, "The simulator's scenes", Unlock::DevCheckout),
];

/// What unlocks `capability`.
pub fn unlock(capability: Capability) -> Unlock {
    CAPABILITIES.iter().find(|(c, _, _)| *c == capability).map(|(_, _, u)| *u).unwrap_or(Unlock::Always)
}

/// The name a person reads for `capability`.
pub fn name(capability: Capability) -> &'static str {
    CAPABILITIES.iter().find(|(c, _, _)| *c == capability).map(|(_, n, _)| *n).unwrap_or("This part of Alelyon")
}

/// How a hosted feature is had: the words every page uses for it.
pub const LIVE_PACKAGE: &str = "Access to that is gained via installing the live package.";

/// What a page shows in place of a capability this build, or this person, does not have: what it is, why it is not
/// here, and how to get it. It is a calm empty state (`ui::absent`), never an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Absent {
    pub what: &'static str,
    pub why: &'static str,
    pub how: &'static str,
}

/// The empty state for `capability`, worded by what unlocks it: a hosted feature names the live package; a part a
/// plug-in brings is not in this build; a part of developing Alelyon itself needs its source checkout.
pub fn absent(capability: Capability) -> Absent {
    let what = name(capability);
    match unlock(capability) {
        Unlock::Always => Absent { what, why: "Every build of Alelyon has this.", how: "" },
        Unlock::SignedIn => Absent {
            what,
            why: "This is a hosted feature: it runs on Alelyon's servers, for a signed-in account.",
            how: LIVE_PACKAGE,
        },
        Unlock::Plugin(_) => Absent {
            what,
            why: "It is not in this build: it is one of the parts that come only with the official Alelyon app.",
            how: "Install the official Alelyon app to use it.",
        },
        Unlock::DevCheckout => Absent {
            what,
            why: "It is not in this build: it reads Alelyon's own source repository, and this copy does not run from a \
                  checkout of it.",
            how: "Run Alelyon from a checkout of its source to see it.",
        },
    }
}

/// The Fleet page in a build that does not carry it (its page is a plug-in of the official app), wherever the build
/// runs from.
pub const FLEET_ABSENT: Absent = Absent {
    what: "Fleet",
    why: "Fleet comes only with the official Alelyon app.",
    how: "Install the official Alelyon app to use it.",
};

/// What the window knows when it asks: whether a person is signed in, the plug-ins this build carries, and whether
/// it runs from a checkout.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    pub signed_in: bool,
    pub plugins: &'a [&'static str],
    pub dev_checkout: bool,
}

/// Whether `capability` is available in `context`.
pub fn available(capability: Capability, context: &Context<'_>) -> bool {
    match unlock(capability) {
        Unlock::Always => true,
        Unlock::SignedIn => context.signed_in,
        Unlock::Plugin(id) => context.plugins.contains(&id),
        Unlock::DevCheckout => context.dev_checkout,
    }
}

/// Whether a registered plug-in's section is shown: when a capability names it, as that capability allows; a plug-in
/// no capability names is shown because the build registered it.
pub fn plugin_shown(id: &str, context: &Context<'_>) -> bool {
    CAPABILITIES.iter().find(|(_, _, u)| matches!(u, Unlock::Plugin(p) if *p == id)).is_none_or(|(c, _, _)| available(*c, context))
}

/// The ids of the plug-ins in `registry`, pages and what is not a page alike, for a [`Context`].
pub fn plugin_ids(registry: &Registry) -> Vec<&'static str> {
    registry.ids()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_capability_is_listed_once_and_unlocked_by_what_the_list_says() {
        for (c, _, _) in CAPABILITIES {
            assert_eq!(CAPABILITIES.iter().filter(|(d, _, _)| *d == c).count(), 1, "{c:?} is listed once");
        }
        let public = Context { signed_in: false, plugins: &[], dev_checkout: false };
        assert!(available(Capability::VerifyReceipts, &public));
        assert!(available(Capability::OwnModel, &public), "a model of your own needs no plug-in, no account and no checkout");
        assert!(available(Capability::StartEars, &public), "the speech engine is open: every build can start it");
        for c in [
            Capability::Friends,
            Capability::ComputeEmulator,
            Capability::ComputeKitJobs,
            Capability::IssueReceipts,
            Capability::DataTrust,
            Capability::ReplayReceipts,
            Capability::SinaiMind,
            Capability::VoiceEnrolment,
            Capability::SimulatorScenes,
        ] {
            assert!(!available(c, &public), "{c:?} is not in the public build signed out");
        }
        let signed_in = Context { signed_in: true, ..public };
        assert!(available(Capability::Friends, &signed_in) && !available(Capability::IssueReceipts, &signed_in));
        let owner = Context { signed_in: false, plugins: &["emulator", "kit", "issue", "datatrust"], dev_checkout: true };
        for c in [
            Capability::ComputeEmulator,
            Capability::ComputeKitJobs,
            Capability::IssueReceipts,
            Capability::DataTrust,
            Capability::CheckoutTools,
        ] {
            assert!(available(c, &owner), "{c:?} comes with its plug-in or the checkout");
        }
        assert!(!available(Capability::Friends, &owner), "a plug-in does not sign anyone in");
        assert!(plugin_shown("emulator", &owner) && plugin_shown("anything else", &owner));
        let official = Context { signed_in: false, plugins: &["replay", "sinai-mind", "voice"], dev_checkout: false };
        for c in [Capability::ReplayReceipts, Capability::SinaiMind, Capability::StartEars, Capability::VoiceEnrolment] {
            assert!(available(c, &official), "{c:?} comes with the official build's plug-in");
        }
        assert!(plugin_ids(&Registry::default()).is_empty(), "the public build carries no plug-in id");
    }

    #[test]
    fn every_absent_capability_says_what_it_is_why_it_is_not_here_and_how_to_get_it() {
        for (c, title, unlock) in CAPABILITIES {
            let a = absent(c);
            assert_eq!(a.what, title, "{c:?} is named as the list names it");
            if unlock == Unlock::Always {
                continue;
            }
            assert!(!a.why.is_empty() && !a.how.is_empty(), "{c:?} says why it is absent and how to get it");
            for word in ["error", "failed", "could not", "unavailable"] {
                assert!(!a.why.to_lowercase().contains(word), "{c:?}'s empty state reads as an error: {}", a.why);
            }
        }
        // a hosted feature names the live package, and only the live package
        let friends = absent(Capability::Friends);
        assert_eq!(friends.how, "Access to that is gained via installing the live package.");
        assert!(friends.why.contains("hosted"));
        // a closed part is not in this build; so is a part of developing Alelyon itself
        for c in [Capability::ComputeEmulator, Capability::IssueReceipts, Capability::SinaiMind, Capability::CheckoutTools, Capability::SimulatorScenes] {
            assert!(absent(c).why.starts_with("It is not in this build"), "{c:?}");
        }
        assert_eq!(name(Capability::SimulatorScenes), "The simulator's scenes");
        // a build without the Fleet page says where it comes from, run from a checkout or not
        assert_eq!(FLEET_ABSENT.why, "Fleet comes only with the official Alelyon app.");
        assert!(!FLEET_ABSENT.why.contains("checkout") && !FLEET_ABSENT.how.is_empty());
    }
}
