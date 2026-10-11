//! The jump menu: one dropdown on each chat's header that lists every place a chat leads, so the header stays one
//! button wide.
//!
//! The entries are grouped: **This chat** (its changes, its folder in the Explorer, its model), **Lattice** (the
//! page's tabs and the Chat and IDE's side views) and **Fleet** (the Fleet page's tabs, as the build's Fleet plug-in
//! names them; a build without one lists none). A filter narrows them by name. Picking one opens it: a Lattice tab or
//! view here; a Fleet tab on the Fleet page (the window takes [`State::jump_out`] and goes there). An entry about the
//! chat, picked from a chat of the grid that is not the one open on the right, opens that chat there first, as an
//! answer from the grid does.

use iced::widget::{Column, button, column, container, mouse_area, row, scrollable, space, stack, text_input};
use iced::{Alignment, Element, Length, Task, padding};

use super::{IdeMsg, Side};
use crate::lattice::{Msg, State, Tab};
use crate::theme;
use crate::ui::{self, label, note};

type El<'a> = Element<'a, Msg>;

/// Where an entry leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The chat's changes: the Changes view, and the first change waiting, as its diff.
    Changes,
    /// The chat's folder, in the Explorer (opened when the editor shows another).
    Folder,
    /// The Models tab, where the chat's model is listed.
    Model,
    Lattice(Tab),
    Side(Side),
    /// A Fleet tab, by its name.
    Fleet(&'static str),
}

impl Target {
    /// Whether it is about the chat, so the chat must be the one open on the right.
    fn about_the_chat(&self) -> bool {
        matches!(self, Target::Changes | Target::Folder)
    }
}

/// The menu, open: the chat it was opened from (None: the chat on the right), and the filter typed.
#[derive(Debug, Clone, Default)]
pub struct Jump {
    pub chat: Option<String>,
    pub filter: String,
}

#[derive(Debug, Clone)]
pub enum JumpMsg {
    Open(Option<String>),
    Filter(String),
    Close,
    /// The first entry the filter leaves (Enter).
    First,
    Pick(Option<String>, Target),
}

pub fn go(msg: JumpMsg) -> Msg {
    Msg::Ide(IdeMsg::Jump(msg))
}

/// The filter's box.
pub const FILTER: &str = "lattice-jump-filter";

/// Every entry: its group, its words, where it leads.
pub fn entries(state: &State) -> Vec<(&'static str, &'static str, Target)> {
    let mut all = vec![
        ("This chat", "Its changes", Target::Changes),
        ("This chat", "Its folder", Target::Folder),
        ("This chat", "Its model", Target::Model),
    ];
    for tab in Tab::ALL {
        all.push(("Lattice", tab.title(), Target::Lattice(tab)));
    }
    for side in Side::ALL {
        all.push(("Lattice", side.title(), Target::Side(side)));
    }
    for (name, title) in &state.fleet_tabs {
        all.push(("Fleet", title, Target::Fleet(name)));
    }
    all
}

/// The entries the filter leaves: those whose words or group hold it, without regard to case.
pub fn shown(state: &State, filter: &str) -> Vec<(&'static str, &'static str, Target)> {
    let want = filter.trim().to_lowercase();
    entries(state)
        .into_iter()
        .filter(|(group, words, _)| want.is_empty() || words.to_lowercase().contains(&want) || group.to_lowercase().contains(&want))
        .collect()
}

impl State {
    pub(in crate::lattice) fn jump(&mut self, msg: JumpMsg) -> Task<Msg> {
        match msg {
            JumpMsg::Open(chat) => {
                self.ide.jump = Some(Jump { chat, filter: String::new() });
                return iced::widget::operation::focus(FILTER);
            }
            JumpMsg::Filter(text) => {
                if let Some(j) = &mut self.ide.jump {
                    j.filter = text;
                }
            }
            JumpMsg::Close => self.ide.jump = None,
            JumpMsg::First => {
                let Some(j) = self.ide.jump.clone() else { return Task::none() };
                if let Some((_, _, target)) = shown(self, &j.filter).into_iter().next() {
                    return self.jump(JumpMsg::Pick(j.chat, target));
                }
            }
            JumpMsg::Pick(chat, target) => {
                self.ide.jump = None;
                // A chat of the grid that is not on the right: it opens there first, then the entry is taken.
                if target.about_the_chat()
                    && let Some(id) = chat
                    && self.open.as_ref().is_none_or(|c| c.id() != id)
                {
                    let again = Msg::Ide(IdeMsg::Jump(JumpMsg::Pick(None, target)));
                    return self.grid(super::grid::GridMsg::Act(id, Box::new(again)));
                }
                return self.jump_to(target);
            }
        }
        Task::none()
    }

    fn jump_to(&mut self, target: Target) -> Task<Msg> {
        match target {
            Target::Lattice(tab) => self.update(Msg::Tab(tab)),
            Target::Model => self.update(Msg::Tab(Tab::Models)),
            Target::Fleet(name) => {
                self.jump_out = Some(name);
                Task::none()
            }
            Target::Side(side) => self.show_side(side),
            Target::Changes => {
                let first = self.changes.as_ref().and_then(|s| {
                    s.changes
                        .iter()
                        .find(|c| matches!(c.state, lattice_protocol::conversation::ChangeState::Pending | lattice_protocol::conversation::ChangeState::Rebased))
                        .map(|c| c.id.clone())
                });
                let side = self.show_side(Side::Changes);
                match first {
                    Some(change) => Task::batch([side, self.update(Msg::ShowDiff(change))]),
                    None => side,
                }
            }
            Target::Folder => {
                let theirs = self.open.as_ref().and_then(|c| c.summary.workspace.as_ref()).map(|w| (w.id.clone(), w.path.clone()));
                let side = self.show_side(Side::Explorer);
                match theirs {
                    Some((id, path)) if self.ide.folder.as_ref().is_none_or(|f| f.id() != id) => {
                        Task::batch([side, self.ide_update(IdeMsg::OpenRecent(path))])
                    }
                    _ => side,
                }
            }
        }
    }

    /// The Chat and IDE with `side` open (never closing it, as pressing its button again would).
    fn show_side(&mut self, side: Side) -> Task<Msg> {
        let tab = if self.tab != Tab::Chat { self.update(Msg::Tab(Tab::Chat)) } else { Task::none() };
        if self.ide.side == side && self.ide.side_open {
            return tab;
        }
        Task::batch([tab, self.ide_update(IdeMsg::Side(side))])
    }
}

/// The header's button that opens the menu, for the chat `chat` (None: the chat on the right).
pub fn button_for<'a>(chat: Option<String>) -> El<'a> {
    super::view::icon("\u{2304}", "Jump to: this chat's changes and folder, Lattice's tabs and Fleet's", Some(go(JumpMsg::Open(chat))))
}

/// The menu, over the page: the filter, then the entries by group; a click outside closes it.
pub fn overlay<'a>(state: &'a State, j: &'a Jump) -> El<'a> {
    let mut list = Column::new().spacing(0);
    let mut group = "";
    let entries = shown(state, &j.filter);
    if entries.is_empty() {
        list = list.push(container(note("Nothing by that name.")).padding(10));
    }
    for (g, words, target) in entries {
        if g != group {
            group = g;
            list = list.push(container(label(g, 11.0, theme::TEXT_FAINT)).padding(padding::top(8).left(12).bottom(2)));
        }
        list = list.push(
            button(label(words, 13.0, theme::TEXT))
                .width(Length::Fill)
                .padding([4.0, 20.0])
                .style(theme::list_row(false))
                .on_press(go(JumpMsg::Pick(j.chat.clone(), target))),
        );
    }
    let card = container(
        column![
            text_input("Jump to…", &j.filter)
                .id(FILTER)
                .on_input(|t| go(JumpMsg::Filter(t)))
                .on_submit(go(JumpMsg::First))
                .padding([7.0, 10.0])
                .size(13.5)
                .style(ui::input_style),
            // The list scrolls within its own height, so the hint under it stays in view.
            container(scrollable(list).height(Length::Shrink).style(theme::scrollbars)).max_height(440),
            row![note("Enter takes the first, Esc closes.")].align_y(Alignment::Center),
        ]
        .spacing(6),
    )
    .padding(10)
    .width(360)
    .style(theme::card);
    let backdrop = mouse_area(container(space()).width(Length::Fill).height(Length::Fill)).on_press(go(JumpMsg::Close));
    stack![backdrop, container(card).align_right(Length::Fill).padding(padding::top(46).right(14))].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_lists_the_chat_lattice_and_fleets_tabs_and_the_filter_narrows_them() {
        let mut state = State::new();
        state.fleet_tabs = vec![("queue", "Card and slots"), ("worktrees", "Worktrees")];
        let all = entries(&state);
        assert_eq!(all.iter().filter(|e| e.0 == "This chat").count(), 3);
        assert_eq!(all.iter().filter(|e| e.0 == "Lattice").count(), Tab::ALL.len() + Side::ALL.len());
        assert_eq!(all.iter().filter(|e| e.0 == "Fleet").map(|e| e.1).collect::<Vec<_>>(), ["Card and slots", "Worktrees"]);
        let narrowed = shown(&state, " WORK ");
        assert_eq!(narrowed.iter().map(|e| e.2.clone()).collect::<Vec<_>>(), [Target::Fleet("worktrees")]);
        assert_eq!(shown(&state, "fleet").len(), 2, "a group's name finds its entries");
        assert!(shown(&state, "nothing like this").is_empty());
    }

    #[test]
    fn picking_goes_to_the_tab_the_side_view_or_out_to_fleet() {
        let mut state = State::new();
        state.fleet_tabs = vec![("worktrees", "Worktrees")];
        let _ = state.jump(JumpMsg::Open(None));
        assert!(state.ide.jump.is_some());
        let _ = state.jump(JumpMsg::Pick(None, Target::Lattice(Tab::Foundry)));
        assert!(state.ide.jump.is_none(), "a pick closes the menu");
        assert_eq!(state.tab, Tab::Foundry);
        let _ = state.jump(JumpMsg::Pick(None, Target::Side(Side::Search)));
        assert_eq!((state.tab, state.ide.side, state.ide.side_open), (Tab::Chat, Side::Search, true));
        // Picked again, the view stays open (its button would close it).
        let _ = state.jump(JumpMsg::Pick(None, Target::Side(Side::Search)));
        assert!(state.ide.side_open);
        let _ = state.jump(JumpMsg::Pick(None, Target::Fleet("worktrees")));
        assert_eq!(state.jump_out, Some("worktrees"));
        let _ = state.jump(JumpMsg::Open(None));
        let _ = state.jump(JumpMsg::Filter("model".into()));
        let _ = state.jump(JumpMsg::First);
        assert_eq!(state.tab, Tab::Models, "Enter takes the first entry left: Its model");
    }
}
