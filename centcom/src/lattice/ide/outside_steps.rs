//! Checklists any agent outside Lattice's chats gives the reader (Claude Code, Codex or Gemini CLI sessions, a
//! script): the local store `tools/user_steps.py` writes (`lattice_core::user_steps`), watched while the chat shows and
//! listed on the side bar's Work view with the chat's own lists, each named by the agent that asked.
//!
//! A check, the switch and Done write the list's state file; the list is done when its last step is checked with the
//! switch on, or at Done. The agent that asked is waiting on that file (`user_steps.py wait`), whichever agent it is.

use std::path::PathBuf;

use iced::widget::{Column, checkbox, column};
use iced::{Element, Task};

use crate::theme;
use lattice_core::user_steps::{self, ExternalList};

use super::{IdeMsg, agent};
use crate::lattice::{Msg, State};
use crate::ui::{self, label};

type El<'a> = Element<'a, Msg>;

/// What the outside checklists ask.
#[derive(Debug, Clone)]
pub enum OutsideMsg {
    /// The store changed (or the chat came on show): read it again.
    Changed,
    Read(Vec<ExternalList>),
    Check(String, usize, bool),
    Notify(String, bool),
    Done(String),
    Acted(Result<(), String>),
}

fn go(msg: OutsideMsg) -> Msg {
    Msg::Ide(IdeMsg::Outside(msg))
}

/// The store's folder, for this process.
pub fn store() -> PathBuf {
    user_steps::dir(&lattice_core::env::ProcessEnv)
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl State {
    pub(in crate::lattice) fn outside(&mut self, msg: OutsideMsg) -> Task<Msg> {
        match msg {
            OutsideMsg::Changed => return read_outside(),
            OutsideMsg::Read(lists) => self.ide.outside_steps = lists,
            OutsideMsg::Check(id, step, done) => {
                return act(move |dir| user_steps::check(&dir, &id, step, done, now()).map(|_| ()));
            }
            OutsideMsg::Notify(id, notify) => return act(move |dir| user_steps::set_notify(&dir, &id, notify, now()).map(|_| ())),
            OutsideMsg::Done(id) => return act(move |dir| user_steps::mark_done(&dir, &id, now()).map(|_| ())),
            OutsideMsg::Acted(result) => {
                if let Err(why) = result {
                    self.problem = Some(why);
                }
                return read_outside();
            }
        }
        Task::none()
    }
}

/// Read the store off the window's thread (it is made when missing, so it can be watched).
pub(in crate::lattice) fn read_outside() -> Task<Msg> {
    Task::perform(
        crate::lattice::off_thread(|| {
            let dir = store();
            let _ = std::fs::create_dir_all(&dir);
            user_steps::open(&dir)
        }),
        |read| go(OutsideMsg::Read(read.unwrap_or_default())),
    )
}

fn act(work: impl FnOnce(PathBuf) -> Result<(), String> + Send + 'static) -> Task<Msg> {
    Task::perform(crate::lattice::off_thread(move || work(store())), |r| {
        go(OutsideMsg::Acted(r.unwrap_or_else(|| Err("The checklist could not be saved.".to_string()))))
    })
}

/// The open lists, pinned above the composer: each named by its agent, each step a box, the switch and Done; folded,
/// the list's head alone.
pub fn bars(state: &State) -> Vec<El<'_>> {
    state
        .ide
        .outside_steps
        .iter()
        .map(|list| {
            let key = format!("outside:{}", list.id);
            let open = !state.ide.folded_steps.contains(&key);
            let checked = list.checked.iter().filter(|c| **c).count();
            let head = agent::steps_head(
                open,
                key,
                column![
                    label(list.title.as_str(), 12.5, theme::TEXT),
                    label(format!("{checked} of {} done \u{00B7} {}", list.steps.len(), list.agent), 11.0, theme::TEXT_DIM),
                ]
                .spacing(1),
            );
            if !open {
                return agent::card(Column::new().push(head));
            }
            let foot = agent::steps_foot(
                checkbox(list.notify)
                    .label("Tell the agent when all are done")
                    .on_toggle({
                        let id = list.id.clone();
                        move |on| go(OutsideMsg::Notify(id.clone(), on))
                    })
                    .size(13)
                    .text_size(11.0)
                    .into(),
                ui::secondary("Done", Some(go(OutsideMsg::Done(list.id.clone())))),
            );
            let mut col = Column::new().spacing(4).push(head);
            for (n, (text, done)) in list.steps.iter().zip(&list.checked).enumerate() {
                let id = list.id.clone();
                col = col.push(
                    checkbox(*done)
                        .label(format!("{}. {text}", n + 1))
                        .on_toggle(move |on| go(OutsideMsg::Check(id.clone(), n, on)))
                        .size(14)
                        .text_size(12.0),
                );
            }
            agent::card(col.push(foot))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_shows_the_open_lists_and_a_refusal_is_said() {
        let mut state = State::new();
        let list = ExternalList {
            id: "steps_0011223344556677".into(),
            title: "Connect OpenRouter".into(),
            steps: vec!["Open the Models page.".into()],
            agent: "Codex session".into(),
            folder: String::new(),
            checked: vec![false],
            notify: true,
            done: false,
            created: 1.0,
        };
        let _ = state.outside(OutsideMsg::Read(vec![list.clone()]));
        assert_eq!(state.ide.outside_steps, [list]);
        assert_eq!(bars(&state).len(), 1);
        let _ = state.outside(OutsideMsg::Acted(Err("That checklist is gone.".into())));
        assert_eq!(state.problem.as_deref(), Some("That checklist is gone."));
    }

    #[test]
    fn a_list_folds_to_its_head_and_all_fold_or_open_together() {
        let mut state = State::new();
        let list = |id: &str| ExternalList {
            id: id.into(),
            title: "Connect OpenRouter".into(),
            steps: vec!["Open the Models page.".into(), "Paste the key.".into()],
            agent: "Codex session".into(),
            folder: String::new(),
            checked: vec![false, false],
            notify: true,
            done: false,
            created: 1.0,
        };
        let _ = state.outside(OutsideMsg::Read(vec![list("steps_0011223344556677"), list("steps_8899aabbccddeeff")]));
        let first = "outside:steps_0011223344556677".to_string();
        let _ = state.ide_update(IdeMsg::FoldSteps(first.clone()));
        assert!(state.ide.folded_steps.contains(&first), "a press on its head folds the list");
        let _ = state.ide_update(IdeMsg::FoldSteps(first.clone()));
        assert!(!state.ide.folded_steps.contains(&first), "and a second press opens it again");
        let _ = state.ide_update(IdeMsg::FoldAllSteps(true));
        assert_eq!(state.ide.folded_steps.len(), 2, "Collapse all folds every list shown");
        assert_eq!(bars(&state).len(), 2, "a folded list keeps its card, its head");
        let _ = state.ide_update(IdeMsg::FoldAllSteps(false));
        assert!(state.ide.folded_steps.is_empty(), "Expand all opens them all");
    }
}
