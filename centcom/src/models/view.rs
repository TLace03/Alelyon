//! The Models panel, drawn over the window as the account panel's pages are drawn: groups on panels, a line per fact,
//! plain words for what each button does.

use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text_input};
use iced::{Alignment, Element, Length};

use crate::theme;

use super::hosted::{self, Msg as H};
use super::{Catalog, Field, Msg, Pick, Ready, State, Surface, lattice_pick};
use crate::ui::{chip, dot, input_style, label, mono, note, primary, secondary, strong, working};

type El<'a> = Element<'a, Msg>;

fn group<'a>(title: &'a str, body: El<'a>) -> El<'a> {
    container(column![crate::ui::subheading(title), body].spacing(10))
        .padding([14.0, 16.0])
        .width(Length::Fill)
        .style(theme::panel)
        .into()
}

fn line<'a>(name: &'a str, value: El<'a>) -> El<'a> {
    row![container(label(name, 13.0, theme::TEXT_DIM)).width(130), value].spacing(12).align_y(Alignment::Center).into()
}

fn readiness<'a>(ready: &Ready) -> El<'a> {
    let color = match ready {
        Ready::Yes(_) => theme::POSITIVE,
        Ready::Asking(_) => theme::TEXT_FAINT,
        Ready::No(_) => theme::CAUTION,
    };
    row![dot(color), label(ready.words().to_string(), 12.5, theme::TEXT_DIM).width(Length::Fill)].spacing(8).align_y(Alignment::Center).into()
}

/// One click per surface: "Use for Sinai", "Use for Lattice chat", or a chip where it is the one in use.
fn switches<'a>(pick: &Pick, sinai: Option<&Pick>, lattice: Option<&Pick>, busy: bool) -> El<'a> {
    let mut r = Row::new().spacing(6).align_y(Alignment::Center);
    for (surface, words, in_use) in
        [(Surface::Sinai, "Use for Sinai", sinai == Some(pick)), (Surface::Lattice, "Use for Lattice chat", lattice == Some(pick))]
    {
        r = r.push(if in_use {
            chip(if surface == Surface::Sinai { "Sinai uses it" } else { "Lattice chat uses it" }, theme::GOLD)
        } else {
            secondary(words, (!busy).then(|| Msg::Use(surface, pick.clone())))
        });
    }
    r.into()
}

fn active<'a>(c: &'a Catalog, s: &'a State, lattice_choice: &'a str, mind: bool) -> El<'a> {
    let sinai = s.sinai();
    let sinai_line: El<'a> = match &sinai {
        Some(pick) => column![
            strong(c.label(pick), 13.5, theme::TEXT),
            readiness(&c.ready(pick, &s.knocked)),
            note(if c.sinai_kept.is_none() { "Not chosen yet: this is Local's model, as Lattice uses it." } else { "" }),
        ]
        .spacing(3)
        .into(),
        None => label("None chosen yet: use one below.", 13.0, theme::TEXT_FAINT).into(),
    };
    let lattice: El<'a> = match (lattice_pick(lattice_choice, &c.local), lattice_choice) {
        (Some(pick), _) => column![strong(c.label(&pick), 13.5, theme::TEXT), readiness(&c.ready(&pick, &s.knocked))].spacing(3).into(),
        (None, "auto") => label(
            if c.local.is_empty() {
                "Auto: Lattice decides, on this PC (no Local model is chosen yet).".to_string()
            } else {
                format!("Auto: Lattice decides, on this PC with {}.", c.local)
            },
            13.0,
            theme::TEXT,
        )
        .into(),
        (None, other) => label(format!("Lattice's own choice ({other})."), 13.0, theme::TEXT).into(),
    };
    let mut col = column![line("Sinai", sinai_line), line("Lattice chat", lattice)].spacing(10);
    if mind {
        col = col.push(note(
            "This build carries Sinai's mind: it answers on Sinai's page. Your model answers there only when you choose it \
             on the page, and only until Alelyon closes.",
        ));
    }
    col.into()
}

fn ggufs<'a>(c: &'a Catalog, s: &'a State, sinai: Option<&Pick>, lattice: Option<&Pick>) -> El<'a> {
    let mut col = Column::new().spacing(10);
    col = col.push(line("Folder", mono(c.models_dir.display().to_string(), 12.0, theme::TEXT).into()));
    col = col.push(line(
        "llama.cpp",
        match &c.server {
            Ok(path) => row![dot(theme::POSITIVE), mono(path.display().to_string(), 12.0, theme::TEXT_DIM)].spacing(8).align_y(Alignment::Center).into(),
            Err(why) => row![dot(theme::CAUTION), label(why.as_str(), 12.5, theme::TEXT_DIM).width(Length::Fill)].spacing(8).align_y(Alignment::Center).into(),
        },
    ));
    if c.ggufs.is_empty() {
        col = col.push(label(
            "No GGUF models are in the folder yet. Add a GGUF file… links one into it from where it is (nothing is copied \
             or downloaded).",
            13.0,
            theme::TEXT_FAINT,
        ));
    }
    for g in &c.ggufs {
        let pick = Pick::Gguf(g.name.clone());
        col = col.push(
            container(
                column![
                    row![
                        strong(g.name.as_str(), 14.0, theme::TEXT),
                        label(crate::ui::bytes(g.size), 12.0, theme::TEXT_FAINT),
                        space().width(Length::Fill),
                        switches(&pick, sinai, lattice, s.working),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                    readiness(&c.ready(&pick, &s.knocked)),
                ]
                .spacing(6),
            )
            .padding([10.0, 12.0])
            .width(Length::Fill)
            .style(theme::well),
        );
    }
    col = col.push(row![secondary("Add a GGUF file…", (!s.working).then_some(Msg::AddFile))]);
    col.into()
}

fn endpoints<'a>(c: &'a Catalog, s: &'a State, sinai: Option<&Pick>, lattice: Option<&Pick>) -> El<'a> {
    let mut col = Column::new().spacing(10);
    if c.endpoints.is_empty() {
        col = col.push(label(
            "No OpenAI-compatible endpoint is switched on. Add one for a server you run (llama.cpp's own, LM Studio, vLLM) \
             or a service you use.",
            13.0,
            theme::TEXT_FAINT,
        ));
    }
    for e in &c.endpoints {
        let pick = Pick::Endpoint(e.id.clone());
        let mut facts = column![
            row![
                strong(e.label.as_str(), 14.0, theme::TEXT),
                chip(if e.local { "on this PC" } else { "off this PC" }, if e.local { theme::POSITIVE } else { theme::GOLD_DIM }),
                space().width(Length::Fill),
                switches(&pick, sinai, lattice, s.working),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![mono(e.address.as_str(), 12.0, theme::TEXT_DIM), label(format!("model {}", if e.model.is_empty() { "not set" } else { e.model.as_str() }), 12.0, theme::TEXT_FAINT)]
                .spacing(12),
            readiness(&c.ready(&pick, &s.knocked)),
        ]
        .spacing(6);
        if e.needs_key {
            facts = facts.push(note(match e.key_kept {
                Some(place) => format!("Its key is kept in {place}; it is never shown here."),
                None => "It takes a key, and none is kept.".to_string(),
            }));
        }
        col = col.push(container(facts).padding([10.0, 12.0]).width(Length::Fill).style(theme::well));
    }
    if c.not_listed > 0 {
        col = col.push(note(format!(
            "{} more in the model registry are turned off or are not OpenAI-compatible; Lattice's Models tab edits them.",
            c.not_listed
        )));
    }
    if let Some(issue) = &c.registry_issue {
        col = col.push(label(issue.as_str(), 12.5, theme::CAUTION));
    }
    match &s.form {
        None => col = col.push(row![secondary("Add an endpoint…", (!s.working).then_some(Msg::NewEndpoint))]),
        Some(form) => {
            let input = |placeholder: &'a str, value: &'a str, field: Field| {
                text_input(placeholder, value).on_input(move |v| Msg::Form(field, v)).padding(8).size(13.5).style(input_style)
            };
            let mut f = column![
                strong("A new endpoint", 14.0, theme::TEXT),
                line("Name", input("My server", &form.label, Field::Label).into()),
                line("Address", input("http://127.0.0.1:8080/v1", &form.address, Field::Address).into()),
                line("Model", input("the model's name on the server", &form.model, Field::Model).into()),
                line("Key (optional)", input("only if the server asks for one", &form.key, Field::Key).secure(true).into()),
                note(
                    "A key goes to Windows Credential Manager as Alelyon/<its name>; the registry keeps only the name, and \
                     the key is not shown again. An endpoint off this PC with a key must use https.",
                ),
            ]
            .spacing(8);
            if let Some(problem) = &form.problem {
                f = f.push(label(problem.as_str(), 12.5, theme::CAUTION));
            }
            f = f.push(
                row![primary("Save", (!s.working).then_some(Msg::SaveEndpoint)), secondary("Cancel", Some(Msg::CancelForm))].spacing(8),
            );
            col = col.push(container(f).padding([12.0, 14.0]).width(Length::Fill).style(theme::well));
        }
    }
    col.into()
}

/// "128k context", "$0.15 in, $0.60 out per million tokens": what a hosted model's row says beside its id.
fn model_facts(m: &lattice_core::hosted::HostedModel) -> String {
    let mut facts = Vec::new();
    if let Some(n) = m.context {
        facts.push(if n >= 1000 { format!("{}k context", n / 1000) } else { format!("{n} context") });
    }
    if let Some((input, output)) = m.price {
        facts.push(if input == 0.0 && output == 0.0 {
            "free".to_string()
        } else {
            format!("${input:.2} in, ${output:.2} out per million tokens")
        });
    }
    facts.join(" \u{b7} ")
}

/// The most models a list shows at once: typing narrows it.
const SHOWN: usize = 60;

/// One provider's models: a filter, and a row each with its switches.
fn hosted_models<'a>(
    s: &'a State,
    id: &'static str,
    connected: bool,
    sinai: Option<&Pick>,
    lattice: Option<&Pick>,
    chosen: &str,
) -> El<'a> {
    let mut col = Column::new().spacing(8);
    match s.hosted.models.get(id) {
        None => col = col.push(note("Reading its list\u{2026}")),
        Some(Err(why)) => col = col.push(label(why.as_str(), 12.5, theme::CAUTION)),
        Some(Ok(models)) => {
            col = col.push(
                text_input("Filter by name, such as llama, qwen or deepseek", &s.hosted.filter)
                    .on_input(|v| Msg::Hosted(H::Filter(v)))
                    .padding(8)
                    .size(13.5)
                    .style(input_style),
            );
            let shown = hosted::shown(models, &s.hosted.filter);
            if shown.is_empty() {
                col = col.push(note("No open-weight chat model of its list holds that."));
            }
            if !connected {
                col = col.push(note("Connect it to use one of these: the list is read without your key, a model is not."));
            }
            let pick = Pick::Endpoint(id.to_string());
            for m in shown.iter().take(SHOWN) {
                let in_use = m.id == chosen;
                let mut buttons = Row::new().spacing(6).align_y(Alignment::Center);
                for (surface, words, using, now) in [
                    (Surface::Lattice, "Use for Lattice chat", "Lattice chat uses it", lattice == Some(&pick)),
                    (Surface::Sinai, "Use for Sinai", "Sinai uses it", sinai == Some(&pick)),
                ] {
                    buttons = buttons.push(if in_use && now {
                        chip(using, theme::GOLD)
                    } else {
                        secondary(words, (connected && !s.hosted.working()).then(|| Msg::Hosted(H::Use(id, m.id.clone(), surface))))
                    });
                }
                col = col.push(
                    container(
                        row![
                            column![
                                strong(m.name.as_str(), 13.5, theme::TEXT),
                                row![mono(m.id.as_str(), 11.5, theme::TEXT_DIM), label(model_facts(m), 11.5, theme::TEXT_FAINT)].spacing(10),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            buttons,
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                    .padding([8.0, 10.0])
                    .width(Length::Fill)
                    .style(theme::well),
                );
            }
            if shown.len() > SHOWN {
                col = col.push(note(format!("Showing {SHOWN} of {}: type to narrow the list.", shown.len())));
            }
        }
    }
    col.into()
}

/// The hosted providers: a card each, connected or how to connect, and its models when browsed.
fn hosted_group<'a>(c: &'a Catalog, s: &'a State, sinai: Option<&Pick>, lattice: Option<&Pick>) -> El<'a> {
    let mut col = Column::new().spacing(10).push(label(
        "Open models that a service runs for you: connect your account once, then choose a model. Your messages go to \
         that service when you use one.",
        13.0,
        theme::TEXT_DIM,
    ));
    let busy = s.hosted.working();
    for p in hosted::providers() {
        let kept = c.hosted.iter().find(|(id, _)| *id == p.id).and_then(|(_, kept)| *kept);
        let mut head = row![
            strong(p.label, 14.0, theme::TEXT),
            chip(if kept.is_some() { "connected" } else { "not connected" }, if kept.is_some() { theme::POSITIVE } else { theme::TEXT_FAINT }),
            space().width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut card = Column::new().spacing(6);
        if kept.is_some() || p.lists_without_key {
            let open = s.hosted.browsing == Some(p.id);
            head = head.push(secondary(if open { "Hide models" } else { "Browse models" }, (!busy).then_some(Msg::Hosted(H::Browse(p.id)))));
        }
        card = card.push(head).push(label(p.about, 12.5, theme::TEXT_DIM));
        match kept {
            Some(place) => card = card.push(note(format!("Its key is kept in {place}; it is never shown here."))),
            None if hosted::signs_in(p) => {
                card = card.push(if s.hosted.signing_in() {
                    row![
                        label("Approve the key in your browser; Alelyon waits for it here.", 12.5, theme::TEXT_DIM).width(Length::Fill),
                        secondary("Cancel", Some(Msg::Hosted(H::CancelSignIn))),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                } else {
                    row![primary("Sign in with OpenRouter", (!busy).then_some(Msg::Hosted(H::SignIn)))]
                });
            }
            None => {
                card = card.push(
                    row![
                        text_input("Paste your key", s.hosted.typed(p.id))
                            .on_input(move |v| Msg::Hosted(H::Typed(p.id, v)))
                            .on_submit(Msg::Hosted(H::Connect(p.id)))
                            .secure(true)
                            .padding(8)
                            .size(13.5)
                            .style(input_style),
                        primary("Connect", (!busy).then_some(Msg::Hosted(H::Connect(p.id)))),
                        secondary("Get a key", Some(Msg::Hosted(H::KeyPage(p.id)))),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
        }
        if s.hosted.browsing == Some(p.id) {
            let chosen = c.endpoint(p.id).map(|e| e.model.as_str()).unwrap_or("");
            card = card.push(hosted_models(s, p.id, kept.is_some(), sinai, lattice, chosen));
        }
        col = col.push(container(card).padding([10.0, 12.0]).width(Length::Fill).style(theme::well));
    }
    col.into()
}

/// The panel: what Sinai and Lattice's chat use now, the GGUF files, the endpoints, and how fresh it is.
pub fn panel<'a>(s: &'a State, lattice_choice: &'a str, mind: bool, phase: f32) -> El<'a> {
    let header = row![
        column![
            strong("Models", 22.0, theme::TEXT),
            label("Models on this PC and endpoints you can reach. One click makes one the model Sinai or Lattice's chat uses.", 13.0, theme::TEXT_DIM),
        ]
        .spacing(4)
        .width(Length::Fill),
        primary("Done", Some(Msg::Close)),
    ]
    .spacing(12)
    .align_y(Alignment::Start);
    let mut col = column![header].spacing(12);
    match &s.catalog {
        None => col = col.push(working(phase, "Reading the models folder and the model registry")),
        Some(c) => {
            let sinai = s.sinai();
            let lattice = lattice_pick(lattice_choice, &c.local);
            if let Some(said) = &s.said {
                col = col.push(match said {
                    Ok(words) => crate::ui::notice(words.as_str(), theme::POSITIVE),
                    Err(words) => crate::ui::notice(words.as_str(), theme::CAUTION),
                });
            }
            if s.working {
                col = col.push(working(phase, "Working on it"));
            }
            col = col
                .push(group("In use", active(c, s, lattice_choice, mind)))
                .push(group("On this PC: GGUF files", ggufs(c, s, sinai.as_ref(), lattice.as_ref())))
                .push(group("Hosted open-weight models", hosted_group(c, s, sinai.as_ref(), lattice.as_ref())))
                .push(group("Endpoints (OpenAI-compatible)", endpoints(c, s, sinai.as_ref(), lattice.as_ref())));
        }
    }
    col = col.push(crate::live::badge(&s.fresh));
    let body = scrollable(container(col).padding(20)).style(theme::scrollbars);
    container(container(body).max_width(880).max_height(820.0).style(theme::panel)).center(Length::Fill).padding(24).into()
}

/// The scrim behind the panel: a press on it closes the panel.
pub fn scrim<'a>() -> El<'a> {
    button(space().width(Length::Fill).height(Length::Fill))
        .padding(0)
        .on_press(Msg::Close)
        .style(|_, _| button::Style {
            background: Some(iced::Background::Color(iced::Color { a: 0.55, ..iced::Color::BLACK })),
            ..button::Style::default()
        })
        .into()
}
