//! Notes in the editor: explanations attached to lines of a file, shown beside them and never written into it
//! (lattice-core's `notes`, kept beside the folder in Lattice's own state).
//!
//! While Notes is on (the file bar's toggle), a file's notes are read when it opens and again after it is saved or
//! read again, and each is placed against the text on disk: a gold mark beside the first line of each one found, and
//! a strip under the editor listing them all, those whose lines changed as no longer matching. An agent's note comes
//! from the reason it gave for an edit; one whose change still waits for review says so. Add a note writes one on the
//! selected lines (of the file as saved: an unsaved file is saved first).

use std::collections::HashMap;

use iced::widget::{Column, Row, button, container, rich_text, scrollable, space, span, text, text_input};
use iced::{Alignment, Element, Length, Padding, Task};
use lattice_core::notes::{Author, NewNote, Note, Notes, Place, place};
use lattice_protocol::conversation::ChangeState;

use super::{Body, CODE_PAD, CODE_SIZE, IdeMsg, LINE_HEIGHT, TabKind, view::ghost};
use crate::lattice::{Msg, STOPPED, State, off_thread};
use crate::theme::{self, fonts};
use crate::ui::{self, label};

type El<'a> = Element<'a, Msg>;

/// A note and where its lines are now.
#[derive(Debug, Clone)]
pub struct Placed {
    pub note: Note,
    pub place: Place,
}

/// A note being written: the file, its lines, the words.
#[derive(Debug, Clone)]
pub struct Draft {
    pub path: String,
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// The notes' state.
#[derive(Debug)]
pub struct NotesState {
    /// Notes are shown (the toggle; on by default).
    pub shown: bool,
    /// The open files' notes, placed, by path.
    pub by_path: HashMap<String, Vec<Placed>>,
    pub draft: Option<Draft>,
    /// The note picked from its mark.
    pub open: Option<String>,
}

impl Default for NotesState {
    fn default() -> Self {
        Self { shown: true, by_path: HashMap::new(), draft: None, open: None }
    }
}

#[derive(Debug, Clone)]
pub enum NotesMsg {
    Show(bool),
    Read(String, Result<Vec<Note>, String>),
    /// A note on the selected lines of the file on show.
    Add,
    Draft(String),
    Save,
    Cancel,
    Saved(String, Result<Note, String>),
    Remove(String, String),
    Removed(String, Result<(), String>),
    /// Pick a note (its mark was pressed): it is marked in the strip and its first line shown.
    Open(String),
}

pub fn go(msg: NotesMsg) -> Msg {
    Msg::Ide(IdeMsg::Notes(msg))
}

/// The notes store, in Lattice's own state.
fn store(state: &lattice_core::StateRoot) -> Notes {
    Notes::new(state, lattice_core::clock::system_clock())
}

impl State {
    /// Read `path`'s notes again (while Notes is on and a folder is open).
    pub(in crate::lattice) fn read_notes(&mut self, path: String) -> Task<Msg> {
        let (Some(services), Some(folder)) = (self.services.clone(), self.ide.folder.clone()) else { return Task::none() };
        if !self.ide.line_notes.shown {
            return Task::none();
        }
        let id = folder.id().to_string();
        let state = services.state.clone();
        let asked = path.clone();
        Task::perform(off_thread(move || store(&state).of_file(&id, &asked)), move |r| {
            go(NotesMsg::Read(path.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    /// The text on disk of the tab showing `path`, as last opened or saved.
    fn saved_text(&self, path: &str) -> Option<String> {
        let tab = self.ide.tab(self.ide.file_tab(path)?)?;
        match &tab.kind {
            TabKind::File { body: Body::Editing(e), .. } => Some(e.saved.clone()),
            _ => None,
        }
    }

    pub(in crate::lattice) fn notes(&mut self, msg: NotesMsg) -> Task<Msg> {
        match msg {
            NotesMsg::Show(on) => {
                self.ide.line_notes.shown = on;
                self.ide.line_notes.by_path.clear();
                if !on {
                    self.ide.line_notes.draft = None;
                    return Task::none();
                }
                let paths: Vec<String> = self
                    .ide
                    .tabs
                    .iter()
                    .filter_map(|t| match &t.kind {
                        TabKind::File { path, body: Body::Editing(_), .. } => Some(path.clone()),
                        _ => None,
                    })
                    .collect();
                return Task::batch(paths.into_iter().map(|p| self.read_notes(p)));
            }
            NotesMsg::Read(path, result) => match result {
                Ok(notes) => {
                    let Some(text) = self.saved_text(&path) else { return Task::none() };
                    let placed = notes.into_iter().map(|note| Placed { place: place(&note, &text), note }).collect();
                    self.ide.line_notes.by_path.insert(path, placed);
                }
                Err(why) => self.problem = Some(why),
            },
            NotesMsg::Add => {
                let Some(tab) = self.ide.active_tab() else { return Task::none() };
                let TabKind::File { path, body: Body::Editing(e), .. } = &tab.kind else { return Task::none() };
                if e.dirty {
                    self.problem = Some("Save the file first: a note is written on the file as it is on disk.".to_string());
                    return Task::none();
                }
                let cursor = e.content.cursor();
                let at = cursor.position.line;
                let other = cursor.selection.map_or(at, |s| s.line);
                let (start, end) = (at.min(other) as u32 + 1, at.max(other) as u32 + 1);
                let path = path.clone();
                self.ide.line_notes.shown = true;
                self.ide.line_notes.draft = Some(Draft { path, start, end, text: String::new() });
                return iced::widget::operation::focus(NOTE_INPUT);
            }
            NotesMsg::Draft(words) => {
                if let Some(d) = &mut self.ide.line_notes.draft {
                    d.text = words;
                }
            }
            NotesMsg::Cancel => self.ide.line_notes.draft = None,
            NotesMsg::Save => {
                let Some(d) = self.ide.line_notes.draft.clone() else { return Task::none() };
                let (Some(services), Some(folder)) = (self.services.clone(), self.ide.folder.clone()) else { return Task::none() };
                let Some(text) = self.saved_text(&d.path) else { return Task::none() };
                let id = folder.id().to_string();
                let state = services.state.clone();
                let path = d.path.clone();
                let new = NewNote { path: d.path, start: d.start, end: d.end, text: d.text, author: Author::Reader, chat: None, change: None };
                return Task::perform(off_thread(move || store(&state).add(&id, &text, new)), move |r| {
                    go(NotesMsg::Saved(path.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            NotesMsg::Saved(path, result) => match result {
                Ok(_) => {
                    self.ide.line_notes.draft = None;
                    return self.read_notes(path);
                }
                Err(why) => self.problem = Some(why),
            },
            NotesMsg::Remove(path, note) => {
                let (Some(services), Some(folder)) = (self.services.clone(), self.ide.folder.clone()) else { return Task::none() };
                let id = folder.id().to_string();
                let state = services.state.clone();
                return Task::perform(off_thread(move || store(&state).remove(&id, &note)), move |r| {
                    go(NotesMsg::Removed(path.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            NotesMsg::Removed(path, result) => {
                if let Err(why) = result {
                    self.problem = Some(why);
                }
                return self.read_notes(path);
            }
            NotesMsg::Open(id) => {
                let found = self.ide.line_notes.by_path.iter().find_map(|(path, notes)| {
                    notes.iter().find(|p| p.note.id == id).and_then(|p| Some((path.clone(), p.place.lines()?.0)))
                });
                self.ide.line_notes.open = Some(id);
                if let Some((path, line)) = found
                    && let Some(tab) = self.ide.file_tab(&path)
                {
                    return self.reveal_in(tab, line, 0, 0);
                }
            }
        }
        Task::none()
    }
}

impl State {
    /// A debug build's photograph (`CENTCOM_LATTICE_NOTES="<start>-<end>:<you|agent>:<words>|..."`): the notes are
    /// added to the file just opened when it has none yet, then read. A release build never reads this.
    #[cfg(debug_assertions)]
    pub(in crate::lattice) fn photograph_notes(&mut self, path: String) -> Task<Msg> {
        let Some(spec) = std::env::var("CENTCOM_LATTICE_NOTES").ok() else { return Task::none() };
        let (Some(services), Some(folder), Some(text)) = (self.services.clone(), self.ide.folder.clone(), self.saved_text(&path)) else {
            return Task::none();
        };
        let id = folder.id().to_string();
        let notes = store(&services.state);
        if notes.of_file(&id, &path).is_ok_and(|n| n.is_empty()) {
            for one in spec.split('|') {
                let mut parts = one.splitn(3, ':');
                let (Some(lines), Some(who), Some(words)) = (parts.next(), parts.next(), parts.next()) else { continue };
                let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
                let (Ok(start), Ok(end)) = (start.parse(), end.parse()) else { continue };
                let author = if who == "agent" { Author::Agent } else { Author::Reader };
                let new = NewNote { path: path.clone(), start, end, text: words.to_string(), author, chat: None, change: None };
                let _ = notes.add(&id, &text, new);
            }
        }
        self.read_notes(path)
    }
}

/// The draft's text box.
pub const NOTE_INPUT: &str = "lattice-note-input";

/// The marks beside the line numbers: a diamond on the first line of each note found, gold for the agent's and the
/// text colour for yours; pressing one picks the note.
pub fn marks<'a>(state: &'a State, path: &str, lines: usize) -> Option<El<'a>> {
    let notes = state.ide.line_notes.by_path.get(path).filter(|_| state.ide.line_notes.shown)?;
    let mut firsts: Vec<(u32, &Placed)> = notes.iter().filter_map(|p| Some((p.place.lines()?.0, p))).collect();
    if firsts.is_empty() {
        return None;
    }
    firsts.sort_by_key(|(line, _)| *line);
    firsts.dedup_by_key(|(line, _)| *line);
    let mut spans = Vec::with_capacity(firsts.len() * 2 + 1);
    let mut at = 1u32;
    for (line, p) in firsts {
        if line as usize > lines {
            break;
        }
        spans.push(span("\n".repeat((line - at) as usize)));
        let colour = if p.note.author == Author::Agent { theme::GOLD } else { theme::TEXT };
        spans.push(span("\u{25C6}").color(colour).link(p.note.id.clone()));
        at = line;
    }
    let marks = rich_text(spans)
        .on_link_click(|id: String| go(NotesMsg::Open(id)))
        .font(fonts().mono)
        .size(CODE_SIZE * 0.8)
        .line_height(text::LineHeight::Absolute(LINE_HEIGHT.into()));
    Some(container(marks).padding(Padding { top: CODE_PAD, bottom: CODE_PAD, left: 0.0, right: 4.0 }).into())
}

/// The strip under the editor: the file's notes, then the one being written.
pub fn strip<'a>(state: &'a State, path: &'a str) -> Option<El<'a>> {
    let ide = &state.ide;
    if !ide.line_notes.shown {
        return None;
    }
    let notes = ide.line_notes.by_path.get(path).map_or(&[][..], |v| &v[..]);
    let draft = ide.line_notes.draft.as_ref().filter(|d| d.path == path);
    if notes.is_empty() && draft.is_none() {
        return None;
    }
    let waiting = |change: &Option<String>| {
        change.as_ref().is_some_and(|id| {
            state.changes.as_ref().is_some_and(|s| {
                s.changes.iter().any(|c| &c.id == id && matches!(c.state, ChangeState::Pending | ChangeState::Rebased))
            })
        })
    };
    let mut col = Column::new().spacing(2);
    for p in notes {
        let picked = ide.line_notes.open.as_deref() == Some(p.note.id.as_str());
        let (where_, faint) = match p.place {
            Place::At { start, end } | Place::Moved { start, end } => (lines_words(start, end), false),
            Place::Stale if waiting(&p.note.change) => ("with its change, waiting for review".to_string(), true),
            Place::Stale => ("its lines changed".to_string(), true),
        };
        let who = if p.note.author == Author::Agent { "Agent" } else { "You" };
        let mut line = Row::new().spacing(8).align_y(Alignment::Start);
        line = line.push(label("\u{25C6}", 11.0, if faint { theme::TEXT_FAINT } else if p.note.author == Author::Agent { theme::GOLD } else { theme::TEXT }));
        line = line.push(label(format!("{where_} \u{00B7} {who}"), 11.5, if picked { theme::GOLD } else { theme::TEXT_FAINT }).width(Length::Shrink));
        line = line.push(label(p.note.text.as_str(), 12.0, if faint { theme::TEXT_DIM } else { theme::TEXT }).width(Length::Fill));
        line = line.push(ghost("Remove", Some(go(NotesMsg::Remove(path.to_string(), p.note.id.clone())))));
        let mut item = button(line).width(Length::Fill).padding([3.0, 10.0]).style(theme::list_row(picked));
        if p.place.lines().is_some() {
            item = item.on_press(go(NotesMsg::Open(p.note.id.clone())));
        }
        col = col.push(item);
    }
    if let Some(d) = draft {
        let input = text_input(&format!("A note on {}", lines_words(d.start, d.end)), &d.text)
            .id(NOTE_INPUT)
            .on_input(|t| go(NotesMsg::Draft(t)))
            .on_submit(go(NotesMsg::Save))
            .padding([5.0, 8.0])
            .size(12.5)
            .style(ui::input_style);
        col = col.push(
            Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([4.0, 10.0])
                .push(input)
                .push(ui::primary("Save note", (!d.text.trim().is_empty()).then(|| go(NotesMsg::Save))))
                .push(ghost("Cancel", Some(go(NotesMsg::Cancel)))),
        );
    }
    let body = container(scrollable(col).style(theme::scrollbars)).max_height(150.0).padding(Padding { top: 4.0, bottom: 4.0, left: 0.0, right: 0.0 });
    Some(Column::new().push(container(space().height(1)).width(Length::Fill).style(theme::line)).push(body).into())
}

/// "line 3" or "lines 3–5".
pub fn lines_words(start: u32, end: u32) -> String {
    if start == end { format!("line {start}") } else { format!("lines {start}\u{2013}{end}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::ide::buffer;
    use crate::lattice::ide::highlight::Lang;
    use iced::widget::text_editor;

    fn note(id: &str, start: u32, quote: &str, author: Author) -> Note {
        Note {
            id: id.into(),
            path: "a.rs".into(),
            start,
            end: start + quote.matches('\n').count() as u32,
            quote: quote.into(),
            sha256: String::new(),
            text: "why".into(),
            author,
            chat: None,
            change: None,
            created: 0.0,
        }
    }

    fn open(state: &mut State, text: &str) -> u64 {
        let editing = super::super::Editing::new("sha".into(), false, buffer::decode(text.as_bytes()).unwrap(), Lang::Rust);
        state.ide.push(TabKind::File { path: "a.rs".into(), lang: Lang::Rust, body: Body::Editing(Box::new(editing)), note: None })
    }

    #[test]
    fn notes_read_are_placed_on_the_file_as_saved_and_hiding_them_forgets_them() {
        let mut state = State::new();
        open(&mut state, "x\nfn a() {}\nfn b() {}\n");
        let read = vec![note("n_1", 1, "fn a() {}", Author::Agent), note("n_2", 3, "gone", Author::Reader)];
        let _ = state.notes(NotesMsg::Read("a.rs".into(), Ok(read)));
        let placed = &state.ide.line_notes.by_path["a.rs"];
        assert_eq!(placed[0].place, Place::Moved { start: 2, end: 2 });
        assert_eq!(placed[1].place, Place::Stale);
        // A file not open has nowhere to place its notes.
        let _ = state.notes(NotesMsg::Read("b.rs".into(), Ok(vec![note("n_3", 1, "x", Author::Reader)])));
        assert!(!state.ide.line_notes.by_path.contains_key("b.rs"));
        let _ = state.notes(NotesMsg::Show(false));
        assert!(!state.ide.line_notes.shown && state.ide.line_notes.by_path.is_empty());
    }

    #[test]
    fn a_note_is_added_on_the_selected_lines_of_a_saved_file_only() {
        let mut state = State::new();
        let id = open(&mut state, "a\nb\nc\nd\n");
        let editor = |state: &mut State, action| {
            let _ = state.ide_update(IdeMsg::Edit(id, action));
        };
        editor(&mut state, text_editor::Action::Move(text_editor::Motion::Down));
        editor(&mut state, text_editor::Action::Select(text_editor::Motion::Down));
        editor(&mut state, text_editor::Action::Select(text_editor::Motion::Down));
        let _ = state.notes(NotesMsg::Add);
        let draft = state.ide.line_notes.draft.clone().expect("a draft");
        assert_eq!((draft.path.as_str(), draft.start, draft.end), ("a.rs", 2, 4));
        assert_eq!(lines_words(draft.start, draft.end), "lines 2\u{2013}4");
        let _ = state.notes(NotesMsg::Cancel);
        editor(&mut state, text_editor::Action::Edit(text_editor::Edit::Insert('x')));
        let _ = state.notes(NotesMsg::Add);
        assert!(state.ide.line_notes.draft.is_none(), "an unsaved file is saved first");
        assert!(state.problem.as_deref().is_some_and(|p| p.starts_with("Save the file first")));
    }
}
