//! CENTCOM's state, its messages, and what each one does.
//!
//! The compute budget is the native Lattice window's ("rounding error"): iced draws a frame only when a
//! message changes something, and every timer here is a subscription held only while its page or its work
//! needs it. Idle, with nothing loading, the window holds one thing: the ears' link, a thread that waits on
//! its socket.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{pane_grid, text_editor};
use iced::{Subscription, Task, event, window};

use crate::capability::{self, Capability};
use crate::catalogue::Section;
use alelyon_plugin_api::{self as plugin, FLEET, Registry, Slot};
use crate::appearance;
use crate::dock;
use crate::compute;
use crate::lattice;
use crate::data;
use crate::research;
use crate::ears::{self, Feed};
use crate::face;
use crate::grants;
use crate::library;
use crate::probe;
use crate::sinai;
use crate::spinner;
use crate::stage;
use crate::trust;

/// The ears' link, as the window shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    /// The first look, at start.
    Connecting,
    Unavailable(String),
    Connected,
    /// Broken; the link is looking again.
    Lost(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Captions,
    Dictation,
    Pc,
    Files,
    Library,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Captions, Tab::Dictation, Tab::Pc, Tab::Files, Tab::Library];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Captions => "Live captions",
            Tab::Dictation => "Dictation",
            Tab::Pc => "PC audio",
            Tab::Files => "Audio files",
            Tab::Library => "Library",
        }
    }

    fn from_name(name: &str) -> Option<Tab> {
        Some(match name {
            "captions" => Tab::Captions,
            "dictation" => Tab::Dictation,
            "pc" => Tab::Pc,
            "files" => Tab::Files,
            "library" => Tab::Library,
            _ => return None,
        })
    }
}

/// Words as they arrive: the settled lines, and the line still being spoken.
#[derive(Debug, Default)]
pub struct Captions {
    pub lines: Vec<(f64, String)>,
    pub stable: String,
    pub settling: String,
    /// Speech has started and its first words are not in yet.
    pub hearing: bool,
}

impl Captions {
    /// Lines kept on screen; older ones scroll away (nothing is stored unless someone copies it).
    const KEEP: usize = 400;

    fn heard(&mut self) {
        self.hearing = true;
    }

    fn partial(&mut self, stable: &str, settling: &str) {
        self.stable = stable.to_string();
        self.settling = settling.to_string();
        self.hearing = false;
    }

    fn settled(&mut self, start: f64, text: &str) {
        self.lines.push((start, text.trim().to_string()));
        if self.lines.len() > Self::KEEP {
            self.lines.drain(..self.lines.len() - Self::KEEP);
        }
        self.dropped();
    }

    fn dropped(&mut self) {
        self.stable.clear();
        self.settling.clear();
        self.hearing = false;
    }

    pub fn text(&self) -> String {
        self.lines.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n")
    }

    /// The lines with the time each began, as the library keeps them.
    pub fn timed(&self) -> String {
        self.lines.iter().map(|(start, t)| format!("[{}] {t}", clock(*start))).collect::<Vec<_>>().join("\n")
    }

    pub fn in_progress(&self) -> bool {
        self.hearing || !self.stable.is_empty() || !self.settling.is_empty()
    }
}

/// Seconds as m:ss.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Export formats for audio files, in the order the page shows them.
pub const FORMATS: [&str; 4] = ["txt", "srt", "vtt", "json"];

/// How CENTCOM was started: which page to open, and whether to photograph it and exit (the proofs).
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub section: Option<Section>,
    pub tab: Option<Tab>,
    /// A Fleet tab (`queue`, `sessions`, `worktrees`, `landing`, `bus`, `machine`), or a Data store with one of its tables
    /// (`relay`, `relay/receipt`), for the page `--section` opens.
    pub page: Option<String>,
    /// A receipt or test case to open on the Trust page and check at once.
    pub verify: Option<PathBuf>,
    /// A folder to open in the Lattice IDE, and a file of it (relative to the folder) to open in its editor.
    pub folder: Option<PathBuf>,
    pub file: Option<String>,
    pub screenshot: Option<PathBuf>,
    pub after_ms: u64,
    /// Show the sign-in screen even in a `--screenshot` run (which otherwise opens straight onto the page, offline).
    pub sign_in: bool,
}

pub const USAGE: &str = "usage: centcom [--section overview|sinai|words|lattice|fleet|data|research|trust|compute|account] \
[--tab captions|dictation|pc|files|library|queue|sessions|landing|bus|machine|simulator|training|<a plug-in's tab>|<scene>|chat|runs|models|morphometry|foundry|<store>[/<table>]|appearance|brain] [--verify <receipt.json>] [--folder <folder> [--file <path in it>]] [--screenshot <file.png> [--after-ms N]] [--sign-in]";

impl Options {
    #[cfg(test)]
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        Options::parse_with(args, &Registry::default())
    }

    /// The command line, with the Compute tabs `registry`'s plug-ins add.
    pub fn parse_with(args: impl IntoIterator<Item = String>, registry: &Registry) -> Result<Options, String> {
        let mut options = Options { after_ms: 2500, ..Options::default() };
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--section" => {
                    let name = value()?;
                    options.section = Some(
                        Section::ALL
                            .into_iter()
                            .find(|s| s.short().eq_ignore_ascii_case(&name) || s.title().eq_ignore_ascii_case(&name))
                            .ok_or_else(|| format!("no section {name:?}"))?,
                    );
                }
                "--tab" => {
                    let name = value()?;
                    match Tab::from_name(&name) {
                        Some(tab) => options.tab = Some(tab),
                        None if registry.in_slot(Slot::SectionPage).any(|p| p.tabs.contains(&name.as_str()))
                            || data::stores::named(&name).is_some()
                            || compute::Tab::from_name(&name).is_some()
                            || registry.in_slot(Slot::ComputeTab).any(|p| p.id == name)
                            || compute::scene_named(&name).is_some()
                            || lattice::Tab::from_name(&name).is_some()
                            || name == "appearance"
                            || name == "brain"
                            || name == "map"
                            || name == "gaps"
                            || name == "papers" =>
                        {
                            options.page = Some(name)
                        }
                        None => return Err(format!("no tab {name:?}")),
                    }
                }
                "--verify" => options.verify = Some(PathBuf::from(value()?)),
                "--folder" => options.folder = Some(PathBuf::from(value()?)),
                "--file" => options.file = Some(value()?.replace('\\', "/")),
                "--screenshot" => options.screenshot = Some(PathBuf::from(value()?)),
                "--sign-in" => options.sign_in = true,
                "--after-ms" => options.after_ms = value()?.parse().map_err(|e| format!("--after-ms: {e}"))?,
                "--help" | "-h" => return Err(USAGE.to_string()),
                other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
            }
        }
        Ok(options)
    }
}

pub struct App {
    pub section: Section,
    pub tab: Tab,
    /// The spinners' angle.
    pub phase: f32,
    pub link: Link,
    commands: Option<ears::Commands>,
    engine: Option<ears::Engine>,
    /// Why the engine could not start, or how it ended.
    pub engine_note: Option<String>,
    /// The engine was started here and has not answered yet.
    pub starting: bool,
    pub state: Option<ears::State>,
    pub captions: Captions,
    pub pc: Captions,
    pub dictation: text_editor::Content,
    pub dictation_partial: String,
    pub file_path: String,
    pub formats: [bool; 4],
    pub jobs: Vec<ears::Job>,
    /// The last thing that went wrong, until it is dismissed.
    pub notice: Option<String>,
    pub probes: Vec<probe::Reading>,
    /// When the services last changed, as `probe::watch` said (it says only a change).
    pub services_fresh: crate::live::Freshness,
    /// When the voiceprint's folder last changed, as its watch said.
    pub voice_fresh: crate::live::Freshness,
    /// A file is being dragged over the window.
    pub hovering: bool,
    /// Sinai's loop: connected, or not (and why the page says so).
    pub sinai_up: Option<bool>,
    sinai_commands: Option<sinai::Commands>,
    /// Sinai's loop said it works research gaps itself (`abilities`); an older loop gets the gap as typed words.
    sinai_works_gaps: bool,
    pub conversation: sinai::Conversation,
    /// A held click being moved before it is allowed: its id, and the x and y as typed.
    pub held_change: Option<(String, String, String)>,
    /// Which of Sinai's journal rows the Actions view shows: by status, and by kind of act (None: every kind).
    pub journal_status: sinai::Status,
    pub journal_kind: Option<String>,
    /// Grants being edited: the draft read from the loop's file; its review (the list to write and what it changes,
    /// or what is wrong with it); and what the last attempt to save did.
    pub grants_draft: Option<grants::Draft>,
    pub grants_review: Option<Result<(Vec<serde_json::Value>, grants::Changes), Vec<String>>>,
    pub grants_note: Option<String>,
    /// How Sinai listens, from its loop's /about (`machine`), and when it was last asked: read when the loop connects,
    /// then at most once a minute while the Sinai page is open.
    pub listening: Option<Result<crate::machine::Listening, String>>,
    listening_at: f64,
    listening_asked: bool,
    /// An act on Sinai's memory waiting for its confirmation (`machine::Pending`), the windows offered to observe while
    /// the chooser is open, and what the last act came to when it could not be sent.
    pub memory_pending: Option<crate::machine::Pending>,
    pub observe_windows: Option<Vec<stage::Window>>,
    pub memory_note: Option<String>,
    /// Voice enrolment (`voice`): the enrolment as its files say, and a re-enrolment or an erase under way.
    pub voice: crate::voice::State,
    /// Signing in: the screen before the main window, the session, and the account menu (`signin`).
    pub signin: crate::signin::State,
    /// Friends, presence and chat, while signed in.
    pub social: crate::social::State,
    /// Pages: public profiles, organizations and posts, while signed in.
    pub pages: crate::pages::State,
    /// The window (for the title bar's drag, buttons and edges), whether it is maximised, and the bar's last press.
    pub window_id: Option<window::Id>,
    pub maximised: bool,
    pub bar_pressed: Option<std::time::Instant>,
    /// The company mark, made once, for the sign-in screen.
    pub mark: Option<iced::widget::image::Handle>,
    /// Alelyon's own window (its HWND), once the window system has said: never captured, and named to the loop so
    /// Sinai never observes it either.
    pub own_window: Option<u64>,
    /// The Stage: the windows the person pinned, whether the chooser is open, the windows it offers, the latest
    /// picture of each window on show (with the share of it that is lit), and the windows that would not draw.
    pub stage_pinned: Vec<i64>,
    pub stage_choosing: bool,
    pub stage_windows: Vec<stage::Window>,
    pub stage_shots: std::collections::HashMap<i64, (iced::widget::image::Handle, f32)>,
    pub stage_gone: Vec<i64>,
    /// The loop has been told which window this is, on this connection.
    watch_sent: bool,
    /// Sinai's face: its animation, which runs only while it is on show, and what the graphics card made of it.
    pub face: face::Face,
    /// The measured brain last put inside Sinai's glass head, so it goes in once per measurement.
    sinai_brain: Option<Arc<lattice::brain::Brain>>,
    /// Sinai's appearance as the Angel window keeps it (with the looks a person kept), and where that file is.
    pub appearance: appearance::Saved,
    appearance_path: Option<PathBuf>,
    /// The Appearance creator, while it is open: it takes the Sinai page.
    pub creator: Option<face::creator::Creator>,
    /// What the person is typing to Sinai.
    pub typed: String,
    /// Dictation goes into the box for Sinai rather than the Dictation page's text.
    pub dictating_to_sinai: bool,
    /// The transcript library, newest first, as last read.
    pub library: Vec<library::Entry>,
    pub library_loading: bool,
    /// When the library's folder last changed, as its watch said.
    pub library_fresh: crate::live::Freshness,
    pub library_query: String,
    /// Transcripts in the library's Deleted folder.
    pub library_deleted: usize,
    /// "Empty Deleted" was pressed once: the page asks before anything is removed for good.
    pub library_confirm_empty: bool,
    /// What was last kept, said where it was kept.
    pub kept: Option<String>,
    /// Trust: the receipt, its data and the keys a person pinned, and the last verdict.
    pub trust_receipt: String,
    pub trust_data: String,
    pub trust_key: String,
    pub trust_witness: String,
    pub trust_checking: bool,
    pub trust_result: Option<Result<(trust::Verdict, trust::Stated), String>>,
    /// Counts changes to the inputs: a verdict is shown only for the inputs it was asked about.
    pub trust_asked: u64,
    /// The Trust page's cards the build's plug-ins add, after Verify.
    pub trust_cards: Vec<compute::Mounted>,
    /// The plug-ins this build carries, whether it runs from a checkout (capability.rs asks), and the catalogue rows
    /// the plug-ins bring.
    plugin_ids: Vec<&'static str>,
    dev_checkout: bool,
    pub plugin_features: Vec<crate::catalogue::Feature>,
    /// Sinai's views, docked as the person last arranged them.
    pub dock: pane_grid::State<dock::Tabs>,
    /// The layout changed and is written a second later (a resize sends many changes).
    dock_changed: bool,
    /// The Fleet page, in a build whose registry carries it (a section plug-in named `fleet`); None shows its empty
    /// state.
    pub fleet: Option<Box<dyn plugin::Section>>,
    /// The Data page: its own state and messages (data/).
    pub data: data::State,
    pub research: research::State,
    /// The Compute and simulation page: its own state and messages (compute/).
    pub compute: compute::State,
    /// The Lattice page: its own state and messages (lattice/).
    pub lattice: lattice::State,
    /// The Models panel, and Sinai's page talking to a model of the person's own (models/).
    pub models: crate::models::State,
    /// A message for Sinai's page to send to the person's own model once it is known (a debug build's
    /// `CENTCOM_SINAI_SAY`, so the screenshot mode can photograph an answer).
    say_once: Option<String>,
    options: Options,
    shot_taken: bool,
}

#[derive(Clone, Debug)]
pub enum Message {
    Go(Section),
    Tab(Tab),
    Tick,
    Ears(Feed),
    Looked(Vec<probe::Reading>),
    StartEngine,
    /// The speech engine this window started has ended, and how (`ears::Engine::watch`).
    EngineEnded(String),
    /// The voiceprint's folder changed (`live`): the Sinai page reads the enrolment again.
    Speaker(crate::live::Seen),
    ToggleCaptions,
    ToggleDictation,
    TogglePc,
    /// The on-air bar's button: everything off.
    OffAir,
    Copy(String),
    ClearCaptions,
    ClearPc,
    Dictation(text_editor::Action),
    ClearDictation,
    FilePath(String),
    Format(usize),
    Transcribe,
    Dropped(PathBuf),
    Hovering(bool),
    Cancel(String),
    OpenFolder(String),
    CopyTranscript(Vec<String>),
    DismissNotice,
    Sinai(sinai::Feed),
    Typed(String),
    SayTyped,
    /// Dictate into the box for Sinai through the ears, or stop.
    DictateToSinai,
    SaveCaptions,
    SavePc,
    SaveDictation,
    /// Keep a finished file's transcript in the library: the audio's path and the job's outputs.
    KeepJob(String, Vec<String>),
    /// The library's folder changed (`live`): it is read again.
    LibraryLive(crate::live::Seen),
    LibraryLoaded(Result<(Vec<library::Entry>, usize), String>),
    LibrarySearch(String),
    LibraryDelete(PathBuf),
    LibraryEmptyAsk,
    LibraryEmptyConfirm,
    LibraryEmptyCancel,
    TrustReceipt(String),
    TrustData(String),
    TrustKey(String),
    TrustWitness(String),
    TrustVerify,
    /// A verdict, with the count of input changes it was asked at.
    TrustVerified(u64, Result<(trust::Verdict, trust::Stated), String>),
    /// A message for a plug-in's card on the Trust page (its place in the list).
    TrustCard(usize, plugin::Message),
    SinaiListen,
    /// Allow (true) or refuse one act Sinai holds, by its id.
    Decide(String, bool),
    /// Move a held click before allowing it: open the change for this id, edit x and y, allow it there, or not.
    ChangeHeld(String),
    ChangeX(String),
    ChangeY(String),
    AllowChanged,
    ChangeCancel,
    /// A second has passed while acts are held: their countdowns move.
    HeldTick,
    JournalStatus(sinai::Status),
    JournalKind(Option<String>),
    /// Edit the grants file the loop names: type into a grant, add or remove one, review the save, write it, or leave.
    GrantsEdit,
    GrantField(usize, grants::Field, String),
    GrantAdd,
    GrantRemove(usize),
    GrantsReview,
    GrantsBack,
    GrantsWrite,
    GrantsCancel,
    /// How Sinai listens, as its loop's /about said.
    Listening(Result<crate::machine::Listening, String>),
    /// A press on one of the Machine's acts; then Confirm (once, or twice for what cannot be undone) or Cancel.
    MemoryAct(crate::machine::Act),
    MemoryConfirm,
    MemoryCancel,
    /// Open or close the list of windows to observe.
    ObserveChoose(bool),
    /// Voice enrolment: ask to re-enrol or to erase, confirm, cancel, record the sentence on show, stop, and the
    /// tick that reads what the running script says.
    VoiceAsk { erase: bool },
    VoiceConfirm,
    VoiceCancel,
    VoiceRecord,
    VoiceStop,
    VoiceTick,
    /// Alelyon's own window, as the window system names it.
    OwnWindow(u64),
    /// The Stage's capture: the windows on offer, a picture, or a window that would not draw.
    Stage(stage::Feed),
    /// Open the Appearance creator, and what is done in it.
    AppearanceOpen,
    Creator(face::creator::Msg),
    /// Pin or unpin a window on the Stage; open or close the chooser.
    StagePin(i64),
    StageChoose(bool),
    /// Open the grants file in Notepad: the person writes it; Sinai only reads it.
    OpenGrants(String),
    DockResized(pane_grid::ResizeEvent),
    DockDragged(pane_grid::DragEvent),
    /// Show one of a dock's tabs.
    DockTab(pane_grid::Pane, usize),
    SaveDock,
    Shoot,
    Shot(window::Screenshot),
    Fleet(plugin::Message),
    Data(data::Msg),
    Research(research::Msg),
    Compute(compute::Msg),
    Lattice(lattice::Msg),
    /// The Models panel, and Sinai's page talking to a model of the person's own (`models`).
    Models(crate::models::Msg),
    /// The sign-in screen and the account menu (`signin`).
    SignIn(crate::signin::Msg),
    /// The window's own title bar and edges.
    Chrome(crate::chrome::Act),
    /// Tab (forward) or Shift+Tab (back) that no widget used: focus moves to the next or previous field.
    FocusMove(bool),
    /// Esc, wherever the keyboard is: it closes the Settings panel when that is open, and does nothing else here.
    Escape,
    WindowId(Option<window::Id>),
    Maximised(bool),
    Resized,
    Social(crate::social::Msg),
    Pages(crate::pages::Msg),
}

impl App {
    #[cfg(test)]
    pub fn boot(options: Options) -> (App, Task<Message>) {
        App::boot_with(options, &Registry::default())
    }

    /// The window as `registry` makes it: the public build's is empty.
    pub fn boot_with(options: Options, registry: &Registry) -> (App, Task<Message>) {
        // Signing in comes first (required, with Use offline); a screenshot of another page skips it.
        let (mut signin_state, signin_task) = crate::signin::State::boot(options.screenshot.is_none() || options.sign_in);
        // A debug build only: `CENTCOM_SETTINGS=<part>` opens the Settings panel at start, so it can be photographed.
        #[cfg(debug_assertions)]
        if let Ok(part) = std::env::var("CENTCOM_SETTINGS") {
            use crate::signin::settings::Pane;
            signin_state.settings =
                Some(Pane::ALL.into_iter().find(|p| p.title().eq_ignore_ascii_case(&part)).unwrap_or(Pane::Account));
            // `CENTCOM_SETTINGS_SEARCH`: something typed in its search.
            signin_state.settings_query = std::env::var("CENTCOM_SETTINGS_SEARCH").unwrap_or_default();
        }
        let (layout, layout_note) = dock::load();
        // Sinai as the person shaped it, read from where the Angel window keeps it, without the move the Angel window's
        // own lookup makes.
        let appearance_path = face::appearance_file();
        let appearance = face::load(appearance_path.as_deref());
        // What capability.rs decides by: the plug-ins this build carries, and whether it runs from a checkout.
        let plugin_ids = capability::plugin_ids(registry);
        let dev_checkout = std::env::current_exe().ok().and_then(|exe| compute::checkout_of(&exe)).is_some();
        let context = capability::Context { signed_in: false, plugins: &plugin_ids, dev_checkout };
        let shown: Vec<&plugin::Plugin> = registry.plugins().iter().filter(|p| capability::plugin_shown(p.id, &context)).collect();
        let trust_cards = shown
            .iter()
            .filter(|p| p.slot == Slot::TrustCard)
            .map(|p| compute::Mounted { id: p.id, title: p.title, page: p.make() })
            .collect();
        let plugin_features = shown.iter().filter_map(|p| p.feature.as_ref()).map(crate::catalogue::Feature::from_plugin).collect();
        let compute = compute::State::with(registry, &context);
        let mut app = App {
            section: options.section.unwrap_or(Section::Overview),
            tab: options.tab.unwrap_or(Tab::Captions),
            phase: 0.0,
            link: Link::Connecting,
            commands: None,
            engine: None,
            engine_note: None,
            starting: false,
            state: None,
            captions: Captions::default(),
            pc: Captions::default(),
            dictation: text_editor::Content::new(),
            dictation_partial: String::new(),
            file_path: String::new(),
            formats: [true, true, false, false],
            jobs: Vec::new(),
            notice: layout_note,
            probes: Vec::new(),
            services_fresh: crate::live::Freshness::waiting(),
            voice_fresh: crate::live::Freshness::waiting(),
            hovering: false,
            sinai_up: None,
            sinai_commands: None,
            sinai_works_gaps: false,
            conversation: sinai::Conversation::default(),
            held_change: None,
            journal_status: sinai::Status::All,
            journal_kind: None,
            grants_draft: None,
            grants_review: None,
            grants_note: None,
            listening: None,
            listening_at: f64::NEG_INFINITY,
            listening_asked: false,
            memory_pending: None,
            observe_windows: None,
            memory_note: None,
            voice: crate::voice::State::default(),
            signin: signin_state,
            social: crate::social::State::default(),
            pages: crate::pages::State::default(),
            window_id: None,
            maximised: false,
            bar_pressed: None,
            mark: crate::brand::mark_rgba().ok().map(|(rgba, w, h)| iced::widget::image::Handle::from_rgba(w, h, rgba)),
            own_window: None,
            stage_pinned: Vec::new(),
            stage_choosing: false,
            stage_windows: Vec::new(),
            stage_shots: std::collections::HashMap::new(),
            stage_gone: Vec::new(),
            watch_sent: false,
            face: face::Face::new(appearance.current.clone()),
            sinai_brain: None,
            appearance,
            appearance_path,
            creator: None,
            typed: String::new(),
            dictating_to_sinai: false,
            library: Vec::new(),
            library_loading: false,
            library_fresh: crate::live::Freshness::waiting(),
            library_query: String::new(),
            library_deleted: 0,
            library_confirm_empty: false,
            kept: None,
            trust_receipt: String::new(),
            trust_data: String::new(),
            trust_key: String::new(),
            trust_witness: String::new(),
            trust_checking: false,
            trust_result: None,
            trust_asked: 0,
            trust_cards,
            plugin_ids,
            dev_checkout,
            plugin_features,
            dock: pane_grid::State::with_configuration(layout),
            dock_changed: false,
            fleet: shown.iter().find(|p| p.slot == Slot::SectionPage && p.id == FLEET).map(|p| p.make()),
            data: data::State::new(),
            research: research::State::new(),
            compute,
            lattice: lattice::State::new(),
            models: crate::models::State::default(),
            say_once: None,
            options,
            shot_taken: false,
        };
        // What the opening page reads first: the services' states, or the library (a page opened directly, as by
        // `--tab library`, reads what switching to it would have read).
        let mut first = Vec::new();
        first.push(signin_task.map(Message::SignIn));
        // Alelyon's own window: the Stage never captures it, and the loop is told which it is.
        first.push(window::latest().and_then(window::raw_id::<Message>).map(Message::OwnWindow));
        first.push(window::latest().map(Message::WindowId));
        if app.section == Section::Sinai {
            app.voice.refresh();
            // which model of the person's own Sinai's page would talk to, and whether it is ready
            first.push(app.models.read().map(Message::Models));
            // `--section sinai --tab models` opens the Models panel, so it can be photographed.
            if app.options.page.as_deref() == Some("models") {
                first.push(Task::done(Message::Models(crate::models::Msg::Open)));
                // A debug build's CENTCOM_MODELS_BROWSE=<provider>: its models listed, and CENTCOM_MODELS_FILTER typed.
                #[cfg(debug_assertions)]
                if let Some(p) = std::env::var("CENTCOM_MODELS_BROWSE").ok().and_then(|id| lattice_core::hosted::provider(id.trim())) {
                    use crate::models::{Msg as M, hosted::Msg as H};
                    first.push(Task::done(Message::Models(M::Hosted(H::Browse(p.id)))));
                    let filter = std::env::var("CENTCOM_MODELS_FILTER").unwrap_or_default();
                    first.push(Task::done(Message::Models(M::Hosted(H::Filter(filter)))));
                }
            }
            // A debug build's CENTCOM_SINAI_SAY: a message for the person's own model, sent once it is known.
            #[cfg(debug_assertions)]
            {
                app.say_once = std::env::var("CENTCOM_SINAI_SAY").ok().filter(|t| !t.trim().is_empty());
            }
        }
        if app.section == Section::Transcription && app.tab == Tab::Library {
            first.push(app.load_library());
        }
        if let Some(name) = app.options.page.clone() {
            // `--section sinai --tab appearance` opens the Appearance creator, so it can be photographed.
            if app.section == Section::Sinai && name == "appearance" {
                app.creator = Some(face::creator::Creator::open(&app.appearance, appearance::Catalog::builtin()));
            }
            // `--section sinai --tab brain` measures on the Lattice page's Morphometry tab out of sight, so Sinai's
            // glass head fills with the brain and can be photographed.
            if app.section == Section::Sinai && name == "brain" {
                app.lattice.tab = lattice::Tab::Morphometry;
                first.push(app.lattice.open().map(Message::Lattice));
            }
            if let Some(fleet) = app.fleet.as_mut() {
                fleet.select(&name);
            }
            if let Some(tab) = app.compute.tab_named(&name) {
                app.compute.tab = tab;
            }
            if let Some(scene) = compute::scene_named(&name) {
                app.compute.scene = scene;
            }
            if let Some(tab) = lattice::Tab::from_name(&name) {
                app.lattice.tab = tab;
            }
            app.data.want(&name);
        }
        if app.section == Section::Compute {
            // what the opening tab reads first (the GPU job records) comes back as a task
            first.push(app.compute.open().map(Message::Compute));
        }
        if app.section == Section::Fleet {
            if let Some(fleet) = app.fleet.as_mut() {
                first.push(fleet.start().map(Message::Fleet));
            }
        }
        // `--folder` opens the Lattice IDE on a folder (and `--file`, a file of it), as an editor's command line does.
        if let Some(folder) = app.options.folder.clone() {
            app.section = Section::Lattice;
            app.lattice.tab = lattice::Tab::Chat;
            app.lattice.ide.startup = Some((folder.display().to_string(), app.options.file.clone()));
        }
        if app.section == Section::Lattice {
            first.push(app.lattice.open().map(Message::Lattice));
        }
        if app.section == Section::Data {
            first.push(Task::done(Message::Data(data::Msg::Refresh)));
        }
        if app.section == Section::Research {
            // `--tab map` opens on the Map of connections.
            // `--tab map` or `--tab gaps` opens the first subject on its Map or its Gaps.
            match app.options.page.as_deref() {
                Some("map") => app.research.view = research::View::Map,
                Some("gaps") => app.research.view = research::View::Gaps,
                _ => {}
            }
            if matches!(app.options.page.as_deref(), Some("map" | "gaps" | "papers")) {
                app.research.hub_open = false;
            }
            first.push(app.research.open().map(Message::Research));
        }
        if let Some(receipt) = app.options.verify.clone() {
            app.section = Section::Trust;
            app.trust_dropped(&receipt);
            first.push(Task::done(Message::TrustVerify));
        }
        if app.section == Section::Trust {
            first.push(app.open_trust_cards());
        }
        (app, Task::batch(first))
    }

    pub fn connected(&self) -> bool {
        self.link == Link::Connected
    }

    /// A spinner is on show and has something to turn for: the window ticks thirty times a second only then. Work
    /// loading on a page that is not on show (a Fleet read while the Overview shows) turns no spinner anyone sees, so
    /// it draws nothing; its page's spinner turns when the page is opened.
    pub fn spinning(&self) -> bool {
        if !self.signin.through() {
            return self.signin.busy.is_some();
        }
        // the rail's mark for the ears, on every page
        let ears = match &self.link {
            Link::Connected => self.recognizer_loading(),
            Link::Connecting | Link::Lost(_) => true,
            Link::Unavailable(_) => self.starting,
        };
        ears || match self.section {
            // the services' tiles until the first look is in (later looks are said only when something changed)
            Section::Overview => self.probes.is_empty(),
            Section::Sinai => {
                self.probes.is_empty()
                    || self.voice.running()
                    || (self.stage_visible() && self.stage_on_show().iter().any(|h| !self.stage_shots.contains_key(h) && !self.stage_gone.contains(h)))
                    || (self.sinai_up == Some(true) && self.conversation.thinking())
                    || self.models.busy()
            }
            Section::Transcription => self.starting || self.jobs.iter().any(ears::Job::running) || self.library_loading,
            Section::Trust => self.trust_checking || self.trust_cards.iter().any(|c| c.page.busy()),
            Section::Fleet => self.fleet.as_ref().is_some_and(|f| f.busy()),
            Section::Data => self.data.busy(),
            Section::Research => self.research.busy(),
            Section::Compute => self.compute.busy(),
            Section::Pages => self.pages.busy(),
            Section::Lattice => self.lattice.busy(),
            Section::Account => self.signin.busy.is_some(),
        } || (self.models.open && self.models.busy())
    }

    /// A research gap for Sinai: where its loop works gaps itself, it is asked to (it reads the archive, looks for newer
    /// work and keeps its finding beside the gap); otherwise the brief goes as words (`hand_to_sinai`).
    pub fn hand_gap_to_sinai(&mut self, subject: &str, headline: &str, brief: String) -> Task<Message> {
        if self.sinai_works_gaps
            && self.answering() == crate::models::Answering::Mind
            && self.sinai_commands.as_ref().is_some_and(|c| c.send(sinai::research_gap(subject, headline)))
        {
            return self.update(Message::Go(Section::Sinai));
        }
        self.hand_to_sinai(brief)
    }

    /// Send work to whoever answers on Sinai's page and open that page, so the person sees it taken up (`research::pursue`).
    /// Sinai's loop takes the text as a request of its own; the person's own model gets it as a message. When nothing
    /// can take it now, the text waits in the page's message box, to be sent with one more click.
    pub fn hand_to_sinai(&mut self, text: String) -> Task<Message> {
        use crate::models::{Answering, Msg as M, byo::Msg as B};
        let opened = self.update(Message::Go(Section::Sinai));
        let handed = match self.answering() {
            Answering::Mind => {
                match &self.sinai_commands {
                    Some(c) if c.send(sinai::say(&text)) => {}
                    _ => {
                        self.typed = text;
                        self.notice = Some("Sinai's loop is not running, so the request waits in the message box: start Sinai, then press Send.".into());
                    }
                }
                Task::none()
            }
            Answering::Own(_) => {
                let typed = self.models.update(M::Own(B::Typed(text)), &mut self.lattice.choice);
                let sent = self.models.update(M::Own(B::Send), &mut self.lattice.choice);
                Task::batch([typed.map(Message::Models), sent.map(Message::Models)])
            }
            Answering::Nobody => {
                self.notice = Some("Nobody answers on this page yet: choose a model here, then press Send. The request waits in the message box.".into());
                self.models.update(M::Own(B::Typed(text)), &mut self.lattice.choice).map(Message::Models)
            }
        };
        Task::batch([opened, handed])
    }

    /// Who answers on Sinai's page: its mind where the build carries it, unless the person picked their own model
    /// instead for this session; else the person's own model, if they chose one (`models::answering`).
    pub fn answering(&self) -> crate::models::Answering {
        crate::models::answering(self.can(Capability::SinaiMind), self.models.instead, self.models.sinai().as_ref())
    }

    /// The window's own timers, as the page on show and the work under way need them (`subscription` holds these).
    pub fn timers(&self) -> Timers {
        let held_at = self.conversation.held_at;
        Timers {
            spin: self.spinning(),
            // Not behind the sign-in screen: each look made the busy spinner redraw the whole window for a second.
            services: self.signin.through() && matches!(self.section, Section::Overview | Section::Sinai),
            // The voiceprint's folder, watched while the Sinai page shows it (it was read again every 30 s).
            speaker: self.signin.through() && self.section == Section::Sinai,
            // A re-enrolment's script is read while it runs, wherever the window is; quickly only where it is on show.
            voice: self.voice.running().then_some(if self.section == Section::Sinai { VOICE_ON_SHOW } else { VOICE_ELSEWHERE }),
            // A held act's countdown moves once a second, only while one is still counting and its page is open.
            held: self.section == Section::Sinai
                && self.conversation.held.iter().any(|h| h.left(held_at, std::time::Instant::now()) > 0.0),
        }
    }

    /// Sinai's own microphone, when its loop has it open: how it is listening.
    /// The window Sinai's hands are armed on, as the loop last said; None when they are not armed (or it has not said).
    pub fn sinai_window(&self) -> Option<i64> {
        self.conversation.hands.as_ref().filter(|h| h.armed && h.handle != 0).map(|h| h.handle)
    }

    /// Whether the Stage is on show: the Sinai page is open and a dock shows the Stage tab.
    pub fn stage_visible(&self) -> bool {
        self.section == Section::Sinai
            && self.grants_draft.is_none()
            && self.creator.is_none()
            && self.dock.iter().any(|(_, tabs)| tabs.views.get(tabs.active) == Some(&dock::View::Stage))
    }

    /// A web page of the account panel that went to the person's own browser instead of Lattice's: the Lattice page
    /// says why, and this window's notice shows it.
    fn take_web_said(&mut self) {
        if let Some(said) = self.lattice.web_said.take() {
            self.notice = Some(said);
        }
    }

    /// Sinai's glass head holds the brain the Lattice page measured last, while its Morphometry tab says so; with none,
    /// the head is opaque, as it always was.
    fn put_brain_in_sinai(&mut self) {
        let morph = &self.lattice.morphometry;
        let wanted = morph.brain_layout.clone().filter(|_| morph.in_sinai);
        if wanted.as_ref().map(Arc::as_ptr) != self.sinai_brain.as_ref().map(Arc::as_ptr) {
            let inside = wanted.as_ref().map(|b| face::Interior::new(lattice::brain::in_head(b, face::cavity())));
            self.face.set_interior(inside);
            self.sinai_brain = wanted;
        }
    }

    /// Whether Sinai's face is on show: the Sinai page is open and a dock shows Sinai's tab. Only then is the face in
    /// the window at all, so only then does its frame clock run.
    pub fn face_visible(&self) -> bool {
        face::on_show(self.section == Section::Sinai, self.grants_draft.is_some() || self.creator.is_some(), self.dock.iter().filter_map(|(_, tabs)| tabs.views.get(tabs.active).copied()))
    }

    /// The windows on the Stage now: Sinai's first, then the pinned, never Alelyon's own.
    pub fn stage_on_show(&self) -> Vec<i64> {
        stage::on_show(self.sinai_window(), &self.stage_pinned, self.own_window.unwrap_or(0) as i64)
    }

    /// Name this window to the loop once a connection, when the Stage is first on show (as the Angel window does), so
    /// Sinai never observes the window that shows what it sees; `on` is the loop's own word for a view being shown.
    fn tell_loop_which_window(&mut self) {
        if self.watch_sent || !self.stage_visible() {
            return;
        }
        if let (Some(own), Some(c)) = (self.own_window, &self.sinai_commands)
            && c.send(sinai::watch(own))
        {
            self.watch_sent = true;
        }
    }

    /// Why the grants file may not be written now, if it may not: Sinai's hands are armed, or the loop runs and has not
    /// said whether they are. Sinai acts on this computer only through the window its hands are armed on, so while
    /// they are not armed anywhere this window cannot be the way Sinai grants itself anything.
    pub fn grants_blocked(&self) -> Option<String> {
        if self.sinai_up != Some(true) {
            return None;
        }
        match &self.conversation.hands {
            None => Some(
                "Sinai's loop has not yet said whether its hands are armed. Grants are written only once it has said they are                  not."
                    .into(),
            ),
            Some(hands) if hands.armed => Some(format!(
                "Sinai's hands are armed{}. Grants are written only while they are not, so this window cannot be the way                  Sinai grants itself anything.",
                if hands.title.is_empty() { String::new() } else { format!(" on \"{}\"", hands.title) }
            )),
            Some(_) => None,
        }
    }

    pub fn sinai_on_air(&self) -> Option<&'static str> {
        let mic = &self.conversation.mic;
        (self.sinai_up == Some(true) && mic.on).then_some(if mic.attending { "in a conversation" } else { "waiting for its name" })
    }

    pub fn recognizer_loading(&self) -> bool {
        self.connected() && self.state.as_ref().is_some_and(|s| !s.recognizer.is_empty() && s.recognizer != "ready")
    }

    /// What is on air now: (the microphone's device and what for, the PC's device).
    pub fn on_air(&self) -> (Option<(String, &'static str)>, Option<String>) {
        let Some(s) = self.state.as_ref().filter(|_| self.connected()) else {
            return (None, None);
        };
        let mic = s.mic_on.then(|| {
            let what = match (s.listening, s.dictating) {
                (true, true) => "captions and dictation",
                (false, true) => "dictation",
                _ => "captions",
            };
            (s.mic_device.clone(), what)
        });
        (mic, s.pc_on.then(|| s.pc_device.clone()))
    }

    fn send(&mut self, command: serde_json::Value) {
        match &self.commands {
            Some(c) if c.send(command) => {}
            _ => self.notice = Some("The speech engine is not connected. Start it on the Transcription page.".into()),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        crate::frame_stats::message(&message);
        let task = self.handle(message);
        crate::frame_stats::section(self.section.short());
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(section) => {
                // The simulator runs only while its page is on show.
                if self.section == Section::Compute && section != Section::Compute {
                    self.compute.close();
                }
                if self.section == Section::Lattice && section != Section::Lattice {
                    self.lattice.close();
                }
                if self.section == Section::Trust && section != Section::Trust {
                    self.trust_cards.iter_mut().for_each(|c| c.page.close());
                }
                self.section = section;
                self.tell_loop_which_window();
                // The services are looked at by `probe::watch` while these pages are open; Sinai's slow facts are read now.
                if section == Section::Sinai {
                    self.voice.refresh();
                    let models = if self.models.catalog.is_none() { self.models.read().map(Message::Models) } else { Task::none() };
                    return Task::batch([self.read_listening(), models]);
                }
                if section == Section::Fleet {
                    return self.fleet.as_mut().map_or_else(Task::none, |f| f.open().map(Message::Fleet));
                }
                if section == Section::Data {
                    return self.data.open().map(Message::Data);
                }
                if section == Section::Research {
                    return self.research.open().map(Message::Research);
                }
                if section == Section::Compute {
                    return self.compute.open().map(Message::Compute);
                }
                if section == Section::Pages && self.pages.active() {
                    return self.pages.open().map(Message::Pages);
                }
                if section == Section::Lattice {
                    return self.lattice.open().map(Message::Lattice);
                }
                if section == Section::Trust {
                    return self.open_trust_cards();
                }
            }
            Message::Tab(tab) => {
                self.tab = tab;
                self.kept = None;
                if tab == Tab::Library {
                    return self.load_library();
                }
            }
            Message::Tick => self.phase = (self.phase + spinner::STEP) % std::f32::consts::TAU,
            Message::Fleet(msg) => {
                if let Some(fleet) = self.fleet.as_mut() {
                    return fleet.update(msg).map(Message::Fleet);
                }
            }
            Message::Data(msg) => return self.data.update(msg).map(Message::Data),
            // A gap handed to Sinai from the Research page: the page marks it, and the brief goes to Sinai's page.
            Message::Research(crate::research::Msg::Pursue(gap)) => {
                let brief = crate::research::pursue::brief(&gap);
                let (subject, headline) = (gap.subject.clone(), gap.headline.clone());
                let marked = self.research.update(crate::research::Msg::Pursue(gap)).map(Message::Research);
                return Task::batch([marked, self.hand_gap_to_sinai(&subject, &headline, brief)]);
            }
            Message::Research(msg) => return self.research.update(msg).map(Message::Research),
            Message::Compute(msg) => return self.compute.update(msg).map(Message::Compute),
            Message::Social(msg) => return self.social.update(msg).map(Message::Social),
            Message::Pages(msg) => return self.pages.update(msg).map(Message::Pages),
            Message::Escape => {
                if self.signin.settings.is_some() {
                    return self.update(Message::SignIn(crate::signin::Msg::SettingsClose));
                }
            }
            Message::FocusMove(back) => {
                return if back { iced::widget::operation::focus_previous() } else { iced::widget::operation::focus_next() };
            }
            Message::WindowId(id) => self.window_id = id,
            Message::Maximised(m) => self.maximised = m,
            Message::Resized => {
                if let Some(id) = self.window_id {
                    return window::is_maximized(id).map(Message::Maximised);
                }
            }
            Message::Chrome(act) => {
                use crate::chrome::Act;
                let Some(id) = self.window_id else { return Task::none() };
                let act = match act {
                    Act::Press => {
                        let now = std::time::Instant::now();
                        let act = crate::chrome::press(self.bar_pressed, now);
                        self.bar_pressed = matches!(act, Act::Press).then_some(now);
                        act
                    }
                    other => other,
                };
                return match act {
                    Act::Press => window::drag(id),
                    Act::Minimise => window::minimize(id, true),
                    Act::ToggleMaximise => window::toggle_maximize(id).chain(window::is_maximized(id).map(Message::Maximised)),
                    // as the account menu's Exit: friends are told this app went offline first
                    Act::Close => self.update(Message::SignIn(crate::signin::Msg::Exit)),
                    Act::Resize(direction) if !self.maximised => window::drag_resize(id, direction),
                    Act::Resize(_) => Task::none(),
                };
            }
            // A web page of the account panel, when Settings sends them to Lattice's own browser (`signin::web`): the
            // Lattice page opens it in a tab of its own.
            Message::SignIn(crate::signin::Msg::Open(url)) if self.signin.web == crate::signin::web::WebPages::Lattice => {
                let task = self.lattice.open_web(url).map(Message::Lattice);
                self.take_web_said();
                return task;
            }
            Message::SignIn(msg) => {
                // The account menu's Exit acts on the whole app (its Settings opens the panel over the page).
                let exit = matches!(msg, crate::signin::Msg::Exit);
                let task = self.signin.update(msg).map(Message::SignIn);
                // the friends panel follows the session: it starts on signing in and forgets on signing out
                let friends = self.social.attach(self.signin.session().filter(|_| self.can(Capability::Friends))).map(Message::Social);
                // so does the Pages page: it is read again on signing in, and forgotten on signing out
                let on_show = self.section == Section::Pages;
                let pages = self.pages.attach(self.signin.session().filter(|_| self.can(Capability::Pages)), on_show).map(Message::Pages);
                if exit {
                    // tell friends this app went offline before the window closes
                    return if self.social.active() {
                        self.social.presence("offline").map(Message::Social).chain(iced::exit())
                    } else {
                        iced::exit()
                    };
                }
                return Task::batch([task, friends, pages]);
            }
            Message::Models(msg) => {
                let task = self.models.update(msg, &mut self.lattice.choice).map(Message::Models);
                if let Some(say) = self.say_once.clone()
                    && matches!(self.answering(), crate::models::Answering::Own(_))
                {
                    self.say_once = None;
                    let typed = self.models.update(crate::models::Msg::Own(crate::models::byo::Msg::Typed(say)), &mut self.lattice.choice);
                    let sent = self.models.update(crate::models::Msg::Own(crate::models::byo::Msg::Send), &mut self.lattice.choice);
                    return Task::batch([task, typed.map(Message::Models), sent.map(Message::Models)]);
                }
                return task;
            }
            Message::Lattice(msg) => {
                let task = self.lattice.update(msg).map(Message::Lattice);
                self.put_brain_in_sinai();
                self.take_web_said();
                return task;
            }
            Message::Ears(feed) => self.ears(feed),
            // The voiceprint's files changed on disk (a re-enrolment, an erase, or the probe writing its decision).
            Message::Speaker(seen) => {
                self.voice_fresh.saw(&seen);
                if !seen.first {
                    self.voice.refresh();
                }
            }
            Message::Listening(read) => {
                self.listening_asked = false;
                self.listening = Some(read);
            }
            Message::MemoryAct(act) => {
                self.observe_windows = None;
                self.memory_note = None;
                let step = crate::machine::press(act);
                self.memory_step(step);
            }
            Message::MemoryConfirm => {
                if let Some(pending) = self.memory_pending.take() {
                    let step = crate::machine::confirm(pending);
                    self.memory_step(step);
                }
            }
            Message::MemoryCancel => self.memory_pending = None,
            Message::VoiceAsk { erase } => self.voice.ask(erase),
            Message::VoiceConfirm => self.voice.confirm(crate::utc::now()),
            Message::VoiceCancel => self.voice.cancel(),
            Message::VoiceRecord => self.voice.record_next(),
            Message::VoiceStop => self.voice.stop(),
            Message::VoiceTick => self.voice.tick(crate::utc::now()),
            Message::ObserveChoose(open) => {
                self.observe_windows = open.then(|| stage::open_windows(self.own_window.unwrap_or(0) as i64));
            }
            Message::Looked(readings) => {
                self.probes = readings;
                self.services_fresh.heard(crate::utc::now(), None);
            }
            Message::StartEngine => {
                if self.engine.is_none() {
                    match ears::Engine::start() {
                        Ok(engine) => {
                            self.engine = Some(engine);
                            self.starting = true;
                            self.engine_note = None;
                        }
                        Err(why) => self.engine_note = Some(why),
                    }
                }
            }
            Message::EngineEnded(status) => {
                if self.engine.is_some() {
                    self.engine = None;
                    self.starting = false;
                    self.engine_note = Some(format!(
                        "The speech engine stopped ({status}). Its log is ~/.alelyon/angel/centcom-ears.log."
                    ));
                }
            }
            Message::ToggleCaptions => {
                let on = !self.state.as_ref().is_some_and(|s| s.listening);
                self.send(ears::listen("mic", on));
            }
            Message::ToggleDictation => {
                let on = !self.state.as_ref().is_some_and(|s| s.dictating);
                if on {
                    self.dictating_to_sinai = false;
                }
                self.send(ears::dictate(on));
            }
            Message::DictateToSinai => {
                let dictating = self.state.as_ref().is_some_and(|s| s.dictating);
                if dictating && self.dictating_to_sinai {
                    self.send(ears::dictate(false));
                } else {
                    // The ears hear the microphone the person speaks into; the words land in the box, to be read
                    // and sent: nothing reaches Sinai until Send.
                    self.dictating_to_sinai = true;
                    if !dictating {
                        self.send(ears::dictate(true));
                    }
                }
            }
            Message::TogglePc => {
                let on = !self.state.as_ref().is_some_and(|s| s.pc_on);
                self.send(ears::listen("pc", on));
            }
            Message::OffAir => {
                // a voice enrolment recording is on air too: it stops with the rest
                if self.voice.on_air() {
                    self.voice.stop();
                }
                let s = self.state.clone().unwrap_or_default();
                if self.connected() {
                    if s.listening {
                        self.send(ears::listen("mic", false));
                    }
                    if s.dictating {
                        self.send(ears::dictate(false));
                    }
                    if s.pc_on {
                        self.send(ears::listen("pc", false));
                    }
                }
                if self.sinai_on_air().is_some()
                    && let Some(c) = &self.sinai_commands
                {
                    c.send(sinai::listen(false));
                }
            }
            Message::Sinai(feed) => match feed {
                sinai::Feed::Unavailable | sinai::Feed::Lost => {
                    self.sinai_up = Some(false);
                    self.sinai_commands = None;
                    self.sinai_works_gaps = false;
                    self.conversation.state.clear();
                    self.conversation.hearing.clear();
                    self.conversation.mic = sinai::Mic::default();
                    self.conversation.held.clear();
                    self.conversation.hands = None;
                    self.held_change = None;
                }
                sinai::Feed::Connected(commands) => {
                    self.sinai_up = Some(true);
                    self.sinai_commands = Some(commands);
                    self.watch_sent = false;
                    self.tell_loop_which_window();
                    self.listening_at = f64::NEG_INFINITY;
                    return self.read_listening();
                }
                sinai::Feed::Event(v) => {
                    match v.get("type").and_then(|t| t.as_str()) {
                        Some("abilities") => self.sinai_works_gaps = v.get("research_gap").and_then(|b| b.as_bool()).unwrap_or(false),
                        // Sinai kept a finding beside a gap: the Research page reads its gaps again to show it.
                        Some("research_finding") => {
                            let subject = v.get("subject").and_then(|s| s.as_str()).unwrap_or("").to_string();
                            return self.research.update(crate::research::Msg::FindingKept(subject)).map(Message::Research);
                        }
                        _ => {}
                    }
                    self.conversation.absorb(&v);
                    // A click being moved that the loop no longer holds is not sent: the change closes with it.
                    if let Some((id, _, _)) = &self.held_change
                        && !self.conversation.held.iter().any(|h| &h.id == id)
                    {
                        self.held_change = None;
                    }
                    // The loop said its microphone changed: how it listens (open, overheard) is asked again, rather than
                    // on a timer while nothing changes.
                    if v.get("type").and_then(|t| t.as_str()) == Some("mic") {
                        self.listening_at = f64::NEG_INFINITY;
                        return self.read_listening();
                    }
                }
            },
            Message::Typed(text) => self.typed = text,
            Message::SayTyped => {
                let text = self.typed.trim().to_string();
                if !text.is_empty() {
                    match &self.sinai_commands {
                        Some(c) if c.send(sinai::say(&text)) => self.typed.clear(),
                        _ => self.notice = Some("Sinai's loop is not running, so nothing was sent.".into()),
                    }
                }
            }
            Message::SinaiListen => {
                let on = !self.conversation.mic.on;
                if let Some(c) = &self.sinai_commands {
                    c.send(sinai::listen(on));
                }
            }
            Message::Decide(id, yes) => {
                if self.held_change.as_ref().is_some_and(|(changing, _, _)| *changing == id) {
                    self.held_change = None;
                }
                if let Some(c) = &self.sinai_commands {
                    c.send(sinai::decide(&id, yes));
                }
            }
            Message::ChangeHeld(id) => {
                // Only a click the loop lets a person move: it carries out a changed x and y, and nothing else.
                if let Some((x, y)) = self.conversation.held.iter().find(|h| h.id == id).and_then(sinai::Held::click_at) {
                    self.held_change = Some((id, x.to_string(), y.to_string()));
                }
            }
            Message::ChangeX(x) => {
                if let Some(change) = &mut self.held_change {
                    change.1 = x;
                }
            }
            Message::ChangeY(y) => {
                if let Some(change) = &mut self.held_change {
                    change.2 = y;
                }
            }
            Message::ChangeCancel => self.held_change = None,
            Message::AllowChanged => {
                let Some((id, x, y)) = self.held_change.clone() else {
                    return Task::none();
                };
                // The loop drops an act whose change it refuses, so only two whole numbers for a held click are sent.
                let Some(changes) = self.conversation.held.iter().find(|h| h.id == id).and_then(|h| sinai::click_change(h, &x, &y)) else {
                    return Task::none();
                };
                match &self.sinai_commands {
                    Some(c) if c.send(sinai::decide_changed(&id, &changes)) => self.held_change = None,
                    _ => self.notice = Some("Sinai's loop is not running, so nothing was sent.".into()),
                }
            }
            Message::HeldTick => {}
            Message::JournalStatus(status) => self.journal_status = status,
            Message::JournalKind(kind) => self.journal_kind = kind,
            Message::GrantsEdit => {
                self.grants_note = None;
                if self.conversation.grants_path.is_empty() {
                    self.grants_note = Some(
                        "Sinai's loop names its grants file when its hands are armed or disarmed, and has not named it since this                          window connected."
                            .into(),
                    );
                    return Task::none();
                }
                match grants::open(Path::new(&self.conversation.grants_path)) {
                    Ok(draft) => {
                        self.grants_draft = Some(draft);
                        self.grants_review = None;
                    }
                    Err(why) => self.grants_note = Some(why),
                }
            }
            Message::GrantField(i, field, value) => {
                if let Some(grant) = self.grants_draft.as_mut().and_then(|d| d.grants.get_mut(i)) {
                    grant.set(field, value);
                    // an edit withdraws the review: what is confirmed is always what is on show
                    self.grants_review = None;
                }
            }
            Message::GrantAdd => {
                if let Some(draft) = &mut self.grants_draft {
                    draft.grants.push(grants::Grant::blank());
                    self.grants_review = None;
                }
            }
            Message::GrantRemove(i) => {
                if let Some(draft) = &mut self.grants_draft
                    && i < draft.grants.len()
                {
                    draft.grants.remove(i);
                    self.grants_review = None;
                }
            }
            Message::GrantsReview => {
                if let Some(draft) = &self.grants_draft {
                    self.grants_review = Some(
                        grants::written(&draft.grants).map(|list| {
                            let changes = grants::changes(&draft.read, &draft.grants, &list);
                            (list, changes)
                        }),
                    );
                }
            }
            Message::GrantsBack => self.grants_review = None,
            Message::GrantsWrite => {
                if let Some(why) = self.grants_blocked() {
                    self.grants_note = Some(why);
                    return Task::none();
                }
                if let (Some(draft), Some(Ok((list, _)))) = (&self.grants_draft, &self.grants_review) {
                    match grants::write(draft, list) {
                        Ok(()) => {
                            self.grants_note = Some(format!(
                                "Written to {}. Sinai's loop reads it again the next time it checks a grant, or when its hands                                  are armed or disarmed; until then the Grants view shows what it last read.",
                                draft.path.display()
                            ));
                            self.grants_draft = None;
                            self.grants_review = None;
                        }
                        Err(why) => self.grants_note = Some(why),
                    }
                }
            }
            Message::GrantsCancel => {
                self.grants_draft = None;
                self.grants_review = None;
            }
            Message::OwnWindow(handle) => {
                self.own_window = Some(handle);
                self.tell_loop_which_window();
            }
            Message::Stage(feed) => match feed {
                stage::Feed::Windows(windows) => self.stage_windows = windows,
                stage::Feed::Shot(handle, shot) => {
                    // a picture from a capture that has since been stopped, of a window no longer on show, is dropped
                    if self.stage_on_show().contains(&handle) {
                        let picture = iced::widget::image::Handle::from_rgba(shot.width, shot.height, shot.rgba);
                        self.stage_shots.insert(handle, (picture, shot.coverage));
                        self.stage_gone.retain(|h| *h != handle);
                    }
                }
                stage::Feed::Gone(handle) => {
                    self.stage_shots.remove(&handle);
                    if !self.stage_gone.contains(&handle) {
                        self.stage_gone.push(handle);
                    }
                }
            },
            Message::StagePin(handle) => {
                if let Some(i) = self.stage_pinned.iter().position(|h| *h == handle) {
                    self.stage_pinned.remove(i);
                    self.stage_shots.remove(&handle);
                } else if self.stage_pinned.len() < stage::MOST {
                    self.stage_pinned.push(handle);
                }
            }
            Message::AppearanceOpen => {
                self.creator = Some(face::creator::Creator::open(&self.appearance, appearance::Catalog::builtin()));
            }
            Message::Creator(msg) => {
                let Some(creator) = self.creator.as_mut() else { return Task::none() };
                match creator.update(msg, appearance::Catalog::builtin()) {
                    face::creator::Outcome::Open => {}
                    face::creator::Outcome::Copy(code) => return iced::clipboard::write(code),
                    face::creator::Outcome::Cancelled => self.creator = None,
                    // Kept and written only now, as the Angel window's creator writes it: whole, then renamed into place.
                    face::creator::Outcome::Done(current, looks) => {
                        self.appearance.current = current.clone();
                        self.appearance.looks = looks;
                        match face::save(&self.appearance, self.appearance_path.as_deref()) {
                            Ok(()) => self.appearance.notice.clear(),
                            Err(why) => self.notice = Some(format!("Sinai's appearance is on show but was not saved: {why}")),
                        }
                        self.face.lock().appearance = current;
                        self.creator = None;
                    }
                }
            }
            Message::StageChoose(open) => {
                self.stage_choosing = open;
                self.tell_loop_which_window();
            }
            Message::SaveCaptions => {
                let text = self.captions.timed();
                return self.keep("Captions", None, &text);
            }
            Message::SavePc => {
                let text = self.pc.timed();
                return self.keep("PC audio", None, &text);
            }
            Message::SaveDictation => {
                let text = self.dictation.text();
                return self.keep("Dictation", None, &text);
            }
            Message::KeepJob(audio, outputs) => {
                let title = Path::new(&audio).file_stem().map(|s| s.to_string_lossy().to_string());
                match outputs.iter().find(|o| o.ends_with(".txt")).map(std::fs::read_to_string) {
                    Some(Ok(text)) => return self.keep("File", title.as_deref(), &text),
                    Some(Err(e)) => self.notice = Some(format!("Could not read the transcript: {e}")),
                    None => self.notice = Some("Only a text (TXT) transcript can be kept in the library; save as TXT too.".into()),
                }
            }
            Message::LibraryLive(seen) => {
                self.library_fresh.saw(&seen);
                if !seen.first {
                    return self.load_library();
                }
            }
            Message::LibraryLoaded(read) => {
                self.library_loading = false;
                match read {
                    Ok((entries, deleted)) => {
                        self.library = entries;
                        self.library_deleted = deleted;
                    }
                    Err(e) => self.notice = Some(format!("The library could not be read: {e}")),
                }
            }
            Message::LibrarySearch(query) => self.library_query = query,
            Message::LibraryDelete(path) => {
                if let Some(root) = library::dir() {
                    match library::delete_in(&root, &path) {
                        Ok(()) => self.kept = Some("Moved to the library's Deleted folder, from which it can be restored.".into()),
                        Err(e) => self.notice = Some(format!("Not deleted: {e}")),
                    }
                    return self.load_library();
                }
            }
            Message::LibraryEmptyAsk => self.library_confirm_empty = true,
            Message::LibraryEmptyCancel => self.library_confirm_empty = false,
            Message::LibraryEmptyConfirm => {
                self.library_confirm_empty = false;
                if let Some(root) = library::dir() {
                    match library::empty_deleted_in(&root) {
                        Ok(n) => self.kept = Some(format!("{n} deleted transcript(s) removed for good.")),
                        Err(e) => self.notice = Some(format!("The Deleted folder was not emptied: {e}")),
                    }
                    return self.load_library();
                }
            }
            Message::TrustReceipt(path) => {
                self.trust_receipt = path;
                self.trust_changed();
            }
            Message::TrustData(path) => {
                self.trust_data = path;
                self.trust_changed();
            }
            Message::TrustKey(key) => {
                self.trust_key = key;
                self.trust_changed();
            }
            Message::TrustWitness(key) => {
                self.trust_witness = key;
                self.trust_changed();
            }
            Message::TrustVerify => return self.trust_verify(),
            Message::TrustCard(i, message) => {
                if let Some(card) = self.trust_cards.get_mut(i) {
                    return card.page.update(message).map(move |m| Message::TrustCard(i, m));
                }
            }
            Message::TrustVerified(asked, result) => {
                self.trust_checking = false;
                // Inputs changed while it ran: the verdict is about files or keys no longer shown, so it is dropped.
                if asked == self.trust_asked {
                    self.trust_result = Some(result);
                }
            }
            Message::OpenGrants(path) => {
                if let Err(e) = std::process::Command::new("notepad.exe").arg(&path).spawn() {
                    self.notice = Some(format!("Could not open {path}: {e}"));
                }
            }
            Message::DockResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.dock.resize(split, ratio);
                self.dock_changed = true;
            }
            Message::DockDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                self.dock.drop(pane, target);
                self.dock_changed = true;
            }
            Message::DockDragged(_) => {}
            Message::DockTab(pane, index) => {
                if let Some(tabs) = self.dock.get_mut(pane)
                    && index < tabs.views.len()
                {
                    tabs.active = index;
                    self.dock_changed = true;
                }
                self.tell_loop_which_window();
            }
            Message::SaveDock => {
                if self.dock_changed {
                    self.dock_changed = false;
                    if let Err(e) = dock::save(&self.dock) {
                        self.notice = Some(format!("The layout was not saved: {e}"));
                    }
                }
            }
            Message::Copy(text) => return iced::clipboard::write(text),
            Message::ClearCaptions => self.captions = Captions::default(),
            Message::ClearPc => self.pc = Captions::default(),
            Message::Dictation(action) => self.dictation.perform(action),
            Message::ClearDictation => {
                self.dictation = text_editor::Content::new();
                self.dictation_partial.clear();
            }
            Message::FilePath(path) => self.file_path = path,
            Message::Format(i) => {
                if let Some(f) = self.formats.get_mut(i) {
                    *f = !*f;
                }
                if !self.formats.iter().any(|f| *f) {
                    self.formats[0] = true; // a transcript needs at least one form
                }
            }
            Message::Transcribe => {
                let path = self.file_path.trim().trim_matches('"').to_string();
                if !path.is_empty() {
                    self.transcribe(&path);
                    self.file_path.clear();
                }
            }
            Message::Dropped(path) => {
                self.hovering = false;
                // On the Trust page a dropped file is a receipt, a test case or the data; on the Lattice page a folder
                // to open in the IDE (or a file of the folder it shows); anywhere else, audio.
                if self.section == Section::Trust {
                    self.trust_dropped(&path);
                } else if self.section == Section::Lattice {
                    return self.lattice.update(lattice::Msg::Ide(lattice::ide::IdeMsg::Dropped(path))).map(Message::Lattice);
                } else {
                    self.section = Section::Transcription;
                    self.tab = Tab::Files;
                    self.transcribe(&path.display().to_string());
                }
            }
            Message::Hovering(on) => self.hovering = on,
            Message::Cancel(job) => self.send(ears::cancel(&job)),
            Message::OpenFolder(path) => {
                // Explorer opens the folder with the file selected; it is the user's own file, by their click.
                let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn();
            }
            Message::CopyTranscript(outputs) => {
                let pick = outputs.iter().find(|o| o.ends_with(".txt")).or(outputs.first());
                match pick.map(std::fs::read_to_string) {
                    Some(Ok(text)) => return iced::clipboard::write(text),
                    Some(Err(e)) => self.notice = Some(format!("Could not read the transcript: {e}")),
                    None => {}
                }
            }
            Message::DismissNotice => self.notice = None,
            Message::Shoot => {
                if !self.shot_taken {
                    self.shot_taken = true;
                    return window::latest().and_then(window::screenshot).map(Message::Shot);
                }
            }
            Message::Shot(shot) => {
                if let Some(path) = &self.options.screenshot
                    && let Err(e) = lattice_app::screenshot::save(path, &shot)
                {
                    eprintln!("centcom: the screenshot was not written: {e}");
                    lattice_app::screenshot::mark_failed();
                }
                return iced::exit();
            }
        }
        Task::none()
    }

    /// Keep `text` in the library as a `kind` transcript, and say where.
    fn keep(&mut self, kind: &str, title: Option<&str>, text: &str) -> Task<Message> {
        let Some(root) = library::dir() else {
            self.notice = Some("USERPROFILE is not set, so the library has no place on this PC.".into());
            return Task::none();
        };
        match library::save_in(&root, &library::stamp_now(), kind, title, text) {
            Ok(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.kept = Some(format!("Kept in the library as \"{name}\"."));
                self.load_library()
            }
            Err(e) => {
                self.notice = Some(format!("Not saved: {e}."));
                Task::none()
            }
        }
    }

    /// A file dropped on the Trust page goes where it belongs; a test case fills in its own keys, visibly.
    fn trust_dropped(&mut self, path: &Path) {
        let shown = path.display().to_string();
        match trust::read(path).map(|v| trust::classify(&v)) {
            Ok(trust::Found::Envelope(_)) => self.trust_receipt = shown,
            Ok(trust::Found::Case { public_key_hex, witness_key_hex, .. }) => {
                self.trust_receipt = shown;
                self.trust_data.clear();
                self.trust_key = public_key_hex;
                self.trust_witness = witness_key_hex;
            }
            Ok(trust::Found::Data(_)) => self.trust_data = shown,
            Ok(trust::Found::Other) => self.notice = Some(format!("{shown} is neither a receipt nor the data of one.")),
            Err(e) => self.notice = Some(e),
        }
        self.trust_changed();
    }

    /// Any change to what is checked clears the verdict: it stood for the inputs as they were.
    fn trust_changed(&mut self) {
        self.trust_asked += 1;
        self.trust_result = None;
    }

    /// Verify off the window's thread: a large data file takes a moment, and the spinner says so.
    fn trust_verify(&mut self) -> Task<Message> {
        let (receipt, data) = (self.trust_receipt.trim().trim_matches('"').to_string(), self.trust_data.trim().trim_matches('"').to_string());
        if receipt.is_empty() {
            return Task::none();
        }
        let (key, witness, asked) = (self.trust_key.clone(), self.trust_witness.clone(), self.trust_asked);
        self.trust_checking = true;
        self.trust_result = None;
        Task::perform(
            async move {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = std::thread::Builder::new().name("centcom-verify".into()).spawn(move || {
                    let _ = tx.send(verify_files(&receipt, &data, &key, &witness));
                });
                rx.await.unwrap_or_else(|_| Err("the verifier stopped".into()))
            },
            move |result| Message::TrustVerified(asked, result),
        )
    }

    /// The Trust page opened: its plug-in cards are told, and read what they show (Data trust looks at its files).
    fn open_trust_cards(&mut self) -> Task<Message> {
        Task::batch(self.trust_cards.iter_mut().enumerate().map(|(i, c)| c.page.open().map(move |m| Message::TrustCard(i, m))))
    }

    /// Whether `capability` is available now (capability.rs holds the list of what unlocks each).
    pub fn can(&self, capability: Capability) -> bool {
        let context = capability::Context { signed_in: self.signin.session().is_some(), plugins: &self.plugin_ids, dev_checkout: self.dev_checkout };
        capability::available(capability, &context)
    }

    /// Read the library off the window's thread.
    fn load_library(&mut self) -> Task<Message> {
        let Some(root) = library::dir() else {
            return Task::none();
        };
        // The spinner turns for the first read only: a read the library's watch asked for replaces the list in place.
        self.library_loading = self.library.is_empty();
        Task::perform(
            async move {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = std::thread::Builder::new().name("centcom-library".into()).spawn(move || {
                    let _ = tx.send(library::list_in(&root).map(|entries| (entries, library::deleted_count_in(&root))));
                });
                rx.await.unwrap_or_else(|_| Err("the library reader stopped".into()))
            },
            Message::LibraryLoaded,
        )
    }

    /// Send an act to the loop, or hold it for its next confirmation. Nothing is sent without a connection: the act
    /// is not kept to be sent later, and the person is told.
    fn memory_step(&mut self, step: crate::machine::Step) {
        match step {
            crate::machine::Step::Ask(pending) => self.memory_pending = Some(pending),
            crate::machine::Step::Send(message) => {
                self.memory_pending = None;
                let sent = self.sinai_commands.as_ref().is_some_and(|c| c.send(message));
                if !sent {
                    self.memory_note = Some("Sinai's loop is not connected, so nothing was sent.".into());
                }
            }
        }
    }

    /// Ask the loop how Sinai listens, when it runs, the Sinai page is open and the last answer is a minute old.
    fn read_listening(&mut self) -> Task<Message> {
        let now = crate::utc::now();
        if self.sinai_up != Some(true) || self.section != Section::Sinai || self.listening_asked || now - self.listening_at < crate::machine::ABOUT_EVERY_S {
            return Task::none();
        }
        self.listening_asked = true;
        self.listening_at = now;
        Task::perform(off_thread("centcom-about", || crate::machine::read_about(crate::machine::loop_port())), |r| {
            Message::Listening(r.unwrap_or_else(|| Err("the reading thread ended without an answer".into())))
        })
    }

    fn transcribe(&mut self, path: &str) {
        if !Path::new(path).is_file() {
            self.notice = Some(format!("There is no file at {path}."));
            return;
        }
        let formats: Vec<&str> = FORMATS.iter().zip(self.formats).filter(|(_, on)| *on).map(|(f, _)| *f).collect();
        self.send(ears::transcribe_file(path, &formats));
    }

    fn ears(&mut self, feed: Feed) {
        match feed {
            // A look that follows "not running" or "lost" keeps showing that, so the page does not flicker
            // between two words every two seconds.
            Feed::Connecting => {
                if self.link == Link::Connected {
                    self.link = Link::Connecting;
                }
            }
            Feed::Unavailable(why) => {
                self.link = Link::Unavailable(why);
                self.commands = None;
                self.state = None;
            }
            Feed::Connected(commands) => {
                self.link = Link::Connected;
                self.commands = Some(commands);
                self.starting = false;
            }
            Feed::Lost(why) => {
                self.link = Link::Lost(why);
                self.commands = None;
                self.state = None;
            }
            Feed::Event(v) => self.event(&v),
        }
    }

    fn event(&mut self, v: &serde_json::Value) {
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let purpose = v.get("purpose").and_then(|p| p.as_str()).unwrap_or("");
        let words = |key: &str| v.get(key).and_then(|t| t.as_str()).unwrap_or("").to_string();
        match kind {
            "ears.state" => {
                self.state = Some(ears::parse_state(v));
                for entry in v.get("jobs").and_then(|j| j.as_array()).into_iter().flatten() {
                    if let Some(id) = entry.get("job").and_then(|j| j.as_str()) {
                        self.job(id).absorb(entry);
                    }
                }
            }
            "ears.speech" => {
                let dropped = v.get("state").and_then(|s| s.as_str()) == Some("dropped");
                let target = match purpose {
                    "listen" => Some(&mut self.captions),
                    "pc" => Some(&mut self.pc),
                    _ => None,
                };
                match (target, dropped) {
                    (Some(c), true) => c.dropped(),
                    (Some(c), false) => c.heard(),
                    (None, true) => self.dictation_partial.clear(),
                    (None, false) => {}
                }
            }
            "ears.partial" => {
                let (stable, settling) = (words("stable"), words("settling"));
                match purpose {
                    "listen" => self.captions.partial(&stable, &settling),
                    "pc" => self.pc.partial(&stable, &settling),
                    "dictation" => self.dictation_partial = format!("{stable} {settling}").trim().to_string(),
                    _ => {}
                }
            }
            "ears.final" => {
                let text = words("text");
                let start = v.get("start").and_then(|s| s.as_f64()).unwrap_or(0.0);
                match purpose {
                    "listen" => self.captions.settled(start, &text),
                    "pc" => self.pc.settled(start, &text),
                    "dictation" => self.dictate(&text),
                    _ => {}
                }
            }
            "ears.error" => self.notice = Some(format!("A reading failed: {}", words("message"))),
            "ears.file" => {
                if let Some(id) = v.get("job").and_then(|j| j.as_str()).map(str::to_string) {
                    self.job(&id).absorb(v);
                }
            }
            "ears.reply" if v.get("ok").and_then(|o| o.as_bool()) == Some(false) => self.notice = Some(words("error")),
            _ => {}
        }
    }

    /// The job with this id, made on first sight, newest first.
    fn job(&mut self, id: &str) -> &mut ears::Job {
        let at = match self.jobs.iter().position(|j| j.id == id) {
            Some(at) => at,
            None => {
                self.jobs.insert(0, ears::Job { id: id.to_string(), ..ears::Job::default() });
                0
            }
        };
        &mut self.jobs[at]
    }

    /// Dictated words go at the end of the text, a space before them: the box for Sinai's, or the Dictation page's.
    fn dictate(&mut self, text: &str) {
        self.dictation_partial.clear();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.dictating_to_sinai {
            let glue = if self.typed.trim_end().is_empty() || self.typed.ends_with(char::is_whitespace) { "" } else { " " };
            self.typed = format!("{}{glue}{text}", self.typed);
            return;
        }
        let current = self.dictation.text();
        let glue = if current.trim_end().is_empty() || current.ends_with(char::is_whitespace) { "" } else { " " };
        self.dictation.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        self.dictation.perform(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(format!("{glue}{text}")))));
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            ears::subscription().map(Message::Ears),
            event::listen_with(files),
            event::listen_with(tab_keys),
            event::listen_with(escape_key),
        ];
        // Sinai's mind is a plug-in: the public build has none to connect to (installed.rs).
        if self.can(Capability::SinaiMind) {
            subscriptions.push(sinai::subscription().map(Message::Sinai));
        }
        let timers = self.timers();
        if timers.spin {
            subscriptions.push(iced::time::every(Duration::from_millis(33)).map(|_| Message::Tick));
        }
        if timers.services {
            subscriptions.push(probe::watch().map(Message::Looked));
        }
        if timers.speaker
            && let Some(folder) = self.voice.speaker_folder()
        {
            subscriptions.push(
                crate::live::Watch::new("speaker", vec![("the voiceprint".into(), crate::live::Source::Folder { path: folder, subtree: true })])
                    .subscription()
                    .map(Message::Speaker),
            );
        }
        if self.signin.through()
            && self.section == Section::Transcription
            && self.tab == Tab::Library
            && let Some(root) = library::dir()
        {
            subscriptions.push(
                crate::live::Watch::new("library", vec![("the library".into(), crate::live::Source::Folder { path: root, subtree: true })])
                    .subscription()
                    .map(Message::LibraryLive),
            );
        }
        subscriptions.push(self.signin.subscription().map(Message::SignIn));
        // The identity service's status, while the sign-in screen or the Account page shows it (change-only).
        if !self.signin.through() || self.section == Section::Account {
            subscriptions.push(self.signin.status_watch().map(Message::SignIn));
        }
        subscriptions.push(self.social.subscription().map(Message::Social));
        subscriptions.push(window::resize_events().map(|_| Message::Resized));
        if let Some(every) = timers.voice {
            subscriptions.push(iced::time::every(every).map(|_| Message::VoiceTick));
        }
        // The Stage captures only while it is on show, and only when it has a window to show or the chooser is open.
        if self.stage_visible() {
            let on_show = self.stage_on_show();
            if !on_show.is_empty() || self.stage_choosing {
                subscriptions.push(stage::subscription(on_show, self.own_window.unwrap_or(0) as i64).map(Message::Stage));
            }
        }
        if timers.held {
            subscriptions.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::HeldTick));
        }
        // Undo and redo in the creator, from the keyboard, unless a text box took the key.
        if self.section == Section::Sinai && self.creator.is_some() {
            subscriptions.push(event::listen_with(creator_keys));
        }
        if self.section == Section::Fleet {
            if let Some(fleet) = &self.fleet {
                subscriptions.push(fleet.subscription().map(Message::Fleet));
            }
        }
        if self.section == Section::Data {
            subscriptions.push(self.data.subscription().map(Message::Data));
        }
        if self.section == Section::Compute {
            subscriptions.push(self.compute.subscription().map(Message::Compute));
        }
        if self.section == Section::Lattice {
            subscriptions.push(self.lattice.subscription().map(Message::Lattice));
        }
        // The Models panel's watch and knocks, while it shows, or while Sinai's page talks to a model of the person's own.
        let own_on_show =
            self.signin.through() && self.section == Section::Sinai && matches!(self.answering(), crate::models::Answering::Own(_));
        subscriptions.push(self.models.subscription(own_on_show).map(Message::Models));
        // A harvest's progress is polled while it runs, whichever page is shown.
        subscriptions.push(self.research.subscription().map(Message::Research));
        // The dialogs Lattice's core asks for are answered whichever page is shown.
        subscriptions.push(self.lattice.background().map(Message::Lattice));
        if let Some(engine) = &self.engine {
            subscriptions.push(engine.watch().map(Message::EngineEnded));
        }
        // A Trust card's timers run whichever page is shown (an issuer it started is watched until it stops).
        for (i, card) in self.trust_cards.iter().enumerate() {
            subscriptions.push(card.page.subscription().with(i).map(|(i, m)| Message::TrustCard(i, m)));
        }
        if self.dock_changed {
            subscriptions.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::SaveDock));
        }
        if crate::frame_stats::enabled() {
            subscriptions.push(event::listen_raw(crate::frame_stats::frames));
        }
        if self.options.screenshot.is_some() && !self.shot_taken {
            subscriptions.push(iced::time::every(Duration::from_millis(self.options.after_ms.max(1))).map(|_| Message::Shoot));
        }
        Subscription::batch(subscriptions)
    }
}

/// How often a running voice enrolment's script is read: quickly while its page shows the sentence to say, else slowly
/// (the on-air bar elsewhere says only that it records).
const VOICE_ON_SHOW: Duration = Duration::from_millis(150);
const VOICE_ELSEWHERE: Duration = Duration::from_secs(1);

/// The window's timers that the page on show and the work under way decide (`App::timers`). Every message redraws the
/// whole window, so a timer runs only while what it moves is on show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timers {
    /// The spinners' tick, thirty a second: a spinner is on show and turning.
    pub spin: bool,
    /// The services' watch (`probe::watch`): the Overview or the Sinai page is open.
    pub services: bool,
    /// The voiceprint's folder, watched for changes (`live`).
    pub speaker: bool,
    /// A running voice enrolment's script, read this often.
    pub voice: Option<Duration>,
    /// A held act's countdown, every second.
    pub held: bool,
}

/// `work` on a thread of its own, awaited without holding the window's.
async fn off_thread<T: Send + 'static>(name: &'static str, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = std::thread::Builder::new().name(name.into()).spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.ok()
}

/// Read a receipt (or a test case) and its data, verify it with the pinned keys, and read what it states.
fn verify_files(receipt: &str, data: &str, key: &str, witness: &str) -> Result<(trust::Verdict, trust::Stated), String> {
    let found = trust::classify(&trust::read(Path::new(receipt))?);
    let (envelope, case_inputs) = match found {
        trust::Found::Envelope(e) => (e, None),
        trust::Found::Case { envelope, inputs, .. } => (envelope, inputs),
        _ => return Err(format!("{receipt} is not a receipt (alelyon.cne/v0) or a test case.")),
    };
    let inputs = if data.is_empty() { case_inputs } else { Some(trust::read(Path::new(data))?) };
    Ok((trust::verify(&envelope, inputs.as_ref(), key, witness), trust::stated(&envelope)))
}

/// Ctrl+Z undoes in the creator; Ctrl+Y or Ctrl+Shift+Z redoes.
fn creator_keys(event: iced::Event, status: event::Status, _window: window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key};
    let iced::Event::Keyboard(Event::KeyPressed { key: Key::Character(c), modifiers, .. }) = event else { return None };
    if status == event::Status::Captured || !modifiers.command() {
        return None;
    }
    match (c.as_str(), modifiers.shift()) {
        ("z", false) => Some(Message::Creator(face::creator::Msg::Undo)),
        ("y", _) | ("z", true) => Some(Message::Creator(face::creator::Msg::Redo)),
        _ => None,
    }
}

/// Tab and Shift+Tab move between fields, as everywhere else on Windows. A widget that uses Tab itself (an editor
/// indenting) captures it first, and then focus stays put.
fn tab_keys(event: iced::Event, status: event::Status, _window: window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    match event {
        iced::Event::Keyboard(Event::KeyPressed { key: Key::Named(Named::Tab), modifiers, .. })
            if status == event::Status::Ignored && !modifiers.command() && !modifiers.alt() =>
        {
            Some(Message::FocusMove(modifiers.shift()))
        }
        _ => None,
    }
}

/// Esc, captured or not: the Settings panel's search box takes the keyboard, and Esc must still close the panel.
fn escape_key(event: iced::Event, _status: event::Status, _window: window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    match event {
        iced::Event::Keyboard(Event::KeyPressed { key: Key::Named(Named::Escape), .. }) => Some(Message::Escape),
        _ => None,
    }
}

/// Files dragged over and dropped on the window.
fn files(event: iced::Event, _status: event::Status, _window: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::FileHovered(_)) => Some(Message::Hovering(true)),
        iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::Hovering(false)),
        iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::Dropped(path)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app() -> App {
        App::boot(Options::default()).0
    }

    #[test]
    fn tab_moves_focus_forward_and_shift_tab_back_unless_a_widget_used_it() {
        use iced::keyboard::{Event, Key, Location, Modifiers, key};
        let press = |modifiers: Modifiers| {
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(key::Named::Tab),
                modified_key: Key::Named(key::Named::Tab),
                physical_key: key::Physical::Code(key::Code::Tab),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let w = window::Id::unique();
        assert!(matches!(tab_keys(press(Modifiers::empty()), event::Status::Ignored, w), Some(Message::FocusMove(false))));
        assert!(matches!(tab_keys(press(Modifiers::SHIFT), event::Status::Ignored, w), Some(Message::FocusMove(true))));
        assert!(tab_keys(press(Modifiers::empty()), event::Status::Captured, w).is_none(), "an editor indenting keeps its Tab");
        assert!(tab_keys(press(Modifiers::CTRL), event::Status::Ignored, w).is_none(), "Ctrl+Tab is not focus");
    }

    /// The account menu's Settings opens the panel over the page and leaves the page where it was; Esc closes it,
    /// whatever has the keyboard (the panel's search box takes it).
    #[test]
    fn settings_open_over_the_page_and_esc_closes_them() {
        use iced::keyboard::{Event, Key, Location, Modifiers, key};
        let mut a = quiet();
        let _ = a.update(Message::Go(Section::Fleet));
        let _ = a.update(Message::SignIn(crate::signin::Msg::Menu(true)));
        let _ = a.update(Message::SignIn(crate::signin::Msg::Settings));
        assert_eq!(a.section, Section::Fleet, "no navigation away");
        assert_eq!(a.signin.settings, Some(crate::signin::settings::Pane::Account));
        assert!(!a.signin.menu_open);
        let _ = a.update(Message::SignIn(crate::signin::Msg::SettingsQuery("motion".into())));
        let esc = iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Named(key::Named::Escape),
            modified_key: Key::Named(key::Named::Escape),
            physical_key: key::Physical::Code(key::Code::Escape),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        });
        let pressed = escape_key(esc, event::Status::Captured, window::Id::unique()).expect("Esc even when captured");
        let _ = a.update(pressed);
        assert_eq!(a.signin.settings, None);
        assert!(a.signin.settings_query.is_empty(), "the search starts empty next time");
        assert_eq!(a.section, Section::Fleet);
        // Esc with the panel closed changes nothing here.
        let _ = a.update(Message::Escape);
        assert_eq!((a.section, a.signin.settings), (Section::Fleet, None));
    }

    /// The window past the sign-in screen (offline, as a `--screenshot` run opens it), on the Overview with its first look
    /// in, the ears not running: nothing loads.
    fn quiet() -> App {
        let mut a = App::boot(Options { screenshot: Some("x.png".into()), ..Options::default() }).0;
        a.link = Link::Unavailable("not running".into());
        a.probes = vec![probe::Reading::Down; probe::SERVICES.len()];
        a
    }

    #[test]
    fn an_idle_window_runs_no_fast_timer_on_any_page() {
        let mut a = quiet();
        for section in Section::ALL.into_iter().filter(|s| *s != Section::Lattice && *s != Section::Compute) {
            a.section = section;
            let t = a.timers();
            assert!(!t.spin, "{section:?}: no spinner turns");
            assert_eq!(t.voice, None);
            assert!(!t.held);
            assert_eq!(t.services, matches!(section, Section::Overview | Section::Sinai), "{section:?}: the services are watched where they show");
            assert_eq!(t.speaker, section == Section::Sinai, "{section:?}: the voiceprint's folder is watched where it shows");
        }
    }

    #[test]
    fn a_spinner_turns_only_on_the_page_that_shows_it() {
        let mut a = quiet();
        a.trust_checking = true;
        a.section = Section::Trust;
        assert!(a.timers().spin, "the receipt being checked, on the Trust page");
        a.section = Section::Overview;
        assert!(!a.timers().spin, "not while the Overview shows");
        a.trust_checking = false;
        a.library_loading = true;
        a.section = Section::Transcription;
        assert!(a.timers().spin);
        a.section = Section::Account;
        assert!(!a.timers().spin);
    }

    #[test]
    fn the_overview_turns_its_spinner_for_the_first_look_only_and_a_look_is_its_live_mark() {
        let mut a = quiet();
        assert!(!a.timers().spin, "the page's own looks (probe::watch) turn nothing");
        a.probes.clear();
        assert!(a.timers().spin, "until the first look is in, its tiles say Checking");
        let _ = a.update(Message::Looked(vec![probe::Reading::Up; probe::SERVICES.len()]));
        assert!(!a.timers().spin);
        assert!(a.services_fresh.words(crate::utc::now()).starts_with("Live · last change"), "{:?}", a.services_fresh);
    }

    #[test]
    fn the_rails_ears_mark_turns_on_every_page_while_the_link_looks() {
        let mut a = quiet();
        a.link = Link::Lost("the connection broke".into());
        for section in [Section::Overview, Section::Fleet, Section::Account] {
            a.section = section;
            assert!(a.timers().spin, "{section:?}");
        }
        a.link = Link::Unavailable("not running".into());
        assert!(!a.timers().spin);
        a.starting = true;
        assert!(a.timers().spin, "the engine this window started, still loading");
    }

    #[test]
    fn behind_the_sign_in_screen_only_its_own_spinner_ticks() {
        let mut a = app();
        assert!(!a.signin.through());
        // (a session kept on this PC would be resuming, with its own spinner)
        a.signin.busy = None;
        a.probes.clear();
        a.trust_checking = true;
        let t = a.timers();
        assert!(!t.spin && !t.services && !t.speaker, "nothing behind the sign-in screen draws: {t:?}");
    }

    #[test]
    fn a_folder_on_the_command_line_opens_the_lattice_ide_on_it() {
        let o = Options::parse(["--folder", r"C:\src\proj", "--file", r"src\main.rs"].map(String::from)).unwrap();
        assert_eq!(o.folder.as_deref(), Some(std::path::Path::new(r"C:\src\proj")));
        assert_eq!(o.file.as_deref(), Some("src/main.rs"));
        let (app, _) = App::boot(o);
        assert_eq!((app.section, app.lattice.tab), (Section::Lattice, lattice::Tab::Chat));
        assert_eq!(app.lattice.ide.startup, Some((r"C:\src\proj".to_string(), Some("src/main.rs".to_string()))));
        assert!(Options::parse(["--folder"].map(String::from)).is_err());
    }

    #[test]
    fn the_machines_acts_reach_the_loop_only_after_their_confirmations() {
        use crate::machine::Act;
        let mut app = app();
        // no loop: nothing is sent, nothing is kept to send later, and the person is told
        let _ = app.update(Message::MemoryAct(Act::Remember(true)));
        assert!(app.memory_note.is_some());
        let (tx, rx) = std::sync::mpsc::channel();
        app.sinai_commands = Some(sinai::Commands::into(tx));
        let sent = |rx: &std::sync::mpsc::Receiver<String>| -> Vec<serde_json::Value> {
            rx.try_iter().map(|t| serde_json::from_str(&t).unwrap()).collect()
        };
        let _ = app.update(Message::MemoryAct(Act::Restore { batch: "b7".into() }));
        assert_eq!(sent(&rx), [json!({"type": "memory_control", "action": "restore", "batch": "b7"})], "one press");
        assert!(app.memory_note.is_none());
        let _ = app.update(Message::MemoryAct(Act::Erase { id: Some("e1".into()), what: "it".into() }));
        assert!(sent(&rx).is_empty() && app.memory_pending.is_some(), "an erase asks first");
        let _ = app.update(Message::MemoryCancel);
        let _ = app.update(Message::MemoryConfirm);
        assert!(sent(&rx).is_empty(), "a cancelled erase is never sent");
        let _ = app.update(Message::MemoryAct(Act::Purge { batch: None, what: String::new() }));
        let _ = app.update(Message::MemoryConfirm);
        assert!(sent(&rx).is_empty() && app.memory_pending.is_some(), "emptying the trash asks twice");
        let _ = app.update(Message::MemoryConfirm);
        assert_eq!(sent(&rx), [json!({"type": "memory_control", "action": "purge"})]);
        assert!(app.memory_pending.is_none());
    }

    #[test]
    fn words_go_where_their_purpose_says() {
        let mut a = app();
        a.event(&json!({"type": "ears.partial", "purpose": "listen", "stable": "hello", "settling": "there"}));
        assert_eq!((a.captions.stable.as_str(), a.captions.settling.as_str()), ("hello", "there"));
        a.event(&json!({"type": "ears.final", "purpose": "listen", "text": " Hello there.", "start": 1.5}));
        assert_eq!(a.captions.lines, [(1.5, "Hello there.".to_string())]);
        assert!(!a.captions.in_progress());
        a.event(&json!({"type": "ears.final", "purpose": "pc", "text": "From a video.", "start": 3.0}));
        assert_eq!(a.pc.text(), "From a video.");
        a.event(&json!({"type": "ears.final", "purpose": "dictation", "text": "Dear Sam,", "start": 0.0}));
        a.event(&json!({"type": "ears.final", "purpose": "dictation", "text": "thank you.", "start": 2.0}));
        assert_eq!(a.dictation.text().trim_end(), "Dear Sam, thank you.");
    }

    #[test]
    fn a_dropped_utterance_clears_its_line_in_progress() {
        let mut a = app();
        a.event(&json!({"type": "ears.speech", "purpose": "listen", "state": "start"}));
        assert!(a.captions.hearing);
        a.event(&json!({"type": "ears.speech", "purpose": "listen", "state": "dropped"}));
        assert!(!a.captions.in_progress());
    }

    #[test]
    fn file_jobs_are_followed_from_their_events_and_the_state() {
        let mut a = app();
        a.event(&json!({"type": "ears.state", "mic": {"on": false}, "pc": {"on": false},
            "recognizer": {"state": "ready", "model": "m"}, "jobs": [{"job": "job-1", "path": "C:/a.wav", "state": "decoding", "progress": 0.0}]}));
        assert_eq!(a.jobs.len(), 1);
        a.event(&json!({"type": "ears.file", "job": "job-1", "state": "transcribing", "progress": 0.5}));
        assert!(a.jobs[0].running());
        a.link = Link::Connected;
        a.signin = crate::signin::State::boot(false).0;
        a.section = Section::Transcription;
        assert!(a.spinning(), "a running job turns the spinners on its page");
        a.section = Section::Fleet;
        assert!(!a.spinning(), "and draws nothing while another page shows");
        a.section = Section::Transcription;
        a.event(&json!({"type": "ears.file", "job": "job-1", "state": "done", "progress": 1.0, "outputs": ["C:/a.txt"]}));
        assert!(!a.jobs[0].running());
        assert!(!a.spinning());
    }

    /// The account panel's web pages. With Lattice's browser chosen, a link waits for the
    /// Lattice services (a test runs none) and nothing goes to the person's own browser; a link's own button, or your
    /// browser chosen, goes there (a test build records the address and launches nothing); an address that is not a web
    /// page is never handed to the shell.
    #[test]
    fn the_account_panels_web_pages_open_where_settings_says() {
        use crate::signin::web::{WebPages, opened};
        use crate::signin::Msg as SignIn;
        let mut a = app();
        a.signin.web = WebPages::Lattice;
        let terms = "https://www.alelyon.com/legal/terms/?test=panel-lattice";
        let _ = a.update(Message::SignIn(SignIn::Open(terms.into())));
        assert_eq!(a.lattice.web_waiting, [terms]);
        assert!(a.lattice.starting, "the services start for it");
        assert!(!opened(terms));
        let privacy = "https://www.alelyon.com/legal/privacy/?test=panel-lattice";
        let _ = a.update(Message::SignIn(SignIn::Open(privacy.into())));
        assert_eq!(a.lattice.web_waiting, [terms, privacy], "one start, both waiting");
        let account = "https://id.api.alelyon.com/account?test=panel-button";
        let _ = a.update(Message::SignIn(SignIn::OpenYours(account.into())));
        assert!(opened(account), "a link's own button");
        a.signin.web = WebPages::Yours;
        let security = "https://id.api.alelyon.com/security?test=panel-yours";
        let _ = a.update(Message::SignIn(SignIn::Open(security.into())));
        assert!(opened(security));
        assert_eq!(a.lattice.web_waiting.len(), 2, "Lattice's page was not asked");
        let file = "file:///C:/Windows/System32/calc.exe";
        let _ = a.update(Message::SignIn(SignIn::Open(file.into())));
        assert!(!opened(file));
        assert!(a.signin.error.as_deref().unwrap_or("").contains("not a web address"), "{:?}", a.signin.error);
    }

    /// A web page Lattice's browser cannot open goes to the person's own browser, and the window says why: the waiting
    /// pages when the services fail to start, a later page once they have failed, and a page the browser refuses (an
    /// address off the public web, such as an identity service on this PC).
    #[test]
    fn a_web_page_lattices_browser_cannot_open_goes_to_your_browser_and_says_why() {
        use crate::signin::web::{WebPages, opened};
        use crate::signin::Msg as SignIn;
        let mut a = app();
        a.signin.web = WebPages::Lattice;
        let reset = "https://id.api.alelyon.com/reset?test=fallback-start";
        let _ = a.update(Message::SignIn(SignIn::Open(reset.into())));
        assert!(!opened(reset));
        let _ = a.update(Message::Lattice(crate::lattice::Msg::Started(Err("no state folder".into()))));
        assert!(opened(reset));
        assert!(a.lattice.web_waiting.is_empty());
        let notice = a.notice.clone().unwrap_or_default();
        assert!(notice.contains("no state folder") && notice.contains("your own browser"), "{notice}");
        let later = "https://www.alelyon.com/legal/privacy/?test=fallback-later";
        let _ = a.update(Message::SignIn(SignIn::Open(later.into())));
        assert!(opened(later), "once they failed, straight to your browser");
        let local = "http://127.0.0.1:8080/account?test=fallback-refused";
        let refused = Err("That is an address on this PC.".to_string());
        let _ = a.update(Message::Lattice(crate::lattice::Msg::WebOpened(local.into(), refused)));
        assert!(opened(local));
        assert!(a.notice.clone().unwrap_or_default().contains("this PC"), "{:?}", a.notice);
    }

    #[test]
    fn the_on_air_bar_names_what_is_listening() {
        let mut a = app();
        a.link = Link::Connected;
        a.event(&json!({"type": "ears.state", "mic": {"on": true, "device": "Headset", "listen": true, "dictation": false},
            "pc": {"on": true, "device": "Speakers"}, "recognizer": {"state": "ready"}}));
        assert_eq!(a.on_air(), (Some(("Headset".to_string(), "captions")), Some("Speakers".to_string())));
        a.link = Link::Lost("cut".into());
        assert_eq!(a.on_air(), (None, None), "a lost link claims nothing it cannot see");
    }

    #[test]
    fn a_command_without_a_link_says_so() {
        let mut a = app();
        let _ = a.update(Message::ToggleCaptions);
        assert!(a.notice.as_deref().unwrap_or("").contains("not connected"));
    }

    #[test]
    fn sinais_buttons_send_the_loops_own_commands() {
        let mut a = app();
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = a.update(Message::Sinai(sinai::Feed::Connected(sinai::Commands::into(tx))));
        let sent = |rx: &std::sync::mpsc::Receiver<String>| -> Vec<serde_json::Value> {
            rx.try_iter().map(|s| serde_json::from_str(&s).unwrap()).collect()
        };
        let _ = a.update(Message::Typed("  Sinai, what time is it?  ".into()));
        let _ = a.update(Message::SayTyped);
        assert_eq!(sent(&rx), [json!({"type": "text", "text": "Sinai, what time is it?"})]);
        assert!(a.typed.is_empty(), "the box clears once the words are sent");
        let _ = a.update(Message::SayTyped);
        assert!(sent(&rx).is_empty(), "an empty box sends nothing");
        let _ = a.update(Message::Decide("4".into(), true));
        let _ = a.update(Message::Decide("5".into(), false));
        assert_eq!(sent(&rx), [json!({"type": "decide", "id": "4", "answer": "yes"}), json!({"type": "decide", "id": "5", "answer": "no"})]);
        // Sinai's microphone on air: the on-air bar's button closes it.
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "mic", "on": true, "attending": false}))));
        assert_eq!(a.sinai_on_air(), Some("waiting for its name"));
        let _ = a.update(Message::OffAir);
        assert_eq!(sent(&rx), [json!({"type": "listen", "on": false})]);
        // and when the loop goes away, nothing it said is claimed as still on air
        let _ = a.update(Message::Sinai(sinai::Feed::Lost));
        assert_eq!(a.sinai_on_air(), None);
    }

    #[test]
    fn how_sinai_listens_is_asked_again_when_the_loop_says_its_microphone_changed_not_on_a_timer() {
        let mut a = app();
        a.section = Section::Sinai;
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = a.update(Message::Sinai(sinai::Feed::Connected(sinai::Commands::into(tx))));
        assert!(a.listening_asked, "asked when the loop connects");
        let _ = a.update(Message::Listening(Err("stand-in".into())));
        assert!(!a.listening_asked);
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "state", "s": "idle"}))));
        assert!(!a.listening_asked, "other news from the loop asks nothing");
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "mic", "on": true, "attending": false}))));
        assert!(a.listening_asked, "the microphone changed: asked again at once, though the last answer is seconds old");
    }

    #[test]
    fn a_held_click_is_moved_then_allowed_and_nothing_unsendable_leaves() {
        let mut a = app();
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = a.update(Message::Sinai(sinai::Feed::Connected(sinai::Commands::into(tx))));
        let sent = |rx: &std::sync::mpsc::Receiver<String>| -> Vec<serde_json::Value> {
            rx.try_iter().map(|s| serde_json::from_str(&s).unwrap()).collect()
        };
        let held = json!({"type": "held", "items": [
            {"id": "h1", "question": "click Send?", "what": "click Send at (412, 300)", "verdict": "coordinate with a human",
             "reason": "unsure", "kind": "click", "fields": {"name": "Send", "x": 412, "y": 300}, "seconds_left": 117.4},
            {"id": "h2", "question": "press ctrl+w?", "what": "press ctrl+w", "verdict": "coordinate with a human",
             "reason": "it closes a tab", "kind": "press", "fields": {"key": "ctrl+w"}, "seconds_left": 90.0}]});
        let _ = a.update(Message::Sinai(sinai::Feed::Event(held.clone())));
        // a pressed key is not moved: the loop would press the original key whatever was asked
        let _ = a.update(Message::ChangeHeld("h2".into()));
        assert_eq!(a.held_change, None);
        // a click opens at its own x and y
        let _ = a.update(Message::ChangeHeld("h1".into()));
        assert_eq!(a.held_change, Some(("h1".into(), "412".into(), "300".into())));
        // not two whole numbers: nothing is sent (the loop would drop the act), and the change stays open
        let _ = a.update(Message::ChangeX("420.5".into()));
        let _ = a.update(Message::AllowChanged);
        assert!(sent(&rx).is_empty());
        assert!(a.held_change.is_some());
        let _ = a.update(Message::ChangeX("420".into()));
        let _ = a.update(Message::AllowChanged);
        assert_eq!(sent(&rx), [json!({"type": "decide", "id": "h1", "answer": "yes", "changes": {"x": 420, "y": 300}})]);
        assert_eq!(a.held_change, None);
        // a change the loop stopped holding closes with it
        let _ = a.update(Message::ChangeHeld("h1".into()));
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "held", "items": []}))));
        assert_eq!(a.held_change, None);
        let _ = a.update(Message::AllowChanged);
        assert!(sent(&rx).is_empty(), "nothing is sent for an act no longer held");
        // and allowing or refusing it as it is closes an open change
        let _ = a.update(Message::Sinai(sinai::Feed::Event(held)));
        let _ = a.update(Message::ChangeHeld("h1".into()));
        let _ = a.update(Message::Decide("h1".into(), false));
        assert_eq!(a.held_change, None);
        assert_eq!(sent(&rx), [json!({"type": "decide", "id": "h1", "answer": "no"})]);
    }

    #[test]
    fn grants_are_written_only_after_review_and_never_while_sinais_hands_are_armed() {
        let dir = std::env::temp_dir().join(format!("centcom-app-grants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("authority.json");
        let mut a = app();
        let (tx, _rx) = std::sync::mpsc::channel();
        let _ = a.update(Message::Sinai(sinai::Feed::Connected(sinai::Commands::into(tx))));
        // until the loop names its file there is nothing to edit
        let _ = a.update(Message::GrantsEdit);
        assert!(a.grants_draft.is_none() && a.grants_note.as_deref().is_some_and(|n| n.contains("has not named it")));
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "standing", "grants": [], "complaints": [], "path": path.display().to_string()}))));
        let _ = a.update(Message::GrantsEdit);
        assert!(a.grants_draft.as_ref().is_some_and(|d| d.grants.is_empty()), "no file yet: an empty list");
        let _ = a.update(Message::GrantAdd);
        let _ = a.update(Message::GrantField(0, grants::Field::IssuedBy, "Thomas Lacy".into()));
        let _ = a.update(Message::GrantField(0, grants::Field::Recipients, "press@example.com".into()));
        let _ = a.update(Message::GrantsReview);
        assert!(matches!(&a.grants_review, Some(Ok((_, c))) if c.adds.iter().any(|l| l.contains("is new"))), "{:?}", a.grants_review);
        // an edit withdraws the review: what is confirmed is what is on show
        let _ = a.update(Message::GrantField(0, grants::Field::Note, "press list only".into()));
        assert!(a.grants_review.is_none());
        let _ = a.update(Message::GrantsReview);
        // the loop has not said whether the hands are armed: nothing is written
        assert!(a.grants_blocked().is_some());
        let _ = a.update(Message::GrantsWrite);
        assert!(!path.exists());
        // armed: nothing is written either
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "working", "handle": 4242, "armed": true, "title": "Alelyon"}))));
        let _ = a.update(Message::GrantsWrite);
        assert!(!path.exists() && a.grants_note.as_deref().is_some_and(|n| n.contains("armed on \"Alelyon\"")), "{:?}", a.grants_note);
        // disarmed: written, as reviewed
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "working", "handle": 0, "armed": false}))));
        assert_eq!(a.grants_blocked(), None);
        let _ = a.update(Message::GrantsWrite);
        let on_disk: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk[0]["recipients"], json!(["press@example.com"]));
        assert_eq!(on_disk[0]["note"], json!("press list only"));
        assert!(a.grants_draft.is_none() && a.grants_note.as_deref().is_some_and(|n| n.starts_with("Written to")));
        // with the loop gone, Sinai acts nowhere: writing is not blocked
        let _ = a.update(Message::Sinai(sinai::Feed::Lost));
        assert_eq!(a.grants_blocked(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dictation_can_fill_the_box_for_sinai_and_nothing_is_sent_until_send() {
        let mut a = app();
        let (ears_tx, ears_rx) = std::sync::mpsc::channel();
        let (loop_tx, loop_rx) = std::sync::mpsc::channel();
        a.link = Link::Connected;
        a.commands = Some(ears::Commands::into(ears_tx));
        let _ = a.update(Message::Sinai(sinai::Feed::Connected(sinai::Commands::into(loop_tx))));
        let _ = a.update(Message::DictateToSinai);
        let sent: Vec<serde_json::Value> = ears_rx.try_iter().map(|s| serde_json::from_str(&s).unwrap()).collect();
        assert_eq!(sent, [json!({"cmd": "dictate", "on": true})]);
        a.event(&json!({"type": "ears.state", "mic": {"on": true, "device": "Headset", "listen": false, "dictation": true}, "pc": {"on": false}, "recognizer": {"state": "ready"}}));
        a.event(&json!({"type": "ears.final", "purpose": "dictation", "text": "Sinai, what is", "start": 0.0}));
        a.event(&json!({"type": "ears.final", "purpose": "dictation", "text": "on my calendar?", "start": 1.0}));
        assert_eq!(a.typed, "Sinai, what is on my calendar?");
        assert!(a.dictation.text().trim().is_empty(), "the Dictation page's text is untouched");
        assert!(loop_rx.try_iter().next().is_none(), "nothing reaches Sinai before Send");
        let _ = a.update(Message::DictateToSinai);
        let sent: Vec<serde_json::Value> = ears_rx.try_iter().map(|s| serde_json::from_str(&s).unwrap()).collect();
        assert_eq!(sent, [json!({"cmd": "dictate", "on": false})], "the same button stops it");
        let _ = a.update(Message::SayTyped);
        let said: Vec<serde_json::Value> = loop_rx.try_iter().map(|s| serde_json::from_str(&s).unwrap()).collect();
        assert_eq!(said, [json!({"type": "text", "text": "Sinai, what is on my calendar?"})]);
    }

    #[test]
    fn a_dropped_test_case_fills_in_its_keys_and_verifies() {
        let mut a = app();
        a.section = Section::Trust;
        let case = std::path::PathBuf::from(alelyon_verify::TEST_VECTORS).join("golden-authenticated.json");
        let _ = a.update(Message::Dropped(case.clone()));
        assert_eq!(a.trust_receipt, case.display().to_string());
        assert_eq!(a.trust_key.len(), 64, "the case's pinned key is shown in its field");
        let (verdict, stated) = verify_files(&a.trust_receipt, &a.trust_data, &a.trust_key, &a.trust_witness).unwrap();
        // authentic and its inputs match; not passed, as the public build does not replay it (installed.rs)
        assert_eq!(verdict.checks[0], ("authenticity", Some(true)), "{verdict:?}");
        assert_eq!(verdict.checks[1], ("inputs", Some(true)), "{verdict:?}");
        assert!(!verdict.ok && !verdict.replayed, "{verdict:?}");
        assert_eq!(stated.program, "show mean(price(\"SYN\"))");
        let forged = std::path::PathBuf::from(alelyon_verify::TEST_VECTORS).join("forgery-bad-signature.json");
        let _ = a.update(Message::Dropped(forged));
        let (verdict, _) = verify_files(&a.trust_receipt, &a.trust_data, &a.trust_key, &a.trust_witness).unwrap();
        assert!(!verdict.ok);
        assert!(verdict.reasons.iter().any(|r| r == "bad-signature"), "{:?}", verdict.reasons);
    }

    #[test]
    fn a_verdict_is_shown_only_for_the_inputs_it_was_asked_about() {
        let mut a = app();
        a.section = Section::Trust;
        let case = std::path::PathBuf::from(alelyon_verify::TEST_VECTORS).join("golden-authenticated.json");
        let _ = a.update(Message::Dropped(case.clone()));
        let receipt = case.display().to_string();
        let asked = a.trust_asked;
        let _ = a.update(Message::TrustVerified(asked, verify_files(&receipt, "", "", "")));
        assert!(a.trust_result.is_some(), "a verdict for the inputs shown is shown");
        let _ = a.update(Message::TrustKey("0".repeat(64)));
        assert!(a.trust_result.is_none(), "editing the key clears the verdict that stood for the old key");
        a.trust_checking = true;
        let _ = a.update(Message::TrustVerified(asked, verify_files(&receipt, "", "", "")));
        assert!(a.trust_result.is_none(), "a verdict asked before the edit is dropped");
        assert!(!a.trust_checking, "and the spinner stops");
    }

    #[test]
    fn the_command_line_is_checked() {
        let o = Options::parse(["--section", "words", "--tab", "files", "--screenshot", "x.png"].map(String::from)).unwrap();
        assert_eq!((o.section, o.tab, o.after_ms), (Some(Section::Transcription), Some(Tab::Files), 2500));
        assert!(Options::parse(["--section".to_string(), "markets".to_string()]).is_err(), "no Markets section");
        assert!(Options::parse(["--fly".to_string()]).is_err());
        assert!(Options::parse(["--section", "fleet", "--tab", "landing"].map(String::from)).is_err(), "no Fleet tab without its page");
        let fleet = Registry::new()
            .with(plugin::Plugin::new(FLEET, "Fleet", Slot::SectionPage, || Box::new(Card::default())).with_tabs(&["landing"]));
        let o = Options::parse_with(["--section", "fleet", "--tab", "landing"].map(String::from), &fleet).unwrap();
        assert_eq!((o.section, o.tab, o.page.as_deref()), (Some(Section::Fleet), None, Some("landing")));
        let o = Options::parse(["--section", "compute", "--tab", "training"].map(String::from)).unwrap();
        assert_eq!((o.section, o.page.as_deref()), (Some(Section::Compute), Some("training")));
        assert!(Options::parse(["--tab", "emulator"].map(String::from)).is_err(), "no plug-in's tab without its plug-in");
        let o = Options::parse(["--section", "data", "--tab", "relay/receipt"].map(String::from)).unwrap();
        assert_eq!((o.section, o.page.as_deref()), (Some(Section::Data), Some("relay/receipt")));
        assert!(Options::parse(["--tab", "accounts"].map(String::from)).is_err(), "no store outside the list");
    }

    #[derive(Default)]
    struct Card(u32);

    #[derive(Clone, Debug)]
    struct Bump;

    impl plugin::Page for Card {
        type Msg = Bump;

        fn open(&mut self) -> Task<Bump> {
            self.0 += 10;
            Task::none()
        }

        fn update(&mut self, _: Bump) -> Task<Bump> {
            self.0 += 1;
            Task::none()
        }

        fn view(&self, _: f32) -> iced::Element<'_, Bump> {
            iced::widget::space().into()
        }

        fn busy(&self) -> bool {
            self.0 > 0
        }
    }

    #[test]
    fn the_public_window_carries_no_plugin_and_a_registry_adds_its_cards_and_tabs() {
        let public = app();
        assert!(public.trust_cards.is_empty() && public.compute.plugins.is_empty() && public.plugin_features.is_empty());
        assert!(public.fleet.is_none(), "the public window carries no Fleet page");
        assert!(public.can(Capability::VerifyReceipts));
        for c in [Capability::Friends, Capability::ComputeEmulator, Capability::ComputeKitJobs, Capability::IssueReceipts, Capability::DataTrust] {
            assert!(!public.can(c), "{c:?} is absent from the public window, signed out");
        }
        let registry = Registry::new()
            .with(plugin::Plugin::new("issue", "Issue", Slot::TrustCard, || Box::new(Card::default())))
            .with(plugin::Plugin::new("emulator", "Emulator", Slot::ComputeTab, || Box::new(Card::default())))
            .with(plugin::Plugin::new(FLEET, "Fleet", Slot::SectionPage, || Box::new(Card::default())));
        let o = Options::parse_with(["--section", "trust"].map(String::from), &registry).unwrap();
        let (mut a, _) = App::boot_with(o, &registry);
        assert_eq!((a.trust_cards.len(), a.compute.plugins.len()), (1, 1));
        assert!(a.fleet.is_some(), "a registry with the Fleet page carries it");
        assert!(a.can(Capability::IssueReceipts) && a.can(Capability::ComputeEmulator) && !a.can(Capability::DataTrust));
        assert!(a.trust_cards[0].page.busy(), "the Trust page opened at boot, so its card was told");
        let _ = a.update(Message::TrustCard(0, plugin::Message::new(Bump)));
        let _ = a.update(Message::TrustCard(3, plugin::Message::new(Bump)));
        let o = Options::parse_with(["--section", "compute", "--tab", "emulator"].map(String::from), &registry).unwrap();
        assert_eq!(o.page.as_deref(), Some("emulator"));
        let (b, _) = App::boot_with(o, &registry);
        assert_eq!(b.compute.tab, compute::Tab::Plugin(0));
    }

    /// One click on a gap hands it to Sinai: the page opens and, with nobody answering yet, the brief waits in the
    /// message box (the public build's case); the Research page marks the gap at once.
    #[test]
    fn a_gap_handed_to_sinai_opens_its_page_with_the_brief() {
        let mut a = quiet();
        let _ = a.update(Message::Go(Section::Research));
        let gap = alelyon_research::store::StoredGap {
            subject: "s".into(),
            kind: alelyon_research::gaps::Kind::Young,
            rank: 0,
            score: 1.0,
            headline: "\"x, y\" is young".into(),
            detail: "d".into(),
            terms: vec![],
            works: vec![],
            threads: vec![],
            dismissed: false,
            pursued: None,
            findings: vec![],
        };
        a.research.gaps = Some(Ok(vec![gap.clone()]));
        assert_eq!(a.answering(), crate::models::Answering::Nobody);
        let _ = a.update(Message::Research(crate::research::Msg::Pursue(gap.clone())));
        assert_eq!(a.section, Section::Sinai);
        assert_eq!(a.models.own.typed, crate::research::pursue::brief(&gap));
        assert!(a.notice.as_deref().is_some_and(|n| n.contains("Nobody answers")));
        assert!(matches!(&a.research.gaps, Some(Ok(g)) if g[0].pursued.is_some()));
    }

    /// Where Sinai's loop says it works gaps itself, the gap goes as a `research_gap` request, not as words; an older
    /// loop, or a person who chose their own model instead, gets the brief.
    #[test]
    fn a_loop_that_works_gaps_is_sent_the_gap_itself() {
        let mut a = quiet();
        a.plugin_ids = vec![alelyon_plugin_api::SINAI_MIND];
        assert_eq!(a.answering(), crate::models::Answering::Mind);
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = a.update(Message::Sinai(sinai::Feed::Connected(sinai::Commands::into(tx))));
        let sent = |rx: &std::sync::mpsc::Receiver<String>| -> Vec<serde_json::Value> {
            rx.try_iter().map(|s| serde_json::from_str(&s).unwrap()).collect()
        };
        let _ = sent(&rx);
        let gap = alelyon_research::store::StoredGap {
            subject: "s".into(),
            kind: alelyon_research::gaps::Kind::Young,
            rank: 0,
            score: 1.0,
            headline: "h".into(),
            detail: "d".into(),
            terms: vec![],
            works: vec![],
            threads: vec![],
            dismissed: false,
            pursued: None,
            findings: vec![],
        };
        let _ = a.update(Message::Research(crate::research::Msg::Pursue(gap.clone())));
        assert_eq!(sent(&rx), [json!({"type": "text", "text": crate::research::pursue::brief(&gap)})], "an older loop gets words");
        let _ = a.update(Message::Sinai(sinai::Feed::Event(json!({"type": "abilities", "research_gap": true}))));
        let _ = a.update(Message::Research(crate::research::Msg::Pursue(gap.clone())));
        assert_eq!(sent(&rx), [json!({"type": "research_gap", "subject": "s", "headline": "h"})]);
        assert_eq!(a.section, Section::Sinai);
        let _ = a.update(Message::Sinai(sinai::Feed::Lost));
        assert!(!a.sinai_works_gaps, "a new loop says again what it can");
    }
}
