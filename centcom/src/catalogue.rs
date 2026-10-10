//! Every section of CENTCOM and every feature the project has, where it lives today and when it moves in.
//!
//! The list is the CENTCOM plan of 2026-10-03 (44 features in seven sections, Markets left out), with the Data
//! section added on 2026-10-04 (the databases in a user-friendly form), so no feature is forgotten while the window
//! grows: a section that is not wired yet still shows what belongs in it and where to find it meanwhile.
//!
//! A build's plug-ins bring their own rows (`alelyon_plugin_api::Feature`), listed on their section's page with these;
//! the public build lists only the rows below.

pub use alelyon_plugin_api::Here;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Overview,
    Sinai,
    Transcription,
    Lattice,
    Fleet,
    Data,
    Research,
    Trust,
    Compute,
    Pages,
    Account,
}

impl Section {
    pub const ALL: [Section; 11] = [
        Section::Overview,
        Section::Sinai,
        Section::Transcription,
        Section::Lattice,
        Section::Fleet,
        Section::Data,
        Section::Research,
        Section::Trust,
        Section::Compute,
        Section::Pages,
        Section::Account,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Overview => "Overview",
            Section::Sinai => "Sinai",
            Section::Transcription => "Transcription",
            Section::Lattice => "Lattice",
            Section::Fleet => "Fleet and Claw",
            Section::Data => "Data",
            Section::Research => "Research",
            Section::Trust => "Trust",
            Section::Compute => "Compute and simulation",
            Section::Pages => "Pages",
            Section::Account => "Account and platform",
        }
    }

    /// The rail's label: one short word.
    pub fn short(self) -> &'static str {
        match self {
            Section::Overview => "Overview",
            Section::Sinai => "Sinai",
            Section::Transcription => "Words",
            Section::Lattice => "Lattice",
            Section::Fleet => "Fleet",
            Section::Data => "Data",
            Section::Research => "Research",
            Section::Trust => "Trust",
            Section::Compute => "Compute",
            Section::Pages => "Pages",
            Section::Account => "Account",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Section::Overview => "\u{25C9}",
            Section::Sinai => "\u{2726}",
            Section::Transcription => "\u{2261}",
            Section::Lattice => "\u{25C7}",
            Section::Fleet => "\u{2691}",
            Section::Data => "\u{25A4}",
            Section::Research => "\u{2315}",
            Section::Trust => "\u{2713}",
            Section::Compute => "\u{2699}",
            Section::Pages => "\u{25CE}",
            Section::Account => "\u{25D0}",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Section::Overview => "Everything Project Angel runs on this computer, at a glance.",
            Section::Sinai => "Talk with Sinai, see what it sees and does, and decide what it may do.",
            Section::Transcription => {
                "Live captions, dictation, the computer's own audio and audio files, turned into words on this PC."
            }
            Section::Lattice => "Chat, agents, code and traces, and the models behind them.",
            Section::Fleet => "The agent sessions working on the project, the graphics card's queue, and the harness.",
            Section::Data => "The project's databases, read-only: the fleet's bus and relay, the ledgers, Lattice's workspaces, and Sinai's memory.",
            Section::Research => "Every paper on a subject: searched, then followed through citations until nothing new turns up.",
            Section::Trust => "Check certified receipts, and the integrity of the data behind them.",
            Section::Compute => "The simulator Sinai will train in, and training runs.",
            Section::Pages => "Your public page, organizations with their tickers, titles they confirm, and posts.",
            Section::Account => "Sign-in, teams, updates, keys and billing.",
        }
    }

    /// The plan page's id for the section's features.
    fn key(self) -> &'static str {
        match self {
            Section::Overview => "",
            Section::Sinai => "sinai",
            Section::Transcription => "ears",
            Section::Lattice => "lattice",
            Section::Fleet => "fleet",
            Section::Data => "data",
            Section::Research => "research",
            Section::Trust => "trust",
            Section::Compute => "compute",
            Section::Pages => "pages",
            Section::Account => "account",
        }
    }

    pub fn features(self) -> impl Iterator<Item = &'static Feature> {
        FEATURES.iter().filter(move |f| f.section == self.key())
    }

    /// The section's rows with those `extra` brings (a build's plug-ins): each extra row before the row it names, or
    /// last.
    pub fn features_with<'a>(self, extra: &'a [Feature]) -> Vec<&'a Feature> {
        let mut rows: Vec<&'a Feature> = self.features().collect();
        for row in extra.iter().filter(|f| f.section == self.key()) {
            let at = row.before.and_then(|id| rows.iter().position(|f| f.id == id)).unwrap_or(rows.len());
            rows.insert(at, row);
        }
        rows
    }
}

/// One feature, as the plan lists it.
#[derive(Debug)]
pub struct Feature {
    pub id: &'static str,
    section: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    /// Where it is reached today.
    pub today: &'static str,
    /// Reached today only from a command line, a script, settings in the environment, or not at all.
    pub no_screen: bool,
    pub plan: &'static str,
    /// The build step it arrives in (the plan's 1-6, or "Later").
    pub step: &'static str,
    /// How far a plug-in's row has moved in, as the plug-in says; a built-in row's is below.
    declared: Option<Here>,
    /// The row a plug-in's row is listed before.
    before: Option<&'static str>,
}

impl Feature {
    /// A plug-in's row, as the catalogue lists it.
    pub fn from_plugin(row: &alelyon_plugin_api::Feature) -> Feature {
        Feature {
            id: row.id,
            section: row.section,
            name: row.name,
            what: row.what,
            today: row.today,
            no_screen: row.no_screen,
            plan: row.plan,
            step: row.step,
            declared: Some(row.here),
            before: row.before,
        }
    }

    pub fn here(&self) -> Here {
        if let Some(here) = self.declared {
            return here;
        }
        match self.id {
            "ears-captions" | "ears-dictation" | "ears-pc" | "ears-files" | "ears-library" | "sinai-listening"
            | "trust-verify" => Here::Now,
            // The queue with its holders, takeovers and what waits; every listed database, read-only.
            // The sessions, their takeovers and claims, and every worktree (what it holds, whether it is in main, when it
            // last moved), read without looking inside any worktree's files.
            "fleet-queue" | "fleet-sessions" | "data-browse" => Here::Now,
            // The simulator, on the processor: watched as it runs.
            "compute-sim" => Here::Now,
            // A trainer's metrics stream read as it grows by the Training Studio's rules (no checkpoint index exists to
            // show).
            "compute-train" => Here::Now,
            // Waiting on you: allow, refuse, or move a held click and allow it there (the one change the loop carries
            // out, as in the Angel window), with each act's countdown and what became of it. Actions: the journal by
            // status and by kind, with what became of each act. Grants: read, and edited here (each save reviewed for
            // what it adds and reduces, never written while Sinai's hands are armed).
            "sinai-waiting" | "sinai-actions" | "sinai-grants" => Here::Now,
            // Stage: the window Sinai's hands are armed on and the windows a person pins, pictured by this window itself
            // while the Stage is on show (PrintWindow, as the Angel window does); never Alelyon's own window.
            "sinai-stage" => Here::Now,
            // Sinai's face and world: the Angel window's bust, moved by the loop, in its valley under the real sun, drawn
            // by the graphics card while it is on show (a still placeholder without one).
            "sinai-face" => Here::Now,
            // The conversation: what Sinai hears as it is said, and each line, but Sinai's own words arrive whole (the
            // loop sends a sentence at a time, with no word timings).
            "sinai-converse" => Here::InPart,
            // The Machine: its memory with the Angel window's acts, each confirmed; how it listens; its memory map drawn;
            // the observatory (tiers 1 to 3 and the last turn); and its eyes (sight open or closed, its looks and what
            // the newest found). Its hands and the gate are the Stage, Waiting on you and Actions docks.
            "sinai-machine" => Here::Now,
            // Voice enrolment: the voiceprint and its registered test shown; re-enrolled through the probe's own two
            // scripts (the sentences recorded here, one press each), or erased (asked twice; moved aside, not deleted).
            "sinai-voiceprint" => Here::Now,
            // Signing in: the screen before the window (email or username and password, the seven providers through the
            // browser, QR pairing, Stay signed in sealed with DPAPI, the service's notices, Use offline) and the account
            // menu; the identity service it talks to is not deployed yet.
            "account-signin" => Here::InPart,
            // Lattice: the agent chat (tools, approvals, questions, diff review before any write) over the native chat
            // core, as a Cursor-style Chat and IDE since 2026-10-07; agent runs started and watched with their traces as
            // a waterfall. Code: the IDE's Explorer (the folder's listing as a tree), editor tabs with syntax colours, the
            // person's own saves (guarded), quick open, a command's whole output, and
            // the agent's changes reviewed as diffs, kept or undone by hunk or file.
            "lattice-chat" | "lattice-agents" | "lattice-code" | "lattice-traces" => Here::Now,
            // Stack: what this build can reach (the linked verifier, the vectors, ACCS, the linked C++ morphometry engine
            // with its canonical space and commitment) and the standing gaps.
            "lattice-stack" => Here::Now,
            // Morphometry: the Python desktop's four views (Model Morphometry, Lineage Morphometry, Template Hierarchy,
            // Registration's transform families) for an installed GGUF model, computed by the C++ port, and
            // two models compared on the canonical frame (morphometry_compare), and probed past the
            // header: weight statistics per cell and one forward
            // pass of a prompt on the pinned llama.cpp build (probe/, its activations and its experts' choices). In
            // part: registration is the reachable identity / axis-permutation subset, and this window issues no
            // registration certificate.
            "lattice-morpho" => Here::InPart,
            // Foundry: the Python desktop's Model Foundry view (fit, footprint, co-resident pairs) for the installed GGUF
            // models against this machine, computed by the C++ port over the native probe (DXGI only,
            // the adapter VK_LOADER_DEVICE_ID_FILTER names, its free memory from its GPU Adapter Memory counter);
            // benchmarks measured on this PC (lattice-core llama::bench, its own --fit on server) with the CPU
            // package's energy; and running costs (the economics model) against declared electricity and
            // hosted prices. The Python view's Ollama listing is the GGUF folder here.
            "lattice-foundry" => Here::Now,
            // Training Studio: everything the Python Studio does, in its format: a workspace created, opened and
            // verified, its document pool edited (each document's normalised text, by SHA-256, in append-only
            // revisions), plans saved and chosen again, and a document snapshot exported. No trainer is launched
            // anywhere; a trainer's live metrics are on the Compute page.
            "lattice-training" => Here::Now,
            // Action Gate: the agents' ledger with its chain re-derived and each session's obligations replayed from it;
            // the intent ledger lives in fam.db (Markets), which is left out.
            "lattice-gate" => Here::InPart,
            // Research: subjects gathered from OpenAlex and arXiv and followed through citations, each paper with its
            // reason, and the OpenAlex key (the free key's page one click away, kept in Credential Manager, today's
            // budget). Not yet: re-gathering on a schedule with what is new, and the archive as a tool for models.
            "research-archive" => Here::InPart,
            // Models: the chat's choices, the local server with the GGUF model it runs (chosen here as the Python model
            // bar chooses it), and the endpoint registry, edited here with keys kept in Credential Manager.
            "lattice-models" => Here::Now,
            // Pages: a person's public page with a title and the tickers they choose, organizations (members, roles,
            // confirmed titles), posts, articles, replies, reposts, likes, follows, blocks and reports, over the
            // identity service's pages routes. In part: that service does not offer them yet, and the website's pages
            // are to come.
            "pages-profile" | "pages-orgs" | "pages-posts" => Here::InPart,
            _ => Here::Later,
        }
    }
}

const fn f(
    id: &'static str,
    section: &'static str,
    name: &'static str,
    what: &'static str,
    today: &'static str,
    no_screen: bool,
    plan: &'static str,
    step: &'static str,
) -> Feature {
    Feature { id, section, name, what, today, no_screen, plan, step, declared: None, before: None }
}

pub const FEATURES: &[Feature] = &[
    f("sinai-converse", "sinai", "Conversation with live captions", "Talk or type to Sinai; both sides appear word by word as they are spoken.", "Angel window: the last 4 lines and a \"hearing\" line", false, "Port, with new captions", "2"),
    f("sinai-face", "sinai", "Sinai's face and world", "The valley, Sinai's bust and expressions, and the Appearance creator.", "Angel window", false, "Port", "2"),
    f("sinai-stage", "sinai", "Stage", "Live tiles of the windows Sinai is looking at, with a chooser.", "Angel window", false, "Port", "2"),
    f("sinai-actions", "sinai", "Actions", "A journal of what Sinai planned, is doing and has done.", "Angel window", false, "Port", "2"),
    f("sinai-waiting", "sinai", "Waiting on you", "Held actions you allow, refuse or change with a click.", "Angel window", false, "Port", "2"),
    f("sinai-grants", "sinai", "Grants", "What Sinai may do on this computer, with a friendlier editor than raw JSON.", "Angel window (a JSON editor)", false, "Port, friendlier", "2"),
    f("sinai-listening", "sinai", "Listening state and on-air light", "Shows when the microphone is open: waiting for \"Sinai\", or in a conversation.", "Angel window, in part", false, "Port, clearer", "2"),
    f("sinai-machine", "sinai", "The Machine", "Sinai's memory (remember, inspect, erase, restore), its memory map, the observatory, hands and eyes, and the gate.", "Angel window", false, "Port", "3"),
    f("sinai-settings", "sinai", "Sinai's behaviour settings", "Tasks, web reading, voice authority, the follow-up window, weather and latitude.", "No screen: environment variables", true, "New", "3"),
    f("sinai-voiceprint", "sinai", "Voice enrolment", "Record your voiceprint, so spoken commands that grant authority work only in your voice.", "No screen: a script", true, "New", "3"),
    f("sinai-service", "sinai", "Sinai service settings", "The You and Organization settings of the shared Sinai service.", "A web settings page", false, "Wire in", "4"),
    f("ears-captions", "ears", "Live captions", "Your microphone transcribed as you speak, with settled words apart from words still settling.", "No screen", true, "New", "2"),
    f("ears-dictation", "ears", "Dictation", "Speak into any text box in the app instead of typing.", "No screen", true, "New", "2"),
    f("ears-pc", "ears", "PC audio", "Transcribe a meeting, call or video playing on this computer, with an on-air light while it runs.", "No screen", true, "New", "2"),
    f("ears-files", "ears", "Audio files", "Drop in a recording (WAV, MP3, M4A, WMA, FLAC), watch its progress, export text or subtitles.", "No screen", true, "New: command line first", "1"),
    f("ears-library", "ears", "Transcript library", "Find, copy, export and delete saved transcripts.", "No screen", true, "New", "3"),
    f("ears-speakers", "ears", "Who said what", "Speaker labels on transcripts with more than one voice.", "No screen (only your voiceprint check exists)", true, "New", "Later"),
    f("lattice-chat", "lattice", "Chat", "Cursor-like chat with tools and context, and a diff review before any file is written.", "The chat core with no screen of its own; web Lattice and the Python Conversation view", false, "New, with the chat-core session", "4"),
    f("lattice-agents", "lattice", "Agents", "Start and watch coding-agent runs.", "Web Lattice; Python AI IDE", false, "Port", "4"),
    f("lattice-code", "lattice", "Code", "The editor side of the AI IDE.", "Web Lattice; Python AI IDE", false, "Port", "4"),
    f("lattice-traces", "lattice", "Traces", "The waterfall of every agent run.", "Native Lattice (already iced)", false, "Move in", "4"),
    f("lattice-models", "lattice", "Models and endpoints", "Local llama.cpp models and cloud keys, set up without Python.", "Python desktop", false, "Port", "4"),
    f("lattice-training", "lattice", "Training Studio", "Data preparation, runs and their metrics.", "Python desktop, in part (no launch)", false, "Port", "5"),
    f("lattice-foundry", "lattice", "Foundry", "Model fit, memory footprint, benchmarks and running costs.", "Python desktop for fit and footprint; benchmarks and costs have no screen", false, "Port and new", "5"),
    f("lattice-morpho", "lattice", "Morphometry", "Model-shape comparisons and probes.", "Python desktop (4 views); compare and probe have no screen", false, "Port", "5"),
    f("lattice-stack", "lattice", "Stack", "What is installed: runtimes, the verifier, versions.", "Python desktop", false, "Port", "5"),
    f("lattice-gate", "lattice", "Action Gate and ledgers", "What agents were allowed or refused, and their intent records.", "Python desktop, read-only", false, "Port", "5"),
    f("fleet-sessions", "fleet", "Fleet", "The agent sessions, their worktrees, claims and findings, read-only.", "Python desktop (15 views)", false, "Port", "5"),
    f("fleet-queue", "fleet", "GPU queue", "Who holds the graphics card and who is waiting, live.", "No screen: tools only", true, "New", "5"),
    f("fleet-claw", "fleet", "AlelyonClaw", "The agent harness, in development builds only.", "Python desktop window", false, "Port", "5"),
    f("research-archive", "research", "Research archive", "Every paper on a subject: searched in OpenAlex and arXiv, then followed through citations backward and forward until nothing new turns up, each paper kept with the reason it belongs and an estimate of what is still missing.", "No screen: searching by hand", true, "New (2026-10-08)", "5"),
    f("data-browse", "data", "Databases", "The project's databases, read-only, as tables to browse: the bus, the relay and its receipts, the ledgers, Lattice's workspaces, and Sinai's memory and experience.", "No screen: scripts and the SQLite command line", true, "New (2026-10-04)", "5"),
    f("trust-verify", "trust", "Verify a receipt", "Drop in a certified receipt and its data; see the verdict and the reasons.", "No screen: command line", true, "New", "3"),
    f("trust-recon", "trust", "Reconciliation", "The insurance reconciliation product.", "No screen (catalogued as building)", true, "New", "Later"),
    f("compute-sim", "compute", "Simulator and renderer", "Watch the physics simulator and renderer Sinai will train in.", "No screen", true, "New", "Later"),
    f("compute-train", "compute", "Model training metrics", "Training runs' curves and checkpoints.", "Training Studio metrics", false, "Port", "5"),
    f("pages-profile", "pages", "Your page and title", "A public page with your title, a few words about you, and the organizations you choose to show after your name.", "No screen", true, "New (2026-10-09)", "5"),
    f("pages-orgs", "pages", "Organizations", "Organizations with unique tickers: members, roles, invitations, and titles they confirm.", "No screen", true, "New (2026-10-09)", "5"),
    f("pages-posts", "pages", "Posts and articles", "Post, write articles, reply, repost, quote and like; follow people and organizations; block and report.", "No screen", true, "New (2026-10-09)", "5"),
    f("account-signin", "account", "Sign-in and account", "Sign in, your profile, and hosted-key redemption.", "Python desktop; the website", false, "Port", "4"),
    f("account-teams", "account", "Teams", "Profile, people and friends.", "Python desktop", false, "Port", "4"),
    f("account-tour", "account", "First-run tour", "A short guided start for someone new.", "No screen", true, "New", "4"),
    f("account-updates", "account", "Updates", "Turn automatic updates on or off.", "Installer only", true, "New", "4"),
    f("account-api", "account", "API keys and usage", "Your hosted API keys and how much you have used.", "Operator command line", true, "New", "Later"),
    f("account-billing", "account", "Billing", "Plans and payment. Metering is not wired yet.", "No screen", true, "New", "Later"),
    f("account-ops", "account", "Operations", "Cloud service status, for operator editions.", "Inside the Markets app", false, "Move in", "5"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_has_46_features_in_ten_sections_and_no_markets() {
        assert_eq!(FEATURES.len(), 46);
        let counted: usize = Section::ALL.iter().map(|s| s.features().count()).sum();
        assert_eq!(counted, 46, "every feature belongs to one section");
        assert_eq!(Section::ALL.iter().filter(|s| s.features().count() > 0).count(), 10);
        assert_eq!(Section::Overview.features().count(), 0);
        assert!(FEATURES.iter().all(|f| !f.name.to_lowercase().contains("market")), "Markets stay out");
        let mut ids: Vec<&str> = FEATURES.iter().map(|f| f.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 46, "ids are unique");
    }

    #[test]
    fn what_is_wired_now_is_transcription_and_sinais_listening_state() {
        let now: Vec<&str> = FEATURES.iter().filter(|f| f.here() == Here::Now).map(|f| f.id).collect();
        assert_eq!(
            now,
            [
                "sinai-face",
                "sinai-stage",
                "sinai-actions",
                "sinai-waiting",
                "sinai-grants",
                "sinai-listening",
                "sinai-machine",
                "sinai-voiceprint",
                "ears-captions",
                "ears-dictation",
                "ears-pc",
                "ears-files",
                "ears-library",
                "lattice-chat",
                "lattice-agents",
                "lattice-code",
                "lattice-traces",
                "lattice-models",
                "lattice-training",
                "lattice-foundry",
                "lattice-stack",
                "fleet-sessions",
                "fleet-queue",
                "data-browse",
                "trust-verify",
                "compute-sim",
                "compute-train"
            ]
        );
        let part: Vec<&str> = FEATURES.iter().filter(|f| f.here() == Here::InPart).map(|f| f.id).collect();
        assert_eq!(
            part,
            [
                "sinai-converse",
                "lattice-morpho",
                "lattice-gate",
                "research-archive",
                "pages-profile",
                "pages-orgs",
                "pages-posts",
                "account-signin"
            ]
        );
    }

    #[test]
    fn a_plugins_row_is_listed_before_the_row_it_names_with_the_progress_it_declares() {
        let row = |id, before| {
            Feature::from_plugin(&alelyon_plugin_api::Feature {
                id,
                section: "trust",
                name: "A plug-in's feature",
                what: "",
                today: "",
                no_screen: false,
                plan: "",
                step: "5",
                here: Here::InPart,
                before,
            })
        };
        let extra = [row("trust-a", Some("trust-recon")), row("trust-b", Some("trust-recon")), row("trust-c", None)];
        let ids: Vec<&str> = Section::Trust.features_with(&extra).iter().map(|f| f.id).collect();
        assert_eq!(ids, ["trust-verify", "trust-a", "trust-b", "trust-recon", "trust-c"]);
        assert_eq!(extra[0].here(), Here::InPart);
        assert_eq!(Section::Compute.features_with(&extra).len(), Section::Compute.features().count(), "rows stay in their section");
    }
}
