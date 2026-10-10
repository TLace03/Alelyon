//! The composer's commands, `/name` (the first part of the plugins feature, since 2026-10-08).
//!
//! Typing `/` at the start of the composer lists the reader's own commands and the open folder's (when it is
//! trusted), as lattice-core reads them (`lattice_core::commands`). What is typed after the `/` narrows the list; Up
//! and Down choose, and Enter or Tab puts the chosen command's text in the composer, what follows its name filled in
//! as its arguments, to be read and changed before it is sent. Esc closes the list until the text changes. A command
//! is text: choosing one runs nothing, sends nothing and grants nothing.

use iced::widget::{Column, Row, button, container, operation, text_editor};
use iced::{Alignment, Element, Length, Task};

use crate::theme;
use lattice_core::commands::{Command, Commands, Source, expand};

use super::IdeMsg;
use crate::lattice::{Msg, State, on};
use crate::ui::{self, label, mono};

type El<'a> = Element<'a, Msg>;

/// At most this many commands are shown at once.
pub const SHOWN: usize = 8;

/// The list's state.
#[derive(Debug, Default)]
pub struct Slash {
    /// What the core listed, once it has answered (read again at each new `/`).
    pub commands: Option<Commands>,
    pub reading: bool,
    /// The name typed so far, to start the choice again when it changes.
    pub name: String,
    pub selected: usize,
    /// The list was closed for this text (Esc, or a choice): it opens again when the text changes.
    pub closed_for: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SlashMsg {
    Read(Commands),
    /// Up (-1) or Down (+1).
    Move(i32),
    /// Choose the one shown at this place (`None`: the one chosen).
    Pick(Option<usize>),
    Close,
}

fn go(msg: SlashMsg) -> Msg {
    Msg::Ide(IdeMsg::Slash(msg))
}

/// The composer's text as a command: the name typed after the `/` and what follows it, or `None` when the text does
/// not start with `/` or runs past one line.
pub fn query(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let body = text.strip_prefix('/')?;
    if body.contains('\n') {
        return None;
    }
    Some(body.split_once(char::is_whitespace).unwrap_or((body, "")))
}

/// The commands the list shows now: those whose name starts with what is typed, then those containing it; none when
/// the list is closed or not read yet.
pub fn matches(state: &State) -> Vec<&Command> {
    let slash = &state.ide.slash;
    let text = state.composer.text();
    if slash.closed_for.as_deref() == Some(text.as_str()) {
        return Vec::new();
    }
    let (Some((name, _)), Some(commands)) = (query(&text), &slash.commands) else { return Vec::new() };
    let name = name.to_lowercase();
    let mut first: Vec<&Command> = Vec::new();
    let mut then: Vec<&Command> = Vec::new();
    for command in &commands.commands {
        let own = command.name.to_lowercase();
        if own.starts_with(&name) {
            first.push(command);
        } else if own.contains(&name) {
            then.push(command);
        }
    }
    first.extend(then);
    first.truncate(SHOWN);
    first
}

/// Whether the list is shown (and so takes Enter, Tab, Up, Down and Esc from the composer).
pub fn open(state: &State) -> bool {
    !matches(state).is_empty()
}

impl State {
    pub(in crate::lattice) fn slash(&mut self, msg: SlashMsg) -> Task<Msg> {
        match msg {
            SlashMsg::Read(commands) => {
                self.ide.slash.reading = false;
                self.ide.slash.commands = Some(commands);
                self.ide.slash.selected = 0;
            }
            SlashMsg::Move(delta) => {
                let shown = matches(self).len();
                if shown > 0 {
                    let at = self.ide.slash.selected as i64 + i64::from(delta);
                    self.ide.slash.selected = at.rem_euclid(shown as i64) as usize;
                }
            }
            SlashMsg::Pick(at) => {
                let picked = {
                    let shown = matches(self);
                    let at = at.unwrap_or(self.ide.slash.selected);
                    shown.get(at).map(|command| (*command).clone())
                };
                let Some(command) = picked else { return Task::none() };
                let text = self.composer.text();
                let arguments = query(&text).map_or("", |(_, arguments)| arguments);
                let expanded = expand(&command, arguments);
                self.composer = text_editor::Content::with_text(&expanded);
                self.composer.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                self.ide.slash.closed_for = Some(self.composer.text());
                return operation::focus(super::composer_id());
            }
            SlashMsg::Close => self.ide.slash.closed_for = Some(self.composer.text()),
        }
        Task::none()
    }

    /// After the composer's text changed: read the commands at a new `/`, start the choice again when the name
    /// changes, and forget them when the text is no command.
    pub(in crate::lattice) fn slash_edited(&mut self) -> Task<Msg> {
        let text = self.composer.text();
        if self.ide.slash.closed_for.as_deref().is_some_and(|closed| closed != text) {
            self.ide.slash.closed_for = None;
        }
        let Some((name, _)) = query(&text) else {
            self.ide.slash.commands = None;
            self.ide.slash.selected = 0;
            return Task::none();
        };
        if self.ide.slash.name != name {
            self.ide.slash.name = name.to_owned();
            self.ide.slash.selected = 0;
        }
        // A new `/` reads the commands again (the folder, or a file, may have changed).
        if text == "/" && !self.ide.slash.reading {
            self.ide.slash.commands = None;
        }
        if self.ide.slash.commands.is_some() || self.ide.slash.reading {
            return Task::none();
        }
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.slash.reading = true;
        let chat = services.chat.clone();
        let workspace = self.chat_workspace();
        Task::perform(on(&services, async move { chat.commands(workspace).await }), |commands| {
            go(SlashMsg::Read(commands.unwrap_or_default()))
        })
    }
}

/// Where a command is from, in a word or a short path.
fn from(command: &Command) -> String {
    match command.source {
        Source::User => "yours".to_string(),
        Source::Plugin => format!("plugin {}", command.name.split(':').next().unwrap_or_default()),
        Source::Folder => command.path.rsplit_once('/').map_or(command.path.clone(), |(dir, _)| dir.to_string()),
    }
}

/// The list over the composer, when it is open.
pub fn menu<'a>(state: &'a State) -> Option<El<'a>> {
    let shown = matches(state);
    let chosen = state.ide.slash.selected.min(shown.len().checked_sub(1)?);
    let mut col = Column::new().spacing(1);
    for (at, command) in shown.iter().enumerate() {
        let mut line = Row::new().spacing(8).align_y(Alignment::Center);
        line = line.push(mono(format!("/{}", command.name), 12.0, theme::GOLD));
        if !command.hint.is_empty() {
            line = line.push(label(ui::cut(&command.hint, 28), 11.5, theme::TEXT_FAINT));
        }
        line = line.push(label(ui::cut(&command.description, 64), 11.5, theme::TEXT_DIM).width(Length::Fill));
        line = line.push(label(ui::cut(&from(command), 28), 10.5, theme::TEXT_FAINT));
        col = col.push(
            button(line).width(Length::Fill).padding([4.0, 8.0]).style(theme::list_row(at == chosen)).on_press(go(SlashMsg::Pick(Some(at)))),
        );
    }
    let mut foot = "Enter or Tab puts its text in the composer \u{00B7} Esc closes".to_string();
    if !shown[chosen].set_aside.is_empty() {
        foot = format!("{} set aside (a command is only text) \u{00B7} {foot}", shown[chosen].set_aside.join(", "));
    }
    col = col.push(container(label(foot, 10.5, theme::TEXT_FAINT)).padding([3.0, 8.0]));
    Some(container(col).padding(4).width(Length::Fill).style(theme::card).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str, body: &str, source: Source) -> Command {
        Command {
            id: format!("user:{name}.md"),
            name: name.into(),
            source,
            path: if source == Source::User { format!("(yours) {name}.md") } else { format!(".claude/commands/{name}.md") },
            description: format!("About {name}"),
            hint: String::new(),
            set_aside: vec![],
            body: body.into(),
        }
    }

    fn typed(state: &mut State, text: &str) {
        state.composer = text_editor::Content::with_text(text);
        let _ = state.slash_edited();
    }

    fn listed(state: &State) -> Vec<String> {
        matches(state).iter().map(|command| command.name.clone()).collect()
    }

    fn ready() -> State {
        let mut state = State::new();
        typed(&mut state, "/");
        let _ = state.slash(SlashMsg::Read(Commands {
            commands: vec![
                command("review", "Review $ARGUMENTS carefully.", Source::User),
                command("frontend:test", "Run the frontend tests.", Source::Folder),
                command("preview", "Preview it.", Source::User),
            ],
            notices: vec![],
        }));
        state
    }

    #[test]
    fn the_name_typed_narrows_the_list_and_text_that_is_no_command_shows_none() {
        assert_eq!(query("/review src/main.rs"), Some(("review", "src/main.rs")));
        assert_eq!(query("/review\n"), Some(("review", "")));
        assert_eq!(query("/two\nlines"), None);
        assert_eq!(query("review"), None);
        let mut state = ready();
        assert_eq!(listed(&state), ["review", "frontend:test", "preview"]);
        typed(&mut state, "/rev");
        assert_eq!(listed(&state), ["review", "preview"], "a name's start comes before its middle");
        typed(&mut state, "/etc/hosts is broken");
        assert!(!open(&state), "nothing matches, so Enter sends");
        typed(&mut state, "Not a command");
        assert!(state.ide.slash.commands.is_none(), "read again at the next /");
    }

    #[test]
    fn a_choice_puts_the_commands_text_in_the_composer_and_sends_nothing() {
        let mut state = ready();
        typed(&mut state, "/rev src/main.rs");
        let _ = state.slash(SlashMsg::Pick(None));
        assert_eq!(state.composer.text(), "Review src/main.rs carefully.");
        assert!(!state.sending, "nothing was sent");
        assert!(!open(&state));
        typed(&mut state, "/");
        let _ = state.slash(SlashMsg::Read(Commands {
            commands: vec![command("review", "Review.", Source::User), command("preview", "Preview it.", Source::User)],
            notices: vec![],
        }));
        let _ = state.slash(SlashMsg::Move(-1));
        assert_eq!(state.ide.slash.selected, 1, "Up from the first wraps to the last");
        let _ = state.slash(SlashMsg::Move(1));
        assert_eq!(state.ide.slash.selected, 0);
        let _ = state.slash(SlashMsg::Pick(Some(1)));
        assert_eq!(state.composer.text(), "Preview it.");
    }

    #[test]
    fn esc_closes_the_list_until_the_text_changes() {
        let mut state = ready();
        typed(&mut state, "/re");
        assert!(open(&state));
        let _ = state.slash(SlashMsg::Close);
        assert!(!open(&state));
        typed(&mut state, "/rev");
        assert!(open(&state), "a change opens it again");
        typed(&mut state, "/re");
        assert!(open(&state), "and the text it was closed for, typed again, shows it too");
    }
}
