//! A part's picker, drawn over the window as the Models panel is: the part's model and state at the top, then the
//! models it can use, a row each with one button.

use std::collections::HashMap;

use iced::widget::{Column, button, column, container, row, scrollable, space};
use iced::{Alignment, Color, Element, Length};

use crate::models::{Catalog, Pick};
use crate::theme;
use crate::ui::{chip, dot, label, mono, note, primary, secondary, strong, working};

use super::{Card, Choice, Kind, Level, Msg, Part, State, offered, runs_at};

type El<'a> = Element<'a, Msg>;

/// A state's colour.
pub fn color(level: Level) -> Color {
    match level {
        Level::Up => theme::POSITIVE,
        Level::Waiting => theme::TEXT_FAINT,
        Level::Off => theme::TEXT_FAINT,
        Level::Problem => theme::CAUTION,
    }
}

fn group<'a>(title: &'a str, body: El<'a>) -> El<'a> {
    container(column![crate::ui::subheading(title), body].spacing(10)).padding([14.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

/// The head: the part's model, where it runs, its state, and what uses the choice.
fn head<'a>(card: &Card) -> El<'a> {
    let mut model = row![strong(card.model.clone(), 15.0, theme::TEXT)].spacing(10).align_y(Alignment::Center);
    if !card.runs.is_empty() {
        model = model.push(label(card.runs.clone(), 12.5, theme::TEXT_DIM));
    }
    column![
        model,
        row![dot(color(card.level)), label(card.state.clone(), 12.5, theme::TEXT_DIM).width(Length::Fill)].spacing(8).align_y(Alignment::Center),
        note(format!("Used by: {}", card.used_by)),
    ]
    .spacing(6)
    .into()
}

/// One model on offer: its name and facts, and "Use" or a chip where it is the one in use.
fn offer<'a>(name: String, facts: Vec<El<'a>>, ready: Option<(Level, String)>, choice: Option<Choice>, in_use: bool, part: Part, busy: bool) -> El<'a> {
    let action: El<'a> = if in_use { chip("In use", theme::GOLD) } else { secondary("Use", (!busy).then(|| Msg::Choose(part, choice))) };
    let mut top = row![strong(name, 14.0, theme::TEXT)].spacing(10).align_y(Alignment::Center);
    for f in facts {
        top = top.push(f);
    }
    top = top.push(space().width(Length::Fill)).push(action);
    let mut col = column![top].spacing(6);
    if let Some((level, words)) = ready {
        col = col.push(row![dot(color(level)), label(words, 12.5, theme::TEXT_DIM).width(Length::Fill)].spacing(8).align_y(Alignment::Center));
    }
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(theme::well).into()
}

fn ready_of(c: &Catalog, pick: &Pick, knocked: &HashMap<String, bool>) -> (Level, String) {
    let r = c.ready(pick, knocked);
    (super::ready_level(&r), r.words().to_string())
}

/// What `part`'s default is called on its row.
fn default_words(part: Part) -> Option<(&'static str, &'static str)> {
    match part {
        Part::Mind => None,
        Part::Eyes => Some(("Off", "Sinai's eyes stay off: nothing looks at your screen for it.")),
        Part::Ears => Some(("The speech engine's own choice", "Its approved model, or the file ANGEL_EARS_MODEL names.")),
        Part::Embeddings => Some(("Sinai's loop's own", "The embedding model Sinai's own loop runs.")),
        Part::Voice => Some(("Sinai's loop's own", "The voice Sinai's own loop speaks with.")),
    }
}

/// The picker for the part open in `s`: `card` is its head and `current` the model in use (for the mind, the one that
/// answers); `catalog` and `knocked` the Models panel's; `mind` whether the build carries Sinai's mind and `mind_up`
/// whether its service answers.
#[allow(clippy::too_many_arguments)]
pub fn panel<'a>(
    s: &'a State,
    card: Card,
    current: Option<Choice>,
    catalog: Option<&'a Catalog>,
    knocked: &'a HashMap<String, bool>,
    mind: bool,
    mind_up: Option<bool>,
    phase: f32,
) -> El<'a> {
    let part = card.part;
    let header = row![
        column![strong(format!("Sinai's {}", part.title().to_lowercase()), 22.0, theme::TEXT), label(part.does(), 13.0, theme::TEXT_DIM)]
            .spacing(4)
            .width(Length::Fill),
        primary("Done", Some(Msg::Close)),
    ]
    .spacing(12)
    .align_y(Alignment::Start);
    let mut col = column![header].spacing(12);
    if let Some(said) = &s.said {
        col = col.push(match said {
            Ok(words) => crate::ui::notice(words.as_str(), theme::POSITIVE),
            Err(words) => crate::ui::notice(words.as_str(), theme::CAUTION),
        });
    }
    if s.working {
        col = col.push(working(phase, "Keeping the choice"));
    }
    col = col.push(group("Now", head(&card)));
    let choices = s.choices();
    let kept = choices.get(part);
    if let Some(c) = kept.filter(|c| choices.used(part) != Some(*c)) {
        col = col.push(note(match c {
            Choice::Cloud { .. } => {
                "A cloud model is kept for this part without your agreement, so nothing is sent and the part uses its default. \
                 Use a free cloud model below to agree to it."
                    .to_string()
            }
            _ => "A choice of a kind this version does not know is kept for this part; it uses its default.".to_string(),
        }));
    }
    // the steps of using a free cloud model take the place of the list while they are open
    if let Some(flow) = &s.flow {
        col = col.push(super::cloud::view(flow, phase).map(Msg::Cloud));
        col = col.push(crate::live::badge(&s.fresh));
        let body = scrollable(container(col).padding(20)).style(theme::scrollbars);
        return container(container(body).max_width(820).max_height(780.0).style(theme::panel)).center(Length::Fill).padding(24).into();
    }
    let busy = s.working;
    let mut offers = Column::new().spacing(10);
    if let Some((name, words)) = default_words(part) {
        offers = offers.push(offer(name.to_string(), Vec::new(), Some((Level::Off, words.to_string())), None, current.is_none(), part, busy));
    }
    let kinds = offered(part, mind);
    for kind in &kinds {
        match kind {
            Kind::SinaiMind => {
                let up = super::port_state(mind_up, "Running on this PC.", "Not running: it starts with Sinai's loop.");
                offers = offers.push(offer(
                    "Sinai's mind".into(),
                    vec![chip("Sinai's own", theme::GOLD_DIM)],
                    Some(up),
                    Some(Choice::SinaiMind),
                    current == Some(Choice::SinaiMind),
                    part,
                    busy,
                ));
            }
            Kind::Gguf => match catalog {
                None => offers = offers.push(working(phase, "Reading the models folder")),
                Some(c) if c.ggufs.is_empty() => {
                    offers = offers.push(note(format!("No GGUF models are in the models folder ({}).", c.models_dir.display())))
                }
                Some(c) => {
                    for g in &c.ggufs {
                        let pick = Pick::Gguf(g.name.clone());
                        let facts = vec![label(crate::ui::bytes(g.size), 12.0, theme::TEXT_FAINT).into(), label(runs_at(&pick, catalog), 12.0, theme::TEXT_FAINT).into()];
                        let choice = Choice::Gguf(g.name.clone());
                        let in_use = current.as_ref() == Some(&choice);
                        offers = offers.push(offer(g.name.clone(), facts, Some(ready_of(c, &pick, knocked)), Some(choice), in_use, part, busy));
                    }
                }
            },
            Kind::Endpoint => match catalog {
                None => {}
                Some(c) if c.endpoints.is_empty() => offers = offers.push(note("No OpenAI-compatible endpoint is set up.")),
                Some(c) => {
                    for e in &c.endpoints {
                        let pick = Pick::Endpoint(e.id.clone());
                        let facts = vec![
                            chip(if e.local { "on this PC" } else { "off this PC" }, if e.local { theme::POSITIVE } else { theme::GOLD_DIM }),
                            mono(format!("{} \u{b7} {}", e.address, if e.model.is_empty() { "no model set" } else { e.model.as_str() }), 11.5, theme::TEXT_FAINT).into(),
                        ];
                        let choice = Choice::Endpoint(e.id.clone());
                        let in_use = current.as_ref() == Some(&choice);
                        offers = offers.push(offer(e.label.clone(), facts, Some(ready_of(c, &pick, knocked)), Some(choice), in_use, part, busy));
                    }
                }
            },
            Kind::SpeechFile => match &s.snapshot {
                None => offers = offers.push(working(phase, "Reading the speech models folder")),
                Some(snap) => {
                    let folder = snap.speech_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "the speech models folder".into());
                    if snap.speech.is_empty() {
                        offers = offers.push(note(format!("No speech model files are in {folder}. Put a whisper.cpp model file (.bin) there, or let Auto configure Sinai download one after you agree.")));
                    } else {
                        offers = offers.push(note(format!("Speech model files in {folder}:")));
                    }
                    for f in &snap.speech {
                        let choice = Choice::SpeechFile(f.name.clone());
                        let in_use = current.as_ref() == Some(&choice);
                        let facts = vec![label(crate::ui::bytes(f.size), 12.0, theme::TEXT_FAINT).into(), label("on this PC (the speech engine)", 12.0, theme::TEXT_FAINT).into()];
                        offers = offers.push(offer(f.name.clone(), facts, Some((Level::Up, "Here, ready for the speech engine's next start.".to_string())), Some(choice), in_use, part, busy));
                    }
                }
            },
            Kind::VoiceFile => match &s.snapshot {
                None => offers = offers.push(working(phase, "Reading the voice folder")),
                Some(snap) => {
                    let folder = snap.voice_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "the voice folder".into());
                    if snap.voices.is_empty() {
                        offers = offers.push(note(format!(
                            "No voice model files are in {folder}. Put a Kokoro voice model (.onnx) there; Alelyon does not download voices."
                        )));
                    } else {
                        offers = offers.push(note(format!("Voice model files in {folder}:")));
                    }
                    for f in &snap.voices {
                        let choice = Choice::VoiceFile(f.name.clone());
                        let in_use = current.as_ref() == Some(&choice);
                        let facts = vec![label(crate::ui::bytes(f.size), 12.0, theme::TEXT_FAINT).into(), label("on this PC (Sinai's loop)", 12.0, theme::TEXT_FAINT).into()];
                        let ready = if mind { "Here, ready for Sinai's own loop's next start." } else { "Here. Sinai's own loop, which comes with the official app, speaks with it." };
                        offers = offers.push(offer(f.name.clone(), facts, Some((Level::Up, ready.to_string())), Some(choice), in_use, part, busy));
                    }
                }
            },
            // set only by the one-model switch on Sinai's page, never from one part's picker
            Kind::OneModel => {}
            Kind::Cloud => {
                offers = offers.push(label("Free in the cloud, on your own account", 12.5, theme::TEXT_DIM));
                if let Some(Choice::Cloud { provider, model, .. }) = current.as_ref() {
                    let facts = vec![chip("in the cloud", theme::GOLD_DIM)];
                    let words = super::cloud::card_lines(part, provider, model, super::cloud::now());
                    offers = offers.push(offer(format!("{} \u{b7} {model}", super::cloud::provider_label(provider)), facts, Some((words.2, words.3)), current.clone(), true, part, busy));
                }
                offers = offers.push(super::cloud::offer_rows(part, busy).map(Msg::Cloud));
            }
        }
    }
    col = col.push(group("Choose a model", offers.into()));
    if kinds.iter().any(|k| matches!(k, Kind::Gguf | Kind::Endpoint)) {
        col = col.push(row![
            label("A GGUF file or an endpoint not listed here is added in the Models panel.", 12.5, theme::TEXT_DIM).width(Length::Fill),
            secondary("Open the Models panel", Some(Msg::AddModels)),
        ]
        .spacing(10)
        .align_y(Alignment::Center));
    }
    if part == Part::Eyes {
        col = col.push(note("Eyes are off unless you choose a model: pictures of your screen are the most sensitive thing Sinai can be given."));
    }
    col = col.push(crate::live::badge(&s.fresh));
    let body = scrollable(container(col).padding(20)).style(theme::scrollbars);
    container(container(body).max_width(820).max_height(780.0).style(theme::panel)).center(Length::Fill).padding(24).into()
}

/// On Sinai's page: the one-model switch, what it needs, and what is missing when it cannot be turned on.
pub fn one_model_strip<'a>(s: &'a State, phase: f32) -> El<'a> {
    use super::one_model;
    let on = one_model::is_on(&s.choices());
    let found = s.snapshot.as_ref().and_then(|snap| snap.one_model.as_ref());
    let can_turn_on = found.is_some_and(one_model::Found::ready) && !s.working;
    let action: El<'a> = if on {
        secondary("Turn off", (!s.working).then_some(Msg::OneModel(false)))
    } else {
        primary("Turn on", can_turn_on.then_some(Msg::OneModel(true)))
    };
    let mut col = Column::new().spacing(8);
    col = col.push(
        row![
            column![
                row![strong("One model", 15.0, theme::TEXT), chip(if on { "On" } else { "Off" }, if on { theme::GOLD } else { theme::TEXT_FAINT })]
                    .spacing(10)
                    .align_y(Alignment::Center),
                note(format!(
                    "{} on this PC as the mind, eyes and voice of Sinai: one program instead of three. Hearing stays on the \
                     speech engine and the embeddings on their own model. Off puts your previous choices back.",
                    found.map(|f| f.title.as_str()).filter(|t| !t.is_empty()).unwrap_or("One local model")
                )),
            ]
            .spacing(2)
            .width(Length::Fill),
            action,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    if let Some(needs) = found.map(|f| f.needs.as_str()).filter(|n| !n.is_empty()) {
        col = col.push(note(needs));
    }
    match found {
        None => col = col.push(working(phase, "Looking for the one model's program and files")),
        Some(f) if !on => {
            if let Some(why) = f.refusal() {
                col = col.push(crate::ui::notice(why, theme::CAUTION));
            }
        }
        Some(_) => {}
    }
    if let Some(said) = &s.one_said {
        col = col.push(match said {
            Ok(w) => crate::ui::notice(w.as_str(), theme::POSITIVE),
            Err(w) => crate::ui::notice(w.as_str(), theme::CAUTION),
        });
    }
    container(col).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

/// The scrim behind the picker: a press on it closes the picker.
pub fn scrim<'a>() -> El<'a> {
    button(space().width(Length::Fill).height(Length::Fill))
        .padding(0)
        .on_press(Msg::Close)
        .style(|_, _| button::Style { background: Some(iced::Background::Color(Color { a: 0.55, ..Color::BLACK })), ..button::Style::default() })
        .into()
}
