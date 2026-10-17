//! Groups, in the Chats side: the work split by what is being done. Each group's session is one continuing stream of
//! its topics, with its tasks beside it; opening a group shows its session. No group is a fresh panel: a message sent
//! there goes to the group already doing that work, the panel following it to that chat, or starts a group; the reader
//! renames, merges, moves a topic and archives. The groups are
//! lattice-core's (`convo::groups`); this side only asks the core. Brainstorm, above the groups, opens the project's
//! brainstorm chat: its messages go to it, asking only, and an idea it offers is a card that sends to a group's session.

use iced::widget::{Column, Row, button, container, row, space, text, text_input};
use iced::{Alignment, Element, Length, Padding, Task};
use lattice_core::convo::groups::{Brainstorm, Group, NEW, Placement};
use lattice_protocol::conversation::ConversationSummary;

use super::IdeMsg;
use crate::lattice::{Msg, STOPPED, State, on};
use crate::theme;
use crate::ui::{self, label, note};

type El<'a> = Element<'a, Msg>;

/// The groups' state.
#[derive(Debug, Default)]
pub struct GroupsState {
    /// Each group, archived ones included, with its name as shown.
    pub list: Vec<(Group, String)>,
    /// The grouping of earlier work waits for the reader's review.
    pub review: bool,
    /// The group whose session is shown (`None`: no group; a message is put in the group it matches).
    pub open: Option<String>,
    /// The panel was left empty on purpose: No group picked (a fresh panel whose next message goes to the group already
    /// doing that work, the panel following it there, or starts a group), or a brainstorm chat not yet begun. Until a
    /// chat opens, the newest topic is not opened by itself.
    pub fresh: bool,
    /// Where the message being sent goes, as the core placed it.
    pub placing: Option<Placement>,
    /// A new group's name, being typed.
    pub naming: Option<String>,
    /// A group being renamed, and the name typed.
    pub renaming: Option<(String, String)>,
    /// A group to be merged into another, being picked.
    pub merging: Option<String>,
    /// A topic to be moved to another group, being picked.
    pub moving: Option<String>,
    pub show_archived: bool,
    /// The last action's refusal.
    pub said: Option<String>,
    /// Each project's brainstorm chat.
    pub brainstorms: Vec<Brainstorm>,
    /// The brainstorm chat is the one shown.
    pub brainstorming: bool,
    /// An idea being sent to a group, as its card names the group.
    pub sending_idea: Option<String>,
    /// The board of the project worked in (`super::board`), and whether it is shown in the editor's place.
    pub board: Vec<lattice_core::convo::board::Card>,
    pub board_shown: bool,
    /// A group's plan, being edited (`super::plan`).
    pub plan_edit: Option<super::plan::PlanEdit>,
}

impl GroupsState {
    pub fn find(&self, id: &str) -> Option<&Group> {
        self.list.iter().map(|(g, _)| g).find(|g| g.id == id)
    }

    pub fn name(&self, id: &str) -> Option<&str> {
        self.list.iter().find(|(g, _)| g.id == id).map(|(_, name)| name.as_str())
    }

    /// The brainstorm chat of `project`.
    pub fn brainstorm_of(&self, project: Option<&str>) -> Option<&str> {
        self.brainstorms.iter().find(|b| b.project.as_deref() == project).map(|b| b.conversation.as_str())
    }

    pub fn is_brainstorm(&self, conversation: &str) -> bool {
        self.brainstorms.iter().any(|b| b.conversation == conversation)
    }

    /// The open group.
    pub fn shown(&self) -> Option<&Group> {
        self.open.as_deref().and_then(|id| self.find(id))
    }
}

#[derive(Debug, Clone)]
pub enum GroupsMsg {
    Read((Vec<(Group, String)>, bool), Vec<Brainstorm>, Vec<lattice_core::convo::board::Card>),
    /// Show the board in the editor's place, or not.
    ShowBoard(bool),
    /// Move a group one place up (`true`) or down in priority.
    Raise(String, bool),
    /// Move a group's card to a column (`None`: back to the one its work gives).
    MoveCard(String, Option<lattice_core::convo::groups::Status>),
    /// The plan editor's messages (`super::plan`).
    Plan(super::plan::PlanMsg),
    /// Show the project's brainstorm chat.
    Brainstorm,
    /// Show a group's session (`None`: no group).
    Open(Option<String>),
    New,
    Naming(String),
    Create,
    Rename(String),
    Renaming(String),
    SaveName,
    Merge(String),
    MergeInto(String),
    /// Move a topic: pick where.
    Move(String),
    /// To this group (`None`: a new group of its own).
    MoveTo(Option<String>),
    Archive(String, bool),
    /// Keep the grouping of earlier work.
    Keep,
    Cancel,
    ShowArchived(bool),
    /// The message sent was put in this group.
    Placed(Result<Group, String>),
    Done(Result<(), String>),
}

pub fn go(msg: GroupsMsg) -> Msg {
    Msg::Ide(IdeMsg::Groups(msg))
}

impl State {
    /// Read the groups (earlier work is grouped first, once).
    pub(in crate::lattice) fn read_groups(&self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let chat = services.chat.clone();
        let project = self.ide.project.clone();
        Task::perform(
            on(&services, async move {
                let _ = chat.group_existing();
                (chat.groups(), chat.brainstorms(), chat.board(project.as_deref()))
            }),
            |r| {
                let (groups, brainstorms, board) = r.unwrap_or_default();
                go(GroupsMsg::Read(groups, brainstorms, board))
            },
        )
    }

    /// A blocking call to the core's groups, then the groups read again.
    fn on_groups<T: Send + 'static>(
        &self,
        work: impl FnOnce(&lattice_core::convo::agent::AgentChat) -> Result<T, String> + Send + 'static,
        then: impl Fn(Result<T, String>) -> GroupsMsg + Send + 'static,
    ) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let chat = services.chat.clone();
        Task::perform(on(&services, async move { work(&chat) }), move |r| go(then(r.unwrap_or_else(|| Err(STOPPED.to_string())))))
    }

    /// The topic `topic` the composer's message went to: put in the group it was placed in.
    pub(in crate::lattice) fn place_sent(&mut self, topic: String) -> Task<Msg> {
        let Some(into) = self.ide.groups.placing.take() else { return Task::none() };
        let project = self.ide.project.clone();
        self.on_groups(move |chat| chat.group_add(&topic, &into, project.as_deref()), GroupsMsg::Placed)
    }

    pub(in crate::lattice) fn groups(&mut self, msg: GroupsMsg) -> Task<Msg> {
        let groups = &mut self.ide.groups;
        match msg {
            GroupsMsg::Plan(msg) => return self.plan(msg),
            GroupsMsg::ShowBoard(on) => {
                groups.board_shown = on;
                if on {
                    self.ide.grid.shown = false;
                    return self.read_groups();
                }
            }
            GroupsMsg::Raise(id, up) => return self.on_groups(move |chat| chat.group_raise(&id, up), GroupsMsg::Done),
            GroupsMsg::MoveCard(id, status) => {
                return self.on_groups(move |chat| chat.group_move(&id, status), GroupsMsg::Done);
            }
            GroupsMsg::Read((list, review), brainstorms, board) => {
                groups.board = board;
                if groups.open.as_deref().is_some_and(|id| !list.iter().any(|(g, _)| g.id == id && !g.archived)) {
                    groups.open = None;
                }
                groups.list = list;
                groups.review = review;
                groups.brainstorms = brainstorms;
            }
            GroupsMsg::Brainstorm => {
                groups.brainstorming = true;
                groups.open = None;
                groups.moving = None;
                groups.fresh = false;
                self.mode = lattice_protocol::conversation::Mode::Ask;
                // Its chat, or a new one begun by the first message.
                match self.ide.groups.brainstorm_of(self.ide.project.as_deref()).map(str::to_owned) {
                    Some(id) if self.open.as_ref().is_none_or(|c| c.id() != id) => return self.update(Msg::Open(id)),
                    Some(_) => {}
                    None => {
                        self.ide.groups.fresh = true;
                        self.start_new_chat();
                    }
                }
            }
            GroupsMsg::Open(None) => {
                groups.open = None;
                groups.moving = None;
                groups.brainstorming = false;
                // No group: a fresh panel, the reader's own desk. Its next message goes to the group already doing that
                // work and the panel follows it there; work no group does starts a group. A draft in the composer stays.
                groups.fresh = true;
                let draft = self.composer.text();
                let next = self.update(Msg::New);
                if !draft.trim().is_empty() {
                    self.composer = iced::widget::text_editor::Content::with_text(draft.trim_end_matches('\n'));
                }
                return next;
            }
            GroupsMsg::Open(Some(id)) => {
                let Some(group) = groups.find(&id) else { return Task::none() };
                let project = group.project.clone();
                groups.open = Some(id);
                groups.moving = None;
                groups.brainstorming = false;
                groups.fresh = false;
                // Its session: the project it is in, and its newest topic.
                return self.update(Msg::WorkIn(project));
            }
            GroupsMsg::New => groups.naming = Some(String::new()),
            GroupsMsg::Naming(name) => groups.naming = Some(name),
            GroupsMsg::Create => {
                let Some(name) = groups.naming.take() else { return Task::none() };
                let project = self.ide.project.clone();
                return self.on_groups(
                    move |chat| chat.group_create(&name, project.as_deref()).map(|g| g.id),
                    |r| match r {
                        Ok(id) => GroupsMsg::Open(Some(id)),
                        Err(why) => GroupsMsg::Done(Err(why)),
                    },
                )
                .chain(self.read_groups());
            }
            GroupsMsg::Rename(id) => {
                let name = groups.name(&id).unwrap_or_default().to_string();
                groups.renaming = Some((id, name));
            }
            GroupsMsg::Renaming(name) => {
                if let Some((_, typed)) = &mut groups.renaming {
                    *typed = name;
                }
            }
            GroupsMsg::SaveName => {
                let Some((id, name)) = groups.renaming.take() else { return Task::none() };
                return self.on_groups(move |chat| chat.group_rename(&id, &name).map(drop), GroupsMsg::Done);
            }
            GroupsMsg::Merge(id) => groups.merging = Some(id),
            GroupsMsg::MergeInto(into) => {
                let Some(from) = groups.merging.take() else { return Task::none() };
                if groups.open.as_deref() == Some(from.as_str()) {
                    groups.open = Some(into.clone());
                }
                return self.on_groups(move |chat| chat.group_merge(&from, &into).map(drop), GroupsMsg::Done);
            }
            GroupsMsg::Move(topic) => groups.moving = Some(topic),
            GroupsMsg::MoveTo(to) => {
                let Some(topic) = groups.moving.take() else { return Task::none() };
                let project = groups.find_of(&topic).and_then(|g| g.project.clone());
                let into = match to {
                    Some(id) => Placement::Group(id),
                    None => Placement::New,
                };
                return self.on_groups(move |chat| chat.group_add(&topic, &into, project.as_deref()).map(drop), GroupsMsg::Done);
            }
            GroupsMsg::Archive(id, archived) => {
                if archived && groups.open.as_deref() == Some(id.as_str()) {
                    groups.open = None;
                }
                return self.on_groups(move |chat| chat.group_archive(&id, archived).map(drop), GroupsMsg::Done);
            }
            GroupsMsg::Keep => return self.on_groups(|chat| chat.group_keep(), GroupsMsg::Done),
            GroupsMsg::Cancel => {
                groups.plan_edit = None;
                groups.naming = None;
                groups.renaming = None;
                groups.merging = None;
                groups.moving = None;
            }
            GroupsMsg::ShowArchived(on) => groups.show_archived = on,
            GroupsMsg::Placed(result) => match result {
                Ok(group) => {
                    groups.open = Some(group.id);
                    return self.read_groups();
                }
                Err(why) => groups.said = Some(why),
            },
            GroupsMsg::Done(result) => {
                groups.said = result.err();
                return self.read_groups();
            }
        }
        Task::none()
    }
}

impl GroupsState {
    /// The group `topic` is in.
    pub fn find_of(&self, topic: &str) -> Option<&Group> {
        self.list.iter().map(|(g, _)| g).find(|g| g.topics.iter().any(|t| t == topic))
    }
}

/// How an idea's card names where it goes: the group's name in quotes, or a new group.
pub fn idea_target(groups: &GroupsState, group: &str) -> String {
    if group == NEW {
        return "a new group".to_string();
    }
    match groups.name(group) {
        Some(name) => format!("\u{201c}{}\u{201d}", ui::cut(name, 40)),
        None => "its group".to_string(),
    }
}

/// What a group is doing, from its topics' rows: needs you, working, or quiet.
fn status(state: &State, rows: &[&ConversationSummary]) -> (&'static str, iced::Color) {
    if rows.iter().any(|r| r.needs_you || state.calling.contains(&r.id)) {
        ("needs you", theme::GOLD)
    } else if rows.iter().any(|r| r.running) {
        ("working", theme::GOLD_DIM)
    } else {
        ("quiet", theme::TEXT_FAINT)
    }
}

pub(super) fn small<'a>(words: &'a str, msg: Option<Msg>) -> El<'a> {
    let b = button(label(words, 11.0, theme::TEXT_DIM)).padding([2.0, 6.0]).style(theme::ghost_button);
    match msg {
        Some(m) => b.on_press(m).into(),
        None => b.into(),
    }
}

/// A section's head on the Chats side, in small faint capitals: the whole line folds the section to it or opens it
/// again (`key` in [`super::Ide::side_folds`]), the arrow saying which; `action` sits at its end.
pub(super) fn section_head<'a>(words: &str, key: &str, folded: bool, action: Option<El<'a>>) -> El<'a> {
    let head = button(
        row![
            label(if folded { "\u{25B8}" } else { "\u{25BE}" }, 11.0, theme::TEXT_FAINT),
            text(words.to_uppercase()).size(11).font(theme::fonts().ui_strong).color(theme::TEXT_FAINT),
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4.0, 6.0])
    .style(theme::ghost_button)
    .on_press(Msg::Ide(IdeMsg::SideFold(key.to_string())));
    let mut line = Row::new().spacing(4).align_y(Alignment::Center).push(head);
    if let Some(action) = action {
        line = line.push(action);
    }
    container(line).padding(Padding { top: 10.0, right: 10.0, bottom: 2.0, left: 6.0 }).into()
}

/// The groups at the top of the Chats side, under a head that folds them away: the review of earlier work's
/// grouping, No group, each group (the open one with its topics and tasks, and what can be done to it; another's
/// topics shown under it by its arrow), New group, and the archived ones.
pub fn section<'a>(state: &'a State, mut col: Column<'a, Msg>, phase: f32) -> Column<'a, Msg> {
    let groups = &state.ide.groups;
    let rows: &[ConversationSummary] = state.list.as_ref().map(|l| l.conversations.as_slice()).unwrap_or_default();
    let project = state.ide.project.as_deref();
    let in_project = |g: &Group| project.is_none() || g.project.as_deref() == project;
    let shown = groups.list.iter().filter(|(g, _)| !g.archived && in_project(g)).count();
    let folded = state.ide.side_folds.contains("groups");
    col = col.push(section_head(
        &format!("Groups \u{00B7} {shown}"),
        "groups",
        folded,
        Some(small("New group", Some(go(GroupsMsg::New)))),
    ));
    let pad = |e: El<'a>| container(e).padding(Padding { top: 2.0, right: 10.0, bottom: 4.0, left: 12.0 });
    if let Some(why) = &groups.said {
        col = col.push(pad(ui::notice(why.clone(), theme::CAUTION)));
    }
    if groups.review {
        let shown = groups.list.iter().filter(|(g, _)| !g.archived).count();
        col = col.push(pad(
            container(
                Column::new()
                    .spacing(6)
                    .push(label(
                        format!(
                            "Your earlier work was put in {shown} groups by what it is about. Look them over, move a topic or merge groups where they don't fit, then keep them."
                        ),
                        12.0,
                        theme::TEXT,
                    ))
                    .push(ui::primary("Keep these groups", Some(go(GroupsMsg::Keep)))),
            )
            .padding(10)
            .style(theme::card)
            .into(),
        ));
    }
    if let Some(name) = &groups.naming {
        col = col.push(pad(
            row![
                text_input("The group's name", name)
                    .on_input(|t| go(GroupsMsg::Naming(t)))
                    .on_submit(go(GroupsMsg::Create))
                    .padding([5.0, 8.0])
                    .size(12.5)
                    .style(ui::input_style),
                small("Make", Some(go(GroupsMsg::Create))),
                small("Cancel", Some(go(GroupsMsg::Cancel))),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
        ));
    }
    // Folded, the head alone (with what asks for the reader above: a refusal, the review, a name being typed).
    if folded {
        return col;
    }
    // The brainstorm chat, beside the groups.
    col = col.push(
        button(
            row![
                label("Brainstorm", 12.5, if groups.brainstorming { theme::GOLD } else { theme::TEXT_DIM }),
                space().width(Length::Fill),
                label("ideas, changes nothing", 10.5, theme::TEXT_FAINT),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([4.0, 10.0])
        .style(theme::list_row(groups.brainstorming))
        .on_press(go(GroupsMsg::Brainstorm)),
    );
    // No group: a message is put in the group it matches.
    let none_open = groups.open.is_none() && !groups.brainstorming;
    col = col.push(
        button(
            row![
                label("No group", 12.5, if none_open { theme::GOLD } else { theme::TEXT_DIM }),
                space().width(Length::Fill),
                label("work goes to its group", 10.5, theme::TEXT_FAINT),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([4.0, 10.0])
        .style(theme::list_row(none_open))
        .on_press(go(GroupsMsg::Open(None))),
    );
    for (group, name) in groups.list.iter().filter(|(g, _)| !g.archived && in_project(g)) {
        col = group_lines(state, group, name, rows, col, phase);
    }
    let archived: Vec<&(Group, String)> = groups.list.iter().filter(|(g, _)| g.archived && in_project(g)).collect();
    if !archived.is_empty() {
        let words = if groups.show_archived { "Hide archived groups".to_string() } else { format!("{} archived (never deleted)", archived.len()) };
        col = col.push(
            button(label(words, 11.0, theme::TEXT_FAINT))
                .padding(Padding { top: 3.0, bottom: 3.0, left: 12.0, right: 8.0 })
                .style(theme::ghost_button)
                .on_press(go(GroupsMsg::ShowArchived(!groups.show_archived))),
        );
        if groups.show_archived {
            for (group, name) in archived {
                col = col.push(pad(
                    row![
                        container(label(ui::cut(name, 30), 12.0, theme::TEXT_FAINT)).width(Length::Fill).clip(true),
                        small("Restore", Some(go(GroupsMsg::Archive(group.id.clone(), false)))),
                    ]
                    .align_y(Alignment::Center)
                    .into(),
                ));
            }
        }
    }
    col
}

/// A group's line; the open group's topics and tasks beneath it, and its actions.
fn group_lines<'a>(
    state: &'a State,
    group: &'a Group,
    name: &'a str,
    rows: &'a [ConversationSummary],
    mut col: Column<'a, Msg>,
    phase: f32,
) -> Column<'a, Msg> {
    let groups = &state.ide.groups;
    let open = groups.open.as_deref() == Some(group.id.as_str());
    let members: Vec<&ConversationSummary> = rows.iter().filter(|r| group.topics.contains(&r.id)).collect();
    let tasks = members.iter().filter(|r| state.is_task(&r.id)).count();
    let topics = members.len() - tasks;
    let (words, colour) = status(state, &members);
    // Its arrow shows its topics under it without opening it (the open group's always show).
    let key = format!("group:{}", group.id);
    let peeked = !open && state.ide.side_folds.contains(&key);
    let arrow = button(label(if open || peeked { "\u{25BE}" } else { "\u{25B8}" }, 11.0, theme::TEXT_FAINT))
        .padding([0.0, 2.0])
        .style(theme::ghost_button)
        .on_press_maybe((!open).then(|| Msg::Ide(IdeMsg::SideFold(key.clone()))));
    let mut line = Row::new().spacing(8).align_y(Alignment::Center).push(arrow);
    line = line.push(if words == "working" {
        crate::spinner::spinner(phase, 10.0)
    } else {
        container(space().width(7).height(7)).style(theme::dot(if words == "needs you" { theme::GOLD } else { theme::LINE })).into()
    });
    let mut words_col = Column::new().push(label(name, 12.5, if open { theme::GOLD } else { theme::TEXT }).wrapping(iced::widget::text::Wrapping::None));
    let counts = match (topics, tasks) {
        (t, 0) => format!("{t} topic{}", if t == 1 { "" } else { "s" }),
        (t, k) => format!("{t} topic{}, {k} task{}", if t == 1 { "" } else { "s" }, if k == 1 { "" } else { "s" }),
    };
    words_col = words_col.push(label(counts, 10.5, theme::TEXT_FAINT));
    line = line.push(container(words_col).width(Length::Fill).clip(true));
    line = line.push(label(words, 10.5, colour));
    // A group being merged away: the others are where it can go.
    if groups.merging.as_deref().is_some_and(|from| from != group.id) {
        line = line.push(small("Merge here", Some(go(GroupsMsg::MergeInto(group.id.clone())))));
    }
    if groups.moving.as_deref().is_some_and(|topic| !group.topics.iter().any(|x| x == topic)) {
        line = line.push(small("Move here", Some(go(GroupsMsg::MoveTo(Some(group.id.clone()))))));
    }
    col = col.push(button(line).width(Length::Fill).padding([4.0, 10.0]).style(theme::list_row(open)).on_press(go(GroupsMsg::Open(Some(group.id.clone())))));
    if !open {
        // Shown by its arrow: its topics and tasks, to open one, without its actions.
        return if peeked { topic_rows(state, &members, col, false) } else { col };
    }
    let pad = |e: El<'a>| container(e).padding(Padding { top: 2.0, right: 10.0, bottom: 2.0, left: 30.0 });
    // Its actions.
    match &groups.renaming {
        Some((id, typed)) if *id == group.id => {
            col = col.push(pad(
                row![
                    text_input("Its name (empty: its first topic's)", typed)
                        .on_input(|t| go(GroupsMsg::Renaming(t)))
                        .on_submit(go(GroupsMsg::SaveName))
                        .padding([4.0, 8.0])
                        .size(12.0)
                        .style(ui::input_style),
                    small("Save", Some(go(GroupsMsg::SaveName))),
                    small("Cancel", Some(go(GroupsMsg::Cancel))),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into(),
            ));
        }
        _ if groups.merging.as_deref() == Some(group.id.as_str()) => {
            col = col.push(pad(
                row![note("Pick the group to merge it into (Merge here)."), small("Cancel", Some(go(GroupsMsg::Cancel)))]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into(),
            ));
        }
        _ if groups.plan_edit.as_ref().is_some_and(|e| e.group == group.id) => {
            if let Some(edit) = &groups.plan_edit {
                col = col.push(pad(super::plan::form(edit)));
            }
        }
        _ => {
            let others = groups.list.iter().any(|(g, _)| g.id != group.id && !g.archived);
            // The plan in brief, from the board's card.
            if let Some(card) = groups.board.iter().find(|c| c.group == group.id) {
                if let Some(words) = super::plan::summary(card) {
                    col = col.push(pad(label(words, 11.0, theme::TEXT_DIM).into()));
                }
                if let Some(late) = &card.behind {
                    col = col.push(pad(
                        label(format!("Behind: \u{201c}{}\u{201d} was due {}", late.title, late.due.as_deref().unwrap_or("")), 11.0, theme::CAUTION).into(),
                    ));
                }
            }
            col = col.push(pad(
                row![
                    small("Plan", Some(super::plan::send(super::plan::PlanMsg::Open(group.id.clone())))),
                    small("Rename", Some(go(GroupsMsg::Rename(group.id.clone())))),
                    small("Merge into\u{2026}", others.then(|| go(GroupsMsg::Merge(group.id.clone())))),
                    small("Archive", Some(go(GroupsMsg::Archive(group.id.clone(), true)))),
                ]
                .spacing(2)
                .into(),
            ));
        }
    }
    col = topic_rows(state, &members, col, true);
    if groups.moving.as_deref().is_some_and(|t| members.iter().any(|r| r.id == t)) {
        col = col.push(pad(note("Pick its group (Move here), or a new group of its own.")));
    }
    if members.is_empty() {
        col = col.push(pad(note("Nothing here yet: your next message starts its first topic.")));
    }
    col
}

/// A group's topics and tasks, newest first, under its line: each opens its conversation. With `actions` (the open
/// group), a topic can be moved to another group; shown by a group's arrow, they are only to open.
pub(super) fn topic_rows<'a>(state: &'a State, members: &[&'a ConversationSummary], mut col: Column<'a, Msg>, actions: bool) -> Column<'a, Msg> {
    let groups = &state.ide.groups;
    for r in members {
        let shown = state.conversation().is_some_and(|c| c.id() == r.id);
        let title = if r.title.is_empty() { "Untitled" } else { r.title.as_str() };
        let task = state.is_task(&r.id);
        let mut line = Row::new().spacing(6).align_y(Alignment::Center);
        line = line.push(container(label(if task { format!("Task: {title}") } else { title.to_string() }, 12.0, if shown { theme::GOLD } else { theme::TEXT_DIM }).wrapping(iced::widget::text::Wrapping::None)).width(Length::Fill).clip(true));
        if actions {
            let moving = groups.moving.as_deref() == Some(r.id.as_str());
            if moving {
                line = line.push(small("New group", Some(go(GroupsMsg::MoveTo(None)))));
                line = line.push(small("Cancel", Some(go(GroupsMsg::Cancel))));
            } else if !task {
                line = line.push(small("Move", Some(go(GroupsMsg::Move(r.id.clone())))));
            }
        } else {
            line = line.push(label(super::ago(crate::utc::now() - r.updated), 10.5, theme::TEXT_FAINT));
        }
        col = col.push(
            button(line)
                .width(Length::Fill)
                .padding(Padding { top: 2.0, right: 10.0, bottom: 2.0, left: 30.0 })
                .style(theme::list_row(shown))
                .on_press(Msg::Open(r.id.clone())),
        );
    }
    if members.is_empty() && !actions {
        col = col.push(container(note("No topics yet.")).padding(Padding { top: 2.0, right: 10.0, bottom: 2.0, left: 30.0 }));
    }
    col
}
