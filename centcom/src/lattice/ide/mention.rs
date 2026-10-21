//! The composer's mentions: a topic of the open group, `@topic`, and a file, `@path` (as Cursor and Claude Code
//! take them; the core's `convo::mentions` gives the agent's model the files a message names).
//!
//! A topic comes first: in an open group, the topics whose titles hold what is typed after the `@`. Choosing one
//! takes the `@word` out of the text and addresses the message to that topic (`GroupsState::to_topic`, shown over the
//! composer): it goes there instead of where the group would route it.
//!
//! Typing `@` at the start of the composer or after a space lists the open folder's files, ranked against what is typed
//! after it as the Explorer's filter ranks them. Up and Down choose, and Enter or Tab puts `@path ` in the composer in
//! place of what was typed; Esc closes the list until the text changes. A mention is text: choosing one reads nothing
//! here; the core reads the file when the message is sent, as the agent's own reads would.

use iced::widget::{Column, Row, button, container, operation, text_editor};
use iced::{Alignment, Element, Length, Task};

use crate::theme;
use lattice_protocol::conversation::RankedPath;

use super::IdeMsg;
use crate::lattice::{Msg, State};
use crate::ui::{self, label, mono};

type El<'a> = Element<'a, Msg>;

/// At most this many entries are shown at once, and at most [`TOPICS`] of them topics.
pub const SHOWN: usize = 8;
pub const TOPICS: usize = 3;

/// One entry of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A topic of the open group: its id and title.
    Topic(String, String),
    File(RankedPath),
}

/// The open group's topics whose titles hold `query` (any case), in the group's order; none with no group open.
fn topics(state: &State, query: &str) -> Vec<Entry> {
    let (Some(group), Some(list)) = (state.ide.groups.shown(), state.list.as_ref()) else { return Vec::new() };
    let wanted = query.to_lowercase();
    group
        .topics
        .iter()
        .filter_map(|id| list.conversations.iter().find(|c| c.id == *id))
        .filter(|c| !c.title.is_empty() && c.title.to_lowercase().contains(&wanted))
        .take(TOPICS)
        .map(|c| Entry::Topic(c.id.clone(), c.title.clone()))
        .collect()
}

/// The list's state.
#[derive(Debug, Default)]
pub struct Mentioning {
    pub selected: usize,
    /// What was typed after the `@`, to start the choice again when it changes.
    pub query: String,
    /// The list was closed for this text (Esc, or a choice): it opens again when the text changes.
    pub closed_for: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MentionMsg {
    /// Up (-1) or Down (+1).
    Move(i32),
    /// Choose the one shown at this place (`None`: the one chosen).
    Pick(Option<usize>),
    Close,
}

fn go(msg: MentionMsg) -> Msg {
    Msg::Ide(IdeMsg::MentionList(msg))
}

/// What is typed after an `@` that ends the composer's text (at its start or after a space), or `None`.
pub fn query(text: &str) -> Option<&str> {
    let word_start = text.rfind(char::is_whitespace).map_or(0, |at| at + 1);
    let word = &text[word_start..];
    word.strip_prefix('@')
}

/// What the list shows now: the open group's matching topics, then the folder's files ranked against what is typed
/// (the first ones when nothing is); nothing when the list is closed or no `@` is being typed.
pub fn matches(state: &State) -> Vec<Entry> {
    let text = state.composer.text();
    if state.ide.mention.closed_for.as_deref() == Some(text.as_str()) {
        return Vec::new();
    }
    let Some(query) = query(&text) else { return Vec::new() };
    let mut shown = topics(state, query);
    let Some(listing) = state.ide.files() else { return shown };
    let room = SHOWN - shown.len();
    let files = if query.is_empty() {
        listing.paths.iter().take(room).map(|path| RankedPath { path: path.clone(), score: 0, positions: Vec::new() }).collect()
    } else {
        lattice_core::text::rank::rank_paths(query, listing.paths.iter().map(String::as_str), room)
    };
    shown.extend(files.into_iter().map(Entry::File));
    shown
}

/// Whether the list is shown (and so takes Enter, Tab, Up, Down and Esc from the composer).
pub fn open(state: &State) -> bool {
    !matches(state).is_empty()
}

impl State {
    pub(in crate::lattice) fn mention(&mut self, msg: MentionMsg) -> Task<Msg> {
        match msg {
            MentionMsg::Move(delta) => {
                let shown = matches(self).len();
                if shown > 0 {
                    let at = self.ide.mention.selected as i64 + i64::from(delta);
                    self.ide.mention.selected = at.rem_euclid(shown as i64) as usize;
                }
            }
            MentionMsg::Pick(at) => {
                let shown = matches(self);
                let Some(picked) = shown.get(at.unwrap_or(self.ide.mention.selected)) else { return Task::none() };
                let text = self.composer.text();
                let Some(typed) = query(&text) else { return Task::none() };
                let kept = &text[..text.len() - typed.len() - 1];
                let next = match picked {
                    Entry::File(file) => format!("{kept}@{} ", file.path),
                    // A topic is not text: the message is addressed to it.
                    Entry::Topic(id, _) => {
                        self.ide.groups.to_topic = Some(id.clone());
                        kept.to_string()
                    }
                };
                self.composer = text_editor::Content::with_text(&next);
                self.composer.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                self.ide.mention.closed_for = Some(self.composer.text());
                return operation::focus(super::composer_id());
            }
            MentionMsg::Close => self.ide.mention.closed_for = Some(self.composer.text()),
        }
        Task::none()
    }

    /// After the composer's text changed: open the list again for new text, and start the choice again when what is
    /// typed after the `@` changes.
    pub(in crate::lattice) fn mention_edited(&mut self) {
        let text = self.composer.text();
        if self.ide.mention.closed_for.as_deref().is_some_and(|closed| closed != text) {
            self.ide.mention.closed_for = None;
        }
        let typed = query(&text).unwrap_or_default().to_string();
        if self.ide.mention.query != typed {
            self.ide.mention.query = typed;
            self.ide.mention.selected = 0;
        }
    }
}

/// The list over the composer, when it is open.
pub fn menu<'a>(state: &'a State) -> Option<El<'a>> {
    let shown = matches(state);
    let chosen = state.ide.mention.selected.min(shown.len().checked_sub(1)?);
    let mut col = Column::new().spacing(1);
    let any_file = shown.iter().any(|e| matches!(e, Entry::File(_)));
    for (at, entry) in shown.into_iter().enumerate() {
        let line = match &entry {
            Entry::File(ranked) => {
                let name = ranked.path.rsplit('/').next().unwrap_or(&ranked.path).to_string();
                Row::new()
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .push(mono(format!("@{}", ui::cut(&name, 32)), 12.0, theme::GOLD))
                    .push(label(ui::cut(&ranked.path, 72), 11.5, theme::TEXT_DIM).width(Length::Fill))
            }
            Entry::Topic(_, title) => Row::new()
                .spacing(8)
                .align_y(Alignment::Center)
                .push(label(format!("@{}", ui::cut(title, 40)), 12.0, theme::GOLD))
                .push(label("a topic of this group: the message goes to it", 11.5, theme::TEXT_DIM).width(Length::Fill)),
        };
        col = col.push(
            button(line).width(Length::Fill).padding([4.0, 8.0]).style(theme::list_row(at == chosen)).on_press(go(MentionMsg::Pick(Some(at)))),
        );
    }
    let foot = if any_file {
        "Enter or Tab mentions it: a file is given to the agent with the message, a topic gets the message \u{00B7} Esc closes"
    } else {
        "Enter or Tab sends the message to that topic \u{00B7} Esc closes"
    };
    col = col.push(container(label(foot, 10.5, theme::TEXT_FAINT)).padding([3.0, 8.0]));
    Some(container(col).padding(4).width(Length::Fill).style(theme::card).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_after_an_at_that_ends_the_text_is_the_query() {
        assert_eq!(query("@"), Some(""));
        assert_eq!(query("Compare @src/ma"), Some("src/ma"));
        assert_eq!(query("line one\n@b"), Some("b"));
        assert_eq!(query("mail me@x"), None);
        assert_eq!(query("@a.rs done"), None);
        assert_eq!(query("plain words"), None);
    }
}
