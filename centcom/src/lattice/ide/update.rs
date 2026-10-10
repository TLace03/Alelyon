//! What the IDE's messages do. They live on the page's [`State`] (as `impl` blocks here) because opening a folder,
//! reading a page through the core and reviewing a change need the page's services and its open conversation.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::{scrollable, text_editor};

use lattice_core::text::find::Query;
use lattice_core::tools::read::{FileListing, SearchArgs, SearchReport};
use lattice_protocol::conversation::{
    AgentChatService, AllowEntry, ChangeSet, CheckpointView, Lines, RegenerateRequest, ViewRef,
};

use super::buffer::{self, Caret};
use super::files::{self, SaveError};
use super::highlight::Lang;
use super::{Body, ChatFilter, Confirm, Editing, Listing, PanelTab, Quick, Side, Splitter, TabKind, editing, find, search};
use super::{term, tree};
use super::{CODE_PAD, LINE_HEIGHT, PAGE_LINES};
use crate::lattice::{Msg, STOPPED, State, off_thread, on};

/// The IDE's messages.
#[derive(Clone, Debug)]
pub enum IdeMsg {
    // The frame.
    Side(Side),
    /// The Tools page's and view's messages.
    Tools(super::tools::ToolsMsg),
    /// The projects' messages.
    Projects(super::projects::ProjectsMsg),
    Notes(super::notes::NotesMsg),
    Grid(super::grid::GridMsg),
    /// The composer's commands list.
    Slash(super::slash::SlashMsg),
    TogglePanel,
    Panel(PanelTab),
    DragStart(Splitter),
    Pointer(iced::Point),
    PointerUp,
    // The folder.
    FolderPath(String),
    /// Open the path typed into the box (attached for the chat after the core's own confirmation).
    OpenFolder,
    OpenRecent(String),
    /// Choose a folder in Windows' picker.
    Browse,
    Picked(Result<Option<PathBuf>, String>),
    FolderOpened(String, Result<Arc<files::Folder>, String>),
    /// A folder or a file dropped on the page.
    Dropped(PathBuf),
    Refresh,
    Listed(String, Box<Result<Listing, String>>),
    Toggle(String),
    Filter(String),
    /// The Explorer's list scrolled: it builds only the rows on show (`ui::virtual_rows`).
    ExplorerScrolled(crate::ui::Scrolled),
    CollapseAll,
    // The editor.
    Open(String),
    Opened(u64, Box<Result<files::Opened, String>>),
    Activate(u64),
    Close(u64),
    Edit(u64, text_editor::Action),
    /// The editor's Tab completions.
    Complete(super::complete::CompleteMsg),
    Scrolled(u64, scrollable::Viewport),
    Undo,
    Redo,
    /// Tab (`true`) or Shift+Tab: indent or unindent by the file's own unit.
    Indent(bool),
    /// Enter: a line end and the indentation the new line takes.
    Newline,
    /// Ctrl+/: the lines commented out, or back in.
    ToggleComment,
    /// Alt+Up (`true`) or Alt+Down: the lines moved.
    MoveLines(bool),
    /// Shift+Alt+Up (`true`) or Shift+Alt+Down: the lines copied.
    CopyLines(bool),
    /// Ctrl+Shift+K: the lines deleted.
    DeleteLines,
    /// Home (`true`: Shift+Home, selecting): the first character that is not a space, then the line's start.
    Home(bool),
    Save,
    Saved(u64, Result<files::Saved, SaveError>),
    Reload(u64),
    Page(u64, u32),
    PageRead(u64, Result<Lines, String>),
    Output(String, String),
    OutputPage(u64, u32),
    OutputRead(u64, Result<Lines, String>),
    // A new file.
    NewFile,
    NewFileName(String),
    CreateFile,
    Created(Result<files::Saved, SaveError>),
    // Quick open.
    QuickOpen,
    QuickQuery(String),
    QuickPick(Option<String>),
    QuickClose,
    // The IDE's own confirmations.
    Confirm(bool),
    ConfirmTick,
    // The keyboard.
    Key(Key),
    // The agent panel.
    ChatFilter(ChatFilter),
    /// Follow the agent's changes in the editor (on), or not.
    Follow(bool),
    ChatSearch(String),
    /// Show every chat of this folder (`true`), or its newest only.
    ChatMore(String, bool),
    ProjectsOpen(bool),
    /// A new chat in this folder: the folder opens, then the chat starts empty.
    NewIn(String),
    Expand(String),
    Suggest(String),
    Mention,
    EditTurn(String),
    CancelEditTurn,
    Regenerate,
    Continue,
    Note(String, String),
    RejectWithNote(String),
    /// A suggested task: start it (a new chat of its own, beginning with its prompt), dismiss it, or show or hide its
    /// prompt on its chip.
    TaskStart(String),
    /// The composer's `@` list of the folder's files.
    MentionList(super::mention::MentionMsg),
    /// Attach images to the composer's message: Windows' file picker.
    AttachImages,
    /// The images read from files picked or dropped (each read, or why not).
    ImagesRead(Result<Vec<Result<super::attach::Attached, String>>, String>),
    /// Take an attached image off the composer's message.
    Unattach(usize),
    /// Approve the plan: the chat goes on in Agent mode.
    PlanApprove(String),
    /// Set the plan aside for more planning.
    PlanKeep(String),
    TaskDismiss(String),
    TaskPrompt(String),
    /// An artifact: open a version of it in a tab (the tab already showing it, when there is one), read it, step its
    /// version, show its Markdown as source or formatted, copy its text, or preview a page or a picture.
    ArtifactOpen(String, u32),
    ArtifactRead(u64, Box<Result<lattice_protocol::conversation::ArtifactView, String>>),
    ArtifactVersion(u64, u32),
    ArtifactSource(u64),
    ArtifactCopy(u64),
    ArtifactPreview(String, u32),
    CancelQueued(String),
    // Checkpoints and standing approvals.
    ReadHistory,
    CheckpointsRead(Result<Vec<CheckpointView>, String>),
    Restore(u32),
    Restored(Result<ChangeSet, String>),
    PermissionsRead(Result<Vec<AllowEntry>, String>),
    Revoke(String),
    // The bottom panel.
    Command(String),
    CommandRead(String, Result<Lines, String>),
    // The terminals.
    /// Start a terminal in the folder the IDE shows.
    TermNew,
    TermStarted(u64, Result<term::Started, String>),
    /// A terminal's output, or its end.
    TermEvent(u64, term::Event),
    /// What a terminal's grid read: the focus, keys, a paste, its size, the wheel.
    TermInput(u64, term::grid::Input),
    TermShow(u64),
    TermClose(u64),
    /// Start a terminal again where an ended one started (the ended one closes).
    TermRestart(u64),
    // Find, replace and go to line in the editor.
    /// Open the find bar on the editor on show (`true`: with the replace row).
    FindOpen(bool),
    FindQuery(String),
    FindReplaceText(String),
    FindToggle(find::Toggle),
    /// The next match (`true`) or the previous.
    FindStep(bool),
    ReplaceOne,
    ReplaceAll,
    FindClose,
    GotoOpen,
    GotoText(String),
    GotoGo,
    GotoClose,
    /// Esc in the editor: its find bar, Go to line or inline edit closes.
    EditorEscape,
    // The inline edit (Ctrl+K).
    InlineOpen,
    InlinePrompt(String),
    InlineSend,
    InlineClose,
    // Search across the folder.
    SearchShow,
    SearchQuery(String),
    SearchInclude(String),
    SearchToggle(find::Toggle),
    SearchRun,
    /// A search's answer, with the query and glob it answers.
    Searched(Box<(Query, String, Result<SearchReport, String>)>),
    SearchFold(String),
    SearchFoldAll,
    /// The search results scrolled: they build only the lines on show.
    SearchScrolled(crate::ui::Scrolled),
    /// Open a result: its path, line (from 1) and byte range.
    SearchOpen(String, u32, usize, usize),
}

/// The IDE's keys, read by [`keys`] whatever has the focus (unless a text box took the key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    QuickOpen,
    Save,
    ToggleSide,
    TogglePanel,
    CloseTab,
    FocusChat,
    Find,
    Replace,
    GotoLine,
    Inline,
    Search,
    FindNext,
    FindPrevious,
    Escape,
    Up,
    Down,
}

/// The IDE's keyboard shortcuts: Ctrl+P quick open, Ctrl+S save, Ctrl+B the side bar, Ctrl+J or Ctrl+` the bottom
/// panel, Ctrl+W close the tab, Ctrl+L the chat, Ctrl+F find, Ctrl+H replace, Ctrl+G go to line, Ctrl+K the inline
/// edit, Ctrl+Shift+F search the folder, F3 and Shift+F3 the next and previous match, Escape and the arrows for the
/// quick-open list.
pub fn keys(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Msg> {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Event, Key as K};
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, physical_key, .. }) = event else { return None };
    let captured = status == iced::event::Status::Captured;
    let key = if modifiers.command() {
        match key.to_latin(physical_key) {
            Some('p') if !modifiers.shift() => Key::QuickOpen,
            Some('s') if !captured => Key::Save,
            Some('b') => Key::ToggleSide,
            Some('j') | Some('`') => Key::TogglePanel,
            Some('w') => Key::CloseTab,
            Some('l') => Key::FocusChat,
            Some('f') if modifiers.shift() => Key::Search,
            Some('f') if !captured => Key::Find,
            Some('h') if !captured => Key::Replace,
            Some('g') if !captured => Key::GotoLine,
            Some('k') if !captured => Key::Inline,
            _ => return None,
        }
    } else {
        if let K::Named(Named::F3) = key {
            return Some(Msg::Ide(IdeMsg::Key(if modifiers.shift() { Key::FindPrevious } else { Key::FindNext })));
        }
        // Esc closes what is open over the page even from inside its box: a text box takes the key to give up its
        // focus, and that must not keep the box itself open.
        if let K::Named(Named::Escape) = key {
            return Some(Msg::Ide(IdeMsg::Key(Key::Escape)));
        }
        if captured {
            return None;
        }
        match key {
            K::Named(Named::ArrowUp) => Key::Up,
            K::Named(Named::ArrowDown) => Key::Down,
            _ => return None,
        }
    };
    Some(Msg::Ide(IdeMsg::Key(key)))
}

/// The pointer while a border is dragged.
pub fn drags(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Msg> {
    use iced::mouse::Event;
    match event {
        iced::Event::Mouse(Event::CursorMoved { position }) => Some(Msg::Ide(IdeMsg::Pointer(position))),
        iced::Event::Mouse(Event::ButtonReleased(_)) | iced::Event::Mouse(Event::CursorLeft) => {
            Some(Msg::Ide(IdeMsg::PointerUp))
        }
        _ => None,
    }
}

/// The last page is what a running command's reader wants: where to read it from, when this page was not it.
pub fn tail_from(lines: &Lines) -> Option<u32> {
    let last = (lines.from + lines.lines.len() as u32).saturating_sub(1);
    let tail = lines.total.saturating_sub(PAGE_LINES - 1).max(1);
    (last < lines.total && lines.from < tail).then_some(tail)
}

fn longest_line(text: &str) -> usize {
    text.lines().map(|l| l.chars().count()).max().unwrap_or(0)
}

impl Editing {
    pub(in crate::lattice) fn new(opened_sha: String, authority: bool, decoded: buffer::Decoded, lang: Lang) -> Editing {
        Editing {
            unit: editing::unit(decoded.text.lines(), lang),
            content: text_editor::Content::with_text(&decoded.text),
            base_sha256: opened_sha,
            bom: decoded.bom,
            ending: decoded.ending,
            lines: decoded.lines,
            longest: longest_line(&decoded.text),
            saved: decoded.text,
            dirty: false,
            history: buffer::History::default(),
            authority,
            saving: false,
            writing: None,
            viewport: None,
            across: None,
            gutter: Editing::numbers(decoded.lines),
        }
    }

    /// After a change: dirty or not, its lines and its widest line, from one reading of the text.
    fn refresh(&mut self) {
        let text = self.content.text();
        self.dirty = text != self.saved;
        let lines = self.content.line_count().max(1);
        if lines != self.lines || self.gutter.1.len() != lines + 1 {
            self.gutter = Editing::numbers(lines);
        }
        self.lines = lines;
        self.longest = longest_line(&text);
    }

    fn caret(&self) -> Caret {
        let c = self.content.cursor().position;
        Caret { line: c.line, column: c.column }
    }

    /// Put `text` in the editor (an undo, a redo, a reload) with the cursor at `caret`.
    fn replace_text(&mut self, text: &str, caret: Caret) {
        self.content = text_editor::Content::with_text(text);
        // The caret held to the new text (a line it was on may be shorter now, or gone).
        let line = caret.line.min(self.content.line_count().saturating_sub(1));
        let column = self.content.line(line).map_or(0, |l| {
            let mut at = caret.column.min(l.text.len());
            while !l.text.is_char_boundary(at) {
                at -= 1;
            }
            at
        });
        self.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column },
            selection: None,
        });
        self.refresh();
    }
}

impl State {
    /// The IDE's messages (see [`IdeMsg`]).
    pub(in crate::lattice) fn ide_update(&mut self, msg: IdeMsg) -> Task<Msg> {
        match msg {
            IdeMsg::Side(side) => {
                if self.ide.side == side && self.ide.side_open {
                    self.ide.side_open = false;
                } else {
                    self.ide.side = side;
                    self.ide.side_open = true;
                    if side == Side::Changes {
                        return self.read_history();
                    }
                    if side == Side::Search {
                        self.ide.term_focus = false;
                        return operation::focus(search::input_id());
                    }
                    if side == Side::Tools {
                        return self.read_tools();
                    }
                }
            }
            IdeMsg::Tools(msg) => return self.tools(msg),
            IdeMsg::Projects(msg) => return self.projects(msg),
            IdeMsg::Notes(msg) => return self.notes(msg),
            IdeMsg::Grid(msg) => return self.grid(msg),
            IdeMsg::Slash(msg) => return self.slash(msg),
            IdeMsg::Complete(msg) => return self.complete(msg),
            IdeMsg::MentionList(msg) => return self.mention(msg),
            IdeMsg::TogglePanel => return self.toggle_panel(),
            IdeMsg::Panel(PanelTab::Terminal) => return self.show_terminals(),
            IdeMsg::Panel(tab) => {
                self.ide.panel = tab;
                self.ide.panel_open = true;
                self.ide.term_focus = false;
            }
            IdeMsg::DragStart(which) => self.ide.drag = Some((which, None)),
            IdeMsg::Pointer(at) => {
                if let Some((which, last)) = self.ide.drag {
                    let now = if which == Splitter::Panel { at.y } else { at.x };
                    if let Some(last) = last {
                        self.ide.resize(which, now - last);
                    }
                    self.ide.drag = Some((which, Some(now)));
                }
            }
            IdeMsg::PointerUp => self.ide.drag = None,

            IdeMsg::FolderPath(path) => self.ide.folder_path = path,
            IdeMsg::OpenFolder => {
                let path = self.ide.folder_path.trim().to_string();
                return self.ask_open_folder(path, false);
            }
            IdeMsg::OpenRecent(path) => {
                self.ide.folder_path = path.clone();
                return self.ask_open_folder(path, false);
            }
            IdeMsg::Browse => {
                if self.ide.picking {
                    return Task::none();
                }
                self.ide.picking = true;
                return Task::perform(off_thread(super::picker::pick_folder), |r| {
                    Msg::Ide(IdeMsg::Picked(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            IdeMsg::Picked(result) => {
                self.ide.picking = false;
                match result {
                    Ok(Some(path)) => {
                        let shown = path.display().to_string();
                        self.ide.folder_path = shown.clone();
                        return self.ask_open_folder(shown, true);
                    }
                    Ok(None) => {}
                    Err(why) => self.problem = Some(why),
                }
            }
            IdeMsg::FolderOpened(path, result) => {
                self.ide.opening = false;
                match result {
                    Ok(folder) => {
                        let changed = self.ide.folder.as_ref().is_none_or(|f| f.id() != folder.id());
                        if changed {
                            // A file's tab belongs to the folder it was opened from (a review or an output belongs to
                            // its chat, and stays).
                            let files: Vec<u64> =
                                self.ide.tabs.iter().filter(|t| matches!(t.kind, TabKind::File { .. })).map(|t| t.id).collect();
                            for id in files {
                                self.ide.close(id);
                            }
                            self.ide.expanded.clear();
                            self.ide.listing = None;
                            self.ide.checkpoints = None;
                            self.ide.permissions = None;
                        }
                        self.ide.folder_path = folder.shown_path();
                        self.ide.folder = Some(folder);
                        if let Err(why) = self.same_folder() {
                            self.ide.folder = None;
                            self.problem = Some(why);
                            return Task::none();
                        }
                        return self.read_listing();
                    }
                    Err(why) => self.problem = Some(format!("{path}: {why}")),
                }
            }
            IdeMsg::Refresh => return self.read_listing(),
            IdeMsg::Listed(id, result) => {
                self.ide.listing_busy = false;
                if self.ide.folder.as_ref().is_some_and(|f| f.id() == id) {
                    self.ide.listing = Some(*result);
                    self.rank_filter();
                    // The file named on the command line opens with the folder's first listing.
                    if let Some((_, Some(file))) = self.ide.startup.take() {
                        return self.open_file(file);
                    }
                }
            }
            IdeMsg::Dropped(path) => {
                // A PNG or JPEG dropped on Lattice is attached to the message being written.
                if super::attach::looks_like_image(&path) {
                    return Task::perform(off_thread(move || Ok(vec![super::attach::read(&path)])), |r| {
                        Msg::Ide(IdeMsg::ImagesRead(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                    });
                }
                if path.is_dir() {
                    let shown = path.display().to_string();
                    self.ide.folder_path = shown.clone();
                    return self.ask_open_folder(shown, true);
                }
                // A file of the folder on show opens in the editor (by its path inside the folder).
                let inside = self.ide.folder.as_ref().and_then(|f| {
                    let root = std::path::PathBuf::from(f.shown_path());
                    path.strip_prefix(&root).ok().map(|rel| rel.to_string_lossy().replace('\\', "/"))
                });
                match inside {
                    Some(rel) if !rel.is_empty() => return self.open_file(rel),
                    _ => {
                        self.problem = Some(
                            "Drop a folder here to open it in the IDE, or a file of the folder it shows.".to_string(),
                        )
                    }
                }
            }
            IdeMsg::Toggle(path) => {
                if !self.ide.expanded.remove(&path) {
                    self.ide.expanded.insert(path);
                }
            }
            IdeMsg::Filter(text) => {
                self.ide.filter = text;
                self.rank_filter();
            }
            IdeMsg::CollapseAll => self.ide.expanded.clear(),
            IdeMsg::ExplorerScrolled(at) => self.ide.explorer_at = at,
            IdeMsg::SearchScrolled(at) => self.ide.search_at = at,

            IdeMsg::Open(path) => return self.open_file(path),
            IdeMsg::Opened(id, result) => return self.opened(id, *result),
            IdeMsg::Activate(id) => {
                if self.ide.tab(id).is_some() {
                    self.ide.active = Some(id);
                    // An open find bar follows the editor on show.
                    if let Some(f) = &mut self.ide.find
                        && f.tab != id
                        && self.ide.tabs.iter().any(|t| t.id == id && matches!(t.kind, TabKind::File { body: Body::Editing(_), .. }))
                    {
                        f.tab = id;
                        f.origin = (0, 0);
                        let _ = self.refind(id, None, false);
                    }
                    if let Some(TabKind::Diff { change, .. }) = self.ide.tab(id).map(|t| &t.kind)
                        && self.diff.as_ref().is_none_or(|d| &d.change != change)
                    {
                        let change = change.clone();
                        return self.update(Msg::ShowDiff(change));
                    }
                }
            }
            IdeMsg::Close(id) => {
                if self.ide.tab(id).is_some_and(|t| t.dirty()) {
                    self.ide.ask(Confirm::Discard { tab: id });
                } else {
                    self.ide.close(id);
                }
            }
            IdeMsg::Edit(id, action) => {
                // The editor is being typed in or clicked: the terminal no longer has the keyboard.
                if !matches!(action, text_editor::Action::Scroll { .. }) {
                    self.ide.term_focus = false;
                }
                return self.edit(id, action);
            }
            IdeMsg::Scrolled(id, viewport) => {
                if let Some(e) = self.editing_mut(id) {
                    e.viewport = Some((viewport.absolute_offset().y, viewport.bounds().height));
                    e.across = Some((viewport.absolute_offset().x, viewport.bounds().width));
                }
            }
            IdeMsg::Undo | IdeMsg::Redo => {
                let undo = matches!(msg, IdeMsg::Undo);
                if let Some(id) = self.ide.active
                    && let Some(e) = self.editing_mut(id)
                {
                    let now = e.content.text();
                    let caret = e.caret();
                    let step = if undo { e.history.undo(&now, caret) } else { e.history.redo(&now, caret) };
                    if let Some((text, at)) = step {
                        e.replace_text(&text, at);
                    }
                    let _ = self.refind(id, None, false);
                    return self.follow(id);
                }
            }
            IdeMsg::Indent(more) => return self.indent_lines(more),
            IdeMsg::Newline => return self.newline(),
            IdeMsg::ToggleComment => return self.toggle_comment(),
            IdeMsg::MoveLines(up) => return self.move_lines(up),
            IdeMsg::CopyLines(up) => return self.copy_lines(up),
            IdeMsg::DeleteLines => return self.delete_lines(),
            IdeMsg::Home(select) => return self.home(select),
            IdeMsg::Save => {
                if let Some(id) = self.ide.active {
                    return self.save(id, false);
                }
            }
            IdeMsg::Saved(id, result) => {
                let mut notes_of = None;
                let Some(tab) = self.ide.tab_mut(id) else { return Task::none() };
                let TabKind::File { body: Body::Editing(e), note, path, .. } = &mut tab.kind else { return Task::none() };
                e.saving = false;
                let written = e.writing.take();
                match result {
                    Ok(saved) => {
                        e.base_sha256 = saved.sha256;
                        // What is saved is what the editor held when Save was pressed; an edit made while the save ran
                        // is still unsaved.
                        if let Some(text) = written {
                            e.saved = text;
                        }
                        e.refresh();
                        e.history.break_step();
                        notes_of = Some(path.clone());
                        *note = Some(if saved.changed_after {
                            ("Saved, then changed by another program: what is on disk is not what was written.".into(), true)
                        } else {
                            (format!("Saved {}.", crate::ui::bytes(saved.size)), false)
                        });
                    }
                    Err(SaveError::NeedsConfirm) => self.ide.ask(Confirm::SaveAuthority { tab: id }),
                    Err(error) => {
                        let words = error.sentence();
                        *note = Some((words.clone(), true));
                        self.problem = Some(format!("{path}: {words}"));
                    }
                }
                // Its notes, placed again on the text saved.
                if let Some(path) = notes_of {
                    return self.read_notes(path);
                }
            }
            IdeMsg::Reload(id) => {
                if self.ide.tab(id).is_some_and(|t| t.dirty()) {
                    self.ide.ask(Confirm::Reload { tab: id });
                } else {
                    return self.reload(id);
                }
            }
            IdeMsg::Page(id, from) => return self.read_page(id, from),
            IdeMsg::PageRead(id, result) => {
                if let Some(tab) = self.ide.tab_mut(id)
                    && let TabKind::File { lang, body: Body::Paged { lines, reading, colours, .. }, .. } = &mut tab.kind
                {
                    *reading = false;
                    match result {
                        Ok(read) => {
                            // Coloured once, when the page arrives.
                            *colours = super::highlight::lines(*lang, read.lines.iter().map(String::as_str), super::highlight::Carry::Code);
                            *lines = Some(read);
                        }
                        Err(why) => self.problem = Some(why),
                    }
                }
            }
            IdeMsg::Output(call, label) => {
                let existing = self.ide.tabs.iter().find(|t| matches!(&t.kind, TabKind::Output { call: c, .. } if *c == call));
                if let Some(t) = existing {
                    self.ide.active = Some(t.id);
                    return Task::none();
                }
                let id = self.ide.push(TabKind::Output { call, label, lines: None, reading: true });
                return self.read_output(id, 1);
            }
            IdeMsg::OutputPage(id, from) => return self.read_output(id, from),
            IdeMsg::OutputRead(id, result) => {
                if let Some(tab) = self.ide.tab_mut(id)
                    && let TabKind::Output { lines, reading, .. } = &mut tab.kind
                {
                    *reading = false;
                    match result {
                        Ok(read) => *lines = Some(read),
                        Err(why) => self.problem = Some(why),
                    }
                }
            }

            IdeMsg::NewFile => {
                self.ide.new_file = Some(String::new());
                self.ide.side = Side::Explorer;
                self.ide.side_open = true;
            }
            IdeMsg::NewFileName(name) => self.ide.new_file = Some(name),
            IdeMsg::CreateFile => {
                let Some(name) = self.ide.new_file.clone() else { return Task::none() };
                return self.create(name.trim().to_string(), false);
            }
            IdeMsg::Created(result) => match result {
                Ok(saved) => {
                    self.ide.new_file = None;
                    let path = saved.path.clone();
                    return Task::batch([self.read_listing(), self.open_file(path)]);
                }
                Err(SaveError::NeedsConfirm) => {
                    if let Some(name) = self.ide.new_file.clone() {
                        self.ide.ask(Confirm::CreateAuthority { path: name.trim().to_string() });
                    }
                }
                Err(error) => self.problem = Some(error.sentence()),
            },

            IdeMsg::QuickOpen => {
                if self.ide.quick.is_some() {
                    self.ide.quick = None;
                    return Task::none();
                }
                self.ide.quick = Some(Quick::default());
                self.quick_rank();
                return operation::focus(super::quick_id());
            }
            IdeMsg::QuickQuery(query) => {
                if let Some(q) = &mut self.ide.quick {
                    q.query = query;
                    q.selected = 0;
                }
                self.quick_rank();
            }
            IdeMsg::QuickPick(path) => {
                let picked = path.or_else(|| {
                    self.ide.quick.as_ref().and_then(|q| q.results.get(q.selected).map(|r| r.path.clone()))
                });
                self.ide.quick = None;
                if let Some(path) = picked {
                    return self.open_file(path);
                }
            }
            IdeMsg::QuickClose => self.ide.quick = None,

            IdeMsg::Confirm(yes) => {
                if !self.ide.confirm_ready() {
                    // A click in the first half second is ignored, not taken as an answer.
                    return Task::none();
                }
                let Some((confirm, _)) = self.ide.confirm.take() else { return Task::none() };
                if !yes {
                    return Task::none();
                }
                return match confirm {
                    Confirm::SaveAuthority { tab } => self.save(tab, true),
                    Confirm::CreateAuthority { path } => self.create(path, true),
                    Confirm::Discard { tab } => {
                        self.ide.close(tab);
                        Task::none()
                    }
                    Confirm::Reload { tab } => self.reload(tab),
                    Confirm::SwitchFolder { path, native } => self.open_folder(path, native),
                    Confirm::Paste { term, text, .. } => {
                        self.paste(term, &text);
                        Task::none()
                    }
                    Confirm::CloseTerminal { term } => {
                        self.ide.close_term(term);
                        Task::none()
                    }
                };
            }
            IdeMsg::ConfirmTick => {}

            IdeMsg::Key(key) => return self.key(key),

            IdeMsg::ChatFilter(filter) => self.ide.chat_filter = filter,
            IdeMsg::Follow(on) => self.ide.follow = on,
            IdeMsg::ChatSearch(text) => self.ide.chat_search = text,
            IdeMsg::ChatMore(folder, on) => {
                if on {
                    self.ide.chat_more.insert(folder);
                } else {
                    self.ide.chat_more.remove(&folder);
                }
            }
            IdeMsg::ProjectsOpen(on) => self.ide.projects_open = on,
            IdeMsg::NewIn(path) => {
                let start = self.update(Msg::New);
                return Task::batch([start, self.ide_update(IdeMsg::OpenRecent(path))]);
            }
            IdeMsg::Expand(call) => {
                if !self.ide.expanded_calls.remove(&call) {
                    self.ide.expanded_calls.insert(call);
                }
            }
            IdeMsg::Suggest(text) => {
                self.composer = text_editor::Content::with_text(&text);
                self.composer.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                return operation::focus(super::composer_id());
            }
            IdeMsg::Mention => {
                if let Some(path) = self.ide.active_tab().and_then(|t| t.path()).map(str::to_string) {
                    let text = self.composer.text();
                    let glue = if text.is_empty() || text.ends_with(char::is_whitespace) { "" } else { " " };
                    self.composer.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                    self.composer.perform(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(format!(
                        "{glue}@{path} "
                    )))));
                    return operation::focus(super::composer_id());
                }
            }
            IdeMsg::EditTurn(turn) => {
                if let Some(text) = self.open.as_ref().and_then(|c| c.turns.iter().find(|t| t.id == turn)).map(|t| t.text.clone()) {
                    self.ide.editing_turn = Some(turn);
                    self.composer = text_editor::Content::with_text(&text);
                    self.composer.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                    return operation::focus(super::composer_id());
                }
            }
            IdeMsg::CancelEditTurn => {
                self.ide.editing_turn = None;
                self.composer = text_editor::Content::new();
            }
            IdeMsg::Regenerate => {
                let (Some(c), Some((services, agent))) = (&self.open, self.agent()) else { return Task::none() };
                if self.sending || c.running {
                    return Task::none();
                }
                self.sending = true;
                let request = RegenerateRequest {
                    conversation: c.id().to_string(),
                    choice: self.choice.clone(),
                    shown: self.shown_for(&self.choice),
                    mode: self.mode,
                };
                return Task::perform(on(&services, async move { agent.regenerate(request).await.map_err(|r| r.message) }), |r| {
                    Msg::Sent(r.unwrap_or_else(|| Err(STOPPED.to_string())))
                });
            }
            IdeMsg::Continue => {
                let (Some(c), Some((services, agent))) = (&self.open, self.agent()) else { return Task::none() };
                if self.sending || c.running {
                    return Task::none();
                }
                self.sending = true;
                let id = c.id().to_string();
                let shown = self.shown_for(&self.choice);
                return Task::perform(on(&services, async move { agent.continue_turn(&id, shown).await.map_err(|r| r.message) }), |r| {
                    Msg::Sent(r.unwrap_or_else(|| Err(STOPPED.to_string())))
                });
            }
            IdeMsg::Note(call, text) => {
                self.ide.notes.insert(call, text);
            }
            IdeMsg::RejectWithNote(call) => {
                let note = self.ide.notes.remove(&call).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
                if let (Some(c), Some((services, agent))) = (&self.open, self.agent()) {
                    let id = c.id().to_string();
                    let decision = lattice_protocol::conversation::Decision::Reject { note };
                    return Task::perform(
                        on(&services, async move { agent.decide(&id, &call, decision).await.map_err(|r| r.message) }),
                        crate::lattice::done,
                    );
                }
            }
            IdeMsg::TaskPrompt(task) => {
                if !self.ide.task_prompts.remove(&task) {
                    self.ide.task_prompts.insert(task);
                }
            }
            IdeMsg::TaskStart(task) => {
                let (Some(c), Some(services)) = (&self.open, self.services.clone()) else { return Task::none() };
                if self.sending {
                    return Task::none();
                }
                self.sending = true;
                // The new chat opens when the send is accepted, as any new chat does; the composer's draft stays.
                self.ide.inline_sending = true;
                let id = c.id().to_string();
                let (chat, choice, shown, mode) =
                    (services.chat.clone(), self.choice.clone(), self.shown_for(&self.choice), self.mode);
                let work =
                    async move { chat.start_task(&id, &task, choice, shown, mode).await.map_err(|r| r.message) };
                return Task::perform(on(&services, work), |r| Msg::Sent(r.unwrap_or_else(|| Err(STOPPED.to_string()))));
            }
            IdeMsg::ArtifactOpen(name, version) => {
                let Some(c) = &self.open else { return Task::none() };
                let conversation = c.id().to_string();
                let found = self.ide.tabs.iter().find(|t| {
                    matches!(&t.kind, TabKind::Artifact { conversation: c, name: n, .. } if *c == conversation && *n == name)
                });
                let id = match found {
                    Some(tab) => {
                        let id = tab.id;
                        self.ide.active = Some(id);
                        id
                    }
                    None => self.ide.push(TabKind::Artifact {
                        conversation,
                        name,
                        version,
                        view: None,
                        rendered: None,
                        source: false,
                    }),
                };
                return self.read_artifact(id, version);
            }
            IdeMsg::ArtifactVersion(id, version) => return self.read_artifact(id, version),
            IdeMsg::ArtifactRead(id, result) => {
                if let Some(tab) = self.ide.tab_mut(id)
                    && let TabKind::Artifact { version, view, rendered, .. } = &mut tab.kind
                {
                    let result = *result;
                    if let Ok(v) = &result {
                        *version = v.version;
                        *rendered = (v.kind == lattice_protocol::conversation::ArtifactKind::Markdown)
                            .then(|| iced::widget::markdown::Content::parse(&v.text));
                    }
                    *view = Some(result);
                }
            }
            IdeMsg::ArtifactSource(id) => {
                if let Some(tab) = self.ide.tab_mut(id)
                    && let TabKind::Artifact { source, .. } = &mut tab.kind
                {
                    *source = !*source;
                }
            }
            IdeMsg::ArtifactCopy(id) => {
                if let Some(tab) = self.ide.tabs.iter().find(|t| t.id == id)
                    && let TabKind::Artifact { view: Some(Ok(v)), .. } = &tab.kind
                {
                    return iced::clipboard::write(v.text.clone());
                }
            }
            IdeMsg::ArtifactPreview(name, version) => {
                let (Some(c), Some(services)) = (&self.open, self.services.clone()) else { return Task::none() };
                let (id, chat) = (c.id().to_string(), services.chat.clone());
                let work = async move { chat.preview_artifact(&id, &name, Some(version)).await.map_err(|r| r.message) };
                return Task::perform(on(&services, work), crate::lattice::done);
            }
            IdeMsg::AttachImages => {
                if self.ide.picking || self.ide.attached.len() >= super::attach::MAX {
                    return Task::none();
                }
                self.ide.picking = true;
                let work = || {
                    super::picker::pick_images()
                        .map(|paths| paths.iter().map(|path| super::attach::read(path)).collect::<Vec<_>>())
                };
                return Task::perform(off_thread(work), |r| {
                    Msg::Ide(IdeMsg::ImagesRead(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            IdeMsg::ImagesRead(result) => {
                self.ide.picking = false;
                let mut why = Vec::new();
                match result {
                    Ok(read) => {
                        for image in read {
                            match image {
                                Ok(_) if self.ide.attached.len() >= super::attach::MAX => {
                                    why.push("A message takes at most 4 images.".to_string());
                                    break;
                                }
                                Ok(image) => self.ide.attached.push(image),
                                Err(e) => why.push(e),
                            }
                        }
                    }
                    Err(e) => why.push(e),
                }
                if !why.is_empty() {
                    self.problem = Some(why.join(" "));
                }
            }
            IdeMsg::Unattach(at) => {
                if at < self.ide.attached.len() {
                    self.ide.attached.remove(at);
                }
            }
            IdeMsg::PlanApprove(plan) => {
                let (Some(c), Some(services)) = (&self.open, self.services.clone()) else { return Task::none() };
                if self.sending {
                    return Task::none();
                }
                self.sending = true;
                // Approving goes on in Agent mode, and the composer shows it.
                self.mode = lattice_protocol::conversation::Mode::Agent;
                let id = c.id().to_string();
                let (chat, choice, shown) = (services.chat.clone(), self.choice.clone(), self.shown_for(&self.choice));
                let work = async move { chat.approve_plan(&id, &plan, choice, shown).await.map_err(|r| r.message) };
                return Task::perform(on(&services, work), |r| Msg::Sent(r.unwrap_or_else(|| Err(STOPPED.to_string()))));
            }
            IdeMsg::PlanKeep(plan) => {
                let (Some(c), Some(services)) = (&self.open, self.services.clone()) else { return Task::none() };
                let (id, chat) = (c.id().to_string(), services.chat.clone());
                let work = async move { chat.keep_planning(&id, &plan).await.map_err(|r| r.message) };
                return Task::perform(on(&services, work), crate::lattice::done);
            }
            IdeMsg::TaskDismiss(task) => {
                let (Some(c), Some(services)) = (&self.open, self.services.clone()) else { return Task::none() };
                let (id, chat) = (c.id().to_string(), services.chat.clone());
                let work = async move { chat.dismiss_task(&id, &task).await.map_err(|r| r.message) };
                return Task::perform(on(&services, work), crate::lattice::done);
            }
            IdeMsg::CancelQueued(queued) => {
                if let (Some(c), Some(services)) = (&self.open, &self.services)
                    && let Err(refusal) = services.chat.cancel_queued(c.id(), &queued)
                {
                    self.problem = Some(refusal.message);
                }
                if let Some(c) = &mut self.open {
                    c.queued.retain(|q| q.queued_id != queued);
                }
            }

            IdeMsg::ReadHistory => return self.read_history(),
            IdeMsg::CheckpointsRead(result) => self.ide.checkpoints = Some(result),
            IdeMsg::PermissionsRead(result) => self.ide.permissions = Some(result),
            IdeMsg::Restore(to) => {
                let (Some(c), Some((services, agent))) = (&self.open, self.agent()) else { return Task::none() };
                let id = c.id().to_string();
                return Task::perform(on(&services, async move { agent.restore(&id, to).await.map_err(|r| r.message) }), |r| {
                    Msg::Ide(IdeMsg::Restored(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            IdeMsg::Restored(result) => match result {
                Ok(set) => {
                    self.changes = Some(set);
                    self.problem = Some(
                        "The restore is staged as changes to review: nothing is written until you keep them.".to_string(),
                    );
                    self.ide.side = Side::Changes;
                    self.ide.side_open = true;
                }
                Err(why) => self.problem = Some(why),
            },
            IdeMsg::Revoke(entry) => {
                let (Some(workspace), Some((services, agent))) = (self.workspace_id(), self.agent()) else { return Task::none() };
                return Task::perform(
                    on(&services, async move { agent.revoke(&workspace, &entry).await.map_err(|r| r.message) }),
                    |r| match r {
                        Some(Ok(())) => Msg::Ide(IdeMsg::ReadHistory),
                        Some(Err(why)) => Msg::Done(Err(why)),
                        None => Msg::Done(Err(STOPPED.to_string())),
                    },
                );
            }

            IdeMsg::Command(call) => {
                self.ide.command = Some(call.clone());
                self.ide.command_lines = None;
                self.ide.panel = PanelTab::Commands;
                self.ide.panel_open = true;
                return self.read_command(call, 1);
            }
            IdeMsg::CommandRead(call, result) => {
                if self.ide.command.as_deref() == Some(call.as_str()) {
                    match result {
                        Ok(lines) => {
                            let tail = tail_from(&lines);
                            self.ide.command_lines = Some(lines);
                            if let Some(from) = tail {
                                return self.read_command(call, from);
                            }
                        }
                        Err(why) => self.problem = Some(why),
                    }
                }
            }

            IdeMsg::TermNew => return self.start_terminal(None),
            IdeMsg::TermStarted(id, result) => {
                self.ide.term_starting = false;
                let taken = result
                    .and_then(|started| started.take().ok_or_else(|| "The terminal was handed over twice.".to_string()));
                match taken {
                    Ok(term) => {
                        self.ide.terms.push(term);
                        self.ide.term_active = Some(id);
                        self.ide.term_problem = None;
                        if self.ide.panel_open && self.ide.panel == PanelTab::Terminal {
                            return self.focus_terminal();
                        }
                    }
                    Err(why) => self.ide.term_problem = Some(why),
                }
            }
            IdeMsg::TermEvent(id, event) => {
                if let Some(t) = self.ide.term_mut(id) {
                    match event {
                        term::Event::Output(bytes) => t.output(&bytes),
                        term::Event::Ended => t.ended(),
                    }
                }
            }
            IdeMsg::TermInput(id, input) => return self.term_input(id, input),
            IdeMsg::TermShow(id) => {
                self.ide.term_active = Some(id);
                return self.focus_terminal();
            }
            IdeMsg::TermClose(id) => {
                if self.ide.terms.iter().any(|t| t.id == id && t.busy()) {
                    self.ide.ask(Confirm::CloseTerminal { term: id });
                } else {
                    self.ide.close_term(id);
                }
            }
            IdeMsg::TermRestart(id) => {
                let cwd = self.ide.terms.iter().find(|t| t.id == id).map(|t| t.cwd.clone());
                self.ide.close_term(id);
                return self.start_terminal(cwd);
            }

            IdeMsg::FindOpen(replace) => return self.open_find(replace),
            IdeMsg::FindQuery(text) => {
                if let Some(f) = &mut self.ide.find {
                    f.query.text = text;
                    f.note = None;
                    let (id, origin) = (f.tab, f.origin);
                    return self.refind(id, Some(origin), true);
                }
            }
            IdeMsg::FindReplaceText(text) => {
                if let Some(f) = &mut self.ide.find {
                    f.replace = Some(text);
                }
            }
            IdeMsg::FindToggle(toggle) => {
                if let Some(f) = &mut self.ide.find {
                    find::flip(&mut f.query, toggle);
                    let (id, origin) = (f.tab, f.origin);
                    return self.refind(id, Some(origin), true);
                }
            }
            IdeMsg::FindStep(forward) => {
                if let Some(f) = &mut self.ide.find
                    && self.ide.active == Some(f.tab)
                {
                    f.step(forward);
                    let id = f.tab;
                    return self.select_hit(id);
                }
            }
            IdeMsg::ReplaceOne => return self.replace_one(),
            IdeMsg::ReplaceAll => return self.replace_all(),
            IdeMsg::FindClose => {
                if let Some(f) = self.ide.find.take() {
                    return operation::focus(super::editor_id(f.tab));
                }
            }
            IdeMsg::GotoOpen => {
                if self.editing_tab().is_some() {
                    self.ide.goto = Some(String::new());
                    self.ide.term_focus = false;
                    return operation::focus(super::goto_id());
                }
            }
            IdeMsg::GotoText(text) => self.ide.goto = Some(text),
            IdeMsg::GotoGo => return self.go_to_line(),
            IdeMsg::GotoClose => {
                self.ide.goto = None;
                if let Some(id) = self.editing_tab() {
                    return operation::focus(super::editor_id(id));
                }
            }
            IdeMsg::EditorEscape => {
                if self.ide.goto.take().is_none() && self.ide.inline.take().is_none() {
                    self.ide.find = None;
                }
            }
            IdeMsg::InlineOpen => return self.open_inline(),
            IdeMsg::InlinePrompt(text) => {
                if let Some(i) = &mut self.ide.inline {
                    i.prompt = text;
                }
            }
            IdeMsg::InlineSend => return self.send_inline(),
            IdeMsg::InlineClose => {
                if let Some(i) = self.ide.inline.take() {
                    return operation::focus(super::editor_id(i.tab));
                }
            }
            IdeMsg::SearchShow => {
                self.ide.side = Side::Search;
                self.ide.side_open = true;
                self.ide.term_focus = false;
                // A one-line selection in the editor is what to look for, as editors seed it.
                if let Some(seed) = self.selected_line() {
                    self.ide.search.query.text = seed;
                }
                return operation::focus(search::input_id());
            }
            IdeMsg::SearchQuery(text) => self.ide.search.query.text = text,
            IdeMsg::SearchInclude(text) => self.ide.search.include = text,
            IdeMsg::SearchToggle(toggle) => {
                find::flip(&mut self.ide.search.query, toggle);
                if !self.ide.search.query.text.is_empty() {
                    return self.run_search();
                }
            }
            IdeMsg::SearchRun => return self.run_search(),
            IdeMsg::Searched(done) => {
                let (query, include, result) = *done;
                let s = &mut self.ide.search;
                // Only the answer to what was asked last is shown.
                if s.asked.as_ref() == Some(&(query, include)) {
                    s.running = false;
                    s.collapsed.clear();
                    s.report = Some(result);
                }
            }
            IdeMsg::SearchFold(path) => {
                let folded = &mut self.ide.search.collapsed;
                if !folded.remove(&path) {
                    folded.insert(path);
                }
            }
            IdeMsg::SearchFoldAll => {
                if let Some(Ok(report)) = &self.ide.search.report {
                    self.ide.search.collapsed = report.files.iter().map(|f| f.path.clone()).collect();
                }
            }
            IdeMsg::SearchOpen(path, line, start, end) => {
                if let Some(id) = self.ide.file_tab(&path) {
                    self.ide.active = Some(id);
                    return self.reveal_in(id, line, start, end);
                }
                self.ide.reveal = Some((path.clone(), line, start, end));
                return self.open_file(path);
            }
        }
        Task::none()
    }

    /// The editor on show, when it edits a file.
    fn editing_tab(&self) -> Option<u64> {
        let tab = self.ide.active_tab()?;
        matches!(&tab.kind, TabKind::File { body: Body::Editing(_), .. }).then_some(tab.id)
    }

    /// The editor's selection, when it is some text on one line.
    fn selected_line(&self) -> Option<String> {
        let id = self.editing_tab()?;
        let TabKind::File { body: Body::Editing(e), .. } = &self.ide.tab(id)?.kind else { return None };
        e.content.selection().filter(|s| !s.is_empty() && !s.contains('\n') && s.chars().count() <= 200)
    }

    /// Open the find bar on the editor on show (`replace`: with its replace row): what is selected on one line is
    /// what it looks for, else what it last looked for; it matches onwards from the cursor.
    fn open_find(&mut self, replace: bool) -> Task<Msg> {
        let Some(id) = self.editing_tab() else { return Task::none() };
        let seed = self.selected_line();
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        let c = e.content.cursor();
        let at = (c.position.line, c.position.column);
        let origin = c.selection.map_or(at, |s| at.min((s.line, s.column)));
        match &mut self.ide.find {
            Some(f) => {
                f.tab = id;
                f.origin = origin;
                f.note = None;
                if let Some(text) = seed {
                    f.query.text = text;
                }
                if replace && f.replace.is_none() {
                    f.replace = Some(String::new());
                }
            }
            none => *none = Some(find::Find::new(id, seed.unwrap_or_default(), replace, origin)),
        }
        self.ide.term_focus = false;
        let select = self.refind(id, Some(origin), true);
        Task::batch([select, operation::focus(super::find_id())])
    }

    /// Match the find bar's query against tab `id`'s text again, the current match the first at or after `from`
    /// (the cursor when `None`); `select`: the editor selects it and brings it into view.
    fn refind(&mut self, id: u64, from: Option<(usize, usize)>, select: bool) -> Task<Msg> {
        let Some(f) = self.ide.find.as_mut().filter(|f| f.tab == id) else { return Task::none() };
        let Some(TabKind::File { body: Body::Editing(e), .. }) = self.ide.tabs.iter().find(|t| t.id == id).map(|t| &t.kind)
        else {
            return Task::none();
        };
        let lines: Vec<String> = e.content.lines().map(|l| l.text.into_owned()).collect();
        let from = from.unwrap_or_else(|| {
            let c = e.content.cursor().position;
            (c.line, c.column)
        });
        f.run(lines.iter().map(String::as_str), from);
        if select { self.select_hit(id) } else { Task::none() }
    }

    /// Select the find bar's current match in tab `id`'s editor and bring it into view.
    fn select_hit(&mut self, id: u64) -> Task<Msg> {
        let Some(hit) = self.ide.find.as_ref().filter(|f| f.tab == id).and_then(find::Find::hit) else {
            return Task::none();
        };
        self.select_range(id, hit.line, hit.start, hit.end, hit.cells)
    }

    /// Select bytes `start..end` of line `line` in tab `id`'s editor (`cells`: where they are on the screen) and
    /// bring them into view.
    fn select_range(&mut self, id: u64, line: usize, start: usize, end: usize, cells: (usize, usize)) -> Task<Msg> {
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        e.history.break_step();
        // `move_to` keeps a selection it is not given a new one for; a move clears it.
        if start == end && e.content.cursor().selection.is_some() {
            e.content.perform(text_editor::Action::Move(text_editor::Motion::Left));
        }
        e.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column: end },
            selection: (start != end).then_some(text_editor::Position { line, column: start }),
        });
        let across = self.reveal_across(id, cells);
        Task::batch([self.follow(id), across])
    }

    /// Scroll tab `id`'s editor across, when `cells` are out of view.
    fn reveal_across(&mut self, id: u64, cells: (usize, usize)) -> Task<Msg> {
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        let (offset, width) = e.across.unwrap_or((0.0, 600.0));
        let w = super::term::cell().0;
        // The gutter's width (its numbers and padding), then the editor's padding, are left of the text.
        let digits = e.lines.max(1).to_string().len().max(3) as f32;
        let left = 22.0 + digits * w + CODE_PAD;
        let (x0, x1) = (left + cells.0 as f32 * w, left + cells.1 as f32 * w);
        let target = if x0 < offset + left {
            Some(x0 - left - 4.0 * w)
        } else if x1 > offset + width {
            Some(x1 - width + 4.0 * w)
        } else {
            None
        };
        match target {
            Some(x) => operation::scroll_to(super::scroll_id(id), AbsoluteOffset { x: Some(x.max(0.0)), y: None }),
            None => Task::none(),
        }
    }

    /// Replace the current match (as one edit, which undo takes back), and go on to the next.
    fn replace_one(&mut self) -> Task<Msg> {
        let Some((id, hit)) = self.ide.find.as_ref().and_then(|f| f.hit().map(|h| (f.tab, h))) else { return Task::none() };
        let Some(line) = self.editing_mut(id).and_then(|e| e.content.line(hit.line).map(|l| l.text.into_owned())) else {
            return Task::none();
        };
        let Some(with) = self.ide.find.as_ref().and_then(|f| f.replacement(&line, hit)) else { return Task::none() };
        if let Some(e) = self.editing_mut(id) {
            e.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: hit.line, column: hit.end },
                selection: Some(text_editor::Position { line: hit.line, column: hit.start }),
            });
        }
        let edit = self.edit(id, text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(with.clone()))));
        // Onwards from what was put in: a match inside the replacement is not replaced again.
        let after = match with.rsplit_once('\n') {
            Some((head, tail)) => (hit.line + head.matches('\n').count() + 1, tail.len()),
            None => (hit.line, hit.start + with.len()),
        };
        let select = self.refind(id, Some(after), true);
        Task::batch([edit, select])
    }

    /// Replace every match (as one edit, which undo takes back).
    fn replace_all(&mut self) -> Task<Msg> {
        let Some(id) = self.ide.find.as_ref().map(|f| f.tab) else { return Task::none() };
        let Some(lines) = self.editing_mut(id).map(|e| e.content.lines().map(|l| (l.text.into_owned(), l.ending)).collect::<Vec<_>>())
        else {
            return Task::none();
        };
        let Some((replaced, count)) = self.ide.find.as_ref().and_then(|f| f.replace_lines(lines.iter().map(|(t, _)| t.as_str())))
        else {
            return Task::none();
        };
        if count == 0 {
            if let Some(f) = &mut self.ide.find {
                f.note = Some("Nothing to replace".to_string());
            }
            return Task::none();
        }
        let mut text = String::new();
        for (i, (line, (_, ending))) in replaced.iter().zip(&lines).enumerate() {
            text.push_str(line);
            if i + 1 < lines.len() {
                text.push_str(if *ending == text_editor::LineEnding::None { "\n" } else { ending.as_str() });
            }
        }
        if let Some(e) = self.editing_mut(id) {
            let caret = e.caret();
            let before = e.content.text();
            e.history.record(&before, caret, buffer::EditKind::Other);
            e.history.break_step();
            e.replace_text(&text, caret);
        }
        if let Some(f) = &mut self.ide.find {
            f.note = Some(if count == 1 { "Replaced 1".to_string() } else { format!("Replaced {count}") });
        }
        self.refind(id, None, false)
    }

    /// Go to the line (and column) typed in Go to line's box.
    fn go_to_line(&mut self) -> Task<Msg> {
        let Some(typed) = self.ide.goto.clone() else { return Task::none() };
        let Some(id) = self.editing_tab() else { return Task::none() };
        let Some((line, column)) = find::parse_goto(&typed) else { return Task::none() };
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        let now = e.content.cursor().position.line;
        let line = line.map_or(now, |l| l - 1).min(e.content.line_count().saturating_sub(1));
        let text = e.content.line(line).map(|l| l.text.into_owned()).unwrap_or_default();
        let byte = find::byte_of(&text, column.map_or(0, |c| c - 1));
        let cells = find::cells(&text, byte);
        self.ide.goto = None;
        let moved = self.select_range(id, line, byte, byte, (cells, cells));
        Task::batch([moved, operation::focus(super::editor_id(id))])
    }

    /// A search result's match selected in tab `id` (line from 1, a byte range); the file may have changed since it
    /// was searched, so the range is held to the line as it is now.
    pub(super) fn reveal_in(&mut self, id: u64, line: u32, start: usize, end: usize) -> Task<Msg> {
        let line = (line as usize).saturating_sub(1);
        let Some(text) = self.editing_mut(id).and_then(|e| e.content.line(line).map(|l| l.text.into_owned())) else {
            return Task::none();
        };
        let (mut start, mut end) = (start.min(text.len()), end.min(text.len()));
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) || start > end {
            (start, end) = (0, 0);
        }
        let cells = (find::cells(&text, start), find::cells(&text, end));
        self.select_range(id, line, start, end, cells)
    }

    /// Open the inline edit on the editor on show: the lines selected (the cursor's line with none).
    fn open_inline(&mut self) -> Task<Msg> {
        let Some(id) = self.editing_tab() else { return Task::none() };
        let Some(path) = self.ide.tab(id).and_then(|t| t.path()).map(str::to_string) else { return Task::none() };
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        let c = e.content.cursor();
        let (a, b) = match c.selection {
            Some(s) if (s.line, s.column) <= (c.position.line, c.position.column) => (s, c.position),
            Some(s) => (c.position, s),
            None => (c.position, c.position),
        };
        // A selection that ends at the start of a line does not take that line.
        let to = if b.line > a.line && b.column == 0 { b.line - 1 } else { b.line };
        self.ide.inline = Some(find::Inline { tab: id, path, from: a.line, to, prompt: String::new() });
        self.ide.term_focus = false;
        operation::focus(super::inline_id())
    }

    /// Send the inline edit to the agent, in Agent mode, in the chat working in this folder.
    fn send_inline(&mut self) -> Task<Msg> {
        let Some(inline) = self.ide.inline.clone() else { return Task::none() };
        if inline.prompt.trim().is_empty() || self.sending {
            return Task::none();
        }
        let here = self.ide.folder.as_ref().map(|f| f.id().to_string());
        if here.is_none() || self.chat_workspace() != here {
            self.problem = Some(
                "The open chat works in another folder, or in none: start a new chat in this folder for an inline edit."
                    .to_string(),
            );
            return Task::none();
        }
        let Some(e) = self.editing_mut(inline.tab) else { return Task::none() };
        let lines: Vec<String> = (inline.from..=inline.to).filter_map(|n| e.content.line(n).map(|l| l.text.into_owned())).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let text = find::inline_message(&inline.path, inline.from, inline.to, &refs, &inline.prompt);
        self.ide.inline = None;
        self.send_text(text, lattice_protocol::conversation::Mode::Agent, None, true)
    }

    /// The editor on show, its language, and the lines its cursor and selection touch (from 0, both included: a
    /// selection that ends at a line's start does not take that line), with the cursor and the selection's other end.
    fn touched(&mut self) -> Option<(u64, Lang, usize, usize, text_editor::Cursor)> {
        let id = self.editing_tab()?;
        let lang = match &self.ide.tab(id)?.kind {
            TabKind::File { lang, .. } => *lang,
            _ => return None,
        };
        let e = self.editing_mut(id)?;
        let c = e.content.cursor();
        let (a, b) = match c.selection {
            Some(s) if (s.line, s.column) <= (c.position.line, c.position.column) => (s, c.position),
            Some(s) => (c.position, s),
            None => (c.position, c.position),
        };
        let to = if b.line > a.line && b.column == 0 { b.line - 1 } else { b.line };
        Some((id, lang, a.line, to, c))
    }

    fn lines_of(&mut self, id: u64, from: usize, to: usize) -> Vec<String> {
        match self.editing_mut(id) {
            Some(e) => (from..=to).filter_map(|n| e.content.line(n).map(|l| l.text.into_owned())).collect(),
            None => Vec::new(),
        }
    }

    /// The file's indentation unit, as learnt when it opened.
    fn unit_of(&mut self, id: u64) -> String {
        self.editing_mut(id).map_or_else(|| "    ".to_string(), |e| e.unit.clone())
    }

    /// Put the cursor at `cursor` in tab `id`, the selection's other end at `anchor` (none: no selection).
    fn place(&mut self, id: u64, cursor: (usize, usize), anchor: Option<(usize, usize)>) {
        if let Some(e) = self.editing_mut(id) {
            if anchor.is_none() && e.content.cursor().selection.is_some() {
                e.content.perform(text_editor::Action::Move(text_editor::Motion::Left));
            }
            e.content.move_to(text_editor::Cursor {
                position: text_editor::Position { line: cursor.0, column: cursor.1 },
                selection: anchor.map(|(line, column)| text_editor::Position { line, column }),
            });
        }
    }

    /// Bytes `from..to` of tab `id` (as (line, byte) both) typed over with `with`, as one edit the undo takes back.
    fn type_over(&mut self, id: u64, from: (usize, usize), to: (usize, usize), with: String) -> Task<Msg> {
        self.place(id, to, Some(from));
        self.edit(id, text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(with))))
    }

    /// Lines `from..=to` of tab `id` replaced by `lines`, as one edit; the cursor then at `cursor`, the selection's
    /// other end at `anchor`.
    fn replace_lines(
        &mut self,
        id: u64,
        from: usize,
        to: usize,
        lines: &[String],
        cursor: (usize, usize),
        anchor: Option<(usize, usize)>,
    ) -> Task<Msg> {
        let end = self.editing_mut(id).and_then(|e| e.content.line(to).map(|l| l.text.len())).unwrap_or(0);
        let edit = self.type_over(id, (from, 0), (to, end), lines.join("\n"));
        self.place(id, cursor, anchor);
        Task::batch([edit, self.follow(id)])
    }

    /// Enter, as an editor for code types it (`editing::newline`).
    fn newline(&mut self) -> Task<Msg> {
        let Some((id, lang, _, _, c)) = self.touched() else { return Task::none() };
        let at = match c.selection {
            Some(s) if (s.line, s.column) < (c.position.line, c.position.column) => s,
            _ => c.position,
        };
        let line = self.lines_of(id, at.line, at.line).pop().unwrap_or_default();
        let unit = self.unit_of(id);
        let (text, (down, column)) = editing::newline(&line, at.column, lang, &unit);
        let typed = self.edit(id, text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text))));
        self.place(id, (at.line + down, column), None);
        Task::batch([typed, self.follow(id)])
    }

    /// Tab and Shift+Tab: with lines selected, each indented or unindented by the file's unit; else Tab types the
    /// unit (spaces to the next stop) and Shift+Tab unindents the line.
    fn indent_lines(&mut self, more: bool) -> Task<Msg> {
        let Some((id, _, from, to, c)) = self.touched() else { return Task::none() };
        let unit = self.unit_of(id);
        let across = c.selection.is_some_and(|s| s.line != c.position.line);
        if more && !across {
            let line = self.lines_of(id, c.position.line, c.position.line).pop().unwrap_or_default();
            let text = if unit == "\t" {
                unit
            } else {
                let at = line.get(..c.position.column).map_or(0, |h| h.chars().count());
                " ".repeat(unit.len() - at % unit.len())
            };
            return self.edit(id, text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text))));
        }
        let lines = self.lines_of(id, from, to);
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        // Each end of the selection moves with its line's text.
        let (out, shift): (Vec<String>, Vec<isize>) = if more {
            let out = editing::indent(&refs, &unit);
            let shift = refs.iter().map(|l| if l.trim().is_empty() { 0 } else { unit.len() as isize }).collect();
            (out, shift)
        } else {
            let (out, lost) = editing::unindent(&refs, &unit);
            (out, lost.into_iter().map(|n| -(n as isize)).collect())
        };
        let moved = |(line, column): (usize, usize)| -> (usize, usize) {
            // An end outside the lines, or at a line's start, stays where it is (the lines stay selected whole).
            if line < from || line > to || column == 0 {
                return (line, column);
            }
            let k = line - from;
            let len = out.get(k).map_or(0, String::len);
            (line, ((column as isize + shift.get(k).copied().unwrap_or(0)).max(0) as usize).min(len))
        };
        let cursor = moved((c.position.line, c.position.column));
        let anchor = c.selection.map(|s| moved((s.line, s.column)));
        self.replace_lines(id, from, to, &out, cursor, anchor)
    }

    /// Ctrl+/: the lines commented out, or back in, as the language writes comments (nothing for JSON or plain text).
    fn toggle_comment(&mut self) -> Task<Msg> {
        let Some((id, lang, from, to, c)) = self.touched() else { return Task::none() };
        let Some(marks) = editing::comment(lang) else { return Task::none() };
        let lines = self.lines_of(id, from, to);
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (out, _) = editing::toggle_comment(&refs, marks);
        // A selection across lines selects them whole; a cursor keeps its place in its line's text.
        let (cursor, anchor) = match c.selection {
            Some(_) if from != to || c.selection.is_some_and(|s| s.line != c.position.line) => {
                ((to, out.last().map_or(0, String::len)), Some((from, 0)))
            }
            _ => {
                let k = c.position.line - from;
                let (old, new) = (refs.get(k).map_or(0, |l| l.len()), out.get(k).map_or(0, String::len));
                let pad = refs.get(k).map_or(0, |l| editing::leading(l).len());
                let column = if c.position.column <= pad {
                    c.position.column
                } else {
                    ((c.position.column as isize + new as isize - old as isize).max(pad as isize) as usize).min(new)
                };
                ((c.position.line, column), None)
            }
        };
        self.replace_lines(id, from, to, &out, cursor, anchor)
    }

    /// Alt+Up and Alt+Down: the lines moved past the one above or below them.
    fn move_lines(&mut self, up: bool) -> Task<Msg> {
        let Some((id, _, from, to, c)) = self.touched() else { return Task::none() };
        let count = self.editing_mut(id).map_or(0, |e| e.content.line_count());
        if (up && from == 0) || (!up && to + 1 >= count) {
            return Task::none();
        }
        let (start, end) = if up { (from - 1, to) } else { (from, to + 1) };
        let mut lines = self.lines_of(id, start, end);
        if up {
            lines.rotate_left(1);
        } else {
            lines.rotate_right(1);
        }
        let shift = |(line, column): (usize, usize)| if up { (line - 1, column) } else { (line + 1, column) };
        let cursor = shift((c.position.line, c.position.column));
        let anchor = c.selection.map(|s| shift((s.line, s.column)));
        self.replace_lines(id, start, end, &lines, cursor, anchor)
    }

    /// Shift+Alt+Up and Shift+Alt+Down: the lines copied above or below themselves; the cursor on the copy that is
    /// above (Up) or below (Down).
    fn copy_lines(&mut self, up: bool) -> Task<Msg> {
        let Some((id, _, from, to, c)) = self.touched() else { return Task::none() };
        let block = self.lines_of(id, from, to);
        let mut lines = block.clone();
        lines.extend(block);
        let n = to - from + 1;
        let shift = |(line, column): (usize, usize)| if up { (line, column) } else { (line + n, column) };
        let cursor = shift((c.position.line, c.position.column));
        let anchor = c.selection.map(|s| shift((s.line, s.column)));
        self.replace_lines(id, from, to, &lines, cursor, anchor)
    }

    /// Ctrl+Shift+K: the lines deleted, their line ends with them.
    fn delete_lines(&mut self) -> Task<Msg> {
        let Some((id, _, from, to, c)) = self.touched() else { return Task::none() };
        let count = self.editing_mut(id).map_or(0, |e| e.content.line_count());
        let len = |state: &mut State, n: usize| state.editing_mut(id).and_then(|e| e.content.line(n).map(|l| l.text.len())).unwrap_or(0);
        let (start, end, cursor) = if to + 1 < count {
            ((from, 0), (to + 1, 0), (from, 0))
        } else if from > 0 {
            let above = len(self, from - 1);
            ((from - 1, above), (to, len(self, to)), (from - 1, c.position.column.min(above)))
        } else {
            ((0, 0), (to, len(self, to)), (0, 0))
        };
        let deleted = self.type_over(id, start, end, String::new());
        self.place(id, cursor, None);
        Task::batch([deleted, self.follow(id)])
    }

    /// Home: the first character that is not a space, then the line's start (Shift+Home selects as it goes).
    fn home(&mut self, select: bool) -> Task<Msg> {
        let Some((id, _, _, _, c)) = self.touched() else { return Task::none() };
        let line = self.lines_of(id, c.position.line, c.position.line).pop().unwrap_or_default();
        let to = editing::home(&line, c.position.column);
        let anchor = if select { Some(c.selection.map_or((c.position.line, c.position.column), |s| (s.line, s.column))) } else { None };
        if let Some(e) = self.editing_mut(id) {
            e.history.break_step();
        }
        self.place(id, (c.position.line, to), anchor);
        let cells = (to, to);
        let across = self.reveal_across(id, cells);
        Task::batch([self.follow(id), across])
    }

    /// Search the folder for what the Search view asks.
    fn run_search(&mut self) -> Task<Msg> {
        let Some(folder) = self.ide.folder.clone() else { return Task::none() };
        let s = &mut self.ide.search;
        if s.query.text.is_empty() {
            s.report = None;
            return Task::none();
        }
        let query = s.query.clone();
        let include = s.include.trim().to_string();
        s.asked = Some((query.clone(), include.clone()));
        s.running = true;
        let args = SearchArgs {
            query: query.clone(),
            glob: (!include.is_empty()).then(|| include.clone()),
            max_matches: search::MAX_RESULTS,
        };
        Task::perform(off_thread(move || folder.search(&args)), move |r| {
            Msg::Ide(IdeMsg::Searched(Box::new((query, include, r.unwrap_or_else(|| Err(STOPPED.to_string()))))))
        })
    }

    /// Show or hide the bottom panel; shown on the terminals, the terminal on show takes the keyboard.
    fn toggle_panel(&mut self) -> Task<Msg> {
        if self.ide.panel_open {
            self.ide.panel_open = false;
            self.ide.term_focus = false;
            Task::none()
        } else if self.ide.panel == PanelTab::Terminal {
            self.show_terminals()
        } else {
            self.ide.panel_open = true;
            Task::none()
        }
    }

    /// Show the terminals (starting one when there is none) and give the one on show the keyboard.
    pub(in crate::lattice) fn show_terminals(&mut self) -> Task<Msg> {
        self.ide.panel = PanelTab::Terminal;
        self.ide.panel_open = true;
        if self.ide.terms.is_empty() {
            if self.ide.term_starting || self.ide.term_problem.is_some() {
                return Task::none();
            }
            return self.start_terminal(None);
        }
        if self.ide.shown_term().is_none() {
            self.ide.term_active = self.ide.terms.last().map(|t| t.id);
        }
        self.focus_terminal()
    }

    /// The terminal on show takes the keyboard, and whichever text box had it loses it (or both would read the keys).
    fn focus_terminal(&mut self) -> Task<Msg> {
        if self.ide.shown_term().is_none() {
            return Task::none();
        }
        self.ide.term_focus = true;
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
    }

    /// Start the person's shell in `cwd`, else the folder the IDE shows, else the person's home folder.
    pub(in crate::lattice) fn start_terminal(&mut self, cwd: Option<PathBuf>) -> Task<Msg> {
        if self.ide.term_starting {
            return Task::none();
        }
        if self.ide.terms.len() >= term::MAX_TERMINALS {
            self.ide.term_problem =
                Some(format!("{} terminals are open, the most at once: close one first.", term::MAX_TERMINALS));
            return Task::none();
        }
        let cwd = cwd
            .or_else(|| self.ide.folder.as_ref().map(|f| PathBuf::from(f.shown_path())))
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from(r"C:\"));
        let id = self.ide.take_term_id();
        let (cols, rows) = self.ide.term_cells;
        self.ide.term_starting = true;
        self.ide.term_problem = None;
        Task::perform(off_thread(move || term::Term::start(id, &cwd, cols, rows).map(term::Started::new)), move |r| {
            Msg::Ide(IdeMsg::TermStarted(id, r.unwrap_or_else(|| Err("The terminal's start was cut short.".to_string()))))
        })
    }

    /// What a terminal's grid read.
    fn term_input(&mut self, id: u64, input: term::grid::Input) -> Task<Msg> {
        use term::grid::Input;
        match input {
            Input::Focus(on) => {
                self.ide.term_focus = on;
                if on {
                    self.ide.term_active = Some(id);
                }
            }
            Input::Keys(bytes) => {
                if let Some(t) = self.ide.term_mut(id) {
                    let _ = t.input(&bytes);
                }
            }
            Input::Paste(text) => {
                let text = term::paste_text(&text).to_string();
                let bracketed = self.ide.terms.iter().find(|t| t.id == id).is_some_and(|t| t.screen.bracketed_paste);
                if term::paste_needs_asking(&text, bracketed) {
                    let runs = !bracketed && text.contains(['\n', '\r']);
                    self.ide.ask(Confirm::Paste { term: id, text, runs });
                } else {
                    self.paste(id, &text);
                }
            }
            Input::Size(cols, rows) => {
                self.ide.term_cells = (cols, rows);
                if let Some(t) = self.ide.term_mut(id) {
                    t.resize(cols, rows);
                }
            }
            Input::Scroll(lines) => {
                if let Some(t) = self.ide.term_mut(id) {
                    t.scroll(lines);
                }
            }
        }
        Task::none()
    }

    /// Type `text` into terminal `id`, as a paste.
    fn paste(&mut self, id: u64, text: &str) {
        if let Some(t) = self.ide.term_mut(id) {
            let bytes = term::keys::paste(text, t.screen.bracketed_paste);
            let _ = t.input(&bytes);
        }
    }

    /// The output of the agent's command `call`, from line `from`.
    pub(in crate::lattice) fn read_command(&mut self, call: String, from: u32) -> Task<Msg> {
        let (Some(c), Some((services, agent))) = (&self.open, self.agent()) else { return Task::none() };
        let view = ViewRef::Output { conversation: c.id().to_string(), call_id: call.clone() };
        Task::perform(
            on(&services, async move { agent.read_lines(view, from, PAGE_LINES).await.map_err(|r| r.message) }),
            move |r| Msg::Ide(IdeMsg::CommandRead(call.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        )
    }

    /// The editor of tab `id`, when it is one.
    pub(in crate::lattice) fn editing_mut(&mut self, id: u64) -> Option<&mut Editing> {
        match self.ide.tab_mut(id).map(|t| &mut t.kind) {
            Some(TabKind::File { body: Body::Editing(e), .. }) => Some(e),
            _ => None,
        }
    }

    /// A new chat's state: nothing open, the composer empty, the last chat's reviews and outputs closed. The folder is
    /// the caller's to attach.
    pub(in crate::lattice) fn start_new_chat(&mut self) {
        self.fixture = false;
        self.ide.close_chat_tabs();
        self.open = None;
        self.changes = None;
        self.diff = None;
        self.ide.editing_turn = None;
        self.ide.checkpoints = None;
        self.ide.command = None;
        self.ide.command_lines = None;
        self.composer = text_editor::Content::new();
        self.folder.view = None;
        self.folder.busy = false;
    }

    /// Attach the editor's folder for the next new chat: natively, since the person already opened it here (no new
    /// authority: the same folder, which the core asked about, or the person chose, when it was opened).
    pub(in crate::lattice) fn attach_open_folder(&mut self) -> Task<Msg> {
        let (Some(folder), Some(services)) = (self.ide.folder.clone(), self.services.clone()) else { return Task::none() };
        self.folder.path = folder.shown_path();
        self.folder.busy = true;
        let core = services.chat.clone();
        let path = PathBuf::from(folder.shown_path());
        Task::perform(on(&services, async move { core.attach_native(None, path).await.map_err(|r| r.message) }), |r| {
            Msg::Attached(r.unwrap_or_else(|| Err(STOPPED.to_string())))
        })
    }

    /// The chat's folder view and the editor's folder are one folder (both name the same workspace id), or say why not.
    pub(in crate::lattice) fn same_folder(&self) -> Result<(), String> {
        match (&self.folder.view, &self.ide.folder) {
            (Some(view), Some(folder)) if view.workspace.id != folder.id() => Err(
                "The folder changed while it was being opened (its identity differs from the one the chat attached), \
                 so it is not shown: open it again."
                    .to_string(),
            ),
            _ => Ok(()),
        }
    }

    /// Open `path` as the editor's folder, asking first when tabs of another folder hold unsaved edits.
    pub(in crate::lattice) fn ask_open_folder(&mut self, path: String, native: bool) -> Task<Msg> {
        if path.is_empty() || self.ide.opening {
            return Task::none();
        }
        if self.ide.any_dirty() {
            self.ide.ask(Confirm::SwitchFolder { path, native });
            return Task::none();
        }
        self.open_folder(path, native)
    }

    /// Open `path` for the editor, and attach it for the chat: to the open chat when it has no folder yet, else to a
    /// new chat (a chat keeps the folder it began in). `native`: the person chose it in Windows' picker, dropped it
    /// on the window or named it on the command line, so the core attaches it with no dialog (`attach_native`); a
    /// typed path is attached through the core's own confirmation.
    pub(in crate::lattice) fn open_folder(&mut self, path: String, native: bool) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.opening = true;
        let state = services.state.clone();
        let asked = path.clone();
        let ide = Task::perform(
            off_thread(move || {
                files::Folder::attach(&PathBuf::from(&asked), Arc::new(lattice_core::ProcessEnv), &state).map(Arc::new)
            }),
            move |r| Msg::Ide(IdeMsg::FolderOpened(path.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        );
        if self.open.as_ref().is_some_and(|c| c.summary.workspace.is_some()) {
            self.start_new_chat();
        }
        self.folder.path = self.ide.folder_path.trim().to_string();
        self.folder.view = None;
        let chat = if native {
            self.folder.busy = true;
            // A recording shown for a photograph is not the store's: the folder goes to the next new chat.
            let id = self.open.as_ref().filter(|_| !self.fixture).map(|c| c.id().to_string());
            let core = services.chat.clone();
            let folder = PathBuf::from(&self.folder.path);
            Task::perform(
                on(&services, async move { core.attach_native(id.as_deref(), folder).await.map_err(|r| r.message) }),
                |r| Msg::Attached(r.unwrap_or_else(|| Err(STOPPED.to_string()))),
            )
        } else {
            self.update(Msg::Attach)
        };
        Task::batch([ide, chat])
    }

    /// The open chat works in a folder the editor does not show: show it, unless tabs hold unsaved edits (the chat's
    /// header then offers the switch).
    pub(in crate::lattice) fn follow_folder(&mut self) -> Task<Msg> {
        if self.fixture {
            return Task::none();
        }
        let Some(badge) = self.open.as_ref().and_then(|c| c.summary.workspace.clone()) else { return Task::none() };
        if self.ide.folder.as_ref().is_some_and(|f| f.id() == badge.id) || self.ide.opening || self.ide.any_dirty() {
            return Task::none();
        }
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.opening = true;
        self.ide.folder_path = badge.path.clone();
        let state = services.state.clone();
        let path = badge.path.clone();
        Task::perform(
            off_thread(move || {
                files::Folder::attach(&PathBuf::from(&path), Arc::new(lattice_core::ProcessEnv), &state).map(Arc::new)
            }),
            move |r| Msg::Ide(IdeMsg::FolderOpened(badge.path.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        )
    }

    pub(in crate::lattice) fn read_listing(&mut self) -> Task<Msg> {
        let Some(folder) = self.ide.folder.clone() else { return Task::none() };
        self.ide.listing_busy = true;
        let id = folder.id().to_string();
        Task::perform(
            off_thread(move || {
                folder.listing().map(|FileListing { paths, truncated, withheld }| Listing {
                    folder: folder.id().to_string(),
                    tree: tree::build(paths.iter().map(String::as_str)),
                    paths,
                    truncated,
                    withheld,
                })
            }),
            move |r| Msg::Ide(IdeMsg::Listed(id.clone(), Box::new(r.unwrap_or_else(|| Err(STOPPED.to_string()))))),
        )
    }

    /// Show `path` in the editor: its tab when it has one, else a new tab read off the window's thread.
    pub(in crate::lattice) fn open_file(&mut self, path: String) -> Task<Msg> {
        if let Some(id) = self.ide.file_tab(&path) {
            self.ide.active = Some(id);
            return Task::none();
        }
        let Some(folder) = self.ide.folder.clone() else {
            self.problem = Some("Open a folder first: the editor shows the folder the chat works in.".to_string());
            return Task::none();
        };
        for parent in tree::folders_of(&path) {
            self.ide.expanded.insert(parent);
        }
        let id = self.ide.push(TabKind::File { path: path.clone(), lang: Lang::of(&path), body: Body::Loading, note: None });
        Task::perform(off_thread(move || folder.open(&path)), move |r| {
            Msg::Ide(IdeMsg::Opened(id, Box::new(r.unwrap_or_else(|| Err(STOPPED.to_string())))))
        })
    }

    fn opened(&mut self, id: u64, result: Result<files::Opened, String>) -> Task<Msg> {
        let mut notes = false;
        let Some(tab) = self.ide.tab_mut(id) else { return Task::none() };
        let TabKind::File { path, lang, body, note } = &mut tab.kind else { return Task::none() };
        match result {
            Ok(opened) => {
                // The file system's own spelling of the path.
                *path = opened.path.clone();
                *lang = Lang::of(path);
                match opened.text {
                    Ok(decoded) => {
                        *body = Body::Editing(Box::new(Editing::new(opened.sha256, opened.authority, decoded, *lang)));
                        notes = true;
                    }
                    Err(why) => {
                        *body = Body::Paged { why: why.sentence().to_string(), lines: None, reading: false, colours: Vec::new() };
                        return self.read_page(id, 1);
                    }
                }
                let _ = note;
            }
            Err(why) => *body = Body::Failed(why),
        }
        let path = path.clone();
        let _ = self.refind(id, None, false);
        let revealed = match self.ide.reveal.take() {
            Some((p, line, start, end)) if p == path => self.reveal_in(id, line, start, end),
            other => {
                self.ide.reveal = other;
                Task::none()
            }
        };
        // Its notes, placed on the text as read.
        let revealed = if notes { Task::batch([revealed, self.read_notes(path.clone())]) } else { revealed };
        #[cfg(debug_assertions)]
        let revealed = Task::batch([revealed, self.photograph(id), if notes { self.photograph_notes(path) } else { Task::none() }]);
        revealed
    }

    /// A debug build's photographs of the find bar, the Search view and the inline edit, on the first file opened:
    /// `CENTCOM_LATTICE_FIND=<text>` finds and searches for it, `CENTCOM_LATTICE_INLINE=<prompt>` opens an inline edit
    /// of lines 10 to 14 with that prompt typed (never sent), `CENTCOM_LATTICE_COMPLETE=<line>:<column>` (from 0) puts
    /// the cursor there and asks for a completion as typing does. A release build never reads them.
    #[cfg(debug_assertions)]
    fn photograph(&mut self, id: u64) -> Task<Msg> {
        if self.ide.find.is_some() || self.ide.inline.is_some() || self.ide.search.asked.is_some() {
            return Task::none();
        }
        let complete_at = std::env::var("CENTCOM_LATTICE_COMPLETE").ok().and_then(|at| {
            let (line, column) = at.split_once(':')?;
            Some((line.trim().parse::<usize>().ok()?, column.trim().parse::<usize>().ok()?))
        });
        if let Some((line, column)) = complete_at {
            if let Some(e) = self.editing_mut(id) {
                e.content.move_to(text_editor::Cursor { position: text_editor::Position { line, column }, selection: None });
            }
            return self.typed(id);
        }
        if let Some(text) = std::env::var_os("CENTCOM_LATTICE_FIND").map(|t| t.to_string_lossy().into_owned()) {
            self.ide.search.query.text = text.clone();
            self.ide.side = Side::Search;
            self.ide.side_open = true;
            let search = self.run_search();
            let open = self.open_find(true);
            if let Some(f) = &mut self.ide.find {
                f.query.text = text;
                f.replace = Some("replaced".to_string());
            }
            let found = self.refind(id, None, true);
            return Task::batch([search, open, found]);
        }
        if let Some(prompt) = std::env::var_os("CENTCOM_LATTICE_INLINE").map(|t| t.to_string_lossy().into_owned()) {
            if let Some(e) = self.editing_mut(id) {
                e.content.move_to(text_editor::Cursor {
                    position: text_editor::Position { line: 14, column: 0 },
                    selection: Some(text_editor::Position { line: 9, column: 0 }),
                });
            }
            let open = self.open_inline();
            if let Some(i) = &mut self.ide.inline {
                i.prompt = prompt;
            }
            return open;
        }
        Task::none()
    }

    fn reload(&mut self, id: u64) -> Task<Msg> {
        let (Some(folder), Some(path)) = (self.ide.folder.clone(), self.ide.tab(id).and_then(|t| t.path()).map(str::to_string))
        else {
            return Task::none();
        };
        if let Some(tab) = self.ide.tab_mut(id)
            && let TabKind::File { body, note, .. } = &mut tab.kind
        {
            *body = Body::Loading;
            *note = Some(("Read again from disk.".to_string(), false));
        }
        Task::perform(off_thread(move || folder.open(&path)), move |r| {
            Msg::Ide(IdeMsg::Opened(id, Box::new(r.unwrap_or_else(|| Err(STOPPED.to_string())))))
        })
    }

    /// A page of a file shown read-only, through the core (`read_lines`, as the agent reads it).
    fn read_page(&mut self, id: u64, from: u32) -> Task<Msg> {
        let (Some(folder), Some((services, agent))) = (self.ide.folder.clone(), self.agent()) else { return Task::none() };
        let Some(tab) = self.ide.tab_mut(id) else { return Task::none() };
        let TabKind::File { path, body: Body::Paged { reading, .. }, .. } = &mut tab.kind else { return Task::none() };
        *reading = true;
        let view = ViewRef::File { workspace: folder.id().to_string(), path: path.clone() };
        Task::perform(
            on(&services, async move { agent.read_lines(view, from.max(1), PAGE_LINES).await.map_err(|r| r.message) }),
            move |r| Msg::Ide(IdeMsg::PageRead(id, r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        )
    }

    fn read_output(&mut self, id: u64, from: u32) -> Task<Msg> {
        let (Some(c), Some((services, agent))) = (&self.open, self.agent()) else { return Task::none() };
        let conversation = c.id().to_string();
        let Some(tab) = self.ide.tab_mut(id) else { return Task::none() };
        let TabKind::Output { call, reading, .. } = &mut tab.kind else { return Task::none() };
        *reading = true;
        let view = ViewRef::Output { conversation, call_id: call.clone() };
        Task::perform(
            on(&services, async move { agent.read_lines(view, from.max(1), PAGE_LINES).await.map_err(|r| r.message) }),
            move |r| Msg::Ide(IdeMsg::OutputRead(id, r.unwrap_or_else(|| Err(STOPPED.to_string())))),
        )
    }

    /// An action in the editor of tab `id`: an edit is recorded for undo first; a wheel turn scrolls the editor's
    /// own scrollable (the editor is as tall as its text, so its line numbers scroll with it).
    fn edit(&mut self, id: u64, action: text_editor::Action) -> Task<Msg> {
        let edited = matches!(action, text_editor::Action::Edit(_));
        // Typing asks for a completion once it pauses; moving the cursor puts the suggestion away.
        let asked = match &action {
            text_editor::Action::Scroll { .. } => Task::none(),
            text_editor::Action::Edit(_) => self.typed(id),
            _ => {
                self.moved();
                Task::none()
            }
        };
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        match &action {
            text_editor::Action::Scroll { lines } => {
                return operation::scroll_by(
                    super::scroll_id(id),
                    AbsoluteOffset { x: 0.0, y: *lines as f32 * LINE_HEIGHT },
                );
            }
            text_editor::Action::Edit(edit) => {
                let kind = buffer::kind_of(edit);
                let caret = e.caret();
                let before = e.content.text();
                e.history.record(&before, caret, kind);
                e.content.perform(action);
                e.refresh();
            }
            text_editor::Action::Click(_) | text_editor::Action::Drag(_) => {
                e.history.break_step();
                e.content.perform(action);
                return Task::none();
            }
            _ => {
                e.history.break_step();
                e.content.perform(action);
            }
        }
        if edited {
            let _ = self.refind(id, None, false);
        }
        Task::batch([asked, self.follow(id)])
    }

    /// Type `text` at the cursor of tab `id`, as one edit.
    pub(in crate::lattice) fn type_text(&mut self, id: u64, text: String) -> Task<Msg> {
        self.edit(id, text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text))))
    }

    /// Keep the cursor of tab `id`'s editor in view.
    fn follow(&mut self, id: u64) -> Task<Msg> {
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        let line = e.content.cursor().position.line as f32;
        let (offset, height) = e.viewport.unwrap_or((0.0, 600.0));
        let top = CODE_PAD + line * LINE_HEIGHT;
        let bottom = top + LINE_HEIGHT;
        let target = if top < offset {
            Some(top - CODE_PAD)
        } else if bottom > offset + height {
            Some(bottom + CODE_PAD - height)
        } else {
            None
        };
        match target {
            Some(y) => operation::scroll_to(super::scroll_id(id), AbsoluteOffset { x: None, y: Some(y.max(0.0)) }),
            None => Task::none(),
        }
    }

    /// Save tab `id` (see [`files::Folder::save`]); `confirmed` once the person confirmed an authority file.
    fn save(&mut self, id: u64, confirmed: bool) -> Task<Msg> {
        let Some(folder) = self.ide.folder.clone() else { return Task::none() };
        let Some(tab) = self.ide.tab_mut(id) else { return Task::none() };
        let TabKind::File { path, body: Body::Editing(e), .. } = &mut tab.kind else { return Task::none() };
        if e.saving {
            return Task::none();
        }
        e.saving = true;
        let text = e.content.text();
        let bytes = buffer::encode(&text, e.bom, e.ending);
        e.writing = Some(text);
        let (path, base) = (path.clone(), e.base_sha256.clone());
        Task::perform(off_thread(move || folder.save(&path, &base, &bytes, confirmed)), move |r| {
            Msg::Ide(IdeMsg::Saved(id, r.unwrap_or_else(|| Err(SaveError::Failed(STOPPED.to_string())))))
        })
    }

    fn create(&mut self, path: String, confirmed: bool) -> Task<Msg> {
        let Some(folder) = self.ide.folder.clone() else { return Task::none() };
        if path.is_empty() {
            return Task::none();
        }
        Task::perform(off_thread(move || folder.create(&path, b"", confirmed)), |r| {
            Msg::Ide(IdeMsg::Created(r.unwrap_or_else(|| Err(SaveError::Failed(STOPPED.to_string())))))
        })
    }

    /// The Explorer's filter, ranked against the listing.
    fn rank_filter(&mut self) {
        let query = self.ide.filter.trim().to_string();
        self.ide.filtered = match (query.is_empty(), self.ide.files()) {
            (false, Some(listing)) => lattice_core::text::rank::rank_paths(&query, listing.paths.iter().map(String::as_str), 200),
            _ => Vec::new(),
        };
    }

    fn quick_rank(&mut self) {
        let paths: Vec<String> = self.ide.files().map(|l| l.paths.clone()).unwrap_or_default();
        if let Some(q) = &mut self.ide.quick {
            q.results = if q.query.trim().is_empty() {
                // Nothing typed: the open tabs, then the folder's first files.
                let mut shown: Vec<_> = self
                    .ide
                    .tabs
                    .iter()
                    .filter_map(|t| t.path().map(str::to_string))
                    .chain(paths.iter().take(40).cloned())
                    .collect();
                shown.dedup();
                shown
                    .into_iter()
                    .take(40)
                    .map(|path| lattice_protocol::conversation::RankedPath { path, score: 0, positions: Vec::new() })
                    .collect()
            } else {
                lattice_core::text::rank::rank_paths(&q.query, paths.iter().map(String::as_str), 40)
            };
            q.selected = q.selected.min(q.results.len().saturating_sub(1));
        }
    }

    fn key(&mut self, key: Key) -> Task<Msg> {
        // While a terminal has the keyboard its keys are its program's (Ctrl+S, Ctrl+W, Ctrl+L, Esc and the arrows
        // too): only the IDE's Ctrl+P, Ctrl+B and Ctrl+J or Ctrl+` act.
        if self.ide.terminal_focused() && !matches!(key, Key::QuickOpen | Key::ToggleSide | Key::TogglePanel) {
            return Task::none();
        }
        match key {
            Key::QuickOpen => {
                self.ide.term_focus = false;
                return self.ide_update(IdeMsg::QuickOpen);
            }
            Key::Save => return self.ide_update(IdeMsg::Save),
            Key::ToggleSide => self.ide.side_open = !self.ide.side_open,
            Key::TogglePanel => return self.toggle_panel(),
            Key::CloseTab => {
                if let Some(id) = self.ide.active {
                    return self.ide_update(IdeMsg::Close(id));
                }
            }
            Key::FocusChat => {
                self.ide.term_focus = false;
                return operation::focus(super::composer_id());
            }
            Key::Find => return self.ide_update(IdeMsg::FindOpen(false)),
            Key::Replace => return self.ide_update(IdeMsg::FindOpen(true)),
            Key::GotoLine => return self.ide_update(IdeMsg::GotoOpen),
            Key::Inline => return self.ide_update(IdeMsg::InlineOpen),
            Key::Search => return self.ide_update(IdeMsg::SearchShow),
            Key::FindNext => return self.ide_update(IdeMsg::FindStep(true)),
            Key::FindPrevious => return self.ide_update(IdeMsg::FindStep(false)),
            Key::Escape => {
                if self.ide.quick.is_some() {
                    self.ide.quick = None;
                } else if self.ide.confirm.is_some() {
                    self.ide.confirm = None;
                } else if self.ide.goto.is_some() {
                    return self.ide_update(IdeMsg::GotoClose);
                } else if self.ide.inline.is_some() {
                    return self.ide_update(IdeMsg::InlineClose);
                } else if self.ide.find.is_some() {
                    return self.ide_update(IdeMsg::FindClose);
                } else if self.ide.new_file.is_some() {
                    self.ide.new_file = None;
                }
            }
            Key::Up | Key::Down => {
                if let Some(q) = &mut self.ide.quick
                    && !q.results.is_empty()
                {
                    let n = q.results.len();
                    q.selected = if key == Key::Up { (q.selected + n - 1) % n } else { (q.selected + 1) % n };
                }
            }
        }
        Task::none()
    }

    /// The Changes view's history: the open chat's checkpoints, and the folder's standing approvals.
    fn read_history(&mut self) -> Task<Msg> {
        let Some((services, agent)) = self.agent() else { return Task::none() };
        let mut tasks = Vec::new();
        if let Some(c) = &self.open {
            let id = c.id().to_string();
            let a = agent.clone();
            tasks.push(Task::perform(on(&services, async move { a.checkpoints(&id).await.map_err(|r| r.message) }), |r| {
                Msg::Ide(IdeMsg::CheckpointsRead(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
            }));
        }
        if let Some(workspace) = self.workspace_id() {
            tasks.push(Task::perform(on(&services, async move { agent.permissions(&workspace).await.map_err(|r| r.message) }), |r| {
                Msg::Ide(IdeMsg::PermissionsRead(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
            }));
        }
        Task::batch(tasks)
    }

    /// Read version `version` of the artifact tab `id` shows, through the core.
    fn read_artifact(&mut self, id: u64, version: u32) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let Some((conversation, name)) = self.ide.tabs.iter().find(|t| t.id == id).and_then(|t| match &t.kind {
            TabKind::Artifact { conversation, name, .. } => Some((conversation.clone(), name.clone())),
            _ => None,
        }) else {
            return Task::none();
        };
        let chat = services.chat.clone();
        let work = async move { chat.artifact(&conversation, &name, Some(version)).await.map_err(|r| r.message) };
        Task::perform(on(&services, work), move |r| {
            Msg::Ide(IdeMsg::ArtifactRead(id, Box::new(r.unwrap_or_else(|| Err(STOPPED.to_string())))))
        })
    }

    /// After the agent's change to `path` was kept: a tab showing it without unsaved edits reads it again; one with
    /// unsaved edits says that its next save will be refused, as the file is no longer the one it opened.
    pub(in crate::lattice) fn kept_on_disk(&mut self, paths: &[String]) -> Task<Msg> {
        let mut tasks = Vec::new();
        let ids: Vec<(u64, bool)> = self
            .ide
            .tabs
            .iter()
            .filter(|t| matches!(&t.kind, TabKind::File { path, .. } if paths.contains(path)))
            .map(|t| (t.id, t.dirty()))
            .collect();
        for (id, dirty) in ids {
            if dirty {
                if let Some(tab) = self.ide.tab_mut(id)
                    && let TabKind::File { note, .. } = &mut tab.kind
                {
                    *note = Some((
                        "The agent's change to this file was written: your unsaved edits are of the older text, so Save \
                         is refused until you reload."
                            .to_string(),
                        true,
                    ));
                }
            } else {
                tasks.push(self.reload(id));
            }
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::ide::{Ide, MAX_TABS};

    fn editing(text: &str) -> Editing {
        Editing::new("sha".into(), false, buffer::decode(text.as_bytes()).unwrap(), Lang::Rust)
    }

    #[test]
    fn an_edit_makes_the_file_dirty_and_undoing_it_makes_it_clean_again() {
        let mut state = State::new();
        let id = state.ide.push(TabKind::File {
            path: "a.rs".into(),
            lang: Lang::Rust,
            body: Body::Editing(Box::new(editing("fn a() {}\n"))),
            note: None,
        });
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Edit(text_editor::Edit::Insert('x'))));
        assert!(state.ide.tab(id).unwrap().dirty());
        let _ = state.ide_update(IdeMsg::Undo);
        assert!(!state.ide.tab(id).unwrap().dirty(), "undo back to the saved text is clean");
        let _ = state.ide_update(IdeMsg::Redo);
        assert!(state.ide.tab(id).unwrap().dirty());
    }

    #[test]
    fn closing_or_reloading_a_dirty_tab_asks_first_and_an_early_click_is_not_an_answer() {
        let mut state = State::new();
        let id = state.ide.push(TabKind::File {
            path: "a.rs".into(),
            lang: Lang::Rust,
            body: Body::Editing(Box::new(editing("x"))),
            note: None,
        });
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Edit(text_editor::Edit::Insert('y'))));
        let _ = state.ide_update(IdeMsg::Close(id));
        assert_eq!(state.ide.confirm.as_ref().map(|(c, _)| c.clone()), Some(Confirm::Discard { tab: id }));
        let _ = state.ide_update(IdeMsg::Confirm(true));
        assert!(state.ide.tab(id).is_some(), "a click in the quiet half second closed the tab");
        state.ide.confirm.as_mut().unwrap().1 -= super::super::CONFIRM_QUIET;
        let _ = state.ide_update(IdeMsg::Confirm(false));
        assert!(state.ide.tab(id).is_some() && state.ide.confirm.is_none(), "No keeps the tab");
        let _ = state.ide_update(IdeMsg::Reload(id));
        assert_eq!(state.ide.confirm.as_ref().map(|(c, _)| c.clone()), Some(Confirm::Reload { tab: id }));
    }

    #[test]
    fn a_save_refused_for_an_authority_file_asks_and_a_changed_file_is_said() {
        let mut state = State::new();
        let id = state.ide.push(TabKind::File {
            path: "AGENTS.md".into(),
            lang: Lang::Markdown,
            body: Body::Editing(Box::new(editing("rules"))),
            note: None,
        });
        let _ = state.ide_update(IdeMsg::Saved(id, Err(SaveError::NeedsConfirm)));
        assert_eq!(state.ide.confirm.as_ref().map(|(c, _)| c.clone()), Some(Confirm::SaveAuthority { tab: id }));
        let _ = state.ide_update(IdeMsg::Saved(id, Err(SaveError::Changed)));
        let TabKind::File { note, .. } = &state.ide.tab(id).unwrap().kind else { panic!() };
        assert!(note.as_ref().is_some_and(|(n, warn)| *warn && n.contains("changed on disk")));
        assert!(state.problem.as_deref().is_some_and(|p| p.starts_with("AGENTS.md: ")));
    }

    #[test]
    fn quick_open_ranks_the_listing_and_the_arrows_move_through_it() {
        let mut state = State::new();
        let mut ide = Ide::default();
        let paths: Vec<String> = ["src/main.rs", "src/lattice/mod.rs", "README.md"].iter().map(|s| s.to_string()).collect();
        ide.listing = Some(Ok(Listing { folder: "f".into(), tree: tree::build(paths.iter().map(String::as_str)), paths, truncated: false, withheld: vec![] }));
        state.ide = ide;
        // No folder is attached in a unit test: the listing is read only for the folder it belongs to.
        assert!(state.ide.files().is_none());
        let _ = state.ide_update(IdeMsg::QuickOpen);
        assert!(state.ide.quick.is_some());
        let _ = state.ide_update(IdeMsg::Key(Key::Escape));
        assert!(state.ide.quick.is_none());
        assert!(MAX_TABS > 1);
    }

    #[test]
    fn an_edit_made_while_a_save_runs_stays_unsaved() {
        let mut state = State::new();
        let id = state.ide.push(TabKind::File {
            path: "a.rs".into(),
            lang: Lang::Rust,
            body: Body::Editing(Box::new(editing("one"))),
            note: None,
        });
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Edit(text_editor::Edit::Insert('x'))));
        // Save pressed: the text at that moment is what is written.
        if let Some(e) = state.editing_mut(id) {
            e.saving = true;
            e.writing = Some(e.content.text());
        }
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Edit(text_editor::Edit::Insert('y'))));
        let saved = files::Saved { path: "a.rs".into(), sha256: "new".into(), size: 4, changed_after: false };
        let _ = state.ide_update(IdeMsg::Saved(id, Ok(saved)));
        let e = state.editing_mut(id).unwrap();
        assert_eq!((e.base_sha256.as_str(), e.saving), ("new", false));
        assert!(e.dirty, "the edit typed during the save was marked saved");
        assert_eq!(e.saved, "xone");
    }

    #[test]
    fn a_model_that_is_not_ready_says_why_and_leaves_the_composer_alone() {
        use lattice_protocol::Locality;
        use lattice_protocol::chat::ChatChoice;
        let mut state = State::new();
        state.choices = vec![
            ChatChoice { id: "auto".into(), label: "Auto".into(), detail: String::new(), locality: Locality::Local, ready: true, refusal: None },
            ChatChoice { id: "cloud".into(), label: "Cloud".into(), detail: String::new(), locality: Locality::Remote, ready: false, refusal: Some("No key is set.".into()) },
        ];
        state.choice = "auto".into();
        state.composer = text_editor::Content::with_text("draft");
        let _ = state.update(Msg::ChoiceLabel(super::super::agent::choice_label(&state.choices[1])));
        assert_eq!((state.choice.as_str(), state.composer.text().as_str()), ("auto", "draft"));
        assert_eq!(state.problem.as_deref(), Some("No key is set."));
        let _ = state.update(Msg::ChoiceLabel("Auto".into()));
        assert_eq!(state.choice, "auto");
    }

    #[test]
    fn the_gutter_holds_every_number_and_where_each_starts() {
        let (text, starts) = Editing::numbers(3);
        assert_eq!(text, "  1\n  2\n  3");
        assert_eq!(starts, vec![0, 4, 8, 11]);
        let (wide, at) = Editing::numbers(1200);
        assert!(wide.starts_with("   1\n") && wide.ends_with("1200"));
        assert_eq!(at.len(), 1201);
        let mut e = editing("a\nb");
        assert_eq!(e.gutter.1.len(), 3);
        e.content.perform(text_editor::Action::Edit(text_editor::Edit::Enter));
        e.refresh();
        assert_eq!((e.lines, e.gutter.1.len()), (3, 4), "a new line grows the gutter");
    }

    #[test]
    fn the_explorers_filter_is_ranked_when_typed_against_the_folders_listing() {
        let s = files::tests::Scratch::new("filter");
        std::fs::create_dir_all(s.work().join("src")).unwrap();
        for name in ["main.rs", "lib.rs", "mainline.md"] {
            std::fs::write(s.work().join("src").join(name), b"x").unwrap();
        }
        let folder = Arc::new(s.folder());
        let listed = folder.listing().unwrap();
        let mut state = State::new();
        state.ide.listing = Some(Ok(Listing {
            folder: folder.id().to_string(),
            tree: tree::build(listed.paths.iter().map(String::as_str)),
            paths: listed.paths,
            truncated: false,
            withheld: Vec::new(),
        }));
        state.ide.folder = Some(folder);
        let _ = state.ide_update(IdeMsg::Filter("main".into()));
        let found: Vec<&str> = state.ide.filtered.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(found.first(), Some(&"src/main.rs"));
        assert!(!found.contains(&"src/lib.rs"));
        let _ = state.ide_update(IdeMsg::Filter(String::new()));
        assert!(state.ide.filtered.is_empty());
    }

    #[test]
    fn a_long_output_is_read_again_from_its_last_page() {
        let page = |from: u32, n: u32, total: u32| Lines { from, total, lines: vec![String::new(); n as usize], truncated: false };
        assert_eq!(tail_from(&page(1, PAGE_LINES, 1000)), Some(1000 - PAGE_LINES + 1));
        assert_eq!(tail_from(&page(601, PAGE_LINES, 1000)), None, "already the last page");
        assert_eq!(tail_from(&page(1, 12, 12)), None, "short output, whole");
        assert_eq!(tail_from(&page(1, 0, 0)), None);
    }

    #[test]
    fn the_keys_map_to_the_ides_actions_only_when_no_text_box_took_them() {
        use iced::keyboard::{Event, Key as K, Location, Modifiers, key};
        let press = |key: K, modifiers: Modifiers| {
            iced::Event::Keyboard(Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let window = iced::window::Id::unique();
        let ignored = iced::event::Status::Ignored;
        let captured = iced::event::Status::Captured;
        let got = |e, s| match keys(e, s, window) {
            Some(Msg::Ide(IdeMsg::Key(k))) => Some(k),
            _ => None,
        };
        assert_eq!(got(press(K::Character("p".into()), Modifiers::CTRL), ignored), Some(Key::QuickOpen));
        assert_eq!(got(press(K::Character("b".into()), Modifiers::CTRL), captured), Some(Key::ToggleSide));
        assert_eq!(got(press(K::Character("s".into()), Modifiers::CTRL), captured), None, "the editor saved it itself");
        assert_eq!(got(press(K::Named(key::Named::Escape), Modifiers::empty()), ignored), Some(Key::Escape));
        assert_eq!(got(press(K::Named(key::Named::ArrowDown), Modifiers::empty()), captured), None);
        assert_eq!(got(press(K::Character("x".into()), Modifiers::empty()), ignored), None);
    }

    /// A page showing terminal 7 (no shell behind it) with the keyboard, and a clean file open.
    fn with_terminal() -> (State, u64) {
        let mut state = State::new();
        let tab = state.ide.push(TabKind::File {
            path: "a.rs".into(),
            lang: Lang::Rust,
            body: Body::Editing(Box::new(editing("x"))),
            note: None,
        });
        state.ide.terms.push(term::Term::unstarted(7, &std::env::temp_dir()));
        state.ide.term_active = Some(7);
        state.ide.panel = PanelTab::Terminal;
        state.ide.panel_open = true;
        let _ = state.ide_update(IdeMsg::TermInput(7, term::grid::Input::Focus(true)));
        assert!(state.ide.terminal_focused());
        (state, tab)
    }

    #[test]
    fn a_terminal_with_the_keyboard_keeps_its_programs_keys_and_the_ides_own_chords_still_act() {
        let (mut state, tab) = with_terminal();
        // Ctrl+W, Ctrl+L, Ctrl+S, Esc and the arrows are the shell's while the terminal has the keyboard.
        for key in [Key::CloseTab, Key::FocusChat, Key::Save, Key::Escape, Key::Up, Key::Down] {
            let _ = state.ide_update(IdeMsg::Key(key));
        }
        assert!(state.ide.tab(tab).is_some() && state.ide.terminal_focused(), "nothing of the IDE's acted");
        // Ctrl+J hides the panel, and the keyboard goes with it.
        let _ = state.ide_update(IdeMsg::Key(Key::TogglePanel));
        assert!(!state.ide.panel_open && !state.ide.term_focus);
        let _ = state.ide_update(IdeMsg::Key(Key::CloseTab));
        assert!(state.ide.tab(tab).is_none(), "without the terminal's focus Ctrl+W closes the tab");
        // Ctrl+P takes the keyboard to the quick-open box.
        let (mut state, _) = with_terminal();
        let _ = state.ide_update(IdeMsg::Key(Key::QuickOpen));
        assert!(state.ide.quick.is_some() && !state.ide.term_focus);
    }

    #[test]
    fn a_click_elsewhere_or_typing_in_the_editor_takes_the_keyboard_from_the_terminal() {
        let (mut state, tab) = with_terminal();
        let _ = state.ide_update(IdeMsg::TermInput(7, term::grid::Input::Focus(false)));
        assert!(!state.ide.terminal_focused());
        let (mut state, _) = with_terminal();
        let _ = state.ide_update(IdeMsg::Edit(tab, text_editor::Action::Scroll { lines: 3 }));
        assert!(state.ide.terminal_focused(), "the wheel over the editor leaves the keyboard where it was");
        let _ = state.ide_update(IdeMsg::Edit(tab, text_editor::Action::Edit(text_editor::Edit::Insert('y'))));
        assert!(!state.ide.terminal_focused());
        let (mut state, _) = with_terminal();
        let _ = state.ide_update(IdeMsg::Panel(PanelTab::Commands));
        assert!(!state.ide.term_focus, "the panel's other tab");
    }

    #[test]
    fn a_paste_that_would_run_lines_waits_for_a_yes_and_a_single_line_is_typed_without_its_end() {
        let (mut state, _) = with_terminal();
        let _ = state.ide_update(IdeMsg::TermInput(7, term::grid::Input::Paste("cd x\r\nrm -r y\r\n".into())));
        assert_eq!(
            state.ide.confirm.as_ref().map(|(c, _)| c.clone()),
            Some(Confirm::Paste { term: 7, text: "cd x\r\nrm -r y\r\n".into(), runs: true })
        );
        assert!(!state.ide.terminal_focused(), "nothing reaches the shell while it is asked");
        state.ide.confirm = None;
        let _ = state.ide_update(IdeMsg::TermInput(7, term::grid::Input::Paste("cargo test\r\n".into())));
        assert!(state.ide.confirm.is_none(), "one line is typed, not asked about");
    }

    #[test]
    fn closing_an_idle_terminal_does_not_ask_and_the_one_beside_it_is_shown() {
        let (mut state, _) = with_terminal();
        state.ide.terms.push(term::Term::unstarted(8, &std::env::temp_dir()));
        let _ = state.ide_update(IdeMsg::TermClose(7));
        assert!(state.ide.confirm.is_none());
        assert_eq!(state.ide.terms.iter().map(|t| t.id).collect::<Vec<_>>(), vec![8]);
        assert_eq!(state.ide.term_active, Some(8));
        let _ = state.ide_update(IdeMsg::TermClose(8));
        assert!(state.ide.terms.is_empty() && !state.ide.term_focus && state.ide.term_active.is_none());
        let _ = state.ide_update(IdeMsg::TermStarted(9, Err("No PowerShell was found on this computer.".into())));
        assert_eq!(state.ide.term_problem.as_deref(), Some("No PowerShell was found on this computer."));
        assert!(!state.ide.term_starting);
    }

    /// A page with `src/a.rs` open for editing (in no folder), and its tab.
    fn editing_page(text: &str) -> (State, u64) {
        let mut state = State::new();
        let id = state.ide.push(TabKind::File {
            path: "src/a.rs".into(),
            lang: Lang::Rust,
            body: Body::Editing(Box::new(editing(text))),
            note: None,
        });
        (state, id)
    }

    fn editor(state: &State, id: u64) -> &Editing {
        match &state.ide.tab(id).unwrap().kind {
            TabKind::File { body: Body::Editing(e), .. } => e,
            _ => panic!("not an editor"),
        }
    }

    /// The cursor and the selection's other end, as (line, column).
    fn caret(state: &State, id: u64) -> ((usize, usize), Option<(usize, usize)>) {
        let c = editor(state, id).content.cursor();
        ((c.position.line, c.position.column), c.selection.map(|s| (s.line, s.column)))
    }

    fn select(state: &mut State, id: u64, from: (usize, usize), to: (usize, usize)) {
        let Some(e) = state.editing_mut(id) else { panic!() };
        e.content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: to.0, column: to.1 },
            selection: Some(text_editor::Position { line: from.0, column: from.1 }),
        });
    }

    #[test]
    fn find_selects_matches_onwards_from_the_cursor_steps_through_them_and_follows_edits() {
        let (mut state, id) = editing_page("let a = 1;\nlet b = a + a;\n");
        let _ = state.ide_update(IdeMsg::FindOpen(false));
        let _ = state.ide_update(IdeMsg::FindQuery("a".into()));
        let f = state.ide.find.as_ref().unwrap();
        assert_eq!((f.hits.len(), f.status()), (3, "1 of 3".to_string()));
        assert_eq!(caret(&state, id), ((0, 5), Some((0, 4))), "the match is selected");
        let _ = state.ide_update(IdeMsg::FindStep(true));
        assert_eq!(caret(&state, id), ((1, 9), Some((1, 8))));
        let _ = state.ide_update(IdeMsg::FindStep(false));
        let _ = state.ide_update(IdeMsg::FindStep(false));
        assert_eq!(caret(&state, id), ((1, 13), Some((1, 12))), "before the first, the last");
        // An edit in the editor finds again: the second match moved on.
        select(&mut state, id, (1, 8), (1, 9));
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("a ".into())))));
        let starts: Vec<(usize, usize)> = state.ide.find.as_ref().unwrap().hits.iter().map(|h| (h.line, h.start)).collect();
        assert_eq!(starts, vec![(0, 4), (1, 8), (1, 13)]);
        let _ = state.ide_update(IdeMsg::Key(Key::Escape));
        assert!(state.ide.find.is_none(), "Esc closes the find bar");
    }

    #[test]
    fn replace_one_replaces_the_match_and_goes_on_and_undo_takes_each_back() {
        let (mut state, id) = editing_page("a1 a2 a3\n");
        let _ = state.ide_update(IdeMsg::FindOpen(true));
        let _ = state.ide_update(IdeMsg::FindQuery("a".into()));
        let _ = state.ide_update(IdeMsg::FindReplaceText("b".into()));
        let _ = state.ide_update(IdeMsg::ReplaceOne);
        assert_eq!(editor(&state, id).content.text(), "b1 a2 a3\n");
        assert_eq!(caret(&state, id), ((0, 4), Some((0, 3))), "the next match is selected");
        let _ = state.ide_update(IdeMsg::ReplaceOne);
        assert_eq!(editor(&state, id).content.text(), "b1 b2 a3\n");
        assert!(editor(&state, id).dirty);
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(editor(&state, id).content.text(), "b1 a2 a3\n");
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(editor(&state, id).content.text(), "a1 a2 a3\n");
        assert!(!editor(&state, id).dirty, "undone to the saved text");
    }

    #[test]
    fn replace_all_is_one_edit_with_groups_filled_in_and_says_how_many() {
        let (mut state, id) = editing_page("x=1\ny=2\nkeep\n");
        let _ = state.ide_update(IdeMsg::FindOpen(true));
        let _ = state.ide_update(IdeMsg::FindToggle(find::Toggle::Regex));
        let _ = state.ide_update(IdeMsg::FindQuery(r"(\w)=(\d)".into()));
        let _ = state.ide_update(IdeMsg::FindReplaceText("$2=$1".into()));
        let _ = state.ide_update(IdeMsg::ReplaceAll);
        assert_eq!(editor(&state, id).content.text(), "1=x\n2=y\nkeep\n");
        let f = state.ide.find.as_ref().unwrap();
        assert_eq!((f.note.as_deref(), f.hits.len()), (Some("Replaced 2"), 0));
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(editor(&state, id).content.text(), "x=1\ny=2\nkeep\n", "one undo for all of it");
        let _ = state.ide_update(IdeMsg::FindQuery("zzz".into()));
        let _ = state.ide_update(IdeMsg::ReplaceAll);
        assert_eq!(state.ide.find.as_ref().unwrap().note.as_deref(), Some("Nothing to replace"));
    }

    #[test]
    fn go_to_line_counts_columns_in_characters_and_holds_to_the_file() {
        let (mut state, id) = editing_page("h\u{e9}llo\nworld\n");
        let _ = state.ide_update(IdeMsg::GotoOpen);
        let _ = state.ide_update(IdeMsg::GotoText("1:3".into()));
        let _ = state.ide_update(IdeMsg::GotoGo);
        assert_eq!(caret(&state, id), ((0, 3), None), "the third character starts at byte 3");
        assert!(state.ide.goto.is_none());
        let _ = state.ide_update(IdeMsg::GotoOpen);
        let _ = state.ide_update(IdeMsg::GotoText("99".into()));
        let _ = state.ide_update(IdeMsg::GotoGo);
        assert_eq!(caret(&state, id).0.0, 2, "past the end, the last line");
    }

    #[test]
    fn an_inline_edit_takes_the_selected_lines_and_needs_the_chat_in_this_folder() {
        let (mut state, id) = editing_page("one\ntwo\nthree\n");
        select(&mut state, id, (0, 1), (2, 0));
        let _ = state.ide_update(IdeMsg::InlineOpen);
        let inline = state.ide.inline.clone().unwrap();
        assert_eq!((inline.from, inline.to, inline.path.as_str()), (0, 1, "src/a.rs"), "a selection ending at a line's start");
        let _ = state.ide_update(IdeMsg::InlinePrompt("number them".into()));
        let _ = state.ide_update(IdeMsg::InlineSend);
        assert!(!state.sending && state.ide.inline.is_some(), "nothing is sent without the folder");
        assert!(state.problem.as_deref().is_some_and(|p| p.contains("another folder, or in none")));
        let _ = state.ide_update(IdeMsg::Key(Key::Escape));
        assert!(state.ide.inline.is_none());
    }

    #[test]
    fn only_the_last_search_asked_is_shown_and_a_result_opens_at_its_match() {
        use lattice_core::tools::read::{FileMatches, LineMatch};
        let (mut state, id) = editing_page("fn main() {}\nlet alpha = 1;\n");
        let asked = Query { text: "alpha".into(), ..Query::default() };
        state.ide.search.asked = Some((asked.clone(), String::new()));
        state.ide.search.running = true;
        let stale = Query { text: "alp".into(), ..Query::default() };
        let report = SearchReport {
            files: vec![FileMatches {
                path: "src/a.rs".into(),
                lines: vec![LineMatch { line: 2, text: "let alpha = 1;".into(), ranges: vec![4..9] }],
            }],
            matches: 1,
            files_matched: 1,
            shown: 1,
            ..SearchReport::default()
        };
        let _ = state.ide_update(IdeMsg::Searched(Box::new((stale, String::new(), Ok(SearchReport::default())))));
        assert!(state.ide.search.report.is_none() && state.ide.search.running, "an older answer is not shown");
        let _ = state.ide_update(IdeMsg::Searched(Box::new((asked, String::new(), Ok(report)))));
        assert!(!state.ide.search.running);
        assert_eq!(state.ide.search.report.as_ref().unwrap().as_ref().unwrap().matches, 1);
        let _ = state.ide_update(IdeMsg::SearchOpen("src/a.rs".into(), 2, 4, 9));
        assert_eq!(state.ide.active, Some(id));
        assert_eq!(caret(&state, id), ((1, 9), Some((1, 4))));
        // A range the file no longer has is held to its line.
        let _ = state.ide_update(IdeMsg::SearchOpen("src/a.rs".into(), 2, 40, 90));
        assert_eq!(caret(&state, id), ((1, 14), None));
    }

    fn text(state: &State, id: u64) -> String {
        editor(state, id).content.text()
    }

    #[test]
    fn enter_keeps_the_indentation_opens_brackets_and_is_undone_in_one_step() {
        let (mut state, id) = editing_page("fn a() {\n    let x = 1;\n}\n");
        state.place(id, (1, 14), None);
        let _ = state.ide_update(IdeMsg::Newline);
        assert_eq!(text(&state, id), "fn a() {\n    let x = 1;\n    \n}\n");
        assert_eq!(caret(&state, id), ((2, 4), None));
        state.place(id, (0, 8), None);
        let _ = state.ide_update(IdeMsg::Newline);
        assert_eq!(text(&state, id), "fn a() {\n    \n    let x = 1;\n    \n}\n", "one deeper after an opening bracket");
        assert_eq!(caret(&state, id), ((1, 4), None));
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(text(&state, id), "fn a() {\n    let x = 1;\n    \n}\n");
        let (mut pair, id) = editing_page("f({})\n");
        pair.place(id, (0, 3), None);
        let _ = pair.ide_update(IdeMsg::Newline);
        assert_eq!(text(&pair, id), "f({\n    \n})\n", "the closing bracket on a line of its own");
        assert_eq!(caret(&pair, id), ((1, 4), None));
    }

    #[test]
    fn tab_indents_by_the_files_own_unit_and_shift_tab_takes_it_off() {
        let (mut state, id) = editing_page("fn a() {\n  let x = 1;\n  let y = 2;\n}\n");
        select(&mut state, id, (1, 0), (2, 12));
        let _ = state.ide_update(IdeMsg::Indent(true));
        assert_eq!(text(&state, id), "fn a() {\n    let x = 1;\n    let y = 2;\n}\n", "two spaces: this file's unit");
        assert_eq!(caret(&state, id), ((2, 14), Some((1, 0))), "the lines stay selected");
        let _ = state.ide_update(IdeMsg::Indent(false));
        assert_eq!(text(&state, id), "fn a() {\n  let x = 1;\n  let y = 2;\n}\n");
        state.place(id, (1, 2), None);
        let _ = state.ide_update(IdeMsg::Indent(true));
        assert_eq!(text(&state, id), "fn a() {\n    let x = 1;\n  let y = 2;\n}\n", "no lines selected: the unit typed");
        let mut tabs = State::new();
        let id = tabs.ide.push(TabKind::File {
            path: "a.go".into(),
            lang: Lang::Go,
            body: Body::Editing(Box::new(editing("func a() {\n\tx := 1\n}\n"))),
            note: None,
        });
        tabs.place(id, (1, 7), None);
        let _ = tabs.ide_update(IdeMsg::Indent(true));
        assert_eq!(text(&tabs, id), "func a() {\n\tx := 1\t\n}\n", "a file indented with tabs gets a tab");
    }

    #[test]
    fn a_comment_toggles_and_the_cursor_keeps_its_place_in_the_text() {
        let (mut state, id) = editing_page("fn a() {\n    let x = 1;\n}\n");
        state.place(id, (1, 8), None);
        let _ = state.ide_update(IdeMsg::ToggleComment);
        assert_eq!(text(&state, id), "fn a() {\n    // let x = 1;\n}\n");
        assert_eq!(caret(&state, id), ((1, 11), None));
        let _ = state.ide_update(IdeMsg::ToggleComment);
        assert_eq!(text(&state, id), "fn a() {\n    let x = 1;\n}\n");
        assert_eq!(caret(&state, id), ((1, 8), None));
        select(&mut state, id, (0, 0), (2, 1));
        let _ = state.ide_update(IdeMsg::ToggleComment);
        assert_eq!(text(&state, id), "// fn a() {\n//     let x = 1;\n// }\n");
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(text(&state, id), "fn a() {\n    let x = 1;\n}\n", "one undo");
    }

    #[test]
    fn lines_move_copy_and_delete_each_as_one_edit() {
        let (mut state, id) = editing_page("a\nb\nc\n");
        state.place(id, (1, 0), None);
        let _ = state.ide_update(IdeMsg::MoveLines(true));
        assert_eq!((text(&state, id), caret(&state, id)), ("b\na\nc\n".to_string(), ((0, 0), None)));
        let _ = state.ide_update(IdeMsg::MoveLines(true));
        assert_eq!(text(&state, id), "b\na\nc\n", "the first line goes no higher");
        let _ = state.ide_update(IdeMsg::MoveLines(false));
        assert_eq!((text(&state, id), caret(&state, id)), ("a\nb\nc\n".to_string(), ((1, 0), None)));
        let _ = state.ide_update(IdeMsg::CopyLines(false));
        assert_eq!((text(&state, id), caret(&state, id)), ("a\nb\nb\nc\n".to_string(), ((2, 0), None)));
        let _ = state.ide_update(IdeMsg::DeleteLines);
        assert_eq!((text(&state, id), caret(&state, id)), ("a\nb\nc\n".to_string(), ((2, 0), None)));
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(text(&state, id), "a\nb\nb\nc\n", "the delete undone in one step");
        select(&mut state, id, (0, 0), (2, 0));
        let _ = state.ide_update(IdeMsg::MoveLines(false));
        assert_eq!(text(&state, id), "b\na\nb\nc\n", "a selection ending at a line's start does not take that line");
    }

    #[test]
    fn home_goes_to_the_first_character_then_the_lines_start_and_shift_selects() {
        let (mut state, id) = editing_page("    let x\n");
        state.place(id, (0, 9), None);
        let _ = state.ide_update(IdeMsg::Home(false));
        assert_eq!(caret(&state, id), ((0, 4), None));
        let _ = state.ide_update(IdeMsg::Home(false));
        assert_eq!(caret(&state, id), ((0, 0), None));
        state.place(id, (0, 9), None);
        let _ = state.ide_update(IdeMsg::Home(true));
        assert_eq!(caret(&state, id), ((0, 4), Some((0, 9))));
    }

    #[test]
    fn the_find_keys_map_and_a_terminal_with_the_keyboard_keeps_them() {
        use iced::keyboard::{Event, Key as K, Location, Modifiers, key};
        let press = |key: K, modifiers: Modifiers| {
            iced::Event::Keyboard(Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let window = iced::window::Id::unique();
        let got = |e, s| match keys(e, s, window) {
            Some(Msg::Ide(IdeMsg::Key(k))) => Some(k),
            _ => None,
        };
        let (ignored, captured) = (iced::event::Status::Ignored, iced::event::Status::Captured);
        let f = || K::Character("f".into());
        assert_eq!(got(press(f(), Modifiers::CTRL), ignored), Some(Key::Find));
        assert_eq!(got(press(f(), Modifiers::CTRL), captured), None, "the editor opened it itself");
        assert_eq!(got(press(f(), Modifiers::CTRL | Modifiers::SHIFT), captured), Some(Key::Search));
        assert_eq!(got(press(K::Character("k".into()), Modifiers::CTRL), ignored), Some(Key::Inline));
        assert_eq!(got(press(K::Named(key::Named::F3), Modifiers::SHIFT), captured), Some(Key::FindPrevious));
        assert_eq!(
            got(press(K::Named(key::Named::Escape), Modifiers::empty()), captured),
            Some(Key::Escape),
            "Esc in a box (which takes it to give up its focus) still closes what the box is in"
        );
        let (mut state, _) = with_terminal();
        let _ = state.ide_update(IdeMsg::Key(Key::Find));
        let _ = state.ide_update(IdeMsg::Key(Key::Inline));
        assert!(state.ide.find.is_none() && state.ide.inline.is_none(), "Ctrl+F and Ctrl+K are the shell's then");
    }
}
