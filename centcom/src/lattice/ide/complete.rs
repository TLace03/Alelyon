//! Tab completions in the editor (as Cursor's Tab and Copilot's inline suggestions do it).
//!
//! When the reader pauses typing in a file, Lattice asks the model they chose for what comes next at the cursor
//! (lattice-core's `complete`), and shows the answer in grey where it would go: its first line after the cursor, the
//! rest in a panel over the lines below. Tab takes all of it, Ctrl+Right its next word, Esc puts it away; typing on
//! asks again. A suggestion is shown only while the cursor stays where it was asked and its line is unchanged.
//!
//! It asks only at the end of a line, or before closing brackets and quotes. Each new keystroke stops the request
//! before it. Nothing is asked while completions are off (the default) or no model is chosen. The Tools page's card
//! switches them on, and lists every model that can write them, grouped by what a thousand completions cost, to filter
//! by price.

use std::collections::HashSet;
use std::time::Duration;

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Text};
use iced::widget::{Column, Row, button, column, container, space};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Task, Theme, mouse};

use crate::theme::{self, fonts};
use lattice_core::complete::Request;
use lattice_core::complete::catalog::{self, Offer, Tier};
use lattice_core::complete::settings::{Choice, Settings};

use super::{CODE_PAD, CODE_SIZE, Editing, IdeMsg, LINE_HEIGHT};
use crate::lattice::{Msg, STOPPED, State, on};
use crate::ui::{self, label, note, strong};

type El<'a> = Element<'a, Msg>;

/// How long typing pauses before a completion is asked for.
pub const DELAY: Duration = Duration::from_millis(350);

/// What may follow the cursor on its line for a completion to be asked: closing brackets, quotes and punctuation.
const CLOSERS: &[char] = &[')', ']', '}', '"', '\'', '`', ';', ',', '>', ' ', '\t'];

/// The completions' state.
#[derive(Debug, Default)]
pub struct Completion {
    /// The reader's settings, once read.
    pub settings: Option<Settings>,
    /// The models that can write completions, once listed, and why a provider's list could not be read.
    pub offers: Option<Vec<Offer>>,
    pub notes: Vec<String>,
    pub listing: bool,
    /// The models are shown to choose from.
    pub choosing: bool,
    /// The price groups shown (none: every group).
    pub shown: HashSet<Tier>,
    /// The suggestion on show.
    pub suggestion: Option<Suggestion>,
    /// The last request's number: an answer to an older one is dropped.
    pub asked: u64,
    /// The request in flight, stopped when the reader types again.
    pub waiting: Option<tokio::task::AbortHandle>,
    /// Why the last completion failed, until one succeeds.
    pub problem: Option<String>,
    /// The card's last sentence, and whether it warns.
    pub said: Option<(String, bool)>,
}

/// A completion: the tab and cursor it was asked at, the cursor's line then, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub tab: u64,
    pub at: (usize, usize),
    pub line: String,
    pub text: String,
}

/// What the completions ask.
#[derive(Debug, Clone)]
pub enum CompleteMsg {
    Read(Settings),
    /// Ask now, if nothing was typed since request `n` (tab, n).
    Ask(u64, u64),
    /// Request `n`'s answer (tab, n, the cursor, its line, the answer).
    Answered(u64, u64, (usize, usize), String, Result<Option<String>, String>),
    Accept,
    AcceptWord,
    Dismiss,
    // The Tools page's card.
    Switch(bool),
    ShowModels,
    HideModels,
    OffersRead(Vec<Offer>, Vec<String>),
    Pick(Choice),
    Filter(Tier),
    Saved(Result<Settings, String>),
}

fn go(msg: CompleteMsg) -> Msg {
    Msg::Ide(IdeMsg::Complete(msg))
}

/// The suggestion for tab `id`'s editor, if it still stands: no selection, the cursor where it was asked, its line as
/// it was.
pub fn live<'a>(c: &'a Completion, id: u64, e: &Editing) -> Option<&'a Suggestion> {
    let s = c.suggestion.as_ref().filter(|s| s.tab == id)?;
    let cursor = e.content.cursor();
    let line = e.content.line(s.at.0)?;
    (cursor.selection.is_none() && (cursor.position.line, cursor.position.column) == s.at && line.text == s.line.as_str())
        .then_some(s)
}

/// Whether a completion may be asked at byte `column` of `line`: only closers and spaces follow it.
pub fn askable(line: &str, column: usize) -> bool {
    line.get(column..).is_some_and(|rest| rest.chars().all(|c| CLOSERS.contains(&c)))
}

/// The text before and after (`line`, byte `column`) of `text`.
pub fn split_at(text: &str, line: usize, column: usize) -> Request {
    let mut offset = 0;
    for (n, l) in text.split('\n').enumerate() {
        if n == line {
            offset += column.min(l.len());
            break;
        }
        offset += l.len() + 1;
    }
    let offset = offset.min(text.len());
    let offset = (0..=offset).rev().find(|&at| text.is_char_boundary(at)).unwrap_or(0);
    Request { prefix: text[..offset].to_string(), suffix: text[offset..].to_string() }
}

/// The suggestion's next word: spaces, then a word's letters, digits and underscores, or else one other character.
pub fn next_word(text: &str) -> &str {
    let spaces = text.len() - text.trim_start_matches([' ', '\t']).len();
    let rest = &text[spaces..];
    let word = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let word = if word == 0 { rest.chars().next().map_or(0, char::len_utf8) } else { word };
    &text[..spaces + word]
}

impl State {
    /// Something was typed in tab `id`: the suggestion goes, the request in flight stops, and a new one is asked for
    /// once typing pauses.
    pub(in crate::lattice) fn typed(&mut self, id: u64) -> Task<Msg> {
        let c = &mut self.ide.completion;
        c.suggestion = None;
        if let Some(waiting) = c.waiting.take() {
            waiting.abort();
        }
        c.asked += 1;
        let on_and_chosen = c.settings.as_ref().is_some_and(|s| s.on && s.choice.is_some());
        let Some(services) = self.services.clone().filter(|_| on_and_chosen) else { return Task::none() };
        let n = c.asked;
        Task::perform(on(&services, tokio::time::sleep(DELAY)), move |_| go(CompleteMsg::Ask(id, n)))
    }

    /// The cursor moved, or the editor was clicked: the suggestion goes.
    pub(in crate::lattice) fn moved(&mut self) {
        let c = &mut self.ide.completion;
        c.suggestion = None;
        if let Some(waiting) = c.waiting.take() {
            waiting.abort();
        }
        c.asked += 1;
    }

    pub(in crate::lattice) fn complete(&mut self, msg: CompleteMsg) -> Task<Msg> {
        match msg {
            CompleteMsg::Read(settings) => self.ide.completion.settings = Some(settings),
            CompleteMsg::Ask(id, n) => return self.ask(id, n),
            CompleteMsg::Answered(id, n, at, line, answer) => {
                let c = &mut self.ide.completion;
                if n != c.asked {
                    return Task::none();
                }
                c.waiting = None;
                match answer {
                    Ok(Some(text)) => {
                        c.problem = None;
                        c.suggestion = Some(Suggestion { tab: id, at, line, text });
                    }
                    Ok(None) => c.problem = None,
                    Err(why) => c.problem = Some(why),
                }
            }
            CompleteMsg::Accept | CompleteMsg::AcceptWord => {
                let Some(id) = self.ide.active else { return Task::none() };
                let text = match self.ide.tab(id).map(|t| &t.kind) {
                    Some(super::TabKind::File { body: super::Body::Editing(e), .. }) => {
                        live(&self.ide.completion, id, e).map(|s| s.text.clone())
                    }
                    _ => None,
                };
                let Some(text) = text else { return Task::none() };
                let take = if matches!(msg, CompleteMsg::AcceptWord) { next_word(&text).to_string() } else { text.clone() };
                let rest = text[take.len()..].to_string();
                let edit = self.type_text(id, take);
                // A word taken leaves the rest on show, at the cursor's new place.
                if !rest.is_empty()
                    && let Some(e) = self.editing_mut(id)
                {
                    let p = e.content.cursor().position;
                    let line = e.content.line(p.line).map(|l| l.text.into_owned()).unwrap_or_default();
                    self.moved();
                    self.ide.completion.suggestion = Some(Suggestion { tab: id, at: (p.line, p.column), line, text: rest });
                    return edit;
                }
                return edit;
            }
            CompleteMsg::Dismiss => self.moved(),
            CompleteMsg::Switch(on) => {
                let mut settings = self.ide.completion.settings.clone().unwrap_or_default();
                settings.on = on;
                return self.save_completion(settings, if on { "On: suggestions appear as you pause typing." } else { "Off." });
            }
            CompleteMsg::ShowModels => {
                self.ide.completion.choosing = true;
                return self.list_offers();
            }
            CompleteMsg::HideModels => self.ide.completion.choosing = false,
            CompleteMsg::OffersRead(offers, notes) => {
                let c = &mut self.ide.completion;
                c.listing = false;
                c.offers = Some(offers);
                c.notes = notes;
            }
            CompleteMsg::Pick(choice) => {
                let mut settings = self.ide.completion.settings.clone().unwrap_or_default();
                settings.choice = Some(choice);
                settings.on = true;
                self.ide.completion.choosing = false;
                return self.save_completion(settings, "Chosen: completions are on with it.");
            }
            CompleteMsg::Filter(tier) => {
                let shown = &mut self.ide.completion.shown;
                if !shown.remove(&tier) {
                    shown.insert(tier);
                }
            }
            CompleteMsg::Saved(result) => match result {
                Ok(settings) => self.ide.completion.settings = Some(settings),
                Err(why) => self.ide.completion.said = Some((why, true)),
            },
        }
        Task::none()
    }

    /// Ask tab `id`'s model, if request `n` is still the latest and the cursor stands where one may be asked.
    fn ask(&mut self, id: u64, n: u64) -> Task<Msg> {
        if n != self.ide.completion.asked || self.ide.active != Some(id) {
            return Task::none();
        }
        let Some(choice) = self.ide.completion.settings.as_ref().filter(|s| s.on).and_then(|s| s.choice.clone()) else {
            return Task::none();
        };
        let Some(services) = self.services.clone() else { return Task::none() };
        let Some(e) = self.editing_mut(id) else { return Task::none() };
        let cursor = e.content.cursor();
        if cursor.selection.is_some() {
            return Task::none();
        }
        let at = (cursor.position.line, cursor.position.column);
        let line = e.content.line(at.0).map(|l| l.text.into_owned()).unwrap_or_default();
        if !askable(&line, at.1) {
            return Task::none();
        }
        let request = split_at(&e.content.text(), at.0, at.1);
        let completer = services.complete.clone();
        let task = services.handle().spawn(async move { completer.complete(&choice, &request).await });
        self.ide.completion.waiting = Some(task.abort_handle());
        Task::perform(async move { task.await.ok() }, move |answer| match answer {
            Some(answer) => go(CompleteMsg::Answered(id, n, at, line.clone(), answer)),
            None => go(CompleteMsg::Answered(id, n, at, line.clone(), Ok(None))),
        })
    }

    /// The settings read once the services run.
    pub(in crate::lattice) fn read_completion(&self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        let completer = services.complete.clone();
        Task::perform(on(&services, async move { completer.settings() }), |read| go(CompleteMsg::Read(read.unwrap_or_default())))
    }

    fn save_completion(&mut self, settings: Settings, said: &'static str) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        self.ide.completion.said = Some((said.to_string(), false));
        if !settings.on {
            self.moved();
        }
        let completer = services.complete.clone();
        Task::perform(on(&services, async move { completer.save(&settings).map(|()| settings) }), |r| {
            go(CompleteMsg::Saved(r.unwrap_or_else(|| Err(STOPPED.to_string()))))
        })
    }

    fn list_offers(&mut self) -> Task<Msg> {
        let Some(services) = self.services.clone() else { return Task::none() };
        if self.ide.completion.listing {
            return Task::none();
        }
        self.ide.completion.listing = true;
        let completer = services.complete.clone();
        Task::perform(on(&services, async move { completer.offers().await }), |read| {
            let (offers, notes) = read.unwrap_or_else(|| (Vec::new(), vec![STOPPED.to_string()]));
            go(CompleteMsg::OffersRead(offers, notes))
        })
    }
}

// ------------------------------------------------------------------- the editor

/// The suggestion drawn over the editor: its first line in grey after the cursor (over what follows it on the line,
/// which is drawn again after it), the rest in a panel over the lines below.
pub struct Ghost<'a> {
    pub suggestion: &'a Suggestion,
}

impl<'a> canvas::Program<Msg> for Ghost<'a> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = super::term::cell().0;
        let s = self.suggestion;
        let (line, column) = s.at;
        let x = CODE_PAD + super::find::cells(&s.line, column) as f32 * w;
        let y = CODE_PAD + line as f32 * LINE_HEIGHT;
        let mut lines = s.text.split('\n');
        let first = lines.next().unwrap_or_default();
        let rest_of_line = s.line.get(column..).unwrap_or_default();
        let text = |content: String, at: Point, color: Color| Text {
            content,
            position: at,
            color,
            size: CODE_SIZE.into(),
            line_height: iced::widget::text::LineHeight::Absolute(LINE_HEIGHT.into()),
            font: fonts().mono,
            ..Text::default()
        };
        let first_cells = super::find::cells(first, first.len()) as f32 * w;
        if !rest_of_line.is_empty() {
            // What follows the cursor moves along after the suggestion, as it will once taken.
            let old = super::find::cells(rest_of_line, rest_of_line.len()) as f32 * w;
            frame.fill_rectangle(Point::new(x, y), Size::new(old + 1.0, LINE_HEIGHT), theme::CANVAS);
            frame.fill_text(text(rest_of_line.to_string(), Point::new(x + first_cells, y), theme::TEXT));
        }
        frame.fill_text(text(first.to_string(), Point::new(x, y), theme::TEXT_FAINT));
        let more: Vec<&str> = lines.collect();
        if !more.is_empty() {
            let widest = more.iter().map(|l| super::find::cells(l, l.len())).max().unwrap_or(0) as f32 * w;
            let top = y + LINE_HEIGHT;
            let height = more.len() as f32 * LINE_HEIGHT;
            frame.fill_rectangle(Point::new(CODE_PAD - 4.0, top), Size::new(widest + 12.0, height), theme::CANVAS);
            frame.fill_rectangle(Point::new(CODE_PAD - 4.0, top), Size::new(2.0, height), theme::with_alpha(theme::GOLD, 0.5));
            for (k, l) in more.iter().enumerate() {
                frame.fill_text(text(l.to_string(), Point::new(CODE_PAD, top + k as f32 * LINE_HEIGHT), theme::TEXT_FAINT));
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The suggestion's layer, over the editor.
pub fn ghost(suggestion: &Suggestion) -> El<'_> {
    Canvas::new(Ghost { suggestion }).width(Length::Fill).height(Length::Fill).into()
}

// ------------------------------------------------------------------- the Tools page

/// What the card says under its title.
const ABOUT: &str = "As you pause typing in a file, a code model suggests what comes next, in grey: Tab takes it, Ctrl+Right its next word, Esc puts it away. Choose a model on this PC (free, nothing leaves it) or one of a connected provider (the code around the cursor is sent to it; text that looks like a key never is).";

/// The Tools page's card: on or off, the model, and when choosing, every model by price.
pub fn card(state: &State) -> El<'_> {
    let c = &state.ide.completion;
    let settings = c.settings.clone().unwrap_or_default();
    let mut col = Column::new().spacing(8);
    let mut head = Row::new().spacing(10).align_y(Alignment::Center);
    head = head.push(ui::dot(if settings.on && settings.choice.is_some() { theme::POSITIVE } else { theme::TEXT_FAINT }));
    let now = match &settings.choice {
        Some(choice) => {
            let offer = c.offers.as_ref().and_then(|o| o.iter().find(|o| &o.choice == choice));
            match (choice, offer) {
                (_, Some(o)) => format!("Model: {} ({}, {})", o.name, o.host, catalog::cost_text(o.per_thousand)),
                (Choice::Local { model }, None) => format!("Model: {model} (This PC, free)"),
                (Choice::Hosted { provider, model }, None) => format!("Model: {model} ({provider})"),
            }
        }
        None => "No model chosen yet.".to_string(),
    };
    head = head.push(column![strong("Tab completions", 14.0, theme::TEXT), label(now, 12.0, theme::TEXT_DIM)].spacing(2));
    head = head.push(space().width(Length::Fill));
    if settings.choice.is_some() {
        head = head.push(small(if settings.on { "Turn off" } else { "Turn on" }, Some(go(CompleteMsg::Switch(!settings.on)))));
    }
    head = head.push(if c.choosing {
        small("Done", Some(go(CompleteMsg::HideModels)))
    } else {
        small("Choose model\u{2026}", Some(go(CompleteMsg::ShowModels)))
    });
    col = col.push(head).push(label(ABOUT, 12.0, theme::TEXT_DIM));
    if let Some((words, warn)) = &c.said {
        col = col.push(label(words.as_str(), 12.0, if *warn { theme::CAUTION } else { theme::POSITIVE }));
    }
    if let Some(problem) = &c.problem {
        col = col.push(label(format!("Last completion: {problem}"), 12.0, theme::CAUTION));
    }
    if c.choosing {
        col = col.push(models(c, c.settings.as_ref().and_then(|s| s.choice.as_ref())));
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

/// Every model, filtered by price group and grouped by it.
fn models<'a>(c: &'a Completion, chosen: Option<&'a Choice>) -> El<'a> {
    let mut col = Column::new().spacing(8);
    let mut chips =
        Row::new().spacing(6).align_y(Alignment::Center).push(label("Price per 1,000:", 12.0, theme::TEXT_DIM));
    for tier in Tier::ALL {
        let on = c.shown.contains(&tier);
        chips = chips.push(
            button(iced::widget::text(chip(tier)).size(11.5).font(fonts().ui))
                .padding([3.0, 9.0])
                .style(if on { theme::primary_button } else { theme::secondary_button })
                .on_press(go(CompleteMsg::Filter(tier))),
        );
    }
    col = col.push(chips);
    col = col.push(note(
        "A cost is for 1,000 completions at the most a request sends (about 2,000 tokens in, 64 out); most cost less. Prices are the providers' own.",
    ));
    let Some(offers) = &c.offers else {
        return col.push(note(if c.listing { "Listing the models\u{2026}" } else { "Not listed yet." })).into();
    };
    let shown: Vec<Offer> = offers.iter().filter(|o| c.shown.is_empty() || c.shown.contains(&o.tier)).cloned().collect();
    if shown.is_empty() {
        col = col.push(note(if offers.is_empty() {
            "No model yet: put a GGUF code model in the models folder, or connect a provider on the Models page."
        } else {
            "None at these prices."
        }));
    }
    for (tier, group) in catalog::grouped(shown) {
        let mut list = Column::new().spacing(4).push(strong(tier.label(), 12.5, theme::GOLD));
        for offer in group {
            let mut words = Column::new().spacing(1).push(label(offer.name.clone(), 12.0, theme::TEXT));
            let mut about = format!("{} \u{00b7} {}", offer.host, catalog::cost_text(offer.per_thousand));
            if let Some((input, output)) = offer.price.filter(|p| p.0 > 0.0 || p.1 > 0.0) {
                about.push_str(&format!(" \u{00b7} ${input:.2} in, ${output:.2} out per million tokens"));
            }
            if !offer.fim {
                about.push_str(" \u{00b7} its name does not say it fills in the middle");
            }
            words = words.push(label(about, 11.0, theme::TEXT_FAINT));
            let action: El<'a> = if chosen == Some(&offer.choice) {
                label("in use", 11.5, theme::POSITIVE).into()
            } else if !offer.ready {
                label(format!("connect {} first", offer.host), 11.5, theme::TEXT_FAINT).into()
            } else {
                small("Use", Some(go(CompleteMsg::Pick(offer.choice.clone()))))
            };
            list = list.push(Row::new().spacing(10).align_y(Alignment::Center).push(container(words).width(Length::Fill)).push(action));
        }
        col = col.push(container(list).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
    }
    for why in &c.notes {
        col = col.push(label(why.as_str(), 11.5, theme::CAUTION));
    }
    col.into()
}

/// A price group's filter, in short.
fn chip(tier: Tier) -> &'static str {
    match tier {
        Tier::Free => "Free",
        Tier::UnderQuarter => "Under $0.25",
        Tier::UnderDollar => "$0.25 to $1",
        Tier::OverDollar => "Over $1",
        Tier::Unlisted => "Not listed",
    }
}

fn small<'a>(words: &'a str, msg: Option<Msg>) -> El<'a> {
    button(iced::widget::text(words).size(12.0).font(fonts().ui)).padding([3.0, 9.0]).style(theme::secondary_button).on_press_maybe(msg).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lattice::ide::highlight::Lang;
    use crate::lattice::ide::{Body, TabKind, buffer};
    use iced::widget::text_editor;
    use lattice_core::complete::settings::{Choice, Settings};

    /// A state with one file open, completions on, and the cursor at the end of `let x = `.
    fn typing() -> (State, u64) {
        let mut state = State::new();
        let text = "fn f() {\n    let x = \n}\n";
        let e = Editing::new("sha".into(), false, buffer::decode(text.as_bytes()).unwrap(), Lang::Rust);
        let id = state.ide.push(TabKind::File { path: "a.rs".into(), lang: Lang::Rust, body: Body::Editing(Box::new(e)), note: None });
        state.ide.completion.settings =
            Some(Settings { on: true, choice: Some(Choice::Local { model: "qwen2.5-coder-1.5b".into() }) });
        let e = state.editing_mut(id).unwrap();
        e.content.move_to(text_editor::Cursor { position: text_editor::Position { line: 1, column: 12 }, selection: None });
        (state, id)
    }

    fn answer(state: &mut State, id: u64, text: &str) {
        let n = state.ide.completion.asked;
        let msg = CompleteMsg::Answered(id, n, (1, 12), "    let x = ".into(), Ok(Some(text.into())));
        let _ = state.ide_update(IdeMsg::Complete(msg));
    }

    fn shown(state: &mut State, id: u64) -> Option<Suggestion> {
        match &state.ide.tab(id)?.kind {
            TabKind::File { body: Body::Editing(e), .. } => live(&state.ide.completion, id, e).cloned(),
            _ => None,
        }
    }

    fn line(state: &mut State, id: u64, n: usize) -> String {
        state.editing_mut(id).unwrap().content.line(n).unwrap().text.into_owned()
    }

    #[test]
    fn tab_takes_the_suggestion_and_ctrl_right_its_next_word() {
        let (mut state, id) = typing();
        answer(&mut state, id, "1 + 2;");
        assert_eq!(shown(&mut state, id).map(|s| s.text), Some("1 + 2;".to_string()));

        let _ = state.ide_update(IdeMsg::Complete(CompleteMsg::AcceptWord));
        assert_eq!(line(&mut state, id, 1), "    let x = 1");
        let rest = shown(&mut state, id).expect("the rest stays on show");
        assert_eq!((rest.text.as_str(), rest.at), (" + 2;", (1, 13)));

        let _ = state.ide_update(IdeMsg::Complete(CompleteMsg::Accept));
        assert_eq!(line(&mut state, id, 1), "    let x = 1 + 2;");
        assert_eq!(shown(&mut state, id), None, "taken whole, nothing is left");
        let _ = state.ide_update(IdeMsg::Undo);
        assert_eq!(line(&mut state, id, 1), "    let x = 1", "taking it is one edit, undone at once");
    }

    #[test]
    fn a_suggestion_goes_when_the_cursor_moves_or_typing_goes_on_and_a_late_answer_is_dropped() {
        let (mut state, id) = typing();
        answer(&mut state, id, "1;");
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Move(text_editor::Motion::Left)));
        assert_eq!(shown(&mut state, id), None, "moved away");
        assert_eq!(state.ide.completion.suggestion, None);

        let (mut state, id) = typing();
        answer(&mut state, id, "1;");
        let _ = state.ide_update(IdeMsg::Complete(CompleteMsg::Dismiss));
        assert_eq!(shown(&mut state, id), None, "Esc puts it away");

        let (mut state, id) = typing();
        let n = state.ide.completion.asked;
        let _ = state.ide_update(IdeMsg::Edit(id, text_editor::Action::Edit(text_editor::Edit::Insert('y'))));
        let late = CompleteMsg::Answered(id, n, (1, 12), "    let x = ".into(), Ok(Some("1;".into())));
        let _ = state.ide_update(IdeMsg::Complete(late));
        assert_eq!(state.ide.completion.suggestion, None, "the answer to a request typed past is dropped");

        // A suggestion asked at a line that has changed since is not shown.
        let (mut state, id) = typing();
        answer(&mut state, id, "1;");
        state.editing_mut(id).unwrap().content.perform(text_editor::Action::Edit(text_editor::Edit::Insert('z')));
        state.editing_mut(id).unwrap().content.move_to(text_editor::Cursor {
            position: text_editor::Position { line: 1, column: 12 },
            selection: None,
        });
        assert_eq!(shown(&mut state, id), None);
        let _ = state.ide_update(IdeMsg::Complete(CompleteMsg::Accept));
        assert_eq!(line(&mut state, id, 1), "    let x = z", "Tab with no suggestion on show types nothing of it");
    }

    #[test]
    fn a_failed_completion_is_said_until_one_succeeds() {
        let (mut state, id) = typing();
        let n = state.ide.completion.asked;
        let failed = CompleteMsg::Answered(id, n, (1, 12), "    let x = ".into(), Err("Connect OpenRouter first.".into()));
        let _ = state.ide_update(IdeMsg::Complete(failed));
        assert_eq!(state.ide.completion.problem.as_deref(), Some("Connect OpenRouter first."));
        answer(&mut state, id, "1;");
        assert_eq!(state.ide.completion.problem, None);
    }

    #[test]
    fn a_completion_is_asked_only_before_closers_and_the_text_is_split_at_the_cursor() {
        assert!(askable("let x = ", 8));
        assert!(askable("f(a, )", 5));
        assert!(askable("say(\"hi\");", 8));
        assert!(!askable("let x = y;", 6), "a word follows");
        let r = split_at("ab\ncdé\nf", 1, 2);
        assert_eq!((r.prefix.as_str(), r.suffix.as_str()), ("ab\ncd", "é\nf"));
        let end = split_at("ab", 0, 9);
        assert_eq!((end.prefix.as_str(), end.suffix.as_str()), ("ab", ""));
    }

    #[test]
    fn the_next_word_is_spaces_then_a_word_or_one_mark() {
        assert_eq!(next_word("foo(bar)"), "foo");
        assert_eq!(next_word("  bar)"), "  bar");
        assert_eq!(next_word("(bar)"), "(");
        assert_eq!(next_word(""), "");
    }
}
