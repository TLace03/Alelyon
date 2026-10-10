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

pub mod agent;
pub mod attach;
pub mod buffer;
pub mod editing;
// Replaces and places files through Win32 (ReplaceFileW, MoveFileExW), so it may use unsafe code, as the job object
// does.
#[allow(unsafe_code)]
pub mod files;
pub mod find;
pub mod highlight;
pub mod mention;
// Windows' folder picker (IFileOpenDialog, through COM), so it may use unsafe code, as the job object does.
#[allow(unsafe_code)]
pub mod picker;
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
/// The editor's font size and the height of one of its lines (fixed, so the line numbers beside it stay level).
pub const CODE_SIZE: f32 = 13.0;
pub const LINE_HEIGHT: f32 = 19.0;
/// The editor's padding, the same above the text and above its line numbers.
pub const CODE_PAD: f32 = 8.0;
/// The most tabs open at once: opening one more closes the oldest that has no unsaved edits.
pub const MAX_TABS: usize = 12;
/// The most lines a page of a read-only file or an output shows.
pub const PAGE_LINES: u32 = 400;
/// How long a confirmation ignores input after it appears (as the core's dialogs do).
pub const CONFIRM_QUIET: Duration = Duration::from_millis(500);

/// What the side bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Explorer,
    Search,
    Chats,
    Changes,
    /// The agent's tools and its MCP servers ([`tools`]).
    Tools,
}

impl Side {
    pub const ALL: [Side; 5] = [Side::Explorer, Side::Search, Side::Chats, Side::Changes, Side::Tools];

    pub fn title(self) -> &'static str {
        match self {
            Side::Explorer => "Explorer",
            Side::Search => "Search (Ctrl+Shift+F)",
            Side::Chats => "Chats",
            Side::Changes => "Changes",
            Side::Tools => "Tools and MCP servers",
        }
    }

    /// The activity bar's glyph.
    pub fn glyph(self) -> &'static str {
        match self {
            Side::Explorer => "\u{2750}",
            Side::Search => "\u{2315}",
            Side::Chats => "\u{25C8}",
            Side::Changes => "\u{00B1}",
            // SOFTWARE-FUNCTION SYMBOL.
            Side::Tools => "\u{2394}",
        }
    }
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
    Agent,
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
    pub active: Option<u64>,
    next_id: u64,
    pub quick: Option<Quick>,
    pub confirm: Option<(Confirm, Instant)>,
    pub side_width: f32,
    pub agent_width: f32,
    pub panel_height: f32,
    /// A border being dragged, and the pointer's last position along it.
    pub drag: Option<(Splitter, Option<f32>)>,
    pub chat_filter: ChatFilter,
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
    pub term_active: Option<u64>,
    pub term_focus: bool,
    next_term: u64,
    pub term_starting: bool,
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
    /// The suggested tasks whose prompts are shown on their chips.
    pub task_prompts: HashSet<String>,
    /// The images attached to the composer's message (at most 4), sent with it.
    pub attached: Vec<attach::Attached>,
    /// The Tools page's and view's state.
    pub tools: tools::Tools,
    /// The project the Chats view shows, which a new chat joins.
    pub project: Option<String>,
    /// The projects, and a project's page being edited.
    pub projects: projects::ProjectsState,
    /// The composer's commands list (`/name`).
    pub slash: slash::Slash,
    /// The composer's file mentions list (`@path`).
    pub mention: mention::Mentioning,
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
            side: Side::Explorer,
            side_open: true,
            panel_open: false,
            panel: PanelTab::Commands,
            tabs: Vec::new(),
            active: None,
            next_id: 1,
            quick: None,
            confirm: None,
            side_width: SIDE_WIDTH,
            agent_width: AGENT_WIDTH,
            panel_height: PANEL_HEIGHT,
            drag: None,
            chat_filter: ChatFilter::All,
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
            term_active: None,
            term_focus: false,
            next_term: 1,
            term_starting: false,
            term_problem: None,
            term_cells: (120, 8),
            find: None,
            goto: None,
            inline: None,
            search: search::SearchView::default(),
            reveal: None,
            inline_sending: false,
            attached: Vec::new(),
            task_prompts: HashSet::new(),
            tools: tools::Tools::default(),
            project: None,
            projects: projects::ProjectsState::default(),
            slash: slash::Slash::default(),
            mention: mention::Mentioning::default(),
        }
    }
}

impl Ide {
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

    /// The tab showing `path` as a file.
    pub fn file_tab(&self, path: &str) -> Option<u64> {
        self.tabs.iter().find(|t| matches!(&t.kind, TabKind::File { path: p, .. } if p == path)).map(|t| t.id)
    }

    /// Add a tab and show it; past [`MAX_TABS`], the oldest tab without unsaved edits (not the new one) closes.
    pub fn push(&mut self, kind: TabKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(EditorTab { id, kind });
        self.active = Some(id);
        while self.tabs.len() > MAX_TABS {
            match self.tabs.iter().position(|t| t.id != id && !t.dirty()) {
                Some(at) => {
                    self.tabs.remove(at);
                }
                None => break,
            }
        }
        id
    }

    /// Close a tab (the caller has asked about unsaved edits); the tab beside it is shown.
    pub fn close(&mut self, id: u64) {
        if let Some(at) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(at);
            if self.active == Some(id) {
                self.active = self.tabs.get(at).or_else(|| self.tabs.get(at.saturating_sub(1))).map(|t| t.id);
            }
        }
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
            Splitter::Agent => self.agent_width = (self.agent_width - delta).clamp(AGENT_RANGE.0, AGENT_RANGE.1),
            Splitter::Panel => self.panel_height = (self.panel_height - delta).clamp(PANEL_RANGE.0, PANEL_RANGE.1),
        }
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
        ide.resize(Splitter::Agent, 50.0);
        assert_eq!(ide.agent_width, AGENT_WIDTH - 50.0);
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
}
