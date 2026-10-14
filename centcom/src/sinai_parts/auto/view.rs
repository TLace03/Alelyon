//! Auto configure on screen: the dials and the button on Sinai's page ([`strip`]), and the proposal and the setup,
//! drawn over the window as a part's picker is ([`panel`]).

use iced::widget::{Column, checkbox, column, container, progress_bar, row, scrollable, space, text_input};
use iced::{Alignment, Element, Length};

use super::catalogue::Effort;
use super::choose::{Pick, Proposal, Row, TYPICAL_USE};
use super::{Budget, Msg, State, Step};
use crate::theme;
use crate::ui::{chip, label, mono, note, primary, secondary, strong, tabs, working};

type El<'a> = Element<'a, Msg>;

fn dollars(usd: f64) -> String {
    if usd <= 0.0 {
        "$0".into()
    } else if usd < 0.01 {
        "under 1 cent".into()
    } else {
        format!("${usd:.2}")
    }
}

/// The month's paid spending, in a line: "Paid cloud this month: $0.42 of your $5.00 cap."
pub fn spending_line(s: &State) -> String {
    match (s.settings.cap(), s.month) {
        (None, Some(m)) if m > 0.0 => format!("Paid cloud this month: {}. This tier allows no paid model, so none is called.", dollars(m)),
        (None, _) => "No paid models at this tier: it costs $0.".into(),
        (Some(cap), Some(m)) if m >= cap - 0.01 => format!("Paid cloud this month: {} of your ${cap:.2} cap. Monthly cap reached: paid parts pause until next month.", dollars(m)),
        (Some(cap), Some(m)) => format!("Paid cloud this month: {} of your ${cap:.2} cap.", dollars(m)),
        (Some(cap), None) => format!("Paid cloud this month: reading, against your ${cap:.2} cap."),
    }
}

/// On Sinai's page: the two dials, Auto configure, the month's spending, a suggestion, and the way back.
pub fn strip<'a>(s: &'a State, phase: f32) -> El<'a> {
    let effort = tabs(&Effort::ALL, s.settings.effort, |e| e.title().to_string(), Msg::Effort);
    let budget = tabs(&Budget::ALL, s.settings.budget, |b| b.title().to_string(), Msg::Budget);
    let mut col = Column::new().spacing(10);
    col = col.push(
        row![
            column![strong("Auto configure Sinai", 15.0, theme::TEXT), note("Choose how hard Sinai works and what it may cost; the proposal comes before anything changes.")]
                .spacing(2)
                .width(Length::Fill),
            primary("Auto configure Sinai", (!s.gathering && s.catalogue().is_ok()).then_some(Msg::Propose)),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    col = col.push(row![label("Effort", 12.5, theme::TEXT_FAINT).width(90), effort, label(s.settings.effort.words(), 12.0, theme::TEXT_DIM)].spacing(10).align_y(Alignment::Center));
    let mut tier = row![label("Price tier", 12.5, theme::TEXT_FAINT).width(90), budget].spacing(10).align_y(Alignment::Center);
    if s.settings.budget == Budget::Custom {
        tier = tier.push(
            text_input("$ a month", &s.custom_text).on_input(Msg::CustomTyped).padding(6).size(13.0).width(110).style(crate::ui::input_style),
        );
    }
    col = col.push(tier);
    let mut words = s.settings.budget.words().to_string();
    if s.settings.budget == Budget::Custom && s.settings.custom_usd.is_none() {
        words.push_str(" Type a number of dollars (up to $1,000): until then no paid model is allowed.");
    }
    col = col.push(note(words));
    col = col.push(row![crate::ui::dot(if s.settings.cap().is_some() { theme::GOLD } else { theme::TEXT_FAINT }), label(spending_line(s), 12.5, theme::TEXT_DIM)].spacing(8).align_y(Alignment::Center));
    if s.gathering && !s.open {
        col = col.push(working(phase, "Reading this PC and the model lists"));
    }
    if s.suggestion.is_some() {
        col = col.push(
            row![
                label("Prices, providers or this PC changed: a different setup may suit you now. Nothing changes until you agree.", 12.5, theme::TEXT).width(Length::Fill),
                secondary("Review", Some(Msg::Review)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    if s.settings.previous.is_some() {
        col = col.push(
            row![
                label("Your setup from before the last Agree is kept.", 12.0, theme::TEXT_FAINT).width(Length::Fill),
                secondary("Back to my previous setup", (s.setup.as_ref().is_none_or(|x| !x.downloading())).then_some(Msg::Back)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    if let Some(said) = &s.said
        && !s.open
    {
        col = col.push(match said {
            Ok(w) => crate::ui::notice(w.as_str(), theme::POSITIVE),
            Err(w) => crate::ui::notice(w.as_str(), theme::CAUTION),
        });
    }
    if s.setup.as_ref().is_some_and(|x| !super::setup_done(x)) && !s.open {
        col = col.push(row![label("Setting up Sinai.", 12.5, theme::TEXT).width(Length::Fill), secondary("Show", Some(Msg::Propose))].spacing(10).align_y(Alignment::Center));
    }
    container(col).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

fn parts_words(r: &Row) -> String {
    r.parts.iter().map(|p| p.title()).collect::<Vec<_>>().join(" + ")
}

fn pick_lines(r: &Row) -> (String, String, String, String) {
    // model, where, download or cost, expected month
    match &r.pick {
        Pick::Local { name, files, bytes, .. } => (
            name.clone(),
            "This PC".into(),
            format!("Download {} ({})", lattice_core::fetch::words(*bytes), files.iter().map(|f| f.name()).collect::<Vec<_>>().join(", ")),
            "$0 a month".into(),
        ),
        Pick::Cloud { name, provider, model, price, monthly, .. } => (
            format!("{name} ({model})"),
            super::cloud::provider_label(provider).to_string(),
            if price.is_free() { "Free on your own account".into() } else { format!("${:.2} sent, ${:.2} written per million tokens", price.input_per_m, price.output_per_m) },
            format!("About {} a month", dollars(*monthly)),
        ),
        Pick::Keep(words) => (words.clone(), "As it is".into(), "Nothing to download".into(), "No change".into()),
        Pick::Off => ("Off".into(), String::new(), "Nothing".into(), "$0 a month".into()),
        Pick::Nothing => ("No change".into(), String::new(), "Nothing".into(), "$0 a month".into()),
    }
}

fn row_view<'a>(r: &Row, step: Option<&Step>, index: usize, phase: f32) -> El<'a> {
    let (model, place, cost, month) = pick_lines(r);
    let mut top = row![strong(parts_words(r), 14.0, theme::GOLD).width(150), strong(model, 14.0, theme::TEXT).width(Length::Fill)].spacing(10).align_y(Alignment::Center);
    if !place.is_empty() {
        top = top.push(chip(place, if matches!(r.pick, Pick::Local { .. }) { theme::POSITIVE } else { theme::GOLD_DIM }));
    }
    if r.pick.paid() {
        top = top.push(chip("paid, within your cap", theme::CAUTION));
    }
    if r.thinking {
        top = top.push(chip("thinking on", theme::GOLD_DIM));
    }
    let mut col = column![top, label(r.why.clone(), 12.5, theme::TEXT_DIM), row![label(cost, 12.5, theme::TEXT).width(Length::Fill), label(month, 12.5, theme::TEXT)].spacing(10)].spacing(4);
    if r.connect {
        col = col.push(note("Your own account for this provider is connected on its card, after Agree."));
    }
    if let Some(step) = step {
        let line: El<'a> = match step {
            Step::Nothing => space().height(0).into(),
            Step::Waiting => note("Waiting for its turn to download."),
            Step::Fetching(p) if p.checking => working(phase, format!("Checking {} against its checksum", p.name)),
            Step::Fetching(p) => column![
                label(format!("Downloading {}: {} of {}", p.name, lattice_core::fetch::words(p.all_done), lattice_core::fetch::words(p.all_total)), 12.5, theme::TEXT),
                progress_bar(0.0..=1.0, if p.all_total > 0 { p.all_done as f32 / p.all_total as f32 } else { 0.0 }).girth(6),
            ]
            .spacing(4)
            .into(),
            Step::Paused => note("Paused: it resumes from where it stopped."),
            Step::Done => label("In place, checked, and kept for this part.", 12.5, theme::POSITIVE).into(),
            Step::Cancelled => note("Cancelled: nothing of it was kept."),
            Step::Failed(why) => label(why.clone(), 12.5, theme::CAUTION).into(),
            Step::Card => row![
                label("Nothing is sent until you agree on this part's own card.", 12.5, theme::TEXT).width(Length::Fill),
                secondary("Open its card", Some(Msg::Card(index))),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            Step::Agreed => label("Agreed on its card.", 12.5, theme::POSITIVE).into(),
        };
        col = col.push(line);
    }
    container(col).padding([10.0, 12.0]).width(Length::Fill).style(theme::well).into()
}

fn proposal_view<'a>(s: &'a State, p: &'a Proposal) -> El<'a> {
    let mut col = Column::new().spacing(10);
    let tier = s.settings.budget.title();
    col = col.push(note(format!(
        "{} effort, {}. Download in all: {}. Expected: about {} a month at typical use ({TYPICAL_USE}).",
        p.effort.title(),
        tier,
        if p.download > 0 { lattice_core::fetch::words(p.download) } else { "nothing".into() },
        dollars(p.monthly)
    )));
    if let Some(g) = &s.gathered {
        let budget = g.machine.budget.map(lattice_core::fetch::words).unwrap_or_else(|| "UNMEASURED".into());
        col = col.push(note(format!("This PC's memory budget: {budget} ({}).", g.machine.basis)));
        if let Some(free) = g.machine.disk_free {
            col = col.push(note(format!("Free disk where models are kept: {}.", lattice_core::fetch::words(free))));
        }
    }
    for (i, r) in p.rows.iter().enumerate() {
        col = col.push(row_view(r, None, i, 0.0));
    }
    col = col.push(checkbox(s.eyes_on).label("Include Sinai's eyes (off until you turn them on)").on_toggle(Msg::EyesOn).size(16).text_size(13.0));
    col = col.push(note(
        "Sizes and checksums come from each model's public file list on Hugging Face; nothing of yours was sent to read them. \
         Nothing is downloaded, connected or paid before you agree. After Agree, each cloud part still has its own card to agree to.",
    ));
    if p.tier.cap().is_some() {
        col = col.push(note("A paid tier is your consent to pay up to its cap a month and no more. At the cap, paid parts pause; Alelyon never moves them to a pricier model or another provider."));
    }
    col = col.push(row![secondary("Not now", Some(Msg::Close)), space().width(Length::Fill), primary("Agree", Some(Msg::Agree))].spacing(10).align_y(Alignment::Center));
    col.into()
}

/// The proposal, or the setup under way, over the window.
pub fn panel<'a>(s: &'a State, phase: f32) -> El<'a> {
    let mut col = column![row![
        column![strong("Auto configure Sinai", 22.0, theme::TEXT), label("One row per part: what it would use, where, why, and what it costs.", 13.0, theme::TEXT_DIM)]
            .spacing(4)
            .width(Length::Fill),
        secondary("Close", Some(Msg::Close)),
    ]
    .spacing(12)]
    .spacing(12);
    if let Some(said) = &s.said {
        col = col.push(match said {
            Ok(w) => crate::ui::notice(w.as_str(), theme::POSITIVE),
            Err(w) => crate::ui::notice(w.as_str(), theme::CAUTION),
        });
    }
    if let Some(setup) = &s.setup {
        for (i, (r, step, _)) in setup.rows.iter().enumerate() {
            col = col.push(row_view(r, Some(step), i, phase));
        }
        let mut buttons = row![].spacing(10).align_y(Alignment::Center);
        if setup.downloading() {
            buttons = buttons.push(secondary("Pause", Some(Msg::Pause)));
        }
        if setup.paused() {
            buttons = buttons.push(secondary("Resume", Some(Msg::Resume)));
        }
        if setup.downloading() || setup.paused() || setup.rows.iter().any(|(_, s, _)| *s == Step::Waiting) {
            buttons = buttons.push(secondary("Cancel downloads", Some(Msg::Cancel)));
        }
        buttons = buttons.push(space().width(Length::Fill)).push(primary("Done", Some(Msg::Close)));
        col = col.push(buttons);
        col = col.push(mono("Downloads go to the models folder (the speech models folder for the ears).", 11.5, theme::TEXT_FAINT));
    } else if s.gathering {
        col = col.push(working(phase, "Reading this PC's memory, the free disk and the model lists"));
    } else if let Some(p) = &s.proposal {
        col = col.push(proposal_view(s, p));
    } else {
        col = col.push(note("No proposal: press Auto configure Sinai on Sinai's page."));
    }
    let body = scrollable(container(col).padding(20)).style(theme::scrollbars);
    container(container(body).max_width(900).max_height(800.0).style(theme::panel)).center(Length::Fill).padding(24).into()
}
