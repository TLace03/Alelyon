//! The keyboard's sheet (Ctrl+Shift+/, or "Keyboard shortcuts" in the command palette): the window's own keys, then
//! those of the page on show, each group as the code that handles it binds it.
//!
//! The tables are written out by hand beside the code they describe; `tests` hold them to the bindings that can be
//! asked about without a screen (the window's own keys and the IDE's).

use iced::widget::{Column, column, container, mouse_area, row, scrollable, space, stack};
use iced::{Alignment, Element, Length, padding};

use crate::app::{App, Message};
use crate::catalogue::Section;
use crate::lattice;
use crate::theme;
use crate::ui::{label, mono, note, strong};

pub struct Group {
    pub title: &'static str,
    pub keys: &'static [(&'static str, &'static str)],
}

pub const WINDOW: Group = Group {
    title: "Everywhere",
    keys: &[
        ("Ctrl+Shift+P or F1", "The command palette: every page, command, setting and chat"),
        ("Ctrl+1 \u{2026} Ctrl+9", "The page in that place on the rail"),
        ("F6", "The keyboard to the rail: Up and Down choose, Enter opens, Esc leaves"),
        ("Ctrl+Shift+/", "These keys"),
        ("Tab, Shift+Tab", "The next or previous box"),
        ("Esc", "Close what lies on top, one thing per press"),
    ],
};

pub const EDITOR: Group = Group {
    title: "Lattice: the editor",
    keys: &[
        ("Ctrl+P", "Go to a file"),
        ("Ctrl+S", "Save"),
        ("Ctrl+W", "Close the tab"),
        ("Ctrl+Tab, Ctrl+Shift+Tab", "The next or previous tab (also Ctrl+PageDown, Ctrl+PageUp)"),
        ("Ctrl+Shift+T", "Reopen the tab closed last"),
        ("Ctrl+F, Ctrl+H", "Find, replace, in the file (F3, Shift+F3: next, previous)"),
        ("Ctrl+G", "Go to a line"),
        ("Ctrl+Shift+F", "Search the folder"),
        ("Ctrl+K", "Ask the agent to edit the selected lines"),
        ("Ctrl+/", "Comment or uncomment the lines"),
        ("Alt+Up, Alt+Down", "Move the lines (with Shift: copy them)"),
        ("Ctrl+Shift+K", "Delete the lines"),
        ("Ctrl+Z, Ctrl+Y", "Undo, redo"),
        ("Tab, Ctrl+Right", "Take a suggestion whole, or its next word"),
        ("Ctrl+B", "Show or hide the side bar"),
        ("Ctrl+J or Ctrl+`", "Show or hide the bottom panel"),
    ],
};

pub const CHAT: Group = Group {
    title: "Lattice: the chat",
    keys: &[
        ("Ctrl+L", "To the message box"),
        ("Ctrl+N", "A new chat"),
        ("Enter, Shift+Enter", "Send, or a new line"),
        ("Up", "In an empty box: edit the last message you sent"),
        ("Esc", "Stop the agent's turn, or leave an edit as it was"),
        ("/ and @", "A command, or a file of the folder"),
    ],
};

pub const TERMINAL: Group = Group {
    title: "Lattice: the terminal",
    keys: &[
        ("Ctrl+C", "Copy the selection, or interrupt the program when nothing is selected"),
        ("Ctrl+Shift+C, Ctrl+Insert", "Copy"),
        ("Ctrl+V, Shift+Insert", "Paste (a long or many-line paste asks first)"),
        ("Ctrl+P, Ctrl+B, Ctrl+J", "Stay the editor's; every other key goes to the program"),
    ],
};

pub const CREATOR: Group = Group {
    title: "Sinai: Appearance",
    keys: &[("Ctrl+Z", "Undo"), ("Ctrl+Y or Ctrl+Shift+Z", "Redo")],
};

/// The groups for the page on show: the window's first.
pub fn groups(app: &App) -> Vec<&'static Group> {
    let mut all = vec![&WINDOW];
    match app.section {
        Section::Lattice if app.lattice.tab == lattice::Tab::Chat => all.extend([&EDITOR, &CHAT, &TERMINAL]),
        Section::Sinai if app.creator.is_some() => all.push(&CREATOR),
        _ => {}
    }
    all
}

/// The sheet over the window; a press beside it closes it.
pub fn view(app: &App) -> Element<'_, Message> {
    let mut list = Column::new().spacing(14);
    for group in groups(app) {
        let mut rows = Column::new().spacing(5);
        for (keys, what) in group.keys {
            rows = rows.push(
                row![container(mono(*keys, 12.0, theme::GOLD)).width(190), label(*what, 12.5, theme::TEXT_DIM).width(Length::Fill)]
                    .spacing(12)
                    .align_y(Alignment::Center),
            );
        }
        list = list.push(column![strong(group.title, 13.0, theme::TEXT), rows].spacing(6));
    }
    let card = container(
        column![
            scrollable(list).height(Length::Shrink).style(theme::scrollbars),
            note("The command palette shows each command's key beside it. Esc closes this."),
        ]
        .spacing(10),
    )
    .padding(16)
    .width(640)
    .max_height(640)
    .style(theme::card);
    let backdrop = mouse_area(container(space()).width(Length::Fill).height(Length::Fill)).on_press(Message::Sheet(false));
    stack![backdrop, container(card).center_x(Length::Fill).padding(padding::top(64))].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor's sheet names a key for every IDE key a person can press (Esc and the arrows aside, which are
    /// listed as the chat's and the window's).
    #[test]
    fn the_sheet_names_the_ides_and_the_windows_keys() {
        let all: String = [&WINDOW, &EDITOR, &CHAT].iter().flat_map(|g| g.keys.iter().map(|(k, _)| *k)).collect::<Vec<_>>().join(" ");
        for key in [
            "Ctrl+P", "Ctrl+S", "Ctrl+W", "Ctrl+Tab", "Ctrl+Shift+T", "Ctrl+F", "Ctrl+H", "Ctrl+G", "Ctrl+Shift+F", "Ctrl+K",
            "Ctrl+B", "Ctrl+J", "Ctrl+L", "Ctrl+N", "Ctrl+Shift+P", "F1", "F6", "Ctrl+Shift+/", "Ctrl+1",
        ] {
            assert!(all.contains(key), "{key} is not on the sheet");
        }
    }
}
