//! What the Research page draws for the hub (ideas to pursue, read next, the frontier, subjects to start, saved
//! papers) and for a subject's gaps.

use alelyon_research::gaps::Kind;
use alelyon_research::store::{PaperRef, StoredGap};
use iced::widget::{Column, Row, button, column, row};
use iced::{Alignment, Element, Length};

use crate::theme;

use super::{Msg, State};
use crate::ui::{self, card, label, note, strong};

type El<'a> = Element<'a, Msg>;

fn kind_colour(k: Kind) -> iced::Color {
    match k {
        Kind::Unbridged | Kind::Combination => theme::GOLD,
        Kind::Forgotten => theme::GOLD_DIM,
        Kind::Young => theme::POSITIVE,
        Kind::Stalled | Kind::BlindSpot => theme::TEXT_DIM,
    }
}

fn paper_line<'a>(p: &'a PaperRef, subject: &'a str, saved: bool) -> El<'a> {
    row![
        button(label(format!("{} {}", p.year.map_or("····".into(), |y| y.to_string()), ui::cut(&p.title, 110)), 12.5, theme::TEXT))
            .padding([3.0, 0.0])
            .width(Length::Fill)
            .on_press(Msg::GoTo(subject.to_string(), p.work))
            .style(theme::ghost_button),
        if saved { ui::secondary("Saved", Some(Msg::Unsave(p.work))) } else { ui::secondary("Save", Some(Msg::Save(p.work, subject.to_string()))) },
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// One gap as a card's row: what kind, the headline, the evidence, its papers, and what can be done with it.
pub fn gap_row<'a>(state: &'a State, g: &'a StoredGap, because: &'a [String], show_subject: bool) -> El<'a> {
    // In a subject's own list the kind is its section's heading; in the hub it is the chip.
    let mut head = Row::new().spacing(8).align_y(Alignment::Center);
    if show_subject {
        head = head.push(ui::chip(g.kind.title(), kind_colour(g.kind))).push(label(format!("in \"{}\"", g.subject), 11.5, theme::TEXT_FAINT));
    }
    if !because.is_empty() {
        head = head.push(label(format!("matches your interest in {}", because.join(", ")), 11.5, theme::GOLD_DIM));
    }
    let mut col = Column::new()
        .spacing(4)
        .push(head)
        .push(strong(g.headline.as_str(), 13.5, theme::TEXT))
        .push(label(g.detail.as_str(), 12.0, theme::TEXT_DIM));
    for p in g.works.iter().take(4) {
        col = col.push(paper_line(p, &g.subject, state.saved_ids.contains(&p.work)));
    }
    if let Some(f) = g.findings.first() {
        col = col.push(finding(f, g.findings.len() - 1));
    }
    let mut actions = Row::new().spacing(8).align_y(Alignment::Center);
    // One click: the brief goes to whoever answers on Sinai's page, and that page opens (`pursue`).
    let ask = if g.pursued.is_some() { "Ask Sinai again" } else { "Work on this with Sinai" };
    actions = actions.push(ui::primary(ask, Some(Msg::Pursue(g.clone()))));
    if g.terms.len() >= 2 {
        actions = actions.push(ui::secondary("Start a subject from this", state.running.is_none().then(|| Msg::StartFromGap(g.clone()))));
    }
    actions = actions.push(ui::secondary("Not useful", Some(Msg::Dismiss(g.subject.clone(), g.headline.clone()))));
    if let Some(at) = &g.pursued {
        actions = actions.push(label(format!("handed to Sinai {at}"), 11.5, theme::GOLD_DIM));
    }
    col.push(actions).into()
}

/// What Sinai (or another agent) concluded about a gap, kept beside it: its verdict, why, the next step and sources.
fn finding(f: &alelyon_research::store::Finding, earlier: usize) -> El<'_> {
    let (title, colour) = match f.verdict.as_str() {
        "real" => ("Real", theme::POSITIVE),
        "closed" => ("Already closed", theme::TEXT_DIM),
        _ => ("Unclear", theme::GOLD_DIM),
    };
    let mut col = Column::new()
        .spacing(3)
        .push(
            row![ui::chip(title, colour), label(format!("{}'s finding, {}", f.by, f.at), 11.5, theme::TEXT_FAINT)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .push(label(f.why.as_str(), 12.0, theme::TEXT));
    if !f.next_step.is_empty() {
        col = col.push(label(format!("Next step: {}", f.next_step), 12.0, theme::GOLD));
    }
    for s in f.sources.iter().take(4) {
        let line = label(ui::cut(s, 110), 11.5, theme::TEXT_DIM);
        col = col.push(match super::pursue::source_link(s) {
            Some(url) => button(line).padding([1.0, 0.0]).style(theme::ghost_button).on_press(Msg::OpenLink(url)).into(),
            None => El::from(line),
        });
    }
    if earlier > 0 {
        col = col.push(label(format!("and {earlier} earlier finding{}", if earlier == 1 { "" } else { "s" }), 11.0, theme::TEXT_FAINT));
    }
    iced::widget::container(col).padding([6.0, 10.0]).style(theme::chip(colour)).into()
}

pub fn hub(state: &State, phase: f32) -> El<'_> {
    let mut page = Column::new().spacing(14).width(Length::Fill);
    let hub = match &state.hub {
        None => return page.push(ui::working(phase, "Reading your archive…")).into(),
        Some(Err(why)) => return page.push(ui::notice(why.as_str(), theme::CAUTION)).into(),
        Some(Ok(h)) => h,
    };
    let empty = matches!(&state.topics, Some(Ok(t)) if t.is_empty());
    page = page.push(column![
        strong("Your research hub", 20.0, theme::TEXT),
        note("What to work on next, from your subjects' gaps and from what you search, open and save here. It is worked out on this PC from \
              the archive; nothing about you is sent anywhere."),
    ]
    .spacing(4));
    if empty {
        return page
            .push(card(
                "Start here",
                Column::new()
                    .push(label(
                        "Start a subject: name it, give the searches you would type and the ideas a relevant paper mentions. The archive gathers \
                         its papers, maps how they connect, and this hub then shows where the subject has gaps worth pursuing.",
                        13.0,
                        theme::TEXT_DIM,
                    ))
                    .push(ui::primary("New subject", state.running.is_none().then_some(Msg::NewSubject))),
            ))
            .into();
    }
    if !hub.interests.is_empty() {
        let mut chips = Row::new().spacing(6).push(label("Your interests, as read here:", 12.0, theme::TEXT_FAINT));
        for w in hub.interests.iter().take(10) {
            chips = chips.push(ui::chip(w.as_str(), theme::TEXT_DIM));
        }
        page = page.push(chips.wrap());
    }
    let mut ideas = Column::new().spacing(14);
    if hub.ideas.is_empty() {
        ideas = ideas.push(label("No gaps found yet: they appear once a subject has enough papers (ten or more).", 12.5, theme::TEXT_DIM));
    }
    for i in &hub.ideas {
        ideas = ideas.push(gap_row(state, &i.gap, &i.because, true));
    }
    page = page.push(card("Ideas to pursue", ideas.push(note("Each idea is a lead raised by counts in your archive, not a finding: check it before relying on it."))));

    if !hub.new_work.is_empty() || !hub.newly_found.is_empty() {
        let mut col = Column::new().spacing(6);
        for n in &hub.new_work {
            col = col.push(column![paper_line(&n.paper, &n.subject, state.saved_ids.contains(&n.paper.work)), label(format!("new in \"{}\"", n.subject), 11.0, theme::TEXT_FAINT)].spacing(0));
        }
        if !hub.newly_found.is_empty() {
            col = col.push(note(format!(
                "Updates also found {} older papers the subjects had not reached before; they are in each subject's papers.",
                hub.newly_found.len()
            )));
        }
        page = page.push(card("New from your watched subjects", col));
    }
    if let Some(c) = reading(state, "Read next", &hub.read_next) {
        page = page.push(c);
    }
    if let Some(c) = reading(state, "New at the frontier", &hub.frontier) {
        page = page.push(c);
    }
    if !hub.proposals.is_empty() {
        let mut col = Column::new().spacing(8);
        for p in &hub.proposals {
            col = col.push(
                row![
                    column![strong(p.name.as_str(), 13.0, theme::TEXT), label(ui::cut(&p.why, 160), 11.5, theme::TEXT_FAINT)].spacing(2).width(Length::Fill),
                    ui::secondary("Start", state.running.is_none().then(|| Msg::StartFrom(p.clone()))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        page = page.push(card("Subjects to start", col));
    }
    if !hub.saved.is_empty() {
        let mut col = Column::new().spacing(4);
        for p in &hub.saved {
            col = col.push(paper_line(p, "", true));
        }
        page = page.push(card("Saved", col));
    }
    page.into()
}

fn reading<'a>(state: &'a State, title: &'static str, list: &'a [alelyon_research::hub::Reading]) -> Option<El<'a>> {
    if list.is_empty() {
        return None;
    }
    let mut col = Column::new().spacing(6);
    for r in list {
        col = col.push(column![paper_line(&r.paper, &r.subject, state.saved_ids.contains(&r.paper.work)), label(r.why.as_str(), 11.0, theme::TEXT_FAINT)].spacing(0));
    }
    Some(card(title, col))
}

pub fn gaps(state: &State, phase: f32) -> El<'_> {
    let mut body = Column::new().spacing(14);
    match &state.gaps {
        None => body = body.push(ui::working(phase, "Reading the gaps…")),
        Some(Err(why)) => body = body.push(ui::notice(why.as_str(), theme::CAUTION)),
        Some(Ok(list)) => {
            let shown: Vec<&StoredGap> = list.iter().filter(|g| !g.dismissed).collect();
            if shown.is_empty() {
                body = body.push(label("No gaps found: the subject may be small (under ten papers) or tightly connected.", 12.5, theme::TEXT_DIM));
            }
            let mut kind = None;
            for g in shown {
                if kind != Some(g.kind) {
                    body = body.push(ui::subheading(g.kind.title()));
                    kind = Some(g.kind);
                }
                body = body.push(gap_row(state, g, &[], false));
            }
            body = body.push(note(
                "Found from this subject's map and its papers' text, with no request to any index. Each is a lead raised by counts, not a \
                 finding: two threads may rarely cite each other because they are unrelated, and an untried combination may be untried \
                 because it fails.",
            ));
        }
    }
    card("Gaps", body)
}
