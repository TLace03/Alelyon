//! The chats grid: several chats side by side, each with its own agent turn running, its status, what it is doing now,
//! what it has changed, and a box to write to it. The core runs one turn per chat, so chats in the grid work at the
//! same time. Oversight stays where it is: an approval any of them asks for opens the one dialog, a chat that needs you
//! says so on its tile, and Open makes a tile the chat on the right, with its whole transcript and its review.

use iced::widget::text::Wrapping;
use iced::widget::{Column, Row, button, container, pick_list, scrollable, space, text_input};
use iced::{Alignment, Element, Length, Padding, Task, padding};
use lattice_protocol::conversation::{Accepted, AgentChatService, ChangeSet, Mode, SendRequest, Snapshot};

use super::IdeMsg;
use crate::lattice::{Msg, STOPPED, State, chat, on};
use crate::theme;
use crate::ui::{self, chip, label, note, strong};

/// The most chats the grid holds at once.
pub const MAX_TILES: usize = 10;

/// One chat in the grid.
pub struct Tile {
    pub id: String,
    /// The chat as last read and followed; None until it is read.
    pub conversation: Option<chat::Conversation>,
    /// Bumped to start the follow again after a stream ended while a turn still runs.
    pub generation: u64,
    pub draft: String,
    pub sending: bool,
    /// The last refusal or failure for this chat, in the core's words.
    pub problem: Option<String>,
    /// The changes this chat made that wait for review, when its folder is attached.
    pub changes: Option<ChangeSet>,
    /// A recording loaded for a photograph (a debug build's `CENTCOM_LATTICE_GRID`): never followed or sent to.
    pub recording: bool,
    /// The mode and the model its next message is sent in: the chat's own until picked in its composer.
    pub mode: Option<Mode>,
    pub choice: Option<String>,
}

impl Tile {
    fn new(id: String) -> Tile {
        Tile { id, conversation: None, generation: 0, draft: String::new(), sending: false, problem: None, changes: None, recording: false, mode: None, choice: None }
    }
}

/// The grid: whether it is on show (in the editor's place), and its chats in the order they were added.
/// What the right column holds while the grid is on show: the editor with its tabs, or the chat panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Beside {
    #[default]
    Editor,
    Chat,
}

#[derive(Default)]
pub struct Grid {
    pub shown: bool,
    pub tiles: Vec<Tile>,
    /// The right column's content while the grid is on show (the reader picks it).
    pub beside: Beside,
    /// An action pressed in a pane whose chat is not the open one: done once that chat has opened.
    pub pending: Option<(String, Box<Msg>)>,
}

impl Grid {
    pub fn tile(&self, id: &str) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.id == id)
    }

    pub fn tile_mut(&mut self, id: &str) -> Option<&mut Tile> {
        self.tiles.iter_mut().find(|t| t.id == id)
    }

    pub fn holds(&self, id: &str) -> bool {
        self.tile(id).is_some()
    }

    /// A batch of a chat's events, for its tile if the grid holds it; whether its changes should be read again.
    pub fn apply(&mut self, id: &str, events: &[lattice_protocol::conversation::ConversationEvent]) -> bool {
        let Some(tile) = self.tile_mut(id) else { return false };
        let Some(c) = &mut tile.conversation else { return false };
        c.apply(events);
        events.iter().any(|e| {
            use lattice_protocol::conversation::ConversationEventKind as Ev;
            matches!(e.kind, Ev::Staged { .. } | Ev::Reviewed { .. } | Ev::Conflict { .. } | Ev::CommandEffect { .. } | Ev::TurnEnded { .. })
        })
    }

    /// A tile's follow ended: start it again if its turn still runs.
    pub fn follow_ended(&mut self, id: &str) {
        if let Some(tile) = self.tile_mut(id)
            && tile.conversation.as_ref().is_some_and(|c| c.running)
        {
            tile.generation += 1;
        }
    }
}

#[derive(Clone, Debug)]
pub enum GridMsg {
    Show(bool),
    /// Put this chat in the grid (and show the grid).
    Add(String),
    Remove(String),
    Opened(String, Result<Snapshot, String>),
    Draft(String, String),
    Send(String),
    Sent(String, Result<Accepted, String>),
    Stop(String),
    Changes(String, Result<ChangeSet, String>),
    /// Make this chat the one on the right.
    Focus(String),
    /// An action in a pane's transcript (an approval, an answer, a review): done in that chat, which opens on the
    /// right first when it is not the open one.
    Act(String, Box<Msg>),
    Mode(String, Mode),
    /// Pick what the right column holds while the grid is on show.
    Beside(Beside),
    /// A model picked in a pane's composer, by its label.
    Choice(String, String),
}

pub fn go(msg: GridMsg) -> Msg {
    Msg::Ide(IdeMsg::Grid(msg))
}

impl State {
    pub(in crate::lattice) fn grid(&mut self, msg: GridMsg) -> Task<Msg> {
        match msg {
            GridMsg::Show(on) => self.ide.grid.shown = on,
            GridMsg::Add(id) => {
                self.ide.grid.shown = true;
                if self.ide.grid.holds(&id) {
                    return Task::none();
                }
                if self.ide.grid.tiles.len() >= MAX_TILES {
                    self.problem = Some(format!("The grid holds {MAX_TILES} chats; take one out first."));
                    return Task::none();
                }
                self.ide.grid.tiles.push(Tile::new(id.clone()));
                return self.grid_open(id);
            }
            GridMsg::Remove(id) => self.ide.grid.tiles.retain(|t| t.id != id),
            GridMsg::Opened(id, result) => {
                let Some(tile) = self.ide.grid.tile_mut(&id) else { return Task::none() };
                match result {
                    Ok(snapshot) => {
                        tile.conversation = Some(chat::Conversation::from_snapshot(snapshot));
                        tile.generation += 1;
                        return self.grid_changes(id);
                    }
                    Err(why) => tile.problem = Some(why),
                }
            }
            GridMsg::Draft(id, text) => {
                if let Some(tile) = self.ide.grid.tile_mut(&id) {
                    tile.draft = text;
                }
            }
            GridMsg::Send(id) => {
                let Some((services, agent)) = self.agent() else { return Task::none() };
                let Some(request) = self.grid_request(&id) else { return Task::none() };
                let Some(tile) = self.ide.grid.tile_mut(&id) else { return Task::none() };
                if tile.sending {
                    return Task::none();
                }
                tile.sending = true;
                tile.problem = None;
                let sent = id.clone();
                return Task::perform(on(&services, async move { agent.send(request).await.map_err(|r| r.message) }), move |r| {
                    go(GridMsg::Sent(sent.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))))
                });
            }
            GridMsg::Sent(id, result) => {
                let Some(tile) = self.ide.grid.tile_mut(&id) else { return Task::none() };
                tile.sending = false;
                match result {
                    Ok(Accepted::Started { .. }) => {
                        tile.draft.clear();
                        if let Some(c) = &mut tile.conversation {
                            c.running = true;
                        }
                        tile.generation += 1;
                    }
                    Ok(Accepted::Queued { position, .. }) => {
                        tile.draft.clear();
                        tile.problem = Some(format!("A turn is running here, so your message waits its turn (number {position} in line)."));
                    }
                    Err(why) => tile.problem = Some(why),
                }
            }
            GridMsg::Stop(id) => {
                if let Some(services) = &self.services {
                    services.chat.stop(&id);
                }
            }
            GridMsg::Changes(id, result) => {
                if let Some(tile) = self.ide.grid.tile_mut(&id) {
                    tile.changes = result.ok();
                }
            }
            GridMsg::Focus(id) => return self.update(Msg::Open(id)),
            GridMsg::Act(id, msg) => {
                if self.open.as_ref().is_some_and(|c| c.id() == id) {
                    return self.update(*msg);
                }
                self.ide.grid.pending = Some((id.clone(), msg));
                return self.update(Msg::Open(id));
            }
            GridMsg::Beside(beside) => self.ide.grid.beside = beside,
            GridMsg::Mode(id, mode) => {
                if let Some(tile) = self.ide.grid.tile_mut(&id) {
                    tile.mode = Some(mode);
                }
            }
            GridMsg::Choice(id, label) => {
                // A model that is not ready stays unchosen, as in the panel's composer.
                let found = self.choices.iter().find(|c| super::agent::choice_label(c) == label && c.ready).map(|c| c.id.clone());
                if let (Some(choice), Some(tile)) = (found, self.ide.grid.tile_mut(&id)) {
                    tile.choice = Some(choice);
                }
            }
        }
        Task::none()
    }

    /// Read a tile's chat from the core.
    fn grid_open(&mut self, id: String) -> Task<Msg> {
        let Some((services, agent)) = self.agent() else { return Task::none() };
        let asked = id.clone();
        Task::perform(on(&services, async move { agent.open(&asked).await.map_err(|r| r.message) }), move |r| {
            go(GridMsg::Opened(id.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    /// Read what a tile's chat changed, when it works in a folder.
    pub(in crate::lattice) fn grid_changes(&mut self, id: String) -> Task<Msg> {
        let Some((services, agent)) = self.agent() else { return Task::none() };
        if self.ide.grid.tile(&id).is_some_and(|t| t.recording) {
            return Task::none();
        }
        let in_folder =
            self.ide.grid.tile(&id).and_then(|t| t.conversation.as_ref()).is_some_and(|c| c.summary.workspace.is_some());
        if !in_folder {
            return Task::none();
        }
        let asked = id.clone();
        Task::perform(on(&services, async move { agent.changes(&asked).await.map_err(|r| r.message) }), move |r| {
            go(GridMsg::Changes(id.clone(), r.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    /// What a tile's send asks the core: that chat, in its own mode, folder and model.
    fn grid_request(&self, id: &str) -> Option<SendRequest> {
        let tile = self.ide.grid.tile(id).filter(|t| !t.recording)?;
        let c = tile.conversation.as_ref()?;
        let text = tile.draft.trim().to_string();
        if text.is_empty() {
            return None;
        }
        let choice = tile.choice.clone().unwrap_or_else(|| {
            if c.summary.pinned_provider.is_empty() { self.choice.clone() } else { c.summary.pinned_provider.clone() }
        });
        Some(SendRequest {
            conversation: Some(id.to_string()),
            text,
            shown: self.shown_for(&choice),
            choice,
            mode: tile.mode.unwrap_or(c.summary.mode),
            workspace: c.summary.workspace.as_ref().map(|w| w.id.clone()),
            edit_of: None,
            project: None,
            images: Vec::new(),
        })
    }
}

impl State {
    /// A debug build only: the recordings `CENTCOM_LATTICE_GRID` names (`;`-separated files of `{"snapshot": Snapshot,
    /// "changes": ChangeSet}`, the protocol's own shapes) put in the grid as tiles, so the screenshot mode can photograph
    /// it without a model; with any other value, the newest chats are put in it once listed. A release build never
    /// reads it.
    #[cfg(debug_assertions)]
    pub(in crate::lattice) fn photograph_grid(&mut self, paths: &str) {
        for (n, path) in paths.split(';').filter(|p| p.ends_with(".json")).enumerate() {
            let read = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()));
            let mut tile = Tile::new(format!("recording-{n}"));
            tile.recording = true;
            match read.and_then(|v| Ok((serde_json::from_value::<Snapshot>(v["snapshot"].clone()).map_err(|e| e.to_string())?, v))) {
                Ok((snapshot, v)) => {
                    tile.conversation = Some(chat::Conversation::from_snapshot(snapshot));
                    tile.changes = serde_json::from_value::<ChangeSet>(v["changes"].clone()).ok();
                }
                Err(why) => tile.problem = Some(format!("{path}: {why}")),
            }
            self.ide.grid.tiles.push(tile);
        }
        self.ide.grid.shown = true;
        // The Chats list beside it, as an agent manager is laid out.
        self.ide.side = super::Side::Chats;
        self.ide.side_open = true;
    }
}

type El<'a> = Element<'a, Msg>;

/// The grid, drawn in the editor's place: the chats as panes that fill it, each drawn as the chat panel on the right
/// is, joined edge to edge with a thin gold line between them.
pub fn view<'a>(state: &'a State, phase: f32) -> El<'a> {
    let grid = &state.ide.grid;
    if grid.tiles.is_empty() {
        return container(note("Put a chat here with \u{25A6} in the Chats list.")).padding(16).into();
    }
    let columns = columns_for(grid.tiles.len());
    let mut rows = Column::new().width(Length::Fill).height(Length::Fill);
    for (r, chunk) in grid.tiles.chunks(columns).enumerate() {
        if r > 0 {
            rows = rows.push(divider_h());
        }
        let mut line = Row::new().width(Length::Fill).height(Length::FillPortion(1));
        for (c, tile) in chunk.iter().enumerate() {
            if c > 0 {
                line = line.push(divider_v());
            }
            line = line.push(container(pane(state, tile, phase)).width(Length::FillPortion(1)).height(Length::Fill));
        }
        // A short last row keeps the columns' widths: the rest of it is empty canvas.
        for _ in chunk.len()..columns {
            line = line.push(divider_v());
            line = line.push(container(space()).width(Length::FillPortion(1)).height(Length::Fill).style(|_| solid(theme::CANVAS)));
        }
        rows = rows.push(line);
    }
    rows.into()
}

/// The columns for `n` chats: one alone, two up to four, three up to six, four up to eight, five up to ten.
pub fn columns_for(n: usize) -> usize {
    match n {
        0 | 1 => 1,
        2..=4 => 2,
        5 | 6 => 3,
        7 | 8 => 4,
        _ => 5,
    }
}

fn solid(color: iced::Color) -> container::Style {
    container::Style { background: Some(color.into()), ..container::Style::default() }
}

/// The thin gold line between two panes side by side.
fn divider_v<'a>() -> El<'a> {
    container(space().width(1)).height(Length::Fill).style(|_| solid(theme::GOLD_DIM)).into()
}

/// The thin gold line between two rows of panes.
fn divider_h<'a>() -> El<'a> {
    container(space().height(1)).width(Length::Fill).style(|_| solid(theme::GOLD_DIM)).into()
}

/// One chat's pane, drawn as the chat panel on the right: its header, its transcript, and its composer.
fn pane<'a>(state: &'a State, tile: &'a Tile, phase: f32) -> El<'a> {
    let id = tile.id.clone();
    let mut col = Column::new().width(Length::Fill).height(Length::Fill);
    col = col.push(pane_header(tile, phase));
    col = col.push(container(space().height(1)).width(Length::Fill).style(theme::line));
    let body: El<'a> = match &tile.conversation {
        None => match &tile.problem {
            Some(why) => container(label(why.as_str(), 12.0, theme::CAUTION)).padding(16).into(),
            None => container(ui::working(phase, "Opening the chat…")).padding(16).into(),
        },
        Some(c) => {
            let act = id.clone();
            scrollable(container(super::agent::transcript(c, state, phase)).padding(Padding { top: 14.0, bottom: 14.0, left: 16.0, right: 18.0 }))
                .anchor_bottom()
                .height(Length::Fill)
                .style(theme::scrollbars)
                .into_element_mapped(move |m| go(GridMsg::Act(act.clone(), Box::new(m))))
        }
    };
    col = col.push(container(body).height(Length::Fill));
    col = col.push(pane_composer(state, tile, phase));
    col.into()
}

trait MapToGrid<'a> {
    fn into_element_mapped(self, f: impl Fn(Msg) -> Msg + 'a) -> El<'a>;
}

impl<'a> MapToGrid<'a> for iced::widget::Scrollable<'a, Msg> {
    fn into_element_mapped(self, f: impl Fn(Msg) -> Msg + 'a) -> El<'a> {
        Element::from(self).map(f)
    }
}

/// The pane's header, as the panel's: the title, what it is doing, and its buttons.
fn pane_header<'a>(tile: &'a Tile, phase: f32) -> El<'a> {
    let id = tile.id.clone();
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    match &tile.conversation {
        Some(c) => {
            // Working is the spinner, as the Chats list shows it, so a narrow pane keeps its title.
            if c.running {
                line = line.push(crate::spinner::spinner(phase, 10.0));
            }
            let title = if c.summary.title.is_empty() { "Untitled chat" } else { c.summary.title.as_str() };
            line = line.push(container(strong(ui::cut(title, 40), 13.5, theme::TEXT).wrapping(Wrapping::None)).width(Length::Fill).clip(true));
            match c.waiting() {
                0 if c.summary.needs_you => line = line.push(chip("needs you", theme::GOLD)),
                0 => {}
                n => line = line.push(chip(format!("{n} waiting"), theme::GOLD)),
            }
        }
        None => line = line.push(container(strong("A chat", 13.5, theme::TEXT)).width(Length::Fill)),
    }
    line = line.push(super::view::icon("\u{2197}", "Open it on the right", Some(go(GridMsg::Focus(id.clone())))));
    line = line.push(super::view::icon("\u{00D7}", "Take it out of the grid", Some(go(GridMsg::Remove(id)))));
    let mut col = Column::new().push(container(line).padding([8.0, 12.0]));
    if let Some(stages) = tile.conversation.as_ref().and_then(|c| super::agent::stage_line(c, tile.changes.as_ref())) {
        col = col.push(container(stages).padding(Padding { top: 0.0, right: 12.0, bottom: 7.0, left: 12.0 }));
    }
    col.into()
}

/// The pane's composer, as the panel's: the box, the mode and the model, Stop and Send.
fn pane_composer<'a>(state: &'a State, tile: &'a Tile, phase: f32) -> El<'a> {
    let id = tile.id.clone();
    let running = tile.conversation.as_ref().is_some_and(|c| c.running);
    let mode = tile.mode.or(tile.conversation.as_ref().map(|c| c.summary.mode)).unwrap_or(Mode::Agent);
    let ready = tile.conversation.is_some() && !tile.recording;
    let placeholder =
        if mode == Mode::Agent { "Tell the agent what to change, or ask about the code…" } else { "Ask about the code (Ask mode: the agent only reads)…" };
    let mut input = text_input(placeholder, &tile.draft).size(13.5).padding(4).style(ui::input_style);
    if ready && !tile.sending {
        let (draft_id, send_id) = (id.clone(), id.clone());
        input = input.on_input(move |t| go(GridMsg::Draft(draft_id.clone(), t))).on_submit(go(GridMsg::Send(send_id)));
    }
    // Stop and Send stand beside the box, as narrow as a pane may be; the mode and the model go on the line under it.
    let mut top = Row::new().spacing(6).align_y(Alignment::Center).push(container(input).width(Length::Fill));
    if tile.sending {
        top = top.push(crate::spinner::spinner(phase, 14.0));
    }
    if running {
        top = top.push(
            button(label("\u{25A0}", 12.0, theme::TEXT)).padding([5.0, 10.0]).style(theme::secondary_button).on_press(go(GridMsg::Stop(id.clone()))),
        );
    }
    let can_send = ready && !tile.sending && !tile.draft.trim().is_empty();
    top = top.push(
        // A message sent while a turn runs waits its turn, as in the panel's composer.
        button(label("\u{2191}", 14.0, theme::ON_GOLD))
            .padding([4.0, 12.0])
            .style(|t, s| {
                let mut style = theme::primary_button(t, s);
                style.border.radius = 14.0.into();
                style
            })
            .on_press_maybe(can_send.then(|| go(GridMsg::Send(id.clone())))),
    );
    let mut card = Column::new().spacing(6).push(top);
    let mut bottom = Row::new().spacing(6).align_y(Alignment::Center);
    for (m, words) in [(Mode::Agent, "Agent"), (Mode::Ask, "Ask")] {
        let on = mode == m;
        let pick = id.clone();
        bottom = bottom.push(
            button(label(words, 12.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([3.0, 9.0])
                .style(theme::segment_button(on))
                .on_press(go(GridMsg::Mode(pick, m))),
        );
    }
    let choice = tile.choice.clone().or_else(|| {
        tile.conversation.as_ref().map(|c| if c.summary.pinned_provider.is_empty() { state.choice.clone() } else { c.summary.pinned_provider.clone() })
    });
    let labels: Vec<String> = state.choices.iter().map(super::agent::choice_label).collect();
    let chosen = state.choices.iter().find(|c| Some(&c.id) == choice.as_ref()).map(super::agent::choice_label);
    let pick = id.clone();
    bottom = bottom.push(
        pick_list(labels, chosen, move |l| go(GridMsg::Choice(pick.clone(), l)))
            .placeholder("Model")
            .text_size(12.0)
            .padding([3.0, 8.0])
            .style(theme::picker)
            .menu_style(theme::picker_menu),
    );
    card = card.push(bottom);
    let mut col = Column::new().spacing(4);
    if let Some(why) = &tile.problem
        && tile.conversation.is_some()
    {
        col = col.push(container(label(why.as_str(), 11.0, theme::CAUTION).wrapping(Wrapping::WordOrGlyph)).padding(padding::left(4)));
    }
    col = col.push(container(card).padding([8.0, 10.0]).width(Length::Fill).style(super::agent::composer_card));
    container(col).padding(Padding { top: 8.0, bottom: 12.0, left: 12.0, right: 12.0 }).into()
}

#[cfg(test)]
mod tests {
    use lattice_protocol::conversation::{ConversationSummary, Origin, WorkspaceBadge};

    use super::*;

    fn snapshot(id: &str, running: bool, needs_you: bool) -> Snapshot {
        let conversation = ConversationSummary {
            id: id.into(),
            title: "t".into(),
            created: 1.0,
            updated: 1.0,
            turns: 0,
            pinned_provider: "local:llama".into(),
            workspace: None,
            mode: Mode::Ask,
            origin: Origin::Native,
            running,
            needs_you,
        };
        Snapshot { conversation, turns: vec![], events: vec![], last_seq: 0, queued: vec![], answering_elsewhere: false }
    }

    #[test]
    fn a_chat_is_put_in_the_grid_once_and_the_grid_holds_at_most_its_size() {
        let mut state = State::new();
        let _ = state.grid(GridMsg::Add("c1".into()));
        let _ = state.grid(GridMsg::Add("c1".into()));
        assert!(state.ide.grid.shown, "putting a chat in the grid shows it");
        assert_eq!(state.ide.grid.tiles.len(), 1, "the same chat once");
        for n in 2..=MAX_TILES + 1 {
            let _ = state.grid(GridMsg::Add(format!("c{n}")));
        }
        assert_eq!(state.ide.grid.tiles.len(), MAX_TILES);
        assert!(state.problem.as_deref().is_some_and(|p| p.contains("take one out")), "a full grid says so");
        let _ = state.grid(GridMsg::Remove("c1".into()));
        assert!(!state.ide.grid.holds("c1"));
        let _ = state.grid(GridMsg::Show(false));
        assert!(!state.ide.grid.shown);
    }

    #[test]
    fn a_reading_fills_its_tile_and_one_for_a_chat_not_in_the_grid_adds_nothing() {
        let mut state = State::new();
        let _ = state.grid(GridMsg::Add("c1".into()));
        assert!(state.ide.grid.tile("c1").unwrap().conversation.is_none(), "not read yet");
        let _ = state.grid(GridMsg::Opened("c1".into(), Ok(snapshot("c1", true, true))));
        let c = state.ide.grid.tile("c1").unwrap().conversation.as_ref().unwrap();
        assert!(c.running && c.summary.needs_you, "what the pane's header shows");
        let _ = state.grid(GridMsg::Opened("nobody".into(), Ok(snapshot("nobody", false, false))));
        assert!(!state.ide.grid.holds("nobody"));
        let _ = state.grid(GridMsg::Opened("c1".into(), Err("gone".into())));
        assert_eq!(state.ide.grid.tile("c1").unwrap().problem.as_deref(), Some("gone"));
    }

    #[test]
    fn beside_the_grid_is_the_editor_until_the_reader_picks_the_chat() {
        let mut state = State::new();
        assert_eq!(state.ide.grid.beside, Beside::Editor);
        let _ = state.grid(GridMsg::Beside(Beside::Chat));
        assert_eq!(state.ide.grid.beside, Beside::Chat);
        let _ = state.grid(GridMsg::Show(false));
        let _ = state.grid(GridMsg::Show(true));
        assert_eq!(state.ide.grid.beside, Beside::Chat, "kept while the grid comes and goes");
    }

    #[test]
    fn the_chat_beside_the_grid_plans_it_sends_in_ask_mode_whatever_was_picked() {
        let mut state = State::new();
        assert_eq!(state.send_request("hi".into(), Mode::Agent, None).mode, Mode::Agent, "no grid: as picked");
        let _ = state.grid(GridMsg::Show(true));
        assert_eq!(state.send_request("hi".into(), Mode::Agent, None).mode, Mode::Agent, "the editor beside the grid");
        let _ = state.grid(GridMsg::Beside(Beside::Chat));
        assert!(state.planning());
        assert_eq!(state.send_request("hi".into(), Mode::Agent, None).mode, Mode::Ask, "the planning chat writes nothing");
        let _ = state.grid(GridMsg::Show(false));
        assert_eq!(state.send_request("hi".into(), Mode::Agent, None).mode, Mode::Agent, "back to the panel: as picked");
    }

    #[test]
    fn a_panes_action_is_done_in_its_own_chat_which_opens_first_when_it_is_not_the_open_one() {
        let mut state = State::new();
        let _ = state.grid(GridMsg::Add("c1".into()));
        let _ = state.grid(GridMsg::Act("c1".into(), Box::new(Msg::Stop)));
        assert_eq!(state.ide.grid.pending.as_ref().map(|(id, _)| id.as_str()), Some("c1"), "kept until c1 has opened");
    }

    #[test]
    fn a_tiles_message_goes_to_its_own_chat_in_that_chats_mode_model_and_folder() {
        let mut state = State::new();
        state.choice = "cloud:other".into();
        let _ = state.grid(GridMsg::Add("c1".into()));
        let mut snap = snapshot("c1", false, false);
        snap.conversation.workspace =
            serde_json::from_value::<WorkspaceBadge>(serde_json::json!({"id": "w1", "name": "demo", "path": "D:/demo", "trusted": true})).ok();
        let _ = state.grid(GridMsg::Opened("c1".into(), Ok(snap)));
        assert!(state.grid_request("c1").is_none(), "nothing written, nothing sent");
        let _ = state.grid(GridMsg::Draft("c1".into(), "  carry on  ".into()));
        let request = state.grid_request("c1").unwrap();
        assert_eq!(request.conversation.as_deref(), Some("c1"));
        assert_eq!((request.text.as_str(), request.mode, request.choice.as_str()), ("carry on", Mode::Ask, "local:llama"));
        assert_eq!(request.workspace.as_deref(), state.ide.grid.tile("c1").unwrap().conversation.as_ref().unwrap().summary.workspace.as_ref().map(|w| w.id.as_str()));
        assert!(request.images.is_empty() && request.edit_of.is_none() && request.project.is_none());
        state.ide.grid.tile_mut("c1").unwrap().recording = true;
        assert!(state.grid_request("c1").is_none(), "a recording is never sent to");
    }

    #[test]
    fn a_batch_or_a_follow_is_only_a_held_and_read_chats_and_the_columns_grow_with_the_chats() {
        assert_eq!((columns_for(1), columns_for(4), columns_for(6), columns_for(8), columns_for(10)), (1, 2, 3, 4, 5));
        let mut grid = Grid::default();
        grid.tiles.push(Tile::new("c1".into()));
        assert!(grid.holds("c1") && !grid.holds("c2"));
        assert!(!grid.apply("c2", &[]), "a batch for a chat the grid does not hold is not its");
        assert!(!grid.apply("c1", &[]), "a tile not read yet takes no batch");
        grid.follow_ended("c1");
        assert_eq!(grid.tile("c1").unwrap().generation, 0, "a chat not running is not followed again");
    }
}
