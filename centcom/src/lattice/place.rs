//! Lattice as the person left it (`crate::session`): taken whenever it changes, put back at start.
//!
//! The tab, the panes and the model and mode are put back at once. The folder, its files and the chat wait for
//! Lattice's services: the folder opens as one named on the command line does, its files open once it is listed, and
//! the chat opens once the list of chats holds it (a chat deleted meanwhile is passed over). Until each has been put
//! back, the place still names it, so a window closed before Lattice was opened forgets nothing.

use lattice_protocol::conversation::Mode;

use super::ide::Side;
use super::{State, Tab};
use crate::session::LatticePlace;

/// What still waits to be put back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    pub folder: Option<String>,
    /// The folder the files belong to, once it has been handed on to open.
    pub files_of: Option<String>,
    pub files: Vec<String>,
    pub active: Option<String>,
    pub chat: Option<String>,
}

impl Pending {
    pub fn done(&self) -> bool {
        self.folder.is_none() && self.files.is_empty() && self.chat.is_none()
    }
}

/// A side view's name in the session (its title carries its shortcut, which may change).
fn side_name(side: Side) -> &'static str {
    match side {
        Side::Work => "work",
        Side::Explorer => "explorer",
        Side::Search => "search",
        Side::Chats => "chats",
        Side::Changes => "changes",
        Side::Tools => "tools",
    }
}

/// A tab's name in the session, as `--tab` names it ([`Tab::from_name`]).
fn tab_name(tab: Tab) -> &'static str {
    match tab {
        Tab::Chat => "chat",
        Tab::Runs => "runs",
        Tab::Models => "models",
        Tab::Training => "training",
        Tab::Morphometry => "morphometry",
        Tab::Foundry => "foundry",
        Tab::System => "system",
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Ask => "ask",
        Mode::Agent => "agent",
    }
}

impl State {
    /// Lattice as it is now, with what still waits to be put back in place of what has not yet opened.
    pub fn session_place(&self) -> LatticePlace {
        let ide = &self.ide;
        let mut place = LatticePlace {
            tab: Some(tab_name(self.tab).to_string()),
            // the folder on show, or the one opening
            folder: (ide.folder.is_some() || ide.opening).then(|| ide.folder_path.trim().to_string()).filter(|p| !p.is_empty()),
            // the reader's own editor's files: an agent's editor goes with its work
            files: ide
                .tabs
                .iter()
                .filter(|t| t.editor == super::ide::YOURS && matches!(t.kind, super::ide::TabKind::File { .. }))
                .filter_map(|t| t.path().map(str::to_owned))
                .collect(),
            active: ide
                .active_tab()
                .filter(|t| t.editor == super::ide::YOURS && matches!(t.kind, super::ide::TabKind::File { .. }))
                .and_then(|t| t.path().map(str::to_owned)),
            side: Some(side_name(ide.side).to_string()),
            side_open: Some(ide.side_open),
            panel_open: Some(ide.panel_open),
            side_width: Some(ide.side_width),
            agent_width: Some(ide.agent_width),
            panel_height: Some(ide.panel_height),
            editor_open: Some(ide.editor_open),
            editor_width: Some(ide.editor_width),
            follow: Some(ide.follow),
            chat: self.open.as_ref().map(|c| c.id().to_string()),
            model: Some(self.choice.clone()).filter(|c| !c.is_empty() && c != "auto"),
            mode: Some(mode_name(self.mode).to_string()),
        };
        if let Some(p) = &self.restoring {
            if p.folder.is_some() {
                place.folder = p.folder.clone();
            }
            if !p.files.is_empty() {
                place.files = p.files.clone();
                place.active = p.active.clone();
            }
            if p.chat.is_some() {
                place.chat = p.chat.clone();
            }
        }
        place
    }

    /// Put `place` back: at once what needs nothing, the rest once the services run (see the module's note).
    pub fn restore_place(&mut self, place: LatticePlace) {
        if let Some(tab) = place.tab.as_deref().and_then(Tab::from_name) {
            self.tab = tab;
        }
        let ide = &mut self.ide;
        if let Some(side) = place.side.as_deref().and_then(|t| Side::ALL.into_iter().find(|x| side_name(*x) == t)) {
            ide.side = side;
        }
        if let Some(open) = place.side_open {
            ide.side_open = open;
        }
        if let Some(open) = place.panel_open {
            ide.panel_open = open;
        }
        // sizes within reason: a file edited by hand cannot hide a pane or swallow the window
        if let Some(w) = place.side_width {
            ide.side_width = w.clamp(160.0, 900.0);
        }
        if let Some(w) = place.agent_width {
            ide.agent_width = w.clamp(260.0, 1200.0);
        }
        if let Some(h) = place.panel_height {
            ide.panel_height = h.clamp(90.0, 900.0);
        }
        if let Some(open) = place.editor_open {
            ide.editor_open = open;
        }
        if let Some(w) = place.editor_width {
            ide.editor_width = w.clamp(super::ide::EDITOR_RANGE.0, super::ide::EDITOR_RANGE.1);
        }
        if let Some(follow) = place.follow {
            ide.follow = follow;
        }
        if let Some(model) = place.model {
            // checked against the models once they are listed (`Msg::Choices` falls back when it is gone)
            self.choice = model;
        }
        self.mode = match place.mode.as_deref() {
            Some("agent") => Mode::Agent,
            _ => self.mode,
        };
        let pending = Pending { folder: place.folder, files_of: None, files: place.files, active: place.active, chat: place.chat };
        self.restoring = (!pending.done()).then_some(pending);
    }

    /// The services run: the folder left open opens, unless the command line named one.
    pub(super) fn restore_folder(&mut self) {
        let Some(p) = &mut self.restoring else { return };
        if let Some(folder) = p.folder.take() {
            if self.ide.startup.is_none() {
                p.files_of = Some(folder.clone());
                self.ide.startup = Some((folder, None));
            } else {
                // the command line's folder wins: the files left open were another folder's
                p.files.clear();
                p.files_of = None;
                p.active = None;
            }
        }
        self.restoring.take_if(|p| p.done());
    }

    /// The folder is listed: the files left open open again, in their order, the one on show last shown.
    pub(super) fn restore_files(&mut self) -> iced::Task<super::Msg> {
        let Some(p) = &mut self.restoring else { return iced::Task::none() };
        // another folder opened first: the files were not its
        if p.files.is_empty() || p.files_of.as_deref() != Some(self.ide.folder_path.trim()) {
            return iced::Task::none();
        }
        let files = std::mem::take(&mut p.files);
        let active = p.active.take();
        p.files_of = None;
        self.restoring.take_if(|p| p.done());
        // the files coming back do not bring the editor with them: it shows as it was left
        let shown = self.ide.editor_open;
        let tasks: Vec<_> = files.into_iter().map(|f| self.open_file(f)).collect();
        self.ide.editor_open = shown;
        if let Some(id) = active.and_then(|a| self.ide.file_tab(&a)) {
            self.ide.active = Some(id);
        }
        iced::Task::batch(tasks)
    }

    /// The folder could not open: its files are forgotten with it.
    pub(super) fn restore_failed(&mut self) {
        if let Some(p) = &mut self.restoring {
            p.files.clear();
            p.files_of = None;
            p.active = None;
        }
        self.restoring.take_if(|p| p.done());
    }

    /// The chats are listed: the one left open opens, if it is still there and no other has been opened.
    pub(super) fn restore_chat(&mut self) -> iced::Task<super::Msg> {
        let Some(p) = &mut self.restoring else { return iced::Task::none() };
        let Some(id) = p.chat.take() else { return iced::Task::none() };
        self.restoring.take_if(|p| p.done());
        let there = self.list.as_ref().is_some_and(|l| l.conversations.iter().any(|c| c.id == id));
        if there && self.open.is_none() && self.opening.is_none() {
            return self.update(super::Msg::Open(id));
        }
        iced::Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panes_model_and_mode_come_back_at_once_and_the_rest_waits_and_is_still_named() {
        let mut state = State::new();
        let place = LatticePlace {
            tab: Some("models".into()),
            folder: Some(r"C:\src\proj".into()),
            files: vec!["a.rs".into(), "b.rs".into()],
            active: Some("a.rs".into()),
            side: Some("search".into()),
            side_open: Some(false),
            panel_open: Some(true),
            side_width: Some(5000.0),
            agent_width: Some(400.0),
            panel_height: Some(200.0),
            editor_open: Some(true),
            editor_width: Some(9000.0),
            follow: Some(false),
            chat: Some("c-1".into()),
            model: Some("hosted:x".into()),
            mode: Some("agent".into()),
        };
        state.restore_place(place.clone());
        assert_eq!(state.tab, Tab::Models);
        assert_eq!((state.ide.side, state.ide.side_open, state.ide.panel_open), (Side::Search, false, true));
        assert_eq!(state.ide.side_width, 900.0, "kept within reason");
        assert_eq!((state.ide.editor_open, state.ide.follow), (true, false), "the editor beside the chat comes back as left");
        assert_eq!(state.ide.editor_width, crate::lattice::ide::EDITOR_RANGE.1, "its width kept within reason");
        assert_eq!((state.choice.as_str(), state.mode), ("hosted:x", Mode::Agent));
        // Nothing has opened yet, and the place still names all of it.
        let now = state.session_place();
        assert_eq!(now.folder, place.folder);
        assert_eq!(now.files, place.files);
        assert_eq!(now.active, place.active);
        assert_eq!(now.chat, place.chat);
        // The services run: the folder is handed to the start-up path, as a command line's would be.
        state.restore_folder();
        assert_eq!(state.ide.startup, Some((r"C:\src\proj".to_string(), None)));
        assert_eq!(state.session_place().files, place.files, "the files still wait for the listing");
        // The folder would not open: its files are let go, and only the chat still waits.
        state.restore_failed();
        assert_eq!(state.restoring, Some(Pending { chat: Some("c-1".into()), ..Pending::default() }));
        // The chat is not in the list (deleted meanwhile): passed over, and nothing waits any more.
        let _ = state.restore_chat();
        assert!(state.restoring.is_none());
        assert!(state.open.is_none() && state.opening.is_none());
    }

    #[test]
    fn a_folder_named_on_the_command_line_wins_and_takes_the_files_left_open_with_it() {
        let mut state = State::new();
        state.ide.startup = Some(("D:\\other".into(), None));
        state.restore_place(LatticePlace { folder: Some("C:\\left".into()), files: vec!["x.rs".into()], ..LatticePlace::default() });
        state.restore_folder();
        assert_eq!(state.ide.startup, Some(("D:\\other".to_string(), None)));
        assert!(state.restoring.is_none());
    }

    #[test]
    fn nothing_left_means_nothing_waits() {
        let mut state = State::new();
        state.restore_place(LatticePlace::default());
        assert!(state.restoring.is_none());
        assert_eq!(state.session_place().files, Vec::<String>::new());
    }
}
