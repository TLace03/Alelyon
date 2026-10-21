//! The open group's record (`convo::record`), under its actions: the reader's notes, which they edit, where its
//! sessions stand, what was decided (each one the reader can take off), what is open, and where the work is. Every
//! session of the group starts with it; it is built by the core from the sessions' own records, so this view only
//! reads it, and writes the notes and what is taken off.

use iced::widget::{Column, Row, column, container, row, text_editor};
use iced::{Alignment, Element, Length, Padding, Task};
use lattice_core::convo::record::{By, Record};

use super::groups::{GroupsMsg, go, small};
use crate::lattice::{Msg, STOPPED, State, on};
use crate::theme;
use crate::ui::{self, label, note};

type El<'a> = Element<'a, Msg>;

/// The record being shown.
#[derive(Debug)]
pub struct RecordView {
    pub group: String,
    /// As last read (`None`: being read).
    pub record: Option<Record>,
    pub notes: text_editor::Content,
    /// The notes differ from the record's.
    pub edited: bool,
}

#[derive(Debug, Clone)]
pub enum RecordMsg {
    /// Show the group's record, or put it away (`None`).
    Show(Option<String>),
    Read(String, Option<Record>),
    Notes(text_editor::Action),
    SaveNotes,
    /// Take a decision off the record.
    TakeOff(String),
    Done(Result<(), String>),
}

pub fn send(msg: RecordMsg) -> Msg {
    go(GroupsMsg::Record(msg))
}

impl State {
    fn read_record(&self, group: String) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let chat = services.chat.clone();
        let asked = group.clone();
        Task::perform(on(&services, async move { chat.thread_record(&asked) }), move |r| send(RecordMsg::Read(group.clone(), r.flatten())))
    }

    pub(in crate::lattice) fn record(&mut self, msg: RecordMsg) -> Task<Msg> {
        let groups = &mut self.ide.groups;
        match msg {
            RecordMsg::Show(None) => groups.record = None,
            RecordMsg::Show(Some(group)) => {
                groups.record = Some(RecordView { group: group.clone(), record: None, notes: text_editor::Content::new(), edited: false });
                return self.read_record(group);
            }
            RecordMsg::Read(group, record) => {
                if let Some(view) = groups.record.as_mut().filter(|v| v.group == group) {
                    // Notes being edited are kept; otherwise they are the record's.
                    if !view.edited {
                        view.notes = text_editor::Content::with_text(record.as_ref().map_or("", |r| r.notes.as_str()));
                    }
                    view.record = record;
                }
            }
            RecordMsg::Notes(action) => {
                if let Some(view) = groups.record.as_mut() {
                    view.edited |= action.is_edit();
                    view.notes.perform(action);
                }
            }
            RecordMsg::SaveNotes => {
                let Some(view) = groups.record.as_mut() else { return Task::none() };
                let (group, notes) = (view.group.clone(), view.notes.text().trim_end().to_string());
                view.edited = false;
                return self.on_record(group, move |chat, g| chat.record_notes(g, &notes));
            }
            RecordMsg::TakeOff(id) => {
                let Some(view) = groups.record.as_ref() else { return Task::none() };
                let group = view.group.clone();
                return self.on_record(group, move |chat, g| chat.record_take_off(g, &id));
            }
            RecordMsg::Done(Err(why)) => groups.said = Some(why),
            RecordMsg::Done(Ok(())) => {
                if let Some(group) = groups.record.as_ref().map(|v| v.group.clone()) {
                    return self.read_record(group);
                }
            }
        }
        Task::none()
    }

    /// A write to the record of `group`, then it is read again.
    fn on_record(
        &self,
        group: String,
        work: impl FnOnce(&lattice_core::convo::agent::AgentChat, &str) -> Result<(), String> + Send + 'static,
    ) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let chat = services.chat.clone();
        Task::perform(on(&services, async move { work(&chat, &group) }), |r| {
            send(RecordMsg::Done(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }
}

/// The record, under the open group's actions.
pub fn view<'a>(view: &'a RecordView) -> El<'a> {
    let mut col = Column::new().spacing(8).width(Length::Fill);
    col = col.push(
        row![label("Its record", 11.5, theme::GOLD), iced::widget::space().width(Length::Fill), small("Close", Some(send(RecordMsg::Show(None))))]
            .align_y(Alignment::Center),
    );
    col = col.push(note("Every session of this group starts with it. It is built from their work and your answers; your notes come first."));
    let Some(record) = &view.record else {
        return pad(col.push(note("Reading\u{2026}")));
    };
    col = col.push(label("Your notes", 10.5, theme::TEXT_FAINT));
    col = col.push(
        text_editor(&view.notes)
            .placeholder("What every session of this group should know: goals, limits, what to avoid\u{2026}")
            .on_action(|a| send(RecordMsg::Notes(a)))
            .height(Length::Fixed(72.0))
            .size(11.5)
            .padding(6),
    );
    if view.edited {
        col = col.push(small("Save the notes", Some(send(RecordMsg::SaveNotes))));
    }
    if !record.stands.is_empty() {
        col = col.push(label("Where it stands", 10.5, theme::TEXT_FAINT));
        for stand in &record.stands {
            col = col.push(column![label(ui::cut(&stand.title, 60), 11.5, theme::TEXT), label(ui::cut(&stand.text, 400), 11.0, theme::TEXT_DIM)]);
        }
    }
    if !record.decisions.is_empty() {
        col = col.push(label("Decided", 10.5, theme::TEXT_FAINT));
        for decision in &record.decisions {
            let who = match decision.by {
                By::Reader => "you",
                By::Agent => "an agent",
            };
            col = col.push(
                Row::new()
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .push(container(label(ui::cut(&decision.text, 200), 11.0, theme::TEXT)).width(Length::Fill))
                    .push(label(who, 10.0, theme::TEXT_FAINT))
                    .push(small("Take off", Some(send(RecordMsg::TakeOff(decision.id.clone()))))),
            );
        }
    }
    if !record.open.is_empty() {
        col = col.push(label("Still open", 10.5, theme::TEXT_FAINT));
        for open in record.open.iter().take(20) {
            col = col.push(label(ui::cut(open, 200), 11.0, theme::TEXT_DIM));
        }
    }
    if !record.pointers.is_empty() {
        col = col.push(label("Where the work is", 10.5, theme::TEXT_FAINT));
        for pointer in record.pointers.iter().rev().take(10) {
            col = col.push(label(ui::cut(pointer, 200), 11.0, theme::TEXT_DIM));
        }
    }
    pad(col)
}

fn pad(col: Column<'_, Msg>) -> El<'_> {
    container(col).padding(Padding { top: 4.0, right: 10.0, bottom: 8.0, left: 30.0 }).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record shows its notes as read; notes being edited are not overwritten by a later read; a read for another
    /// group is not shown; closing it, or opening another group, puts it away.
    /// Mutants: a read replacing the reader's unsaved notes; another group's record shown.
    #[test]
    fn the_record_keeps_notes_being_edited_and_shows_only_its_groups_record() {
        let mut state = State::new();
        let record = |notes: &str| Record { group: "g".into(), name: "Parser".into(), notes: notes.into(), ..Record::default() };
        let _ = state.update(send(RecordMsg::Show(Some("g".into()))));
        assert!(state.ide.groups.record.as_ref().is_some_and(|v| v.group == "g" && v.record.is_none()), "being read");
        let _ = state.update(send(RecordMsg::Read("other".into(), Some(record("not this one")))));
        assert!(state.ide.groups.record.as_ref().unwrap().record.is_none());
        let _ = state.update(send(RecordMsg::Read("g".into(), Some(record("Ship Friday.")))));
        assert_eq!(state.ide.groups.record.as_ref().unwrap().notes.text().trim_end(), "Ship Friday.");
        let _ = state.update(send(RecordMsg::Notes(text_editor::Action::Edit(text_editor::Edit::Insert('!')))));
        assert!(state.ide.groups.record.as_ref().unwrap().edited);
        let typed = state.ide.groups.record.as_ref().unwrap().notes.text();
        let _ = state.update(send(RecordMsg::Read("g".into(), Some(record("Ship Friday.")))));
        assert_eq!(state.ide.groups.record.as_ref().unwrap().notes.text(), typed, "unsaved notes are kept");
        let _ = state.update(send(RecordMsg::Show(None)));
        assert!(state.ide.groups.record.is_none());
        let _ = state.update(send(RecordMsg::Show(Some("g".into()))));
        let _ = state.update(go(GroupsMsg::Open(Some("h".into()))));
        assert!(state.ide.groups.record.is_some(), "a group the window does not list is not opened");
    }
}
