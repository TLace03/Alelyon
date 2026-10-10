//! The agent's side of the IDE: the panel on the right (the conversation as a list of compact steps, each tool call a
//! line that opens to its output, approvals and questions inline, the changes waiting above the composer, and the
//! composer itself), the Chats view of the side bar (every conversation, as an agent manager lists them), and the
//! bottom panel's list of the agent's commands.

use iced::widget::{
    Column, Row, button, column, container, markdown, pick_list, rich_text, row, scrollable, space, span, text,
    text_editor, text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, padding};

use crate::theme::{self, fonts};
use lattice_protocol::Locality;
use lattice_protocol::chat::{ChatTurn, Role};
use lattice_protocol::conversation::{
    ApprovalDetail, ArtifactKind, ChangeState, CommandMode, DecidedBy, ExitReason, Mode, PlanOutcome, ReviewOp,
    TaskOutcome,
    TodoStatus, TrustState,
    TurnStatus,
};

use super::view::{ghost, icon};
use super::{ChatFilter, IdeMsg, Side, names_a_file, subject, verb};
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

/// The user's message, as a quiet card.
fn bubble(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::RAISED)),
        text_color: Some(theme::TEXT),
        border: Border { color: theme::LINE, width: 1.0, radius: 10.0.into() },
        ..container::Style::default()
    }
}

/// The composer: a raised card, gold edged while it has the focus.
pub(super) fn composer_card(_: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::CANVAS)),
        text_color: Some(theme::TEXT),
        border: Border { color: theme::LINE, width: 1.0, radius: 12.0.into() },
        ..container::Style::default()
    }
}

// --------------------------------------------------------------------------------------------- the panel

pub fn panel<'a>(state: &'a State, phase: f32) -> El<'a> {
    let mut col = Column::new().height(Length::Fill);
    col = col.push(header(state));
    col = col.push(container(space().height(1)).width(Length::Fill).style(theme::line));
    let body: El<'a> = match state.conversation() {
        None if state.opening.is_some() => container(ui::working(phase, "Opening the chat…")).padding(18).into(),
        None => empty_state(state),
        Some(c) => scrollable(container(transcript(c, state, phase)).padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 18.0 }))
            .anchor_bottom()
            .height(Length::Fill)
            .style(theme::scrollbars)
            .into(),
    };
    col = col.push(container(body).height(Length::Fill));
    if let Some(bar) = todos_bar(state) {
        col = col.push(bar);
    }
    if let Some(bar) = changes_bar(state) {
        col = col.push(bar);
    }
    col = col.push(composer(state, phase));
    col.into()
}

fn header<'a>(state: &'a State) -> El<'a> {
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    match state.conversation() {
        Some(c) => {
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
        None => line = line.push(strong("New chat", 13.5, theme::TEXT)),
    }
    line = line.push(space().width(Length::Fill));
    // The grid: chats side by side, shown in the editor's place.
    let grid = &state.ide.grid;
    let grid_tip = if grid.shown { "Back to the editor" } else { "Chats side by side" };
    line = line.push(icon("\u{25A6}", grid_tip, Some(super::grid::go(super::grid::GridMsg::Show(!grid.shown)))));
    line = line.push(icon("\u{25C8}", "All chats", Some(go(IdeMsg::Side(Side::Chats)))));
    if let Some(c) = state.conversation() {
        line = line.push(icon("\u{2399}", "Archive this chat (it is never deleted)", Some(Msg::Archive(c.id().to_string()))));
    }
    line = line.push(icon("+", "New chat", Some(Msg::New)));
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

/// No chat open: what the agent does, and a few starts.
fn empty_state<'a>(state: &'a State) -> El<'a> {
    let mut col = Column::new().spacing(12).padding(22);
    col = col.push(label("\u{25C7}", 30.0, theme::GOLD));
    col = col.push(strong("What should we work on?", 17.0, theme::TEXT));
    col = col.push(label(
        "The agent reads the folder you open, proposes changes you review before anything is written, and runs \
         commands only after you confirm each one.",
        12.5,
        theme::TEXT_DIM,
    ));
    let active = state.ide.active_tab().and_then(|t| t.path()).map(str::to_string);
    let mut starts: Vec<String> = vec!["Explain how this folder is organised.".to_string()];
    if let Some(path) = &active {
        starts.push(format!("Explain @{path}"));
        starts.push(format!("Find bugs in @{path}"));
        starts.push(format!("Write tests for @{path}"));
    } else {
        starts.push("Find the place that handles errors and explain it.".to_string());
        starts.push("Suggest three small improvements.".to_string());
    }
    let mut chips = Column::new().spacing(6);
    for s in starts {
        chips = chips.push(
            button(label(ui::cut(&s, 56), 12.5, theme::TEXT))
                .width(Length::Fill)
                .padding([7.0, 10.0])
                .style(theme::secondary_button)
                .on_press(go(IdeMsg::Suggest(s.clone()))),
        );
    }
    col = col.push(chips);
    // The newest chats, to go back to one.
    if let Some(list) = &state.list
        && !list.conversations.is_empty()
    {
        col = col.push(space().height(4));
        col = col.push(text("RECENT CHATS").size(11).font(fonts().ui_strong).color(theme::TEXT_FAINT));
        let now = crate::utc::now();
        for c in list.conversations.iter().take(5) {
            let title = if c.title.is_empty() { "Untitled" } else { c.title.as_str() };
            col = col.push(
                button(row![label(ui::cut(title, 40), 12.5, theme::TEXT), space().width(Length::Fill), label(super::ago(now - c.updated), 11.0, theme::TEXT_FAINT)].align_y(Alignment::Center))
                    .width(Length::Fill)
                    .padding([5.0, 8.0])
                    .style(theme::list_row(false))
                    .on_press(Msg::Open(c.id.clone())),
            );
        }
    }
    scrollable(col).height(Length::Fill).style(theme::scrollbars).into()
}

pub(super) fn transcript<'a>(c: &'a Conversation, state: &'a State, phase: f32) -> El<'a> {
    let mut col = Column::new().spacing(14).width(Length::Fill);
    let last_answer = c.turns.iter().rposition(|t| t.role == Role::Assistant);
    for (i, turn) in c.turns.iter().enumerate() {
        col = col.push(match turn.role {
            Role::User => user_turn(turn, state, c.running),
            Role::Assistant => answer(c, i, turn),
        });
        for activity in c.activity_for(&turn.id) {
            col = col.push(activity_view(activity, state));
        }
    }
    for activity in c.unplaced() {
        col = col.push(activity_view(activity, state));
    }
    if let Some(live) = &c.live {
        col = col.push(markdown::view(live.items(), crate::lattice::view::settings()).map(|_| Msg::Link));
    }
    if c.dropped > 0 {
        col = col.push(note(format!("{} pieces of the streamed answer were not shown; the saved answer is whole.", c.dropped)));
    }
    if c.running {
        col = col.push(working_line(c, phase));
    } else if let Some(a) = c.activity.last() {
        // After a turn: go again, or go on where a limit or a closed window stopped it.
        let mut acts = Row::new().spacing(6);
        if matches!(a.status, Some(TurnStatus::MaxTurns | TurnStatus::Interrupted)) {
            acts = acts.push(ui::secondary("Continue", (!state.sending).then(|| go(IdeMsg::Continue))));
        }
        if last_answer.is_some() && !state.sending {
            acts = acts.push(ghost("\u{27F3} Regenerate", Some(go(IdeMsg::Regenerate))));
        }
        col = col.push(acts);
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

fn user_turn<'a>(turn: &'a ChatTurn, state: &'a State, running: bool) -> El<'a> {
    let editing = state.ide.editing_turn.as_deref() == Some(turn.id.as_str());
    let card = container(text(turn.text.as_str()).size(13.5).color(theme::TEXT).font(fonts().ui))
        .padding([9.0, 12.0])
        .width(Length::Fill)
        .style(if editing {
            |_: &iced::Theme| container::Style {
                background: Some(Background::Color(theme::RAISED)),
                text_color: Some(theme::TEXT),
                border: Border { color: theme::GOLD_DIM, width: 1.0, radius: 10.0.into() },
                ..container::Style::default()
            }
        } else {
            bubble
        });
    let mut acts = Row::new().spacing(2);
    // The images attached to this message stay in the chat's record; the line says how many.
    if let Some(count) = state.conversation().and_then(|c| c.images.get(&turn.id)) {
        acts = acts.push(label(
            format!("\u{1F5BC} {count} image{} attached", if *count == 1 { "" } else { "s" }),
            11.0,
            theme::TEXT_FAINT,
        ));
    }
    acts = acts.push(space().width(Length::Fill));
    if !turn.id.is_empty() && !running {
        acts = acts.push(ghost(if editing { "Editing below…" } else { "Edit" }, (!editing).then(|| go(IdeMsg::EditTurn(turn.id.clone())))));
    }
    column![card, acts].spacing(2).into()
}

fn answer<'a>(c: &'a Conversation, i: usize, turn: &'a ChatTurn) -> El<'a> {
    let mut col = Column::new().spacing(6);
    let mut who = Row::new().spacing(8).align_y(Alignment::Center);
    who = who.push(label("\u{25C7}", 12.0, theme::GOLD));
    who = who.push(label("Lattice", 11.5, theme::TEXT_FAINT));
    if !turn.provider.is_empty() {
        who = who.push(label(turn.provider.as_str(), 11.0, theme::TEXT_FAINT));
    }
    if turn.cancelled {
        who = who.push(ui::chip("stopped", theme::TEXT_DIM));
    }
    if turn.truncated {
        who = who.push(ui::chip("cut short", theme::CAUTION));
    }
    if let (Some(p), Some(q)) = (turn.prompt_tokens, turn.completion_tokens) {
        who = who.push(label(format!("{p} in · {q} out"), 11.0, theme::TEXT_FAINT));
    }
    col = col.push(who);
    if let Some(content) = c.rendered(i) {
        col = col.push(markdown::view(content.items(), crate::lattice::view::settings()).map(|_| Msg::Link));
    }
    if !turn.error.is_empty() {
        col = col.push(ui::notice(turn.error.as_str(), theme::CAUTION));
    }
    if !turn.unsupported.is_empty() {
        col = col.push(note(format!("Figures no tool backs: {}", turn.unsupported.join(", "))));
    }
    col.into()
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

fn activity_view<'a>(a: &'a Activity, state: &'a State) -> El<'a> {
    if a.items.is_empty() && a.label.is_empty() {
        return space().height(0).into();
    }
    let mut col = Column::new().spacing(4);
    let mut top = Row::new().spacing(8).align_y(Alignment::Center);
    if !a.label.is_empty() {
        top = top.push(label(a.label.as_str(), 11.0, if a.local { theme::POSITIVE } else { theme::CAUTION }));
        if !a.local {
            top = top.push(label("off this computer", 11.0, theme::CAUTION));
        }
    }
    if let Some(status) = a.status {
        let (words, color) = match status {
            TurnStatus::Completed => ("done", theme::TEXT_FAINT),
            TurnStatus::Stopped => ("stopped", theme::TEXT_DIM),
            TurnStatus::Failed => ("failed", theme::DANGER),
            TurnStatus::Refused => ("refused", theme::CAUTION),
            TurnStatus::MaxTurns => ("turn limit reached", theme::CAUTION),
            TurnStatus::Interrupted => ("interrupted", theme::CAUTION),
        };
        top = top.push(label(words, 11.0, color));
    }
    col = col.push(top);
    for item in &a.items {
        col = col.push(item_view(item, state));
    }
    col.into()
}

fn item_view<'a>(item: &'a Item, state: &'a State) -> El<'a> {
    match item {
        Item::Tool(t) => step(t, state),
        Item::Approval { call, detail, allow_always, decided } => approval(call, detail, *allow_always, decided.as_ref(), state),
        Item::Question { call, text: q, options, answered } => question(call, q, options, *answered, state),
        Item::Task { task, title, summary, prompt, outcome } => {
            task_chip(task, title, summary, prompt, outcome.as_ref(), state)
        }
        Item::Plan { plan, title, text, outcome } => plan_card(plan, title, text, outcome.as_ref(), state),
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
        ApprovalDetail::Command { text: command, cwd, mode, timeout_s, remote, staged_waiting, background } => {
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
            let choice = |n: &'a str, words: &'a str, msg: Msg, primary: bool| -> El<'a> {
                let inner = row![mono(n, 11.5, if primary { theme::ON_GOLD } else { theme::GOLD }), text(words).size(12.5).font(fonts().ui)].spacing(8);
                button(inner)
                    .width(Length::Fill)
                    .padding([6.0, 10.0])
                    .style(if primary { theme::primary_button } else { theme::secondary_button })
                    .on_press(msg)
                    .into()
            };
            let mut choices = Column::new().spacing(4);
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
            col = col.push(choices);
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
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(theme::notice(theme::GOLD)).into()
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
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(theme::notice(theme::GOLD)).into()
}

/// A task the agent suggested (a chip): its title and summary; Start (a new chat of its own, in this folder and project,
/// beginning with the prompt) and Dismiss while it waits; and the prompt itself on request, so what Start sends can
/// be read first. Once settled, what became of it, and a started task's chat a click away.
fn task_chip<'a>(
    task: &'a str,
    title: &'a str,
    summary: &'a str,
    prompt: &'a str,
    outcome: Option<&'a TaskOutcome>,
    state: &'a State,
) -> El<'a> {
    let shown = state.ide.task_prompts.contains(task);
    // A chip that waits is gold; a settled one is quiet.
    let tone = if outcome.is_none() { theme::GOLD } else { theme::TEXT_FAINT };
    let mut col = Column::new()
        .spacing(6)
        .push(label("Suggested task", 11.5, tone))
        .push(strong(title, 13.5, theme::TEXT))
        .push(label(summary, 12.5, theme::TEXT_DIM));
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    let settled = |words: String| label(words, 11.5, theme::TEXT_FAINT).width(Length::Fill);
    match outcome {
        None => {
            line = line.push(ui::primary("Start", (!state.sending).then(|| go(IdeMsg::TaskStart(task.to_string())))));
            line = line.push(ui::secondary("Dismiss", Some(go(IdeMsg::TaskDismiss(task.to_string())))));
            line = line.push(space().width(Length::Fill));
        }
        Some(TaskOutcome::Started { conversation }) => {
            line = line.push(settled("Started as a new chat".to_string()));
            line = line.push(ghost("Open it", Some(Msg::Open(conversation.clone()))));
        }
        Some(TaskOutcome::Dismissed) => line = line.push(settled("Dismissed".to_string())),
        Some(TaskOutcome::Withdrawn { reason }) if reason.is_empty() => {
            line = line.push(settled("The agent withdrew it".to_string()))
        }
        Some(TaskOutcome::Withdrawn { reason }) => line = line.push(settled(format!("The agent withdrew it: {reason}"))),
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
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(theme::notice(tone)).into()
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
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(theme::notice(tone)).into()
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
    container(line).padding([8.0, 12.0]).width(Length::Fill).style(theme::notice(theme::TEXT_FAINT)).into()
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

/// The agent's to-do list, above the composer: each step with where it stands, the one in progress in gold. Once
/// every step is done it is one line.
fn todos_bar<'a>(state: &'a State) -> Option<El<'a>> {
    let todos = &state.conversation()?.todos;
    if todos.is_empty() {
        return None;
    }
    let done = todos.iter().filter(|step| step.status == TodoStatus::Done).count();
    let head = row![
        label("\u{2630}", 13.0, theme::GOLD),
        label("To-do", 12.5, theme::TEXT),
        label(format!("{done} of {} done", todos.len()), 11.5, theme::TEXT_DIM),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let mut col = Column::new().spacing(3).push(head);
    if done < todos.len() {
        for step in todos {
            let (mark, tone) = match step.status {
                TodoStatus::Done => ("\u{2713}", theme::TEXT_FAINT),
                TodoStatus::InProgress => ("\u{25B8}", theme::GOLD),
                TodoStatus::Pending => ("\u{25CB}", theme::TEXT_DIM),
            };
            let words = if step.status == TodoStatus::Done { theme::TEXT_FAINT } else { theme::TEXT };
            col = col.push(
                row![label(mark, 12.0, tone), label(step.content.as_str(), 12.0, words)]
                    .spacing(8)
                    .align_y(Alignment::Center),
            );
        }
    }
    Some(
        container(container(col).padding([7.0, 10.0]).width(Length::Fill).style(theme::notice(theme::TEXT_FAINT)))
            .padding(Padding { top: 8.0, bottom: 0.0, left: 12.0, right: 12.0 })
            .into(),
    )
}

/// The agent's changes waiting for review, as one bar above the composer.
fn changes_bar<'a>(state: &'a State) -> Option<El<'a>> {
    let set = state.changes.as_ref()?;
    let waiting: Vec<_> = set.changes.iter().filter(|c| matches!(c.state, ChangeState::Pending | ChangeState::Rebased)).collect();
    if waiting.is_empty() {
        return None;
    }
    let (added, removed) = waiting.iter().fold((0u32, 0u32), |(a, r), c| (a + c.added, r + c.removed));
    let first = waiting[0].id.clone();
    let free = !state.reviewing && !set.command_running;
    let n = waiting.len();
    let line = row![
        label("\u{00B1}", 14.0, theme::GOLD),
        label(format!("{n} file{} changed", if n == 1 { "" } else { "s" }), 12.5, theme::TEXT),
        counts(added, removed),
        space().width(Length::Fill),
        ghost("Review", Some(Msg::ShowDiff(first))),
        ghost("Undo all", (!state.reviewing).then_some(Msg::Review(ReviewOp::UndoAll { note: None }))),
        container(ui::primary("Keep all", free.then_some(Msg::Review(ReviewOp::KeepAll)))).padding(padding::left(4)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    Some(
        container(container(line).padding([7.0, 10.0]).width(Length::Fill).style(theme::notice(theme::GOLD)))
            .padding(Padding { top: 8.0, bottom: 0.0, left: 12.0, right: 12.0 })
            .into(),
    )
}

/// The composer: the folder and the file in view as context, the box, the mode and the model, Send or Stop.
fn composer<'a>(state: &'a State, phase: f32) -> El<'a> {
    let running = state.conversation().is_some_and(|c| c.running);
    let mut card = Column::new().spacing(6);
    // Context: the folder (and its trust), and the file in view, to mention.
    let mut context = Row::new().spacing(6).align_y(Alignment::Center);
    let attached = state.folder.view.as_ref().map(|v| (v.workspace.name.clone(), v.workspace.trusted, v.trust)).or_else(|| {
        state.conversation().and_then(|c| {
            c.summary.workspace.as_ref().map(|w| (w.name.clone(), w.trusted, if w.trusted { TrustState::Trusted } else { TrustState::Untrusted }))
        })
    });
    match attached {
        Some((name, trusted, _)) => {
            context = context.push(ui::chip(format!("\u{2750} {}", ui::cut(&name, 22)), if trusted { theme::POSITIVE } else { theme::GOLD_DIM }));
            if !trusted {
                context = context.push(ghost("Trust…", (!state.folder.busy).then_some(Msg::Trust)));
            }
        }
        None if state.ide.tools.auto.is_some_and(|(on, _)| on) => {
            context = context.push(label(
                format!(
                    "No folder: in Agent mode the agent uses your whole desktop (auto mode; {} stops it).",
                    lattice_core::desktop::STOP_KEYS
                ),
                11.5,
                theme::GOLD,
            ));
        }
        None => {
            let browser_on = state.ide.tools.browser.as_ref().is_some_and(|(on, _, _)| *on);
            let words = if browser_on {
                "No folder: in Agent mode the agent uses its browser and your own MCP servers (Connections)."
            } else {
                "No folder: the agent can only answer."
            };
            context = context.push(label(words, 11.5, theme::TEXT_FAINT));
        }
    }
    if let Some(path) = state.ide.active_tab().and_then(|t| t.path()) {
        context = context.push(
            button(label(format!("@ {}", ui::cut(super::file_name(path), 22)), 11.5, theme::TEXT_DIM))
                .padding([2.0, 8.0])
                .style(theme::secondary_button)
                .on_press(go(IdeMsg::Mention)),
        );
    }
    // The images attached to this message, each with a way to take it off; and a way to attach more.
    for (at, attached) in state.ide.attached.iter().enumerate() {
        context = context.push(
            button(label(format!("\u{1F5BC} {}  \u{00D7}", ui::cut(&attached.name, 22)), 11.5, theme::TEXT_DIM))
                .padding([2.0, 8.0])
                .style(theme::secondary_button)
                .on_press(go(IdeMsg::Unattach(at))),
        );
    }
    if state.ide.attached.len() < super::attach::MAX {
        context = context.push(ghost("Image", (!state.ide.picking).then(|| go(IdeMsg::AttachImages))));
    }
    if let Some(turn) = &state.ide.editing_turn {
        let _ = turn;
        context = context.push(space().width(Length::Fill));
        context = context.push(label("Editing a message: Send replaces it and what came after.", 11.5, theme::GOLD));
        context = context.push(ghost("Cancel", Some(go(IdeMsg::CancelEditTurn))));
    }
    card = card.push(context.wrap());
    let placeholder = if state.mode == Mode::Agent {
        "Tell the agent what to change, or ask about the code…"
    } else {
        "Ask about the code (Ask mode: the agent only reads)…"
    };
    // While the commands list is open, it takes Enter, Tab, Up, Down and Esc.
    let listing = super::slash::open(state);
    // The files list, likewise, when an `@` is being typed (and no command is).
    let mentioning = !listing && super::mention::open(state);
    card = card.push(
        text_editor(&state.composer)
            .id(super::composer_id())
            .placeholder(placeholder)
            .on_action(Msg::Compose)
            .min_height(54)
            .max_height(220)
            .padding(4)
            .size(13.5)
            .key_binding(move |press| composer_keys(press, running, listing, mentioning))
            .style(|_, _| text_editor::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                placeholder: theme::TEXT_FAINT,
                value: theme::TEXT,
                selection: theme::with_alpha(theme::GOLD, 0.30),
            }),
    );
    // The mode, the model, Send.
    let mut bottom = Row::new().spacing(6).align_y(Alignment::Center);
    // Beside the grid the chat plans: Ask only, so it reads and answers and writes nothing.
    if state.planning() {
        bottom = bottom.push(label("Ask \u{00B7} plans and answers, writes nothing", 12.0, theme::GOLD));
    }
    for (mode, words) in [(Mode::Agent, "Agent"), (Mode::Ask, "Ask")].into_iter().filter(|_| !state.planning()) {
        let on = state.mode == mode;
        bottom = bottom.push(
            button(label(words, 12.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([3.0, 9.0])
                .style(theme::segment_button(on))
                .on_press(Msg::Mode(mode)),
        );
    }
    let labels: Vec<String> = state.choices.iter().map(choice_label).collect();
    let chosen = state.choices.iter().find(|c| c.id == state.choice).map(choice_label);
    bottom = bottom.push(
        // A model that is not ready says why when chosen (`ChoiceLabel`), and stays unchosen.
        pick_list(labels, chosen, Msg::ChoiceLabel)
        .placeholder("Model")
        .text_size(12.0)
        .padding([3.0, 8.0])
        .style(theme::picker)
        .menu_style(theme::picker_menu),
    );
    bottom = bottom.push(space().width(Length::Fill));
    if state.sending {
        bottom = bottom.push(crate::spinner::spinner(phase, 14.0));
    }
    if running {
        bottom = bottom.push(
            button(label("\u{25A0}", 12.0, theme::TEXT))
                .padding([5.0, 10.0])
                .style(theme::secondary_button)
                .on_press(Msg::Stop),
        );
    }
    let can_send = !state.sending && !state.composer.text().trim().is_empty() && state.services_ready();
    bottom = bottom.push(
        button(label(if running { "Queue \u{2191}" } else { "\u{2191}" }, 14.0, theme::ON_GOLD))
            .padding([4.0, 12.0])
            .style(|t, s| {
                let mut style = theme::primary_button(t, s);
                style.border.radius = 14.0.into();
                style
            })
            .on_press_maybe(can_send.then_some(Msg::Send)),
    );
    card = card.push(bottom);
    let mut col = Column::new().spacing(4);
    // The commands list, over the composer.
    if let Some(menu) = super::slash::menu(state) {
        col = col.push(menu);
    } else if let Some(menu) = super::mention::menu(state) {
        col = col.push(menu);
    }
    col = col.push(container(card).padding([8.0, 10.0]).width(Length::Fill).style(composer_card));
    // One line about the model chosen: where it runs, and why it cannot, when it cannot.
    if let Some(choice) = state.choices.iter().find(|c| c.id == state.choice) {
        let (words, color) = match (&choice.refusal, choice.locality) {
            (Some(refusal), _) => (refusal.clone(), theme::CAUTION),
            (None, Locality::Remote) => ("Leaves this computer: you confirm the first send.".to_string(), theme::CAUTION),
            (None, Locality::Local) => (choice.detail.clone(), theme::TEXT_FAINT),
        };
        col = col.push(container(label(ui::cut(&words, 120), 11.0, color)).padding(padding::left(4)));
    }
    if running {
        col = col.push(
            row![
                text_input("Steer the running turn…", &state.steer).on_input(Msg::Steer).on_submit(Msg::SendSteer).padding([5.0, 8.0]).size(12.5).style(ui::input_style),
                ghost("Steer", (!state.steer.trim().is_empty()).then_some(Msg::SendSteer)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    container(col).padding(Padding { top: 8.0, bottom: 12.0, left: 12.0, right: 12.0 }).into()
}

const NOT_READY: &str = " (not ready)";

/// A model choice as the picker lists it.
pub fn choice_label(c: &lattice_protocol::chat::ChatChoice) -> String {
    if c.ready { c.label.clone() } else { format!("{}{NOT_READY}", c.label) }
}

/// The composer's keys: Enter sends, Shift+Enter breaks the line, Esc stops a running turn. While the commands list
/// is open (`listing`), Enter and Tab choose, Up and Down move, and Esc closes it.
fn composer_keys(
    press: text_editor::KeyPress,
    running: bool,
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
        container(text_input("Search", &ide.chat_search).on_input(|t| go(IdeMsg::ChatSearch(t))).padding([5.0, 8.0]).size(12.5).style(ui::input_style))
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
