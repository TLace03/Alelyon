//! The Lattice page: the native Lattice's agent chat as a Cursor-style Chat and IDE (`ide/`, since
//! 2026-10-07), its runs and traces, and the models behind them (one consolidated application: the chat core's
//! window is this section).
//!
//! Nothing here runs until the page is first opened: then the chat core and the runs service start on worker threads
//! of their own ([`core::Services`]). Every call that can touch the disk or the network runs there, never on the
//! window's thread, with a spinner while it does. A conversation's events arrive through the core's follow stream,
//! batched at least 80 ms apart; nothing wakes the page while nothing happens.
//!
//! What the chat can do is the core's to decide, not this page's: a command, a trust, a keep of an authority file or
//! the first send off this computer each open a dialog the core asks for itself, and only Confirm in it lets the
//! action go on (`core.rs`).

pub mod brain;
pub mod chat;
pub mod core;
pub mod digest;
pub mod foundry;
pub mod foundry_store;
pub mod ide;
pub mod morphometry;
pub mod morphometry_store;
pub mod probes;
pub mod stages;
pub mod obligations;
pub mod system;
pub mod training;
mod view;

use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{BoxStream, StreamExt};
use iced::widget::text_editor;
use iced::{Subscription, Task};

use lattice_app::runstate::RunState;
use lattice_app::spans::{SpanTree, visible_rows};
use lattice_app::wfmodel::{WfRow, build_rows};
use lattice_protocol::chat::{ChatChoice, LocalRuntime, Shown};
use lattice_protocol::conversation::{
    Accepted, AgentChatService, ChangeSet, ChatList, ConversationEvent, CoreStatus, Decision, FileDiff, MatchScope, Mode,
    ReviewOp, ReviewOutcome, SendRequest, Snapshot, TrustState, WorkspaceView,
};
use lattice_protocol::{AgentInfo, ModelChoice, RunDetail, RunSummary, StartRun};

pub use view::view;

use self::core::{Ask, Services};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// The Chat and IDE.
    Chat,
    Runs,
    Models,
    Training,
    Morphometry,
    Foundry,
    System,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Chat,
        Tab::Runs,
        Tab::Models,
        Tab::Training,
        Tab::Morphometry,
        Tab::Foundry,
        Tab::System,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Chat => "Chat and IDE",
            Tab::Runs => "Agent runs and traces",
            Tab::Models => "Models",
            Tab::Training => "Training Studio",
            Tab::Morphometry => "Morphometry",
            Tab::Foundry => "Foundry",
            Tab::System => "Stack and Action Gate",
        }
    }

    /// The tab `--tab` names: `chat` (or `code`, the editor being part of it now), `runs`, `models`, `training`,
    /// `morphometry`, `foundry` or `system`.
    pub fn from_name(name: &str) -> Option<Tab> {
        match name {
            "chat" | "ide" | "code" => Some(Tab::Chat),
            "runs" => Some(Tab::Runs),
            "models" => Some(Tab::Models),
            "training" => Some(Tab::Training),
            "morphometry" => Some(Tab::Morphometry),
            "foundry" => Some(Tab::Foundry),
            "system" => Some(Tab::System),
            _ => None,
        }
    }
}

/// One model endpoint as the Models tab lists it: never a key's value, only whether the key it names is present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointRow {
    pub label: String,
    pub kind: &'static str,
    pub model: String,
    pub local: bool,
    pub ready: bool,
    pub status: String,
    pub key_name: String,
    pub builtin: bool,
    /// Where the key it names is found, in words ("Credential Manager", "an env file", ...), never its value.
    pub key_source: Option<String>,
    /// The row as the registry holds it (before the managed server's model is projected onto it), for editing.
    pub saved: lattice_core::registry::ModelEndpoint,
}

/// The endpoint being edited: its fields as typed, and the key typed for it (sent to Credential Manager on save, then
/// forgotten here).
#[derive(Clone, Debug, Default)]
pub struct EndpointForm {
    /// `None` for a new endpoint, else the id being edited (ids are not renamed).
    pub editing: Option<String>,
    pub builtin: bool,
    pub id: String,
    pub label: String,
    pub anthropic: bool,
    pub base_url: String,
    pub model: String,
    pub key_name: String,
    pub enabled: bool,
    pub note: String,
    pub key: String,
    /// Save was pressed: the summary waits for Confirm.
    pub confirming: bool,
}

/// What a confirmed edit does, off the window's thread.
#[derive(Clone, Debug)]
pub enum RegistryAct {
    Save { endpoint: Box<lattice_core::registry::ModelEndpoint>, key: Option<String> },
    Remove { id: String },
    ForgetKey { name: String },
}

/// The registry as read: where it lives, its rows, and what could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub path: String,
    pub rows: Vec<EndpointRow>,
    pub issues: Vec<String>,
}

/// The Training Studio tab: a workspace folder opened by its path, or a new one created, its document pool edited
/// and plans saved, as the Python Training Studio does them. A trainer's live metrics are the Compute page's Training
/// tab (`compute/train.rs`, the same Python reader ported), not repeated here.
pub struct Training {
    pub workspace_path: String,
    pub workspace: Option<Result<training::Workspace, String>>,
    pub opening: bool,
    /// A new workspace: the folder it goes in and its name.
    pub new_parent: String,
    pub new_name: String,
    /// The document to add and its attribution.
    pub doc_path: String,
    pub doc_source: String,
    pub doc_license: String,
    pub doc_group: String,
    /// Where a document snapshot is exported.
    pub export_path: String,
    /// The plan being chosen: a preset and its experts ("layer:expert").
    pub preset: &'static str,
    pub experts: std::collections::BTreeSet<String>,
    /// A write under way, and what the last one said.
    pub writing: bool,
    pub said: Option<Result<String, String>>,
}

impl Default for Training {
    fn default() -> Training {
        Training {
            workspace_path: String::new(),
            workspace: None,
            opening: false,
            new_parent: String::new(),
            new_name: String::new(),
            doc_path: String::new(),
            doc_source: String::new(),
            doc_license: String::new(),
            doc_group: String::new(),
            export_path: String::new(),
            preset: "s",
            experts: Default::default(),
            writing: false,
            said: None,
        }
    }
}

/// What a Training Studio write does, off the window's thread.
#[derive(Clone, Debug)]
pub enum TrainingAct {
    Create(std::path::PathBuf),
    Add { file: std::path::PathBuf, source: String, license: String, group: String },
    Remove(String),
    SavePlan { preset: &'static str, experts: Vec<String> },
    Export(std::path::PathBuf),
}

/// A folder name the Python Studio accepts for a new workspace: not `.` or `..`, and no path separator or character
/// Windows refuses in a name.
pub fn simple_folder_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.chars().any(|c| "/\\:*?\"<>|".contains(c))
}

/// The System tab's reading.
#[derive(Clone, Debug)]
pub struct System {
    pub stack: Vec<system::StackRow>,
    pub ledger: Result<system::Ledger, String>,
    /// Each session's obligations, replayed from the ledger (`derive_evidence`), in first-seen order.
    pub evidence: Vec<(String, obligations::Evidence)>,
}

/// How long a dialog ignores input after it appears (the core's CP1).
pub const DIALOG_QUIET: Duration = lattice_core::ports::IGNORE_INPUT_FOR;
/// How often the runs list is read again while a run is working and the tab is shown.
const RUNS_POLL: Duration = Duration::from_secs(2);

/// A dialog the core asked for, shareable through messages (an [`Ask`] answers once and is not `Clone`).
#[derive(Clone)]
pub struct AskCell(Arc<Mutex<Option<Ask>>>);

impl std::fmt::Debug for AskCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AskCell")
    }
}

/// The dialog on show: its facts, when it appeared, and the ask it answers.
pub struct Dialog {
    pub facts: lattice_core::ports::Dialog,
    pub shown_at: Instant,
    ask: Ask,
}

impl Dialog {
    /// Input is taken only once the dialog has been on show for [`DIALOG_QUIET`].
    pub fn ready(&self) -> bool {
        self.shown_at.elapsed() >= DIALOG_QUIET
    }
}

/// The runs tab: the list, the selected run as a waterfall, and the form that starts one.
#[derive(Default)]
pub struct Runs {
    pub list: Vec<RunSummary>,
    pub selected: Option<String>,
    pub detail: Option<RunState>,
    pub rows: Vec<WfRow>,
    pub agents: Vec<AgentInfo>,
    pub models: Vec<ModelChoice>,
    pub task: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub reading: bool,
    pub starting: bool,
}

/// Where the chat's folder stands for the next send.
#[derive(Default)]
pub struct Folder {
    /// The path typed into the box.
    pub path: String,
    pub view: Option<WorkspaceView>,
    pub busy: bool,
}

pub struct State {
    pub tab: Tab,
    shown: bool,
    services: Option<Arc<Services>>,
    pub starting: bool,
    pub failed: Option<String>,
    /// Web pages the account panel sent to Lattice's browser before the services ran (`open_web`).
    pub web_waiting: Vec<String>,
    /// A web page that opened in the person's own browser instead, and why (app.rs shows it as its notice).
    pub web_said: Option<String>,

    pub list: Option<ChatList>,
    pub listing: bool,
    pub status: Option<CoreStatus>,
    pub local: Option<LocalRuntime>,
    pub choices: Vec<ChatChoice>,
    pub choice: String,
    pub mode: Mode,
    pub open: Option<chat::Conversation>,
    pub opening: Option<String>,
    /// Bumped to start the follow again after a stream ended while a turn still runs.
    follow_generation: u64,
    pub composer: text_editor::Content,
    pub sending: bool,
    pub folder: Folder,
    pub changes: Option<ChangeSet>,
    pub diff: Option<FileDiff>,
    /// The diff's lines coloured as code, hunk by hunk, when it arrives (not on every frame).
    pub diff_colours: Vec<Vec<ide::highlight::Spans>>,
    pub reviewing: bool,
    pub answers: std::collections::HashMap<String, String>,
    pub steer: String,
    /// The last refusal or failure, in the core's own sentence, until dismissed.
    pub problem: Option<String>,
    /// Conversations that asked for the reader while not open.
    pub calling: HashSet<String>,
    /// The open conversation is a recording loaded for a photograph (a debug build's `CENTCOM_LATTICE_SNAPSHOT`), not
    /// one of the store's: it is not followed, and its changes are not read again.
    fixture: bool,

    pub dialog: Option<Dialog>,
    waiting_dialogs: VecDeque<Ask>,

    pub runs: Runs,
    /// The IDE: the folder's tree, the editor and its tabs, the panels (`ide/`).
    pub ide: ide::Ide,
    pub endpoints: Option<Endpoints>,
    pub form: Option<EndpointForm>,
    /// A removal or a forgotten key waiting for Confirm.
    pub pending_act: Option<RegistryAct>,
    pub acting: bool,
    pub system: Option<System>,
    pub system_reading: bool,
    pub training: Training,
    pub morphometry: morphometry::State,
    pub foundry: foundry::State,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Tab(Tab),
    Started(Result<Arc<Services>, String>),
    /// Lattice's browser opened (or did not open) a web page the account panel sent it.
    WebOpened(String, Result<(), String>),
    Refresh,
    Listed(Result<ChatList, String>),
    Status(CoreStatus, LocalRuntime),
    Choices(Vec<ChatChoice>),
    /// A model chosen by the label the picker shows.
    ChoiceLabel(String),
    Mode(Mode),
    New,
    Open(String),
    Opened(String, Result<Snapshot, String>),
    Compose(text_editor::Action),
    Send,
    Sent(Result<Accepted, String>),
    Batch(String, Vec<ConversationEvent>),
    FollowEnded(String),
    Stop,
    /// The reader's Stop on a background command's card: its conversation's call.
    StopCommand(String),
    Steer(String),
    SendSteer,
    Decide(String, bool),
    AllowAlways(String),
    /// "Allow always" for a pending MCP tool's call: the core's own dialog, then the call runs.
    AllowMcpAlways(String),
    AnswerText(String, String),
    Answer(String),
    Attach,
    Attached(Result<WorkspaceView, String>),
    Trust,
    Trusted(Result<TrustState, String>),
    Changes(Result<ChangeSet, String>),
    ShowDiff(String),
    Diff(Result<FileDiff, String>),
    Review(ReviewOp),
    Reviewed(Result<ReviewOutcome, String>),
    Archive(String),
    /// A call that only needs the list read again afterwards, and its refusal if any.
    Done(Result<(), String>),
    Asked(AskCell),
    DialogTick,
    DialogAnswer(bool),
    /// A link in an answer was clicked: links are shown, never opened.
    Link,
    Dismiss,
    // The runs tab.
    RunsRead(Vec<RunSummary>, Vec<AgentInfo>, Vec<ModelChoice>),
    RunsPoll,
    SelectRun(String),
    RunRead(String, Result<RunDetail, String>),
    Task(String),
    Agent(String),
    Model(String),
    StartRun,
    RunStarted(Result<RunSummary, String>),
    StopRun(String),
    // The IDE.
    Ide(ide::IdeMsg),
    // The Models and System tabs.
    EndpointsRead(Endpoints),
    NewEndpoint,
    EditEndpoint(String),
    Form(FormField, String),
    FormFlag(FormFlag, bool),
    SaveEndpoint,
    CancelForm,
    AskRemove(String),
    AskForgetKey(String),
    ConfirmAct,
    CancelAct,
    Acted(Result<String, String>),
    /// The person chose the GGUF model Local runs, and what came of writing it.
    ChooseLocal(String),
    LocalChosen(Result<String, String>),
    SystemRead(Box<System>),
    // The Training Studio tab.
    WorkspacePath(String),
    OpenWorkspace,
    WorkspaceOpened(Box<Result<training::Workspace, String>>),
    NewParent(String),
    ChooseNewParent,
    NewName(String),
    DocPath(String),
    DocSource(String),
    DocLicense(String),
    DocGroup(String),
    PlanPreset(&'static str),
    ToggleExpert(String),
    /// Choose a saved plan's preset and experts again (the Python Studio's "Open plan").
    UsePlan(usize),
    ExportPath(String),
    Train(TrainingAct),
    Trained(Box<Result<(training::Workspace, String), String>>),
    // The Morphometry tab.
    Morpho(morphometry::Msg),
    // The Foundry tab.
    Foundry(foundry::Msg),
}

impl State {
    pub fn new() -> State {
        State {
            tab: Tab::Chat,
            shown: false,
            services: None,
            starting: false,
            failed: None,
            web_waiting: Vec::new(),
            web_said: None,
            list: None,
            listing: false,
            status: None,
            local: None,
            choices: Vec::new(),
            choice: "auto".to_string(),
            mode: Mode::Ask,
            open: None,
            opening: None,
            follow_generation: 0,
            composer: text_editor::Content::new(),
            sending: false,
            folder: Folder::default(),
            changes: None,
            diff: None,
            diff_colours: Vec::new(),
            reviewing: false,
            answers: Default::default(),
            steer: String::new(),
            problem: None,
            calling: HashSet::new(),
            fixture: false,
            dialog: None,
            waiting_dialogs: VecDeque::new(),
            runs: Runs::default(),
            ide: ide::Ide::default(),
            endpoints: None,
            form: None,
            pending_act: None,
            acting: false,
            system: None,
            system_reading: false,
            training: Training::default(),
            morphometry: morphometry::State::default(),
            foundry: foundry::State::default(),
        }
    }

    /// A web page of the account panel, in Lattice's own browser in a tab of its own (since 2026-10-08):
    /// the reader's own action, which neither shows this page nor switches the agent's browser on, and leaves the
    /// agent its page. The services start first when this page never opened. A page the browser will not open (its
    /// policy: the public web only) or cannot open goes to the person's own browser instead, and `web_said` says so.
    pub fn open_web(&mut self, url: String) -> Task<Msg> {
        if let Some(services) = self.services.clone() {
            let chat = services.chat.clone();
            let shown = url.clone();
            let work = async move { chat.browser_open_tab(url).await.map_err(|refusal| refusal.message) };
            return Task::perform(on(&services, work), move |result| {
                Msg::WebOpened(shown.clone(), result.unwrap_or_else(|| Err(STOPPED.to_string())))
            });
        }
        if let Some(why) = self.failed.clone() {
            self.open_yours_instead(&url, &why);
            return Task::none();
        }
        self.web_waiting.push(url);
        if self.starting {
            return Task::none();
        }
        self.starting = true;
        Task::perform(off_thread(|| Services::start().map(Arc::new)), |done| {
            Msg::Started(done.unwrap_or_else(|| Err("Lattice's services stopped while starting.".to_string())))
        })
    }

    /// `url` in the person's own browser, because Lattice's could not take it (`why`), said in `web_said`.
    fn open_yours_instead(&mut self, url: &str, why: &str) {
        let why = why.trim_end_matches('.');
        self.web_said = Some(match crate::signin::web::open_yours(url) {
            Ok(()) => format!("Lattice's browser did not open it ({why}), so it opened in your own browser."),
            Err(also) => format!("Lattice's browser did not open it ({why}), and your own browser did not either: {also}"),
        });
    }

    /// The page opened: start the services the first time, then read what the tab shows.
    pub fn open(&mut self) -> Task<Msg> {
        self.shown = true;
        if self.services.is_none() {
            if self.starting || self.failed.is_some() {
                return Task::none();
            }
            self.starting = true;
            return Task::perform(off_thread(|| Services::start().map(Arc::new)), |done| {
                Msg::Started(done.unwrap_or_else(|| Err("Lattice's services stopped while starting.".to_string())))
            });
        }
        self.refresh()
    }

    pub fn close(&mut self) {
        self.shown = false;
    }

    pub fn busy(&self) -> bool {
        self.starting
            || self.listing
            || self.opening.is_some()
            || self.sending
            || self.reviewing
            || self.folder.busy
            || self.runs.reading
            || self.runs.starting
            || self.ide.busy()
            || self.system_reading
            || self.training.opening
            || self.acting
            || self.morphometry.busy()
            || self.foundry.busy()
            || self.open.as_ref().is_some_and(|c| c.running)
            // A chat in the grid at work, while the grid is on show (its tile's spinner turns).
            || (self.ide.grid.shown && self.ide.grid.tiles.iter().any(|t| !t.recording && (t.sending || t.conversation.as_ref().is_some_and(|c| c.running))))
    }

    /// The chat is the planning chat beside the grid: Ask only.
    pub fn planning(&self) -> bool {
        self.ide.grid.shown && self.ide.grid.beside == ide::grid::Beside::Chat
    }

    /// The conversation shown, if any.
    pub fn conversation(&self) -> Option<&chat::Conversation> {
        self.open.as_ref()
    }

    /// The services have started (a folder can be opened, a message sent).
    pub fn services_ready(&self) -> bool {
        self.services.is_some()
    }

    pub fn shared_writes(&self) -> Option<bool> {
        self.list.as_ref().map(|l| l.shared_writes).or(self.status.as_ref().map(|s| s.shared_writes))
    }

    fn refresh(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let task = match self.tab {
            Tab::Chat | Tab::Models => {
                self.listing = true;
                // The projects, which the Chats view groups chats by.
                let projects = self.read_projects();
                let agent = services.agent();
                let (a, b, c) = (agent.clone(), agent.clone(), agent);
                Task::batch([
                    projects,
                    Task::perform(on(&services, async move { a.list().await.map_err(|r| r.message) }), |r| {
                        Msg::Listed(r.unwrap_or_else(|| Err(STOPPED.to_string())))
                    }),
                    Task::perform(on(&services, async move { b.choices().await }), |r| Msg::Choices(r.unwrap_or_default())),
                    Task::perform(
                        on(&services, async move {
                            let status = c.status();
                            (status, c.local_runtime().await)
                        }),
                        |r| match r {
                            Some((status, local)) => Msg::Status(status, local),
                            None => Msg::Done(Err(STOPPED.to_string())),
                        },
                    ),
                ])
            }
            Tab::Runs => self.read_runs(),
            Tab::System => self.read_system(),
            Tab::Training => Task::none(),
            Tab::Morphometry => self.morphometry.open(Some(services.state.clone())).map(Msg::Morpho),
            Tab::Foundry => self.foundry.open().map(Msg::Foundry),
        };
        if self.tab == Tab::Models {
            return Task::batch([task, self.read_endpoints()]);
        }
        task
    }

    /// The registry of model endpoints, read off the window's thread (`<globals>/model_endpoints.json`, or
    /// `ALELYON_MODEL_CONFIG`, as the Python side reads it).
    fn read_endpoints(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let state = services.state.clone();
        Task::perform(off_thread(move || endpoints_of(&state)), |r| match r {
            Some(e) => Msg::EndpointsRead(e),
            None => Msg::Done(Err(STOPPED.to_string())),
        })
    }

    fn read_system(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        if self.system_reading {
            return Task::none();
        }
        self.system_reading = true;
        let globals = services.state.globals.clone();
        Task::perform(
            off_thread(move || {
                let checkout = std::env::current_exe().ok().and_then(|exe| crate::compute::checkout_of(&exe));
                let ledger = system::read_ledger(&system::ledger_path(&globals));
                let now = crate::utc::now() as i64;
                let evidence = match &ledger {
                    Ok(l) => obligations::sessions(&l.records)
                        .into_iter()
                        .map(|s| {
                            let e = obligations::derive(&l.records, &s, now);
                            (s, e)
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                };
                System { stack: system::stack(checkout.as_deref()), ledger, evidence }
            }),
            |r| match r {
                Some(s) => Msg::SystemRead(Box::new(s)),
                None => Msg::Done(Err(STOPPED.to_string())),
            },
        )
    }

    fn read_runs(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        if self.runs.reading {
            return Task::none();
        }
        self.runs.reading = true;
        let runs = services.runs.clone();
        Task::perform(off_thread(move || (runs.runs(), runs.agents(), runs.models())), |r| match r {
            Some((list, agents, models)) => Msg::RunsRead(list, agents, models),
            None => Msg::RunsRead(Vec::new(), Vec::new(), Vec::new()),
        })
    }

    fn agent(&self) -> Option<(Arc<Services>, Arc<dyn AgentChatService>)> {
        self.services.as_ref().map(|s| (s.clone(), s.agent()))
    }

    fn shown_for(&self, choice: &str) -> Shown {
        let picked = self.choices.iter().find(|c| c.id == choice);
        Shown {
            locality: picked.map(|c| c.locality).unwrap_or(lattice_protocol::Locality::Local),
            label: picked.map(|c| c.label.clone()).unwrap_or_else(|| choice.to_string()),
        }
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Tab(tab) => {
                self.tab = tab;
                return self.refresh();
            }
            Msg::Started(Ok(services)) => {
                self.starting = false;
                self.services = Some(services);
                #[cfg(debug_assertions)]
                self.load_fixture();
                // The agent's browser: on or off, for the composer and the Tools view.
                let mut tasks = vec![self.refresh(), self.read_browser(), self.read_completion()];
                // A folder named on the command line opens once the services run.
                if let Some((folder, _)) = &self.ide.startup {
                    self.ide.folder_path = folder.clone();
                    let path = folder.clone();
                    tasks.push(self.open_folder(path, true));
                }
                // A debug build's photograph of the terminal: the panel opens on one started in the folder named on the
                // command line (a release build never reads this).
                #[cfg(debug_assertions)]
                if std::env::var_os("CENTCOM_LATTICE_TERMINAL").is_some() {
                    self.ide.panel = ide::PanelTab::Terminal;
                    self.ide.panel_open = true;
                    let cwd = self.ide.startup.as_ref().map(|(folder, _)| std::path::PathBuf::from(folder));
                    tasks.push(self.start_terminal(cwd));
                }
                // A debug build's photograph of the Tools page: it opens with the Tools view, every server open on it,
                // and starts the enabled servers of the reader's own MCP settings (a release build never reads this).
                #[cfg(debug_assertions)]
                if std::env::var_os("CENTCOM_LATTICE_TOOLS").is_some() {
                    tasks.push(self.photograph_tools());
                }
                // A debug build's photograph of a project: the Chats view with it picked, and its page (a release build
                // never reads this).
                #[cfg(debug_assertions)]
                if std::env::var_os("CENTCOM_LATTICE_PROJECT").is_some() {
                    tasks.push(self.photograph_project());
                }
                #[cfg(debug_assertions)]
                if let Ok(paths) = std::env::var("CENTCOM_LATTICE_GRID") {
                    self.photograph_grid(&paths);
                }
                // A debug build's photograph of the composer: it starts with this text (a `/` lists the commands; a
                // release build never reads this).
                #[cfg(debug_assertions)]
                if let Ok(text) = std::env::var("CENTCOM_LATTICE_COMPOSER") {
                    self.composer = text_editor::Content::with_text(&text);
                    tasks.push(self.slash_edited());
                }
                // Web pages the account panel sent before the services ran.
                for url in std::mem::take(&mut self.web_waiting) {
                    tasks.push(self.open_web(url));
                }
                return Task::batch(tasks);
            }
            Msg::Started(Err(why)) => {
                self.starting = false;
                for url in std::mem::take(&mut self.web_waiting) {
                    self.open_yours_instead(&url, &why);
                }
                self.failed = Some(why);
            }
            Msg::WebOpened(_, Ok(())) => {}
            Msg::WebOpened(url, Err(why)) => self.open_yours_instead(&url, &why),
            Msg::Refresh => return self.refresh(),
            Msg::Listed(result) => {
                self.listing = false;
                match result {
                    Ok(list) => {
                        if let Some(services) = &self.services {
                            for id in services.attention.take() {
                                if self.open.as_ref().is_none_or(|c| c.id() != id) {
                                    self.calling.insert(id);
                                }
                            }
                        }
                        // A debug build's photograph of the grid: the newest chats put in it, up to its size (a release
                        // build never reads this).
                        #[cfg(debug_assertions)]
                        let grid_photo: Vec<String> = if std::env::var("CENTCOM_LATTICE_GRID").is_ok_and(|v| !v.contains(".json"))
                            && self.ide.grid.tiles.is_empty()
                        {
                            list.conversations.iter().take(ide::grid::MAX_TILES).map(|c| c.id.clone()).collect()
                        } else {
                            Vec::new()
                        };
                        self.list = Some(list);
                        #[cfg(debug_assertions)]
                        if !grid_photo.is_empty() {
                            let tasks: Vec<_> = grid_photo.into_iter().map(|id| self.grid(ide::grid::GridMsg::Add(id))).collect();
                            return Task::batch(tasks);
                        }
                    }
                    Err(why) => self.problem = Some(why),
                }
            }
            Msg::Status(status, local) => {
                self.status = Some(status);
                self.local = Some(local);
            }
            Msg::Choices(choices) => {
                if !choices.iter().any(|c| c.id == self.choice)
                    && let Some(first) = choices.iter().find(|c| c.ready).or(choices.first())
                {
                    self.choice = first.id.clone();
                }
                self.choices = choices;
            }
            Msg::ChoiceLabel(label) => match self.choices.iter().find(|c| ide::agent::choice_label(c) == label) {
                Some(c) if c.ready => self.choice = c.id.clone(),
                Some(c) => {
                    self.problem = Some(c.refusal.clone().unwrap_or_else(|| format!("{} is not ready.", c.label)));
                }
                None => {}
            },
            Msg::Mode(mode) => {
                self.mode = mode;
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent()) {
                    let id = c.id().to_string();
                    return Task::perform(
                        on(&services, async move { agent.set_mode(&id, mode).await.map_err(|r| r.message) }),
                        done,
                    );
                }
            }
            Msg::New => {
                let kept = self.folder.view.take();
                self.start_new_chat();
                // A new chat works in the folder the editor shows, as the next send's folder.
                match &self.ide.folder {
                    Some(f) => match kept.filter(|v| v.workspace.id == f.id()) {
                        Some(view) => self.folder = Folder { path: f.shown_path(), view: Some(view), busy: false },
                        None => return self.attach_open_folder(),
                    },
                    None => self.folder = Folder::default(),
                }
            }
            Msg::Open(id) => {
                let Some((services, agent)) = self.agent() else { return Task::none() };
                self.fixture = false;
                self.calling.remove(&id);
                self.opening = Some(id.clone());
                let asked = id.clone();
                return Task::perform(
                    on(&services, async move { agent.open(&asked).await.map_err(|r| r.message) }),
                    move |r| Msg::Opened(id.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))),
                );
            }
            Msg::Opened(id, result) => {
                if self.opening.as_deref() == Some(id.as_str()) {
                    self.opening = None;
                }
                match result {
                    Ok(snapshot) => {
                        // Another chat's reviews and outputs close; the same chat read again keeps them.
                        if self.open.as_ref().is_none_or(|c| c.id() != snapshot.conversation.id) {
                            self.ide.close_chat_tabs();
                        }
                        self.mode = snapshot.conversation.mode;
                        if !snapshot.conversation.pinned_provider.is_empty()
                            && self.choices.iter().any(|c| c.id == snapshot.conversation.pinned_provider)
                        {
                            self.choice = snapshot.conversation.pinned_provider.clone();
                        }
                        self.folder = Folder {
                            path: snapshot.conversation.workspace.as_ref().map(|w| w.path.clone()).unwrap_or_default(),
                            view: None,
                            busy: false,
                        };
                        self.diff = None;
                        self.ide.editing_turn = None;
                        self.ide.checkpoints = None;
                        self.ide.command = None;
                        self.ide.command_lines = None;
                        self.open = Some(chat::Conversation::from_snapshot(snapshot));
                        self.follow_generation += 1;
                        let mut next = vec![self.read_changes(), self.follow_folder()];
                        // An action pressed in this chat's pane in the grid, done now that it is the open chat.
                        if let Some((pending, msg)) = self.ide.grid.pending.take() {
                            if pending == id {
                                next.push(self.update(*msg));
                            }
                        }
                        if self.ide.side_open && self.ide.side == ide::Side::Changes {
                            next.push(self.ide_update(ide::IdeMsg::ReadHistory));
                        }
                        return Task::batch(next);
                    }
                    Err(why) => self.problem = Some(why),
                }
            }
            Msg::Compose(action) => {
                // The composer is being typed in or clicked: the terminal no longer has the keyboard.
                if !matches!(action, text_editor::Action::Scroll { .. }) {
                    self.ide.term_focus = false;
                }
                let edits = action.is_edit();
                self.composer.perform(action);
                // A `/` at the start lists the commands.
                if edits {
                    self.mention_edited();
                    return self.slash_edited();
                }
            }
            Msg::Send => {
                let text = self.composer.text().trim().to_string();
                // A message being edited replaces that turn (and hides what came after it).
                let edit_of = self.ide.editing_turn.clone();
                return self.send_text(text, self.mode, edit_of, false);
            }
            Msg::Sent(result) => {
                self.sending = false;
                // An inline edit's message or a task's prompt was not the composer's: a draft there stays.
                let from_composer = !std::mem::take(&mut self.ide.inline_sending);
                match result {
                    Ok(Accepted::Started { conversation, user_turn, .. }) => {
                        if from_composer {
                            self.composer = text_editor::Content::new();
                            self.ide.attached.clear();
                        }
                        if self.ide.editing_turn.take().is_some() {
                            // The edited turn replaced the old one and what followed it: read the chat again.
                            return Task::batch([self.update(Msg::Open(conversation.id.clone())), self.refresh()]);
                        }
                        let same = self.open.as_ref().is_some_and(|c| c.id() == conversation.id);
                        if !same {
                            // A new conversation: open it (its snapshot holds the question).
                            return Task::batch([self.update(Msg::Open(conversation.id.clone())), self.refresh()]);
                        }
                        if let Some(c) = &mut self.open {
                            c.summary = *conversation;
                            c.asked(*user_turn);
                        }
                        return self.refresh();
                    }
                    Ok(Accepted::Queued { position, .. }) => {
                        if from_composer {
                            self.composer = text_editor::Content::new();
                        }
                        self.problem = Some(format!(
                            "A turn is running here, so your message waits its turn (number {position} in line)."
                        ));
                    }
                    Err(why) => self.problem = Some(why),
                }
            }
            Msg::Batch(id, events) => {
                // A chat in the grid takes the batch too (a chat open on the right as well is followed once).
                let mut grid_task = if self.ide.grid.apply(&id, &events) { self.grid_changes(id.clone()) } else { Task::none() };
                // Following the agent: the newest change staged in this batch opens as its diff. One of the open
                // chat's opens at once; one of another chat in the grid opens that chat on the right first.
                let followed = self.ide.follow.then(|| ide::followed_change(&events)).flatten();
                let open_here = self.open.as_ref().is_some_and(|c| c.id() == id);
                if let Some(change) = followed.clone()
                    && !open_here
                    && self.ide.grid.holds(&id)
                {
                    grid_task = Task::batch([grid_task, self.grid(ide::grid::GridMsg::Act(id.clone(), Box::new(Msg::ShowDiff(change))))]);
                }
                if let Some(c) = &mut self.open
                    && c.id() == id
                {
                    let staged = events.iter().any(|e| {
                        use lattice_protocol::conversation::ConversationEventKind as Ev;
                        matches!(
                            e.kind,
                            Ev::Staged { .. } | Ev::Reviewed { .. } | Ev::Conflict { .. } | Ev::CommandEffect { .. }
                        )
                    });
                    let ended = events.iter().any(|e| {
                        matches!(e.kind, lattice_protocol::conversation::ConversationEventKind::TurnEnded { .. })
                    });
                    let shown = self.ide.command.clone();
                    let writes = shown.as_ref().is_some_and(|call| {
                        use lattice_protocol::conversation::ConversationEventKind as Ev;
                        events.iter().any(|e| match &e.kind {
                            Ev::CommandProgress { call_id, .. } | Ev::CommandExited { call_id, .. } => call_id == call,
                            _ => false,
                        })
                    });
                    c.apply(&events);
                    let mut next = Vec::new();
                    if staged {
                        next.push(self.read_changes());
                    }
                    if writes && let Some(call) = shown {
                        next.push(self.read_command(call, 1));
                    }
                    if ended {
                        next.push(self.refresh());
                    }
                    next.push(grid_task);
                    if let Some(change) = followed.filter(|_| open_here) {
                        next.push(self.update(Msg::ShowDiff(change)));
                    }
                    return Task::batch(next);
                }
                return grid_task;
            }
            Msg::FollowEnded(id) => {
                self.ide.grid.follow_ended(&id);
                if self.open.as_ref().is_some_and(|c| c.id() == id && c.running) {
                    self.follow_generation += 1;
                }
            }
            Msg::Stop => {
                if let (Some(c), Some(services)) = (&self.open, &self.services) {
                    services.chat.stop(c.id());
                }
            }
            Msg::StopCommand(call) => {
                if let (Some(c), Some(services)) = (&self.open, &self.services) {
                    services.chat.stop_command(c.id(), &call);
                }
            }
            Msg::Steer(text) => self.steer = text,
            Msg::SendSteer => {
                let text = std::mem::take(&mut self.steer);
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent())
                    && !text.trim().is_empty()
                {
                    let id = c.id().to_string();
                    return Task::perform(on(&services, async move { agent.steer(&id, text).await.map_err(|r| r.message) }), done);
                }
            }
            Msg::Decide(call, approve) => {
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent()) {
                    let id = c.id().to_string();
                    let decision = if approve { Decision::Approve } else { Decision::Reject { note: None } };
                    return Task::perform(
                        on(&services, async move { agent.decide(&id, &call, decision).await.map_err(|r| r.message) }),
                        done,
                    );
                }
            }
            Msg::AllowAlways(call) => {
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent()) {
                    let id = c.id().to_string();
                    return Task::perform(
                        on(&services, async move {
                            agent.allow_always(&id, &call, MatchScope::Exact).await.map(|_| ()).map_err(|r| r.message)
                        }),
                        done,
                    );
                }
            }
            Msg::AllowMcpAlways(call) => {
                if let (Some(c), Some(services)) = (&self.open, self.services.clone()) {
                    let id = c.id().to_string();
                    let chat = services.chat.clone();
                    return Task::perform(
                        on(&services, async move { chat.allow_mcp_always(&id, &call).await.map_err(|r| r.message) }),
                        done,
                    );
                }
            }
            Msg::AnswerText(call, text) => {
                self.answers.insert(call, text);
            }
            Msg::Answer(call) => {
                let text = self.answers.remove(&call).unwrap_or_default();
                if let (Some(c), Some(services)) = (&self.open, &self.services)
                    && let Err(refusal) = services.chat.answer(c.id(), &call, text)
                {
                    self.problem = Some(refusal.message);
                }
            }
            Msg::Attach => {
                let path = self.folder.path.trim().to_string();
                let Some((services, agent)) = self.agent() else { return Task::none() };
                if path.is_empty() || self.folder.busy {
                    return Task::none();
                }
                self.folder.busy = true;
                let id = self.open.as_ref().map(|c| c.id().to_string());
                return Task::perform(
                    on(&services, async move { agent.attach_workspace(id.as_deref(), path).await.map_err(|r| r.message) }),
                    |r| Msg::Attached(r.unwrap_or_else(|| Err(STOPPED.to_string()))),
                );
            }
            Msg::Attached(result) => {
                self.folder.busy = false;
                match result {
                    Ok(view) => {
                        self.folder.path = view.workspace.path.clone();
                        self.folder.view = Some(view);
                        if let Err(why) = self.same_folder() {
                            self.problem = Some(why);
                        }
                        return self.refresh();
                    }
                    Err(why) => self.problem = Some(why),
                }
            }
            Msg::Trust => {
                let Some(workspace) = self.workspace_id() else { return Task::none() };
                let Some((services, agent)) = self.agent() else { return Task::none() };
                self.folder.busy = true;
                return Task::perform(on(&services, async move { agent.trust(&workspace).await.map_err(|r| r.message) }), |r| {
                    Msg::Trusted(r.unwrap_or_else(|| Err(STOPPED.to_string())))
                });
            }
            Msg::Trusted(result) => {
                self.folder.busy = false;
                match result {
                    Ok(trust) => {
                        if let Some(view) = &mut self.folder.view {
                            view.trust = trust;
                            view.workspace.trusted = trust == TrustState::Trusted;
                        }
                        return self.refresh();
                    }
                    Err(why) => self.problem = Some(why),
                }
            }
            Msg::Changes(result) => match result {
                Ok(set) => self.changes = Some(set),
                Err(why) => self.problem = Some(why),
            },
            Msg::ShowDiff(change) => {
                // The change is reviewed in an editor tab of its own.
                let path = self.changes.as_ref().and_then(|s| s.changes.iter().find(|c| c.id == change)).map(|c| c.path.clone());
                let existing = self
                    .ide
                    .tabs
                    .iter()
                    .find(|t| matches!(&t.kind, ide::TabKind::Diff { change: c, .. } if *c == change))
                    .map(|t| t.id);
                match existing {
                    Some(id) => self.ide.active = Some(id),
                    None => {
                        self.ide.push(ide::TabKind::Diff { change: change.clone(), path: path.unwrap_or_default() });
                    }
                }
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent()) {
                    let id = c.id().to_string();
                    return Task::perform(on(&services, async move { agent.diff(&id, &change).await.map_err(|r| r.message) }), |r| {
                        Msg::Diff(r.unwrap_or_else(|| Err(STOPPED.to_string())))
                    });
                }
            }
            Msg::Diff(result) => match result {
                Ok(diff) => {
                    // A tab opened before the change list was read learns its file's path from the diff.
                    for t in &mut self.ide.tabs {
                        if let ide::TabKind::Diff { change, path } = &mut t.kind
                            && *change == diff.change
                        {
                            *path = diff.path.clone();
                        }
                    }
                    let lang = ide::highlight::Lang::of(&diff.path);
                    self.diff_colours = diff
                        .hunks
                        .iter()
                        .map(|h| ide::highlight::lines(lang, h.lines.iter().map(|l| l.text.as_str()), ide::highlight::Carry::Code))
                        .collect();
                    self.diff = Some(diff);
                }
                Err(why) => self.problem = Some(why),
            },
            Msg::Review(op) => {
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent()) {
                    self.reviewing = true;
                    let id = c.id().to_string();
                    return Task::perform(on(&services, async move { agent.review(&id, vec![op]).await.map_err(|r| r.message) }), |r| {
                        Msg::Reviewed(r.unwrap_or_else(|| Err(STOPPED.to_string())))
                    });
                }
            }
            Msg::Reviewed(result) => {
                self.reviewing = false;
                // The files a keep wrote, for the tabs that show them.
                let kept: Vec<String> = result
                    .as_ref()
                    .map(|outcome| {
                        outcome
                            .results
                            .iter()
                            .filter(|r| {
                                matches!(
                                    r.result,
                                    lattice_protocol::conversation::ReviewResult::Kept { .. }
                                        | lattice_protocol::conversation::ReviewResult::PartlyKept { .. }
                                )
                            })
                            .map(|r| r.path.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                match result {
                    Ok(outcome) => {
                        let failed: Vec<String> = outcome
                            .results
                            .iter()
                            .filter_map(|r| match &r.result {
                                lattice_protocol::conversation::ReviewResult::Failed { .. }
                                | lattice_protocol::conversation::ReviewResult::Conflict { .. }
                                | lattice_protocol::conversation::ReviewResult::Skipped { .. } => {
                                    Some(format!("{}: {}", r.path, view::review_words(&r.result)))
                                }
                                _ => None,
                            })
                            .collect();
                        if !failed.is_empty() {
                            self.problem = Some(failed.join("; "));
                        }
                    }
                    Err(why) => self.problem = Some(why),
                }
                let diff = self.diff.as_ref().map(|d| d.change.clone());
                let mut next = vec![self.read_changes(), self.kept_on_disk(&kept)];
                if !kept.is_empty() {
                    next.push(self.read_listing());
                }
                if let Some(change) = diff {
                    next.push(self.update(Msg::ShowDiff(change)));
                }
                return Task::batch(next);
            }
            Msg::Archive(id) => {
                let Some((services, agent)) = self.agent() else { return Task::none() };
                if self.open.as_ref().is_some_and(|c| c.id() == id) {
                    self.open = None;
                    self.changes = None;
                    self.diff = None;
                    self.ide.close_chat_tabs();
                }
                return Task::perform(on(&services, async move { agent.archive(&id).await.map_err(|r| r.message) }), done);
            }
            Msg::Done(result) => {
                if let Err(why) = result {
                    self.problem = Some(why);
                }
                return self.refresh();
            }
            Msg::Asked(cell) => {
                if let Some(ask) = cell.0.lock().unwrap_or_else(|p| p.into_inner()).take() {
                    self.waiting_dialogs.push_back(ask);
                }
                self.next_dialog();
            }
            Msg::DialogTick => {}
            Msg::DialogAnswer(yes) => {
                if let Some(dialog) = &self.dialog {
                    // A click before the dialog is ready is ignored, not taken as an answer (CP1).
                    if !dialog.ready() {
                        return Task::none();
                    }
                }
                if let Some(mut dialog) = self.dialog.take() {
                    dialog.ask.answer(yes);
                }
                self.next_dialog();
            }
            Msg::Link => {}
            Msg::Dismiss => self.problem = None,
            Msg::RunsRead(list, agents, models) => {
                self.runs.reading = false;
                if self.runs.agent.is_none() {
                    self.runs.agent = agents.first().map(|a| a.id.clone());
                }
                if self.runs.model.is_none() {
                    self.runs.model = models.iter().find(|m| m.ready).or(models.first()).map(|m| m.id.clone());
                }
                self.runs.list = list;
                self.runs.agents = agents;
                self.runs.models = models;
                if let Some(id) = self.runs.selected.clone() {
                    return self.read_run(id);
                }
            }
            Msg::RunsPoll => return self.read_runs(),
            Msg::SelectRun(id) => {
                self.runs.selected = Some(id.clone());
                return self.read_run(id);
            }
            Msg::RunRead(id, result) => {
                if self.runs.selected.as_deref() == Some(id.as_str()) {
                    match result {
                        Ok(detail) => {
                            let state = RunState::from_detail(detail);
                            let tree = SpanTree::build(&state.spans);
                            let rows = visible_rows(&state.spans, &tree, &HashSet::new());
                            self.runs.rows = build_rows(&state, &rows);
                            self.runs.detail = Some(state);
                        }
                        Err(why) => self.problem = Some(why),
                    }
                }
            }
            Msg::Task(text) => self.runs.task = text,
            Msg::Agent(id) => self.runs.agent = Some(id),
            Msg::Model(id) => self.runs.model = Some(id),
            Msg::StartRun => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let (Some(agent), Some(model)) = (self.runs.agent.clone(), self.runs.model.clone()) else {
                    return Task::none();
                };
                let task = self.runs.task.trim().to_string();
                if task.is_empty() || self.runs.starting {
                    return Task::none();
                }
                self.runs.starting = true;
                let runs = services.runs.clone();
                return Task::perform(off_thread(move || runs.start(StartRun { task, agent, model })), |r| {
                    Msg::RunStarted(match r {
                        Some(r) => r.map_err(|e| e.message),
                        None => Err(STOPPED.to_string()),
                    })
                });
            }
            Msg::RunStarted(result) => {
                self.runs.starting = false;
                match result {
                    Ok(run) => {
                        self.runs.task.clear();
                        self.runs.selected = Some(run.id.clone());
                        return self.read_runs();
                    }
                    Err(why) => self.problem = Some(why),
                }
            }
            Msg::Ide(msg) => return self.ide_update(msg),
            Msg::EndpointsRead(endpoints) => self.endpoints = Some(endpoints),
            Msg::NewEndpoint => {
                self.form = Some(EndpointForm { enabled: true, ..EndpointForm::default() });
            }
            Msg::EditEndpoint(id) => {
                if let Some(row) = self.endpoints.as_ref().and_then(|e| e.rows.iter().find(|r| r.saved.id == id)) {
                    let e = &row.saved;
                    self.form = Some(EndpointForm {
                        editing: Some(e.id.clone()),
                        builtin: e.builtin,
                        id: e.id.clone(),
                        label: e.label.clone(),
                        anthropic: e.kind == lattice_core::registry::EndpointKind::Anthropic,
                        base_url: e.base_url.clone(),
                        model: e.model.clone(),
                        key_name: e.api_key_name.clone(),
                        enabled: e.enabled,
                        note: e.note.clone(),
                        key: String::new(),
                        confirming: false,
                    });
                }
            }
            Msg::Form(field, value) => {
                if let Some(f) = &mut self.form {
                    f.confirming = false;
                    match field {
                        FormField::Id => f.id = value,
                        FormField::Label => f.label = value,
                        FormField::BaseUrl => f.base_url = value,
                        FormField::Model => f.model = value,
                        FormField::KeyName => f.key_name = value.to_ascii_uppercase(),
                        FormField::Note => f.note = value,
                        FormField::Key => f.key = value,
                    }
                }
            }
            Msg::FormFlag(flag, on) => {
                if let Some(f) = &mut self.form {
                    f.confirming = false;
                    match flag {
                        FormFlag::Enabled => f.enabled = on,
                        FormFlag::Anthropic => f.anthropic = on,
                    }
                }
            }
            Msg::SaveEndpoint => {
                let Some(f) = &mut self.form else { return Task::none() };
                if !f.confirming {
                    f.confirming = true;
                    return Task::none();
                }
                let Some(services) = self.services.clone() else { return Task::none() };
                let endpoint = endpoint_of_form(f);
                let key = (!f.key.trim().is_empty()).then(|| std::mem::take(&mut f.key));
                return self.act(&services, RegistryAct::Save { endpoint: Box::new(endpoint), key });
            }
            Msg::CancelForm => {
                // The typed key is dropped with the form; it was never written anywhere.
                self.form = None;
            }
            Msg::AskRemove(id) => self.pending_act = Some(RegistryAct::Remove { id }),
            Msg::AskForgetKey(name) => self.pending_act = Some(RegistryAct::ForgetKey { name }),
            Msg::CancelAct => self.pending_act = None,
            Msg::ConfirmAct => {
                let (Some(act), Some(services)) = (self.pending_act.take(), self.services.clone()) else {
                    return Task::none();
                };
                return self.act(&services, act);
            }
            Msg::ChooseLocal(name) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                return self.choose_local(&services, name);
            }
            Msg::LocalChosen(result) => {
                self.acting = false;
                self.problem = Some(result.unwrap_or_else(|why| why));
                return self.refresh();
            }
            Msg::Acted(result) => {
                self.acting = false;
                match result {
                    Ok(words) => {
                        self.form = None;
                        self.problem = Some(words);
                    }
                    Err(why) => {
                        if let Some(f) = &mut self.form {
                            f.confirming = false;
                        }
                        self.problem = Some(why);
                    }
                }
                return Task::batch([self.read_endpoints(), self.refresh()]);
            }
            Msg::SystemRead(system) => {
                self.system_reading = false;
                self.system = Some(*system);
            }
            Msg::WorkspacePath(path) => self.training.workspace_path = path,
            Msg::OpenWorkspace => {
                let path = std::path::PathBuf::from(self.training.workspace_path.trim());
                if path.as_os_str().is_empty() || self.training.opening {
                    return Task::none();
                }
                self.training.opening = true;
                return Task::perform(off_thread(move || training::open_workspace(&path)), |r| {
                    Msg::WorkspaceOpened(Box::new(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            Msg::WorkspaceOpened(result) => {
                self.training.opening = false;
                self.training.workspace = Some(*result);
            }
            Msg::NewParent(path) => self.training.new_parent = path,
            Msg::ChooseNewParent => {
                return Task::perform(off_thread(ide::picker::pick_folder), |r| match r.unwrap_or(Ok(None)) {
                    Ok(Some(path)) => Msg::NewParent(path.display().to_string()),
                    Ok(None) => Msg::NewParent(String::new()),
                    Err(why) => Msg::Trained(Box::new(Err(why))),
                });
            }
            Msg::NewName(name) => self.training.new_name = name,
            Msg::DocPath(path) => self.training.doc_path = path,
            Msg::DocSource(text) => self.training.doc_source = text,
            Msg::DocLicense(text) => self.training.doc_license = text,
            Msg::DocGroup(text) => self.training.doc_group = text,
            Msg::PlanPreset(key) => {
                if self.training.preset != key {
                    self.training.preset = key;
                    self.training.experts.clear();
                }
            }
            Msg::ToggleExpert(expert) => {
                if !self.training.experts.remove(&expert) {
                    self.training.experts.insert(expert);
                }
            }
            Msg::UsePlan(index) => {
                let plan = self.training.workspace.as_ref().and_then(|w| w.as_ref().ok()).and_then(|w| w.plans.get(index));
                if let Some(Ok(plan)) = plan.map(|entry| &entry.plan) {
                    self.training.preset = plan.preset.key;
                    self.training.experts = plan.selected_experts.iter().cloned().collect();
                    self.training.said = Some(Ok(
                        "The saved plan's preset and experts are chosen again. The pool shown is the current one; the plan \
                         keeps its own documents."
                            .to_string(),
                    ));
                }
            }
            Msg::ExportPath(path) => self.training.export_path = path,
            Msg::Train(act) => return self.train(act),
            Msg::Trained(result) => {
                self.training.writing = false;
                match *result {
                    Ok((workspace, words)) => {
                        self.training.workspace_path = workspace.path.display().to_string();
                        self.training.workspace = Some(Ok(workspace));
                        self.training.said = Some(Ok(words));
                    }
                    Err(why) => self.training.said = Some(Err(why)),
                }
            }
            Msg::Morpho(msg) => return self.morphometry.update(msg).map(Msg::Morpho),
            Msg::Foundry(msg) => return self.foundry.update(msg).map(Msg::Foundry),
            Msg::StopRun(id) => {
                if let Some(services) = &self.services
                    && let Err(refusal) = services.runs.stop(&id)
                {
                    self.problem = Some(refusal.message);
                }
                return self.read_runs();
            }
        }
        Task::none()
    }

    /// A debug build only: the conversation recorded in the file `CENTCOM_LATTICE_SNAPSHOT` names (`{"snapshot":
    /// Snapshot, "changes": ChangeSet, "diff": FileDiff, "artifact": ArtifactView}`, the protocol's own shapes) is shown as if opened, so the
    /// screenshot mode can photograph the agent's panel and a review without a model. A release build never reads it.
    #[cfg(debug_assertions)]
    fn load_fixture(&mut self) {
        let Some(path) = std::env::var_os("CENTCOM_LATTICE_SNAPSHOT") else { return };
        let read = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| {
            serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())
        });
        let value = match read {
            Ok(value) => value,
            Err(why) => {
                self.problem = Some(format!("CENTCOM_LATTICE_SNAPSHOT: {why}"));
                return;
            }
        };
        match serde_json::from_value::<Snapshot>(value["snapshot"].clone()) {
            Ok(snapshot) => {
                self.mode = snapshot.conversation.mode;
                self.open = Some(chat::Conversation::from_snapshot(snapshot));
                self.fixture = true;
            }
            Err(why) => self.problem = Some(format!("CENTCOM_LATTICE_SNAPSHOT: {why}")),
        }
        self.changes = serde_json::from_value(value["changes"].clone()).ok();
        if let Ok(diff) = serde_json::from_value::<FileDiff>(value["diff"].clone()) {
            self.ide.push(ide::TabKind::Diff { change: diff.change.clone(), path: diff.path.clone() });
            let _ = self.update(Msg::Diff(Ok(diff)));
        }
        // An artifact (`"artifact": ArtifactView`), shown in its tab as read.
        if let Ok(artifact) =
            serde_json::from_value::<lattice_protocol::conversation::ArtifactView>(value["artifact"].clone())
        {
            let id = self.ide.push(ide::TabKind::Artifact {
                conversation: String::new(),
                name: artifact.name.clone(),
                version: artifact.version,
                view: None,
                rendered: None,
                source: false,
            });
            let _ = self.update(Msg::Ide(ide::IdeMsg::ArtifactRead(id, Box::new(Ok(artifact)))));
        }
    }

    fn next_dialog(&mut self) {
        if self.dialog.is_none()
            && let Some(ask) = self.waiting_dialogs.pop_front()
        {
            self.dialog = Some(Dialog { facts: ask.request.dialog(), shown_at: Instant::now(), ask });
        }
    }

    fn workspace_id(&self) -> Option<String> {
        self.folder
            .view
            .as_ref()
            .map(|v| v.workspace.id.clone())
            .or_else(|| self.open.as_ref().and_then(|c| c.summary.workspace.as_ref().map(|w| w.id.clone())))
    }

    fn read_changes(&mut self) -> Task<Msg> {
        if self.fixture {
            return Task::none();
        }
        let (Some(c), Some((services, agent))) = (&self.open, self.agent()) else { return Task::none() };
        if c.summary.workspace.is_none() {
            self.changes = None;
            return Task::none();
        }
        let id = c.id().to_string();
        Task::perform(on(&services, async move { agent.changes(&id).await.map_err(|r| r.message) }), |r| {
            Msg::Changes(r.unwrap_or_else(|| Err(STOPPED.to_string())))
        })
    }

    fn read_run(&mut self, id: String) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let runs = services.runs.clone();
        let asked = id.clone();
        Task::perform(off_thread(move || runs.run(&asked)), move |r| {
            Msg::RunRead(
                id.clone(),
                match r {
                    Some(r) => r.map_err(|e| e.message),
                    None => Err(STOPPED.to_string()),
                },
            )
        })
    }

    /// The page's own subscriptions, while it is shown: the open conversation's follow, the runs poll while a run
    /// works, and the dialog's half-second quiet.
    pub fn subscription(&self) -> Subscription<Msg> {
        let mut subs = Vec::new();
        if let (Some(services), Some(c)) = (&self.services, self.open.as_ref().filter(|_| !self.fixture)) {
            subs.push(Subscription::run_with(
                Follow {
                    service: services.agent(),
                    conversation: c.id().to_string(),
                    after: c.last_seq,
                    generation: self.follow_generation,
                },
                follow_stream,
            ));
        }
        // Each chat in the grid is followed while the page shows, as the open one is.
        if let Some(services) = &self.services {
            for tile in &self.ide.grid.tiles {
                if let Some(c) = tile.conversation.as_ref().filter(|_| !tile.recording) {
                    subs.push(Subscription::run_with(
                        Follow { service: services.agent(), conversation: tile.id.clone(), after: c.last_seq, generation: tile.generation },
                        follow_stream,
                    ));
                }
            }
        }
        if self.tab == Tab::Runs && self.runs.list.iter().any(|r| r.status.is_active()) {
            subs.push(iced::time::every(RUNS_POLL).map(|_| Msg::RunsPoll));
        }
        if self.dialog.as_ref().is_some_and(|d| !d.ready()) {
            subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Msg::DialogTick));
        }
        if self.tab == Tab::Chat {
            subs.push(iced::event::listen_with(ide::update::keys));
            if self.ide.drag.is_some() {
                subs.push(iced::event::listen_with(ide::update::drags));
            }
            if self.ide.confirm.is_some() && !self.ide.confirm_ready() {
                subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Msg::Ide(ide::IdeMsg::ConfirmTick)));
            }
        }
        if let Some(services) = &self.services {
            subs.push(Subscription::run_with(Changed(services.clone()), changed_stream));
            if self.tab == Tab::Chat && self.ide.tools_shown() {
                subs.push(Subscription::run_with(McpChanged(services.clone()), mcp_stream));
                subs.push(Subscription::run_with(BrowserChanged(services.clone()), browser_stream));
            }
        }
        Subscription::batch(subs)
    }

    /// Send `text` to the open chat (a new one when none is open) in `mode`, in the chat's folder, as the composer
    /// sends; `edit_of` replaces that turn; `inline`: an inline edit's message, which leaves the composer's draft.
    pub(in crate::lattice) fn send_text(
        &mut self,
        text: String,
        mode: Mode,
        edit_of: Option<String>,
        inline: bool,
    ) -> Task<Msg> {
        let Some((services, agent)) = self.agent() else { return Task::none() };
        if text.is_empty() || self.sending {
            return Task::none();
        }
        // The planning chat beside the grid asks only: whatever mode was last picked, it writes nothing.
        let mode = if self.planning() { Mode::Ask } else { mode };
        self.sending = true;
        self.ide.inline_sending = inline;
        let mut request = self.send_request(text, mode, edit_of);
        // The attached images go with the composer's message only.
        if inline {
            request.images.clear();
        }
        Task::perform(on(&services, async move { agent.send(request).await.map_err(|r| r.message) }), |r| {
            Msg::Sent(r.unwrap_or_else(|| Err(STOPPED.to_string())))
        })
    }

    /// What a send asks the core: the open chat (a new one when none is open), the model picked, in the chat's
    /// folder; a new chat joins the project the Chats view has picked (an open one keeps its own).
    pub(in crate::lattice) fn send_request(&self, text: String, mode: Mode, edit_of: Option<String>) -> SendRequest {
        // Every send from the planning chat beside the grid is Ask, whichever path built it.
        let mode = if self.planning() { Mode::Ask } else { mode };
        SendRequest {
            conversation: self.open.as_ref().map(|c| c.id().to_string()),
            text,
            choice: self.choice.clone(),
            shown: self.shown_for(&self.choice),
            mode,
            workspace: self.chat_workspace(),
            edit_of,
            project: if self.open.is_none() { self.ide.project.clone() } else { None },
            images: self.ide.attached.iter().map(|a| a.image.clone()).collect(),
        }
    }

    /// The workspace the chat works in: the attached folder's, else the open chat's own.
    pub(in crate::lattice) fn chat_workspace(&self) -> Option<String> {
        self.folder.view.as_ref().map(|v| v.workspace.id.clone()).or_else(|| {
            self.open.as_ref().and_then(|c| c.summary.workspace.as_ref().map(|w| w.id.clone()))
        })
    }

    /// What runs whichever page is shown: the dialogs the core asks for (an action begun on this page can ask after
    /// the reader has moved on, and its dialog must still be answered), and the terminals' output (a program writing
    /// while the page is hidden must not fill its pipe and stall).
    pub fn background(&self) -> Subscription<Msg> {
        let mut subs = Vec::new();
        if let Some(services) = &self.services {
            subs.push(Subscription::run_with(Asks(services.clone()), asks_stream));
        }
        for t in &self.ide.terms {
            subs.push(Subscription::run_with(t.feed(), terminal_stream));
        }
        Subscription::batch(subs)
    }
}

impl Default for State {
    fn default() -> State {
        State::new()
    }
}

pub(crate) const STOPPED: &str = "Lattice's services stopped before answering.";

/// A field of the endpoint form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    Id,
    Label,
    BaseUrl,
    Model,
    KeyName,
    Note,
    Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFlag {
    Enabled,
    Anthropic,
}

/// The endpoint a form describes. The managed llama.cpp row keeps its kind; any other is Anthropic or
/// OpenAI-compatible, as chosen.
fn endpoint_of_form(f: &EndpointForm) -> lattice_core::registry::ModelEndpoint {
    use lattice_core::registry::EndpointKind;
    let managed = f.editing.as_deref() == Some("llamacpp-local");
    lattice_core::registry::ModelEndpoint {
        id: f.id.trim().to_string(),
        label: f.label.trim().to_string(),
        kind: if managed {
            EndpointKind::Llamacpp
        } else if f.anthropic {
            EndpointKind::Anthropic
        } else {
            EndpointKind::OpenaiCompatible
        },
        base_url: f.base_url.trim().to_string(),
        model: f.model.trim().to_string(),
        api_key_name: f.key_name.trim().to_string(),
        enabled: f.enabled,
        builtin: f.builtin,
        note: f.note.clone(),
    }
}

impl State {
    /// Run a confirmed registry act off the window's thread: the registry written by lattice-core's writer (Python's
    /// `upsert`/`remove`, byte for byte), a key kept in or forgotten from Credential Manager.
    fn act(&mut self, services: &Arc<Services>, act: RegistryAct) -> Task<Msg> {
        use lattice_core::{keys, registry};
        self.acting = true;
        let state = services.state.clone();
        Task::perform(
            off_thread(move || {
                let env = lattice_core::ProcessEnv;
                let path = registry::config_path(&env, &state);
                match act {
                    RegistryAct::Save { endpoint, key } => {
                        let name = endpoint.api_key_name.clone();
                        let label = endpoint.label.clone();
                        if let Some(key) = key {
                            if name.is_empty() {
                                return Err("A key was typed but the endpoint names no key: give it a key name, or clear the key.".to_string());
                            }
                            let secret = lattice_agents_secret(key);
                            keys::vault_store(&name, &secret)?;
                        }
                        registry::upsert(&path, *endpoint).map_err(|e| e.to_string())?;
                        Ok(format!("Saved {label} in {}.", path.display()))
                    }
                    RegistryAct::Remove { id } => {
                        registry::remove(&path, &id).map_err(|e| e.to_string())?;
                        Ok(format!("{id} is removed (a built-in is disabled instead, as the Python Lattice does)."))
                    }
                    RegistryAct::ForgetKey { name } => match keys::vault_remove(&name)? {
                        true => Ok(format!("{name} is forgotten from Credential Manager.")),
                        false => Ok(format!("Credential Manager held no {name}.")),
                    },
                }
            }),
            |r| Msg::Acted(r.unwrap_or_else(|| Err(STOPPED.to_string()))),
        )
    }
}

impl State {
    /// A Training Studio write, off the window's thread: lattice's port of `workspace.py`'s writers, each refusing
    /// when the workspace on disk is not the one this window last opened.
    fn train(&mut self, act: TrainingAct) -> Task<Msg> {
        if self.training.writing {
            return Task::none();
        }
        let current = self.training.workspace.as_ref().and_then(|w| w.as_ref().ok()).cloned();
        if current.is_none() && !matches!(act, TrainingAct::Create(_)) {
            return Task::none();
        }
        self.training.writing = true;
        self.training.said = None;
        Task::perform(
            off_thread(move || match act {
                TrainingAct::Create(root) => training::create_workspace(&root)
                    .map(|w| (w, "Workspace created: an empty pool at revision 0. Nothing is trained.".to_string())),
                TrainingAct::Add { file, source, license, group } => {
                    let ws = current.expect("checked");
                    let before = ws.revision.number;
                    training::add_document(&ws, &file, &source, &license, &group).map(|(w, doc)| {
                        let words = if w.revision.number == before {
                            format!("{} is already in the pool with this attribution; nothing was written.", doc.name)
                        } else {
                            format!("{} added as revision {}: its normalised text, named by its SHA-256.", doc.name, w.revision.number)
                        };
                        (w, words)
                    })
                }
                TrainingAct::Remove(id) => training::remove_document(&current.expect("checked"), &id).map(|w| {
                    let words = "Removed from this pool. Its text and the plans that bind it are kept.".to_string();
                    (w, words)
                }),
                TrainingAct::Export(file) => training::export_corpus(&current.expect("checked"), &file).map(|(w, path)| {
                    let words = format!(
                        "Document snapshot exported to {}. Tokenizing, splitting and admission to training are still to do.",
                        path.display()
                    );
                    (w, words)
                }),
                TrainingAct::SavePlan { preset, experts } => {
                    training::save_plan(&current.expect("checked"), preset, &experts).map(|(w, path)| {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        (w, format!("Plan saved: {name}. Training has not started."))
                    })
                }
            }),
            |r| Msg::Trained(Box::new(r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        )
    }

    /// Choose the GGUF model Local runs, off the window's thread, as the Python model bar chooses it
    /// (`set_selected_model`: a name that is no GGUF file in the models folder is refused, and the choice is written to
    /// `analyst_model.json` as Python writes it). Asked nothing first, as there: it is a preference, read again at the
    /// next Local turn, and a server running another model is restarted for it then.
    fn choose_local(&mut self, services: &Arc<Services>, name: String) -> Task<Msg> {
        use lattice_core::local_model;
        self.acting = true;
        let state = services.state.clone();
        Task::perform(
            off_thread(move || {
                let paths = lattice_core::llama::files::LlamaPaths::from_env(&lattice_core::ProcessEnv);
                local_model::set_selected_model(&state, &paths.models_dir, &name)
                    .map(|name| {
                        format!(
                            "Local runs {name} from its next turn; a server running another model is restarted for it then."
                        )
                    })
                    .map_err(|why| format!("The model was not chosen: {why}."))
            }),
            |r| Msg::LocalChosen(r.unwrap_or_else(|| Err(STOPPED.to_string()))),
        )
    }
}

/// A typed key as the secret type the key store takes (it prints as stars).
fn lattice_agents_secret(key: String) -> lattice_core::keys::SecretString {
    lattice_core::keys::SecretString::new(key)
}

/// The model registry as the Models tab lists it. Reads `model_endpoints.json` and whether each named key is present
/// (the key store's own rule; a value is never read into this list).
fn endpoints_of(state: &lattice_core::StateRoot) -> Endpoints {
    use lattice_core::registry;
    let env: Arc<dyn lattice_core::Env> = Arc::new(lattice_core::ProcessEnv);
    let keys = lattice_core::KeyStore::new(env.clone(), state);
    let path = registry::config_path(env.as_ref(), state);
    let report = registry::load_with_report(&path);
    let rows = report
        .endpoints
        .into_iter()
        .map(|saved| (registry::runtime_endpoint(saved.clone(), env.as_ref(), state), saved))
        .map(|(e, saved)| EndpointRow {
            label: e.label.clone(),
            kind: e.kind.as_str(),
            model: e.model.clone(),
            local: e.local(env.as_ref()),
            ready: e.ready(&keys),
            status: e.status(&keys),
            key_name: e.api_key_name.clone(),
            builtin: e.builtin,
            key_source: keys.source(&e.api_key_name).map(|s| match s {
                lattice_core::keys::KeySource::Environment => "the environment".to_string(),
                lattice_core::keys::KeySource::CredentialManager => "Credential Manager".to_string(),
                lattice_core::keys::KeySource::File(path) => format!("a plain-text file, {}", path.display()),
            }),
            saved,
        })
        .collect();
    Endpoints { path: path.display().to_string(), rows, issues: report.issues.iter().map(|i| i.as_str().to_string()).collect() }
}

pub(crate) fn done(r: Option<Result<(), String>>) -> Msg {
    Msg::Done(r.unwrap_or_else(|| Err(STOPPED.to_string())))
}

/// Run `future` on the services' own runtime; `None` when it ended first.
pub(crate) fn on<T: Send + 'static>(
    services: &Services,
    future: impl std::future::Future<Output = T> + Send + 'static,
) -> impl std::future::Future<Output = Option<T>> + Send + 'static {
    let handle = services.handle().spawn(future);
    async move { handle.await.ok() }
}

/// Run `work` on a thread of its own and hand back its result.
pub(crate) async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let spawned = std::thread::Builder::new().name("centcom-lattice-call".into()).spawn(move || {
        let _ = tx.send(work());
    });
    if spawned.is_err() {
        return None;
    }
    rx.await.ok()
}

/// The follow's subscription data: it hashes by conversation and generation only, so `after` moving as events arrive
/// does not start it again.
#[derive(Clone)]
struct Follow {
    service: Arc<dyn AgentChatService>,
    conversation: String,
    after: u64,
    generation: u64,
}

impl Hash for Follow {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.conversation.hash(state);
        self.generation.hash(state);
    }
}

fn follow_stream(follow: &Follow) -> BoxStream<'static, Msg> {
    let id = follow.conversation.clone();
    match follow.service.follow(&follow.conversation, follow.after) {
        Ok(stream) => {
            let batch_id = id.clone();
            stream
                .map(move |events| Msg::Batch(batch_id.clone(), events))
                .chain(futures::stream::once(async move { Msg::FollowEnded(id) }))
                .boxed()
        }
        Err(refusal) => futures::stream::once(async move { Msg::Done(Err(refusal.message)) }).boxed(),
    }
}

/// The services, hashed as one thing: there is only ever one.
#[derive(Clone)]
struct Asks(Arc<Services>);

impl Hash for Asks {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "lattice-asks".hash(state);
    }
}

fn asks_stream(asks: &Asks) -> BoxStream<'static, Msg> {
    asks.0.asks().map(|ask| Msg::Asked(AskCell(Arc::new(Mutex::new(Some(ask)))))).boxed()
}

#[derive(Clone)]
struct Changed(Arc<Services>);

impl Hash for Changed {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "lattice-chats-changed".hash(state);
    }
}

/// The conversation list changed (a turn ended, a conversation was archived, the web wrote): read it again.
fn changed_stream(changed: &Changed) -> BoxStream<'static, Msg> {
    changed.0.chat.chats_changed().map(|_| Msg::Refresh).boxed()
}

struct McpChanged(Arc<Services>);

impl Hash for McpChanged {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "lattice-mcp-changed".hash(state);
    }
}

/// An MCP server started, stopped, ended or was decided about: read the servers again.
fn mcp_stream(changed: &McpChanged) -> BoxStream<'static, Msg> {
    let receiver = changed.0.chat.mcp_changed();
    futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.changed().await.ok()?;
        Some((Msg::Ide(ide::IdeMsg::Tools(ide::tools::ToolsMsg::Changed)), receiver))
    })
    .boxed()
}

struct BrowserChanged(Arc<Services>);

impl Hash for BrowserChanged {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "lattice-browser-changed".hash(state);
    }
}

/// The agent's browser started, stopped, moved to another page or failed: read it again.
fn browser_stream(changed: &BrowserChanged) -> BoxStream<'static, Msg> {
    let receiver = changed.0.chat.browser().changed();
    futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.changed().await.ok()?;
        Some((Msg::Ide(ide::IdeMsg::Tools(ide::tools::ToolsMsg::BrowserChanged)), receiver))
    })
    .boxed()
}

/// A terminal's output, then its end.
fn terminal_stream(feed: &ide::term::Feed) -> BoxStream<'static, Msg> {
    ide::term::feed_stream(feed).map(|(id, event)| Msg::Ide(ide::IdeMsg::TermEvent(id, event))).boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_are_named_as_the_command_line_names_them() {
        for tab in Tab::ALL {
            let name = match tab {
                Tab::Chat => "chat",
                Tab::Runs => "runs",
                Tab::Models => "models",
                Tab::Training => "training",
                Tab::Morphometry => "morphometry",
                Tab::Foundry => "foundry",
                Tab::System => "system",
            };
            assert_eq!(Tab::from_name(name), Some(tab));
        }
        assert_eq!(Tab::from_name("waterfall"), None);
        // The Code tab became the IDE's editor: its old name opens the Chat and IDE.
        assert_eq!(Tab::from_name("code"), Some(Tab::Chat));
    }

    #[test]
    fn a_form_keeps_the_managed_servers_kind_and_otherwise_chooses_one() {
        use lattice_core::registry::EndpointKind;
        let mut f = EndpointForm { id: " my-vllm ".into(), label: "Mine".into(), enabled: true, ..EndpointForm::default() };
        assert_eq!(endpoint_of_form(&f).kind, EndpointKind::OpenaiCompatible);
        assert_eq!(endpoint_of_form(&f).id, "my-vllm");
        f.anthropic = true;
        assert_eq!(endpoint_of_form(&f).kind, EndpointKind::Anthropic);
        f.editing = Some("llamacpp-local".into());
        assert_eq!(endpoint_of_form(&f).kind, EndpointKind::Llamacpp);
    }

    #[test]
    fn a_typed_key_is_dropped_with_a_cancelled_form() {
        let mut state = State::new();
        let _ = state.update(Msg::NewEndpoint);
        let _ = state.update(Msg::Form(FormField::Key, "typed".into()));
        assert_eq!(state.form.as_ref().unwrap().key, "typed");
        let _ = state.update(Msg::CancelForm);
        assert!(state.form.is_none());
    }

    #[test]
    fn save_asks_first_and_an_edit_takes_the_question_back() {
        let mut state = State::new();
        let _ = state.update(Msg::NewEndpoint);
        let _ = state.update(Msg::SaveEndpoint);
        assert!(state.form.as_ref().unwrap().confirming, "the first Save only shows what it will do");
        let _ = state.update(Msg::Form(FormField::Label, "x".into()));
        assert!(!state.form.as_ref().unwrap().confirming, "an edit after the summary needs a new Save");
    }

    #[test]
    fn nothing_starts_until_the_page_is_opened_and_a_new_page_is_quiet() {
        let state = State::new();
        assert!(state.services.is_none() && !state.starting && !state.busy());
        assert!(state.dialog.is_none());
    }

    #[test]
    fn a_dialog_ignores_an_answer_in_its_first_half_second() {
        use lattice_core::ports::ConfirmRequest;
        let mut state = State::new();
        let (tx, rx) = futures::channel::oneshot::channel();
        state.waiting_dialogs.push_back(core::tests_ask(ConfirmRequest::AttachFolder { path: "C:/w".into() }, tx));
        state.next_dialog();
        let _ = state.update(Msg::DialogAnswer(true));
        assert!(state.dialog.is_some(), "a click in the quiet half second answered the dialog");
        state.dialog.as_mut().unwrap().shown_at = Instant::now() - DIALOG_QUIET;
        let _ = state.update(Msg::DialogAnswer(true));
        assert!(state.dialog.is_none());
        assert_eq!(futures::executor::block_on(rx), Ok(true));
    }
}
