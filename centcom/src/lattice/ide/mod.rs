//! The Lattice page's Chat and IDE: the Chat feature turned into a Cursor-style Chat + IDE (since 2026-10-07), laid
//! out like Antigravity, Codex and Claude Code, in Alelyon's own black and gold.
//!
//! Its layout is the IDE's: an activity bar; a side bar with the Explorer (the attached folder's tree), the Chats (every
//! conversation, the running and those waiting for the reader first, as an agent manager lists them) and the Changes
//! (what the agent proposed, its checkpoints and its standing approvals); the editor with its tabs (files, the agent's
//! diffs to review, a command's whole output); a bottom panel with the agent's commands and a terminal; the agent on
//! the right, its transcript a list of compact steps; and a status bar.
//!
//! What may happen is unchanged from the chat's: the agent's edits are staged and written only when kept (by hunk,
//! file or all), every command it asks for waits for the reader, and every decision that widens what it may do is
//! asked by the core itself in its own dialog. The person's own typing is new: a file opened in the editor is saved
//! by the person (Ctrl+S), guarded as follows ([`files`]): never over a file that changed on disk since it
//! was opened, never outside the folder or past the core's path rules, an authority file only after a confirmation,
//! and only UTF-8 text.
//!
//! Nothing here reads the disk on the window's thread: listings, opens and saves run on threads of their own with a
//! spinner while they do.
//!
//! The bottom panel's terminals ([`term`]) are the person's own PowerShell, a real interactive
//! terminal: what runs there is only what the person types or pastes into it.
//!
//! The editor finds and replaces (Ctrl+F, Ctrl+H) and goes to a line (Ctrl+G) ([`find`]); the Search view searches
//! the folder (Ctrl+Shift+F) ([`search`]); and Ctrl+K asks the agent to edit the selected lines, its change then waiting
//! in the review as any of its changes does.

pub mod activity;
pub mod agent;
pub mod board;
pub mod attach;
pub mod buffer;
pub mod complete;
pub mod editing;
pub mod file_icons;
// Replaces and places files through Win32 (ReplaceFileW, MoveFileExW), so it may use unsafe code, as the job object
// does.
#[allow(unsafe_code)]
pub mod files;
pub mod find;
pub mod grid;
pub mod groups;
pub mod sessions;
pub mod record;
pub mod highlight;
pub mod jump;
pub mod mention;
pub mod outside_steps;
pub mod notes;
// Windows' folder picker (IFileOpenDialog, through COM), so it may use unsafe code, as the job object does.
#[allow(unsafe_code)]
pub mod picker;
pub mod plan;
pub mod projects;
pub mod search;
pub mod slash;
pub mod term;
pub mod connections;
pub mod tools;
pub mod tree;
pub mod update;
pub mod view;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::text_editor;

use lattice_protocol::conversation::{AllowEntry, CheckpointView, Lines, RankedPath};

pub use update::IdeMsg;

/// The side bar's, the agent panel's and the bottom panel's sizes when the window opens, and their bounds.
pub const SIDE_WIDTH: f32 = 252.0;
pub const AGENT_WIDTH: f32 = 430.0;
pub const PANEL_HEIGHT: f32 = 210.0;
pub const SIDE_RANGE: (f32, f32) = (170.0, 520.0);
pub const AGENT_RANGE: (f32, f32) = (320.0, 820.0);
pub const PANEL_RANGE: (f32, f32) = (96.0, 640.0);
/// The editor beside the chat (chat first): its width when it first opens, and its bounds.
pub const EDITOR_WIDTH: f32 = 680.0;
pub const EDITOR_RANGE: (f32, f32) = (360.0, 1400.0);
/// The chat's reading column: never wider than this, centred in the room it has.
pub const CHAT_MAX: f32 = 920.0;
/// The most files the editor's head names as being worked on.
pub const WORKING_KEPT: usize = 8;
/// The editor's font size and the height of one of its lines (fixed, so the line numbers beside it stay level).
pub const CODE_SIZE: f32 = 13.0;
pub const LINE_HEIGHT: f32 = 19.0;
/// The editor's padding, the same above the text and above its line numbers.
pub const CODE_PAD: f32 = 8.0;
/// The most tabs open at once: opening one more closes the oldest that has no unsaved edits.
pub const MAX_TABS: usize = 12;
/// How many closed files Ctrl+Shift+T can bring back.
pub const CLOSED_KEPT: usize = 20;
/// The most lines a page of a read-only file or an output shows.
pub const PAGE_LINES: u32 = 400;
/// How long a confirmation ignores input after it appears (as the core's dialogs do).
pub const CONFIRM_QUIET: Duration = Duration::from_millis(500);

/// The files a batch of a chat's events touches, in order: the paths its file tools name (reading, editing, writing;
/// not deleting) and the files its changes and commands staged, each with the staged change's id when it has one.
pub fn worked_files(events: &[lattice_protocol::conversation::ConversationEvent]) -> Vec<(String, Option<String>)> {
    use lattice_protocol::conversation::ConversationEventKind as Ev;
    let mut out = Vec::new();
    for e in events {
        match &e.kind {
            Ev::ToolCall { tool, target: Some(path), .. } if names_a_file(tool) && tool != "delete_file" && !path.trim().is_empty() => {
                out.push((path.trim().to_string(), None));
            }
            Ev::Staged { change, path, .. } => out.push((path.clone(), Some(change.to_string()))),
            Ev::CommandEffect { files, .. } => out.extend(files.iter().map(|f| (f.path.clone(), None))),
            _ => {}
        }
    }
    out
}

/// A file a running agent works on, as the editor's head names it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkedFile {
    /// The file, as the agent named it (relative to its folder).
    pub path: String,
    /// The chat or task working on it (its id), and how the head names it.
    pub by: String,
    pub who: String,
    /// It works in the folder the editor shows, so the file can open there.
    pub here: bool,
}

/// The newest change a batch of a chat's events staged, which follow mode opens.
pub fn followed_change(events: &[lattice_protocol::conversation::ConversationEvent]) -> Option<String> {
    use lattice_protocol::conversation::ConversationEventKind as Ev;
    events.iter().rev().find_map(|e| match &e.kind {
        Ev::Staged { change, .. } => Some(change.to_string()),
        _ => None,
    })
}

/// What the side bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// What the agents work on and what waits for the reader, a list each ([`WorkList`]): the side bar's first view.
    Work,
    /// The open folder's files: no longer on the activity bar (the agents' files are followed in the editor and named
    /// on the Work side), but the jump menu's folder and New file still show it.
    Explorer,
    Search,
    Chats,
    Changes,
    /// The agent's tools and its MCP servers ([`tools`]).
    Tools,
}

impl Side {
    /// The activity bar's views, in its order.
    pub const ALL: [Side; 5] = [Side::Work, Side::Search, Side::Chats, Side::Changes, Side::Tools];

    pub fn title(self) -> &'static str {
        match self {
            Side::Work => "Work: what the agents work on, your checklists, changes and tools",
            Side::Explorer => "Explorer",
            Side::Search => "Search (Ctrl+Shift+F)",
            Side::Chats if crate::lattice::ONE_SURFACE => "Groups and topics",
            Side::Chats => "Chats",
            Side::Changes => "Changes",
            Side::Tools => "Tools and MCP servers",
        }
    }

    /// The activity bar's glyph.
    pub fn glyph(self) -> &'static str {
        match self {
            // IDENTICAL TO: the lists in hand.
            Side::Work => "\u{2261}",
            Side::Explorer => "\u{2750}",
            Side::Search => "\u{2315}",
            Side::Chats => "\u{25C8}",
            Side::Changes => "\u{00B1}",
            // SOFTWARE-FUNCTION SYMBOL.
            Side::Tools => "\u{2394}",
        }
    }
}

/// The open folder's stack as the advisor's catalog (lattice-core `advice`) reads its manifests: what it uses,
/// and the improvements the catalog suggests for that.
#[derive(Clone, Debug, Default)]
pub struct StackReview {
    /// (area title, option name), in the catalog's order.
    pub detected: Vec<(String, String)>,
    pub suggestions: Vec<lattice_core::advice::Suggestion>,
    pub dismissed: bool,
    /// The suggestions already sent to the agent.
    pub asked: HashSet<usize>,
}

impl StackReview {
    pub fn of(root: &std::path::Path) -> StackReview {
        let project = lattice_core::advice::scan(root);
        StackReview {
            detected: lattice_core::advice::detected(&project).into_iter().map(|(a, c)| (a.title.clone(), c.name.clone())).collect(),
            suggestions: lattice_core::advice::review(&project),
            ..StackReview::default()
        }
    }
}

/// A list of the Work side, each folding to its head: the files the agents work on now, the agent's to-do list, the
/// changes waiting for review, the stack's suggestions, the checklists the agents gave the reader, and the tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkList {
    Working,
    Todos,
    Changes,
    Stack,
    Checklists,
    Tools,
}

impl WorkList {
    /// The lists in the order the Work side shows them: the short ones first, so that long checklists never push the
    /// changes waiting out of sight, and the tools last.
    pub const ORDER: [WorkList; 6] =
        [WorkList::Working, WorkList::Todos, WorkList::Changes, WorkList::Stack, WorkList::Checklists, WorkList::Tools];

    /// Its key among the reader's folds ([`Ide::side_folds`]).
    pub fn key(self) -> &'static str {
        match self {
            WorkList::Working => "work:working",
            WorkList::Todos => "work:todos",
            WorkList::Checklists => "work:checklists",
            WorkList::Changes => "work:changes",
            WorkList::Stack => "work:stack",
            WorkList::Tools => "work:tools",
        }
    }
}

/// How much of a turn's work the chat shows: folded to its one line, opened to its latest steps, or opened to all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    Folded,
    Open,
    All,
}

/// What the bottom panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    /// The agent's commands in the open chat, each with its output.
    Commands,
    /// The person's own terminal.
    Terminal,
}

/// A border being dragged to resize what is beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Splitter {
    Side,
    /// The right column's border: the editor beside the chat, or what sits beside the board or the tasks grid.
    Right,
    Panel,
}

/// Which chats the Chats view lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatFilter {
    #[default]
    All,
    Running,
    NeedsYou,
}

/// The folder's files, as listed and as a tree.
#[derive(Clone, Debug)]
pub struct Listing {
    /// The workspace id it is the listing of.
    pub folder: String,
    pub paths: Vec<String>,
    pub truncated: bool,
    pub withheld: Vec<String>,
    pub tree: tree::Node,
}

/// A file being edited.
pub struct Editing {
    pub content: text_editor::Content,
    /// SHA-256 of the bytes on disk the text was read from, or last written: a save is checked against it.
    pub base_sha256: String,
    pub bom: bool,
    pub ending: buffer::Ending,
    /// The text as opened or last saved: the file is dirty while the editor's text differs from it.
    pub saved: String,
    pub dirty: bool,
    pub history: buffer::History,
    pub authority: bool,
    pub saving: bool,
    /// The text a save in flight is writing: it becomes `saved` when the save succeeds (edits made meanwhile stay
    /// unsaved).
    pub writing: Option<String>,
    pub lines: usize,
    /// The longest line, in characters: the editor is at least that wide, and scrolls across.
    pub longest: usize,
    /// The editor's scroll offset and visible height, as last reported (to keep the cursor in view).
    pub viewport: Option<(f32, f32)>,
    /// Its scroll offset across and visible width, as last reported (to keep a match in view).
    pub across: Option<(f32, f32)>,
    /// The file's indentation unit (spaces or a tab), learnt when it opened, as editors keep it.
    pub unit: String,
    /// The line numbers, one per line, built when the count changes (not on every frame): the text and where each
    /// number starts in it.
    pub gutter: (String, Vec<usize>),
}

impl Editing {
    /// The gutter's text for `lines` lines, `digits` wide, and each number's start in it.
    pub fn numbers(lines: usize) -> (String, Vec<usize>) {
        let digits = lines.max(1).to_string().len().max(3);
        let mut text = String::with_capacity(lines * (digits + 1));
        let mut starts = Vec::with_capacity(lines + 1);
        for n in 1..=lines.max(1) {
            starts.push(text.len());
            if n > 1 {
                starts.pop();
                text.push('\n');
                starts.push(text.len());
            }
            text.push_str(&format!("{n:>digits$}"));
        }
        starts.push(text.len());
        (text, starts)
    }
}

/// What a file's tab holds.
pub enum Body {
    Loading,
    Editing(Box<Editing>),
    /// Shown a page at a time through the core: too large, binary, or not UTF-8.
    Paged { why: String, lines: Option<Lines>, reading: bool, colours: Vec<highlight::Spans> },
    Failed(String),
}

/// What an editor tab shows.
pub enum TabKind {
    File {
        path: String,
        lang: highlight::Lang,
        body: Body,
        /// The last save's or reload's result: its words, and whether they warn.
        note: Option<(String, bool)>,
    },
    /// An agent's change to review, by hunk, file or all.
    Diff { change: String, path: String },
    /// A command's whole output.
    Output { call: String, label: String, lines: Option<Lines>, reading: bool },
    /// The Tools page: the agent's tools and its MCP servers ([`tools`]).
    Tools,
    /// The Connections page: the services the agent uses in its own browser
    /// ([`connections`]).
    Connections,
    /// A project's page ([`projects`]), by its id, with its name as the tab shows it.
    Project { id: String, name: String },
    /// A document the agent saved (an artifact): one version of it, read through the core.
    Artifact {
        conversation: String,
        name: String,
        version: u32,
        view: Option<Result<lattice_protocol::conversation::ArtifactView, String>>,
        /// Its Markdown, parsed once it is read.
        rendered: Option<iced::widget::markdown::Content>,
        /// Markdown shown as its source instead.
        source: bool,
    },
}

pub struct EditorTab {
    pub id: u64,
    pub kind: TabKind,
    /// The editor it belongs to: [`YOURS`], or the chat or task whose agent's work it shows.
    pub editor: String,
}

/// The reader's own editor, numbered 1: what they open themselves.
pub const YOURS: &str = "you";

/// One of the numbered editors beside the chat: the reader's own, or an agent's, holding the files that agent works
/// on (its chat's or task's id is its key).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorSlot {
    pub key: String,
    /// How its tab names it: "Yours", "This chat" or a task's title.
    pub who: String,
    /// The tab on show in it while another editor is on show.
    pub active: Option<u64>,
}

impl EditorTab {
    pub fn title(&self) -> String {
        match &self.kind {
            TabKind::File { path, .. } => file_name(path).to_string(),
            TabKind::Diff { path, .. } => format!("Review {}", file_name(path)),
            TabKind::Output { label, .. } => format!("Output: {}", crate::ui::cut(label, 24)),
            TabKind::Tools => "Tools".to_string(),
            TabKind::Connections => "Connections".to_string(),
            TabKind::Project { name, .. } => format!("Project: {}", crate::ui::cut(name, 24)),
            TabKind::Artifact { name, version, view, .. } => {
                let title = match view {
                    Some(Ok(v)) => v.title.as_str(),
                    _ => name.as_str(),
                };
                format!("{} v{version}", crate::ui::cut(title, 24))
            }
        }
    }

    pub fn dirty(&self) -> bool {
        matches!(&self.kind, TabKind::File { body: Body::Editing(e), .. } if e.dirty)
    }

    pub fn path(&self) -> Option<&str> {
        match &self.kind {
            TabKind::File { path, .. } | TabKind::Diff { path, .. } => Some(path),
            TabKind::Output { .. }
            | TabKind::Tools
            | TabKind::Connections
            | TabKind::Project { .. }
            | TabKind::Artifact { .. } => None,
        }
    }

    /// The editor's scrollable, by tab.
    pub fn scroll_id(&self) -> iced::widget::Id {
        scroll_id(self.id)
    }
}

pub fn scroll_id(tab: u64) -> iced::widget::Id {
    iced::widget::Id::from(format!("ide-editor-{tab}"))
}

/// The composer's id, to give it the focus.
pub fn composer_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-composer")
}

/// An editor's id, by tab, to give it the focus back.
pub fn editor_id(tab: u64) -> iced::widget::Id {
    iced::widget::Id::from(format!("ide-code-{tab}"))
}

/// The find bar's box, the replace box, Go to line's box and the inline edit's box.
pub fn find_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-find")
}

pub fn replace_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-replace")
}

pub fn goto_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-goto")
}

pub fn inline_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-inline")
}

/// The quick-open box's id.
pub fn quick_id() -> iced::widget::Id {
    iced::widget::Id::new("ide-quick-open")
}

/// A path's last part.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The quick-open box (Ctrl+P): the files whose names fit what is typed, ranked as the file picker ranks them.
#[derive(Debug, Default)]
pub struct Quick {
    pub query: String,
    pub results: Vec<RankedPath>,
    pub selected: usize,
}

/// A question this window asks before it acts (drawn by its own code, refusing by default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Save an authority file.
    SaveAuthority { tab: u64 },
    /// Make a new authority file.
    CreateAuthority { path: String },
    /// Close a tab whose edits are not saved.
    Discard { tab: u64 },
    /// Read a file again from disk, dropping unsaved edits.
    Reload { tab: u64 },
    /// Open another folder, closing tabs with unsaved edits (`native`: chosen in Windows' picker or dropped).
    SwitchFolder { path: String, native: bool },
    /// Paste text into a terminal: `runs`, its lines would run at once (else it is long).
    Paste { term: u64, text: String, runs: bool },
    /// Close a terminal in which a program still runs.
    CloseTerminal { term: u64 },
}

/// The IDE's own state (the chat's, the services and the dialogs stay on [`super::State`]).
pub struct Ide {
    /// The folder the editor reads and writes (the chat's attached folder).
    pub folder: Option<Arc<files::Folder>>,
    /// The path typed into the open-folder box.
    pub folder_path: String,
    pub opening: bool,
    /// Windows' folder picker is open.
    pub picking: bool,
    pub listing: Option<Result<Listing, String>>,
    pub listing_busy: bool,
    pub expanded: HashSet<String>,
    pub filter: String,
    pub side: Side,
    pub side_open: bool,
    pub panel_open: bool,
    pub panel: PanelTab,
    pub tabs: Vec<EditorTab>,
    /// The tab on show, in the editor on show.
    pub active: Option<u64>,
    next_id: u64,
    /// The numbered editors, the reader's first, then each agent's in the order it began work; and the one on show.
    pub editors: Vec<EditorSlot>,
    pub shown: String,
    pub quick: Option<Quick>,
    pub confirm: Option<(Confirm, Instant)>,
    pub side_width: f32,
    pub agent_width: f32,
    pub panel_height: f32,
    /// A border being dragged, and the pointer's last position along it.
    pub drag: Option<(Splitter, Option<f32>)>,
    pub chat_filter: ChatFilter,
    /// Follow the agents: the editor opens beside the chat on the files they work on, and each change the open chat
    /// stages opens there as its diff, as it is made.
    pub follow: bool,
    /// The editor is on show beside the chat (Ctrl+E): the chat is the page, the editor a panel on its right.
    pub editor_open: bool,
    /// The editor panel's width.
    pub editor_width: f32,
    /// The reader hid the editor while agents worked: following leaves it hidden until a turn starts.
    pub editor_dismissed: bool,
    /// The files the running agents work on, newest first.
    pub working_on: Vec<WorkedFile>,
    /// What the Chats list is searched for (titles), the folders whose every chat is shown, and whether the
    /// projects are listed under Projects.
    pub chat_search: String,
    /// What was said in sessions that matches the search (Enter): the words searched and the best match of each
    /// session (lattice-core `search_sessions`).
    pub chat_hits: Option<(String, Vec<lattice_core::convo::recall::Hit>)>,
    pub chat_more: HashSet<String>,
    pub projects_open: bool,
    /// Tool steps opened to show their output.
    pub expanded_calls: HashSet<String>,
    /// A new file's name being typed in the Explorer.
    pub new_file: Option<String>,
    /// The files the Explorer's filter finds, ranked when the filter or the listing changes (not on every frame).
    pub filtered: Vec<RankedPath>,
    /// Where the Explorer's list and the search results stand scrolled: each builds only the rows on show.
    pub explorer_at: crate::ui::Scrolled,
    pub search_at: crate::ui::Scrolled,
    /// A user turn being edited, to send again in its place.
    pub editing_turn: Option<String>,
    /// A refusal's note being typed, by call.
    pub notes: std::collections::HashMap<String, String>,
    pub checkpoints: Option<Result<Vec<CheckpointView>, String>>,
    pub permissions: Option<Result<Vec<AllowEntry>, String>>,
    /// The agent command the bottom panel shows.
    pub command: Option<String>,
    pub command_lines: Option<Lines>,
    /// A folder named on the command line, and a file of it, to open once the services run.
    pub startup: Option<(String, Option<String>)>,
    /// The person's terminals, the one on show, and whether it has the keyboard.
    pub terms: Vec<term::Term>,
    /// The chats side by side, shown in the editor's place when on.
    pub grid: grid::Grid,
    /// The jump menu, while it is open.
    pub jump: Option<jump::Jump>,
    pub term_active: Option<u64>,
    pub term_focus: bool,
    next_term: u64,
    pub term_starting: bool,
    /// Commands the agent runs in a new terminal, waiting for it to start: its id, the command, and who to tell.
    pub term_runs: Vec<(u64, String, futures::channel::oneshot::Sender<Result<u64, String>>)>,
    pub term_problem: Option<String>,
    /// The terminal pane's size in cells, as last measured (a new terminal starts at it).
    pub term_cells: (u16, u16),
    /// The find bar of the editor on show (Ctrl+F; with the replace row, Ctrl+H).
    pub find: Option<find::Find>,
    /// Go to line's box (Ctrl+G), and what is typed in it.
    pub goto: Option<String>,
    /// The inline edit being asked for (Ctrl+K).
    pub inline: Option<find::Inline>,
    /// The Search view.
    pub search: search::SearchView,
    /// A search result to select once its file has opened: its path, line (from 1) and byte range.
    pub reveal: Option<(String, u32, usize, usize)>,
    /// The message being sent is not the composer's (an inline edit's, or a suggested task's prompt): the composer's
    /// draft stays.
    pub inline_sending: bool,
    /// The message being sent is a suggested task's Start: on one chat surface it runs beside the chat, which stays.
    pub task_starting: bool,
    /// The suggested tasks whose prompts are shown on their chips.
    pub task_prompts: HashSet<String>,
    /// The open folder's stack and the catalog's suggestions for it (read when the folder opens).
    pub stack: Option<StackReview>,
    /// The images attached to the composer's message (at most 4), sent with it.
    pub attached: Vec<attach::Attached>,
    /// The Tools page's and view's state.
    pub tools: tools::Tools,
    /// The project the Chats view shows, which a new chat joins.
    pub project: Option<String>,
    /// The projects, and a project's page being edited.
    pub projects: projects::ProjectsState,
    /// The work's groups (`groups`).
    pub groups: groups::GroupsState,
    /// The notes beside the open files' lines.
    pub line_notes: notes::NotesState,
    /// The composer's commands list (`/name`).
    pub slash: slash::Slash,
    /// The composer's file mentions list (`@path`).
    pub mention: mention::Mentioning,
    /// The editor's Tab completions.
    pub completion: complete::Completion,
    /// The checklists agents outside the chats gave the reader, not yet done (`outside_steps`).
    pub outside_steps: Vec<lattice_core::user_steps::ExternalList>,
    /// The tray's checklists folded to their heads, by key (`chat:<id>` for the chat's own,
    /// `outside:<id>` for other agents'); the rest show their steps.
    pub folded_steps: HashSet<String>,
    /// The turns whose work the reader folded or opened, by turn id; any other is open while it runs and folded after.
    pub work_folds: std::collections::HashMap<String, Fold>,
    /// The older topics of the stream folded to their line, by topic id; the rest show their conversation.
    pub folded_topics: HashSet<String>,
    /// The parts folded or opened by the reader: on the Chats side `groups`, `projects` and `topics` (a section
    /// folded to its head), `group:<id>` and `project:<id>` (topics shown beneath one); on the Work side each list
    /// ([`WorkList::key`]); in the tools `tools:modes` and `tools:use` (a section folded to its head).
    pub side_folds: HashSet<String>,
    /// A message was sent since the app started: the empty chat's starts stay away after the first.
    pub sent: bool,
    /// Files whose tabs closed (by hand or past [`MAX_TABS`]), the latest last: Ctrl+Shift+T opens them again.
    pub closed: Vec<String>,
}

impl Default for Ide {
    fn default() -> Ide {
        Ide {
            folder: None,
            folder_path: String::new(),
            opening: false,
            picking: false,
            listing: None,
            listing_busy: false,
            expanded: HashSet::new(),
            filter: String::new(),
            side: Side::Work,
            side_open: true,
            panel_open: false,
            panel: PanelTab::Commands,
            tabs: Vec::new(),
            active: None,
            next_id: 1,
            editors: vec![EditorSlot { key: YOURS.into(), who: "Yours".into(), active: None }],
            shown: YOURS.into(),
            quick: None,
            confirm: None,
            side_width: SIDE_WIDTH,
            agent_width: AGENT_WIDTH,
            panel_height: PANEL_HEIGHT,
            drag: None,
            chat_filter: ChatFilter::All,
            follow: true,
            editor_open: false,
            editor_width: EDITOR_WIDTH,
            editor_dismissed: false,
            working_on: Vec::new(),
            chat_search: String::new(),
            chat_hits: None,
            chat_more: HashSet::new(),
            projects_open: false,
            expanded_calls: HashSet::new(),
            new_file: None,
            filtered: Vec::new(),
            explorer_at: Default::default(),
            search_at: Default::default(),
            editing_turn: None,
            notes: Default::default(),
            checkpoints: None,
            permissions: None,
            command: None,
            command_lines: None,
            startup: None,
            terms: Vec::new(),
            grid: grid::Grid::default(),
            jump: None,
            term_active: None,
            term_focus: false,
            next_term: 1,
            term_starting: false,
            term_runs: Vec::new(),
            term_problem: None,
            term_cells: (120, 8),
            find: None,
            goto: None,
            inline: None,
            search: search::SearchView::default(),
            reveal: None,
            inline_sending: false,
            task_starting: false,
            attached: Vec::new(),
            task_prompts: HashSet::new(),
            stack: None,
            tools: tools::Tools::default(),
            project: None,
            projects: projects::ProjectsState::default(),
            groups: groups::GroupsState::default(),
            line_notes: notes::NotesState::default(),
            slash: slash::Slash::default(),
            mention: mention::Mentioning::default(),
            completion: complete::Completion::default(),
            outside_steps: Vec::new(),
            folded_steps: HashSet::new(),
            work_folds: std::collections::HashMap::new(),
            folded_topics: HashSet::new(),
            side_folds: HashSet::new(),
            sent: false,
            closed: Vec::new(),
        }
    }
}

impl Ide {
    /// Whether the reader folded `list` of the Work side to its head.
    pub fn work_folded(&self, list: WorkList) -> bool {
        self.side_folds.contains(list.key())
    }

    /// The Work side on show with `list` open on it.
    pub fn show_work(&mut self, list: WorkList) {
        self.side = Side::Work;
        self.side_open = true;
        self.side_folds.remove(list.key());
    }

    /// The Tools view or the Tools page is shown: only then does a server's change read the servers again.
    pub fn tools_shown(&self) -> bool {
        (self.side_open && self.side == Side::Tools)
            || self.active_tab().is_some_and(|t| matches!(t.kind, TabKind::Tools | TabKind::Connections))
    }

    /// Something is being read or written (a terminal that runs is not: it costs nothing while it waits).
    pub fn busy(&self) -> bool {
        self.opening
            || self.picking
            || self.term_starting
            || self.search.running
            || self.listing_busy
            || (self.tools.reading && self.tools.overview.is_none())
            || self.tabs.iter().any(|t| match &t.kind {
                TabKind::File { body, .. } => match body {
                    Body::Loading => true,
                    Body::Editing(e) => e.saving,
                    Body::Paged { reading, .. } => *reading,
                    Body::Failed(_) => false,
                },
                TabKind::Output { reading, .. } => *reading,
                // Read while its text is on its way.
                TabKind::Artifact { view, .. } => view.is_none(),
                TabKind::Diff { .. } | TabKind::Tools | TabKind::Connections | TabKind::Project { .. } => false,
            })
    }

    pub fn tab(&self, id: u64) -> Option<&EditorTab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn tab_mut(&mut self, id: u64) -> Option<&mut EditorTab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn active_tab(&self) -> Option<&EditorTab> {
        self.active.and_then(|id| self.tab(id))
    }

    /// The tabs of the editor on show, in order.
    pub fn shown_tabs(&self) -> impl Iterator<Item = &EditorTab> {
        self.tabs.iter().filter(|t| t.editor == self.shown)
    }

    /// The tab showing `path` as a file, in the editor on show.
    pub fn file_tab(&self, path: &str) -> Option<u64> {
        self.file_tab_in(&self.shown, path)
    }

    /// The tab showing `path` as a file, in editor `key`.
    pub fn file_tab_in(&self, key: &str, path: &str) -> Option<u64> {
        self.tabs.iter().find(|t| t.editor == key && matches!(&t.kind, TabKind::File { path: p, .. } if p == path)).map(|t| t.id)
    }

    /// Add a tab to the editor on show and show it, the editor with it.
    pub fn push(&mut self, kind: TabKind) -> u64 {
        let key = self.shown.clone();
        self.push_in(&key, kind)
    }

    /// Add a tab to editor `key`: shown at once when that editor is on show, else the tab it shows when it is. Past
    /// [`MAX_TABS`] in that editor, its oldest tab without unsaved edits (not the new one) closes.
    pub fn push_in(&mut self, key: &str, kind: TabKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(EditorTab { id, kind, editor: key.to_string() });
        if key == self.shown {
            self.active = Some(id);
            self.reveal();
        } else if let Some(slot) = self.editors.iter_mut().find(|e| e.key == key) {
            slot.active = Some(id);
        }
        while self.tabs.iter().filter(|t| t.editor == key).count() > MAX_TABS {
            match self.tabs.iter().position(|t| t.editor == key && t.id != id && !t.dirty()) {
                Some(at) => {
                    let gone = self.tabs.remove(at);
                    self.remember_closed(&gone);
                }
                None => break,
            }
        }
        id
    }

    /// Show tab `id` in editor `key` (the tab on show there, whether that editor is on show or not).
    pub fn activate_in(&mut self, key: &str, id: u64) {
        if key == self.shown {
            self.active = Some(id);
        } else if let Some(slot) = self.editors.iter_mut().find(|e| e.key == key) {
            slot.active = Some(id);
        }
    }

    /// Close a tab (the caller has asked about unsaved edits); the tab beside it in its editor is shown.
    pub fn close(&mut self, id: u64) {
        let Some(at) = self.tabs.iter().position(|t| t.id == id) else { return };
        let gone = self.tabs.remove(at);
        self.remember_closed(&gone);
        let same: Vec<u64> = self.tabs.iter().filter(|t| t.editor == gone.editor).map(|t| t.id).collect();
        let before = self.tabs[..at].iter().filter(|t| t.editor == gone.editor).count();
        let beside = same.get(before).or_else(|| same.get(before.saturating_sub(1))).copied();
        if self.active == Some(id) {
            self.active = beside;
        }
        if let Some(slot) = self.editors.iter_mut().find(|e| e.key == gone.editor && e.active == Some(id)) {
            slot.active = beside;
        }
    }

    /// The numbered editor `key` exists (an agent's begins when it first works on a file); `who` names it.
    pub fn ensure_editor(&mut self, key: &str, who: &str) {
        match self.editors.iter_mut().find(|e| e.key == key) {
            Some(slot) => slot.who = who.to_string(),
            None => self.editors.push(EditorSlot { key: key.to_string(), who: who.to_string(), active: None }),
        }
    }

    /// Put editor `key` on show, its own tab with it.
    pub fn show_editor(&mut self, key: &str) {
        if key == self.shown || !self.editors.iter().any(|e| e.key == key) {
            return;
        }
        let leaving = std::mem::replace(&mut self.shown, key.to_string());
        let kept = self.active;
        if let Some(slot) = self.editors.iter_mut().find(|e| e.key == leaving) {
            slot.active = kept;
        }
        self.active = self.editors.iter().find(|e| e.key == key).and_then(|e| e.active).filter(|id| self.tab(*id).is_some());
        if self.active.is_none() {
            let first = self.shown_tabs().next().map(|t| t.id);
            self.active = first;
        }
    }

    /// Close an agent's editor: its tabs without unsaved edits close, and once none is left the editor goes (the
    /// reader's own stays). The reader's editor comes on show if it was.
    pub fn close_editor(&mut self, key: &str) {
        if key == YOURS {
            return;
        }
        let clean: Vec<u64> = self.tabs.iter().filter(|t| t.editor == key && !t.dirty()).map(|t| t.id).collect();
        for id in clean {
            self.close(id);
        }
        if !self.tabs.iter().any(|t| t.editor == key) {
            if self.shown == key {
                self.show_editor(YOURS);
            }
            self.editors.retain(|e| e.key != key);
        }
    }

    /// A file tab that closed is kept for Ctrl+Shift+T (at most [`CLOSED_KEPT`], once each).
    fn remember_closed(&mut self, tab: &EditorTab) {
        if let TabKind::File { path, .. } = &tab.kind {
            self.closed.retain(|p| p != path);
            self.closed.push(path.clone());
            if self.closed.len() > CLOSED_KEPT {
                self.closed.remove(0);
            }
        }
    }

    /// The tab after (or before) the one on show, wrapping round: Ctrl+Tab and Ctrl+Shift+Tab, Ctrl+PageDown and
    /// Ctrl+PageUp.
    pub fn cycle(&mut self, forward: bool) {
        let ids: Vec<u64> = self.shown_tabs().map(|t| t.id).collect();
        let n = ids.len();
        if n == 0 {
            return;
        }
        let at = self.active.and_then(|id| ids.iter().position(|t| *t == id));
        let next = match at {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None => 0,
        };
        self.active = Some(ids[next]);
    }

    /// The confirmation on show, once its quiet time has passed.
    pub fn confirm_ready(&self) -> bool {
        self.confirm.as_ref().is_some_and(|(_, at)| at.elapsed() >= CONFIRM_QUIET)
    }

    pub fn ask(&mut self, confirm: Confirm) {
        self.confirm = Some((confirm, Instant::now()));
    }

    /// The listing of the folder on show, when it has been read.
    pub fn files(&self) -> Option<&Listing> {
        match (&self.listing, &self.folder) {
            (Some(Ok(l)), Some(f)) if l.folder == f.id() => Some(l),
            _ => None,
        }
    }

    /// Close the tabs of a chat that is no longer the open one: its reviews and its commands' outputs.
    pub fn close_chat_tabs(&mut self) {
        let chat: Vec<u64> = self
            .tabs
            .iter()
            .filter(|t| matches!(t.kind, TabKind::Diff { .. } | TabKind::Output { .. }))
            .map(|t| t.id)
            .collect();
        for id in chat {
            self.close(id);
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(EditorTab::dirty)
    }

    /// The terminal on show in the bottom panel, when the panel shows the terminals.
    pub fn shown_term(&self) -> Option<&term::Term> {
        if !self.panel_open || self.panel != PanelTab::Terminal {
            return None;
        }
        let id = self.term_active?;
        self.terms.iter().find(|t| t.id == id)
    }

    pub fn term_mut(&mut self, id: u64) -> Option<&mut term::Term> {
        self.terms.iter_mut().find(|t| t.id == id)
    }

    /// A terminal on show has the keyboard (and nothing over it asks for it).
    pub fn terminal_focused(&self) -> bool {
        self.term_focus && self.quick.is_none() && self.confirm.is_none() && self.shown_term().is_some()
    }

    /// A number for the next terminal.
    /// The id the next terminal will have.
    pub fn next_term_id(&self) -> u64 {
        self.next_term
    }

    pub fn take_term_id(&mut self) -> u64 {
        let id = self.next_term;
        self.next_term += 1;
        id
    }

    /// Close terminal `id` (ending its shell and what it started); the one beside it is shown.
    pub fn close_term(&mut self, id: u64) {
        if let Some(at) = self.terms.iter().position(|t| t.id == id) {
            self.terms.remove(at);
            if self.term_active == Some(id) {
                self.term_active = self.terms.get(at).or_else(|| self.terms.get(at.saturating_sub(1))).map(|t| t.id);
            }
        }
        if self.terms.is_empty() {
            self.term_focus = false;
        }
    }

    /// Bound a dragged size to its range (the agent panel and the bottom panel grow towards the pointer's left and
    /// top, the side bar towards its right).
    pub fn resize(&mut self, which: Splitter, delta: f32) {
        match which {
            Splitter::Side => self.side_width = (self.side_width + delta).clamp(SIDE_RANGE.0, SIDE_RANGE.1),
            Splitter::Right if self.tasks_shown() => self.agent_width = (self.agent_width - delta).clamp(AGENT_RANGE.0, AGENT_RANGE.1),
            Splitter::Right => self.editor_width = (self.editor_width - delta).clamp(EDITOR_RANGE.0, EDITOR_RANGE.1),
            Splitter::Panel => self.panel_height = (self.panel_height - delta).clamp(PANEL_RANGE.0, PANEL_RANGE.1),
        }
    }

    /// The board or the tasks grid is the page: the right column then holds the chat or the editor.
    pub fn tasks_shown(&self) -> bool {
        self.grid.shown || self.groups.board_shown
    }

    /// Show or hide the editor beside the chat. Hidden while agents work, following leaves it hidden until a turn
    /// starts; shown again by hand, it is the reader's to keep.
    pub fn toggle_editor(&mut self, agents_working: bool) {
        self.editor_open = !self.editor_open;
        self.editor_dismissed = !self.editor_open && agents_working;
        if !self.editor_open {
            self.term_focus = false;
        }
    }

    /// A tab came on show by the reader's or an agent's doing: the editor shows beside the chat.
    pub fn reveal(&mut self) {
        if !self.tasks_shown() {
            self.editor_open = true;
        }
    }

    /// Note what a batch of chat `by`'s events shows it working on (`who` names it, `here` says it works in the
    /// folder the editor shows) and return the files it touched, in order. A turn that starts lets following show
    /// the editor again; one that ends takes that chat's files off the list.
    pub fn note_work(
        &mut self,
        by: &str,
        who: &str,
        here: bool,
        events: &[lattice_protocol::conversation::ConversationEvent],
    ) -> Vec<(String, Option<String>)> {
        use lattice_protocol::conversation::ConversationEventKind as Ev;
        if events.iter().any(|e| matches!(e.kind, Ev::TurnStarted { .. })) {
            self.editor_dismissed = false;
        }
        let touched = worked_files(events);
        if !touched.is_empty() {
            self.ensure_editor(by, who);
        }
        for (path, _) in &touched {
            self.working_on.retain(|w| !(w.by == by && &w.path == path));
            self.working_on.insert(0, WorkedFile { path: path.clone(), by: by.to_string(), who: who.to_string(), here });
        }
        self.working_on.truncate(WORKING_KEPT);
        if events.iter().any(|e| matches!(e.kind, Ev::TurnEnded { .. })) {
            self.working_on.retain(|w| w.by != by);
            // an agent's editor with nothing open goes with its turn (one on show stays until another is shown)
            if !self.tabs.iter().any(|t| t.editor == by) && self.shown != by {
                self.editors.retain(|e| e.key != by);
            }
        }
        touched
    }
}

/// The verb a step of the agent's turn is shown with, by its tool.
pub fn verb(tool: &str) -> &'static str {
    match tool {
        "read_file" => "Read",
        "list_dir" => "Listed",
        "glob" => "Found files",
        "grep" => "Searched",
        "edit_file" => "Edited",
        "write_file" => "Wrote",
        "delete_file" => "Deleted",
        "run_command" => "Ran",
        "ask_question" => "Asked",
        "spawn_agent" => "Sent a helper",
        "git_status" => "Read the git status",
        "git_diff" => "Read the changes",
        "git_branch" => "Made a branch",
        "git_commit" => "Committed",
        "git_push_pr" => "Pushed",
        "remember" => "Kept a note",
        "forget" => "Forgot a note",
        "suggest_task" => "Suggested a task",
        "withdraw_task" => "Withdrew a task",
        "write_artifact" => "Saved",
        "read_artifact" => "Read the artifact",
        "update_todos" => "Updated the to-do list",
        "give_steps" => "Gave you a checklist",
        "read_steps" => "Read your checklist",
        "propose_plan" => "Proposed a plan",
        "browser_open" => "Opened",
        "browser_look" => "Looked at the page",
        "browser_click" => "Clicked",
        "browser_type" => "Typed",
        "browser_key" => "Pressed",
        "browser_scroll" => "Scrolled",
        "browser_back" => "Went back",
        "browser_read" => "Read the page",
        "web_search" => "Searched the web",
        "command_output" => "Read the command's output",
        "stop_command" => "Stopped the command",
        "desktop_look" => "Looked at the screen",
        "desktop_click" => "Clicked",
        "desktop_type" => "Typed",
        "desktop_key" => "Pressed",
        "desktop_scroll" => "Scrolled",
        "use_skill" => "Read the skill",
        "read_skill_file" => "Read a skill's file",
        _ => "Used",
    }
}

/// What a tool's step is about, as its card names it: the target, else the summary after the tool's name (nothing
/// for a browser step that names none, such as a look: its verb says it all).
pub fn subject<'a>(tool: &str, summary: &'a str, target: Option<&'a str>) -> &'a str {
    match target {
        Some(t) if !t.is_empty() => t,
        _ => match summary.strip_prefix(tool).map(str::trim_start) {
            Some("") if tool.starts_with("browser_") || tool.starts_with("desktop_") => "",
            Some(rest) if !rest.is_empty() => rest,
            _ => summary,
        },
    }
}

/// Whether a tool's step is about a file of the folder (so its subject opens in the editor).
pub fn names_a_file(tool: &str) -> bool {
    matches!(tool, "read_file" | "edit_file" | "write_file" | "delete_file")
}

/// "3 s", "2 min 5 s", "1 h 4 min": a span of seconds as a person reads it.
pub fn span(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min {} s", s / 60, s % 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// "now", "5m", "3h", "2d": how long ago, short, for the chat list.
pub fn ago(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    match s {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> TabKind {
        TabKind::File { path: path.into(), lang: highlight::Lang::of(path), body: Body::Loading, note: None }
    }

    #[test]
    fn a_folder_s_stack_is_read_from_its_manifests_with_the_catalog_s_suggestions() {
        let root = std::env::temp_dir().join(format!("centcom-stack-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"), r#"{"dependencies": {"express": "^4", "pg": "^8"}}"#).unwrap();
        let review = StackReview::of(&root);
        let used: Vec<&str> = review.detected.iter().map(|(_, name)| name.as_str()).collect();
        assert!(used.contains(&"PostgreSQL") && used.contains(&"TypeScript (Node.js)"), "{used:?}");
        let checks: Vec<&str> = review.suggestions.iter().map(|s| s.check.as_str()).collect();
        assert!(checks.contains(&"pg-pool") && checks.contains(&"express-helmet"), "{checks:?}");
        // Shown for the folder open now; dismissed, it stays dismissed.
        let mut state = crate::lattice::State::new();
        state.ide.folder_path = root.to_string_lossy().into_owned();
        let _ = state.update(crate::lattice::Msg::Ide(IdeMsg::StackRead(state.ide.folder_path.clone(), Box::new(review))));
        assert!(state.ide.stack.as_ref().is_some_and(|s| !s.dismissed));
        let _ = state.update(crate::lattice::Msg::Ide(IdeMsg::StackDismiss));
        assert!(state.ide.stack.as_ref().is_some_and(|s| s.dismissed));
        let _ = state.update(crate::lattice::Msg::Ide(IdeMsg::StackRead("D:/elsewhere".into(), Box::default())));
        assert!(state.ide.stack.as_ref().is_some_and(|s| s.dismissed), "another folder's review is not taken");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn following_opens_the_newest_change_a_batch_staged_and_nothing_without_one() {
        use lattice_protocol::conversation::{ConversationEvent, ConversationEventKind as Ev};
        let staged = |seq, change: &str| ConversationEvent {
            seq,
            at: seq as f64,
            kind: Ev::Staged { change: change.into(), path: "a.rs".into(), added: 1, removed: 0, authority: false },
        };
        let other = ConversationEvent { seq: 3, at: 3.0, kind: Ev::Delta { text: "done".into() } };
        assert_eq!(followed_change(&[staged(1, "c_one"), staged(2, "c_two"), other.clone()]).as_deref(), Some("c_two"));
        assert_eq!(followed_change(&[other]), None);
        assert_eq!(followed_change(&[]), None);
    }

    #[test]
    fn opening_past_the_limit_closes_the_oldest_clean_tab_and_closing_shows_a_neighbour() {
        let mut ide = Ide::default();
        let first = ide.push(file("a.rs"));
        for n in 1..MAX_TABS {
            ide.push(file(&format!("f{n}.rs")));
        }
        assert_eq!(ide.tabs.len(), MAX_TABS);
        let newest = ide.push(file("new.rs"));
        assert_eq!(ide.tabs.len(), MAX_TABS);
        assert!(ide.tab(first).is_none() && ide.active == Some(newest));
        let second = ide.tabs[0].id;
        let third = ide.tabs[1].id;
        ide.active = Some(second);
        ide.close(second);
        assert_eq!(ide.active, Some(third));
        assert_eq!(ide.file_tab("new.rs"), Some(newest));
    }

    #[test]
    fn a_chats_reviews_and_outputs_close_with_it_and_files_stay() {
        let mut ide = Ide::default();
        let a = ide.push(file("a.rs"));
        ide.push(TabKind::Diff { change: "ch_0000000000000001".into(), path: "a.rs".into() });
        ide.push(TabKind::Output { call: "c1".into(), label: "cargo test".into(), lines: None, reading: false });
        ide.close_chat_tabs();
        assert_eq!(ide.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![a]);
        assert_eq!(ide.active, Some(a));
    }

    #[test]
    fn sizes_stay_in_their_ranges_and_the_right_hand_panel_grows_leftwards() {
        let mut ide = Ide::default();
        ide.resize(Splitter::Side, 10_000.0);
        assert_eq!(ide.side_width, SIDE_RANGE.1);
        ide.resize(Splitter::Right, 50.0);
        assert_eq!(ide.editor_width, EDITOR_WIDTH - 50.0, "beside the chat, the editor's border");
        ide.grid.shown = true;
        ide.resize(Splitter::Right, 50.0);
        assert_eq!(ide.agent_width, AGENT_WIDTH - 50.0, "beside the tasks, the right column's");
        ide.resize(Splitter::Panel, -10_000.0);
        assert_eq!(ide.panel_height, PANEL_RANGE.1);
    }

    #[test]
    fn a_step_names_its_subject_and_spans_read_as_people_read_them() {
        assert_eq!(subject("read_file", "read_file src/a.rs", Some("src/a.rs")), "src/a.rs");
        assert_eq!(subject("glob", "glob **/*.rs", None), "**/*.rs");
        assert_eq!(subject("mcp_tool", "mcp_tool", None), "mcp_tool");
        assert_eq!((verb("run_command"), verb("something_else")), ("Ran", "Used"));
        assert_eq!((verb("browser_click"), verb("browser_look")), ("Clicked", "Looked at the page"));
        assert_eq!(subject("browser_look", "browser_look", None), "", "a look names nothing more");
        assert_eq!((verb("desktop_look"), subject("desktop_look", "desktop_look", None)), ("Looked at the screen", ""));
        assert_eq!(subject("browser_click", "browser_click Post (share)", Some("Post (share)")), "Post (share)");
        assert_eq!((verb("use_skill"), subject("use_skill", "use_skill guide", Some("guide"))), ("Read the skill", "guide"));
        assert_eq!(verb("read_skill_file"), "Read a skill's file");
        assert!(names_a_file("edit_file") && !names_a_file("run_command") && !names_a_file("read_skill_file"));
        assert_eq!((span(4.4), span(125.0), span(3_840.0)), ("4 s".into(), "2 min 5 s".into(), "1 h 4 min".into()));
        assert_eq!((ago(10.0), ago(300.0), ago(7_200.0), ago(200_000.0)), ("now".into(), "5m".into(), "2h".into(), "2d".into()));
    }

    fn ev(seq: u64, kind: lattice_protocol::conversation::ConversationEventKind) -> lattice_protocol::conversation::ConversationEvent {
        lattice_protocol::conversation::ConversationEvent { seq, at: seq as f64, kind }
    }

    fn tool(seq: u64, tool: &str, target: &str) -> lattice_protocol::conversation::ConversationEvent {
        use lattice_protocol::conversation::ConversationEventKind as Ev;
        ev(seq, Ev::ToolCall { call_id: format!("k{seq}"), tool: tool.into(), summary: tool.into(), target: Some(target.into()) })
    }

    #[test]
    fn the_files_an_agent_works_on_are_the_ones_its_file_tools_and_changes_name() {
        use lattice_protocol::conversation::{ChangedPath, ConversationEventKind as Ev, PathChange};
        let events = [
            tool(1, "read_file", "src/a.rs"),
            tool(2, "list_dir", "src"),
            tool(3, "grep", "fn main"),
            tool(4, "delete_file", "old.rs"),
            tool(5, "edit_file", " src/b.rs "),
            ev(6, Ev::Staged { change: "ch_1".into(), path: "src/b.rs".into(), added: 2, removed: 1, authority: false }),
            ev(7, Ev::CommandEffect {
                call_id: "c".into(),
                before: 1,
                after: 2,
                files: vec![ChangedPath { path: "Cargo.lock".into(), change: PathChange::Modified }],
            }),
        ];
        assert_eq!(
            worked_files(&events),
            vec![
                ("src/a.rs".to_string(), None),
                ("src/b.rs".to_string(), None),
                ("src/b.rs".to_string(), Some("ch_1".to_string())),
                ("Cargo.lock".to_string(), None),
            ],
            "reads, edits, staged changes and a command's files; not listings, searches or deletions"
        );
    }

    #[test]
    fn the_editor_names_what_running_agents_work_on_and_a_turn_s_end_takes_it_off() {
        use lattice_protocol::Locality;
        use lattice_protocol::conversation::{ConversationEventKind as Ev, Mode, TurnKind, TurnStatus};
        let mut ide = Ide::default();
        let started = ev(1, Ev::TurnStarted {
            turn: "t".into(),
            kind: TurnKind::Agent,
            mode: Mode::Agent,
            label: "Local".into(),
            locality: Locality::Local,
        });
        ide.editor_dismissed = true;
        let touched = ide.note_work("chat-1", "This chat", true, &[started, tool(2, "read_file", "a.rs"), tool(3, "edit_file", "b.rs")]);
        assert_eq!(touched.len(), 2);
        assert!(!ide.editor_dismissed, "a turn that starts lets following show the editor again");
        assert_eq!(ide.working_on.iter().map(|w| w.path.as_str()).collect::<Vec<_>>(), ["b.rs", "a.rs"], "newest first");
        ide.note_work("task-2", "Fix the tests", false, &[tool(4, "read_file", "a.rs")]);
        assert_eq!(ide.working_on.len(), 3, "the same file worked on by two agents is named for each");
        assert!(!ide.working_on[0].here && ide.working_on[0].who == "Fix the tests");
        ide.note_work("chat-1", "This chat", true, &[tool(5, "read_file", "a.rs")]);
        assert_eq!(ide.working_on.iter().filter(|w| w.by == "chat-1").count(), 2, "a file read again moves up, not twice");
        ide.note_work("chat-1", "This chat", true, &[ev(6, Ev::TurnEnded { turn: "t".into(), status: TurnStatus::Completed })]);
        assert_eq!(ide.working_on.len(), 1, "a turn that ends takes its chat's files off");
        for n in 0..20 {
            ide.note_work("task-2", "Fix the tests", false, &[tool(10 + n, "read_file", &format!("f{n}.rs"))]);
        }
        assert_eq!(ide.working_on.len(), WORKING_KEPT);
    }

    #[test]
    fn the_editor_hidden_while_agents_work_stays_hidden_until_the_next_turn() {
        let mut ide = Ide::default();
        ide.toggle_editor(true);
        assert!(ide.editor_open && !ide.editor_dismissed, "shown by hand");
        ide.toggle_editor(true);
        assert!(!ide.editor_open && ide.editor_dismissed, "hidden while an agent works: following leaves it hidden");
        ide.toggle_editor(false);
        ide.toggle_editor(false);
        assert!(!ide.editor_open && !ide.editor_dismissed, "hidden with nothing at work: the next work may show it");
    }

    #[test]
    fn each_agents_files_open_in_its_own_numbered_editor_and_the_readers_view_stays() {
        let mut ide = Ide::default();
        let mine = ide.push(file("a.rs"));
        ide.ensure_editor("c1", "This chat");
        let theirs = ide.push_in("c1", file("b.rs"));
        assert_eq!(ide.active, Some(mine), "an agent's file opens in its editor, not in the one on show");
        assert_eq!(ide.shown_tabs().count(), 1);
        assert_eq!(ide.file_tab("b.rs"), None, "the editor on show does not hold it");
        assert_eq!(ide.file_tab_in("c1", "b.rs"), Some(theirs));
        assert_eq!(ide.editors.iter().map(|e| e.who.as_str()).collect::<Vec<_>>(), ["Yours", "This chat"], "numbered in order");
        ide.show_editor("c1");
        assert_eq!((ide.shown.as_str(), ide.active), ("c1", Some(theirs)));
        ide.show_editor(YOURS);
        assert_eq!(ide.active, Some(mine), "coming back, the reader's tab is on show again");
        ide.cycle(true);
        assert_eq!(ide.active, Some(mine), "Ctrl+Tab goes round the editor on show only");
        ide.show_editor("c1");
        ide.close_editor("c1");
        assert!(ide.editors.iter().all(|e| e.key != "c1") && ide.tab(theirs).is_none(), "closing an agent's editor closes its tabs");
        assert_eq!((ide.shown.as_str(), ide.active), (YOURS, Some(mine)), "and the reader's editor comes back on show");
        ide.close_editor(YOURS);
        assert_eq!(ide.editors.len(), 1, "the reader's own stays");
    }

    #[test]
    fn an_agents_editor_with_nothing_open_goes_with_its_turn() {
        use lattice_protocol::conversation::{ConversationEventKind as Ev, TurnStatus};
        let mut ide = Ide::default();
        ide.note_work("t1", "Fix the tests", false, &[tool(1, "read_file", "elsewhere.rs")]);
        assert_eq!(ide.editors.len(), 2, "an agent that works on a file gets its editor");
        ide.note_work("t1", "Fix the tests", false, &[ev(2, Ev::TurnEnded { turn: "t".into(), status: TurnStatus::Completed })]);
        assert_eq!(ide.editors.len(), 1, "with nothing open in it, it goes when the turn ends");
        ide.note_work("t2", "Refactor", true, &[tool(3, "read_file", "a.rs")]);
        ide.push_in("t2", file("a.rs"));
        ide.note_work("t2", "Refactor", true, &[ev(4, Ev::TurnEnded { turn: "t".into(), status: TurnStatus::Completed })]);
        assert_eq!(ide.editors.len(), 2, "with a file open, it stays for the reader to read");
    }

    #[test]
    fn a_tab_shows_the_editor_beside_the_chat_but_not_in_place_of_the_tasks() {
        let mut ide = Ide::default();
        assert!(!ide.editor_open, "the chat comes first");
        ide.push(file("a.rs"));
        assert!(ide.editor_open, "a tab that comes on show brings the editor");
        ide.editor_open = false;
        ide.grid.shown = true;
        ide.push(file("b.rs"));
        assert!(!ide.editor_open, "with the tasks on show, the right column is theirs");
    }
}
