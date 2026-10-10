//! The composer's file mentions, `@path` (as Cursor and Claude Code
//! take them; the core's `convo::mentions` gives the agent's model the files a message names).
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

/// At most this many files are shown at once.
pub const SHOWN: usize = 8;

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

/// The files the list shows now: the folder's, ranked against what is typed (the first ones when nothing is); none
/// when the list is closed, no folder is open, or no `@` is being typed.
pub fn matches(state: &State) -> Vec<RankedPath> {
    let text = state.composer.text();
    if state.ide.mention.closed_for.as_deref() == Some(text.as_str()) {
        return Vec::new();
    }
    let (Some(query), Some(listing)) = (query(&text), state.ide.files()) else { return Vec::new() };
    if query.is_empty() {
        return listing
            .paths
            .iter()
            .take(SHOWN)
            .map(|path| RankedPath { path: path.clone(), score: 0, positions: Vec::new() })
            .collect();
    }
    lattice_core::text::rank::rank_paths(query, listing.paths.iter().map(String::as_str), SHOWN)
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
                let next = format!("{kept}@{} ", picked.path);
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
    for (at, ranked) in shown.into_iter().enumerate() {
        let name = ranked.path.rsplit('/').next().unwrap_or(&ranked.path).to_string();
        let line = Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(mono(format!("@{}", ui::cut(&name, 32)), 12.0, theme::GOLD))
            .push(label(ui::cut(&ranked.path, 72), 11.5, theme::TEXT_DIM).width(Length::Fill));
        col = col.push(
            button(line).width(Length::Fill).padding([4.0, 8.0]).style(theme::list_row(at == chosen)).on_press(go(MentionMsg::Pick(Some(at)))),
        );
    }
    let foot = "Enter or Tab mentions the file: the agent is given it with the message \u{00B7} Esc closes";
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
