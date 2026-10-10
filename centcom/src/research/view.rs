//! What the Research page draws: the OpenAlex key, the subjects on the left, and a subject's papers (or the
//! new-subject form, or a harvest's progress) on the right.

use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text_input};
use iced::{Alignment, Element, Length};

use crate::theme;

use alelyon_research::store::Watch;

use super::{Filter, KeyState, Msg, State, View, link_of, stop_words};
use crate::ui::{self, card, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

const PURPOSE: &str = "Every paper on a subject, not just the ones today's keywords find: the archive searches OpenAlex and arXiv, \
                       then follows citations backward and forward until a round finds nothing new, and keeps each paper \
                       with the reason it belongs.";

pub fn view(state: &State, phase: f32) -> El<'_> {
    let mut page = Column::new().spacing(18).push(ui::heading("Research", PURPOSE)).push(key_card(state, phase));
    if let Some(why) = &state.error {
        page = page.push(ui::notice(why.as_str(), theme::CAUTION));
    }
    page.push(row![subjects(state), container(space().width(1)).height(Length::Fill).style(theme::line), main(state, phase)].spacing(18))
        .into()
}

fn key_card(state: &State, phase: f32) -> El<'_> {
    let mut body = Column::new().spacing(10);
    let paste = || {
        row![
            text_input("Paste your OpenAlex API key", &state.key_input)
                .secure(true)
                .on_input(Msg::KeyInput)
                .on_submit(Msg::SaveKey)
                .padding([7.0, 10.0])
                .size(13.0)
                .style(ui::input_style)
                .width(Length::Fill),
            ui::primary("Save key", (!state.key_input.trim().is_empty()).then_some(Msg::SaveKey)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };
    match &state.key {
        KeyState::Looking => body = body.push(ui::working(phase, "Looking for a key…")),
        KeyState::Missing => {
            body = body
                .push(label(
                    "OpenAlex is the archive's citation graph: about 250 million papers and what each one cites. Without a key, \
                     it answers from a small daily budget shared by everyone on your internet connection, which one subject can \
                     use up. A key is free and takes about thirty seconds: make an account, then copy the key from its API settings.",
                    13.0,
                    theme::TEXT_DIM,
                ))
                .push(
                    row![ui::primary("Get a free key at OpenAlex", Some(Msg::GetKey)), ui::secondary("How keys work", Some(Msg::KeyHelp))]
                        .spacing(8),
                )
                .push(paste())
                .push(note("The key is kept in Windows Credential Manager, never in a file, and is sent to api.openalex.org only."));
        }
        KeyState::Present(source) => {
            let budget: El<'_> = match (&state.budget, state.checking) {
                (_, true) => ui::working(phase, "Asking OpenAlex for today's budget…"),
                (Some(Ok(b)), _) => {
                    let left = b.credits_remaining as f64 / b.credits_limit.max(1) as f64;
                    let colour = if left < 0.1 { theme::CAUTION } else { theme::TEXT };
                    let refill = b.resets_in_seconds.map(|s| format!("; refills in {} h {} min", s / 3600, (s % 3600) / 60)).unwrap_or_default();
                    label(format!("{} of {} credits left today{refill}. A page of results costs 1, a search 10.", b.credits_remaining, b.credits_limit), 13.0, colour)
                        .into()
                }
                (Some(Err(why)), _) => ui::notice(format!("OpenAlex did not confirm the key: {why}"), theme::CAUTION),
                (None, false) => label("Budget not checked yet.", 13.0, theme::TEXT_DIM).into(),
            };
            body = body
                .push(row![ui::dot(theme::POSITIVE), label(format!("Key found in {source}."), 13.0, theme::TEXT)].spacing(8).align_y(Alignment::Center))
                .push(budget);
            let mut actions = Row::new().spacing(8).push(ui::secondary("Check budget", (!state.checking).then_some(Msg::CheckBudget)));
            actions = actions.push(ui::secondary(if state.replacing { "Keep this key" } else { "Replace key" }, Some(Msg::ReplaceKey)));
            if source == "Credential Manager" {
                actions = actions.push(ui::secondary("Forget key", Some(Msg::ForgetKey)));
            }
            actions = actions.push(ui::secondary("OpenAlex key settings", Some(Msg::GetKey)));
            body = body.push(actions);
            if state.replacing {
                body = body.push(paste());
            }
        }
    }
    card("OpenAlex key", body)
}

fn subjects(state: &State) -> El<'_> {
    let hub = button(column![strong("Your hub", 13.5, if state.hub_open { theme::GOLD } else { theme::TEXT }), label("Ideas, gaps and what to read next", 11.5, theme::TEXT_FAINT)].spacing(2))
        .width(Length::Fill)
        .padding([8.0, 10.0])
        .on_press(Msg::OpenHub)
        .style(theme::list_row(state.hub_open));
    let mut list = Column::new()
        .spacing(6)
        .width(290)
        .push(hub)
        .push(ui::primary("New subject", state.running.is_none().then_some(Msg::NewSubject)));
    match &state.topics {
        None => {}
        Some(Err(why)) => list = list.push(ui::notice(why.as_str(), theme::CAUTION)),
        Some(Ok(topics)) if topics.is_empty() => list = list.push(label("No subjects yet.", 12.5, theme::TEXT_FAINT)),
        Some(Ok(topics)) => {
            for t in topics {
                let chosen = !state.creating && !state.hub_open && state.chosen.as_deref() == Some(t.name.as_str());
                let sub = match &t.last {
                    Some(r) => format!("{} papers · {}", t.members, if r.stop == "closed" { "complete" } else { "incomplete" }),
                    None => format!("{} papers", t.members),
                };
                list = list.push(
                    button(column![strong(t.name.as_str(), 13.5, if chosen { theme::GOLD } else { theme::TEXT }), label(sub, 11.5, theme::TEXT_FAINT)].spacing(2))
                        .width(Length::Fill)
                        .padding([8.0, 10.0])
                        .on_press(Msg::Pick(t.name.clone()))
                        .style(theme::list_row(chosen)),
                );
            }
        }
    }
    list.into()
}

fn main(state: &State, phase: f32) -> El<'_> {
    let mut body = Column::new().spacing(14).width(Length::Fill);
    if let Some(r) = &state.running {
        let verb = if r.update { "Updating" } else { "Gathering" };
        // A still line, not a turning spinner: a run lasts minutes to an hour and must not keep the window redrawing.
        let _ = phase;
        let mut lines = Column::new()
            .spacing(4)
            .push(strong(format!("{verb} \"{}\" in the background", r.topic), 13.5, theme::TEXT))
            .push(note("The run is a process of its own and saves after every round: closing this window does not stop it."));
        for l in &r.lines {
            lines = lines.push(mono(l.as_str(), 12.0, theme::TEXT_DIM));
        }
        let stopping = r.stopping;
        lines = lines.push(ui::secondary(if stopping { "Stopping after this request…" } else { "Stop" }, (!stopping).then_some(Msg::StopHarvest)));
        body = body.push(card("Harvest", lines));
    }
    if let Some(outcome) = &state.outcome {
        body = body.push(match outcome {
            Ok(words) => ui::notice(words.as_str(), theme::TEXT_DIM),
            Err(why) => ui::notice(format!("The harvest failed: {why}"), theme::CAUTION),
        });
    }
    if state.creating {
        return body.push(form(state)).into();
    }
    if state.hub_open {
        return body.push(super::hub_view::hub(state, phase)).push(agents(state)).into();
    }
    let Some(name) = &state.chosen else {
        return body.push(label("Start a new subject, or choose one on the left.", 14.0, theme::TEXT_DIM)).into();
    };
    let topic = match &state.topics {
        Some(Ok(t)) => t.iter().find(|t| &t.name == name),
        _ => None,
    };
    if let Some(t) = topic {
        let mut facts = Column::new().spacing(6);
        facts = facts.push(ui::fact("Searches", label(t.queries.join(" · "), 13.0, theme::TEXT)));
        facts = facts.push(ui::fact("Concepts", label(t.phrases.iter().map(|p| p.replace('|', " / ")).collect::<Vec<_>>().join(" · "), 13.0, theme::TEXT)));
        if let Some(r) = &t.last {
            let kind = if r.kind == "update" { "Update" } else { "Full gathering" };
            facts = facts
                .push(ui::fact("Last run", label(format!("{kind}: {} ({})", stop_words(&r.stop), r.finished_at), 13.0, theme::TEXT)))
                .push(ui::fact("Papers", label(format!("{} accepted of {} read, {} requests", r.accepted, r.archived, r.calls), 13.0, theme::TEXT)))
                .push(ui::fact(
                    "Found by",
                    label(format!("search {}, citations {}, both {}", r.by_search, r.by_citation, r.by_both), 13.0, theme::TEXT),
                ))
                .push(match &t.last_full {
                    Some(full) => ui::fact(
                        "Still missing",
                        label(full.missing.as_str(), 13.0, if full.missing.starts_with("UNMEASURED") { theme::TEXT_DIM } else { theme::TEXT }),
                    ),
                    None => ui::fact("Still missing", label("UNMEASURED: no full gathering saved", 13.0, theme::TEXT_DIM)),
                });
            if let Some(h) = &r.halt_reason {
                facts = facts.push(ui::fact("Why it stopped", label(h.as_str(), 13.0, theme::CAUTION)));
            }
        }
        let idle = state.running.is_none();
        facts = facts.push(ui::fact(
            "Watch",
            row![
                ui::tabs(&Watch::ALL, t.watch, |w: Watch| match w {
                    Watch::Off => "Off".to_string(),
                    Watch::Daily => "Daily".to_string(),
                    Watch::Weekly => "Weekly".to_string(),
                }, Msg::SetWatch),
                label(
                    match t.watch {
                        Watch::Off => "Not updated on its own.".to_string(),
                        _ => format!("Updated with what is new while Alelyon is open, up to {} requests each time.", super::UPDATE_CALLS),
                    },
                    12.0,
                    theme::TEXT_FAINT,
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ));
        facts = facts.push(
            row![
                ui::primary("Update now", idle.then_some(Msg::UpdateNow)),
                ui::secondary("Gather again in full", idle.then_some(Msg::GatherAgain)),
                ui::secondary("Forget subject", idle.then_some(Msg::Forget)),
            ]
            .spacing(8),
        );
        body = body.push(match state.view {
            View::Papers => card(t.name.as_str(), facts),
            // The map wants the room: the subject in one line.
            View::Map | View::Gaps => row![
                strong(t.name.as_str(), 18.0, theme::TEXT),
                label(
                    t.last.as_ref().map(|r| format!("{} papers · {}", t.members, stop_words(&r.stop))).unwrap_or_default(),
                    12.5,
                    theme::TEXT_DIM
                ),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
        });
    }
    body = body.push(ui::tabs(&[View::Papers, View::Map, View::Gaps], state.view, |v: View| match v {
        View::Papers => "Papers".to_string(),
        View::Map => "Map of connections".to_string(),
        View::Gaps => "Gaps".to_string(),
    }, Msg::ShowView));
    body = match state.view {
        View::Papers => body.push(papers(state, phase)),
        View::Map => body.push(map(state, phase)),
        View::Gaps => body.push(super::hub_view::gaps(state, phase)),
    };
    body.push(agents(state)).into()
}

/// How models and agents reach the same archive.
fn agents(state: &State) -> El<'_> {
    let body = Column::new()
        .push(label(
            "Models and agents can research with the same subjects, papers and maps through a read-only MCP server that this \
             program runs (`centcom --research-mcp`): list the subjects, read a subject's threads, foundations, bridges and \
             frontier, look up a paper's connections, or find how two papers connect. Add it in Lattice's Tools page, Claude \
             Code or any MCP client with the entry below. It reads the archive only; it fetches nothing.",
            12.5,
            theme::TEXT_DIM,
        ))
        .push(
            row![
                ui::secondary("Copy MCP setup", Some(Msg::CopyMcp)),
                label(if state.copied { "Copied." } else { "" }, 12.0, theme::TEXT_FAINT),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    card("For AI models and agents", body)
}

fn map(state: &State, phase: f32) -> El<'_> {
    let mut body = Column::new().spacing(10);
    let drawn = match &state.drawn {
        None => return card("Map of connections", body.push(ui::working(phase, "Building the map…"))),
        Some(Err(why)) => return card("Map of connections", body.push(ui::notice(why.as_str(), theme::CAUTION))),
        Some(Ok(d)) if d.graph.nodes.is_empty() => {
            return card("Map of connections", body.push(label("No papers to map yet.", 13.0, theme::TEXT_DIM)));
        }
        Some(Ok(d)) => d,
    };
    let g = &drawn.graph;
    let roles = |r: &str| g.nodes.iter().filter(|n| n.role.name() == r).count();
    body = body.push(note(format!(
        "{} papers in {} threads. Each dot is a paper, coloured by its thread and sized by its influence (PageRank over the \
         subject's citations); ringed dots are foundations ({}). Bridges between threads: {}. Frontier (newest, not yet \
         cited here): {}. Click a paper to see what it cites (gold) and what cites it (blue).",
        g.nodes.len(),
        g.communities.len(),
        roles("foundation"),
        roles("bridge"),
        roles("frontier")
    )));
    let canvas: El<'_> = match state.map() {
        Some(c) => c.width(Length::Fill).height(Length::Fixed(620.0)).into(),
        None => space().into(),
    };
    let drawing = container(canvas).padding(4).width(Length::Fill).style(theme::panel);
    body = body.push(row![drawing, side(state, drawn)].spacing(14));
    card("Map of connections", body)
}

/// Beside the map: the chosen paper's connections, or the threads.
fn side<'a>(state: &'a State, drawn: &'a super::map::Drawn) -> El<'a> {
    let mut col = Column::new().spacing(8).width(Length::Fixed(340.0));
    if let Some(near) = &state.near {
        match near {
            Err(why) => col = col.push(ui::notice(why.as_str(), theme::CAUTION)),
            Ok(None) => col = col.push(label("That paper is not in this subject's map.", 12.5, theme::TEXT_DIM)),
            Ok(Some(n)) => {
                col = col
                    .push(strong(n.node.paper.title.as_str(), 14.0, theme::TEXT))
                    .push(label(
                        format!(
                            "{} · {} · influence {} of {}",
                            n.node.paper.year.map_or("year unknown".into(), |y| y.to_string()),
                            n.node.role.name(),
                            n.rank,
                            n.of
                        ),
                        12.0,
                        theme::TEXT_DIM,
                    ));
                if let Some(c) = &n.community {
                    col = col.push(
                        button(label(format!("Thread: {}", c.label), 12.0, drawn.colour(c.id)))
                            .padding([2.0, 0.0])
                            .on_press(Msg::Thread(Some(c.id)))
                            .style(theme::ghost_button),
                    );
                }
                let list = |title: String, papers: Vec<(i64, String, Option<i32>)>| -> El<'a> {
                    let mut l = Column::new().spacing(2).push(strong(title, 12.5, theme::TEXT));
                    for (w, t, y) in papers.into_iter().take(12) {
                        l = l.push(
                            button(label(format!("{} {}", y.map_or("····".into(), |y| y.to_string()), ui::cut(&t, 70)), 12.0, theme::TEXT_DIM))
                                .padding([2.0, 0.0])
                                .on_press(Msg::Select(Some(w)))
                                .style(theme::ghost_button),
                        );
                    }
                    l.into()
                };
                let refs = |v: &[alelyon_research::store::PaperRef]| v.iter().map(|p| (p.work, p.title.clone(), p.year)).collect::<Vec<_>>();
                col = col
                    .push(list(format!("Cites ({}) here", n.cites.len()), refs(&n.cites)))
                    .push(list(format!("Cited by ({}) here", n.cited_by.len()), refs(&n.cited_by)))
                    .push(list(
                        format!("Most similar ({})", n.similar.len()),
                        n.similar.iter().map(|(p, _)| (p.work, p.title.clone(), p.year)).collect(),
                    ))
                    .push(ui::secondary("Back to the threads", Some(Msg::Select(None))));
                return container(scrollable(col).style(theme::scrollbars).height(Length::Fixed(628.0))).into();
            }
        }
    }
    col = col.push(strong("Threads", 13.0, theme::TEXT)).push(label("Click one to light it on the map.", 11.5, theme::TEXT_FAINT));
    for c in &drawn.graph.communities {
        let on = state.thread == Some(c.id);
        let years = c.years.map(|y| format!(", {}–{}", y.0, y.1)).unwrap_or_default();
        col = col.push(
            button(
                row![
                    ui::dot(drawn.colour(c.id)),
                    column![
                        label(c.label.as_str(), 12.5, if on { theme::GOLD } else { theme::TEXT }),
                        label(format!("{} papers{years}", c.size), 11.0, theme::TEXT_FAINT)
                    ]
                    .spacing(1),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([4.0, 6.0])
            .on_press(Msg::Thread(Some(c.id)))
            .style(theme::list_row(on)),
        );
    }
    container(scrollable(col).style(theme::scrollbars).height(Length::Fixed(628.0))).into()
}

fn form(state: &State) -> El<'_> {
    let f = &state.form;
    let field = |title: &'static str, hint: &'static str, value: &str, on: fn(String) -> Msg, help: &'static str| -> El<'static> {
        column![
            strong(title, 13.0, theme::TEXT),
            text_input(hint, value).on_input(on).padding([7.0, 10.0]).size(13.0).style(ui::input_style).width(Length::Fill),
            label(help, 11.5, theme::TEXT_FAINT),
        ]
        .spacing(4)
        .into()
    };
    let fields = Column::new()
        .spacing(12)
        .push(field("Name", "Low-bit weights", &f.name, Msg::FormName, "What you will call this subject."))
        .push(field(
            "Searches",
            "binary neural network weights; 1-bit quantization neural network",
            &f.queries,
            Msg::FormQueries,
            "Sent to OpenAlex and arXiv. Separate several with ;",
        ))
        .push(field(
            "Concepts",
            "binarized | binary weights | 1 bit; quantization | quantized; neural network | deep learning",
            &f.concepts,
            Msg::FormConcepts,
            "The ideas a relevant paper's title or abstract mentions, separated by ; and each idea's spellings by |. A paper that \
             mentions two different ideas is in; older papers that use other words come in through citations. Be specific: \
             \"binary\" alone also matches chemistry's binary mixtures.",
        ))
        .push(field("Budget", "2000", &f.budget, Msg::FormBudget, "Requests the run may make. A free key allows about 10,000 credits a day."))
        .push(row![ui::primary("Gather", state.running.is_none().then_some(Msg::Gather))].spacing(8));
    card("New subject", fields)
}

fn papers(state: &State, phase: f32) -> El<'_> {
    let mut body = Column::new().spacing(10);
    body = body.push(ui::tabs(&Filter::ALL, state.filter, |f: Filter| f.title().to_string(), Msg::Show));
    body = body.push(
        row![
            text_input("Find papers in this subject by words in their title or abstract", &state.find)
                .on_input(Msg::Find)
                .on_submit(Msg::FindGo)
                .padding([7.0, 10.0])
                .size(13.0)
                .style(ui::input_style)
                .width(Length::Fill),
            ui::secondary("Find", Some(Msg::FindGo)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if state.loading {
        body = body.push(ui::working(phase, "Reading the archive…"));
    }
    if let Some(hits) = &state.hits {
        match hits {
            Err(why) => body = body.push(ui::notice(why.as_str(), theme::CAUTION)),
            Ok(hits) => {
                body = body.push(label(format!("{} found", hits.len()), 12.0, theme::TEXT_FAINT));
                for h in hits {
                    body = body.push(
                        row![
                            label(h.year.map_or("····".into(), |y| y.to_string()), 12.0, theme::TEXT_FAINT).width(44),
                            label(h.title.as_str(), 13.0, theme::TEXT).width(Length::Fill),
                        ]
                        .spacing(8),
                    );
                }
                return card("Search", body);
            }
        }
    }
    let Some(members) = &state.members else { return card("Papers", body) };
    let members = match members {
        Err(why) => return card("Papers", body.push(ui::notice(why.as_str(), theme::CAUTION))),
        Ok(m) => m,
    };
    let visible = state.visible();
    body = body.push(label(format!("{} papers, oldest first", visible.len()), 12.0, theme::TEXT_FAINT));
    let mut list = Column::new().spacing(4);
    for &i in visible.iter().take(state.shown) {
        let m = &members[i];
        let open = state.detail == Some(i);
        let reason = reason_words(&m.reason);
        let mut entry = Column::new().spacing(4).push(
            row![
                label(m.work.year.map_or("····".into(), |y| y.to_string()), 12.0, theme::TEXT_FAINT).width(44),
                label(m.work.title.as_str(), 13.0, if open { theme::GOLD } else { theme::TEXT }).width(Length::Fill),
                ui::chip(reason, if m.reason.starts_with("text:") { theme::TEXT_DIM } else { theme::GOLD_DIM }),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        if open {
            if !m.work.authors.is_empty() {
                entry = entry.push(label(ui::cut(&m.work.authors.join(", "), 240), 12.0, theme::TEXT_DIM));
            }
            if let Some(v) = &m.work.venue {
                entry = entry.push(label(v.as_str(), 12.0, theme::TEXT_DIM));
            }
            if let Some(a) = &m.work.abstract_text {
                entry = entry.push(label(ui::cut(a, 1600), 12.5, theme::TEXT));
            }
            let found = match m.via.as_str() {
                "search" => "Found by keyword search.",
                "citation" => "Found only by following citations.",
                "both" => "Found by keyword search and by citations.",
                _ => "",
            };
            let mut meta = Row::new().spacing(8).align_y(Alignment::Center).push(label(found, 12.0, theme::TEXT_FAINT));
            if let Some(n) = m.work.cited_by_count {
                meta = meta.push(label(format!("cited {n} times"), 12.0, theme::TEXT_FAINT));
            }
            if let Some(url) = link_of(m) {
                meta = meta.push(ui::secondary("Open paper", Some(Msg::OpenLink(url))));
            }
            meta = meta.push(ui::secondary("Show on map", Some(Msg::ShowOnMap(m.id))));
            meta = meta.push(if state.saved_ids.contains(&m.id) {
                ui::secondary("Saved", Some(Msg::Unsave(m.id)))
            } else {
                ui::secondary("Save", Some(Msg::Save(m.id, state.chosen.clone().unwrap_or_default())))
            });
            entry = entry.push(meta);
        }
        list = list.push(
            button(entry).width(Length::Fill).padding([6.0, 8.0]).on_press(Msg::Detail(if open { None } else { Some(i) })).style(theme::list_row(open)),
        );
    }
    body = body.push(container(scrollable(list).style(theme::scrollbars).height(Length::Fixed(560.0))).padding(4).style(theme::panel));
    if visible.len() > state.shown {
        body = body.push(ui::secondary("Show more", Some(Msg::ShowMore)));
    }
    card("Papers", body)
}

/// A saved reason ("text:2", "cites:3:1", "cited_by:4") in words.
pub fn reason_words(reason: &str) -> String {
    let mut parts = reason.split(':');
    match (parts.next(), parts.next()) {
        (Some("text"), Some(n)) => format!("{n} concepts"),
        (Some("cites"), Some(n)) => format!("cites {n} core"),
        (Some("cited_by"), Some(n)) => format!("cited by {n} core"),
        (Some("peripheral"), Some(n)) => format!("on the edge, cited by {n}"),
        _ => reason.to_string(),
    }
}
