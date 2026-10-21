//! The agent's side of the IDE: the chat (the reader's messages as bubbles on the right; the agent's answers under its
//! mark, each turn's work one line that opens to its steps, each tool call a line that opens to its output; what
//! waits for the reader inline; and the composer, a pill), the Work view of the side bar (what the agents work on and
//! why, the to-do list, the changes waiting, the stack's suggestions, the checklists and the tools, each a list that
//! folds), the Chats view (every conversation, as an agent manager lists them), and the bottom panel's list of the
//! agent's commands.

use std::collections::HashSet;

use iced::widget::{
    Column, Row, button, checkbox, column, container, markdown, pick_list, rich_text, row, scrollable, space,
    span, text, text_editor, text_input, toggler,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, padding};

use crate::theme::{self, fonts};
use lattice_protocol::Locality;
use lattice_protocol::chat::{ChatTurn, Role};
use lattice_protocol::conversation::{
    ApprovalDetail, ArtifactKind, ChangeState, CommandMode, DecidedBy, ExitReason, Mode, PlanOutcome, ReviewOp,
    TaskOutcome,
    TodoStatus,
    TurnStatus,
};

use super::view::{ghost, icon};
use super::{ChatFilter, Fold, IdeMsg, Side, WorkList, names_a_file, subject, verb};
use crate::lattice::chat::{Activity, Conversation, Item, Tool};
use crate::lattice::{Msg, State};
use crate::ui::{self, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

fn go(msg: IdeMsg) -> Msg {
    Msg::Ide(msg)
}

fn solid(color: Color) -> container::Style {
    container::Style { background: Some(color.into()), ..container::Style::default() }
}

/// The reader's message while it is edited below: its bubble edged in gold.
fn editing_bubble(t: &iced::Theme) -> container::Style {
    let mut style = theme::bubble(t);
    style.border.color = theme::GOLD_DIM;
    style
}

/// A button style made round-ended: the composer's Stop and Send.
fn round(style: fn(&iced::Theme, button::Status) -> button::Style) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |t, s| {
        let mut b = style(t, s);
        b.border.radius = theme::PILL.into();
        b
    }
}

/// How the chat's answers are set: a size up from the panels' Markdown, to read at length.
fn reading() -> markdown::Settings {
    crate::lattice::view::settings_sized(15.0)
}

/// How wide the reader's message grows before it wraps: about four fifths of the reading column.
const BUBBLE_MAX: f32 = 640.0;

/// The composer of the grid's tiles: a raised card, gold edged while it has the focus.
pub(super) fn composer_card(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::CANVAS)),
        text_color: Some(theme::TEXT),
        border: Border { color: theme::LINE, width: 1.0, radius: 12.0.into() },
        ..container::Style::default()
    }
}

/// A card in the chat that asks or offers something (an approval, a question, a plan, a decision, a task, a saved
/// document): a notice in `color`, round-cornered as the chat's other cards are.
fn prompt_card(color: Color) -> impl Fn(&iced::Theme) -> container::Style {
    move |t| {
        let mut style = theme::notice(color)(t);
        style.border.radius = 14.0.into();
        style
    }
}

// --------------------------------------------------------------------------------------------- the panel

pub fn panel<'a>(state: &'a State, phase: f32) -> El<'a> {
    let mut col = Column::new().height(Length::Fill);
    col = col.push(header(state));
    col = col.push(container(space().height(1)).width(Length::Fill).style(theme::line));
    let body: El<'a> = match state.conversation() {
        None if state.opening.is_some() => container(ui::working(phase, "Opening the chat…")).padding(18).into(),
        // No chat yet: the composer is the page's centre, not its foot.
        None => return col.push(landing(state, phase)).into(),
        Some(c) => scrollable(container(stream(c, state, phase)).padding(Padding { top: 20.0, bottom: 18.0, left: 20.0, right: 22.0 }))
            .anchor_bottom()
            .height(Length::Fill)
            .style(theme::scrollbars)
            .into(),
    };
    col = col.push(container(body).height(Length::Fill));
    col = col.push(composer(state, phase));
    col.into()
}

/// No chat yet: the page's centre. The greeting, then the composer (the point of focus), then the starts until the
/// first message is sent. The space above and below holds it a little over the middle of the panel; the chat, once
/// begun, has its composer at its foot.
fn landing<'a>(state: &'a State, phase: f32) -> El<'a> {
    let mut col = Column::new().spacing(14).width(Length::Fill);
    col = col.push(container(welcome(state)).padding([0.0, 20.0]));
    col = col.push(composer(state, phase));
    if !state.ide.sent && !state.sending {
        col = col.push(container(starts(state)).padding([0.0, 16.0]));
    }
    // The newest chats, to go back to one (on one surface the stream opens on the newest by itself).
    if !crate::lattice::ONE_SURFACE
        && let Some(list) = &state.list
        && !list.conversations.is_empty()
    {
        let mut recent = Column::new().spacing(2).push(text("RECENT CHATS").size(11).font(fonts().ui_strong).color(theme::TEXT_FAINT));
        let now = crate::utc::now();
        for c in list.conversations.iter().take(5) {
            let title = if c.title.is_empty() { "Untitled" } else { c.title.as_str() };
            recent = recent.push(
                button(row![label(ui::cut(title, 40), 12.5, theme::TEXT), space().width(Length::Fill), label(super::ago(now - c.updated), 11.0, theme::TEXT_FAINT)].align_y(Alignment::Center))
                    .width(Length::Fill)
                    .padding([5.0, 8.0])
                    .style(theme::list_row(false))
                    .on_press(Msg::Open(c.id.clone())),
            );
        }
        col = col.push(container(recent).padding([0.0, 16.0]));
    }
    column![space().height(Length::FillPortion(2)), col, space().height(Length::FillPortion(3))].height(Length::Fill).into()
}

/// The open topic, and on one chat surface the older topics loaded above it, each under a divider that names it
/// and can continue it.
fn stream<'a>(c: &'a Conversation, state: &'a State, phase: f32) -> El<'a> {
    if !crate::lattice::ONE_SURFACE {
        return transcript(c, state, phase);
    }
    let mut col = Column::new().spacing(14).width(Length::Fill);
    if state.next_earlier().is_some() {
        let words = if state.loading_earlier { "Reading earlier…" } else { "Show earlier" };
        col = col.push(
            container(ghost(words, (!state.loading_earlier).then_some(Msg::Earlier))).center_x(Length::Fill),
        );
    }
    for older in &state.earlier {
        let folded = state.ide.folded_topics.contains(older.id());
        col = col.push(divider(older, true, folded));
        if !folded {
            // Its questions and approvals are answered in it, not in the topic open below.
            let id = older.id().to_string();
            col = col.push(transcript(older, state, phase).map(move |m| in_topic(&id, m)));
        }
    }
    if !state.earlier.is_empty() {
        col = col.push(divider(c, false, false));
    }
    col = col.push(transcript(c, state, phase));
    // What the tasks wait for, as cards: answered here, on the task's tile, or by the agent on the reader's word.
    col.push(task_prompts(state)).push(topic_prompts(state)).into()
}

/// `m` done in the topic `id` when it answers something there (a question, an approval, a plan, a choice); anything
/// else as it is.
pub(in crate::lattice) fn in_topic(id: &str, m: Msg) -> Msg {
    let answers = matches!(
        m,
        Msg::Decide(..)
            | Msg::AllowAlways(..)
            | Msg::Answer(..)
            | Msg::AnswerText(..)
            | Msg::Ide(IdeMsg::PlanApprove(..))
            | Msg::Ide(IdeMsg::PlanKeep(..))
            | Msg::Ide(IdeMsg::Choose(..))
    );
    if answers { Msg::InTopic(id.to_string(), Box::new(m)) } else { m }
}

/// What the open group's other topics wait for (`convo::fleet`), so the reader answers it here without opening
/// them: each question with its options, and each approval, under its topic's name, done in that topic. A topic drawn
/// above (the open one, an earlier one not folded) shows its own in place, and a task's tile has its card in
/// `task_prompts`.
fn topic_prompts<'a>(state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(10).width(Length::Fill);
    for wait in waiting_here(state) {
        let card = if wait.kind == "question" {
            question(&wait.call, &wait.text, &wait.options, false, state)
        } else {
            approval_line(&wait.call, &wait.text)
        };
        let title = if wait.topic_title.is_empty() { "A topic" } else { wait.topic_title.as_str() };
        let id = wait.topic.clone();
        col = col.push(
            Column::new()
                .spacing(4)
                .push(label(format!("\u{201c}{}\u{201d} asks you", ui::cut(title, 48)), 11.5, theme::GOLD))
                .push(card.map(move |m| in_topic(&id, m))),
        );
    }
    col.into()
}

/// What `topic_prompts` shows: the open group's waits in topics not drawn above and not on a task's tile.
pub(in crate::lattice) fn waiting_here(state: &State) -> Vec<&lattice_core::convo::fleet::Wait> {
    let Some(group) = state.ide.groups.open.as_deref() else { return Vec::new() };
    let mut drawn: Vec<&str> = state.open.iter().map(|c| c.id()).collect();
    drawn.extend(state.earlier.iter().filter(|c| !state.ide.folded_topics.contains(c.id())).map(|c| c.id()));
    drawn.extend(state.ide.grid.tiles.iter().map(|t| t.id.as_str()));
    state.ide.groups.waiting.iter().filter(|w| w.group.as_deref() == Some(group) && !drawn.contains(&w.topic.as_str())).collect()
}

/// An approval another topic waits for, as the fleet words it: what it would allow, approved or refused here (allowing
/// it always is done in the topic itself, where its full request shows).
fn approval_line<'a>(call: &'a str, what: &'a str) -> El<'a> {
    let col = Column::new()
        .spacing(8)
        .push(label("The agent asks to", 11.5, theme::GOLD))
        .push(label(what, 13.5, theme::TEXT))
        .push(
            row![
                ui::primary("Approve", Some(Msg::Decide(call.to_string(), true))),
                ui::secondary("Refuse", Some(Msg::Decide(call.to_string(), false))),
            ]
            .spacing(6),
        );
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(theme::GOLD)).into()
}

/// The questions, approvals and plans the tasks wait for, each under its task's name; each action is done in that
/// task.
fn task_prompts<'a>(state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(10).width(Length::Fill);
    for tile in &state.ide.grid.tiles {
        let Some(c) = &tile.conversation else { continue };
        let title = if c.summary.title.is_empty() { "A task" } else { c.summary.title.as_str() };
        for item in c.activity.iter().flat_map(|a| a.items.iter()) {
            let card = match item {
                Item::Approval { call, detail, allow_always, decided: None } => approval(call, detail, *allow_always, None, state),
                Item::Question { call, text, options, answered: false } => question(call, text, options, false, state),
                Item::Plan { plan, title: p, text, outcome: None } => plan_card(plan, p, text, None, state),
                Item::Choice { call, area, options, recommended, why, step, chosen: None } => {
                    choice_card(call, area, options, recommended, why, *step, None, state)
                }
                _ => continue,
            };
            let id = tile.id.clone();
            col = col.push(
                Column::new()
                    .spacing(4)
                    .push(label(format!("Task \u{201c}{}\u{201d} asks you", ui::cut(title, 48)), 11.5, theme::GOLD))
                    .push(card.map(move |m| super::grid::go(super::grid::GridMsg::Act(id.clone(), Box::new(m))))),
            );
        }
    }
    col.into()
}

/// Where one topic begins in the stream: its title and when. An older one's line folds its conversation away or
/// opens it again (`folded`), says how many messages it holds, and has Continue this.
fn divider<'a>(c: &'a Conversation, older: bool, folded: bool) -> El<'a> {
    let title = if c.summary.title.is_empty() { "Untitled" } else { c.summary.title.as_str() };
    let when = super::ago(crate::utc::now() - c.summary.updated);
    if !older {
        return row![
            container(space().height(1)).width(Length::Fixed(18.0)).style(theme::line),
            label(format!("{} \u{00B7} {when}", ui::cut(title, 48)), 11.5, theme::TEXT_FAINT),
            container(space().height(1)).width(Length::Fill).style(theme::line),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into();
    }
    let n = c.turns.len();
    let line = row![
        label(if folded { "\u{25B8}" } else { "\u{25BE}" }, 11.0, theme::GOLD),
        label(format!("{} \u{00B7} {when} \u{00B7} {n} message{}", ui::cut(title, 48), if n == 1 { "" } else { "s" }), 11.5, theme::TEXT_DIM),
        container(space().height(1)).width(Length::Fill).style(theme::line),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let tip = if folded { "Show this topic's conversation" } else { "Fold this topic to its line" };
    row![
        ui::tip(
            button(line).width(Length::Fill).padding([3.0, 6.0]).style(theme::ghost_button).on_press(go(IdeMsg::FoldTopic(c.id().to_string()))),
            tip,
        ),
        ghost("Continue this", Some(Msg::Open(c.id().to_string()))),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

/// The folder the next message works in, by name, and whether it is trusted: the open folder's, else the chat's own.
fn attached_folder(state: &State) -> Option<(String, bool)> {
    state.folder.view.as_ref().map(|v| (v.workspace.name.clone(), v.workspace.trusted)).or_else(|| {
        state.conversation().and_then(|c| c.summary.workspace.as_ref().map(|w| (w.name.clone(), w.trusted)))
    })
}

/// The last entry of the composer's project list: it makes a project rather than picking one.
const NEW_PROJECT: &str = "+ New project";

/// Where the next message works, as chips of the composer: on one chat surface the project (its folder, instructions
/// and model; the list ends with a way to make one) and, for a project of several folders, which of them; then the
/// folder the agent works in and whether it is trusted (Trust, when not), and Allow all when the project has it on.
fn places<'a>(state: &'a State) -> Vec<El<'a>> {
    const NONE: &str = "No project";
    let mut out: Vec<El<'a>> = Vec::new();
    let picked = state.ide.project.as_deref().and_then(|id| state.ide.projects.find(id));
    if crate::lattice::ONE_SURFACE {
        let projects = state.ide.projects.list.as_deref().unwrap_or_default();
        let mut names: Vec<String> = vec![NONE.to_string()];
        names.extend(projects.iter().filter(|p| !p.archived).map(|p| p.name.clone()));
        names.push(NEW_PROJECT.to_string());
        let current = Some(picked.map_or_else(|| NONE.to_string(), |p| p.name.clone()));
        let ids: Vec<(String, String)> = projects.iter().filter(|p| !p.archived).map(|p| (p.name.clone(), p.id.clone())).collect();
        let pick = pick_list(names, current, move |name: String| {
            if name == NEW_PROJECT {
                go(IdeMsg::Projects(super::projects::ProjectsMsg::Create))
            } else {
                Msg::WorkIn(ids.iter().find(|(n, _)| *n == name).map(|(_, id)| id.clone()))
            }
        })
        .text_size(12.0)
        .padding([4.0, 12.0])
        .style(theme::pill_picker)
        .menu_style(theme::picker_menu);
        out.push(ui::tip(pick, "The project the next message works in: its folder, instructions and model"));
        let short = |f: &str| super::file_name(&f.replace('\\', "/")).to_string();
        let folders: Vec<String> = picked.map(|p| p.folders().into_iter().map(str::to_string).collect()).unwrap_or_default();
        // A project of several folders: which of them the chat works in.
        if folders.len() > 1 {
            let shown: Vec<String> = folders.iter().map(|f| short(f)).collect();
            let current = state.work_folder().map(|f| short(&f));
            out.push(
                pick_list(shown, current, move |name: String| {
                    Msg::WorkFolder(folders.iter().find(|f| short(f) == name).cloned().unwrap_or_default())
                })
                .text_size(12.0)
                .padding([4.0, 12.0])
                .style(theme::pill_picker)
                .menu_style(theme::picker_menu)
                .into(),
            );
        }
    }
    if let Some((name, trusted)) = attached_folder(state) {
        let tone = if trusted { theme::POSITIVE } else { theme::GOLD_DIM };
        out.push(container(label(format!("\u{2750} {}", ui::cut(&name, 22)), 11.5, tone)).padding([4.0, 11.0]).style(theme::chip(tone)).into());
        if !trusted {
            out.push(ghost("Trust…", (!state.folder.busy).then_some(Msg::Trust)));
        }
    }
    // Allow all, when the project worked in has it on: one click to its page (the switch, and what was decided).
    if let Some(p) = picked.filter(|p| p.allow_all) {
        out.push(ui::tip(
            button(label("Allow all", 11.5, theme::GOLD))
                .padding([4.0, 11.0])
                .style(theme::pill_button(true))
                .on_press(go(IdeMsg::Projects(super::projects::ProjectsMsg::Open(p.id.clone())))),
            "The agent acts without asking in this project; only spending money asks. See what it decided for you.",
        ));
    }
    out
}

fn header<'a>(state: &'a State) -> El<'a> {
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    match state.conversation() {
        Some(c) => {
            // The group whose session this is, or the brainstorm chat.
            if state.ide.groups.brainstorming {
                line = line.push(ui::chip("Brainstorm", theme::GOLD_DIM));
            } else if let Some(name) = state.ide.groups.open.as_deref().and_then(|id| state.ide.groups.name(id)) {
                line = line.push(ui::chip(ui::cut(name, 28), theme::TEXT_DIM));
            }
            let title = if c.summary.title.is_empty() { "Untitled chat" } else { c.summary.title.as_str() };
            line = line.push(strong(ui::cut(title, 40), 13.5, theme::TEXT));
            if c.running {
                line = line.push(ui::chip("working", theme::GOLD));
            }
            if c.elsewhere {
                line = line.push(ui::chip("answered in another window", theme::CAUTION));
            }
            match c.waiting() {
                0 => {}
                n => line = line.push(ui::chip(format!("{n} waiting for you"), theme::GOLD)),
            }
        }
        None if state.ide.groups.brainstorming => line = line.push(strong("Brainstorm", 13.5, theme::TEXT)),
        None if crate::lattice::ONE_SURFACE => line = line.push(strong("Chat", 13.5, theme::TEXT)),
        None => line = line.push(strong("New chat", 13.5, theme::TEXT)),
    }
    line = line.push(space().width(Length::Fill));
    // Where the chat leads: one menu, not a row of buttons.
    line = line.push(super::jump::button_for(None));
    // The editor beside the chat (while the board or the tasks hold the page, the chat is beside them instead).
    if !state.ide.tasks_shown() {
        let shown = state.ide.editor_open;
        let tip = if shown { "Hide the editor (Ctrl+E)" } else { "Show the editor (Ctrl+E)" };
        line = line.push(icon("</>", tip, Some(go(IdeMsg::ToggleEditor))));
    }
    if crate::lattice::ONE_SURFACE {
        // The Tasks view: the tasks side by side, read-only, in the chat's place.
        let grid = &state.ide.grid;
        let tip = if grid.shown { "Back to the chat" } else { "Tasks, side by side" };
        line = line.push(icon("\u{25A6}", tip, Some(super::grid::go(super::grid::GridMsg::Show(!grid.shown)))));
        // The board: every group's card in its column.
        let board = state.ide.groups.board_shown;
        let tip = if board { "Back to the chat" } else { "Board: every group's status and priority" };
        line = line.push(icon("\u{25A4}", tip, Some(super::groups::go(super::groups::GroupsMsg::ShowBoard(!board)))));
    }
    if !crate::lattice::ONE_SURFACE {
        // The grid: chats side by side, shown in the chat's place.
        let grid = &state.ide.grid;
        let grid_tip = if grid.shown { "Back to the chat" } else { "Chats side by side" };
        line = line.push(icon("\u{25A6}", grid_tip, Some(super::grid::go(super::grid::GridMsg::Show(!grid.shown)))));
        line = line.push(icon("\u{25C8}", "All chats", Some(go(IdeMsg::Side(Side::Chats)))));
        if let Some(c) = state.conversation() {
            line = line.push(icon("\u{2399}", "Archive this chat (it is never deleted)", Some(Msg::Archive(c.id().to_string()))));
        }
        line = line.push(icon("+", "New chat (Ctrl+N)", Some(Msg::New)));
    }
    let mut col = Column::new().push(container(line).padding([8.0, 12.0]));
    if let Some(stages) = state.conversation().and_then(|c| stage_line(c, state.changes.as_ref())) {
        col = col.push(container(stages).padding(Padding { top: 0.0, right: 12.0, bottom: 7.0, left: 12.0 }));
    }
    // The chat works in another folder than the one the editor shows.
    if let (Some(c), Some(folder)) = (state.conversation(), &state.ide.folder)
        && let Some(w) = &c.summary.workspace
        && w.id != folder.id()
    {
        col = col.push(
            container(
                row![
                    label(format!("This chat works in {}; the editor shows {}.", w.name, folder.name()), 11.5, theme::CAUTION),
                    space().width(Length::Fill),
                    ghost("Show its folder", Some(go(IdeMsg::OpenRecent(w.path.clone())))),
                ]
                .align_y(Alignment::Center),
            )
            .padding(Padding { top: 0.0, right: 12.0, bottom: 6.0, left: 12.0 }),
        );
    }
    col.into()
}

/// Where a chat is in its work, under its title: the stages, the one it is at in gold, those passed as plain text and
/// those ahead faint, then how far along its task list is and the step in progress. Nothing until it has begun one.
pub(super) fn stage_line<'a>(c: &'a Conversation, changes: Option<&'a lattice_protocol::conversation::ChangeSet>) -> Option<El<'a>> {
    use crate::lattice::stages::{Mark, progress};
    let p = progress(c, changes);
    if p.now().is_none() && p.tasks.is_none() {
        return None;
    }
    let mut line = Row::new().spacing(5).align_y(Alignment::Center);
    for (i, (stage, mark)) in p.stages.iter().enumerate() {
        if i > 0 {
            line = line.push(label("\u{203A}", 11.0, theme::TEXT_FAINT));
        }
        let color = match mark {
            Mark::Now => theme::GOLD,
            Mark::Done => theme::TEXT,
            Mark::Ahead => theme::TEXT_FAINT,
        };
        line = line.push(label(stage.words(), 11.0, color).wrapping(iced::widget::text::Wrapping::None));
    }
    if let Some(tasks) = &p.tasks {
        line = line.push(space().width(Length::Fill));
        let words = match &tasks.current {
            Some(step) => format!("{}/{} \u{00B7} {}", tasks.done, tasks.total, ui::cut(step, 40)),
            None => format!("{}/{}", tasks.done, tasks.total),
        };
        line = line.push(label(words, 11.0, theme::TEXT_DIM).wrapping(iced::widget::text::Wrapping::None));
    }
    Some(container(line).width(Length::Fill).clip(true).into())
}

/// The hour of the day where the reader is, 0 to 23 (Windows' time zone, as Sinai's sky reads it).
fn local_hour() -> u32 {
    let local = crate::utc::now() + f64::from(crate::face::world::local_offset_seconds());
    (local.rem_euclid(86_400.0) / 3_600.0) as u32
}

/// How the empty chat greets the reader at an hour of the day.
pub(super) fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

/// The name a greeting uses: the first word of the account's name ("Ada" of "Ada Lovelace"); `None` for a blank one.
pub(super) fn first_name(name: &str) -> Option<&str> {
    name.split_whitespace().next()
}

/// The empty chat's welcome: the agent's mark and a greeting (by the reader's name and the time of day, when the
/// account gives a name), and what the agent does.
fn welcome<'a>(state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(10);
    let big = |words: String, color: Color| text(words).size(28.0).font(fonts().display).color(color);
    let name = state.reader.as_deref().and_then(first_name);
    let hello: El<'a> = match name {
        Some(name) => row![big(format!("{}, ", greeting(local_hour())), theme::TEXT), big(name.to_string(), theme::GOLD)].into(),
        None => big("What should we work on?".to_string(), theme::TEXT).into(),
    };
    col = col.push(
        row![container(label("\u{25C7}", 17.0, theme::GOLD)).center(34).style(theme::avatar), hello]
            .spacing(14)
            .align_y(Alignment::Center),
    );
    // Greeted by name, the question follows, about the folder open.
    if name.is_some() {
        let place = state.ide.folder.as_ref().map(|f| format!(" in {}", ui::cut(f.name(), 32))).unwrap_or_default();
        col = col.push(label(format!("What should we build, fix or explain{place} today?"), 17.0, theme::TEXT_DIM));
    }
    col = col.push(label(
        "The agent reads the folder you open, proposes changes you review before anything is written, and runs \
         commands only after you confirm each one.",
        13.5,
        if name.is_some() { theme::TEXT_FAINT } else { theme::TEXT_DIM },
    ));
    // No group open, with groups at work: the message goes to the one already doing that work.
    if crate::lattice::ONE_SURFACE
        && state.ide.groups.open.is_none()
        && !state.ide.groups.brainstorming
        && state.ide.groups.list.iter().any(|(g, _)| !g.archived && g.project == state.ide.project)
    {
        col = col.push(label(
            "It goes to the group already doing that work, and this page follows it to that chat; work no group does yet \
             starts a new group.",
            13.5,
            theme::TEXT_DIM,
        ));
    }
    col.into()
}

/// The empty chat's starts, under the composer: four cards (each puts its message in the composer, to send or change
/// first), two to a row.
fn starts<'a>(state: &'a State) -> El<'a> {
    let active = state.ide.active_tab().and_then(|t| t.path()).map(str::to_string);
    // (glyph, what it does, the message it starts with)
    let mut starts: Vec<(&str, String, String)> =
        vec![("\u{2750}", "Map this folder".to_string(), "Explain how this folder is organised.".to_string())];
    if let Some(path) = &active {
        let name = super::file_name(path);
        starts.push(("\u{2630}", format!("Explain {name}"), format!("Explain @{path}")));
        starts.push(("\u{26A0}", format!("Find bugs in {name}"), format!("Find bugs in @{path}")));
        starts.push(("\u{2713}", format!("Write tests for {name}"), format!("Write tests for @{path}")));
    } else {
        starts.push(("\u{26A0}", "Trace the errors".to_string(), "Find the place that handles errors and explain it.".to_string()));
        starts.push(("\u{2726}", "Suggest improvements".to_string(), "Suggest three small improvements.".to_string()));
    }
    let card = |(glyph, title, message): (&'a str, String, String)| -> El<'a> {
        let inner = row![
            container(label(glyph, 15.0, theme::GOLD)).center(30).style(theme::avatar),
            column![strong(ui::cut(&title, 40), 13.5, theme::TEXT), label(ui::cut(&message, 70), 12.0, theme::TEXT_DIM)].spacing(3),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        button(inner)
            .width(Length::FillPortion(1))
            .padding([12.0, 14.0])
            .style(|t, s| {
                let mut style = theme::list_row(false)(t, s);
                style.background = Some(Background::Color(if matches!(s, button::Status::Hovered | button::Status::Pressed) {
                    theme::WARM_HOVER
                } else {
                    theme::SIDEBAR
                }));
                style.border = Border { color: theme::EDGE, width: 1.0, radius: 14.0.into() };
                style
            })
            .on_press(go(IdeMsg::Suggest(message)))
            .into()
    };
    let mut cards = Column::new().spacing(10);
    let mut starts = starts.into_iter();
    while let Some(first) = starts.next() {
        let mut pair = Row::new().spacing(10).push(card(first));
        pair = match starts.next() {
            Some(second) => pair.push(card(second)),
            None => pair.push(space().width(Length::FillPortion(1))),
        };
        cards = cards.push(pair);
    }
    cards.into()
}

pub(super) fn transcript<'a>(c: &'a Conversation, state: &'a State, phase: f32) -> El<'a> {
    let mut col = Column::new().spacing(18).width(Length::Fill);
    let last_answer = c.turns.iter().rposition(|t| t.role == Role::Assistant);
    // The agent's mark heads a turn's work, and the answer under that work goes on beneath it unheaded.
    let mut headed = false;
    for (i, turn) in c.turns.iter().enumerate() {
        col = col.push(match turn.role {
            Role::User => user_turn(turn, state, c.running),
            // The newest answer can be written again once nothing runs.
            Role::Assistant => answer(c, i, turn, !headed, Some(i) == last_answer && !c.running && !state.sending),
        });
        headed = false;
        for activity in c.activity_for(&turn.id) {
            if let Some(work) = activity_view(c, activity, state, phase) {
                col = col.push(work);
                headed = true;
            }
        }
    }
    for activity in c.unplaced() {
        if let Some(work) = activity_view(c, activity, state, phase) {
            col = col.push(work);
        }
    }
    if let Some(live) = &c.live {
        col = col.push(markdown::view_with(live.items(), reading(), &Answer));
    }
    if c.dropped > 0 {
        col = col.push(note(format!("{} pieces of the streamed answer were not shown; the saved answer is whole.", c.dropped)));
    }
    let turn_shows = c.activity.last().is_some_and(|a| a.status.is_none() && !a.turn.is_empty());
    if c.running && !turn_shows {
        // Sent, and its turn not begun: no work of it shows yet to carry the spinner.
        col = col.push(working_line(c, phase));
    } else if !c.running
        && let Some(a) = c.activity.last()
        && matches!(a.status, Some(TurnStatus::MaxTurns | TurnStatus::Interrupted))
    {
        // Go on where a limit or a closed window stopped the turn.
        col = col.push(ui::secondary("Continue", (!state.sending).then(|| go(IdeMsg::Continue))));
    }
    for q in &c.queued {
        col = col.push(
            container(
                row![
                    label("Queued", 11.0, theme::GOLD),
                    label(ui::cut(&q.text, 70), 12.5, theme::TEXT_DIM).width(Length::Fill),
                    ghost("Cancel", Some(go(IdeMsg::CancelQueued(q.queued_id.clone())))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([6.0, 10.0])
            .width(Length::Fill)
            .style(theme::well),
        );
    }
    col.into()
}

/// The reader's message: a bubble on the right, and under it how many images went with it and Edit.
fn user_turn<'a>(turn: &'a ChatTurn, state: &'a State, running: bool) -> El<'a> {
    let editing = state.ide.editing_turn.as_deref() == Some(turn.id.as_str());
    let bubble = container(text(turn.text.as_str()).size(14.5).color(theme::TEXT).font(fonts().ui))
        .padding([10.0, 15.0])
        .max_width(BUBBLE_MAX)
        .style(if editing { editing_bubble } else { theme::bubble });
    let mut acts = Row::new().spacing(4).align_y(Alignment::Center);
    // The images attached to this message stay in the chat's record; the line says how many.
    if let Some(count) = state.conversation().and_then(|c| c.images.get(&turn.id)) {
        acts = acts.push(label(
            format!("\u{1F5BC} {count} image{} attached", if *count == 1 { "" } else { "s" }),
            11.0,
            theme::TEXT_FAINT,
        ));
    }
    if !turn.id.is_empty() && !running {
        acts = acts.push(ghost(if editing { "Editing below…" } else { "Edit" }, (!editing).then(|| go(IdeMsg::EditTurn(turn.id.clone())))));
        acts = acts.push(ghost("Fork", (!editing).then(|| go(IdeMsg::ForkTurn(turn.id.clone())))));
    }
    column![row![space().width(Length::Fill), bubble], row![space().width(Length::Fill), acts]].spacing(2).into()
}

/// The agent's mark over its work and its answer: a gold diamond on a square of gold wash, its name, and what answered
/// (the model's label: green on this computer, and saying so when it is off it; `None`, where is not known).
fn agent_head<'a>(model: &'a str, local: Option<bool>) -> El<'a> {
    let mark = container(label("\u{25C7}", 13.0, theme::GOLD)).center(24).style(theme::avatar);
    let mut line = Row::new().spacing(9).align_y(Alignment::Center).push(mark).push(strong("Lattice", 13.5, theme::TEXT));
    let tone = match local {
        Some(true) => theme::POSITIVE,
        Some(false) => theme::CAUTION,
        None => theme::TEXT_FAINT,
    };
    if !model.is_empty() {
        line = line.push(label(model, 11.5, tone));
    }
    if local == Some(false) {
        line = line.push(label("off this computer", 11.5, theme::CAUTION));
    }
    line.into()
}

/// An answer: under the agent's mark (`head`, unless its turn's work above carries it), the Markdown, then Copy,
/// Regenerate on the newest answer (`again`), the model and the tokens, and how it ended when it was cut.
fn answer<'a>(c: &'a Conversation, i: usize, turn: &'a ChatTurn, head: bool, again: bool) -> El<'a> {
    let mut col = Column::new().spacing(8);
    if head {
        col = col.push(agent_head(turn.provider.as_str(), None));
    }
    if let Some(content) = c.rendered(i) {
        col = col.push(markdown::view_with(content.items(), reading(), &Answer));
    }
    if !turn.error.is_empty() {
        col = col.push(ui::notice(turn.error.as_str(), theme::CAUTION));
    }
    if !turn.unsupported.is_empty() {
        col = col.push(note(format!("Figures no tool backs: {}", turn.unsupported.join(", "))));
    }
    let mut acts = Row::new().spacing(2).align_y(Alignment::Center);
    if !turn.text.trim().is_empty() {
        acts = acts.push(ui::tip(ghost("Copy", Some(go(IdeMsg::CopyText(turn.text.clone())))), "Copy the answer as written (Markdown)"));
    }
    if again {
        acts = acts.push(ui::tip(ghost("\u{27F3} Regenerate", Some(go(IdeMsg::Regenerate))), "Write this answer again"));
    }
    let mut about: Vec<String> = Vec::new();
    if !head && !turn.provider.is_empty() {
        about.push(turn.provider.clone());
    }
    if let (Some(p), Some(q)) = (turn.prompt_tokens, turn.completion_tokens) {
        about.push(format!("{p} in \u{00B7} {q} out"));
    }
    if !about.is_empty() {
        acts = acts.push(container(label(about.join(" \u{00B7} "), 11.0, theme::TEXT_FAINT)).padding(padding::left(8)));
    }
    if turn.cancelled {
        acts = acts.push(ui::chip("stopped", theme::TEXT_DIM));
    }
    if turn.truncated {
        acts = acts.push(ui::chip("cut short", theme::CAUTION));
    }
    col.push(acts).into()
}

/// How an answer's Markdown is drawn: as iced draws it, with each code block headed by its language and a Copy.
struct Answer;

impl<'a> markdown::Viewer<'a, Msg> for Answer {
    fn on_link_click(_url: markdown::Uri) -> Msg {
        Msg::Link
    }

    fn code_block(&self, settings: markdown::Settings, language: Option<&'a str>, code: &'a str, lines: &'a [markdown::Text]) -> El<'a> {
        let head = row![
            label(language.unwrap_or(""), 11.0, theme::TEXT_FAINT),
            space().width(Length::Fill),
            ui::tip(ghost("Copy", Some(go(IdeMsg::CopyText(code.to_string())))), "Copy this code"),
        ]
        .align_y(Alignment::Center);
        column![head, markdown::code_block(settings, lines, Self::on_link_click)].spacing(2).into()
    }
}

/// While the agent works: what it is doing and for how long (Esc in the composer stops it).
fn working_line<'a>(c: &'a Conversation, phase: f32) -> El<'a> {
    let a = c.activity.last();
    let stage = a.and_then(|a| a.stage.clone()).unwrap_or_else(|| "Working".to_string());
    let since = a.and_then(|a| a.started).map(|at| super::span(crate::utc::now() - at));
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    line = line.push(crate::spinner::spinner(phase, 14.0));
    line = line.push(label(format!("{stage}…"), 13.0, theme::GOLD));
    if let Some(since) = since {
        line = line.push(label(since, 12.0, theme::TEXT_FAINT));
    }
    line = line.push(label("· Esc to stop", 12.0, theme::TEXT_FAINT));
    line.into()
}

/// A turn's work: the agent's mark, the work as one line that opens to its steps, and under it what waits for the
/// reader or must be seen however the work is folded ([`pinned`]). Nothing for a turn that did nothing and is over.
fn activity_view<'a>(c: &'a Conversation, a: &'a Activity, state: &'a State, phase: f32) -> Option<El<'a>> {
    let running = c.running && a.status.is_none() && c.activity.last().is_some_and(|last| std::ptr::eq(last, a));
    if a.items.is_empty() && a.label.is_empty() && !running {
        return None;
    }
    let mut col = Column::new().spacing(10).push(agent_head(a.label.as_str(), Some(a.local)));
    let steps: Vec<&Item> = a.items.iter().filter(|item| !pinned(item)).collect();
    if !steps.is_empty() || running || a.status.and_then(ended_words).is_some() {
        col = col.push(work(a, &steps, running, state, phase));
    }
    for item in a.items.iter().filter(|item| pinned(item)) {
        col = col.push(item_view(item, state));
    }
    Some(col.into())
}

/// Whether an item shows under its turn's work however that is folded: it waits for the reader (an approval, a
/// question, a plan, a stack decision or a suggested task not yet settled), or must be seen (an error, a change that
/// could not be kept, a document saved beside the chat).
pub(super) fn pinned(item: &Item) -> bool {
    match item {
        Item::Approval { decided, .. } => decided.is_none(),
        Item::Question { answered, .. } => !answered,
        Item::Plan { outcome, .. } => outcome.is_none(),
        Item::Choice { chosen, .. } => chosen.is_none(),
        Item::Task { outcome, .. } => outcome.is_none(),
        Item::Error(_) | Item::Conflict { .. } | Item::Artifact { .. } => true,
        Item::Tool(_) | Item::Staged { .. } | Item::Steered(_) | Item::Notice(_) => false,
    }
}

/// How a turn ended, when it did not simply finish.
fn ended_words(status: TurnStatus) -> Option<(&'static str, Color)> {
    match status {
        TurnStatus::Completed => None,
        TurnStatus::Stopped => Some(("stopped", theme::TEXT_DIM)),
        TurnStatus::Failed => Some(("failed", theme::DANGER)),
        TurnStatus::Refused => Some(("refused", theme::CAUTION)),
        TurnStatus::MaxTurns => Some(("turn limit reached", theme::CAUTION)),
        TurnStatus::Interrupted => Some(("interrupted", theme::CAUTION)),
    }
}

/// The steps a turn's open work shows before "Show earlier steps".
pub(super) const STEPS_SHOWN: usize = 8;

/// How much of a turn's work shows: as the reader last left it, else open while it runs and folded once it ends.
pub(super) fn fold_of(folds: &std::collections::HashMap<String, Fold>, turn: &str, running: bool) -> Fold {
    folds.get(turn).copied().unwrap_or(if running { Fold::Open } else { Fold::Folded })
}

/// A turn's work as one line that folds and opens: while it runs, what it is doing and for how long (Esc stops it);
/// after, how long it worked; then how many steps, files and commands, and how it ended when it did not finish. Open,
/// its steps: the latest [`STEPS_SHOWN`], or all once asked.
fn work<'a>(a: &'a Activity, steps: &[&'a Item], running: bool, state: &'a State, phase: f32) -> El<'a> {
    let fold = fold_of(&state.ide.work_folds, &a.turn, running);
    let open = fold != Fold::Folded && !steps.is_empty();
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    if !steps.is_empty() {
        line = line.push(label(if open { "\u{25BE}" } else { "\u{25B8}" }, 12.0, theme::GOLD));
    }
    if running {
        line = line.push(crate::spinner::spinner(phase, 13.0));
        line = line.push(label(format!("{}\u{2026}", a.stage.as_deref().unwrap_or("Working")), 13.0, theme::GOLD));
        if let Some(at) = a.started {
            line = line.push(label(super::span(crate::utc::now() - at), 12.0, theme::TEXT_FAINT));
        }
    } else {
        let took = a.started.zip(a.ended).map(|(from, to)| super::span(to - from));
        line = line.push(label(took.map_or_else(|| "Worked".to_string(), |t| format!("Worked for {t}")), 13.0, theme::TEXT));
    }
    let tally = Tally::of(&a.items);
    if tally.steps > 0 {
        line = line.push(label(format!("\u{00B7} {}", tally.words()), 12.0, theme::TEXT_FAINT));
    }
    if let Some((words, color)) = a.status.and_then(ended_words) {
        line = line.push(label(format!("\u{00B7} {words}"), 12.0, color));
    }
    line = line.push(space().width(Length::Fill));
    if running {
        line = line.push(label("Esc to stop", 11.5, theme::TEXT_FAINT));
    }
    let toggle = (!steps.is_empty()).then(|| go(IdeMsg::Work(a.turn.clone(), if open { Fold::Folded } else { Fold::Open })));
    let head = button(line).width(Length::Fill).padding([8.0, 10.0]).on_press_maybe(toggle).style(|t, s| {
        let mut style = theme::ghost_button(t, s);
        style.border.radius = 11.0.into();
        style
    });
    let mut col = Column::new().spacing(2).push(head);
    if open {
        let hidden = if fold == Fold::All { 0 } else { steps.len().saturating_sub(STEPS_SHOWN) };
        let mut list = Column::new().spacing(5);
        if hidden > 0 {
            let words = format!("Show {hidden} earlier step{}", if hidden == 1 { "" } else { "s" });
            list = list.push(ghost(words, Some(go(IdeMsg::Work(a.turn.clone(), Fold::All)))));
        }
        for item in &steps[hidden..] {
            list = list.push(item_view(item, state));
        }
        col = col.push(container(space().height(1)).width(Length::Fill).style(theme::line));
        col = col.push(container(list).padding(Padding { top: 8.0, right: 10.0, bottom: 8.0, left: 12.0 }));
    }
    container(col).padding(3).width(Length::Fill).style(theme::work_card).into()
}

/// What a turn's work comes to, for its folded line: its steps (tool calls), the files it edited or proposed edits to,
/// and its commands, with how many of those failed.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Tally {
    pub steps: usize,
    pub files: usize,
    pub commands: usize,
    pub failed: usize,
}

impl Tally {
    pub(super) fn of(items: &[Item]) -> Tally {
        let mut files: HashSet<String> = HashSet::new();
        let mut tally = Tally::default();
        for item in items {
            match item {
                Item::Tool(t) => {
                    tally.steps += 1;
                    match t.tool.as_str() {
                        "edit_file" | "write_file" | "delete_file" => {
                            files.insert(subject(&t.tool, &t.summary, t.target.as_deref()).replace('\\', "/"));
                        }
                        "run_command" => {
                            tally.commands += 1;
                            if matches!(&t.exit, Some((code, _, reason)) if *code != Some(0) || *reason != ExitReason::Exited) {
                                tally.failed += 1;
                            }
                        }
                        _ => {}
                    }
                }
                Item::Staged { path, .. } => {
                    files.insert(path.replace('\\', "/"));
                }
                _ => {}
            }
        }
        tally.files = files.len();
        tally
    }

    /// "9 steps · 2 files edited · 3 commands, 1 failed".
    pub(super) fn words(&self) -> String {
        let count = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut parts = vec![count(self.steps, "step", "steps")];
        if self.files > 0 {
            parts.push(count(self.files, "file edited", "files edited"));
        }
        if self.commands > 0 {
            let mut commands = count(self.commands, "command", "commands");
            if self.failed > 0 {
                commands.push_str(&format!(", {} failed", self.failed));
            }
            parts.push(commands);
        }
        parts.join(" \u{00B7} ")
    }
}

fn item_view<'a>(item: &'a Item, state: &'a State) -> El<'a> {
    match item {
        Item::Tool(t) => step(t, state),
        Item::Approval { call, detail, allow_always, decided } => approval(call, detail, *allow_always, decided.as_ref(), state),
        Item::Question { call, text: q, options, answered } => question(call, q, options, *answered, state),
        Item::Task { task, title, summary, prompt, group, outcome } => {
            task_chip(task, title, summary, prompt, group.as_deref(), outcome.as_ref(), state)
        }
        Item::Plan { plan, title, text, outcome } => plan_card(plan, title, text, outcome.as_ref(), state),
        Item::Choice { call, area, options, recommended, why, step, chosen } => {
            choice_card(call, area, options, recommended, why, *step, chosen.as_ref(), state)
        }
        Item::Artifact { name, title, kind, version, bytes } => artifact_card(name, title, *kind, *version, *bytes),
        Item::Staged { change, path, added, removed, authority, result } => {
            let mut line = Row::new().spacing(8).align_y(Alignment::Center);
            line = line.push(dot(theme::GOLD));
            line = line.push(label("Proposed", 12.5, theme::TEXT_DIM));
            line = line.push(link(path.as_str(), go(IdeMsg::Open(path.clone()))));
            line = line.push(counts(*added, *removed));
            if *authority {
                line = line.push(ui::chip("authority file", theme::CAUTION));
            }
            line = line.push(space().width(Length::Fill));
            line = line.push(match result {
                Some(r) => El::from(label(crate::lattice::view::review_words(r), 11.5, theme::TEXT_FAINT)),
                None => ghost("Review", Some(Msg::ShowDiff(change.clone()))),
            });
            line.into()
        }
        Item::Conflict { path, reason } => ui::notice(format!("Could not keep {path}: {reason}"), theme::CAUTION),
        Item::Steered(text) => row![label("\u{21B3}", 12.0, theme::GOLD), label(format!("You steered: {text}"), 12.5, theme::TEXT_DIM)].spacing(6).into(),
        Item::Notice(text) => note(text.as_str()),
        Item::Error(text) => ui::notice(text.as_str(), theme::DANGER),
    }
}

fn dot<'a>(color: Color) -> El<'a> {
    container(space().width(7).height(7)).style(theme::dot(color)).into()
}

fn counts<'a>(added: u32, removed: u32) -> El<'a> {
    let pieces: Vec<text::Span<'a, (), iced::Font>> = vec![
        span(format!("+{added}")).color(theme::POSITIVE).font(fonts().mono).size(11.5),
        span(" "),
        span(format!("\u{2212}{removed}")).color(theme::DANGER).font(fonts().mono).size(11.5),
    ];
    rich_text(pieces).into()
}

fn link<'a>(words: &'a str, msg: Msg) -> El<'a> {
    button(mono(ui::cut(words, 48), 12.0, theme::TEXT))
        .padding([0.0, 2.0])
        .style(|_, status| button::Style {
            background: matches!(status, button::Status::Hovered).then(|| Background::Color(theme::HOVER)),
            text_color: theme::TEXT,
            border: Border { radius: 4.0.into(), ..Border::default() },
            ..button::Style::default()
        })
        .on_press(msg)
        .into()
}

/// How a background command's call answers when it has started (lattice-core's `convo::background`).
const BACKGROUND_STARTED: &str = "Started in the background";

/// One tool call as one line: its kind, what it is about, how it ended; opened, its output.
fn step<'a>(t: &'a Tool, state: &'a State) -> El<'a> {
    let open = state.ide.expanded_calls.contains(&t.call);
    let running = t.tool == "run_command" && t.progress.is_some() && t.exit.is_none();
    // A background command (`run_command {background: true}`): it runs on after its turn, with its own Stop.
    let background = t.tool == "run_command" && t.output.as_deref().is_some_and(|o| o.starts_with(BACKGROUND_STARTED));
    let failed = matches!(&t.exit, Some((code, _, reason)) if *code != Some(0) || *reason != ExitReason::Exited);
    let color = if failed {
        theme::CAUTION
    } else {
        match t.tool.as_str() {
            "edit_file" | "write_file" | "delete_file" => theme::GOLD,
            "run_command" => theme::POSITIVE,
            _ => theme::TEXT_FAINT,
        }
    };
    let what = subject(&t.tool, &t.summary, t.target.as_deref());
    // A withdrawal names its task by the title its chip shows, not by its id.
    let what = match t.tool.as_str() {
        "withdraw_task" => state.open.as_ref().and_then(|c| c.task_title(what)).unwrap_or(what),
        _ => what,
    };
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    line = line.push(dot(color));
    line = line.push(strong(verb(&t.tool), 12.5, theme::TEXT));
    if names_a_file(&t.tool) && t.tool != "delete_file" {
        line = line.push(link(what, go(IdeMsg::Open(what.to_string()))));
    } else {
        line = line.push(mono(ui::cut(what, 52), 12.0, theme::TEXT_DIM));
    }
    if t.withheld {
        line = line.push(ui::chip("withheld: looks like a secret", theme::CAUTION));
    }
    line = line.push(space().width(Length::Fill));
    if background {
        line = line.push(ui::chip("background", theme::GOLD_DIM));
    }
    if running {
        if let Some((bytes, lines, _)) = &t.progress {
            line = line.push(label(format!("{lines} lines · {}", ui::bytes(*bytes)), 11.0, theme::TEXT_FAINT));
        }
        if background {
            line = line.push(
                button(label("Stop", 11.5, theme::TEXT))
                    .padding([2.0, 8.0])
                    .style(theme::ghost_button)
                    .on_press(Msg::StopCommand(t.call.clone())),
            );
        }
    }
    if let Some((code, ms, reason)) = &t.exit {
        let words = match reason {
            ExitReason::Exited => format!("exit {} · {:.1} s", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()), *ms as f64 / 1000.0),
            ExitReason::Stopped => "stopped".to_string(),
            ExitReason::TimedOut => "timed out".to_string(),
            ExitReason::Failed => "could not start".to_string(),
        };
        line = line.push(label(words, 11.0, if failed { theme::CAUTION } else { theme::TEXT_FAINT }));
    }
    let has_more = t.output.is_some() || t.progress.is_some() || !t.effect.is_empty();
    if has_more {
        line = line.push(
            button(label(if open { "\u{25BE}" } else { "\u{25B8}" }, 11.0, theme::TEXT_FAINT))
                .padding([0.0, 4.0])
                .style(theme::ghost_button)
                .on_press(go(IdeMsg::Expand(t.call.clone()))),
        );
    }
    let mut col = Column::new().spacing(4).push(line);
    if open || running {
        let mut detail = Column::new().spacing(4);
        if let Some((_, _, tail)) = &t.progress
            && t.exit.is_none()
            && !tail.is_empty()
        {
            detail = detail.push(mono(ui::cut(tail, 400), 11.5, theme::TEXT_DIM));
        }
        if let Some(output) = &t.output
            && !t.withheld
            && open
        {
            detail = detail.push(mono(ui::cut(output, 1200), 11.5, theme::TEXT_DIM));
            if t.truncated {
                detail = detail.push(note("Only the start is shown here; the model read more."));
            }
        }
        if !t.effect.is_empty() {
            detail = detail.push(label(
                format!("It changed {} file{} on disk: see Changes.", t.effect.len(), if t.effect.len() == 1 { "" } else { "s" }),
                11.5,
                theme::CAUTION,
            ));
        }
        if t.tool == "run_command" {
            detail = detail.push(ghost("Whole output", Some(go(IdeMsg::Output(t.call.clone(), ui::cut(what, 60))))));
        }
        col = col.push(
            row![
                container(space().width(1)).height(Length::Shrink).style(theme::line),
                container(detail).padding(Padding { top: 2.0, bottom: 4.0, left: 10.0, right: 0.0 }),
            ]
            .padding(padding::left(3)),
        );
    }
    col.into()
}

/// A call waiting for the reader, as a prompt with numbered choices: run it, always allow it, or say no (with what
/// the agent should do instead). Approving a command only asks: the core then opens its own confirmation.
fn approval<'a>(call: &'a str, detail: &'a ApprovalDetail, allow_always: bool, decided: Option<&'a (bool, DecidedBy)>, state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(8);
    match detail {
        ApprovalDetail::Command { text: command, cwd, mode, timeout_s, remote, staged_waiting, background, cost } => {
            col = col.push(strong("Run this command?", 13.0, theme::GOLD));
            col = col.push(container(mono(format!("$ {command}"), 12.5, theme::TEXT)).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
            let how = match mode {
                CommandMode::Direct => "run directly, no shell",
                CommandMode::PowerShell => "in Windows PowerShell",
            };
            let ends = if *background {
                "keeps running in the background until stopped".to_string()
            } else {
                format!("stops after {timeout_s} s")
            };
            col = col.push(label(
                format!("in {} · {how} · {ends}", if cwd.is_empty() { "the folder's top" } else { cwd }),
                11.5,
                theme::TEXT_FAINT,
            ));
            if let Some(remote) = remote {
                col = col.push(label(format!("Its output goes to {remote}, off this computer."), 11.5, theme::CAUTION));
            }
            // A command the list of billed commands names (lattice-core `exec::cost`).
            if let Some(cost) = cost {
                col = col.push(label(cost.as_str(), 12.0, theme::CAUTION));
            }
            if *staged_waiting > 0 {
                col = col.push(label(
                    format!("Review the {staged_waiting} proposed change(s) first: a command cannot run while changes wait."),
                    11.5,
                    theme::CAUTION,
                ));
            }
        }
        ApprovalDetail::Mcp { server, tool, arguments_preview } => {
            col = col.push(strong(format!("Use {tool} from {server}?"), 13.0, theme::GOLD));
            col = col.push(container(mono(ui::cut(arguments_preview, 600), 11.5, theme::TEXT_DIM)).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
            col = col.push(label(
                format!("{server} is an MCP server, a program on this PC: it runs as you and may use the network."),
                11.5,
                theme::TEXT_FAINT,
            ));
        }
        ApprovalDetail::Browser { site, action, what, effect } => {
            col = col.push(strong(format!("Let the agent act on {site}?"), 13.0, theme::GOLD));
            let effect = lattice_core::browser::policy::Effect::parse(effect).map_or(effect.as_str(), |e| e.words());
            col = col.push(
                container(
                    column![
                        label(format!("It will {action}: {}", ui::cut(what, 300)), 12.5, theme::TEXT),
                        label(format!("It says its effect is {effect}."), 11.5, theme::TEXT_DIM),
                    ]
                    .spacing(3),
                )
                .padding([8.0, 10.0])
                .width(Length::Fill)
                .style(theme::well),
            );
            col = col.push(label("Look at its window before you say yes: what a page says can mislead the agent.", 11.5, theme::TEXT_FAINT));
            col = col.push(ghost("Show the browser", Some(go(IdeMsg::Tools(super::tools::ToolsMsg::BrowserShow)))));
        }
        ApprovalDetail::Desktop { app, action, what, effect } => {
            col = col.push(strong(format!("Let the agent act in {app}?"), 13.0, theme::GOLD));
            let effect = lattice_core::browser::policy::Effect::parse(effect).map_or(effect.as_str(), |e| e.words());
            col = col.push(
                container(
                    column![
                        label(format!("It will {action}: {}", ui::cut(what, 300)), 12.5, theme::TEXT),
                        label(format!("It says its effect is {effect}."), 11.5, theme::TEXT_DIM),
                    ]
                    .spacing(3),
                )
                .padding([8.0, 10.0])
                .width(Length::Fill)
                .style(theme::well),
            );
            col = col.push(label(
                format!(
                    "Auto mode: look at your screen before you say yes. {} stops the agent at once.",
                    lattice_core::desktop::STOP_KEYS
                ),
                11.5,
                theme::TEXT_FAINT,
            ));
        }
    }
    let mcp = matches!(detail, ApprovalDetail::Mcp { .. });
    let browser = matches!(detail, ApprovalDetail::Browser { .. } | ApprovalDetail::Desktop { .. });
    match decided {
        Some((approved, by)) => {
            let who = match by {
                DecidedBy::Reader => "by you".to_string(),
                DecidedBy::Standing { .. } => "by a standing approval".to_string(),
                DecidedBy::Policy { rule } => format!("by the rule {rule}"),
            };
            col = col.push(label(format!("{} {who}", if *approved { "Approved" } else { "Refused" }), 11.5, theme::TEXT_FAINT));
        }
        None => {
            // Each choice a pill, numbered; the first, gold.
            let choice = |n: &'a str, words: &'a str, msg: Msg, primary: bool| -> El<'a> {
                let inner = row![mono(n, 11.5, if primary { theme::ON_GOLD } else { theme::GOLD }), text(words).size(12.5).font(fonts().ui)].spacing(8);
                let b = button(inner).padding([6.0, 14.0]).on_press(msg);
                if primary { b.style(round(theme::primary_button)).into() } else { b.style(theme::pill_button(false)).into() }
            };
            let mut choices = Row::new().spacing(6).align_y(Alignment::Center);
            let yes = if mcp {
                "Yes, use it… (you confirm next)"
            } else if browser {
                "Yes, do it… (you confirm next)"
            } else {
                "Yes, run it… (you confirm next)"
            };
            choices = choices.push(choice("1", yes, Msg::Decide(call.to_string(), true), true));
            if allow_always {
                let (always, msg) = if mcp {
                    ("Yes, and always allow this tool…", Msg::AllowMcpAlways(call.to_string()))
                } else {
                    ("Yes, and always allow this exact command here…", Msg::AllowAlways(call.to_string()))
                };
                choices = choices.push(choice("2", always, msg, false));
            }
            choices = choices.push(choice(if allow_always { "3" } else { "2" }, "No", Msg::Decide(call.to_string(), false), false));
            col = col.push(choices.wrap().vertical_spacing(6));
            let typed = state.ide.notes.get(call).map(String::as_str).unwrap_or("");
            let owned = call.to_string();
            col = col.push(
                row![
                    text_input("No, and tell the agent what to do instead…", typed)
                        .on_input(move |t| go(IdeMsg::Note(owned.clone(), t)))
                        .on_submit(go(IdeMsg::RejectWithNote(call.to_string())))
                        .padding([6.0, 8.0])
                        .size(12.5)
                        .style(ui::input_style),
                    ghost("Send", (!typed.trim().is_empty()).then(|| go(IdeMsg::RejectWithNote(call.to_string())))),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
    }
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(theme::GOLD)).into()
}

fn question<'a>(call: &'a str, q: &'a str, options: &'a [String], answered: bool, state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(8).push(label("The agent asks", 11.5, theme::GOLD)).push(label(q, 13.5, theme::TEXT));
    if answered {
        col = col.push(label("Answered", 11.5, theme::TEXT_FAINT));
    } else {
        let typed = state.answers.get(call).map(String::as_str).unwrap_or("");
        let mut opts = Row::new().spacing(6);
        for o in options {
            opts = opts.push(ui::secondary(o.as_str(), Some(Msg::AnswerText(call.to_string(), o.clone()))));
        }
        col = col.push(opts.wrap());
        let owned = call.to_string();
        col = col.push(
            row![
                text_input("Your answer", typed)
                    .on_input(move |t| Msg::AnswerText(owned.clone(), t))
                    .on_submit(Msg::Answer(call.to_string()))
                    .padding([6.0, 8.0])
                    .size(12.5)
                    .style(ui::input_style),
                ui::primary("Answer", Some(Msg::Answer(call.to_string()))),
            ]
            .spacing(6),
        );
    }
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(theme::GOLD)).into()
}

/// A task the agent suggested (a chip): its title and summary; Start (a new chat of its own, in this folder and project,
/// beginning with the prompt) and Dismiss while it waits; and the prompt itself on request, so what Start sends can
/// be read first. Once settled, what became of it, and a started task's chat a click away.
fn task_chip<'a>(
    task: &'a str,
    title: &'a str,
    summary: &'a str,
    prompt: &'a str,
    group: Option<&'a str>,
    outcome: Option<&'a TaskOutcome>,
    state: &'a State,
) -> El<'a> {
    let shown = state.ide.task_prompts.contains(task);
    // A chip that waits is gold; a settled one is quiet.
    let tone = if outcome.is_none() { theme::GOLD } else { theme::TEXT_FAINT };
    // An idea from the brainstorm chat goes to a group's session, not to a task.
    let target = group.map(|g| super::groups::idea_target(&state.ide.groups, g));
    let mut col = Column::new()
        .spacing(6)
        .push(label(if group.is_some() { "Idea for a group" } else { "Suggested task" }, 11.5, tone))
        .push(strong(title, 13.5, theme::TEXT))
        .push(label(summary, 12.5, theme::TEXT_DIM));
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    let settled = |words: String| label(words, 11.5, theme::TEXT_FAINT).width(Length::Fill);
    match (outcome, &target) {
        (None, Some(target)) => {
            let words = if group == Some(lattice_core::convo::groups::NEW) { "Start a new group".to_string() } else { format!("Send to {target}") };
            line = line.push(
                button(iced::widget::text(words).size(13.5).font(fonts().ui_strong))
                    .padding([7.0, 16.0])
                    .style(theme::primary_button)
                    .on_press_maybe((!state.sending).then(|| go(IdeMsg::TaskStart(task.to_string())))),
            );
            line = line.push(ui::secondary("Dismiss", Some(go(IdeMsg::TaskDismiss(task.to_string())))));
            line = line.push(label("Nothing is sent until you press it", 11.0, theme::TEXT_FAINT));
            line = line.push(space().width(Length::Fill));
        }
        (Some(TaskOutcome::Started { conversation }), Some(target)) => {
            line = line.push(settled(format!("Sent to {target}")));
            line = line.push(ghost("Open it", Some(Msg::Open(conversation.clone()))));
        }
        (None, None) => {
            line = line.push(ui::primary("Start", (!state.sending).then(|| go(IdeMsg::TaskStart(task.to_string())))));
            line = line.push(ui::secondary("Dismiss", Some(go(IdeMsg::TaskDismiss(task.to_string())))));
            if crate::lattice::ONE_SURFACE {
                line = line.push(label("Runs beside this chat", 11.0, theme::TEXT_FAINT));
            }
            line = line.push(space().width(Length::Fill));
        }
        (Some(TaskOutcome::Started { conversation }), None) => {
            let words = if crate::lattice::ONE_SURFACE {
                // Where the task is now, from the topics' list.
                match state.list.as_ref().and_then(|l| l.conversations.iter().find(|c| c.id == *conversation)) {
                    Some(c) if c.needs_you || state.calling.contains(&c.id) => "Running beside this chat: it needs you",
                    Some(c) if c.running => "Running beside this chat",
                    _ => "Ran as a topic of its own",
                }
            } else {
                "Started as a new chat"
            };
            line = line.push(settled(words.to_string()));
            line = line.push(ghost("Open it", Some(Msg::Open(conversation.clone()))));
        }
        (Some(TaskOutcome::Dismissed), _) => line = line.push(settled("Dismissed".to_string())),
        (Some(TaskOutcome::Withdrawn { reason }), _) if reason.is_empty() => {
            line = line.push(settled("The agent withdrew it".to_string()))
        }
        (Some(TaskOutcome::Withdrawn { reason }), _) => line = line.push(settled(format!("The agent withdrew it: {reason}"))),
    }
    line = line.push(ghost(if shown { "Hide prompt" } else { "Show prompt" }, Some(go(IdeMsg::TaskPrompt(task.to_string())))));
    col = col.push(line);
    if shown {
        col = col.push(
            container(mono(prompt, 12.0, theme::TEXT))
                .padding([8.0, 10.0])
                .width(Length::Fill)
                .style(|_: &iced::Theme| solid(theme::CANVAS)),
        );
    }
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(tone)).into()
}

/// A stack decision on a to-do step (the advisor's catalog): the question, why, and each option with its summary, how
/// it fits this project, its pros and cons, licence and cost, and Choose; the recommended one first. Once made, what
/// was chosen, and whether the agent chose it on its own.
#[allow(clippy::too_many_arguments)]
fn choice_card<'a>(
    call: &'a str,
    area: &'a str,
    options: &'a [lattice_protocol::conversation::ChoiceFit],
    recommended: &'a str,
    why: &'a str,
    step: Option<u32>,
    chosen: Option<&'a (String, bool)>,
    state: &'a State,
) -> El<'a> {
    let Some(area) = lattice_core::advice::area(area) else {
        return container(label(format!("A decision in {area}, which this build's catalog does not know."), 12.0, theme::CAUTION)).into();
    };
    let tone = if chosen.is_none() { theme::GOLD } else { theme::TEXT_FAINT };
    let head = match step {
        Some(n) => format!("Decision for step {n} \u{00B7} {}", area.title),
        None => format!("Decision \u{00B7} {}", area.title),
    };
    let mut col = Column::new().spacing(8).push(label(head, 11.5, tone)).push(strong(area.question.as_str(), 13.5, theme::TEXT));
    if !why.is_empty() {
        col = col.push(label(why, 12.5, theme::TEXT_DIM));
    }
    if let Some((option, by_agent)) = chosen {
        let name = area.choice(option).map_or(option.as_str(), |c| c.name.as_str());
        let words = if *by_agent { format!("Chosen by the agent: {name}") } else { format!("Chosen: {name}") };
        return container(col.push(label(words, 12.5, theme::TEXT))).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(tone)).into();
    }
    for fit in options {
        let Some(choice) = area.choice(&fit.option) else { continue };
        let mut name = Row::new().spacing(8).align_y(Alignment::Center).push(strong(choice.name.as_str(), 13.0, theme::TEXT));
        if fit.option == recommended {
            name = name.push(ui::chip("recommended", theme::GOLD));
        }
        if fit.used {
            name = name.push(ui::chip("in use here", theme::POSITIVE));
        }
        name = name.push(space().width(Length::Fill));
        name = name.push(ui::primary("Choose", (!state.sending).then(|| go(IdeMsg::Choose(call.to_string(), fit.option.clone())))));
        let mut option = Column::new().spacing(4).push(name).push(label(choice.summary.as_str(), 12.0, theme::TEXT_DIM));
        if !fit.fit.is_empty() {
            option = option.push(label(format!("Here: {}", fit.fit), 12.0, theme::TEXT));
        }
        let list = |items: &'a [String], mark: &'static str, colour| {
            items.iter().fold(Column::new().spacing(2), |c, item| c.push(label(format!("{mark} {item}"), 11.5, colour)))
        };
        option = option.push(
            row![
                container(list(&choice.pros, "+", theme::TEXT)).width(Length::FillPortion(1)),
                container(list(&choice.cons, "\u{2212}", theme::TEXT_DIM)).width(Length::FillPortion(1)),
            ]
            .spacing(12),
        );
        option = option.push(label(format!("{} \u{00B7} {}", choice.licence, choice.cost), 11.0, theme::TEXT_FAINT));
        col = col.push(container(option).padding([8.0, 10.0]).width(Length::Fill).style(|_: &iced::Theme| solid(theme::CANVAS)));
    }
    col = col.push(label(
        format!("From Lattice's catalog as of {}; the agent's lines on how each fits are its own.", lattice_core::advice::catalog().as_of),
        11.0,
        theme::TEXT_FAINT,
    ));
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(tone)).into()
}

// --------------------------------------------------------------------------------------------- the Work side

/// The side bar's Work view, where the folder's files were: what the agents work on now and what waits for the
/// reader, a list each ([`WorkList`]) that folds to its head. First what the agents work on, each agent under its name
/// with the step it is on (why it has its files open) and the files, each opening in the editor; then the chat's
/// to-do list, the changes waiting for review, the stack's suggestions, the checklists the agents gave the reader and
/// the tools. A list with nothing in it is left out, but for the first and the tools; with no folder open, a way to
/// open one comes first.
pub fn work_side<'a>(state: &'a State, phase: f32) -> El<'a> {
    let mut col = Column::new().spacing(2).padding(padding::bottom(14));
    if state.ide.folder.is_none() {
        col = col.push(no_folder(state, phase));
    }
    for list in WorkList::ORDER {
        if let Some(section) = work_section(state, list) {
            col = col.push(section);
        }
    }
    column![
        super::view::section_title("Work", vec![]),
        scrollable(col.width(Length::Fill)).height(Length::Fill).style(theme::scrollbars),
    ]
    .height(Length::Fill)
    .into()
}

/// No folder open: the way to open one (the Explorer, from the jump menu, also takes a typed path).
fn no_folder<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let open: El<'a> = if ide.opening {
        ui::working(phase, "Opening the folder\u{2026}")
    } else if ide.picking {
        ui::working(phase, "Choose a folder in Windows' picker\u{2026}")
    } else {
        ui::secondary("Open folder\u{2026}", state.services_ready().then(|| go(IdeMsg::Browse)))
    };
    column![label("No folder is open: open the one to work in, and the agent works in it.", 12.0, theme::TEXT_DIM), open]
        .spacing(8)
        .padding(Padding { top: 2.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .into()
}

/// One list of the Work side: its head, and under it the list while it is open; `None` while it has nothing to show
/// (what the agents work on and the tools are always there).
fn work_section<'a>(state: &'a State, list: WorkList) -> Option<El<'a>> {
    let (title, count, calls, action): (&str, String, bool, Option<El<'a>>) = match list {
        WorkList::Working => {
            let n = state.ide.working_on.len();
            ("Working on", if n == 0 { String::new() } else { plural(n, "file", "files") }, false, None)
        }
        WorkList::Todos => {
            let todos = &state.conversation()?.todos;
            if todos.is_empty() {
                return None;
            }
            let done = todos.iter().filter(|step| step.status == TodoStatus::Done).count();
            let count = if done == todos.len() { format!("all {done} done") } else { format!("{done} of {}", todos.len()) };
            // a step that waits on a stack decision asks the reader
            ("To-do", count, todos.iter().any(|step| step.decision.is_some()), None)
        }
        WorkList::Checklists => {
            let (lists, checked, total) = checklist_counts(state);
            if lists == 0 {
                return None;
            }
            // with more than one, the head folds or opens them all
            let action = (lists > 1).then(|| {
                let all_folded = steps_keys(state).iter().all(|key| state.ide.folded_steps.contains(key));
                super::view::icon(
                    if all_folded { "\u{229E}" } else { "\u{229F}" },
                    if all_folded { "Expand all" } else { "Collapse all" },
                    Some(go(IdeMsg::FoldAllSteps(!all_folded))),
                )
            });
            ("Checklists", format!("{checked} of {total}"), checked < total, action)
        }
        WorkList::Changes => {
            let n = waiting_changes(state).len();
            if n == 0 {
                return None;
            }
            ("Changes", plural(n, "file", "files"), true, None)
        }
        WorkList::Stack => {
            let stack = state.ide.stack.as_ref().filter(|s| !s.dismissed && !s.suggestions.is_empty())?;
            let n = stack.suggestions.len();
            let dismiss = super::view::icon("\u{00D7}", "Dismiss the suggestions", Some(go(IdeMsg::StackDismiss)));
            ("Stack", plural(n, "suggestion", "suggestions"), false, Some(dismiss))
        }
        WorkList::Tools => ("Tools", tools_on(&state.ide.tools).map(|n| format!("{n} on")).unwrap_or_default(), false, None),
    };
    let folded = state.ide.work_folded(list);
    let mut col = Column::new().push(work_head(list, title, count, calls, folded, action));
    if !folded {
        let body = match list {
            WorkList::Working => working_list(state),
            WorkList::Todos => todos_list(state)?,
            WorkList::Checklists => checklists(state)?,
            WorkList::Changes => changes_list(state)?,
            WorkList::Stack => stack_list(state)?,
            WorkList::Tools => tools_panel(state),
        };
        col = col.push(container(body).width(Length::Fill).padding(Padding { top: 2.0, right: 12.0, bottom: 8.0, left: 16.0 }));
    }
    Some(col.into())
}

/// Whether a list of the Work side asks something of the reader: a checklist's step unchecked, a change waiting for
/// review, or a to-do step waiting on a decision. The activity bar marks the Work view while one does.
pub(super) fn work_calls(state: &State) -> bool {
    let (_, checked, total) = checklist_counts(state);
    checked < total
        || !waiting_changes(state).is_empty()
        || state.conversation().is_some_and(|c| c.todos.iter().any(|step| step.decision.is_some()))
}

/// "1 file", "3 files".
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A list's head on the Work side: an arrow, its name in small capitals, how much it holds, and a gold dot while it
/// asks something of the reader (`calls`); the whole line folds the list to it or opens it again. `action` stands at
/// its end.
fn work_head<'a>(list: WorkList, title: &'a str, count: String, calls: bool, folded: bool, action: Option<El<'a>>) -> El<'a> {
    let mut words = Row::new()
        .spacing(7)
        .align_y(Alignment::Center)
        .push(label(if folded { "\u{25B8}" } else { "\u{25BE}" }, 11.0, theme::TEXT_FAINT))
        .push(ui::caps(title, 11.0, theme::TEXT_DIM));
    if !count.is_empty() {
        words = words.push(label(count, 11.5, theme::TEXT_FAINT));
    }
    if calls {
        words = words.push(dot(theme::GOLD));
    }
    let tip = if folded { "Show this list" } else { "Fold this list to its head" };
    let head = ui::tip(
        button(words).width(Length::Fill).padding([4.0, 6.0]).style(theme::ghost_button).on_press(go(IdeMsg::SideFold(list.key().to_string()))),
        tip,
    );
    let mut line = Row::new().spacing(4).align_y(Alignment::Center).push(head);
    if let Some(action) = action {
        line = line.push(action);
    }
    container(line).padding(Padding { top: 6.0, right: 8.0, bottom: 0.0, left: 4.0 }).into()
}

/// What the agents work on now: each agent by name, the step of its to-do list it is on (why it has its files open),
/// and its files, newest first, each opening in the editor at a press (one in another folder than the editor's cannot,
/// and its tip says so). While no agent works, a line on what shows here.
fn working_list<'a>(state: &'a State) -> El<'a> {
    let working = &state.ide.working_on;
    if working.is_empty() {
        return label("No agent is working now. The files an agent works on show here as it goes, and open in the editor.", 11.5, theme::TEXT_FAINT).into();
    }
    // the agents in the order of their newest file
    let mut agents: Vec<&str> = Vec::new();
    for w in working {
        if !agents.contains(&w.by.as_str()) {
            agents.push(w.by.as_str());
        }
    }
    let mut col = Column::new().spacing(10);
    for by in agents {
        let files: Vec<&super::WorkedFile> = working.iter().filter(|w| w.by == by).collect();
        let mut agent = Column::new().spacing(2);
        agent = agent.push(row![dot(theme::GOLD), strong(ui::cut(&files[0].who, 32), 12.5, theme::TEXT)].spacing(7).align_y(Alignment::Center));
        if let Some(step) = step_on(state, by) {
            agent = agent.push(row![label("\u{25B8}", 11.5, theme::GOLD), label(step, 11.5, theme::TEXT_DIM)].spacing(6));
        }
        for w in files {
            let line = row![super::view::file_icon(&w.path), label(super::file_name(&w.path), 12.0, if w.here { theme::TEXT } else { theme::TEXT_DIM })]
                .spacing(6)
                .align_y(Alignment::Center);
            let tip = if w.here { w.path.clone() } else { format!("{} (in another folder than the editor's)", w.path) };
            agent = agent.push(ui::tip(
                button(line).width(Length::Fill).padding([2.0, 6.0]).style(theme::ghost_button).on_press_maybe(w.here.then(|| go(IdeMsg::Open(w.path.clone())))),
                tip,
            ));
        }
        col = col.push(agent);
    }
    col.into()
}

/// The step in progress on chat or task `by`'s to-do list: why its agent has its files open.
fn step_on<'a>(state: &'a State, by: &str) -> Option<&'a str> {
    let chat = state.conversation().filter(|c| c.id() == by).or_else(|| {
        state.ide.grid.tiles.iter().find(|t| t.id == by).and_then(|t| t.conversation.as_ref())
    })?;
    chat.todos.iter().find(|step| step.status == TodoStatus::InProgress).map(|step| step.content.as_str())
}

/// How many of the agent's switchable tools are on: its browser, your desktop, the MCP servers and the plugins.
/// `None` before any of them has been read.
pub(super) fn tools_on(tools: &super::tools::Tools) -> Option<usize> {
    let read = [
        tools.browser.as_ref().map(|(on, _, _)| usize::from(*on)),
        tools.auto.map(|(on, _)| usize::from(on)),
        tools.overview.as_ref().map(|o| o.servers.iter().filter(|s| s.enabled && !s.off).count()),
        tools.plugins.as_ref().map(|p| p.iter().filter(|p| p.on).count()),
    ];
    read.iter().any(Option::is_some).then(|| read.iter().flatten().sum())
}

/// "1 of 2 servers on", or, with none added, that the Tools page adds them; "Reading…" before they are read.
pub(super) fn switched(read: Option<(usize, usize)>, one: &str, many: &str) -> String {
    match read {
        None => "Reading\u{2026}".to_string(),
        Some((_, 0)) => format!("No {many} yet: the Tools page adds them."),
        Some((on, of)) => format!("{on} of {of} {} on.", if of == 1 { one } else { many }),
    }
}

/// "3 skills and 1 command: / lists the commands."; "Reading…" before they are read.
pub(super) fn skills_words(read: Option<(usize, usize)>) -> String {
    let count = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match read {
        None => "Reading\u{2026}".to_string(),
        Some((skills, commands)) => {
            format!("{} and {}: / lists the commands.", count(skills, "skill", "skills"), count(commands, "command", "commands"))
        }
    }
}

/// What a tool's card offers at its end: nothing (the tool is always there), a switch (on or off as last read, and
/// what flipping it sends; `None` before it is read, when it waits), or the Tools page.
enum Offer {
    Always,
    Switch(Option<bool>, fn(bool) -> Msg),
    Page,
}

/// A tool's card, narrow enough for the side bar: its initial in its colour, its name, a line under it (what it asks
/// of the reader, or how many are on), and its offer; what it does in full is its tip.
fn tool_card<'a>(initial: &'a str, tone: Color, name: &'a str, line: String, what: String, offer: Offer) -> El<'a> {
    let mark = container(label(initial, 12.0, tone)).center(26).style(theme::chip(tone));
    let words = column![strong(name, 12.5, theme::TEXT), label(line, 11.0, theme::TEXT_DIM)].spacing(1).width(Length::Fill);
    let end: El<'a> = match offer {
        Offer::Always => space().width(0).into(),
        Offer::Switch(on, flip) => toggler(on.unwrap_or(false)).on_toggle_maybe(on.map(|_| flip)).size(18.0).style(theme::switch).into(),
        Offer::Page => ghost("Open", Some(go(IdeMsg::Tools(super::tools::ToolsMsg::Open(None))))),
    };
    let card = container(row![mark, words, end].spacing(8).align_y(Alignment::Center))
        .padding([7.0, 10.0])
        .width(Length::Fill)
        .style(|_: &iced::Theme| container::Style {
            background: Some(Background::Color(theme::WARM_RAISED)),
            text_color: Some(theme::TEXT),
            border: Border { color: Color { a: 0.12, ..theme::GOLD }, width: 1.0, radius: 12.0.into() },
            ..container::Style::default()
        });
    ui::tip(card, what)
}

/// A section's head in the tools: small faint capitals after an arrow; the whole line folds the section to it or
/// opens it again (`key` in [`super::Ide::side_folds`]).
fn fold_head<'a>(words: &'a str, key: &str, folded: bool) -> El<'a> {
    button(row![label(if folded { "\u{25B8}" } else { "\u{25BE}" }, 11.0, theme::TEXT_FAINT), ui::caps(words, 11.0, theme::TEXT_FAINT)].spacing(7).align_y(Alignment::Center))
        .padding([2.0, 2.0])
        .style(theme::ghost_button)
        .on_press(go(IdeMsg::SideFold(key.to_string())))
        .into()
}

/// The tools, the last list of the Work side (the composer's Tools chip shows it): how the agent works on the next
/// message (Agent or Ask, each saying what it may do), then what it can use, a card each. Each of the two folds to its
/// head. The folder's files and commands are always there, with what each asks; the agent's browser and your desktop
/// are switched here; the MCP servers, the plugins, and the skills and commands are counted, a press from the Tools
/// page. Under them, the keys that reach the rest.
fn tools_panel<'a>(state: &'a State) -> El<'a> {
    use super::tools::ToolsMsg;
    let tools = &state.ide.tools;
    let mut col = Column::new().spacing(8);
    let modes_folded = state.ide.side_folds.contains("tools:modes");
    col = col.push(fold_head("How Lattice works on this", "tools:modes", modes_folded));
    if !modes_folded {
        let mut modes = Column::new().spacing(6);
        for (mode, name, what) in [
            (Mode::Agent, "Agent", "Edits the folder: each change waits for your review, and commands ask first."),
            (Mode::Ask, "Ask", "Reads and answers. Never changes a file."),
        ] {
            let on = state.mode == mode;
            modes = modes.push(
                button(column![strong(name, 13.0, if on { theme::GOLD } else { theme::TEXT }), label(what, 11.5, theme::TEXT_DIM)].spacing(2))
                    .width(Length::Fill)
                    .padding([8.0, 12.0])
                    .style(theme::pill_button(on))
                    .on_press_maybe((!state.planning()).then_some(Msg::Mode(mode))),
            );
        }
        col = col.push(modes);
        if state.planning() {
            col = col.push(label("Beside the tasks the chat plans: Ask only, so it writes nothing.", 11.5, theme::GOLD));
        }
    }
    let use_folded = state.ide.side_folds.contains("tools:use");
    col = col.push(fold_head("What it can use", "tools:use", use_folded));
    if use_folded {
        return col.push(keys_line()).into();
    }
    let servers = tools.overview.as_ref().map(|o| (o.servers.iter().filter(|s| s.enabled && !s.off).count(), o.servers.len()));
    let plugins = tools.plugins.as_ref().map(|p| (p.iter().filter(|p| p.on).count(), p.len()));
    let extras = tools.skills.as_ref().zip(tools.commands.as_ref()).map(|(s, c)| (s.skills.len(), c.commands.len()));
    let page = "Open: the Tools page, where they are added and switched.";
    let cards: Vec<El<'a>> = vec![
        tool_card(
            "F",
            theme::POSITIVE,
            "Files",
            "You review each edit".to_string(),
            "Reads and searches the folder. Each edit waits for your review.".to_string(),
            Offer::Always,
        ),
        tool_card(
            "C",
            theme::CAUTION,
            "Commands",
            "Asks first".to_string(),
            "Runs builds, tests and tools in the folder, each after you confirm it.".to_string(),
            Offer::Always,
        ),
        tool_card(
            "B",
            theme::POSITIVE,
            "Browser",
            "Asks to post, pay or delete".to_string(),
            "Its own browser, on the public web: opens pages, reads them and acts on them. It asks before it posts, pays or deletes.".to_string(),
            Offer::Switch(tools.browser.as_ref().map(|(on, _, _)| *on), |on| go(IdeMsg::Tools(ToolsMsg::BrowserSet(on)))),
        ),
        tool_card(
            "D",
            theme::CAUTION,
            "Your desktop",
            "Asks for money and accounts".to_string(),
            format!(
                "Auto mode: sees your screen and works your apps, never Alelyon, a password manager or Windows' sign-in. It asks before money and accounts. {} stops it.",
                lattice_core::desktop::STOP_KEYS
            ),
            Offer::Switch(tools.auto.map(|(on, _)| on), |on| go(IdeMsg::Tools(ToolsMsg::AutoSet(on)))),
        ),
        tool_card("M", theme::GOLD, "MCP servers", switched(servers, "server", "servers"), format!("Each server's tools ask before they run. {page}"), Offer::Page),
        tool_card("P", theme::GOLD, "Plugins", switched(plugins, "plugin", "plugins"), page.to_string(), Offer::Page),
        tool_card("S", theme::GOLD, "Skills and commands", skills_words(extras), page.to_string(), Offer::Page),
    ];
    col = col.push(Column::with_children(cards).spacing(6));
    // Allow all, in the project worked in, changes what asks.
    if let Some(p) = state.ide.project.as_deref().and_then(|id| state.ide.projects.find(id)).filter(|p| p.allow_all) {
        col = col.push(label(
            format!("Allow all is on in {}: the agent acts without asking there; only spending money asks.", ui::cut(&p.name, 32)),
            11.5,
            theme::GOLD,
        ));
    }
    col.push(keys_line()).into()
}

/// The keys that reach the rest, under the tools, and the Tools page.
fn keys_line<'a>() -> El<'a> {
    let key = |keys: &'a str, what: &'a str| -> El<'a> {
        row![container(mono(keys, 11.0, theme::TEXT)).padding([1.0, 6.0]).style(theme::chip(theme::TEXT_FAINT)), label(what, 11.5, theme::TEXT_DIM)]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
    };
    // a row that wraps, to fit the side bar
    row![
        key("/", "commands"),
        key("@", "a file"),
        key("Ctrl+Shift+P", "the palette"),
        ghost("The Tools page", Some(go(IdeMsg::Tools(super::tools::ToolsMsg::Open(None))))),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .wrap()
    .vertical_spacing(6)
    .into()
}

/// The open folder's stack and the catalog's suggestions for it: what the project uses, then each suggestion with why
/// and Ask the agent (the list's head dismisses them).
fn stack_list<'a>(state: &'a State) -> Option<El<'a>> {
    let stack = state.ide.stack.as_ref().filter(|s| !s.dismissed && !s.suggestions.is_empty())?;
    let used: Vec<&str> = stack.detected.iter().map(|(_, name)| name.as_str()).collect();
    let mut col = Column::new().spacing(10);
    col = col.push(label(format!("Uses {}", ui::cut(&used.join(" \u{00B7} "), 90)), 11.5, theme::TEXT_FAINT));
    for (i, s) in stack.suggestions.iter().enumerate() {
        let asked = stack.asked.contains(&i);
        col = col.push(
            Column::new()
                .spacing(3)
                .push(strong(format!("{} ({})", s.suggest, s.name), 12.5, theme::TEXT))
                .push(label(s.why.as_str(), 11.5, theme::TEXT_DIM))
                .push(ghost(if asked { "Asked" } else { "Ask the agent" }, (!asked && !state.sending).then(|| go(IdeMsg::StackAsk(i))))),
        );
    }
    col = col.push(label("Read from the folder's manifests only, so check each before acting.", 11.0, theme::TEXT_FAINT));
    Some(col.into())
}

/// A plan the agent proposed in Ask mode: its title and text; Approve (the chat goes on in Agent mode, where the agent
/// carries it out) and Keep planning while it waits. Once settled, what became of it.
fn plan_card<'a>(plan: &'a str, title: &'a str, text: &'a str, outcome: Option<&'a PlanOutcome>, state: &'a State) -> El<'a> {
    let tone = if outcome.is_none() { theme::GOLD } else { theme::TEXT_FAINT };
    let mut col = Column::new()
        .spacing(6)
        .push(label("Plan", 11.5, tone))
        .push(strong(title, 13.5, theme::TEXT))
        .push(
            container(label(text, 12.5, theme::TEXT))
                .padding([8.0, 10.0])
                .width(Length::Fill)
                .style(|_: &iced::Theme| solid(theme::CANVAS)),
        );
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    let settled = |words: &'static str| label(words, 11.5, theme::TEXT_FAINT).width(Length::Fill);
    match outcome {
        None => {
            line = line.push(ui::primary("Approve", (!state.sending).then(|| go(IdeMsg::PlanApprove(plan.to_string())))));
            line = line.push(ui::secondary("Keep planning", Some(go(IdeMsg::PlanKeep(plan.to_string())))));
            line = line.push(space().width(Length::Fill));
            line = line.push(label("Approving goes on in Agent mode", 11.0, theme::TEXT_FAINT));
        }
        Some(PlanOutcome::Approved) => line = line.push(settled("Approved: carried out in Agent mode")),
        Some(PlanOutcome::KeptPlanning) => line = line.push(settled("Set aside for more planning")),
        Some(PlanOutcome::Replaced) => line = line.push(settled("Replaced by a newer plan")),
    }
    col = col.push(line);
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(prompt_card(tone)).into()
}

/// A document the agent saved beside the chat (an artifact): its kind, title, version and size. Open shows it in a tab
/// of the editor (Markdown formatted); Preview opens a page or a picture in the preview browser, which has no network.
fn artifact_card<'a>(name: &'a str, title: &'a str, kind: ArtifactKind, version: u32, bytes: u64) -> El<'a> {
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    line = line.push(label(kind_glyph(kind), 12.0, theme::GOLD));
    line = line.push(strong(title, 13.0, theme::TEXT));
    line = line.push(ui::chip(format!("v{version}"), theme::TEXT_DIM));
    line = line.push(label(ui::bytes(bytes), 11.0, theme::TEXT_FAINT));
    line = line.push(space().width(Length::Fill));
    if matches!(kind, ArtifactKind::Html | ArtifactKind::Svg) {
        line = line.push(ghost("Preview", Some(go(IdeMsg::ArtifactPreview(name.to_string(), version)))));
    }
    line = line.push(ui::secondary("Open", Some(go(IdeMsg::ArtifactOpen(name.to_string(), version)))));
    container(line).padding([8.0, 12.0]).width(Length::Fill).style(prompt_card(theme::TEXT_FAINT)).into()
}

/// An artifact's kind, as one glyph.
pub fn kind_glyph(kind: ArtifactKind) -> &'static str {
    match kind {
        // TRIGRAM FOR HEAVEN: lines of text.
        ArtifactKind::Markdown => "\u{2630}",
        // POSITION INDICATOR: a page.
        ArtifactKind::Html => "\u{2316}",
        // CIRCLE WITH LEFT HALF BLACK: a picture.
        ArtifactKind::Svg => "\u{25D0}",
        ArtifactKind::Text => "\u{2261}",
    }
}

/// The agent's to-do list: each step with where it stands, the one in progress in gold; a step that needs a stack
/// decision says so, and once made, what was chosen.
fn todos_list<'a>(state: &'a State) -> Option<El<'a>> {
    let todos = &state.conversation()?.todos;
    if todos.is_empty() {
        return None;
    }
    let mut col = Column::new().spacing(6);
    for step in todos {
        let (mark, tone) = match step.status {
            TodoStatus::Done => ("\u{2713}", theme::TEXT_FAINT),
            TodoStatus::InProgress => ("\u{25B8}", theme::GOLD),
            TodoStatus::Pending => ("\u{25CB}", theme::TEXT_DIM),
        };
        let words = if step.status == TodoStatus::Done { theme::TEXT_FAINT } else { theme::TEXT };
        let mut line = Column::new().spacing(3).push(row![label(mark, 12.0, tone), label(step.content.as_str(), 12.0, words).width(Length::Fill)].spacing(8));
        // what the step waits on, or what was chosen, under it
        if let Some(area) = step.decision.as_deref() {
            let title = lattice_core::advice::area(area).map_or(area, |a| a.title.as_str());
            line = line.push(container(ui::chip(format!("decide: {title}"), theme::GOLD)).padding(padding::left(18)));
        } else if let Some(chosen) = &step.chosen {
            line = line.push(container(label(format!("\u{2192} {chosen}"), 11.0, theme::TEXT_FAINT)).padding(padding::left(18)));
        }
        col = col.push(line);
    }
    Some(col.into())
}

/// How many checklists the agents gave the reader (the chat's own not yet told done, and other agents'), and how many
/// of their steps are checked, of how many.
fn checklist_counts(state: &State) -> (usize, usize, usize) {
    let mut lists: Vec<(usize, usize)> = Vec::new();
    if let Some(c) = state.conversation() {
        for list in c.checklists.iter().filter(|list| !list.told) {
            lists.push((list.steps.iter().filter(|(_, done)| *done).count(), list.steps.len()));
        }
    }
    for list in &state.ide.outside_steps {
        lists.push((list.checked.iter().filter(|c| **c).count(), list.steps.len()));
    }
    (lists.len(), lists.iter().map(|l| l.0).sum(), lists.iter().map(|l| l.1).sum())
}

/// The checklists the agents gave the reader, the chat's own and other agents'. Each folds to its head; with more
/// than one, the Work side's head for them folds or opens them all.
fn checklists<'a>(state: &'a State) -> Option<El<'a>> {
    let mut cards = steps_bars(state);
    cards.extend(super::outside_steps::bars(state));
    (!cards.is_empty()).then(|| Column::with_children(cards).spacing(10).into())
}

/// The keys of the checklists (`chat:<id>` for the chat's own, `outside:<id>` for other agents'),
/// as [`IdeState::folded_steps`](super::IdeState) holds them.
pub(super) fn steps_keys(state: &State) -> Vec<String> {
    let chat = state.conversation().into_iter().flat_map(|c| c.checklists.iter().filter(|list| !list.told));
    chat.map(|list| format!("chat:{}", list.id))
        .chain(state.ide.outside_steps.iter().map(|list| format!("outside:{}", list.id)))
        .collect()
}

/// A checklist's head: the whole line folds the list to it or opens it again, the arrow saying which.
pub(super) fn steps_head<'a>(open: bool, key: String, parts: Column<'a, Msg>) -> El<'a> {
    let arrow = label(if open { "\u{25BE}" } else { "\u{25B8}" }, 11.0, theme::TEXT_FAINT);
    button(row![arrow, parts.width(Length::Fill)].spacing(6))
        .padding(0)
        .width(Length::Fill)
        .style(theme::ghost_button)
        .on_press(go(IdeMsg::FoldSteps(key)))
        .into()
}

/// The checklists the agent gave the reader and not yet told done, on the Work side: each step a box to check, the
/// switch that tells the agent when all are, and Done; folded, the list's head alone.
fn steps_bars<'a>(state: &'a State) -> Vec<El<'a>> {
    let Some(c) = state.conversation() else { return Vec::new() };
    c.checklists
        .iter()
        .filter(|list| !list.told)
        .map(|list| {
            let key = format!("chat:{}", list.id);
            let open = !state.ide.folded_steps.contains(&key);
            let checked = list.steps.iter().filter(|(_, done)| *done).count();
            let head = steps_head(
                open,
                key,
                column![label(list.title.as_str(), 12.5, theme::TEXT), label(format!("{checked} of {} done", list.steps.len()), 11.0, theme::TEXT_DIM)]
                    .spacing(1),
            );
            if !open {
                return card(Column::new().push(head));
            }
            let foot = steps_foot(
                checkbox(list.notify)
                    .label("Tell the agent when all are done")
                    .on_toggle({
                        let id = list.id.clone();
                        move |on| go(IdeMsg::StepsNotify(id.clone(), on))
                    })
                    .size(13)
                    .text_size(11.0)
                    .into(),
                ui::secondary("Done", Some(go(IdeMsg::StepsDone(list.id.clone())))),
            );
            let mut col = Column::new().spacing(4).push(head);
            for (n, (text, done)) in list.steps.iter().enumerate() {
                let id = list.id.clone();
                col = col.push(
                    checkbox(*done)
                        .label(format!("{}. {text}", n + 1))
                        .on_toggle(move |on| go(IdeMsg::StepCheck(id.clone(), n as u32, on)))
                        .size(14)
                        .text_size(12.0),
                );
            }
            card(col.push(foot))
        })
        .collect()
}

/// A checklist's foot: the switch that tells the agent when all are done, and Done under it at the end, to fit the
/// side bar's width.
pub(super) fn steps_foot<'a>(notify: El<'a>, done: El<'a>) -> El<'a> {
    column![notify, row![space().width(Length::Fill), done]].spacing(4).into()
}

/// A checklist on the Work side: its head and steps, bare.
pub(super) fn card(col: Column<'_, Msg>) -> El<'_> {
    container(col.spacing(4)).padding([2.0, 0.0]).width(Length::Fill).into()
}

/// The agent's changes waiting for review.
fn waiting_changes(state: &State) -> Vec<&lattice_protocol::conversation::ChangeView> {
    let Some(set) = state.changes.as_ref() else { return Vec::new() };
    set.changes.iter().filter(|c| matches!(c.state, ChangeState::Pending | ChangeState::Rebased)).collect()
}

/// The changes waiting for review, a file to a line: its name (its diff, a press away; its path, its tip) and its
/// counts, and under it whether it was rebased or is an authority file; then Undo all and Keep all.
fn changes_list<'a>(state: &'a State) -> Option<El<'a>> {
    let set = state.changes.as_ref()?;
    let waiting = waiting_changes(state);
    if waiting.is_empty() {
        return None;
    }
    let mut col = Column::new().spacing(4);
    for change in waiting {
        let line = row![
            ui::tip(link(super::file_name(&change.path), Msg::ShowDiff(change.id.clone())), change.path.as_str()),
            space().width(Length::Fill),
            counts(change.added, change.removed),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let mut entry = Column::new().spacing(1).push(line);
        let marks: Vec<&str> = [(matches!(change.state, ChangeState::Rebased), "rebased"), (change.authority, "authority file")]
            .into_iter()
            .filter_map(|(on, words)| on.then_some(words))
            .collect();
        if !marks.is_empty() {
            entry = entry.push(label(marks.join(" \u{00B7} "), 11.0, if change.authority { theme::CAUTION } else { theme::TEXT_DIM }));
        }
        col = col.push(entry);
    }
    let free = !state.reviewing && !set.command_running;
    col = col.push(
        row![
            space().width(Length::Fill),
            ghost("Undo all", (!state.reviewing).then_some(Msg::Review(ReviewOp::UndoAll { note: None }))),
            ui::primary("Keep all", free.then_some(Msg::Review(ReviewOp::KeepAll))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    Some(col.into())
}

/// The composer, a pill over the page: the box, and under it what goes with the message (images, the mode, where it
/// works, the file in view, the model) as chips that wrap, with Stop and Send at the end. Under the pill, a line on
/// where the message goes when that needs saying; while a turn runs, a box to steer it.
fn composer<'a>(state: &'a State, phase: f32) -> El<'a> {
    let running = state.conversation().is_some_and(|c| c.running);
    let mut pill = Column::new().spacing(6);
    // An earlier message being edited: Send replaces it.
    if state.ide.editing_turn.is_some() {
        pill = pill.push(
            row![
                label("Editing a message: Send replaces it and what came after.", 11.5, theme::GOLD),
                space().width(Length::Fill),
                ghost("Cancel", Some(go(IdeMsg::CancelEditTurn))),
            ]
            .align_y(Alignment::Center),
        );
    }
    // Addressed to one topic (`@topic`): it goes there, until taken off.
    if let Some(id) = &state.ide.groups.to_topic {
        let title = state.list.as_ref().and_then(|l| l.conversations.iter().find(|c| c.id == *id)).map_or("a topic", |c| c.title.as_str());
        pill = pill.push(
            row![
                label(format!("To \u{201c}{}\u{201d}: only this topic gets the message.", ui::cut(title, 48)), 11.5, theme::GOLD),
                space().width(Length::Fill),
                ghost("Not to it", Some(go(IdeMsg::Groups(super::groups::GroupsMsg::To(None))))),
            ]
            .align_y(Alignment::Center),
        );
    }
    // The images attached to this message, each with a way to take it off.
    if !state.ide.attached.is_empty() {
        let mut images = Row::new().spacing(6);
        for (at, attached) in state.ide.attached.iter().enumerate() {
            images = images.push(
                button(label(format!("\u{1F5BC} {}  \u{00D7}", ui::cut(&attached.name, 22)), 11.5, theme::TEXT_DIM))
                    .padding([3.0, 10.0])
                    .style(theme::pill_button(false))
                    .on_press(go(IdeMsg::Unattach(at))),
            );
        }
        pill = pill.push(images.wrap().vertical_spacing(6));
    }
    let placeholder = if state.mode == Mode::Agent {
        "Tell the agent what to change, or ask about the code…"
    } else {
        "Ask about the code (Ask mode: the agent only reads)…"
    };
    // While the commands list is open, it takes Enter, Tab, Up, Down and Esc.
    let listing = super::slash::open(state);
    // The files list, likewise, when an `@` is being typed (and no command is).
    let mentioning = !listing && super::mention::open(state);
    let editing = state.ide.editing_turn.is_some();
    // Up in an empty box edits the last message sent, as in the chat apps
    let last_mine = (!running && !editing && state.composer.text().trim().is_empty())
        .then(|| state.conversation().and_then(|c| c.turns.iter().rev().find(|t| t.role == Role::User && !t.id.is_empty())).map(|t| t.id.clone()))
        .flatten();
    pill = pill.push(
        text_editor(&state.composer)
            .id(super::composer_id())
            .placeholder(placeholder)
            .on_action(Msg::Compose)
            .min_height(46)
            .max_height(240)
            .padding([6.0, 4.0])
            .size(14.5)
            .key_binding(move |press| composer_keys(press, running, editing, last_mine.clone(), listing, mentioning))
            .style(|_, _| text_editor::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                placeholder: theme::TEXT_FAINT,
                value: theme::TEXT,
                selection: theme::with_alpha(theme::GOLD, 0.30),
            }),
    );
    // What goes with the message: images, the mode, where it works, the file in view.
    let mut with = Row::new().spacing(6).align_y(Alignment::Center);
    if state.ide.attached.len() < super::attach::MAX {
        with = with.push(ui::tip(
            button(label("+", 15.0, theme::TEXT))
                .padding([1.0, 11.0])
                .style(theme::pill_button(false))
                .on_press_maybe((!state.ide.picking).then(|| go(IdeMsg::AttachImages))),
            "Attach images (up to four)",
        ));
    }
    // Beside the grid the chat plans: Ask only, so it reads and answers and writes nothing.
    if state.planning() {
        with = with.push(label("Ask \u{00B7} plans and answers, writes nothing", 12.0, theme::GOLD));
    }
    for (mode, words) in [(Mode::Agent, "Agent"), (Mode::Ask, "Ask")].into_iter().filter(|_| !state.planning()) {
        let on = state.mode == mode;
        let what = match mode {
            Mode::Agent => "Agent: may stage edits for you to review, and ask to run commands",
            Mode::Ask => "Ask: reads and answers; writes nothing",
        };
        with = with.push(ui::tip(
            button(label(words, 12.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([4.0, 12.0])
                .style(theme::pill_button(on))
                .on_press(Msg::Mode(mode)),
            what,
        ));
    }
    // The tools: how many are on; a press shows them on the side bar's Work view, or folds them there.
    let tools_open = state.ide.side_open && state.ide.side == Side::Work && !state.ide.work_folded(WorkList::Tools);
    let mut tools_chip = Row::new().spacing(6).align_y(Alignment::Center).push(label("Tools", 12.0, if tools_open { theme::GOLD } else { theme::TEXT_DIM }));
    if let Some(n) = tools_on(&state.ide.tools) {
        tools_chip = tools_chip.push(label(format!("{n} on"), 11.5, theme::TEXT_FAINT));
    }
    with = with.push(ui::tip(
        button(tools_chip).padding([4.0, 12.0]).style(theme::pill_button(tools_open)).on_press(go(if tools_open {
            IdeMsg::SideFold(WorkList::Tools.key().to_string())
        } else {
            IdeMsg::ShowWork(WorkList::Tools)
        })),
        "What the agent can use, on the side bar: its browser, your desktop, MCP servers, plugins, skills and commands",
    ));
    for place in places(state) {
        with = with.push(place);
    }
    if let Some(path) = state.ide.active_tab().and_then(|t| t.path()) {
        with = with.push(ui::tip(
            button(label(format!("@ {}", ui::cut(super::file_name(path), 22)), 11.5, theme::TEXT_DIM))
                .padding([4.0, 11.0])
                .style(theme::pill_button(false))
                .on_press(go(IdeMsg::Mention)),
            "Mention the file in view",
        ));
    }
    // The model, last of the chips: its list is as wide as its longest label, so it wraps with them rather than
    // crowding them.
    let labels: Vec<String> = state.choices.iter().map(choice_label).collect();
    let chosen = state.choices.iter().find(|c| c.id == state.choice).map(choice_label);
    with = with.push(
        // A model that is not ready says why when chosen (`ChoiceLabel`), and stays unchosen.
        pick_list(labels, chosen, Msg::ChoiceLabel)
            .placeholder("Model")
            .text_size(12.0)
            .padding([4.0, 12.0])
            .style(theme::pill_picker)
            .menu_style(theme::picker_menu),
    );
    // Stop and Send, at the line's end.
    let mut end = Row::new().spacing(6).align_y(Alignment::Center);
    if state.sending {
        end = end.push(crate::spinner::spinner(phase, 14.0));
    }
    if running {
        end = end.push(ui::tip(
            button(label("\u{25A0}", 12.0, theme::TEXT)).padding([8.0, 12.0]).style(round(theme::secondary_button)).on_press(Msg::Stop),
            "Stop the turn (Esc)",
        ));
    }
    let empty = state.composer.text().trim().is_empty();
    let can_send = !state.sending && !empty && state.services_ready();
    let send_tip = if state.sending {
        "Sending\u{2026}"
    } else if !state.services_ready() {
        "Lattice is starting: send once it is ready."
    } else if empty {
        "Type a message first. Enter sends; Shift+Enter is a new line."
    } else if running {
        "Queue it: sent when the running turn ends (Enter)"
    } else {
        "Send (Enter). Shift+Enter is a new line."
    };
    end = end.push(ui::tip(
        // no colour of its own: the button's, gold's black words when it can send and faint ones when it cannot
        button(text(if running { "Queue \u{2191}" } else { "\u{2191}" }).size(15.0).font(fonts().ui_strong))
            .padding([5.0, 13.0])
            .style(round(theme::primary_button))
            .on_press_maybe(can_send.then_some(Msg::Send)),
        send_tip,
    ));
    pill = pill.push(row![container(with.wrap().vertical_spacing(6)).width(Length::Fill), end].spacing(8).align_y(Alignment::End));
    let mut col = Column::new().spacing(6);
    // The commands list, over the composer.
    if let Some(menu) = super::slash::menu(state) {
        col = col.push(menu);
    } else if let Some(menu) = super::mention::menu(state) {
        col = col.push(menu);
    }
    col = col.push(container(pill).padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 16.0 }).width(Length::Fill).style(theme::composer));
    if let Some(line) = under_composer(state) {
        col = col.push(line);
    }
    if running {
        col = col.push(
            row![
                text_input("Steer the running turn…", &state.steer).on_input(Msg::Steer).on_submit(Msg::SendSteer).padding([6.0, 12.0]).size(12.5).style(ui::input_style),
                ghost("Steer", (!state.steer.trim().is_empty()).then_some(Msg::SendSteer)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    container(col).padding(Padding { top: 10.0, bottom: 14.0, left: 16.0, right: 16.0 }).into()
}

/// The line under the composer, when there is something to say: with no folder, what the agent can reach; the model
/// chosen (why it cannot run, that it leaves this computer, or where it runs); and how the last message was routed.
/// The context meter's words for `used`: how much of the budget the last turn used, what was left out, and /compact.
pub(in crate::lattice) fn context_words(used: &lattice_core::convo::fork::ContextUse) -> String {
    let mut words = format!("Context {}% full at the last turn", used.percent());
    if used.left_out > 0 {
        words.push_str(&format!(
            ", {} earlier exchange{} left out",
            used.left_out,
            if used.left_out == 1 { "" } else { "s" }
        ));
    }
    if used.percent() >= 60 || used.left_out > 0 {
        words.push_str(". /compact [what to keep] summarises it now");
    }
    words
}

fn context_line<'a>(state: &'a State) -> Option<El<'a>> {
    let open = state.open.as_ref()?;
    let used = state.services.as_ref()?.chat.context_use(open.id())?;
    let tone = if used.percent() >= 85 { theme::CAUTION } else { theme::TEXT_FAINT };
    Some(label(context_words(&used), 11.0, tone).into())
}

fn under_composer<'a>(state: &'a State) -> Option<El<'a>> {
    let mut parts: Vec<El<'a>> = Vec::new();
    // How full the open chat's context was at its last turn, and /compact when it fills.
    if let Some(line) = context_line(state) {
        parts.push(line);
    }
    if attached_folder(state).is_none() {
        if state.ide.tools.auto.is_some_and(|(on, _)| on) {
            parts.push(
                label(
                    format!(
                        "No folder: in Agent mode the agent uses your whole desktop (auto mode; {} stops it).",
                        lattice_core::desktop::STOP_KEYS
                    ),
                    11.0,
                    theme::GOLD,
                )
                .into(),
            );
        } else {
            let browser_on = state.ide.tools.browser.as_ref().is_some_and(|(on, _, _)| *on);
            let words = if browser_on {
                "No folder: in Agent mode the agent uses its browser and your own MCP servers (Connections)."
            } else {
                "No folder: the agent can only answer."
            };
            parts.push(label(words, 11.0, theme::TEXT_FAINT).into());
        }
    }
    if let Some(choice) = state.choices.iter().find(|c| c.id == state.choice) {
        let (words, color) = match (&choice.refusal, choice.locality) {
            (Some(refusal), _) => (refusal.clone(), theme::CAUTION),
            (None, Locality::Remote) => ("Leaves this computer: you confirm the first send.".to_string(), theme::CAUTION),
            (None, Locality::Local) => (choice.detail.clone(), theme::TEXT_FAINT),
        };
        if !words.is_empty() {
            parts.push(label(ui::cut(&words, 120), 11.0, color).into());
        }
    }
    if let Some(routed) = state.routed.as_deref().filter(|r| !r.is_empty()) {
        parts.push(label(routed, 11.0, theme::GOLD_DIM).into());
    }
    if parts.is_empty() {
        return None;
    }
    Some(container(Row::with_children(parts).spacing(12).wrap().vertical_spacing(2)).padding(padding::left(16).right(16)).into())
}

const NOT_READY: &str = " (not ready)";

/// A model choice as the picker lists it.
pub fn choice_label(c: &lattice_protocol::chat::ChatChoice) -> String {
    if c.ready { c.label.clone() } else { format!("{}{NOT_READY}", c.label) }
}

/// The composer's keys: Enter sends, Shift+Enter breaks the line, Esc stops a running turn (or, with none running,
/// leaves a message being edited as it was); Up in an empty box edits the last message sent (`last_mine`). While the
/// commands list is open (`listing`), Enter and Tab choose, Up and Down move, and Esc closes it.
fn composer_keys(
    press: text_editor::KeyPress,
    running: bool,
    editing: bool,
    last_mine: Option<String>,
    listing: bool,
    mentioning: bool,
) -> Option<text_editor::Binding<Msg>> {
    use super::mention::MentionMsg;
    use super::slash::SlashMsg;
    use iced::keyboard::Key;
    use iced::keyboard::key::Named;
    use text_editor::Binding;
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    let slash = |msg: SlashMsg| Some(Binding::Custom(Msg::Ide(IdeMsg::Slash(msg))));
    if listing {
        match press.key.as_ref() {
            Key::Named(Named::Enter) if !press.modifiers.shift() => return slash(SlashMsg::Pick(None)),
            Key::Named(Named::Tab) => return slash(SlashMsg::Pick(None)),
            Key::Named(Named::ArrowUp) => return slash(SlashMsg::Move(-1)),
            Key::Named(Named::ArrowDown) => return slash(SlashMsg::Move(1)),
            Key::Named(Named::Escape) => return slash(SlashMsg::Close),
            _ => {}
        }
    }
    let mention = |msg: MentionMsg| Some(Binding::Custom(Msg::Ide(IdeMsg::MentionList(msg))));
    if mentioning {
        match press.key.as_ref() {
            Key::Named(Named::Enter) if !press.modifiers.shift() => return mention(MentionMsg::Pick(None)),
            Key::Named(Named::Tab) => return mention(MentionMsg::Pick(None)),
            Key::Named(Named::ArrowUp) => return mention(MentionMsg::Move(-1)),
            Key::Named(Named::ArrowDown) => return mention(MentionMsg::Move(1)),
            Key::Named(Named::Escape) => return mention(MentionMsg::Close),
            _ => {}
        }
    }
    match press.key.as_ref() {
        Key::Named(Named::Enter) if !press.modifiers.shift() => Some(Binding::Custom(Msg::Send)),
        Key::Named(Named::Escape) if running => Some(Binding::Custom(Msg::Stop)),
        // Esc while a message is being edited leaves it as it was, as the Cancel beside it does.
        Key::Named(Named::Escape) if editing => Some(Binding::Custom(Msg::Ide(IdeMsg::CancelEditTurn))),
        Key::Named(Named::ArrowUp) if last_mine.is_some() && press.modifiers.is_empty() => {
            last_mine.map(|turn| Binding::Custom(Msg::Ide(IdeMsg::EditTurn(turn))))
        }
        _ => Binding::from_key_press(press),
    }
}

// --------------------------------------------------------------------------------------------- the Chats view

/// Every chat, grouped by the folder it works in, the running and those waiting for the reader marked: the agent
/// manager's list.
/// A folder's chats shown before its Show more.
pub const CHATS_SHOWN: usize = 8;

/// A place at the top of the Chats list: its glyph, its name, what it opens.
fn place<'a>(glyph: &'a str, words: &'a str, on: bool, msg: Msg) -> El<'a> {
    button(row![label(glyph, 13.0, if on { theme::GOLD } else { theme::TEXT_DIM }).width(18), label(words, 12.5, if on { theme::GOLD } else { theme::TEXT })].spacing(8).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([4.0, 10.0])
        .style(theme::list_row(on))
        .on_press(msg)
        .into()
}

pub fn chats<'a>(state: &'a State, phase: f32) -> El<'a> {
    let ide = &state.ide;
    let mut col = Column::new().spacing(2);
    // Search, then the places.
    col = col.push(
        container(
            text_input("Search (Enter: what was said)", &ide.chat_search)
                .on_input(|t| go(IdeMsg::ChatSearch(t)))
                .on_submit(go(IdeMsg::ChatSearchSaid))
                .padding([5.0, 8.0])
                .size(12.5)
                .style(ui::input_style),
        )
            .padding(Padding { top: 8.0, right: 10.0, bottom: 6.0, left: 10.0 }),
    );
    col = col.push(place("+", "New", false, Msg::New));
    col = col.push(place("\u{25A3}", "Projects", ide.projects_open, go(IdeMsg::ProjectsOpen(!ide.projects_open))));
    if ide.projects_open {
        col = col.push(container(super::projects::picker(state)).padding(Padding { top: 2.0, right: 10.0, bottom: 6.0, left: 36.0 }));
    }
    col = col.push(place(Side::Tools.glyph(), "Tools", false, go(IdeMsg::Side(Side::Tools))));
    // Every chat, or those working, or those waiting for you.
    let mut filters = Row::new().spacing(4).padding(Padding { top: 10.0, right: 10.0, bottom: 2.0, left: 10.0 });
    for (f, words) in [(ChatFilter::All, "All"), (ChatFilter::Running, "Running"), (ChatFilter::NeedsYou, "Needs you")] {
        let on = ide.chat_filter == f;
        filters = filters.push(
            button(label(words, 11.5, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([2.0, 8.0])
                .style(theme::segment_button(on))
                .on_press(go(IdeMsg::ChatFilter(f))),
        );
    }
    col = col.push(filters);
    // What was said in the sessions that matches the search: each opens its session where it stopped.
    if let Some((query, hits)) = ide.chat_hits.as_ref().filter(|(q, _)| q == ide.chat_search.trim()) {
        col = col.push(said_hits(query, hits));
    }
    let mut rows = Column::new().spacing(1);
    match &state.list {
        None => rows = rows.push(container(ui::working(phase, "Reading the chats…")).padding(12)),
        Some(list) => {
            if list.index_unreadable {
                rows = rows.push(container(ui::notice("The chat list could not be read; nothing was written over it.", theme::CAUTION)).padding(8));
            }
            let search = ide.chat_search.trim().to_lowercase();
            let shown: Vec<_> = list
                .conversations
                .iter()
                .filter(|c| match ide.chat_filter {
                    ChatFilter::All => true,
                    ChatFilter::Running => c.running,
                    ChatFilter::NeedsYou => c.needs_you || state.calling.contains(&c.id),
                })
                .filter(|c| super::projects::shows(state, &c.id))
                .filter(|c| search.is_empty() || c.title.to_lowercase().contains(&search))
                .collect();
            if shown.is_empty() {
                rows = rows.push(container(note(if !search.is_empty() {
                    "No chat's title has that."
                } else {
                    match ide.chat_filter {
                        ChatFilter::All if ide.project.is_some() => "No chats in this project yet: a new chat now joins it.",
                        ChatFilter::All => "No chats yet.",
                        ChatFilter::Running => "No chat is working now.",
                        ChatFilter::NeedsYou => "Nothing is waiting for you.",
                    }
                }))
                .padding(12));
            }
            // By folder, in the order each folder's newest chat comes; the chats with no folder last, as Other.
            let mut groups: Vec<(Option<&lattice_protocol::conversation::WorkspaceBadge>, Vec<&lattice_protocol::conversation::ConversationSummary>)> =
                Vec::new();
            for c in shown {
                let key = c.workspace.as_ref().map(|w| w.id.as_str());
                match groups.iter_mut().find(|(w, _)| w.map(|w| w.id.as_str()) == key) {
                    Some((_, v)) => v.push(c),
                    None => groups.push((c.workspace.as_ref(), vec![c])),
                }
            }
            groups.sort_by_key(|(w, _)| w.is_none());
            for (folder, items) in groups {
                let name = folder.map(|w| w.name.clone()).unwrap_or_else(|| "Other".to_string());
                let key = folder.map(|w| w.id.clone()).unwrap_or_default();
                let mut head = Row::new().spacing(4).align_y(Alignment::Center).padding(Padding { top: 10.0, bottom: 2.0, left: 12.0, right: 8.0 });
                head = head.push(label(name, 11.5, theme::TEXT_FAINT).width(Length::Fill));
                if let Some(w) = folder {
                    head = head.push(icon("+", "A new chat in this folder", Some(go(IdeMsg::NewIn(w.path.clone())))));
                }
                rows = rows.push(head);
                let all = ide.chat_more.contains(&key) || !search.is_empty();
                let hidden = items.len().saturating_sub(CHATS_SHOWN);
                for c in items.iter().take(if all { items.len() } else { CHATS_SHOWN }) {
                    rows = rows.push(chat_row(state, c, phase));
                }
                if hidden > 0 && search.is_empty() {
                    let words = if all { "Show fewer".to_string() } else { format!("Show {hidden} more") };
                    rows = rows.push(
                        button(label(words, 11.5, theme::TEXT_FAINT))
                            .padding(Padding { top: 3.0, bottom: 3.0, left: 34.0, right: 8.0 })
                            .style(theme::ghost_button)
                            .on_press(go(IdeMsg::ChatMore(key, !all))),
                    );
                }
            }
            if !list.archived.is_empty() {
                rows = rows.push(container(note(format!("{} archived (never deleted)", list.archived.len()))).padding(12));
            }
        }
    }
    col = col.push(scrollable(rows).height(Length::Fill).style(theme::scrollbars));
    col.into()
}

/// One chat's row: what it is doing (working, waiting for you, or still), its title, how long ago, and its grid button.
fn chat_row<'a>(state: &'a State, c: &'a lattice_protocol::conversation::ConversationSummary, phase: f32) -> El<'a> {
    let open = state.conversation().is_some_and(|o| o.id() == c.id);
    let needs = c.needs_you || state.calling.contains(&c.id);
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    line = line.push(if c.running {
        crate::spinner::spinner(phase, 10.0)
    } else if needs {
        dot(theme::GOLD)
    } else {
        dot(theme::LINE)
    });
    let title = if c.title.is_empty() { "Untitled" } else { c.title.as_str() };
    line = line.push(container(label(title, 12.5, if open { theme::GOLD } else { theme::TEXT }).wrapping(iced::widget::text::Wrapping::None)).width(Length::Fill).clip(true));
    line = line.push(label(super::ago(crate::utc::now() - c.updated), 10.5, theme::TEXT_FAINT));
    let in_grid = state.ide.grid.holds(&c.id);
    let grid_msg = if in_grid { super::grid::GridMsg::Remove(c.id.clone()) } else { super::grid::GridMsg::Add(c.id.clone()) };
    let grid_tip = if in_grid { "Take it out of the grid" } else { "Put it in the grid" };
    Row::new()
        .align_y(Alignment::Center)
        .push(container(button(line).width(Length::Fill).padding([5.0, 10.0]).style(theme::list_row(open)).on_press(Msg::Open(c.id.clone()))).width(Length::Fill))
        .push(icon(if in_grid { "\u{25A3}" } else { "\u{25A6}" }, grid_tip, Some(super::grid::go(grid_msg))))
        .into()
}

// --------------------------------------------------------------------------------------------- the bottom panel

/// The open chat's commands: each with how it ended, and the one chosen with its output.
pub fn commands<'a>(state: &'a State, phase: f32) -> El<'a> {
    let Some(c) = state.conversation() else {
        return container(note("Open a chat: the commands its agent ran, after your approval, are listed here with their output.")).padding(14).into();
    };
    let calls: Vec<&Tool> = c
        .activity
        .iter()
        .flat_map(|a| a.items.iter())
        .filter_map(|i| match i {
            Item::Tool(t) if t.tool == "run_command" => Some(t),
            _ => None,
        })
        .collect();
    if calls.is_empty() {
        return container(note("This chat's agent has run no command. Each one asks you first, and its output streams here.")).padding(14).into();
    }
    let mut list = Column::new().spacing(1);
    for t in calls.iter().rev() {
        let on = state.ide.command.as_deref() == Some(t.call.as_str());
        let (words, color) = match (&t.exit, &t.progress) {
            (Some((Some(0), ms, ExitReason::Exited)), _) => (format!("exit 0 · {:.1} s", *ms as f64 / 1000.0), theme::POSITIVE),
            (Some((code, _, ExitReason::Exited)), _) => (format!("exit {}", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())), theme::CAUTION),
            (Some((_, _, ExitReason::Stopped)), _) => ("stopped".to_string(), theme::TEXT_FAINT),
            (Some((_, _, ExitReason::TimedOut)), _) => ("timed out".to_string(), theme::CAUTION),
            (Some((_, _, ExitReason::Failed)), _) => ("could not start".to_string(), theme::CAUTION),
            (None, Some(_)) => ("running".to_string(), theme::GOLD),
            (None, None) => ("waiting".to_string(), theme::TEXT_FAINT),
        };
        let what = subject(&t.tool, &t.summary, t.target.as_deref());
        list = list.push(
            button(column![mono(ui::cut(what, 44), 11.5, if on { theme::GOLD } else { theme::TEXT }), label(words, 10.5, color)].spacing(1))
                .width(Length::Fill)
                .padding([4.0, 10.0])
                .style(theme::list_row(on))
                .on_press(go(IdeMsg::Command(t.call.clone()))),
        );
    }
    let left = container(scrollable(list).height(Length::Fill).style(theme::scrollbars)).width(280).height(Length::Fill);
    let right: El<'a> = match (&state.ide.command, &state.ide.command_lines) {
        (None, _) => container(note("Choose a command to see its output.")).padding(12).into(),
        (Some(_), None) => container(ui::working(phase, "Reading its output…")).padding(12).into(),
        (Some(call), Some(lines)) => {
            let mut out = Column::new().spacing(0).padding([6.0, 12.0]);
            for line in &lines.lines {
                out = out.push(mono(line.as_str(), 12.0, theme::TEXT));
            }
            if lines.lines.is_empty() {
                out = out.push(note("It wrote nothing."));
            }
            let label_of = calls.iter().find(|t| &t.call == call).map(|t| subject(&t.tool, &t.summary, t.target.as_deref()).to_string()).unwrap_or_default();
            column![
                container(
                    row![
                        label(format!("lines {} of {}", lines.lines.len(), lines.total), 11.0, theme::TEXT_FAINT),
                        space().width(Length::Fill),
                        ghost("Open in the editor", Some(go(IdeMsg::Output(call.clone(), ui::cut(&label_of, 60))))),
                    ]
                    .align_y(Alignment::Center)
                )
                .padding([2.0, 10.0]),
                scrollable(out)
                    .direction(scrollable::Direction::Both { vertical: scrollable::Scrollbar::new(), horizontal: scrollable::Scrollbar::new() })
                    .height(Length::Fill)
                    .width(Length::Fill)
                    .style(theme::scrollbars),
            ]
            .into()
        }
    };
    row![left, container(space().width(1)).height(Length::Fill).style(theme::line), container(right).width(Length::Fill).height(Length::Fill).style(|_| solid(theme::CANVAS))].into()
}

/// The sessions in which what was searched was said, best first: each line its title, when, and the words around the
/// match; a click opens the session where it stopped.
fn said_hits<'a>(query: &'a str, hits: &'a [lattice_core::convo::recall::Hit]) -> El<'a> {
    let mut col = Column::new().spacing(2).padding(Padding { top: 8.0, right: 10.0, bottom: 4.0, left: 10.0 });
    col = col.push(label(
        if hits.is_empty() { format!("Nothing said in a session matches \u{201c}{}\u{201d}.", ui::cut(query, 40)) } else { "Said in sessions".to_string() },
        11.0,
        theme::TEXT_FAINT,
    ));
    for hit in hits {
        let title = if hit.title.is_empty() { "Untitled" } else { hit.title.as_str() };
        let line = Column::new()
            .spacing(1)
            .push(
                Row::new()
                    .spacing(6)
                    .push(container(label(ui::cut(title, 40), 12.0, theme::TEXT)).width(Length::Fill).clip(true))
                    .push(label(super::ago(crate::utc::now() - hit.ts), 10.5, theme::TEXT_FAINT)),
            )
            .push(label(ui::cut(&hit.snippet, 120), 11.0, theme::TEXT_DIM));
        col = col.push(
            button(line).width(Length::Fill).padding([4.0, 8.0]).style(theme::list_row(false)).on_press(Msg::Open(hit.conversation.clone())),
        );
    }
    col.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Key, Modifiers, key};

    fn press(named: key::Named, modifiers: Modifiers) -> text_editor::KeyPress {
        text_editor::KeyPress {
            key: Key::Named(named),
            modified_key: Key::Named(named),
            physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    fn custom(b: Option<text_editor::Binding<Msg>>) -> Option<String> {
        match b {
            Some(text_editor::Binding::Custom(m)) => Some(format!("{m:?}")),
            _ => None,
        }
    }

    #[test]
    fn up_in_an_empty_box_edits_the_last_message_and_esc_leaves_an_edit() {
        let up = |last: Option<&str>, listing: bool| {
            custom(composer_keys(press(key::Named::ArrowUp, Modifiers::empty()), false, false, last.map(str::to_string), listing, false))
        };
        assert!(up(Some("t7"), false).is_some_and(|m| m.contains("EditTurn") && m.contains("t7")));
        assert_eq!(up(None, false), None, "a box with words in it (or nothing sent) keeps Up for the caret");
        assert!(up(Some("t7"), true).is_some_and(|m| m.contains("Slash")), "the commands list takes Up first");
        let shift_up = custom(composer_keys(press(key::Named::ArrowUp, Modifiers::SHIFT), false, false, Some("t7".into()), false, false));
        assert_eq!(shift_up, None, "Shift+Up selects, as ever");
        let esc = |running, editing| custom(composer_keys(press(key::Named::Escape, Modifiers::empty()), running, editing, None, false, false));
        assert!(esc(true, true).is_some_and(|m| m == "Stop"), "a running turn stops first");
        assert!(esc(false, true).is_some_and(|m| m.contains("CancelEditTurn")));
    }
    fn tool(call: &str, name: &str, target: &str, exit: Option<(Option<i32>, u64, ExitReason)>) -> Item {
        Item::Tool(Tool {
            call: call.into(),
            tool: name.into(),
            summary: format!("{name} {target}"),
            target: Some(target.into()),
            output: None,
            withheld: false,
            truncated: false,
            progress: None,
            exit,
            effect: Vec::new(),
        })
    }

    #[test]
    fn a_turns_folded_line_counts_its_steps_files_and_failed_commands() {
        let items = vec![
            tool("1", "read_file", "src/a.rs", None),
            tool("2", "edit_file", "src/a.rs", None),
            tool("3", "write_file", "src\\b.rs", None),
            tool("4", "run_command", "cargo test", Some((Some(0), 900, ExitReason::Exited))),
            tool("5", "run_command", "cargo build", Some((Some(101), 300, ExitReason::Exited))),
            Item::Staged { change: "c1".into(), path: "src/b.rs".into(), added: 3, removed: 1, authority: false, result: None },
            Item::Notice("a note".into()),
        ];
        let tally = Tally::of(&items);
        assert_eq!(tally, Tally { steps: 5, files: 2, commands: 2, failed: 1 }, "a file edited and staged counts once, by either slash");
        assert_eq!(tally.words(), "5 steps \u{00B7} 2 files edited \u{00B7} 2 commands, 1 failed");
        assert_eq!(Tally::of(&items[..1]).words(), "1 step", "nothing edited or run: steps alone");
    }

    #[test]
    fn what_waits_for_the_reader_or_must_be_seen_shows_however_the_work_is_folded() {
        let question = |answered| Item::Question { call: "q".into(), text: "Which?".into(), options: Vec::new(), answered };
        assert!(pinned(&question(false)), "a question not answered waits for the reader");
        assert!(!pinned(&question(true)), "an answered one folds with the work");
        assert!(pinned(&Item::Error("boom".into())));
        assert!(pinned(&Item::Conflict { path: "a".into(), reason: "moved".into() }));
        assert!(!pinned(&tool("1", "read_file", "a", None)));
        assert!(!pinned(&Item::Notice("n".into())) && !pinned(&Item::Steered("go".into())));
    }

    #[test]
    fn a_turns_work_is_open_while_it_runs_folded_after_and_as_the_reader_left_it() {
        let mut folds = std::collections::HashMap::new();
        assert_eq!(fold_of(&folds, "t1", true), Fold::Open);
        assert_eq!(fold_of(&folds, "t1", false), Fold::Folded);
        folds.insert("t1".to_string(), Fold::Folded);
        assert_eq!(fold_of(&folds, "t1", true), Fold::Folded, "folded by the reader while it runs, it stays folded");
        folds.insert("t1".to_string(), Fold::All);
        assert_eq!(fold_of(&folds, "t1", false), Fold::All, "opened by the reader once it ended, it stays open");
        assert_eq!(fold_of(&folds, "t2", false), Fold::Folded, "another turn keeps its own");
    }
    #[test]
    fn the_empty_chat_greets_by_the_hour_and_the_first_word_of_the_name() {
        assert_eq!(greeting(5), "Good morning");
        assert_eq!(greeting(11), "Good morning");
        assert_eq!(greeting(12), "Good afternoon");
        assert_eq!(greeting(17), "Good afternoon");
        assert_eq!(greeting(18), "Good evening");
        assert_eq!(greeting(2), "Good evening", "the small hours keep the evening's greeting");
        assert_eq!(first_name("Ada Lovelace"), Some("Ada"));
        assert_eq!(first_name("  Ada  "), Some("Ada"));
        assert_eq!(first_name("   "), None, "a blank name greets no one by name");
    }

    #[test]
    fn a_tools_card_counts_in_words_that_agree_with_their_number() {
        assert_eq!(switched(None, "server", "servers"), "Reading\u{2026}");
        assert_eq!(switched(Some((0, 0)), "server", "servers"), "No servers yet: the Tools page adds them.");
        assert_eq!(switched(Some((1, 1)), "server", "servers"), "1 of 1 server on.");
        assert_eq!(switched(Some((1, 3)), "plugin", "plugins"), "1 of 3 plugins on.");
        assert_eq!(skills_words(Some((0, 1))), "0 skills and 1 command: / lists the commands.");
        assert_eq!(skills_words(Some((1, 2))), "1 skill and 2 commands: / lists the commands.");
        assert_eq!(skills_words(None), "Reading\u{2026}");
    }

    #[test]
    fn the_tools_chip_counts_what_is_switched_on_once_anything_is_read() {
        let mut tools = super::super::tools::Tools::default();
        assert_eq!(tools_on(&tools), None, "nothing read yet: no count");
        tools.auto = Some((false, false));
        assert_eq!(tools_on(&tools), Some(0));
        tools.auto = Some((true, true));
        assert_eq!(tools_on(&tools), Some(1));
    }

    /// Chat `id` as read, with the to-do list `todos`.
    fn chat_with(id: &str, todos: Vec<lattice_protocol::conversation::TodoItem>) -> Conversation {
        use lattice_protocol::conversation::{ConversationSummary, Origin, Snapshot};
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
        let mut c = Conversation::from_snapshot(snapshot);
        c.todos = todos;
        c
    }

    #[test]
    fn what_an_agent_works_on_says_the_step_it_is_on_and_the_work_side_marks_what_waits() {
        use lattice_protocol::conversation::TodoItem;
        let step = |content: &str, status| TodoItem { content: content.into(), status, decision: None, chosen: None };
        let mut state = State::new();
        state.open = Some(chat_with("c1", vec![step("Read the sorter", TodoStatus::Done), step("Write the test", TodoStatus::InProgress)]));
        assert_eq!(step_on(&state, "c1"), Some("Write the test"), "the open chat's step in progress: why its files are open");
        assert_eq!(step_on(&state, "t1"), None, "no such chat or task, no step");
        state.ide.grid.tiles.push(super::super::grid::Tile {
            id: "t1".into(),
            conversation: Some(chat_with("t1", vec![step("Profile the loop", TodoStatus::InProgress)])),
            generation: 0,
            draft: String::new(),
            sending: false,
            problem: None,
            changes: None,
            recording: false,
            mode: None,
            choice: None,
        });
        assert_eq!(step_on(&state, "t1"), Some("Profile the loop"), "a task on the grid says its own step");
        state.open.as_mut().expect("open").todos[1].status = TodoStatus::Done;
        assert_eq!(step_on(&state, "c1"), None, "every step done: none in progress");

        assert!(!work_calls(&state), "nothing waits for the reader");
        state.open.as_mut().expect("open").checklists.push(crate::lattice::chat::Checklist {
            id: "l1".into(),
            title: "Set up".into(),
            steps: vec![("Install the SDK".into(), false), ("Sign in".into(), true)],
            notify: false,
            told: false,
        });
        assert_eq!(checklist_counts(&state), (1, 1, 2));
        assert!(work_calls(&state), "a checklist's step unchecked asks the reader");
        state.open.as_mut().expect("open").checklists[0].steps[0].1 = true;
        assert!(!work_calls(&state), "every step checked: nothing waits");
        state.open.as_mut().expect("open").checklists[0].told = true;
        assert_eq!(checklist_counts(&state), (0, 0, 0), "a list told done leaves the Work side");
        state.open.as_mut().expect("open").todos[0].decision = Some("database".into());
        assert!(work_calls(&state), "a step waiting on a stack decision asks the reader");
    }
}
