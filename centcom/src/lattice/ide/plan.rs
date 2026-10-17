//! A group's plan, edited by the reader: its goal, and its milestones in order, each with an optional date
//! (`YYYY-MM-DD`) and whether it is done. The agent proposes the plan and ticks milestones (lattice-core
//! `convo::groups`); once the reader saves it here, the agent's proposals no longer replace it, and no date is moved
//! but by the reader.

use iced::widget::{Column, Row, button, checkbox, container, row, text_input};
use iced::{Alignment, Element, Length, Task};
use lattice_core::convo::groups::{MAX_MILESTONES, Milestone, Plan};

use super::groups::{GroupsMsg, go};
use crate::lattice::{Msg, STOPPED, State, on};
use crate::theme;
use crate::ui::{self, label, note, strong};

type El<'a> = Element<'a, Msg>;

/// A plan being edited.
#[derive(Debug, Clone, Default)]
pub struct PlanEdit {
    pub group: String,
    pub goal: String,
    pub milestones: Vec<Milestone>,
}

/// The editor's messages.
#[derive(Debug, Clone)]
pub enum PlanMsg {
    /// Edit a group's plan: it is read first.
    Open(String),
    Loaded(String, Option<Plan>),
    Goal(String),
    Title(usize, String),
    Due(usize, String),
    Done(usize, bool),
    Add,
    Remove(usize),
    Save,
    Cancel,
}

pub fn send(msg: PlanMsg) -> Msg {
    go(GroupsMsg::Plan(msg))
}

impl State {
    pub(in crate::lattice) fn plan(&mut self, msg: PlanMsg) -> Task<Msg> {
        let groups = &mut self.ide.groups;
        match msg {
            PlanMsg::Open(group) => {
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                let asked = group.clone();
                return Task::perform(on(&services, async move { chat.group_plan(&asked) }), move |plan| {
                    send(PlanMsg::Loaded(group.clone(), plan.flatten()))
                });
            }
            PlanMsg::Loaded(group, plan) => {
                let plan = plan.unwrap_or_default();
                groups.plan_edit = Some(PlanEdit { group, goal: plan.goal, milestones: plan.milestones });
            }
            PlanMsg::Cancel => groups.plan_edit = None,
            PlanMsg::Save => {
                let Some(edit) = groups.plan_edit.take() else { return Task::none() };
                let Some(services) = self.services.clone() else { return Task::none() };
                let chat = services.chat.clone();
                let plan = Plan { goal: edit.goal, milestones: edit.milestones, edited: true };
                let group = edit.group;
                return Task::perform(on(&services, async move { chat.set_group_plan(&group, plan).map(drop) }), |r| {
                    go(GroupsMsg::Done(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            msg => {
                let Some(edit) = groups.plan_edit.as_mut() else { return Task::none() };
                match msg {
                    PlanMsg::Goal(goal) => edit.goal = goal,
                    PlanMsg::Title(i, title) => {
                        if let Some(m) = edit.milestones.get_mut(i) {
                            m.title = title;
                        }
                    }
                    PlanMsg::Due(i, due) => {
                        if let Some(m) = edit.milestones.get_mut(i) {
                            m.due = Some(due).filter(|d| !d.trim().is_empty());
                        }
                    }
                    PlanMsg::Done(i, done) => {
                        if let Some(m) = edit.milestones.get_mut(i) {
                            m.done = done;
                        }
                    }
                    PlanMsg::Add if edit.milestones.len() < MAX_MILESTONES => edit.milestones.push(Milestone::default()),
                    PlanMsg::Remove(i) if i < edit.milestones.len() => {
                        edit.milestones.remove(i);
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }
}

fn small<'a>(words: &'a str, msg: Option<Msg>) -> El<'a> {
    button(label(words, 11.0, theme::TEXT_DIM)).padding([2.0, 6.0]).style(theme::ghost_button).on_press_maybe(msg).into()
}

/// The editor.
pub fn form<'a>(edit: &'a PlanEdit) -> El<'a> {
    let mut col = Column::new().spacing(6);
    col = col.push(strong("Plan", 12.5, theme::TEXT));
    col = col.push(
        text_input("The group's goal, in one line", &edit.goal)
            .on_input(|t| send(PlanMsg::Goal(t)))
            .padding([4.0, 8.0])
            .size(12.0)
            .style(ui::input_style),
    );
    col = col.push(label("Milestones, in order (a date is optional: YYYY-MM-DD)", 11.0, theme::TEXT_FAINT));
    for (i, m) in edit.milestones.iter().enumerate() {
        let line: Row<'a, Msg> = row![
            checkbox(m.done).on_toggle(move |on| send(PlanMsg::Done(i, on))).size(14),
            text_input("Milestone", &m.title)
                .on_input(move |t| send(PlanMsg::Title(i, t)))
                .padding([3.0, 6.0])
                .size(12.0)
                .style(ui::input_style)
                .width(Length::FillPortion(3)),
            text_input("Date", m.due.as_deref().unwrap_or(""))
                .on_input(move |t| send(PlanMsg::Due(i, t)))
                .padding([3.0, 6.0])
                .size(12.0)
                .style(ui::input_style)
                .width(Length::FillPortion(1)),
            small("\u{2715}", Some(send(PlanMsg::Remove(i)))),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        col = col.push(line);
    }
    if edit.milestones.is_empty() {
        col = col.push(note("No milestones yet."));
    }
    col = col.push(
        row![
            small("Add a milestone", (edit.milestones.len() < MAX_MILESTONES).then(|| send(PlanMsg::Add))),
            small("Save", Some(send(PlanMsg::Save))),
            small("Cancel", Some(send(PlanMsg::Cancel))),
        ]
        .spacing(4),
    );
    col = col.push(label("Once you save it, the agent suggests changes in words instead of replacing it.", 11.0, theme::TEXT_FAINT));
    container(col).padding(8).width(Length::Fill).style(theme::card).into()
}

/// What a group's card or line says of its plan: the milestones done, the next, and whether it is behind.
pub fn summary(card: &lattice_core::convo::board::Card) -> Option<String> {
    if card.milestones_total == 0 {
        return None;
    }
    let mut out = format!("Milestones {} of {}", card.milestones_done, card.milestones_total);
    if let Some(next) = &card.next {
        out.push_str(&format!(" \u{00B7} next: {}", next.title));
        if let Some(due) = &next.due {
            out.push_str(&format!(" (due {due})"));
        }
    }
    Some(out)
}
