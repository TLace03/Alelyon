//! Projects in the Chat and IDE (named projects, each of which may also be
//! bound to a folder).
//!
//! The Chats view picks a project: its chats are listed, and a new chat while it is picked joins it (the send names
//! it, and the core puts the new chat in it). A project's page edits its name, its instructions for the agent, its
//! folder, its reference files (text files on this PC that go with every turn of its chats) and the model a new chat
//! in it starts with, and archives it. The projects are lattice-core's (`projects.json`); this page only asks the
//! core. A project's folder opens only when asked (Open its folder), and as a typed path does: through the core's own
//! confirmation.

use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text_editor, text_input};
use iced::{Alignment, Element, Length, Task};

use crate::theme::{self, fonts};
use lattice_core::projects::{Change, MAX_FILES, Project};

use super::{IdeMsg, TabKind};
use crate::lattice::{Msg, STOPPED, State, on};
use crate::ui::{self, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

/// A project's page, being edited.
#[derive(Debug)]
pub struct Edit {
    pub id: String,
    pub name: String,
    pub instructions: text_editor::Content,
    pub folder: String,
    pub files: Vec<String>,
    /// The model a new chat in it starts with (a choice's id).
    pub choice: Option<String>,
    /// The path typed to add a reference file.
    pub adding: String,
    pub saving: bool,
    pub error: Option<String>,
}

/// The projects' state.
#[derive(Debug, Default)]
pub struct ProjectsState {
    /// What the core listed last.
    pub list: Option<Vec<Project>>,
    pub edit: Option<Edit>,
    /// The last action's sentence, and whether it warns.
    pub said: Option<(String, bool)>,
    /// The Chats view lists the archived projects too.
    pub show_archived: bool,
}

impl ProjectsState {
    pub fn find(&self, id: &str) -> Option<&Project> {
        self.list.as_ref()?.iter().find(|project| project.id == id)
    }
}

#[derive(Debug, Clone)]
pub enum ProjectsMsg {
    Read(Option<Vec<Project>>),
    /// Pick a project for the Chats view (`None`: every chat).
    Pick(Option<String>),
    Create,
    Created(Result<Project, String>),
    /// Open a project's page.
    Open(String),
    Name(String),
    Instructions(text_editor::Action),
    Folder(String),
    /// The page's folder is the one the editor has open.
    UseOpenFolder,
    Adding(String),
    Add,
    Remove(usize),
    Choice(Option<String>),
    Save,
    Saved(Result<Project, String>),
    Archive(String, bool),
    Archived(Result<Project, String>),
    /// Open the project's folder in the editor (through the core's confirmation).
    OpenFolder(String),
    /// Put the open chat in the picked project (`true`), or take it out of it.
    AssignOpen(bool),
    Assigned(Result<(), String>),
    ShowArchived(bool),
}

fn go(msg: ProjectsMsg) -> Msg {
    Msg::Ide(IdeMsg::Projects(msg))
}

/// Two spellings of one folder: the same once case, separators and a trailing separator are set aside.
pub fn same_folder(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.trim().trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

impl State {
    pub(in crate::lattice) fn projects(&mut self, msg: ProjectsMsg) -> Task<Msg> {
        match msg {
            ProjectsMsg::Read(list) => {
                if let Some(list) = list {
                    // A picked project archived elsewhere is no longer picked.
                    if let Some(id) = &self.ide.project
                        && !list.iter().any(|project| project.id == *id && !project.archived)
                    {
                        self.ide.project = None;
                    }
                    self.ide.projects.list = Some(list);
                }
            }
            ProjectsMsg::Pick(None) => self.ide.project = None,
            ProjectsMsg::Pick(Some(id)) => {
                let Some(project) = self.ide.projects.find(&id) else { return Task::none() };
                // An archived project is not picked: its page opens.
                if project.archived {
                    return self.projects(ProjectsMsg::Open(id));
                }
                // Its model, when it names one that is ready.
                if let Some(choice) = &project.choice
                    && self.choices.iter().any(|c| c.id == *choice && c.ready)
                {
                    self.choice = choice.clone();
                }
                self.ide.project = Some(id);
            }
            ProjectsMsg::Create => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                return Task::perform(
                    on(&services, async move { chat.project_create("New project".to_string()).await.map_err(|r| r.message) }),
                    |r| go(ProjectsMsg::Created(r.unwrap_or_else(|| Err(STOPPED.to_string())))),
                );
            }
            ProjectsMsg::Created(result) => match result {
                Ok(project) => {
                    let id = project.id.clone();
                    self.ide.projects.list.get_or_insert_with(Vec::new).push(project);
                    self.ide.project = Some(id.clone());
                    self.ide.projects.said = Some(("A new project: name it, and give it what its chats should know.".to_string(), false));
                    let open = self.projects(ProjectsMsg::Open(id));
                    return Task::batch([open, self.read_projects()]);
                }
                Err(why) => self.problem = Some(why),
            },
            ProjectsMsg::Open(id) => {
                let Some(project) = self.ide.projects.find(&id) else { return Task::none() };
                let name = project.name.clone();
                // A page already being edited keeps its edits.
                if self.ide.projects.edit.as_ref().is_none_or(|edit| edit.id != id) {
                    self.ide.projects.edit = Some(Edit {
                        id: project.id.clone(),
                        name: project.name.clone(),
                        instructions: text_editor::Content::with_text(&project.instructions),
                        folder: project.folder.clone().unwrap_or_default(),
                        files: project.files.clone(),
                        choice: project.choice.clone(),
                        adding: String::new(),
                        saving: false,
                        error: None,
                    });
                }
                let existing = self.ide.tabs.iter().find(|t| matches!(&t.kind, TabKind::Project { id: open, .. } if *open == id)).map(|t| t.id);
                let tab = match existing {
                    Some(tab) => tab,
                    None => self.ide.push(TabKind::Project { id, name }),
                };
                self.ide.active = Some(tab);
            }
            ProjectsMsg::Name(name) => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    edit.name = name;
                }
            }
            ProjectsMsg::Instructions(action) => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    edit.instructions.perform(action);
                }
            }
            ProjectsMsg::Folder(folder) => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    edit.folder = folder;
                }
            }
            ProjectsMsg::UseOpenFolder => {
                let open = self.ide.folder.as_ref().map(|f| f.shown_path());
                if let (Some(edit), Some(open)) = (&mut self.ide.projects.edit, open) {
                    edit.folder = open;
                }
            }
            ProjectsMsg::Adding(path) => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    edit.adding = path;
                }
            }
            ProjectsMsg::Add => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    let path = edit.adding.trim().trim_matches('"').to_string();
                    if !path.is_empty() && !edit.files.contains(&path) && edit.files.len() < MAX_FILES {
                        edit.files.push(path);
                        edit.adding.clear();
                    }
                }
            }
            ProjectsMsg::Remove(at) => {
                if let Some(edit) = &mut self.ide.projects.edit
                    && at < edit.files.len()
                {
                    edit.files.remove(at);
                }
            }
            ProjectsMsg::Choice(choice) => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    edit.choice = choice;
                }
            }
            ProjectsMsg::Save => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let Some(edit) = &mut self.ide.projects.edit else { return Task::none() };
                if edit.saving {
                    return Task::none();
                }
                edit.saving = true;
                edit.error = None;
                self.ide.projects.said = None;
                let folder = edit.folder.trim().trim_matches('"').to_string();
                let change = Change {
                    name: Some(edit.name.clone()),
                    instructions: Some(edit.instructions.text().trim_end().to_string()),
                    folder: Some((!folder.is_empty()).then_some(folder)),
                    files: Some(edit.files.clone()),
                    choice: Some(edit.choice.clone()),
                    archived: None,
                };
                let (chat, id) = (services.chat.clone(), edit.id.clone());
                return Task::perform(
                    on(&services, async move { chat.project_change(id, change).await.map_err(|r| r.message) }),
                    |r| go(ProjectsMsg::Saved(r.unwrap_or_else(|| Err(STOPPED.to_string())))),
                );
            }
            ProjectsMsg::Saved(result) => {
                if let Some(edit) = &mut self.ide.projects.edit {
                    edit.saving = false;
                }
                match result {
                    Ok(project) => {
                        for tab in &mut self.ide.tabs {
                            if let TabKind::Project { id, name } = &mut tab.kind
                                && *id == project.id
                            {
                                *name = project.name.clone();
                            }
                        }
                        self.ide.projects.said = Some(("Saved. Its chats use it from their next turn.".to_string(), false));
                    }
                    Err(why) => {
                        if let Some(edit) = &mut self.ide.projects.edit {
                            edit.error = Some(why);
                        }
                    }
                }
                return self.read_projects();
            }
            ProjectsMsg::Archive(id, archived) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                let change = Change { archived: Some(archived), ..Change::default() };
                return Task::perform(
                    on(&services, async move { chat.project_change(id, change).await.map_err(|r| r.message) }),
                    |r| go(ProjectsMsg::Archived(r.unwrap_or_else(|| Err(STOPPED.to_string())))),
                );
            }
            ProjectsMsg::Archived(result) => {
                self.ide.projects.said = Some(match result {
                    Ok(project) if project.archived => {
                        if self.ide.project.as_deref() == Some(project.id.as_str()) {
                            self.ide.project = None;
                        }
                        ("Archived. Its chats are kept, under Every chat; it can be brought back here.".to_string(), false)
                    }
                    Ok(_) => ("It is back in the Chats view.".to_string(), false),
                    Err(why) => (why, true),
                });
                return self.read_projects();
            }
            ProjectsMsg::OpenFolder(folder) => {
                if self.ide.opening {
                    return Task::none();
                }
                self.ide.folder_path = folder.clone();
                // A path from the project's page is a typed path: the core asks before the chat may use it.
                return self.ask_open_folder(folder, false);
            }
            ProjectsMsg::AssignOpen(into) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let (Some(project), Some(open)) = (self.ide.project.clone(), self.open.as_ref().map(|c| c.id().to_string())) else {
                    return Task::none();
                };
                let chat = services.chat.clone();
                let project = into.then_some(project);
                return Task::perform(
                    on(&services, async move { chat.project_assign(open, project).await.map_err(|r| r.message) }),
                    |r| go(ProjectsMsg::Assigned(r.unwrap_or_else(|| Err(STOPPED.to_string())))),
                );
            }
            ProjectsMsg::Assigned(result) => {
                if let Err(why) = result {
                    self.problem = Some(why);
                }
                return self.read_projects();
            }
            ProjectsMsg::ShowArchived(show) => self.ide.projects.show_archived = show,
        }
        Task::none()
    }

    /// A debug build's photograph (`CENTCOM_LATTICE_PROJECT=<id>`): the Chats view with that project picked, and its
    /// page open. A release build never reads this.
    #[cfg(debug_assertions)]
    pub(in crate::lattice) fn photograph_project(&mut self) -> Task<Msg> {
        let Some(id) = std::env::var("CENTCOM_LATTICE_PROJECT").ok() else { return Task::none() };
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.side = super::Side::Chats;
        self.ide.side_open = true;
        let chat = services.chat.clone();
        let read = Task::perform(on(&services, async move { chat.projects().await.ok() }), |list| go(ProjectsMsg::Read(list.flatten())));
        read.chain(Task::done(go(ProjectsMsg::Pick(Some(id.clone()))))).chain(Task::done(go(ProjectsMsg::Open(id))))
    }

    /// Read the projects again.
    pub(in crate::lattice) fn read_projects(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        if self.fixture {
            return Task::none();
        }
        let chat = services.chat.clone();
        Task::perform(on(&services, async move { chat.projects().await.ok() }), |list| go(ProjectsMsg::Read(list.flatten())))
    }
}

// ------------------------------------------------------------------------------------------------- views

fn chip<'a>(words: &str, selected: bool, faint: bool, msg: Msg) -> El<'a> {
    let color = if selected {
        theme::GOLD
    } else if faint {
        theme::TEXT_FAINT
    } else {
        theme::TEXT_DIM
    };
    button(label(ui::cut(words, 18), 11.5, color)).padding([2.0, 8.0]).style(theme::segment_button(selected)).on_press(msg).into()
}

/// The Chats view's projects: every chat, or one project's; a new project; for the picked one, its folder, its
/// settings, and the open chat put in it.
pub fn picker<'a>(state: &'a State) -> El<'a> {
    let projects = &state.ide.projects;
    let picked = state.ide.project.as_deref();
    let mut chips = Row::new().spacing(4).align_y(Alignment::Center);
    chips = chips.push(chip("Every chat", picked.is_none(), false, go(ProjectsMsg::Pick(None))));
    let list = projects.list.as_deref().unwrap_or_default();
    for project in list.iter().filter(|project| !project.archived) {
        chips = chips.push(chip(&project.name, picked == Some(project.id.as_str()), false, go(ProjectsMsg::Pick(Some(project.id.clone())))));
    }
    let archived = list.iter().filter(|project| project.archived).count();
    if archived > 0 {
        let words = if projects.show_archived { "Hide archived".to_string() } else { format!("{archived} archived") };
        chips = chips.push(chip(&words, false, true, go(ProjectsMsg::ShowArchived(!projects.show_archived))));
        if projects.show_archived {
            for project in list.iter().filter(|project| project.archived) {
                chips = chips.push(chip(&project.name, false, true, go(ProjectsMsg::Open(project.id.clone()))));
            }
        }
    }
    chips = chips.push(
        button(label("+ Project", 11.5, theme::TEXT_DIM)).padding([2.0, 8.0]).style(theme::ghost_button).on_press(go(ProjectsMsg::Create)),
    );
    // The chips wrap onto more lines in a narrow side bar, so none is out of sight.
    let mut col = Column::new().spacing(4).push(chips.wrap().vertical_spacing(4));
    if let Some(project) = picked.and_then(|id| projects.find(id)) {
        let mut line = Row::new().spacing(4).align_y(Alignment::Center);
        let about = match &project.folder {
            Some(folder) => ui::cut(super::file_name(&folder.replace('\\', "/")), 22),
            None => "No folder".to_string(),
        };
        line = line.push(label(about, 11.0, theme::TEXT_FAINT).width(Length::Fill));
        let small = |words: &'a str, msg: Msg| -> El<'a> {
            button(label(words, 11.0, theme::GOLD)).padding([1.0, 5.0]).style(theme::ghost_button).on_press(msg).into()
        };
        if let Some(folder) = &project.folder
            && !state.ide.folder.as_ref().is_some_and(|open| same_folder(&open.shown_path(), folder))
        {
            line = line.push(small("Open its folder", go(ProjectsMsg::OpenFolder(folder.clone()))));
        }
        if let Some(open) = state.conversation() {
            line = line.push(if project.chats.iter().any(|chat| chat == open.id()) {
                small("Take the open chat out", go(ProjectsMsg::AssignOpen(false)))
            } else {
                small("Add the open chat", go(ProjectsMsg::AssignOpen(true)))
            });
        }
        line = line.push(small("Settings", go(ProjectsMsg::Open(project.id.clone()))));
        col = col.push(line);
    }
    container(col).padding([0.0, 10.0]).into()
}

/// The name of the project a chat is in, if any.
pub fn project_of<'a>(state: &'a State, conversation: &str) -> Option<&'a str> {
    let list = state.ide.projects.list.as_ref()?;
    list.iter().find(|project| project.chats.iter().any(|chat| chat == conversation)).map(|project| project.name.as_str())
}

/// Whether a chat is shown under the picked project.
pub fn shows(state: &State, conversation: &str) -> bool {
    match state.ide.project.as_deref() {
        None => true,
        Some(id) => state.ide.projects.find(id).is_some_and(|project| project.chats.iter().any(|chat| chat == conversation)),
    }
}

/// A project's page.
pub fn page<'a>(state: &'a State, id: &'a str) -> El<'a> {
    let projects = &state.ide.projects;
    let (Some(edit), Some(project)) = (projects.edit.as_ref().filter(|edit| edit.id == id), projects.find(id)) else {
        return container(note("This project is not here now.")).padding(24).into();
    };
    let mut col = Column::new().spacing(10).padding([18.0, 24.0]).max_width(900);
    col = col.push(
        column![
            strong(if project.archived { "Project (archived)" } else { "Project" }, 20.0, theme::TEXT),
            label(
                "A project groups chats. Its instructions and reference files go with every turn of its chats, before the folder's rules, and cannot let the agent do anything it otherwise could not. Pick it in Chats: a new chat then joins it.",
                12.5,
                theme::TEXT_DIM,
            ),
        ]
        .spacing(4),
    );
    if let Some((words, warn)) = &projects.said {
        col = col.push(ui::notice(words.clone(), if *warn { theme::CAUTION } else { theme::POSITIVE }));
    }
    col = col.push(label("Name", 12.0, theme::TEXT_FAINT));
    col = col.push(text_input("Its name", &edit.name).on_input(|t| go(ProjectsMsg::Name(t))).padding([6.0, 8.0]).size(13.0).style(ui::input_style));
    col = col.push(label("Instructions for the agent", 12.0, theme::TEXT_FAINT));
    col = col.push(
        text_editor(&edit.instructions)
            .on_action(|a| go(ProjectsMsg::Instructions(a)))
            .font(fonts().ui)
            .size(13.0)
            .height(Length::Fixed(150.0))
            .padding(8)
            .style(theme::editor),
    );
    col = col.push(label("Folder (optional; Open its folder in Chats opens it, after the usual question)", 12.0, theme::TEXT_FAINT));
    col = col.push(
        row![
            text_input(r"D:\Projects\demo", &edit.folder).on_input(|t| go(ProjectsMsg::Folder(t))).padding([6.0, 8.0]).size(12.5).style(ui::input_style),
            ui::secondary("Use the open folder", state.ide.folder.is_some().then(|| go(ProjectsMsg::UseOpenFolder))),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    col = col.push(label(
        format!("Reference files (text files on this PC, at most {MAX_FILES}; up to 128 KiB in all go with each turn)"),
        12.0,
        theme::TEXT_FAINT,
    ));
    let mut files = Column::new().spacing(4);
    if edit.files.is_empty() {
        files = files.push(note("None yet."));
    }
    for (at, file) in edit.files.iter().enumerate() {
        files = files.push(
            row![
                mono(ui::cut(file, 90), 11.5, theme::TEXT_DIM).width(Length::Fill),
                button(label("Remove", 11.0, theme::TEXT_DIM)).padding([1.0, 6.0]).style(theme::ghost_button).on_press(go(ProjectsMsg::Remove(at))),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    col = col.push(container(files).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
    col = col.push(
        row![
            text_input(r"D:\Projects\notes\brief.md", &edit.adding)
                .on_input(|t| go(ProjectsMsg::Adding(t)))
                .on_submit(go(ProjectsMsg::Add))
                .padding([6.0, 8.0])
                .size(12.5)
                .style(ui::input_style),
            ui::secondary("Add the file", (!edit.adding.trim().is_empty() && edit.files.len() < MAX_FILES).then(|| go(ProjectsMsg::Add))),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    col = col.push(label("Model for a new chat", 12.0, theme::TEXT_FAINT));
    let mut models = Row::new().spacing(4).align_y(Alignment::Center);
    models = models.push(chip("The one picked", edit.choice.is_none(), false, go(ProjectsMsg::Choice(None))));
    for choice in state.choices.iter().filter(|c| c.ready) {
        let on = edit.choice.as_deref() == Some(choice.id.as_str());
        models = models.push(chip(&choice.label, on, false, go(ProjectsMsg::Choice(Some(choice.id.clone())))));
    }
    if let Some(named) = &edit.choice
        && !state.choices.iter().any(|c| c.id == *named && c.ready)
    {
        models = models.push(label(format!("{} is not ready here", ui::cut(named, 24)), 11.0, theme::CAUTION));
    }
    col = col.push(scrollable(models).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(2).scroller_width(2))));
    if let Some(why) = &edit.error {
        col = col.push(ui::notice(why.clone(), theme::CAUTION));
    }
    let mut actions = Row::new().spacing(8).align_y(Alignment::Center);
    actions = actions.push(ui::primary(if edit.saving { "Saving\u{2026}" } else { "Save" }, (!edit.saving).then(|| go(ProjectsMsg::Save))));
    actions = actions.push(ui::secondary(
        if project.archived { "Bring it back" } else { "Archive" },
        Some(go(ProjectsMsg::Archive(project.id.clone(), !project.archived))),
    ));
    actions = actions.push(space().width(Length::Fill));
    actions = actions.push(label(format!("{} chat(s)", project.chats.len()), 11.5, theme::TEXT_FAINT));
    col = col.push(actions);
    scrollable(container(col).center_x(Length::Fill)).height(Length::Fill).style(theme::scrollbars).into()
}

#[cfg(test)]
mod tests {
    use lattice_protocol::Locality;
    use lattice_protocol::chat::ChatChoice;
    use lattice_protocol::conversation::{ConversationSummary, Mode, Origin, Snapshot};

    use super::*;

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: id.into(),
            name: name.into(),
            instructions: String::new(),
            folder: None,
            files: vec![],
            choice: None,
            chats: vec![],
            created: 1.0,
            updated: 1.0,
            archived: false,
        }
    }

    fn listed(state: &mut State, list: Vec<Project>) {
        let _ = state.projects(ProjectsMsg::Read(Some(list)));
    }

    fn choice(id: &str, ready: bool) -> ChatChoice {
        ChatChoice { id: id.into(), label: id.into(), detail: String::new(), locality: Locality::Local, ready, refusal: None }
    }

    fn open_chat(state: &mut State, id: &str) {
        let conversation = ConversationSummary {
            id: id.into(),
            title: "t".into(),
            created: 1.0,
            updated: 1.0,
            turns: 0,
            pinned_provider: String::new(),
            workspace: None,
            mode: Mode::Agent,
            origin: Origin::Native,
            running: false,
            needs_you: false,
        };
        let snapshot = Snapshot { conversation, turns: vec![], events: vec![], last_seq: 0, queued: vec![], answering_elsewhere: false };
        state.open = Some(crate::lattice::chat::Conversation::from_snapshot(snapshot));
    }

    /// A send with no chat open names the picked project (the core puts the new chat in it); a send to an open chat
    /// names none, so a chat keeps its own project.
    #[test]
    fn a_new_chat_joins_the_picked_project_and_an_open_one_keeps_its_own() {
        let mut state = State::new();
        listed(&mut state, vec![project("p_a", "Launch")]);
        assert_eq!(state.send_request("hi".into(), Mode::Agent, None).project, None, "no project is picked");
        let _ = state.projects(ProjectsMsg::Pick(Some("p_a".into())));
        assert_eq!(state.send_request("hi".into(), Mode::Agent, None).project.as_deref(), Some("p_a"));
        open_chat(&mut state, "c1");
        let request = state.send_request("hi".into(), Mode::Agent, None);
        assert_eq!((request.conversation.as_deref(), request.project), (Some("c1"), None));
        let _ = state.projects(ProjectsMsg::Pick(None));
        assert_eq!(state.ide.project, None);
    }

    /// Picked, the Chats view lists that project's chats only; a project not listed (yet) shows none.
    #[test]
    fn the_chats_view_shows_the_picked_projects_chats_only() {
        let mut state = State::new();
        let mut launch = project("p_a", "Launch");
        launch.chats = vec!["c1".into()];
        listed(&mut state, vec![launch]);
        assert!(shows(&state, "c1") && shows(&state, "c2"), "every chat");
        assert_eq!((project_of(&state, "c1"), project_of(&state, "c2")), (Some("Launch"), None));
        let _ = state.projects(ProjectsMsg::Pick(Some("p_a".into())));
        assert!(shows(&state, "c1") && !shows(&state, "c2"));
        let _ = state.projects(ProjectsMsg::Pick(Some("p_nothere".into())));
        assert_eq!(state.ide.project.as_deref(), Some("p_a"), "a project not listed is not picked");
    }

    /// An archived project opens its page rather than being picked, and a picked project archived elsewhere is no
    /// longer picked once the projects are read again.
    #[test]
    fn an_archived_project_opens_its_page_and_is_never_left_picked() {
        let mut state = State::new();
        let mut old = project("p_old", "Old");
        old.archived = true;
        listed(&mut state, vec![project("p_a", "Launch"), old.clone()]);
        let _ = state.projects(ProjectsMsg::Pick(Some("p_old".into())));
        assert_eq!(state.ide.project, None, "an archived project is not picked");
        let tab = state.ide.active_tab().unwrap();
        assert!(matches!(&tab.kind, TabKind::Project { id, .. } if id == "p_old"));
        assert_eq!(tab.title(), "Project: Old");
        let _ = state.projects(ProjectsMsg::Pick(Some("p_a".into())));
        assert_eq!(state.ide.project.as_deref(), Some("p_a"));
        let mut archived = project("p_a", "Launch");
        archived.archived = true;
        listed(&mut state, vec![archived, old]);
        assert_eq!(state.ide.project, None);
    }

    /// Picking a project takes the model it names only when that model is ready here.
    #[test]
    fn picking_a_project_takes_its_model_only_when_it_is_ready() {
        let mut state = State::new();
        state.choices = vec![choice("local", true), choice("endpoint:hosted", true), choice("cloud", false)];
        state.choice = "local".into();
        let mut hosted = project("p_h", "Hosted");
        hosted.choice = Some("endpoint:hosted".into());
        let mut cloud = project("p_c", "Cloud");
        cloud.choice = Some("cloud".into());
        listed(&mut state, vec![hosted, cloud]);
        let _ = state.projects(ProjectsMsg::Pick(Some("p_c".into())));
        assert_eq!(state.choice, "local", "a model that is not ready was taken");
        let _ = state.projects(ProjectsMsg::Pick(Some("p_h".into())));
        assert_eq!(state.choice, "endpoint:hosted");
    }

    /// The page keeps its edits when it is opened again, adds each file once (quotes taken off, at most
    /// [`MAX_FILES`]), and a save renames its tab or shows why it was refused.
    #[test]
    fn the_page_keeps_its_edits_adds_each_file_once_and_a_save_renames_its_tab() {
        let mut state = State::new();
        listed(&mut state, vec![project("p_a", "Launch")]);
        let _ = state.projects(ProjectsMsg::Open("p_a".into()));
        let _ = state.projects(ProjectsMsg::Name("Launch plan".into()));
        for typed in [r#""C:\notes\a.md""#, r"C:\notes\a.md", "  "] {
            let _ = state.projects(ProjectsMsg::Adding(typed.into()));
            let _ = state.projects(ProjectsMsg::Add);
        }
        let _ = state.projects(ProjectsMsg::Open("p_a".into()));
        let edit = state.ide.projects.edit.as_ref().unwrap();
        assert_eq!((edit.name.as_str(), edit.files.clone()), ("Launch plan", vec![r"C:\notes\a.md".to_string()]));
        assert_eq!(state.ide.tabs.iter().filter(|t| matches!(t.kind, TabKind::Project { .. })).count(), 1);
        for n in 0..MAX_FILES + 2 {
            let _ = state.projects(ProjectsMsg::Adding(format!(r"C:\notes\{n}.txt")));
            let _ = state.projects(ProjectsMsg::Add);
        }
        assert_eq!(state.ide.projects.edit.as_ref().unwrap().files.len(), MAX_FILES);
        let _ = state.projects(ProjectsMsg::Saved(Ok(project("p_a", "Launch plan"))));
        assert_eq!(state.ide.active_tab().unwrap().title(), "Project: Launch plan");
        assert!(state.ide.projects.said.as_ref().is_some_and(|(_, warn)| !warn));
        let _ = state.projects(ProjectsMsg::Saved(Err("A project's name is at most 80 characters.".into())));
        assert!(state.ide.projects.edit.as_ref().unwrap().error.is_some());
    }

    /// One folder however it is spelt; another folder is another.
    #[test]
    fn a_folder_is_the_same_however_it_is_spelt() {
        assert!(same_folder(r"D:\Work\Proj", "d:/work/proj/"));
        assert!(same_folder(r" C:\x\ ", r"C:\x"));
        assert!(!same_folder(r"C:\x\proj", r"C:\x\proj2"));
        assert!(!same_folder(r"C:\x", r"D:\x"));
    }
}
