//! The command palette: every page, tab and command of the window, found by typing a few of its letters.
//!
//! Ctrl+Shift+P (or F1) opens it over whatever is on show, as in the editors people already know. What was run from it
//! lately comes first while nothing is typed; once something is, the entries holding its letters in order are ranked
//! (a letter starting a word counts more, letters side by side more still). Up and Down choose, Enter runs, Esc or a
//! press beside it closes it. Each entry shows its own shortcut, when it has one, so the palette also teaches the keys.
//!
//! An entry is a list of the window's own messages, run in order, so the palette can do nothing a button cannot.

use iced::widget::{Column, button, column, container, mouse_area, row, space, stack, text_input};
use iced::{Alignment, Element, Length, padding};

use crate::app::{App, Message, Tab};
use crate::capability::Capability;
use crate::catalogue::Section;
use crate::lattice;
use crate::lattice::ide::IdeMsg;
use crate::lattice::ide::jump::{self, JumpMsg, Target};
use crate::lattice::ide::update::Key;
use crate::theme;
use crate::ui::{self, label, mono, note};

/// The box's id, focused when the palette opens.
pub const INPUT: &str = "centcom-palette";

/// The rows on show at once: typing narrows the rest.
pub const SHOWN: usize = 14;

/// How many entries run lately are kept, the latest first.
pub const RECENT_KEPT: usize = 30;

/// The palette, while it is open.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub query: String,
    /// The chosen row, among those shown.
    pub selected: usize,
}

#[derive(Clone, Debug)]
pub enum PaletteMsg {
    /// Ctrl+Shift+P or F1: open it, or close it when it is open.
    Toggle,
    Query(String),
    /// Up (-1) or Down (+1).
    Move(i32),
    /// Run the row chosen (Enter), or the row at this place in the list (a press on it).
    Run(Option<usize>),
    Close,
}

/// One thing the palette can do.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Stable across runs: what the recent list keeps.
    pub id: String,
    pub group: &'static str,
    pub title: String,
    /// Its shortcut, when it has one.
    pub keys: &'static str,
    pub run: Vec<Message>,
}

const SECTION_KEYS: [&str; 9] = ["Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4", "Ctrl+5", "Ctrl+6", "Ctrl+7", "Ctrl+8", "Ctrl+9"];

/// The IDE's own commands, as its keys name them.
const EDITOR: [(&str, &str, Key); 15] = [
    ("Go to file", "Ctrl+P", Key::QuickOpen),
    ("Search the folder", "Ctrl+Shift+F", Key::Search),
    ("Find in the file", "Ctrl+F", Key::Find),
    ("Replace in the file", "Ctrl+H", Key::Replace),
    ("Go to line", "Ctrl+G", Key::GotoLine),
    ("Save", "Ctrl+S", Key::Save),
    ("Close the tab", "Ctrl+W", Key::CloseTab),
    ("Next tab", "Ctrl+Tab", Key::NextTab),
    ("Previous tab", "Ctrl+Shift+Tab", Key::PreviousTab),
    ("Reopen the closed tab", "Ctrl+Shift+T", Key::Reopen),
    ("Show or hide the side bar", "Ctrl+B", Key::ToggleSide),
    ("Show or hide the bottom panel", "Ctrl+J", Key::TogglePanel),
    ("Show or hide the editor beside the chat", "Ctrl+E", Key::ToggleEditor),
    ("Ask the agent to edit the selection", "Ctrl+K", Key::Inline),
    ("Go to the chat's message box", "Ctrl+L", Key::FocusChat),
];

/// The messages that put the window on `section` (none when it is there already: going again would read its page
/// afresh).
fn go(app: &App, section: Section) -> Vec<Message> {
    if app.section == section { Vec::new() } else { vec![Message::Go(section)] }
}

/// The Lattice page with its Chat and IDE on show.
fn to_chat(app: &App) -> Vec<Message> {
    let mut run = go(app, Section::Lattice);
    if app.lattice.tab != lattice::Tab::Chat {
        run.push(Message::Lattice(lattice::Msg::Tab(lattice::Tab::Chat)));
    }
    run
}

/// Everything the palette offers for the window as it is now, in its resting order.
pub fn entries(app: &App) -> Vec<Entry> {
    let mut all = Vec::new();
    for (i, section) in Section::ALL.into_iter().enumerate() {
        all.push(Entry {
            id: format!("go:{}", section.short().to_lowercase()),
            group: "Go to",
            title: section.title().to_string(),
            keys: SECTION_KEYS.get(i).copied().unwrap_or(""),
            // already there: the entry only closes the palette onto the page
            run: go(app, section),
        });
    }
    for tab in Tab::ALL {
        let mut run = go(app, Section::Transcription);
        run.push(Message::Tab(tab));
        all.push(Entry { id: format!("words:{}", tab.title()), group: "Transcription", title: tab.title().to_string(), keys: "", run });
    }
    // Lattice's tabs and side views, and Fleet's tabs, as the jump menu lists them (one list, two doors).
    for (group, words, target) in jump::entries(&app.lattice) {
        let run = match &target {
            Target::Lattice(_) | Target::Side(_) => {
                let mut run = go(app, Section::Lattice);
                run.push(Message::Lattice(jump::go(JumpMsg::Pick(None, target.clone()))));
                run
            }
            // the window takes the tab out of Lattice's state and goes to the Fleet page
            Target::Fleet(_) if app.fleet.is_some() => vec![Message::Lattice(jump::go(JumpMsg::Pick(None, target.clone())))],
            _ => continue,
        };
        all.push(Entry { id: format!("{}:{words}", group.to_lowercase()), group, title: words.to_string(), keys: "", run });
    }
    for (words, keys, key) in EDITOR {
        let mut run = to_chat(app);
        run.push(Message::Lattice(lattice::Msg::Ide(IdeMsg::Key(key))));
        all.push(Entry { id: format!("editor:{words}"), group: "Editor", title: words.to_string(), keys, run });
    }
    if lattice::ONE_SURFACE {
        // One chat surface: the chat box, the projects to work in, and the topics to continue.
        let mut ask = to_chat(app);
        ask.push(Message::Lattice(lattice::Msg::Ide(IdeMsg::Key(Key::FocusChat))));
        all.push(Entry { id: "chat:ask".into(), group: "Chat", title: "Ask the agent".into(), keys: "Ctrl+L", run: ask });
        for project in app.lattice.ide.projects.list.iter().flatten().filter(|p| !p.archived) {
            let mut run = to_chat(app);
            run.push(Message::Lattice(lattice::Msg::WorkIn(Some(project.id.clone()))));
            all.push(Entry { id: format!("project:{}", project.id), group: "Work in", title: project.name.clone(), keys: "", run });
        }
    } else {
        let mut new_chat = to_chat(app);
        new_chat.push(Message::Lattice(lattice::Msg::New));
        new_chat.push(Message::Lattice(lattice::Msg::Ide(IdeMsg::Key(Key::FocusChat))));
        all.push(Entry { id: "chat:new".into(), group: "Chat", title: "New chat".into(), keys: "", run: new_chat });
    }
    // The chats Lattice has listed (newest first): each opens by its title (on one surface, as a topic to continue).
    if let Some(list) = &app.lattice.list {
        let group = if lattice::ONE_SURFACE { "Continue topic" } else { "Open chat" };
        for c in list.conversations.iter().take(40) {
            let mut run = to_chat(app);
            run.push(Message::Lattice(lattice::Msg::Open(c.id.clone())));
            let title = if c.title.trim().is_empty() { "Untitled".to_string() } else { c.title.clone() };
            all.push(Entry { id: format!("chat:{}", c.id), group, title, keys: "", run });
        }
    }
    all.push(Entry { id: "window:keys".into(), group: "Window", title: "Keyboard shortcuts".into(), keys: "Ctrl+Shift+/", run: vec![Message::Sheet(true)] });
    all.push(Entry { id: "window:rail".into(), group: "Window", title: "Put the keyboard on the rail".into(), keys: "F6", run: vec![Message::RailFocus] });
    all.push(Entry { id: "window:models".into(), group: "Window", title: "Models".into(), keys: "", run: vec![Message::Models(crate::models::Msg::Open)] });
    all.push(Entry {
        id: "window:whats-new".into(),
        group: "Window",
        title: "What's new".into(),
        keys: "",
        run: vec![Message::SignIn(crate::signin::Msg::WhatsNew(true))],
    });
    if app.social.active() && app.can(Capability::Friends) {
        all.push(Entry { id: "window:friends".into(), group: "Window", title: "Friends".into(), keys: "", run: vec![Message::Social(crate::social::Msg::Panel(true))] });
    }
    // Each page to a window of its own, or back to the main window from one (`windows`); the main window keeps a page.
    for section in Section::ALL {
        use crate::windows::Msg;
        let (title, run) = match app.windows.of(section) {
            Some(p) => (format!("{}: back to the main window", section.title()), vec![Message::Windows(Msg::Back(p.id))]),
            None if app.section != section || app.windows.instead_of(section).is_some() => {
                (format!("{} in a window of its own", section.title()), vec![Message::Windows(Msg::PopOut(section))])
            }
            None => continue,
        };
        all.push(Entry { id: format!("window:own:{}", section.short().to_lowercase()), group: "Window", title, keys: "", run });
    }
    // ... and each of Transcription's tabs
    for tab in Tab::ALL {
        use crate::windows::{Msg, Part};
        let part = Part::Words(tab);
        let (title, run) = match app.windows.of_part(part) {
            Some(p) => (format!("Transcription's {}: back to its page", tab.title()), vec![Message::Windows(Msg::Back(p.id))]),
            None => (format!("Transcription's {} in a window of its own", tab.title()), vec![Message::Windows(Msg::PopOutPart(part))]),
        };
        all.push(Entry { id: format!("window:own:words:{tab:?}").to_lowercase(), group: "Window", title, keys: "", run });
    }
    // ... and each of Compute's tabs
    for tab in app.compute.tabs() {
        use crate::windows::{Msg, Part};
        let part = Part::Compute(tab);
        let name = app.compute.title(tab);
        let (title, run) = match app.windows.of_part(part) {
            Some(p) => (format!("Compute's {name}: back to its page"), vec![Message::Windows(Msg::Back(p.id))]),
            None => (format!("Compute's {name} in a window of its own"), vec![Message::Windows(Msg::PopOutPart(part))]),
        };
        all.push(Entry { id: format!("window:own:compute:{tab:?}").to_lowercase(), group: "Window", title, keys: "", run });
    }
    // ... and each of Lattice's tabs
    for tab in lattice::Tab::ALL {
        use crate::windows::{Msg, Part};
        let part = Part::Lattice(tab);
        let (title, run) = match app.windows.of_part(part) {
            Some(p) => (format!("Lattice's {}: back to its page", tab.title()), vec![Message::Windows(Msg::Back(p.id))]),
            None => (format!("Lattice's {} in a window of its own", tab.title()), vec![Message::Windows(Msg::PopOutPart(part))]),
        };
        all.push(Entry { id: format!("window:own:lattice:{tab:?}").to_lowercase(), group: "Window", title, keys: "", run });
    }
    for pane in crate::signin::settings::Pane::ALL {
        all.push(Entry {
            id: format!("settings:{}", pane.title()),
            group: "Settings",
            title: pane.title().to_string(),
            keys: "",
            run: vec![Message::SignIn(crate::signin::Msg::Settings), Message::SignIn(crate::signin::Msg::SettingsPane(pane))],
        });
    }
    all
}

/// How well `query` matches `words`: None when its letters (spaces ignored) are not all there in order. A letter that
/// starts a word, or follows the one matched before it, counts more; letters skipped count against.
pub fn score(query: &str, words: &str) -> Option<i32> {
    let want: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if want.is_empty() {
        return Some(0);
    }
    let have: Vec<char> = words.chars().flat_map(char::to_lowercase).collect();
    let (mut at, mut score, mut last) = (0usize, 0i32, None::<usize>);
    for (i, c) in have.iter().enumerate() {
        if at == want.len() {
            break;
        }
        if *c != want[at] {
            continue;
        }
        let starts_word = i == 0 || !have[i - 1].is_alphanumeric();
        score += 1;
        if starts_word {
            score += 8;
        }
        match last {
            Some(prev) if prev + 1 == i => score += 5,
            Some(prev) => score -= ((i - prev - 1) as i32).min(8),
            None => score -= (i as i32).min(4),
        }
        last = Some(i);
        at += 1;
    }
    if at < want.len() {
        return None;
    }
    // typed as it is written, from a word's start, wins outright ("tab" is Next tab's, not the t of Next and the ab of
    // tab); the whole of the words, more so
    let typed: String = want.iter().collect();
    let words: String = have.iter().collect();
    let at_word = words
        .match_indices(&typed)
        .map(|(i, _)| i)
        .find(|&i| i == 0 || !words[..i].chars().last().is_some_and(char::is_alphanumeric));
    if let Some(i) = at_word {
        // scored where it is typed whole, not where the first letters happened to fall
        let n = want.len() as i32;
        let lead = (words[..i].chars().count() as i32).min(4);
        score = score.max(9 + 6 * (n - 1) - lead) + 25;
    }
    if words.starts_with(&typed) {
        score += 10;
    }
    // the more of the words the letters cover, the better: "trst" is Trust before Training Studio
    Some(score + (10 * want.len() / have.len().max(1)) as i32)
}

/// The entries `query` leaves, best first. With nothing typed, those run lately (in `recent`, latest first) lead and
/// the rest keep their resting order; with something typed, the score decides, then how lately each was run.
pub fn ranked(all: Vec<Entry>, query: &str, recent: &[String]) -> Vec<Entry> {
    let lately = |e: &Entry| recent.iter().position(|id| *id == e.id).unwrap_or(usize::MAX);
    let mut scored: Vec<(i32, usize, usize, Entry)> = all
        .into_iter()
        .enumerate()
        .filter_map(|(order, e)| {
            // the title first; the group's name finds its entries too, for less
            let s = score(query, &e.title).or_else(|| score(query, &format!("{} {}", e.group, e.title)).map(|s| s - 10))?;
            Some((s, lately(&e), order, e))
        })
        .collect();
    if query.trim().is_empty() {
        scored.sort_by_key(|(_, lately, order, _)| (*lately, *order));
    } else {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    }
    scored.into_iter().map(|(.., e)| e).collect()
}

/// `id` run just now: first in `recent`, once.
pub fn remember(recent: &mut Vec<String>, id: &str) {
    recent.retain(|r| r != id);
    recent.insert(0, id.to_string());
    recent.truncate(RECENT_KEPT);
}

/// The palette over the window: the box, then the rows it leaves; a press beside it closes it.
pub fn view<'a>(app: &'a App, p: &'a Palette) -> Element<'a, Message> {
    let found = ranked(entries(app), &p.query, &app.palette_recent);
    let more = found.len().saturating_sub(SHOWN);
    let mut list = Column::new().spacing(0);
    if found.is_empty() {
        list = list.push(container(note("Nothing by that name.")).padding(10));
    }
    for (i, e) in found.into_iter().take(SHOWN).enumerate() {
        let selected = i == p.selected;
        let words = row![
            label(e.title, 13.5, if selected { theme::GOLD } else { theme::TEXT }),
            label(e.group, 11.5, theme::TEXT_FAINT),
            space().width(Length::Fill),
            mono(e.keys, 11.5, theme::TEXT_DIM),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        list = list.push(
            button(words)
                .width(Length::Fill)
                .padding([5.0, 12.0])
                .style(theme::list_row(selected))
                .on_press(Message::Palette(PaletteMsg::Run(Some(i)))),
        );
    }
    let hint = if more > 0 {
        format!("{more} more: type to narrow. Up and Down choose, Enter runs, Esc closes.")
    } else {
        "Up and Down choose, Enter runs, Esc closes.".to_string()
    };
    let card = container(
        column![
            text_input("Type a page, a command or a chat…", &p.query)
                .id(INPUT)
                .on_input(|t| Message::Palette(PaletteMsg::Query(t)))
                .on_submit(Message::Palette(PaletteMsg::Run(None)))
                .padding([8.0, 12.0])
                .size(14.0)
                .style(ui::input_style),
            list,
            note(hint),
        ]
        .spacing(6),
    )
    .padding(10)
    .width(620)
    .style(theme::card);
    let backdrop = mouse_area(container(space()).width(Length::Fill).height(Length::Fill)).on_press(Message::Palette(PaletteMsg::Close));
    stack![backdrop, container(card).center_x(Length::Fill).padding(padding::top(72))].into()
}

/// Up and Down while the palette is open: the text box leaves them alone.
pub fn nav_keys(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key as K, key::Named};
    match event {
        iced::Event::Keyboard(Event::KeyPressed { key: K::Named(Named::ArrowUp), .. }) => Some(Message::Palette(PaletteMsg::Move(-1))),
        iced::Event::Keyboard(Event::KeyPressed { key: K::Named(Named::ArrowDown), .. }) => Some(Message::Palette(PaletteMsg::Move(1))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, group: &'static str, title: &str) -> Entry {
        Entry { id: id.into(), group, title: title.into(), keys: "", run: Vec::new() }
    }

    #[test]
    fn letters_in_order_match_and_word_starts_and_runs_count_more() {
        assert!(score("gtf", "Go to file").is_some());
        assert!(score("fgt", "Go to file").is_none(), "out of order");
        assert!(score("zz", "Go to file").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("GO TO", "go to file").is_some(), "case and spaces do not matter");
        assert!(score("sav", "Save").unwrap() > score("sav", "Search all views").unwrap(), "side by side beats spread out");
        assert!(score("nt", "Next tab").unwrap() > score("nt", "Agent runs").unwrap(), "word starts beat the middle of words");
        assert!(score("trst", "Trust").unwrap() > score("trst", "Training Studio").unwrap(), "letters close together win");
        assert!(score("tab", "Next tab").unwrap() > score("tab", "Previous tab").unwrap(), "a word typed whole, in fewer words");
        assert!(score("tab", "Next tab").unwrap() > score("tab", "Settings About").unwrap());
    }

    #[test]
    fn nothing_typed_puts_the_recent_first_and_typing_ranks_by_score() {
        let all = || vec![entry("a", "Go to", "Overview"), entry("b", "Go to", "Lattice"), entry("c", "Editor", "Save")];
        let recent = vec!["c".to_string()];
        let ids = |v: Vec<Entry>| v.into_iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(ranked(all(), "", &recent)), ["c", "a", "b"]);
        assert_eq!(ids(ranked(all(), "lat", &recent)), ["b"]);
        assert_eq!(ids(ranked(all(), "editor", &recent)), ["c"], "a group's name finds its entries");
    }

    #[test]
    fn the_recent_list_keeps_each_once_latest_first_and_stays_short() {
        let mut recent = Vec::new();
        for i in 0..(RECENT_KEPT + 5) {
            remember(&mut recent, &format!("e{i}"));
        }
        remember(&mut recent, "e10");
        assert_eq!(recent.len(), RECENT_KEPT);
        assert_eq!(recent[0], "e10");
        assert_eq!(recent.iter().filter(|r| *r == "e10").count(), 1);
    }
}
