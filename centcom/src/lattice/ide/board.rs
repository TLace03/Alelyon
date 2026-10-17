//! The board, in the editor's place: each group of the project worked in as a card in its column (Planned, In
//! progress, Waiting on you, Done), highest priority first, with its progress and what it waits for; then what is
//! working now. A card follows the work (lattice-core `convo::board`); the reader moves one to another column (kept
//! until its work changes), raises or lowers its priority, and opens its session.

use iced::widget::{Column, Row, button, container, row, scrollable, space};
use iced::{Alignment, Element, Length, Padding};
use lattice_core::convo::board::Card;
use lattice_core::convo::groups::Status;

use super::groups::{GroupsMsg, go};
use crate::lattice::{Msg, State};
use crate::theme;
use crate::ui::{label, note, strong};

type El<'a> = Element<'a, Msg>;

/// The columns, in order, with their names.
pub const COLUMNS: [(Status, &str); 4] =
    [(Status::Planned, "Planned"), (Status::InProgress, "In progress"), (Status::Waiting, "Waiting on you"), (Status::Done, "Done")];

fn small<'a>(words: &'a str, msg: Msg) -> El<'a> {
    button(label(words, 11.0, theme::TEXT_DIM)).padding([2.0, 6.0]).style(theme::ghost_button).on_press(msg).into()
}

/// What a card says of its progress: its steps and its tasks.
pub fn progress(card: &Card) -> String {
    let mut parts = Vec::new();
    if card.todos_total > 0 {
        parts.push(format!("{} of {} steps done", card.todos_done, card.todos_total));
    }
    parts.push(format!("{} topic{}", card.topics, if card.topics == 1 { "" } else { "s" }));
    match (card.tasks, card.tasks_running) {
        (0, _) => {}
        (n, 0) => parts.push(format!("{n} task{}", if n == 1 { "" } else { "s" })),
        (n, k) => parts.push(format!("{n} task{}, {k} working", if n == 1 { "" } else { "s" })),
    }
    parts.join(" \u{00B7} ")
}

/// Tokens as a person reads them: 950, 12.3k, 4.1M.
fn tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// Time worked as a person reads it: 40 s, 12 min, 1 h 5 min.
fn worked(seconds: f64) -> String {
    let s = seconds.round() as u64;
    match s {
        0..60 => format!("{s} s"),
        60..3_600 => format!("{} min", s / 60),
        _ => format!("{} h {} min", s / 3_600, (s % 3_600) / 60),
    }
}

/// What a card says it spent, measured: tokens, time worked, how its answers are priced, the answers whose tokens
/// were not reported, and the commands that may cost money; `None` before any answer.
pub fn spent(card: &Card) -> Option<Vec<String>> {
    let s = &card.spent;
    let answers = s.local + s.subscription + s.unpriced;
    if answers == 0 && s.costly_asked == 0 {
        return None;
    }
    let mut lines = vec![format!(
        "{} tokens in, {} out \u{00B7} {} worked",
        tokens(s.tokens_in),
        tokens(s.tokens_out),
        worked(s.seconds)
    )];
    let mut priced = Vec::new();
    if s.local > 0 {
        priced.push(format!("{} on this PC (free)", s.local));
    }
    if s.subscription > 0 {
        priced.push(format!("{} on your subscription", s.subscription));
    }
    if s.unpriced > 0 {
        priced.push(format!("{} at a price not known here", s.unpriced));
    }
    if !priced.is_empty() {
        lines.push(format!("Answers: {}", priced.join(", ")));
    }
    if s.unreported > 0 {
        lines.push(format!(
            "{} answer{} reported no tokens (not counted)",
            s.unreported,
            if s.unreported == 1 { "" } else { "s" }
        ));
    }
    if s.costly_asked > 0 {
        lines.push(format!("Commands that may cost money: {} asked, {} approved", s.costly_asked, s.costly_approved));
    }
    Some(lines)
}

fn card_view<'a>(state: &'a State, card: &'a Card, first: bool, last: bool) -> El<'a> {
    let open = state.ide.groups.open.as_deref() == Some(card.group.as_str());
    let mut col = Column::new().spacing(5);
    col = col.push(
        button(strong(card.name.as_str(), 13.0, if open { theme::GOLD } else { theme::TEXT }))
            .padding(0)
            .style(theme::ghost_button)
            .on_press(go(GroupsMsg::Open(Some(card.group.clone())))),
    );
    if !card.goal.is_empty() {
        col = col.push(label(card.goal.as_str(), 11.5, theme::TEXT));
    }
    col = col.push(label(progress(card), 11.5, theme::TEXT_DIM));
    if let Some(words) = super::plan::summary(card) {
        col = col.push(label(words, 11.5, theme::TEXT_DIM));
    }
    if let Some(late) = &card.behind {
        col = col.push(label(format!("Behind: \u{201c}{}\u{201d} was due {}", late.title, late.due.as_deref().unwrap_or("")), 11.5, theme::CAUTION));
    }
    for (i, line) in spent(card).unwrap_or_default().into_iter().enumerate() {
        let warn = i > 0 && line.starts_with("Commands that may cost money");
        col = col.push(label(line, 11.0, if warn { theme::CAUTION } else { theme::TEXT_FAINT }));
    }
    if !card.waits.is_empty() {
        col = col.push(label(format!("Waits for {}", card.waits.join(", ")), 11.5, theme::GOLD));
    }
    if card.moved {
        col = col.push(label("You put it here; it follows the work again when the work changes.", 11.0, theme::TEXT_FAINT));
    }
    // Priority, and the other columns.
    let mut tools = Row::new().spacing(2).align_y(Alignment::Center);
    if !first {
        tools = tools.push(small("\u{2191}", go(GroupsMsg::Raise(card.group.clone(), true))));
    }
    if !last {
        tools = tools.push(small("\u{2193}", go(GroupsMsg::Raise(card.group.clone(), false))));
    }
    tools = tools.push(small("Plan", super::plan::send(super::plan::PlanMsg::Open(card.group.clone()))));
    tools = tools.push(space().width(Length::Fill));
    for (status, name) in COLUMNS.iter().filter(|(s, _)| *s != card.status) {
        tools = tools.push(small(name, go(GroupsMsg::MoveCard(card.group.clone(), Some(*status)))));
    }
    if card.moved {
        tools = tools.push(small("Follow the work", go(GroupsMsg::MoveCard(card.group.clone(), None))));
    }
    col = col.push(tools);
    if let Some(edit) = state.ide.groups.plan_edit.as_ref().filter(|e| e.group == card.group) {
        col = col.push(super::plan::form(edit));
    }
    container(col).width(Length::Fill).padding(10).style(theme::card).into()
}

/// The board.
pub fn view<'a>(state: &'a State, phase: f32) -> El<'a> {
    let groups = &state.ide.groups;
    let project = state.ide.project.as_deref().and_then(|p| state.ide.projects.find(p)).map(|p| p.name.as_str());
    let head = row![
        strong(format!("Board \u{2014} {}", project.unwrap_or("work in no project")), 14.0, theme::TEXT),
        space().width(Length::Fill),
        small("Back to the editor", go(GroupsMsg::ShowBoard(false))),
    ]
    .align_y(Alignment::Center);
    let mut col = Column::new().spacing(10).padding(14).push(head);
    if !state.project_manager_allowed() {
        return col
            .push(note("The board, with plans, milestones and priorities, comes with a paid plan (Pro and up). Upgrade in Settings, under Account."))
            .into();
    }
    col = col.push(note(
        "Highest priority first: when the task slots are full, the highest group's tasks start first. A card follows the work; one you move stays until its work changes.",
    ));
    let cards = &groups.board;
    if cards.is_empty() {
        col = col.push(note("No groups in this project yet: your first message starts one."));
    }
    let mut columns = Row::new().spacing(10);
    for (status, name) in COLUMNS {
        let mine: Vec<&Card> = cards.iter().filter(|c| c.status == status).collect();
        let mut column = Column::new().spacing(8).width(Length::FillPortion(1));
        column = column.push(strong(format!("{name} ({})", mine.len()), 12.5, if status == Status::Waiting && !mine.is_empty() { theme::GOLD } else { theme::TEXT_DIM }));
        for card in mine {
            let first = cards.first().is_some_and(|c| c.group == card.group);
            let last = cards.last().is_some_and(|c| c.group == card.group);
            column = column.push(card_view(state, card, first, last));
        }
        columns = columns.push(column);
    }
    col = col.push(columns);
    // What is working now, each task beneath the topic it came from.
    col = col.push(container(strong("Working now", 12.5, theme::TEXT)).padding(Padding { top: 8.0, ..Padding::ZERO }));
    col = col.push(super::activity::working(state, Column::new().spacing(2), phase));
    scrollable(col).height(Length::Fill).style(theme::scrollbars).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::ide::{IdeMsg, grid::GridMsg};

    fn card(group: &str, status: Status) -> Card {
        Card {
            group: group.into(),
            name: group.to_uppercase(),
            status,
            moved: false,
            topics: 2,
            tasks: 3,
            tasks_running: 1,
            todos_done: 2,
            todos_total: 5,
            waits: Vec::new(),
            rank: 0,
            goal: String::new(),
            milestones_done: 1,
            milestones_total: 3,
            next: Some(lattice_core::convo::groups::Milestone { title: "Cards".into(), due: Some("2026-10-20".into()), done: false }),
            behind: None,
            spent: Default::default(),
        }
    }

    #[test]
    fn a_cards_progress_names_its_steps_topics_and_working_tasks() {
        assert_eq!(progress(&card("a", Status::InProgress)), "2 of 5 steps done \u{00B7} 2 topics \u{00B7} 3 tasks, 1 working");
        let quiet = Card { todos_total: 0, todos_done: 0, topics: 1, tasks: 0, tasks_running: 0, ..card("b", Status::Planned) };
        assert_eq!(progress(&quiet), "1 topic");
    }

    #[test]
    fn a_cards_plan_names_its_milestones_and_the_next_with_its_date() {
        assert_eq!(super::super::plan::summary(&card("a", Status::InProgress)).as_deref(), Some("Milestones 1 of 3 \u{00B7} next: Cards (due 2026-10-20)"));
        let none = Card { milestones_total: 0, milestones_done: 0, next: None, ..card("b", Status::Planned) };
        assert_eq!(super::super::plan::summary(&none), None);
    }

    #[test]
    fn a_cards_spending_says_what_was_measured_and_what_was_not() {
        use lattice_core::convo::board::Spent;
        assert_eq!(spent(&card("a", Status::Planned)), None, "nothing answered yet");
        let measured = Card {
            spent: Spent {
                tokens_in: 12_345,
                tokens_out: 950,
                unreported: 1,
                seconds: 3_900.0,
                local: 2,
                subscription: 1,
                unpriced: 1,
                costly_asked: 2,
                costly_approved: 1,
            },
            ..card("b", Status::InProgress)
        };
        assert_eq!(
            spent(&measured).unwrap(),
            [
                "12.3k tokens in, 950 out \u{00B7} 1 h 5 min worked",
                "Answers: 2 on this PC (free), 1 on your subscription, 1 at a price not known here",
                "1 answer reported no tokens (not counted)",
                "Commands that may cost money: 2 asked, 1 approved",
            ]
        );
    }

    #[test]
    fn the_plan_editor_edits_a_copy_until_saved() {
        use super::super::plan::{PlanMsg, send};
        use lattice_core::convo::groups::Plan;
        let mut state = State::new();
        let plan = Plan { goal: "Ship".into(), milestones: Vec::new(), edited: false };
        let _ = state.update(send(PlanMsg::Loaded("g".into(), Some(plan))));
        let _ = state.update(send(PlanMsg::Add));
        let _ = state.update(send(PlanMsg::Title(0, "Cart".into())));
        let _ = state.update(send(PlanMsg::Due(0, "2026-10-20".into())));
        let _ = state.update(send(PlanMsg::Done(0, true)));
        let _ = state.update(send(PlanMsg::Due(0, " ".into())));
        let edit = state.ide.groups.plan_edit.clone().unwrap();
        assert_eq!((edit.group.as_str(), edit.goal.as_str(), edit.milestones.len()), ("g", "Ship", 1));
        assert_eq!((edit.milestones[0].title.as_str(), edit.milestones[0].due.clone(), edit.milestones[0].done), ("Cart", None, true));
        let _ = state.update(send(PlanMsg::Remove(0)));
        assert!(state.ide.groups.plan_edit.as_ref().unwrap().milestones.is_empty());
        let _ = state.update(send(PlanMsg::Cancel));
        assert!(state.ide.groups.plan_edit.is_none());
    }

    #[test]
    fn the_board_and_the_tasks_view_take_the_editors_place_one_at_a_time() {
        let mut state = State::new();
        let groups = |m: GroupsMsg| Msg::Ide(IdeMsg::Groups(m));
        let _ = state.update(groups(GroupsMsg::Read((Vec::new(), false), Vec::new(), vec![card("a", Status::Waiting)])));
        assert_eq!(state.ide.groups.board.len(), 1);
        let _ = state.update(Msg::Ide(IdeMsg::Grid(GridMsg::Show(true))));
        let _ = state.update(groups(GroupsMsg::ShowBoard(true)));
        assert!(state.ide.groups.board_shown && !state.ide.grid.shown);
        let _ = state.update(Msg::Ide(IdeMsg::Grid(GridMsg::Show(true))));
        assert!(!state.ide.groups.board_shown && state.ide.grid.shown);
    }
}
