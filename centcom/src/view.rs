//! What CENTCOM draws: the rail, the on-air bar, and each page.
//!
//! Words are plain and say what will happen. Anything in flight shows the spinner with what it is doing, and
//! whatever is listening is named in a bar across the top of every page (the CHARTER's on-air honesty).

use iced::widget::{
    Column, Row, Text, button, column, container, pane_grid, progress_bar, row, scrollable, space, stack, text, text_editor,
    text_input,
};
use iced::widget::image;
use iced::{Alignment, Background, Border, Color, ContentFit, Element, Length, Theme, border};

use crate::theme::{self, fonts};

use crate::app::{App, FORMATS, Link, Message, Tab, clock};
use crate::capability::Capability;
use crate::catalogue::{Feature, Here, Section};
use crate::dock;
use crate::ears;
use crate::grants;
use crate::probe::{self, Reading};
use crate::sinai;
use crate::spinner::spinner;
use crate::stage;
use crate::trust;
use crate::ui;

type El<'a> = Element<'a, Message>;

/// The rail's width: an icon and a one-word label.
const RAIL_W: f32 = 84.0;

/// The window: its own title bar and resize edges (it has no Windows frame) around what it shows. Signed in, the bar
/// is the canvas's black above the main window; on the sign-in screen it is transparent over the backdrop.
pub fn view(app: &App) -> El<'_> {
    crate::frame_stats::view();
    let bar = crate::chrome::bar(app.signin.through(), app.maximised, Message::Chrome);
    let framed: El<'_> = if app.signin.through() {
        column![bar, content(app)].into()
    } else {
        stack![content(app), column![bar]].into()
    };
    if app.maximised { framed } else { stack![framed, crate::chrome::edges(Message::Chrome)].into() }
}

fn content(app: &App) -> El<'_> {
    // Signing in comes before everything (`signin`); Use offline opens the window without an account.
    if !app.signin.through() {
        return crate::signin::view(&app.signin, app.mark.as_ref(), spinner(app.phase, 22.0)).map(Message::SignIn);
    }
    let page = match app.section {
        Section::Overview => overview(app),
        Section::Sinai => sinai(app),
        Section::Transcription => transcription(app),
        Section::Trust => trust_page(app),
        // the fleet reads the source repository's own stores, and its page is a plug-in of the builds that carry it:
        // a build without it, or a copy not run from a checkout, says so, calmly
        Section::Fleet => match &app.fleet {
            Some(fleet) if app.can(Capability::CheckoutTools) => fleet.view(app.phase).map(Message::Fleet),
            _ => absent_page(Section::Fleet, "Fleet", Capability::CheckoutTools),
        },
        Section::Data => crate::data::view(&app.data, app.phase).map(Message::Data),
        Section::Research => crate::research::view(&app.research, app.phase).map(Message::Research),
        Section::Compute => crate::compute::view(&app.compute, app.phase).map(Message::Compute),
        Section::Lattice => crate::lattice::view(&app.lattice, app.phase).map(Message::Lattice),
        Section::Account => account_page(app),
    };
    let mut body = Column::new().width(Length::Fill).height(Length::Fill);
    if let Some(bar) = on_air_bar(app) {
        body = body.push(bar);
    }
    if let Some(bar) = notice_bar(app) {
        body = body.push(bar);
    }
    body = body.push(if app.section == Section::Lattice {
        // Lattice's page is its IDE and tabs: it fills the window to its edges and keeps its own margins.
        El::from(container(page).width(Length::Fill).height(Length::Fill))
    } else if app.section == Section::Sinai {
        // Sinai's page is its docks: they fill the window and scroll inside themselves.
        El::from(container(page).padding([20.0, 24.0]).width(Length::Fill).height(Length::Fill))
    } else {
        scrollable(container(page).padding([24.0, 32.0]).max_width(1180.0).width(Length::Fill))
            .style(theme::scrollbars)
            .height(Length::Fill)
            .into()
    });
    // the rail lies on the page's own black, with no fill of its own, so the window reads as one surface from its
    // title bar down; the gold hairline stays between it and the page (2026-10-09)
    let main = row![rail(app), container(space().width(1)).height(Length::Fill).style(theme::line), body];
    if app.models.open {
        // the Models panel, over the window; a press beside it closes it
        let panel = crate::models::view::panel(&app.models, &app.lattice.choice, app.can(Capability::SinaiMind), app.phase).map(Message::Models);
        return stack![main, crate::models::view::scrim().map(Message::Models), panel].into();
    }
    if app.hovering {
        stack![main, drop_overlay(app.section)].into()
    } else if app.signin.settings.is_some() {
        // Settings, over the page, which stays on show (and live) behind the scrim; a press on the scrim closes it.
        let scrim = button(space().width(Length::Fill).height(Length::Fill))
            .padding(0)
            .on_press(Message::SignIn(crate::signin::Msg::SettingsClose))
            .style(|_, _| button::Style { background: Some(Background::Color(Color { a: 0.45, ..Color::BLACK })), ..button::Style::default() });
        let panel = container(iced::widget::opaque(crate::signin::settings::panel(&app.signin).map(Message::SignIn))).center(Length::Fill).padding(24);
        stack![main, scrim, panel].into()
    } else if app.signin.menu_open {
        // the account menu, over the rail's foot; a press anywhere else closes it
        let scrim = button(space().width(Length::Fill).height(Length::Fill))
            .padding(0)
            .on_press(Message::SignIn(crate::signin::Msg::Menu(false)))
            .style(|_, _| button::Style::default());
        let menu = container(crate::signin::menu(&app.signin).map(Message::SignIn))
            .padding(iced::Padding { left: RAIL_W + 6.0, bottom: 70.0, ..iced::Padding::ZERO })
            .align_bottom(Length::Fill);
        stack![main, scrim, menu].into()
    } else if app.social.open && app.social.active() && app.can(Capability::Friends) {
        // the friends panel, over the window's right edge (after the Riot Client's)
        let panel = container(crate::social::panel(&app.social).map(Message::Social)).align_right(Length::Fill).padding(8);
        stack![main, panel].into()
    } else if app.signin.whats_new_open {
        let scrim = button(space().width(Length::Fill).height(Length::Fill))
            .padding(0)
            .on_press(Message::SignIn(crate::signin::Msg::WhatsNew(false)))
            .style(|_, _| button::Style { background: Some(Background::Color(Color { a: 0.55, ..Color::BLACK })), ..button::Style::default() });
        let card = container(crate::signin::whats_new().map(Message::SignIn)).center(Length::Fill);
        stack![main, scrim, card].into()
    } else {
        main.into()
    }
}

// ------------------------------------------------------------------- text

fn label<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(content).size(size).color(color).font(fonts().ui)
}

fn strong<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(content).size(size).color(color).font(fonts().ui_strong)
}

fn mono<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    text(content).size(size).color(color).font(fonts().mono)
}

fn dot<'a>(color: Color) -> El<'a> {
    container(space().width(8).height(8)).style(theme::dot(color)).into()
}

fn chip<'a>(words: impl text::IntoFragment<'a>, color: Color) -> El<'a> {
    container(label(words, 11.5, color)).padding([2.0, 8.0]).style(theme::chip(color)).into()
}

fn heading<'a>(title: &'a str, purpose: &'a str) -> El<'a> {
    ui::heading(title, purpose)
}

fn subheading<'a>(words: &'a str) -> El<'a> {
    ui::subheading(words)
}

/// A spinner with what it is doing.
fn working<'a>(app: &App, words: impl text::IntoFragment<'a>) -> El<'a> {
    row![spinner(app.phase, 16.0), label(words, 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center).into()
}

// A button's words take the button's own colour, so a disabled one looks disabled.
fn primary<'a>(words: &'a str, message: Message) -> El<'a> {
    ui::primary(words, Some(message))
}

fn secondary<'a>(words: &'a str, message: Option<Message>) -> El<'a> {
    button(text(words).size(13.0).font(fonts().ui)).padding([6.0, 12.0]).on_press_maybe(message).style(theme::secondary_button).into()
}

fn ghost<'a>(words: &'a str, message: Message) -> El<'a> {
    button(text(words).size(12.5).font(fonts().ui)).padding([4.0, 8.0]).on_press(message).style(theme::ghost_button).into()
}

// ------------------------------------------------------------------- frame

fn rail(app: &App) -> El<'_> {
    let (mic, pc) = app.on_air();
    let mut items = Column::new().spacing(6).width(Length::Fill).align_x(Alignment::Center);
    // the mark over the name, in the sign-in screen's gold, as its form is headed
    let brand: El<'_> = match app.mark.as_ref() {
        Some(mark) => column![image(mark.clone()).width(30).height(30), ui::caps("Alelyon", 9.5, theme::GOLD)]
            .spacing(6)
            .align_x(Alignment::Center)
            .into(),
        None => ui::caps("Alelyon", 10.5, theme::GOLD),
    };
    items = items.push(container(brand).padding([12.0, 0.0]));
    for section in Section::ALL {
        let selected = app.section == section;
        let color = if selected { theme::GOLD } else { theme::TEXT_DIM };
        let mut icon = Row::new().spacing(4).align_y(Alignment::Center).push(label(section.glyph(), 18.0, color));
        let live = match section {
            Section::Transcription => mic.is_some() || pc.is_some(),
            Section::Sinai => app.sinai_on_air().is_some(),
            _ => false,
        };
        if live {
            icon = icon.push(dot(theme::DANGER));
        }
        items = items.push(
            button(column![icon, label(section.short(), 10.5, color)].spacing(4).align_x(Alignment::Center).width(Length::Fill))
                .width(Length::Fill)
                .padding([9.0, 2.0])
                .on_press(Message::Go(section))
                .style(theme::rail_button(selected)),
        );
    }
    items = items.push(space().height(Length::Fill));
    // friends, while signed in: how many are online, and a gold count of unread messages
    if app.social.active() && app.can(Capability::Friends) {
        let (online, unread) = app.social.list.as_ref().map(|l| (l.online(), l.unread())).unwrap_or((0, 0));
        let mut face = column![label("☺", 16.0, if app.social.open { theme::GOLD } else { theme::TEXT_DIM })]
            .spacing(3)
            .align_x(Alignment::Center)
            .width(Length::Fill);
        // gold on the chosen button, as a section's label is: faint words do not hold on its wash
        face = face.push(label(format!("{online} online"), 10.0, if app.social.open { theme::GOLD } else { theme::TEXT_FAINT }));
        if unread > 0 {
            face = face.push(label(format!("{unread} new"), 10.0, theme::GOLD));
        }
        items = items.push(
            button(face)
                .padding([8.0, 4.0])
                .on_press(Message::Social(crate::social::Msg::Panel(!app.social.open)))
                .style(theme::rail_button(app.social.open)),
        );
    }
    // the account: its initial when signed in, else a person outline; it opens the account menu
    let (glyph, words, color) = match app.signin.account() {
        Some(a) => (a.name().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or("·".into()), "you", theme::GOLD),
        None => ("○".to_string(), "offline", theme::TEXT_FAINT),
    };
    // with the menu open, the button is chosen: its words gold, as a section's are
    let (color, small) = if app.signin.menu_open { (theme::GOLD, theme::GOLD) } else { (color, theme::TEXT_FAINT) };
    items = items.push(
        button(column![label(glyph, 16.0, color), label(words, 10.0, small)].spacing(3).align_x(Alignment::Center).width(Length::Fill))
            .padding([8.0, 4.0])
            .on_press(Message::SignIn(crate::signin::Msg::Menu(!app.signin.menu_open)))
            .style(theme::rail_button(app.signin.menu_open)),
    );
    let engine: El<'_> = match &app.link {
        Link::Connected if !app.recognizer_loading() => dot(theme::POSITIVE),
        Link::Connected | Link::Connecting | Link::Lost(_) => spinner(app.phase, 14.0),
        Link::Unavailable(_) if app.starting => spinner(app.phase, 14.0),
        Link::Unavailable(_) => dot(theme::TEXT_FAINT),
    };
    items = items.push(
        container(column![engine, label("ears", 10.0, theme::TEXT_FAINT)].spacing(4).align_x(Alignment::Center))
            .padding([12.0, 0.0]),
    );
    container(items.padding([0.0, 8.0])).width(RAIL_W).height(Length::Fill).into()
}

/// What is listening right now, on every page, with one button that stops all of it.
fn on_air_bar(app: &App) -> Option<El<'_>> {
    let (mic, pc) = app.on_air();
    let sinai = app.sinai_on_air();
    if mic.is_none() && pc.is_none() && sinai.is_none() && !app.voice.on_air() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(how) = sinai {
        parts.push(format!("Sinai's microphone, {how}"));
    }
    if let Some((device, what)) = mic {
        parts.push(format!("the microphone ({device}) for {what}"));
    }
    if let Some(device) = pc {
        parts.push(format!("what this computer is playing ({device})"));
    }
    if app.voice.on_air() {
        parts.push("the microphone, recording a sentence for your voiceprint".to_string());
    }
    let words = format!("On air: {}.", parts.join("; "));
    Some(
        container(
            row![dot(theme::DANGER), label(words, 13.5, theme::TEXT), space().width(Length::Fill), secondary("Stop listening", Some(Message::OffAir))]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding([8.0, 16.0])
        .width(Length::Fill)
        .style(theme::notice(theme::DANGER))
        .into(),
    )
}

fn notice_bar(app: &App) -> Option<El<'_>> {
    let words = app.notice.as_deref()?;
    Some(
        container(
            row![label(words, 13.0, theme::TEXT), space().width(Length::Fill), ghost("Dismiss", Message::DismissNotice)]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .padding([8.0, 16.0])
        .width(Length::Fill)
        .style(theme::notice(theme::CAUTION))
        .into(),
    )
}

fn drop_overlay<'a>(section: Section) -> El<'a> {
    let (title, words) = if section == Section::Trust {
        ("Drop to check", "A receipt, its data, or a test case: read on this PC, nothing sent anywhere.")
    } else {
        ("Drop to transcribe", "The file is read on this PC and its transcript saved beside it.")
    };
    container(
        container(
            column![strong(title, 20.0, theme::GOLD), label(words, 13.5, theme::TEXT_DIM)]
                .spacing(6)
                .align_x(Alignment::Center),
        )
        .padding(32)
        .style(theme::card),
    )
    .center(Length::Fill)
    .style(theme::scrim)
    .into()
}

// ------------------------------------------------------------------- overview

fn overview(app: &App) -> El<'_> {
    let mut page = Column::new().spacing(22).push(heading("Alelyon", Section::Overview.purpose()));
    page = page.push(services(app, "Running on this computer", |_| true));
    page = page.push(subheading("Sections"));
    for section in Section::ALL.into_iter().filter(|s| *s != Section::Overview) {
        let features = section.features_with(&app.plugin_features);
        let total = features.len();
        let here = features.iter().filter(|f| f.here() != Here::Later).count();
        let status = if here == 0 { chip(format!("{total} to come"), theme::TEXT_FAINT) } else { chip(format!("{here} of {total} here"), theme::POSITIVE) };
        page = page.push(
            button(
                row![
                    container(label(section.glyph(), 20.0, theme::GOLD)).width(32),
                    column![strong(section.title(), 15.0, theme::TEXT), label(section.purpose(), 13.0, theme::TEXT_DIM)].spacing(3).width(Length::Fill),
                    status,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([12.0, 14.0])
            .on_press(Message::Go(section))
            .style(theme::list_row(false)),
        );
    }
    page.into()
}

/// The local services, as tiles, each saying whether it is running (a spinner until the first look is in).
fn services<'a>(app: &'a App, title: &'a str, keep: impl Fn(&probe::Service) -> bool) -> El<'a> {
    let mut tiles: Vec<El<'a>> = Vec::new();
    for (i, service) in probe::SERVICES.iter().enumerate().filter(|(_, s)| keep(s)) {
        let status: El<'a> = match app.probes.get(i) {
            None => working(app, "Checking"),
            Some(Reading::Up) => row![dot(theme::POSITIVE), label("Running", 13.0, theme::TEXT)].spacing(8).align_y(Alignment::Center).into(),
            Some(Reading::Down) => row![dot(theme::TEXT_FAINT), label("Not running", 13.0, theme::TEXT_DIM)].spacing(8).align_y(Alignment::Center).into(),
        };
        tiles.push(
            button(
                column![
                    strong(service.name, 14.0, theme::TEXT),
                    label(service.what, 12.5, theme::TEXT_DIM),
                    row![status, space().width(Length::Fill), mono(format!(":{}", service.port), 11.5, theme::TEXT_FAINT)].align_y(Alignment::Center),
                ]
                .spacing(6),
            )
            .width(Length::FillPortion(1))
            .padding(14)
            .on_press(Message::Go(service.section))
            .style(|t: &Theme, s| {
                let mut style = theme::secondary_button(t, s);
                style.border.radius = 12.0.into();
                style
            })
            .into(),
        );
    }
    let mut grid = Column::new().spacing(12);
    let mut tiles = tiles.into_iter();
    loop {
        let three: Vec<El<'a>> = tiles.by_ref().take(3).collect();
        if three.is_empty() {
            break;
        }
        let mut line = Row::new().spacing(12);
        let n = three.len();
        for tile in three {
            line = line.push(tile);
        }
        for _ in n..3 {
            line = line.push(space().width(Length::FillPortion(1)));
        }
        grid = grid.push(line);
    }
    // No Check again: the services are watched while this page shows them, and a change is on show when it happens.
    let live = crate::live::badge(&app.services_fresh);
    column![row![subheading(title), space().width(Length::Fill), live].align_y(Alignment::Center), grid].spacing(12).into()
}

// ------------------------------------------------------------------- transcription

fn transcription(app: &App) -> El<'_> {
    let mut page = Column::new().spacing(20).push(heading("Transcription", Section::Transcription.purpose())).push(engine_card(app));
    let mut tabs = Row::new().spacing(8);
    for tab in Tab::ALL {
        tabs = tabs.push(
            button(label(tab.title(), 13.0, if app.tab == tab { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([7.0, 14.0])
                .on_press(Message::Tab(tab))
                .style(theme::segment_button(app.tab == tab)),
        );
    }
    page = page.push(tabs);
    page.push(match app.tab {
        Tab::Captions => captions_tab(app),
        Tab::Dictation => dictation_tab(app),
        Tab::Pc => pc_tab(app),
        Tab::Files => files_tab(app),
        Tab::Library => library_tab(app),
    })
    .into()
}

fn engine_card(app: &App) -> El<'_> {
    let line: El<'_> = match &app.link {
        Link::Connected if app.recognizer_loading() => working(app, "Loading the speech model. The first start takes a few seconds."),
        Link::Connected => {
            let model = app.state.as_ref().map(|s| s.model.trim_end_matches(".bin").to_string()).unwrap_or_default();
            row![dot(theme::POSITIVE), label("The speech engine is ready.", 13.5, theme::TEXT), mono(model, 12.0, theme::TEXT_FAINT)]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
        }
        Link::Connecting => working(app, "Looking for the speech engine"),
        Link::Lost(why) => column![working(app, "Reconnecting to the speech engine"), label(why.as_str(), 12.0, theme::TEXT_FAINT)].spacing(4).into(),
        Link::Unavailable(_) if app.starting => working(app, "Starting the speech engine and loading its model"),
        Link::Unavailable(why) => {
            let start: El<'_> = match ears::engine_exe() {
                Some(Ok(_)) => primary("Start the speech engine", Message::StartEngine),
                Some(Err(why)) => label(why, 12.5, theme::CAUTION).into(),
                None => label(crate::installed::EARS_NOT_IN_BUILD, 12.5, theme::TEXT_FAINT).into(),
            };
            column![row![dot(theme::TEXT_FAINT), label(why.as_str(), 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center), start]
                .spacing(10)
                .into()
        }
    };
    let mut card = Column::new().spacing(8).push(line);
    if let Some(note) = &app.engine_note {
        card = card.push(label(note.as_str(), 12.5, theme::CAUTION));
    }
    card = card.push(label("Everything stays on this PC. Nothing is kept unless you save it to the library, copy it, or save a file's transcript.", 12.0, theme::TEXT_FAINT));
    container(card).padding(16).width(Length::Fill).style(theme::panel).into()
}

/// A start or stop button for one use of a device, and the line that says whether it is on.
fn toggle<'a>(on: bool, start: &'a str, stop: &'a str, message: Message, live: Option<String>, enabled: bool) -> El<'a> {
    let action: El<'a> = if on {
        secondary(stop, Some(message))
    } else if enabled {
        primary(start, message)
    } else {
        secondary(start, None)
    };
    let status: El<'a> = match live {
        Some(device) if on => row![dot(theme::DANGER), label(format!("On air: {device}"), 13.0, theme::TEXT)].spacing(8).align_y(Alignment::Center).into(),
        _ => label("Off", 13.0, theme::TEXT_FAINT).into(),
    };
    row![action, status].spacing(14).align_y(Alignment::Center).into()
}

fn caption_box<'a>(app: &'a App, captions: &'a crate::app::Captions, empty: &'a str) -> El<'a> {
    let mut lines = Column::new().spacing(8);
    if captions.lines.is_empty() && !captions.in_progress() {
        lines = lines.push(label(empty, 13.5, theme::TEXT_FAINT));
    }
    for (start, words) in &captions.lines {
        lines = lines.push(
            row![container(mono(clock(*start), 11.5, theme::TEXT_FAINT)).width(48), label(words.as_str(), 15.0, theme::TEXT).width(Length::Fill)]
                .spacing(10),
        );
    }
    if captions.in_progress() {
        let words: El<'a> = if captions.stable.is_empty() && captions.settling.is_empty() {
            working(app, "Hearing speech")
        } else {
            row![label(captions.stable.as_str(), 15.0, theme::TEXT), label(captions.settling.as_str(), 15.0, theme::TEXT_FAINT)]
                .spacing(5)
                .wrap()
                .into()
        };
        lines = lines.push(row![container(mono("now", 11.5, theme::GOLD)).width(48), words].spacing(10));
    }
    container(scrollable(container(lines).padding(16).width(Length::Fill)).anchor_bottom().style(theme::scrollbars).height(Length::Fill))
        .height(Length::Fixed(340.0))
        .width(Length::Fill)
        .style(theme::well)
        .into()
}

/// Keep, copy or clear what is on screen; and, once kept, where it went.
fn copy_clear<'a>(app: &'a App, text: String, clear: Message, save: Message) -> El<'a> {
    let has = !text.trim().is_empty();
    let buttons = row![
        secondary("Save to library", has.then_some(save)),
        secondary("Copy all", has.then_some(Message::Copy(text))),
        secondary("Clear", has.then_some(clear)),
    ]
    .spacing(8);
    match &app.kept {
        Some(kept) => column![buttons, label(kept.as_str(), 12.5, theme::POSITIVE)].spacing(6).into(),
        None => buttons.into(),
    }
}

fn captions_tab(app: &App) -> El<'_> {
    let s = app.state.clone().unwrap_or_default();
    let device = (!s.mic_device.is_empty()).then(|| s.mic_device.clone());
    column![
        toggle(s.listening, "Start live captions", "Stop live captions", Message::ToggleCaptions, device, app.connected()),
        caption_box(app, &app.captions, "Turn captions on and speak: your words appear here as you say them. Grey words are still settling."),
        copy_clear(app, app.captions.text(), Message::ClearCaptions, Message::SaveCaptions),
        label("Sinai acts only on speech addressed to it by name.", 12.0, theme::TEXT_FAINT),
    ]
    .spacing(14)
    .into()
}

fn dictation_tab(app: &App) -> El<'_> {
    let s = app.state.clone().unwrap_or_default();
    let device = (!s.mic_device.is_empty()).then(|| s.mic_device.clone());
    let preview: El<'_> = if app.dictation_partial.is_empty() {
        label("Speak; each finished sentence is added at the end. You can edit the text as you go.", 12.5, theme::TEXT_FAINT).into()
    } else {
        row![spinner(app.phase, 12.0), label(app.dictation_partial.as_str(), 13.5, theme::TEXT_FAINT)].spacing(8).align_y(Alignment::Center).into()
    };
    column![
        toggle(s.dictating, "Start dictation", "Stop dictation", Message::ToggleDictation, device, app.connected()),
        text_editor(&app.dictation)
            .placeholder("Your dictated text")
            .on_action(Message::Dictation)
            .height(Length::Fixed(300.0))
            .padding(14)
            .size(15)
            .font(fonts().ui)
            .style(theme::editor),
        preview,
        copy_clear(app, app.dictation.text(), Message::ClearDictation, Message::SaveDictation),
    ]
    .spacing(14)
    .into()
}

fn pc_tab(app: &App) -> El<'_> {
    let s = app.state.clone().unwrap_or_default();
    let device = (!s.pc_device.is_empty()).then(|| s.pc_device.clone());
    column![
        toggle(s.pc_on, "Caption what this computer plays", "Stop", Message::TogglePc, device, app.connected()),
        caption_box(app, &app.pc, "Turn this on to caption a meeting, a call or a video playing on this computer."),
        copy_clear(app, app.pc.text(), Message::ClearPc, Message::SavePc),
    ]
    .spacing(14)
    .into()
}

fn input_style(_: &Theme, status: text_input::Status) -> text_input::Style {
    let edge = match status {
        text_input::Status::Focused { .. } => theme::GOLD,
        text_input::Status::Hovered => theme::GOLD_DIM,
        _ => theme::LINE,
    };
    text_input::Style {
        background: Background::Color(theme::CANVAS),
        border: Border { color: edge, width: 1.0, radius: 8.0.into() },
        icon: theme::TEXT_DIM,
        placeholder: theme::TEXT_FAINT,
        value: theme::TEXT,
        selection: theme::with_alpha(theme::GOLD, 0.30),
    }
}

fn bar_style(_: &Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: Background::Color(theme::LINE),
        bar: Background::Color(theme::GOLD),
        border: border::rounded(3.0),
    }
}

fn files_tab(app: &App) -> El<'_> {
    let mut formats = Row::new().spacing(6).align_y(Alignment::Center).push(label("Save as", 13.0, theme::TEXT_DIM));
    for (i, (name, on)) in FORMATS.iter().zip(app.formats).enumerate() {
        formats = formats.push(
            button(label(name.to_uppercase(), 12.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([4.0, 10.0])
                .on_press(Message::Format(i))
                .style(theme::segment_button(on)),
        );
    }
    let entry = row![
        text_input("Drop an audio file on this window, or type its path", &app.file_path)
            .on_input(Message::FilePath)
            .on_submit(Message::Transcribe)
            .padding(9)
            .size(14)
            .font(fonts().ui)
            .style(input_style),
        if app.connected() && !app.file_path.trim().is_empty() { primary("Transcribe", Message::Transcribe) } else { secondary("Transcribe", None) },
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let mut page = Column::new()
        .spacing(14)
        .push(entry)
        .push(formats)
        .push(label("WAV, MP3, M4A, WMA and FLAC. Transcripts are saved beside the audio file and never overwrite one.", 12.5, theme::TEXT_FAINT));
    if app.jobs.is_empty() {
        page = page.push(
            container(label("No files yet. Drop a recording anywhere on this window to start.", 13.5, theme::TEXT_FAINT))
                .padding(24)
                .width(Length::Fill)
                .style(theme::well),
        );
    }
    for job in &app.jobs {
        page = page.push(job_card(app, job));
    }
    page.into()
}

/// The transcripts a person kept: search, copy, show, delete (to a Deleted folder), and, asked twice, empty it.
fn library_tab(app: &App) -> El<'_> {
    // No Refresh: the library's folder is watched while this tab shows it.
    let refresh: El<'_> = if app.library_loading { spinner(app.phase, 16.0) } else { crate::live::badge(&app.library_fresh) };
    let mut page = Column::new()
        .spacing(12)
        .push(
            row![
                text_input("Search the library", &app.library_query).on_input(Message::LibrarySearch).padding(9).size(14).font(fonts().ui).style(input_style),
                refresh,
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .push(label("Kept on this PC, in ~/.alelyon/angel/transcripts, until you delete them.", 12.5, theme::TEXT_FAINT));
    if let Some(kept) = &app.kept {
        page = page.push(label(kept.as_str(), 13.0, theme::POSITIVE));
    }
    let shown: Vec<_> = app.library.iter().filter(|e| e.matches(&app.library_query)).collect();
    if app.library.is_empty() && !app.library_loading {
        page = page.push(
            container(label("Nothing kept yet. Save captions or dictation, or keep a file's transcript, and it appears here.", 13.5, theme::TEXT_FAINT))
                .padding(24)
                .width(Length::Fill)
                .style(theme::well),
        );
    } else if shown.is_empty() && !app.library.is_empty() {
        page = page.push(label("No kept transcript has every word of that search.", 13.5, theme::TEXT_FAINT));
    }
    for entry in shown {
        page = page.push(
            container(
                column![
                    row![
                        chip(entry.kind.clone(), theme::GOLD),
                        strong(entry.name.as_str(), 14.0, theme::TEXT),
                        space().width(Length::Fill),
                        secondary("Copy", Some(Message::Copy(entry.text.clone()))),
                        secondary("Show in folder", Some(Message::OpenFolder(entry.path.display().to_string()))),
                        secondary("Delete", Some(Message::LibraryDelete(entry.path.clone()))),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    label(entry.preview(), 13.0, theme::TEXT_DIM),
                ]
                .spacing(8),
            )
            .padding(14)
            .width(Length::Fill)
            .style(theme::panel),
        );
    }
    if app.library_deleted > 0 {
        let n = app.library_deleted;
        page = page.push(if app.library_confirm_empty {
            row![
                label(format!("Remove {n} deleted transcript(s) for good? This cannot be undone."), 13.0, theme::CAUTION),
                space().width(Length::Fill),
                secondary("Remove for good", Some(Message::LibraryEmptyConfirm)),
                secondary("Keep them", Some(Message::LibraryEmptyCancel)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        } else {
            row![
                label(format!("{n} in the Deleted folder: move a file back to restore it."), 12.5, theme::TEXT_FAINT),
                space().width(Length::Fill),
                secondary("Empty Deleted", Some(Message::LibraryEmptyAsk)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        });
    }
    page.into()
}

fn job_card<'a>(app: &'a App, job: &'a ears::Job) -> El<'a> {
    let state: El<'a> = match job.state.as_str() {
        "decoding" => working(app, "Reading the file"),
        "transcribing" => working(app, format!("Transcribing, {:.0}%", job.progress * 100.0)),
        "done" => {
            let detail = match (job.lines, job.seconds) {
                (Some(lines), Some(seconds)) => format!("Done: {lines} lines, {} of audio", clock(seconds)),
                _ => "Done".to_string(),
            };
            row![dot(theme::POSITIVE), label(detail, 13.0, theme::TEXT)].spacing(8).align_y(Alignment::Center).into()
        }
        "cancelled" => label("Stopped", 13.0, theme::TEXT_DIM).into(),
        "failed" => label(format!("Failed: {}", job.error.as_deref().unwrap_or("no reason given")), 13.0, theme::DANGER).into(),
        other => label(other.to_string(), 13.0, theme::TEXT_DIM).into(),
    };
    let mut actions = Row::new().spacing(8);
    if job.running() {
        actions = actions.push(secondary("Stop", Some(Message::Cancel(job.id.clone()))));
    }
    if let Some(first) = job.outputs.first() {
        actions = actions.push(secondary("Keep in library", Some(Message::KeepJob(job.path.clone(), job.outputs.clone()))));
        actions = actions.push(secondary("Copy the text", Some(Message::CopyTranscript(job.outputs.clone()))));
        actions = actions.push(secondary("Show in folder", Some(Message::OpenFolder(first.clone()))));
    }
    let mut card = Column::new()
        .spacing(10)
        .push(row![strong(job.name(), 14.5, theme::TEXT), space().width(Length::Fill), actions].align_y(Alignment::Center))
        .push(state);
    if job.running() || job.state == "done" {
        card = card.push(progress_bar(0.0..=1.0, job.progress).girth(6).style(bar_style));
    }
    for output in &job.outputs {
        card = card.push(mono(output.as_str(), 11.5, theme::TEXT_FAINT));
    }
    container(card).padding(16).width(Length::Fill).style(theme::panel).into()
}

// ------------------------------------------------------------------- sinai

fn sinai(app: &App) -> El<'_> {
    if let Some(draft) = &app.grants_draft {
        return scrollable(grants_editor(app, draft)).height(Length::Fill).into();
    }
    if let Some(creator) = &app.creator {
        return crate::face::creator::view(creator, crate::appearance::Catalog::builtin()).map(Message::Creator);
    }
    column![loop_card(app), docks(app)].spacing(16).height(Length::Fill).into()
}

/// The grants file, edited: each grant's fields, then a review of what the save adds and reduces, then the write.
fn grants_editor<'a>(app: &'a App, draft: &'a grants::Draft) -> El<'a> {
    let field = |placeholder: &'static str, value: &'a str, i: usize, which: grants::Field| -> El<'a> {
        text_input(placeholder, value)
            .on_input(move |v| Message::GrantField(i, which, v))
            .padding(8)
            .size(13.5)
            .font(fonts().ui)
            .style(input_style)
            .into()
    };
    let named = |name: &'static str, input: El<'a>| -> El<'a> { column![label(name, 12.0, theme::TEXT_DIM), input].spacing(4).into() };
    let mut page = Column::new()
        .spacing(14)
        .push(heading("Grants", "What Sinai may do beyond this computer, written by you. Sinai's loop only reads this file, and a grant cannot authorise work on grants."))
        .push(mono(draft.path.display().to_string(), 12.0, theme::TEXT_FAINT));
    if draft.grants.is_empty() {
        page = page.push(label("No grants: Sinai asks before every act that reaches a person.", 13.0, theme::TEXT_FAINT));
    }
    for (i, g) in draft.grants.iter().enumerate() {
        let card = column![
            row![
                strong(format!("Grant {}", i + 1), 14.0, theme::GOLD),
                space().width(Length::Fill),
                secondary("Remove", Some(Message::GrantRemove(i)))
            ]
            .align_y(Alignment::Center),
            row![
                container(named("Granted by (a person, not a role)", field("Thomas Lacy", &g.issued_by, i, grants::Field::IssuedBy))).width(Length::FillPortion(3)),
                container(named("Lasts (days, from when the loop reads the file)", field("7", &g.days, i, grants::Field::Days))).width(Length::FillPortion(2)),
            ]
            .spacing(12),
            named("Recipients (exact addresses, separated by commas; no wildcards)", field("press@example.com", &g.recipients, i, grants::Field::Recipients)),
            named("Subjects (keywords that must appear; none means any subject)", field("launch, availability", &g.subjects, i, grants::Field::Subjects)),
            row![
                container(named("Spending limit, in total", field("0", &g.spend_limit, i, grants::Field::SpendLimit))).width(Length::FillPortion(3)),
                container(named("Currency", field("USD", &g.currency, i, grants::Field::Currency))).width(Length::FillPortion(1)),
            ]
            .spacing(12),
            named("Note (what you meant; Sinai reads it back to you)", field("launch announcement, press list only", &g.note, i, grants::Field::Note)),
        ]
        .spacing(10);
        page = page.push(container(card).padding(14).width(Length::Fill).style(theme::panel));
    }
    page = page.push(row![secondary("Add a grant", Some(Message::GrantAdd))]);
    match &app.grants_review {
        None => {
            page = page.push(
                row![primary("Review the save…", Message::GrantsReview), secondary("Cancel", Some(Message::GrantsCancel))].spacing(8),
            );
        }
        Some(Err(wrong)) => {
            let mut card = Column::new().spacing(6).push(strong("Not saved yet: put these right first.", 14.0, theme::CAUTION));
            for w in wrong {
                card = card.push(label(w.as_str(), 13.0, theme::TEXT));
            }
            page = page.push(container(card).padding(12).width(Length::Fill).style(theme::notice(theme::CAUTION)));
            page = page.push(row![secondary("Cancel", Some(Message::GrantsCancel))]);
        }
        Some(Ok((_, changes))) => {
            let mut card = Column::new().spacing(6).push(strong("Save these grants?", 15.0, theme::TEXT));
            if changes.adds.is_empty() && changes.reduces.is_empty() {
                card = card.push(label("No grant changes as written.", 13.0, theme::TEXT_DIM));
            }
            if !changes.adds.is_empty() {
                card = card.push(strong("Adds authority", 13.5, theme::GOLD));
                for line in &changes.adds {
                    card = card.push(label(format!("· {line}"), 13.0, theme::TEXT));
                }
            }
            if !changes.reduces.is_empty() {
                card = card.push(strong("Reduces authority", 13.5, theme::TEXT_DIM));
                for line in &changes.reduces {
                    card = card.push(label(format!("· {line}"), 13.0, theme::TEXT));
                }
            }
            for line in grants::ALWAYS {
                card = card.push(label(line, 12.5, theme::TEXT_DIM));
            }
            let blocked = app.grants_blocked();
            if let Some(why) = &blocked {
                card = card.push(label(why.clone(), 13.0, theme::CAUTION));
            }
            let write: El<'_> = if blocked.is_some() {
                secondary("Write the grants file", None)
            } else {
                primary("Write the grants file", Message::GrantsWrite)
            };
            card = card.push(row![write, secondary("Back to editing", Some(Message::GrantsBack))].spacing(8));
            page = page.push(container(card).padding(14).width(Length::Fill).style(theme::notice(theme::GOLD)));
        }
    }
    if let Some(note) = &app.grants_note {
        page = page.push(label(note.as_str(), 13.0, theme::CAUTION));
    }
    page.into()
}

/// Sinai's six views in docks, laid out as the person last arranged them (at first, the default
/// layout): drag a title bar to move a dock, drag a gap to resize, click a tab to switch.
fn docks(app: &App) -> El<'_> {
    pane_grid(&app.dock, |pane, tabs, _maximized| {
        let mut bar = Row::new().spacing(6).align_y(Alignment::Center);
        if tabs.views.len() == 1 {
            bar = bar.push(ui::caps(tabs.views[0].title(), 11.5, theme::GOLD));
        } else {
            for (i, view) in tabs.views.iter().enumerate() {
                let active = i == tabs.active;
                bar = bar.push(
                    button(label(view.title(), 12.0, if active { theme::GOLD } else { theme::TEXT_DIM }))
                        .padding([3.0, 8.0])
                        .on_press(Message::DockTab(pane, i))
                        .style(theme::segment_button(active)),
                );
            }
        }
        pane_grid::Content::new(container(dock_body(app, tabs.showing())).padding(12).width(Length::Fill).height(Length::Fill))
            .title_bar(pane_grid::TitleBar::new(bar).padding([8.0, 12.0]))
            .style(theme::panel)
    })
    .spacing(10)
    .on_resize(8, Message::DockResized)
    .on_drag(Message::DockDragged)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn scrolled(content: El<'_>) -> El<'_> {
    scrollable(container(content).padding(iced::padding::right(8)).width(Length::Fill)).style(theme::scrollbars).height(Length::Fill).into()
}

fn dock_body(app: &App, view: dock::View) -> El<'_> {
    match view {
        dock::View::Angel => angel(app),
        dock::View::Waiting => scrolled(waiting(app)),
        dock::View::Actions => scrolled(actions(app)),
        dock::View::Grants => scrolled(grants(app)),
        dock::View::Machine => scrolled(machine(app)),
        dock::View::Stage => scrolled(stage_view(app)),
    }
}

/// The processes behind Sinai, one compact row each (the dock is narrow in the default layout).
fn machine(app: &App) -> El<'_> {
    let mut list = Column::new().spacing(8);
    for (i, service) in probe::SERVICES.iter().enumerate().filter(|(_, s)| s.section == Section::Sinai) {
        let status: El<'_> = match app.probes.get(i) {
            None => spinner(app.phase, 10.0),
            Some(Reading::Up) => dot(theme::POSITIVE),
            Some(Reading::Down) => dot(theme::TEXT_FAINT),
        };
        list = list.push(
            // no port column: the dock is narrow in the default layout, and the Overview shows ports
            row![container(status).width(14), label(service.name, 13.0, theme::TEXT).width(Length::Fill)].spacing(6).align_y(Alignment::Center),
        );
    }
    list = list.push(listening(app));
    // in the Angel window's order: what it is doing and measured, its memory map, then its memory
    list = list.push(observatory(app));
    list = list.push(eyes(app));
    list = list.push(voice_enrolment(app));
    list = list.push(map_view(app));
    list = list.push(memory(app));
    list = list.push(label(
        "Erasing moves to the trash, where it can be restored; deleting for good asks twice. Nothing acts unless you press.",
        12.0,
        theme::TEXT_FAINT,
    ));
    list.into()
}

/// How Sinai listens, from its loop's /about.
fn listening(app: &App) -> El<'_> {
    let mut c = Column::new().spacing(4).push(subheading("How Sinai listens"));
    match &app.listening {
        _ if app.sinai_up != Some(true) => c = c.push(label("Not known while the loop is not running.", 12.5, theme::TEXT_FAINT)),
        None => c = c.push(working(app, "Asking the loop")),
        Some(Err(why)) => c = c.push(label(why.as_str(), 12.5, theme::CAUTION)),
        Some(Ok(l)) => {
            let yes = |b: Option<bool>| match b {
                Some(true) => "yes",
                Some(false) => "no",
                None => "not said",
            };
            c = c
                .push(label(format!("Follow-up: {}", l.follow_up()), 12.5, theme::TEXT))
                .push(label(format!("A spoken command can grant authority: {}", yes(l.voice_authority)), 12.5, theme::TEXT))
                .push(label(
                    format!(
                        "Voice gate: {}{}",
                        if l.gate_status.is_empty() { "not said" } else { l.gate_status.as_str() },
                        l.gate_threshold.map(|t| format!(", threshold {t:.3}")).unwrap_or_default()
                    ),
                    12.5,
                    theme::TEXT,
                ))
                .push(label(
                    format!(
                        "Microphone open at start: {} · overheard and discarded: {}",
                        yes(l.listen_at_start),
                        l.overheard.map(|n| n.to_string()).unwrap_or("not said".into())
                    ),
                    12.0,
                    theme::TEXT_DIM,
                ));
            if !l.gate_measured_by.is_empty() {
                c = c.push(label(format!("Measured by {}", l.gate_measured_by), 12.0, theme::TEXT_FAINT));
            }
            c = c.push(label(
                "These are set when the loop starts (its environment); the voice gate bounds casual misuse, not a recording.",
                12.0,
                theme::TEXT_FAINT,
            ));
        }
    }
    c.into()
}

/// The memory map, drawn (`memory_map`), from the loop's newest snapshot.
fn map_view(app: &App) -> El<'_> {
    let n = |x: Option<u64>| x.map(|n| n.to_string()).unwrap_or("?".into());
    let mut c = Column::new().spacing(4);
    let Some(m) = &app.conversation.memory else { return c.into() };
    c = c.push(subheading("The memory map"));
    match &m.map {
        None => c = c.push(label("Not available: this loop does not send the scene graph.", 12.0, theme::TEXT_FAINT)),
        Some(map) if map.cells.is_empty() => {
            c = c.push(label("No scene records yet: observing a window, or Sinai looking, builds them.", 12.0, theme::TEXT_FAINT))
        }
        Some(map) => {
            c = c
                .push(
                    iced::widget::canvas(crate::memory_map::Drawing { map })
                        .width(Length::Fill)
                        .height(crate::memory_map::height_for(map, 360.0)),
                )
                .push(label(
                    format!(
                        "{} record(s), {} link(s). Gold: in the last request; blue: retained. Point at one to see it.",
                        map.cells.len(),
                        n(m.edges.map(|e| e as u64))
                    ),
                    12.0,
                    theme::TEXT_DIM,
                ))
                .push(label(format!("Retrieval: {} · {}", map.retrieval, map.kv_status), 11.5, theme::TEXT_FAINT));
            for words in [&map.layout, &map.rule] {
                if !words.is_empty() {
                    c = c.push(label(words.as_str(), 11.5, theme::TEXT_FAINT));
                }
            }
        }
    }
    c.into()
}

/// Voice enrolment: the voiceprint that gates spoken authority, how it was measured, and Re-enrol and Erase,
/// each asked first; while re-enrolling, the sentence to read and a button to record it.
fn voice_enrolment(app: &App) -> El<'_> {
    use crate::voice::{Phase, Said};
    let v = &app.voice;
    let mut col = Column::new()
        .spacing(4)
        .push(row![subheading("Voice enrolment"), space().width(Length::Fill), crate::live::badge(&app.voice_fresh)].align_y(Alignment::Center));
    if let Err(why) = &v.places {
        return col.push(label(why.as_str(), 12.0, theme::TEXT_FAINT)).into();
    }
    match &v.enrolment {
        None => col = col.push(working(app, "Reading the voiceprint")),
        Some(e) if e.enrolled() => {
            col = col.push(label(
                format!(
                    "Enrolled on {}: a voiceprint of your voice from {} recordings, accepted above a likeness of {:.4}.",
                    if e.date.is_empty() { e.created.as_str() } else { e.date.as_str() },
                    e.utterances.unwrap_or(0),
                    e.threshold.unwrap_or(0.0)
                ),
                12.5,
                theme::TEXT,
            ));
            if let Some(run) = &e.run {
                col = col.push(label(format!("Its registered test: {}.", run.words()), 12.0, theme::TEXT_DIM));
            }
            col = col.push(label(
                format!("{} clip(s) of your voice kept on this PC (speaker/owner). A spoken command that grants authority is accepted only in this voice; one that takes authority away, from any voice.", e.clips),
                11.5,
                theme::TEXT_FAINT,
            ));
        }
        Some(e) => {
            col = col.push(label(
                "Not enrolled: there is no measured voiceprint, so any voice can give Sinai spoken commands that grant authority.",
                12.5,
                theme::TEXT,
            ));
            if e.clips > 0 {
                col = col.push(label(format!("{} clip(s) of your voice are in speaker/owner.", e.clips), 11.5, theme::TEXT_FAINT));
            }
        }
    }
    if let Some(e) = &v.enrolment {
        for problem in &e.problems {
            col = col.push(label(problem.as_str(), 11.5, theme::CAUTION));
        }
        if !e.aside.is_empty() {
            col = col.push(label(format!("Set aside earlier, not deleted: {}", e.aside.join(", ")), 11.0, theme::TEXT_FAINT));
        }
    }
    let small = |words: &'static str, msg: Message| -> El<'_> {
        button(text(words).size(12.0).font(fonts().ui)).padding([3.0, 8.0]).on_press(msg).style(theme::secondary_button).into()
    };
    match &v.phase {
        Phase::Idle => {
            let enrolled = v.enrolment.as_ref().is_some_and(|e| e.enrolled());
            let mut buttons = Row::new().spacing(4).push(small(if enrolled { "Re-enrol…" } else { "Enrol…" }, Message::VoiceAsk { erase: false }));
            if enrolled || v.enrolment.as_ref().is_some_and(|e| e.clips > 0) {
                buttons = buttons.push(small("Erase…", Message::VoiceAsk { erase: true }));
            }
            col = col.push(buttons);
        }
        Phase::Confirm { erase, step } => {
            col = col.push(
                container(
                    column![
                        label(crate::voice::question(*erase, *step), 12.5, theme::TEXT),
                        row![
                            primary(if *erase && *step < 2 { "Yes, continue" } else { "Yes" }, Message::VoiceConfirm),
                            secondary("Cancel", Some(Message::VoiceCancel)),
                            label(if *erase { format!("step {step} of 2") } else { String::new() }, 11.5, theme::TEXT_FAINT),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(6),
                )
                .padding(8)
                .style(theme::notice(theme::CAUTION)),
            );
        }
        Phase::Recording { prompt, listening, saved, note, .. } => {
            let mut panel = Column::new().spacing(6);
            match prompt {
                Some(Said::Prompt { index, total, voiceprint, sentence }) => {
                    panel = panel
                        .push(label(
                            format!("Sentence {index} of {total} ({}); {saved} kept", if *voiceprint { "for the voiceprint" } else { "a trial" }),
                            11.5,
                            theme::TEXT_DIM,
                        ))
                        .push(label(sentence.as_str(), 16.0, theme::TEXT));
                    panel = panel.push(if *listening {
                        row![dot(theme::DANGER), label("Listening: read it aloud now, as you usually speak to Sinai. It stops by itself when you stop.", 12.0, theme::TEXT)]
                            .spacing(6)
                            .align_y(Alignment::Center)
                            .into()
                    } else {
                        Element::from(
                            row![primary("Record this sentence", Message::VoiceRecord), secondary("Stop", Some(Message::VoiceStop))].spacing(6),
                        )
                    });
                }
                _ => panel = panel.push(working(app, "Starting the recorder")),
            }
            if let Some(note) = note {
                panel = panel.push(label(note.as_str(), 12.0, theme::CAUTION));
            }
            col = col.push(container(panel).padding(8).style(theme::notice(theme::GOLD)));
        }
        Phase::Scoring { .. } => {
            col = col.push(working(app, "Running the registered test: your 16 trials and 54 synthetic voices (about ten minutes on this PC)"));
            col = col.push(secondary("Stop", Some(Message::VoiceStop)));
        }
        Phase::Finished(result) => {
            let (words, colour) = match result {
                Ok(w) => (w.as_str(), theme::GOLD),
                Err(w) => (w.as_str(), theme::CAUTION),
            };
            col = col.push(label(words, 12.5, colour)).push(small("Done", Message::VoiceCancel));
        }
    }
    col = col.push(label(
        "The voice gate bounds casual misuse; it is not proof against a recording or a clone of your voice.",
        11.0,
        theme::TEXT_FAINT,
    ));
    col.into()
}

/// Sinai's eyes: whether its sight is open, where its hands are, its latest looks (from its journal), and what the
/// newest look found, with what each half of that rests on (the vision model's words, the system's accessibility
/// reading). Pictures of the window are the Stage's; the loop sends none.
fn eyes(app: &App) -> El<'_> {
    let c = &app.conversation;
    let mut col = Column::new().spacing(4).push(subheading("Its eyes"));
    if app.sinai_up != Some(true) {
        return col.push(label("Not known while the loop is not running.", 12.5, theme::TEXT_FAINT)).into();
    }
    col = col.push(label(
        match &c.sight {
            Some(s) => s.words(),
            None => "Whether its eyes are open is not known yet: the loop says so when they open or close.".into(),
        },
        12.5,
        theme::TEXT,
    ));
    col = col.push(label(
        match &c.hands {
            Some(h) if h.armed => format!("Its hands are on {}; the Stage shows that window.", if h.title.is_empty() { "a window" } else { h.title.as_str() }),
            Some(_) => "Its hands are not armed on any window.".to_string(),
            None => "Where its hands are is not known yet.".to_string(),
        },
        12.0,
        theme::TEXT_DIM,
    ));
    let looks: Vec<&sinai::Note> = c.journal.iter().rev().filter(|n| n.kind == "look").take(5).collect();
    if looks.is_empty() {
        col = col.push(label("No look since this window connected.", 12.0, theme::TEXT_FAINT));
    }
    for n in looks {
        let how = if n.tense == "doing" { "looking now".to_string() } else if n.outcome.is_empty() { n.tense.clone() } else { n.outcome.clone() };
        col = col.push(label(format!("{} · {}: {}", n.wall, n.what, ui::cut(&how, 120)), 12.0, if n.allowed { theme::TEXT_DIM } else { theme::CAUTION }));
    }
    let newest = c.memory.as_ref().and_then(|m| m.working.first().or(m.saved.first()));
    if let Some(v) = newest {
        col = col
            .push(label(format!("The newest look: {} · {}", v.title, v.created), 12.5, theme::TEXT))
            .push(label(
                if v.description.is_empty() { "The vision model said nothing.".to_string() } else { format!("Seen: {}", ui::cut(&v.description, 400)) },
                12.0,
                theme::TEXT_DIM,
            ))
            .push(label(format!("Vision: {} · accessibility: {}", v.vision, v.accessibility), 11.5, theme::TEXT_FAINT));
        if !v.controls.is_empty() {
            let names: Vec<String> = v.controls.iter().map(|(n, r)| format!("{n} ({r})")).collect();
            col = col.push(label(format!("Controls it read: {}", ui::cut(&names.join(", "), 300)), 11.5, theme::TEXT_FAINT));
        }
        if !v.located.is_empty() {
            col = col.push(label(format!("It located: {}", v.located), 11.5, theme::TEXT_FAINT));
        }
        col = col.push(label("A description is the vision model's report; it may be wrong, and nothing here checks it.", 11.0, theme::TEXT_FAINT));
    }
    col.into()
}

/// The observatory, as the Angel window shows it: what Sinai is doing and the rule that said so (tier 1, measured,
/// only while it answers), the last turn's ledger, the sampled forward pass (tier 2, measured, when asked for) and the
/// derived landscape (tier 3, a lexical heuristic). A reading the loop did not send is "unmeasured", never a zero.
fn observatory(app: &App) -> El<'_> {
    use crate::machine::{self, Act};
    let c = &app.conversation;
    let mut col = Column::new().spacing(4).push(subheading("The observatory"));
    if app.sinai_up != Some(true) {
        return col.push(label("Not known while the loop is not running.", 12.5, theme::TEXT_FAINT)).into();
    }
    let num = |v: &serde_json::Value, key: &str, f: &dyn Fn(f64) -> String| v.get(key).and_then(|x| x.as_f64()).map(f).unwrap_or_else(|| "unmeasured".into());
    // tier 1
    if c.pulse.is_object() {
        let p = &c.pulse;
        col = col
            .push(label(format!("Now: {}", p.get("state").and_then(|s| s.as_str()).unwrap_or("unmeasured")), 12.5, theme::TEXT))
            .push(label(p.get("rule").and_then(|s| s.as_str()).unwrap_or(""), 11.5, theme::TEXT_FAINT))
            .push(label(
                format!(
                    "{} · confidence (top-k) {} · repetition {} · {} token(s) · last gap {}",
                    num(p, "rate_tok_s", &|x| format!("{x:.1} tokens/s")),
                    num(p, "confidence_topk", &|x| format!("{:.0}%", x * 100.0)),
                    num(p, "repetition", &|x| format!("{:.0}%", x * 100.0)),
                    num(p, "tokens", &|x| format!("{x:.0}")),
                    num(p, "last_gap_s", &|x| format!("{x:.2} s")),
                ),
                12.0,
                theme::TEXT_DIM,
            ));
    } else {
        col = col.push(label("Tier 1 reads only while Sinai answers; it has not answered since this window connected.", 12.0, theme::TEXT_FAINT));
    }
    // the last turn
    col = col.push(label("Last turn · measured", 12.5, theme::TEXT));
    for line in machine::last_turn_lines(&c.last_turn) {
        col = col.push(mono(line, 11.5, theme::TEXT_DIM));
    }
    // tier 2
    let forward_on = c.memory.as_ref().is_some_and(|m| m.forward_on);
    col = col.push(
        row![
            label("Tier 2 · measured forward pass", 12.5, theme::TEXT).width(Length::Fill),
            button(text(if forward_on { "Stop sampling" } else { "Sample the next turn" }).size(12.0).font(fonts().ui))
                .padding([3.0, 8.0])
                .on_press(Message::MemoryAct(Act::Forward(!forward_on)))
                .style(theme::secondary_button),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    let f = &c.forward;
    if f.is_object() {
        col = col.push(label(f.get("status").and_then(|s| s.as_str()).unwrap_or("unmeasured"), 12.0, theme::TEXT_DIM));
        if let Some(reason) = f.get("reason").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
            col = col.push(label(reason, 11.5, theme::TEXT_FAINT));
        }
        match machine::kv_span(f) {
            Some((span, capacity, low, high)) => {
                col = col
                    .push(progress_bar(0.0..=1.0, span as f32 / capacity as f32).girth(6).style(bar_style))
                    .push(label(
                        if span == 0 {
                            format!("KV: sequence empty at this sample ({capacity} context capacity)")
                        } else {
                            format!("KV: {span} positions of {capacity} (positions {low} to {high}); cells, bytes and values unmeasured")
                        },
                        11.5,
                        theme::TEXT_FAINT,
                    ))
            }
            None => col = col.push(label("KV positions unavailable.", 11.5, theme::TEXT_FAINT)),
        }
        let readings = machine::layer_readings(f);
        for (layer, signal, rms, peak) in readings.iter().take(24) {
            col = col.push(mono(format!("L{layer:02} {signal:7} RMS {rms:.4} max {peak:.4}"), 11.0, theme::TEXT_DIM));
        }
        if readings.len() > 24 {
            col = col.push(label(format!("and {} more layer reading(s)", readings.len() - 24), 11.0, theme::TEXT_FAINT));
        }
    } else {
        col = col.push(label("Nothing sampled yet. Sampling reads the model's layers and KV positions on the next turn.", 12.0, theme::TEXT_FAINT));
    }
    // tier 3
    col = col.push(label("Tier 3 · derived landscape", 12.5, theme::TEXT));
    let l = &c.landscape;
    if l.is_object() {
        col = col
            .push(label(l.get("state").and_then(|s| s.as_str()).unwrap_or("unmeasured"), 12.0, theme::TEXT_DIM))
            .push(label(l.get("rule").and_then(|s| s.as_str()).unwrap_or(""), 11.5, theme::TEXT_FAINT));
        if let Some(n) = l.get("lexical_novelty").and_then(|x| x.as_f64()) {
            col = col.push(label(format!("Lexical novelty {:.0}%", n * 100.0), 12.0, theme::TEXT_DIM));
        }
        if let Some(topics) = l.get("topics").filter(|t| !t.is_null()) {
            col = col.push(label(format!("Topics: {}", ui::cut(&topics.to_string(), 200)), 11.5, theme::TEXT_FAINT));
        }
    } else {
        col = col.push(label("Nothing derived yet.", 12.0, theme::TEXT_FAINT));
    }
    col = col.push(label("Lexical heuristic; not a reading of private thoughts.", 11.5, theme::TEXT_FAINT));
    col.into()
}

/// Sinai's memory, as the loop's newest snapshot says, with the Angel window's acts (each confirmed as `machine::Act`
/// says: an erase once, a permanent delete twice).
fn memory(app: &App) -> El<'_> {
    use crate::machine::Act;
    let act = |words: String, a: Act| -> El<'_> {
        button(text(words).size(12.0).font(fonts().ui)).padding([3.0, 8.0]).on_press(Message::MemoryAct(a)).style(theme::secondary_button).into()
    };
    let mut c = Column::new().spacing(4).push(subheading("Its memory"));
    if let Some(p) = &app.memory_pending {
        let step = p.given + 1;
        let of = p.act.confirmations();
        c = c.push(
            container(
                column![
                    label(p.act.question(step), 12.5, theme::TEXT),
                    row![
                        primary(if step < of { "Yes, continue" } else { "Yes" }, Message::MemoryConfirm),
                        secondary("Cancel", Some(Message::MemoryCancel)),
                        label(if of > 1 { format!("step {step} of {of}") } else { String::new() }, 11.5, theme::TEXT_FAINT),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ]
                .spacing(6),
            )
            .padding(8)
            .style(theme::notice(theme::CAUTION)),
        );
    }
    if let Some(note) = &app.memory_note {
        c = c.push(label(note.as_str(), 12.0, theme::CAUTION));
    }
    let Some(m) = &app.conversation.memory else {
        let why = if app.sinai_up == Some(true) { "No snapshot yet, or the memory was just erased." } else { "Not known while the loop is not running." };
        return c.push(label(why, 12.5, theme::TEXT_FAINT)).into();
    };
    c = c.push(label(
        format!(
            "{} · {} conversation(s) kept",
            if m.on { "Remembering" } else { "Not remembering" },
            m.total.map(|n| n.to_string()).unwrap_or("?".into())
        ),
        12.5,
        theme::TEXT,
    ));
    c = c.push(
        Row::new()
            .spacing(4)
            .push(act(if m.on { "Stop remembering".into() } else { "Remember".into() }, Act::Remember(!m.on)))
            .push(act("Erase all…".into(), Act::Erase { id: None, what: String::new() }))
            .wrap(),
    );
    if !m.error.is_empty() {
        c = c.push(label(m.error.as_str(), 12.5, theme::CAUTION));
    }
    for e in &m.episodes {
        let mut marks = Row::new().spacing(4);
        for status in crate::machine::STATUSES {
            if status != e.status {
                marks = marks.push(act(status.replace('_', " "), Act::Mark { id: e.id.clone(), status: status.into() }));
            }
        }
        let what = format!("the conversation \"{}\"", ui::cut(&e.user, 60));
        marks = marks.push(act("To trash…".into(), Act::Erase { id: Some(e.id.clone()), what }));
        c = c.push(
            column![
                label(format!("{} · {}", e.status.replace('_', " "), e.created), 11.5, theme::TEXT_FAINT),
                label(format!("You: {}", ui::cut(&e.user, 140)), 12.5, theme::TEXT),
                label(format!("Sinai: {}", ui::cut(&e.assistant, 140)), 12.5, theme::TEXT_DIM),
                marks.wrap(),
            ]
            .spacing(2),
        );
    }
    if m.total.is_some_and(|t| t as usize > m.episodes.len()) {
        c = c.push(label(format!("the newest {} shown", m.episodes.len()), 12.0, theme::TEXT_FAINT));
    }
    c = c.push(subheading("Trash"));
    if m.trash.is_empty() {
        c = c.push(label("Empty.", 12.5, theme::TEXT_FAINT));
    }
    let n = |x: Option<u64>| x.map(|n| n.to_string()).unwrap_or("?".into());
    for t in &m.trash {
        let mut line = format!("{} · {} conversation(s), {} observation(s), {} record line(s)", t.when, n(t.episodes), n(t.views), n(t.record));
        if !t.glimpse.is_empty() {
            line = format!("{line} · {}", ui::cut(&t.glimpse, 80));
        }
        c = c.push(label(line, 12.0, theme::TEXT_DIM));
        c = c.push(
            Row::new()
                .spacing(4)
                .push(act("Restore".into(), Act::Restore { batch: t.batch.clone() }))
                .push(act("Delete permanently…".into(), Act::Purge { batch: Some(t.batch.clone()), what: format!("the erase of {}", t.when) })),
        );
    }
    if !m.trash.is_empty() {
        c = c.push(act("Empty trash…".into(), Act::Purge { batch: None, what: String::new() }));
    }
    c = c.push(subheading("What it has observed"));
    c = c.push(label(
        format!(
            "{} · {} working, {} saved{}",
            if m.visual_observing { "observing a window" } else { "not observing" },
            m.working_count,
            m.saved_count,
            if m.visual_saving { " (saving is on)" } else { "" }
        ),
        12.5,
        theme::TEXT,
    ));
    if !m.visual_status.is_empty() {
        c = c.push(label(m.visual_status.as_str(), 12.0, theme::TEXT_FAINT));
    }
    let mut visual = Row::new().spacing(4);
    visual = if m.visual_observing {
        visual.push(act("Stop observing".into(), Act::Observe(None)))
    } else if app.observe_windows.is_some() {
        visual.push(secondary("Close the list", Some(Message::ObserveChoose(false))))
    } else {
        visual.push(secondary("Observe a window…", Some(Message::ObserveChoose(true))))
    };
    visual = visual
        .push(act("Clear working views…".into(), Act::ForgetVisual))
        .push(act(if m.visual_saving { "Stop saving".into() } else { "Save observations".into() }, Act::SaveVisual(!m.visual_saving)));
    c = c.push(visual.wrap());
    if let Some(windows) = &app.observe_windows {
        c = c.push(label("Read this window while idle, at most once every 5 seconds:", 12.0, theme::TEXT_DIM));
        if windows.is_empty() {
            c = c.push(label("No window to offer.", 12.0, theme::TEXT_FAINT));
        }
        for w in windows {
            c = c.push(act(ui::cut(&w.title, 60).to_string(), Act::Observe(Some((w.handle, w.title.clone())))));
        }
    }
    for v in &m.working {
        c = c.push(label(format!("{} · {}: {}", v.title, v.created, ui::cut(&v.description, 120)), 12.0, theme::TEXT_DIM));
    }
    for v in &m.saved {
        c = c.push(
            Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(label(format!("{} · {}: {}", v.title, v.created, ui::cut(&v.description, 100)), 12.0, theme::TEXT_DIM).width(Length::Fill))
                .push(act("To trash…".into(), Act::Erase { id: Some(v.id.clone()), what: format!("the observation of {}", v.title) })),
        );
    }
    if !m.meaning.is_empty() {
        c = c.push(label(format!("Recall by meaning: {}", m.meaning), 12.0, theme::TEXT_DIM));
    }
    if !m.path.is_empty() {
        c = c.push(label(format!("Kept in {}", m.path), 11.5, theme::TEXT_FAINT));
    }
    c.into()
}

/// The Stage: pictures of the window Sinai's hands are armed on and of the windows the person pins, taken by this
/// window while the Stage is on show; a chooser to pin or unpin; then what of Sinai's is still to move in.
fn stage_view(app: &App) -> El<'_> {
    let sinais = app.sinai_window();
    let shown = app.stage_on_show();
    let choose = if app.stage_choosing {
        secondary("Done", Some(Message::StageChoose(false)))
    } else {
        secondary("Choose windows…", Some(Message::StageChoose(true)))
    };
    let mut list = Column::new().spacing(10).push(
        row![
            label(
                "The window Sinai's hands are armed on, and the windows you pin, pictured on this PC while the Stage is on show. Nothing is kept or sent.",
                12.5,
                theme::TEXT_DIM
            )
            .width(Length::Fill),
            choose
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if app.stage_choosing {
        if app.stage_windows.is_empty() {
            list = list.push(working(app, "Looking at the open windows"));
        }
        for w in &app.stage_windows {
            let pinned = app.stage_pinned.contains(&w.handle);
            list = list.push(
                button(row![label(if pinned { "Pinned" } else { "Pin" }, 12.5, if pinned { theme::GOLD } else { theme::TEXT_DIM }).width(56), label(w.title.as_str(), 13.0, theme::TEXT)].spacing(8))
                    .width(Length::Fill)
                    .padding([6.0, 10.0])
                    .on_press(Message::StagePin(w.handle))
                    .style(theme::list_row(pinned)),
            );
        }
    }
    if shown.is_empty() && !app.stage_choosing {
        list = list.push(label(
            "Nothing is on the Stage. It shows the window Sinai's hands are armed on, and any window you pin; nothing is captured until then.",
            13.0,
            theme::TEXT_FAINT,
        ));
    }
    let mut line = Row::new().spacing(10);
    for (i, handle) in shown.iter().enumerate() {
        let is_sinais = Some(*handle) == sinais;
        let title = app
            .stage_windows
            .iter()
            .find(|w| w.handle == *handle)
            .map(|w| w.title.clone())
            .or_else(|| app.conversation.hands.as_ref().filter(|_| is_sinais).map(|h| h.title.clone()))
            .unwrap_or_default();
        let picture: El<'_> = match app.stage_shots.get(handle) {
            Some((picture, coverage)) => {
                let shot = image(picture.clone()).content_fit(ContentFit::Contain).width(Length::Fill);
                if *coverage < stage::THIN {
                    column![shot, label("This window answered with a nearly empty frame.", 12.0, theme::CAUTION)].spacing(4).into()
                } else {
                    shot.into()
                }
            }
            None if app.stage_gone.contains(handle) => {
                label("This window is not drawing now: it may be minimized or closed.", 12.5, theme::TEXT_FAINT).into()
            }
            None => working(app, "Taking its first picture"),
        };
        let tag = if is_sinais { chip("Sinai's hands are here", theme::GOLD) } else { chip("pinned", theme::TEXT_DIM) };
        let tile = container(column![row![tag, label(title, 12.5, theme::TEXT).width(Length::Fill)].spacing(8).align_y(Alignment::Center), picture].spacing(6))
            .padding(8)
            .width(Length::FillPortion(1))
            .style(if is_sinais { theme::notice(theme::GOLD) } else { theme::notice(theme::TEXT_FAINT) });
        line = line.push(tile);
        if i % 2 == 1 {
            list = list.push(line);
            line = Row::new().spacing(10);
        }
    }
    if shown.len() % 2 == 1 {
        list = list.push(line.push(space().width(Length::FillPortion(1))));
    }
    list = list.push(subheading("Moving in"));
    for feature in Section::Sinai.features() {
        list = list.push(feature_row(feature));
    }
    list.into()
}

/// Sinai's loop: whether it is there, what Sinai is doing, and its microphone, with the button that opens or
/// closes it.
fn loop_card(app: &App) -> El<'_> {
    let c = &app.conversation;
    if !app.can(Capability::SinaiMind) || matches!(app.answering(), crate::models::Answering::Own(_)) {
        return own_card(app);
    }
    let body: El<'_> = match app.sinai_up {
        None => working(app, "Looking for Sinai's loop"),
        Some(false) => column![
            row![dot(theme::TEXT_FAINT), label("Sinai's loop is not running.", 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center),
            label("When Angel is running, the conversation, what Sinai holds for you and what it has done appear here.", 12.5, theme::TEXT_FAINT),
        ]
        .spacing(6)
        .into(),
        Some(true) => {
            let state: El<'_> = if c.thinking() {
                working(app, "Sinai is thinking")
            } else {
                let words = if c.state.is_empty() { "Sinai is here".to_string() } else { format!("Sinai: {}", c.state) };
                row![dot(theme::POSITIVE), label(words, 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center).into()
            };
            let mic: El<'_> = match (&c.mic.error, app.sinai_on_air()) {
                (Some(error), _) => label(format!("Sinai's microphone could not open: {error}"), 13.0, theme::CAUTION).into(),
                (None, Some(how)) => {
                    let left = c.mic.follow_up_s.filter(|_| c.mic.attending).map(|s| format!(" ({s:.0} s left)")).unwrap_or_default();
                    row![dot(theme::DANGER), label(format!("Sinai's microphone is on air: {how}{left}."), 13.0, theme::TEXT), space().width(Length::Fill), secondary("Close the microphone", Some(Message::SinaiListen))]
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .into()
                }
                (None, None) => row![dot(theme::TEXT_FAINT), label("Sinai's microphone is off.", 13.0, theme::TEXT_DIM), space().width(Length::Fill), secondary("Open the microphone", Some(Message::SinaiListen))]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
            };
            column![state, mic, label("Sinai acts only on speech addressed to it by name, or inside a conversation you started.", 12.0, theme::TEXT_FAINT)]
                .spacing(8)
                .into()
        }
    };
    // Sinai's mind answers; the person's own model is offered only as an explicit alternative, never in its place.
    let own = app.models.sinai();
    let label_of = own.as_ref().map(|p| app.models.catalog.as_ref().map(|c| c.label(p)).unwrap_or_else(|| p.key()));
    let alternative = row![
        label("Sinai's mind is answering.", 12.5, theme::TEXT_DIM),
        space().width(Length::Fill),
        secondary("Models…", Some(Message::Models(crate::models::Msg::Open))),
        match &label_of {
            Some(name) => button(label(format!("Talk to {name} instead"), 13.0, theme::TEXT))
                .padding([6.0, 12.0])
                .on_press(Message::Models(crate::models::Msg::Instead(true)))
                .style(theme::secondary_button)
                .into(),
            None => El::from(space().width(0)),
        },
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    container(column![body, alternative].spacing(10)).padding(16).width(Length::Fill).style(theme::panel).into()
}

/// The card above the conversation when the person's own model is the one answering (or none is, in a build with no
/// mind): which model, whether it is ready, and the way to the Models panel.
fn own_card(app: &App) -> El<'_> {
    let m = &app.models;
    let mind = app.can(Capability::SinaiMind);
    let mut col = Column::new().spacing(6);
    if !mind {
        col = col.push(
            row![dot(theme::TEXT_FAINT), label(crate::installed::SINAI_NOT_INSTALLED, 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center),
        );
    }
    let models = secondary("Models…", Some(Message::Models(crate::models::Msg::Open)));
    match (m.sinai(), &m.catalog) {
        (_, None) => col = col.push(working(app, "Looking for a model of your own")),
        (None, Some(_)) => {
            col = col.push(
                row![
                    label(
                        "To talk here, choose a model of your own: a GGUF file run on this PC, or an endpoint you use.",
                        12.5,
                        theme::TEXT_FAINT
                    )
                    .width(Length::Fill),
                    models
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
        (Some(pick), Some(catalog)) => {
            let ready = catalog.ready(&pick, &m.knocked);
            let colour = match ready {
                crate::models::Ready::Yes(_) => theme::POSITIVE,
                crate::models::Ready::Asking(_) => theme::TEXT_FAINT,
                crate::models::Ready::No(_) => theme::CAUTION,
            };
            let who = if mind {
                format!("Your model, {}, is answering, not Sinai's mind.", catalog.label(&pick))
            } else {
                format!("Your model, {}, is answering. It is not Sinai's mind.", catalog.label(&pick))
            };
            let mut top = row![dot(colour), strong(who, 13.5, theme::TEXT).width(Length::Fill), models].spacing(10).align_y(Alignment::Center);
            if mind {
                top = top.push(primary("Back to Sinai's mind", Message::Models(crate::models::Msg::Instead(false))));
            }
            col = col.push(top).push(label(ready.words().to_string(), 12.5, theme::TEXT_FAINT));
        }
    }
    container(col).padding(16).width(Length::Fill).style(theme::panel).into()
}

/// Sinai's tab: its face above, drawn by the graphics card only while it is on show, and the conversation below.
fn angel(app: &App) -> El<'_> {
    let face: El<'_> = if app.face_visible() {
        let shape = container(secondary("Appearance…", Some(Message::AppearanceOpen))).align_right(Length::Fill).padding(8);
        stack![crate::face::view(&app.face), shape].into()
    } else {
        space().into()
    };
    column![container(face).height(Length::FillPortion(1)), container(conversation(app)).height(Length::FillPortion(1))]
        .spacing(10)
        .height(Length::Fill)
        .into()
}

/// The conversation with the person's own model: its lines, the answer as it streams, and the box to type in.
fn own_conversation(app: &App) -> El<'_> {
    use crate::models::byo::{Msg as Own, Phase, Who};
    let s = &app.models.own;
    let say = |m: Own| Message::Models(crate::models::Msg::Own(m));
    let name = app.models.sinai().map(|p| app.models.catalog.as_ref().map(|c| c.label(&p)).unwrap_or_else(|| p.key()));
    let mut lines = Column::new().spacing(10);
    if s.lines.is_empty() && s.partial.is_empty() {
        lines = lines.push(label(
            match &name {
                Some(_) => "Type below: your model answers here, and Sinai's face speaks its answer.",
                None => "Choose a model of your own in Models… to talk here.",
            },
            13.5,
            theme::TEXT_FAINT,
        ));
    }
    let model_name = name.clone().unwrap_or_else(|| "Model".into());
    for (who, words) in &s.lines {
        let (who, color) = match who {
            Who::You => ("You".to_string(), theme::TEXT_DIM),
            Who::Model => (model_name.clone(), theme::GOLD),
        };
        // the name above the words: a model's name can be long, and the dock narrow
        lines = lines.push(column![strong(who, 12.5, color), label(words.as_str(), 14.5, theme::TEXT).width(Length::Fill)].spacing(2));
    }
    match &s.phase {
        Phase::Loading(model) => lines = lines.push(working(app, format!("Starting llama.cpp on this PC with {model} (a large model takes a while)"))),
        Phase::Thinking => lines = lines.push(working(app, format!("{model_name} is reading your message"))),
        Phase::Speaking => {
            lines = lines.push(
                column![strong(model_name.clone(), 12.5, theme::GOLD), label(s.partial.as_str(), 14.5, theme::TEXT).width(Length::Fill)].spacing(2),
            )
        }
        Phase::Idle | Phase::Listening => {}
    }
    if let Some(problem) = &s.problem {
        lines = lines.push(label(problem.as_str(), 13.0, theme::CAUTION));
    }
    let mut col = Column::new().spacing(10).height(Length::Fill).push(
        container(scrollable(container(lines).padding(16).width(Length::Fill)).anchor_bottom().style(theme::scrollbars).height(Length::Fill))
            .height(Length::Fill)
            .width(Length::Fill)
            .style(theme::well),
    );
    if let Some(host) = &s.confirm {
        col = col.push(
            container(
                row![
                    label(format!("What you type goes to {host}, off this PC. Send it?"), 13.0, theme::TEXT).width(Length::Fill),
                    primary("Send", say(Own::Confirm(true))),
                    secondary("Cancel", Some(say(Own::Confirm(false)))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([8.0, 12.0])
            .width(Length::Fill)
            .style(theme::notice(theme::GOLD)),
        );
    }
    let busy = s.phase.busy();
    let placeholder = match &name {
        Some(n) => format!("Type to {n}"),
        None => "Choose a model first (Models…)".to_string(),
    };
    let mut entry = row![text_input(&placeholder, &s.typed).on_input(move |t| say(Own::Typed(t))).on_submit(say(Own::Send)).padding(9).size(14).font(fonts().ui).style(input_style)]
        .spacing(10)
        .align_y(Alignment::Center);
    entry = if busy {
        entry.push(secondary("Stop", Some(say(Own::Stop))))
    } else if s.typed.trim().is_empty() || name.is_none() {
        entry.push(secondary("Send", None))
    } else {
        entry.push(primary("Send", say(Own::Send)))
    };
    col.push(entry).into()
}

fn conversation(app: &App) -> El<'_> {
    if !app.can(Capability::SinaiMind) || matches!(app.answering(), crate::models::Answering::Own(_)) {
        return own_conversation(app);
    }
    let c = &app.conversation;
    let mut lines = Column::new().spacing(10);
    if c.lines.is_empty() && c.hearing.is_empty() {
        let empty = if app.sinai_up == Some(true) {
            "Say \"Sinai\" with the microphone open, or type below."
        } else {
            "The conversation appears here when Sinai's loop is running."
        };
        lines = lines.push(label(empty, 13.5, theme::TEXT_FAINT));
    }
    for (who, words) in &c.lines {
        let (name, color) = if who == "you" { ("You", theme::TEXT_DIM) } else { ("Sinai", theme::GOLD) };
        lines = lines.push(row![container(strong(name, 12.5, color)).width(48), label(words.as_str(), 14.5, theme::TEXT).width(Length::Fill)].spacing(10));
    }
    if !c.hearing.is_empty() {
        lines = lines.push(row![container(spinner(app.phase, 12.0)).width(48), label(c.hearing.as_str(), 14.5, theme::TEXT_FAINT)].spacing(10));
    }
    // Dictating through the ears fills the box; nothing reaches Sinai until Send.
    let dictating = app.dictating_to_sinai && app.state.as_ref().is_some_and(|s| s.dictating) && app.connected();
    let speak: El<'_> = if dictating {
        row![dot(theme::DANGER), secondary("Stop dictating", Some(Message::DictateToSinai))].spacing(6).align_y(Alignment::Center).into()
    } else {
        secondary("Dictate", app.connected().then_some(Message::DictateToSinai))
    };
    let entry = row![
        text_input("Type to Sinai, or dictate", &app.typed).on_input(Message::Typed).on_submit(Message::SayTyped).padding(9).size(14).font(fonts().ui).style(input_style),
        speak,
        if app.typed.trim().is_empty() { secondary("Send", None) } else { primary("Send", Message::SayTyped) },
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let entry: El<'_> = if dictating && !app.dictation_partial.is_empty() {
        column![entry, row![spinner(app.phase, 12.0), label(app.dictation_partial.as_str(), 13.0, theme::TEXT_FAINT)].spacing(8).align_y(Alignment::Center)]
            .spacing(6)
            .into()
    } else {
        entry.into()
    };
    column![
        container(scrollable(container(lines).padding(16).width(Length::Fill)).anchor_bottom().style(theme::scrollbars).height(Length::Fill))
            .height(Length::Fill)
            .width(Length::Fill)
            .style(theme::well),
        entry,
    ]
    .spacing(10)
    .height(Length::Fill)
    .into()
}

/// Acts Sinai holds until a person answers, each with its own buttons and the seconds it still waits (counted from
/// when the loop sent them, since it does not resend them as time passes); then what became of acts lately decided.
fn waiting(app: &App) -> El<'_> {
    let c = &app.conversation;
    let now = std::time::Instant::now();
    let mut list = Column::new().spacing(10);
    if c.held.is_empty() {
        list = list.push(label("Nothing is waiting for your answer.", 13.0, theme::TEXT_FAINT));
    }
    for held in &c.held {
        let left = held.left(c.held_at, now);
        let about = if held.reason.is_empty() { held.what.clone() } else { format!("{} ({})", held.what, held.reason) };
        let mut card = Column::new().spacing(6).push(label(held.question.as_str(), 14.0, theme::TEXT)).push(label(about, 12.5, theme::TEXT_DIM));
        let changing = app.held_change.as_ref().filter(|(id, _, _)| *id == held.id);
        if left <= 0.0 {
            card = card.push(label("Sinai no longer waits for this: unanswered, the act was dropped.", 12.5, theme::TEXT_FAINT));
        } else if let Some((_, x, y)) = changing {
            let ready = sinai::click_change(held, x, y).is_some();
            let coordinate = |value: &str, on: fn(String) -> Message| -> El<'_> {
                text_input("", value).on_input(on).padding(7).size(13).width(90).font(fonts().mono).style(input_style).into()
            };
            card = card
                .push(label(
                    "Allow it at another point: Sinai clicks there instead, once it has checked the window is still the one it \
                     saw. Nothing else about the act changes.",
                    12.5,
                    theme::TEXT_DIM,
                ))
                .push(
                    row![
                        label("x", 13.0, theme::TEXT_DIM),
                        coordinate(x, Message::ChangeX),
                        label("y", 13.0, theme::TEXT_DIM),
                        coordinate(y, Message::ChangeY),
                        if ready { primary("Allow there", Message::AllowChanged) } else { secondary("Allow there", None) },
                        secondary("Cancel", Some(Message::ChangeCancel)),
                        space().width(Length::Fill),
                        label(format!("{left:.0} s left"), 12.0, theme::TEXT_FAINT),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
        } else {
            let mut buttons = row![primary("Allow", Message::Decide(held.id.clone(), true)), secondary("Refuse", Some(Message::Decide(held.id.clone(), false)))]
                .spacing(8)
                .align_y(Alignment::Center);
            // Only a click can be moved first: the loop carries out a changed x and y, and nothing else.
            if held.click_at().is_some() {
                buttons = buttons.push(secondary("Change…", Some(Message::ChangeHeld(held.id.clone()))));
            }
            card = card.push(buttons.push(space().width(Length::Fill)).push(label(format!("{left:.0} s left"), 12.0, theme::TEXT_FAINT)));
        }
        let edge = if left > 0.0 { theme::GOLD } else { theme::TEXT_FAINT };
        list = list.push(container(card).padding(12).width(Length::Fill).style(theme::notice(edge)));
    }
    let decided: Vec<&sinai::Judged> = c.judged.iter().rev().filter(|j| j.resolved).take(5).collect();
    if !decided.is_empty() {
        list = list.push(label("Decided lately", 12.5, theme::TEXT_DIM));
        for judged in decided {
            let color = if judged.outcome.contains("refused") {
                theme::DANGER
            } else if judged.outcome.starts_with("granted") {
                theme::POSITIVE
            } else {
                theme::TEXT_DIM
            };
            list = list.push(
                row![label(judged.what.as_str(), 12.5, theme::TEXT).width(Length::Fill), label(judged.outcome.as_str(), 12.5, color)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            );
        }
    }
    list.into()
}

/// What Sinai may do, as the loop reads the grants file, with any grant that failed to load. Read-only here:
/// the file is the person's to write (in Notepad, from the button), and Sinai only ever reads it.
fn grants(app: &App) -> El<'_> {
    let c = &app.conversation;
    let mut list = Column::new().spacing(8);
    let open: El<'_> = if c.grants_path.is_empty() {
        space().width(0).into()
    } else {
        row![secondary("Edit…", Some(Message::GrantsEdit)), secondary("Open the file", Some(Message::OpenGrants(c.grants_path.clone())))]
            .spacing(6)
            .into()
    };
    list = list.push(row![space().width(Length::Fill), open].align_y(Alignment::Center));
    if let Some(note) = &app.grants_note {
        list = list.push(label(note.as_str(), 12.5, theme::CAUTION));
    }
    if !c.standing_seen {
        // The loop says what is in force only when Sinai's hands are armed or disarmed: until then, it is unknown here.
        list = list.push(label(
            "Not known yet: Sinai's loop says what is in force when its hands are armed or disarmed, and has not said since \
             this window connected.",
            13.0,
            theme::TEXT_FAINT,
        ));
    } else if c.grants.is_empty() && c.grant_complaints.is_empty() {
        list = list.push(label("No standing grants: Sinai asks before every act that reaches a person.", 13.0, theme::TEXT_FAINT));
    }
    for grant in &c.grants {
        list = list.push(label(grant.as_str(), 13.0, theme::TEXT));
    }
    for complaint in &c.grant_complaints {
        list = list.push(label(format!("Not loaded, so not in force: {complaint}"), 13.0, theme::CAUTION));
    }
    if !c.grants_path.is_empty() {
        list = list.push(mono(c.grants_path.as_str(), 11.5, theme::TEXT_FAINT));
    }
    list.into()
}

/// What Sinai has done and is doing, newest first, by status and by kind of act; the acts waiting on you lead.
fn actions(app: &App) -> El<'_> {
    let c = &app.conversation;
    let mut list = Column::new().spacing(8);
    list = list.push(ui::tabs(&sinai::Status::ALL, app.journal_status, |s| s.title().to_string(), Message::JournalStatus));
    let kinds = c.kinds();
    if !kinds.is_empty() {
        // Every kind first, then each kind the journal holds, by its place in that list.
        let all: Vec<Option<usize>> = std::iter::once(None).chain((0..kinds.len()).map(Some)).collect();
        let chosen = app.journal_kind.as_ref().and_then(|k| kinds.iter().position(|kind| kind == k));
        list = list.push(ui::tabs(
            &all,
            chosen,
            |t| t.map_or_else(|| "Every kind".to_string(), |i| kinds[i].clone()),
            |t| Message::JournalKind(t.map(|i| kinds[i].clone())),
        ));
    }
    let every = app.journal_status == sinai::Status::All && app.journal_kind.is_none();
    if every {
        for held in &c.held {
            list = list.push(
                row![
                    mono("now", 11.5, theme::TEXT_FAINT),
                    label(held.what.as_str(), 13.0, theme::TEXT).width(Length::Fill),
                    chip("waiting on you", theme::GOLD)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
    }
    let shown: Vec<&sinai::Note> = c
        .journal
        .iter()
        .rev()
        .filter(|n| app.journal_status.shows(n) && app.journal_kind.as_deref().is_none_or(|k| n.kind == k))
        .take(60)
        .collect();
    if c.journal.is_empty() && (c.held.is_empty() || !every) {
        list = list.push(label("Sinai has not acted yet.", 13.0, theme::TEXT_FAINT));
    } else if shown.is_empty() && !c.journal.is_empty() {
        list = list.push(label("Nothing in the journal matches.", 13.0, theme::TEXT_FAINT));
    }
    for note in shown {
        let (tense, color) = match note.tense.as_str() {
            "done" if !note.allowed => ("not done", theme::DANGER),
            "done" => ("done", theme::POSITIVE),
            "doing" => ("doing", theme::GOLD),
            other => (other, theme::TEXT_DIM),
        };
        list = list.push(
            row![
                mono(note.wall.as_str(), 11.5, theme::TEXT_FAINT),
                mono(note.kind.as_str(), 11.5, theme::TEXT_FAINT),
                label(note.what.as_str(), 13.0, if note.allowed { theme::TEXT } else { theme::DANGER }).width(Length::Fill),
                chip(tense, color)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        // the loop's own words for what became of it ("carried out", "refused: ...", "failed: ...")
        if !note.outcome.is_empty() {
            list = list.push(label(note.outcome.as_str(), 12.0, if note.allowed { theme::TEXT_DIM } else { theme::DANGER }));
        }
        if !note.detail.is_empty() {
            list = list.push(label(note.detail.as_str(), 12.0, theme::TEXT_FAINT));
        }
    }
    list.into()
}

// ------------------------------------------------------------------- trust

fn trust_page(app: &App) -> El<'_> {
    let field = |placeholder: &'static str, value: &str, on: fn(String) -> Message| -> El<'_> {
        text_input(placeholder, value).on_input(on).padding(9).size(14).font(fonts().ui).style(input_style).into()
    };
    let go: El<'_> = if app.trust_checking {
        working(app, "Checking the receipt")
    } else if app.trust_receipt.trim().is_empty() {
        secondary("Verify", None)
    } else {
        primary("Verify", Message::TrustVerify)
    };
    let verify = container(
        column![
            subheading("Verify a receipt"),
            label("Drop a receipt, its data, or a test case on this window, or type the paths. The check runs on this PC with the project's own verifier.", 13.0, theme::TEXT_DIM),
            field("The receipt (.json)", &app.trust_receipt, Message::TrustReceipt),
            field("Its data (.json), when the receipt was computed from data", &app.trust_data, Message::TrustData),
            field("The public key you trust (64 hexadecimal characters), obtained apart from the receipt", &app.trust_key, Message::TrustKey),
            field("A witness key (optional)", &app.trust_witness, Message::TrustWitness),
            label("The receipt carries its own key; a key taken from it proves nothing about who signed it, so it is never used.", 12.0, theme::TEXT_FAINT),
            go,
        ]
        .spacing(10),
    )
    .padding(16)
    .width(Length::Fill)
    .style(theme::panel);
    let mut page = Column::new().spacing(20).push(heading("Trust", Section::Trust.purpose())).push(verify);
    match &app.trust_result {
        Some(Ok((verdict, stated))) => page = page.push(verdict_card(verdict, stated)),
        Some(Err(why)) => page = page.push(container(label(why.as_str(), 13.0, theme::TEXT)).padding(12).width(Length::Fill).style(theme::notice(theme::CAUTION))),
        None => {}
    }
    // the cards a build's plug-ins add (none in the public build)
    for (i, card) in app.trust_cards.iter().enumerate() {
        page = page.push(card.page.view(app.phase).map(move |m| Message::TrustCard(i, m)));
    }
    page = page.push(subheading("Moving in"));
    for feature in Section::Trust.features_with(&app.plugin_features) {
        page = page.push(feature_row(feature));
    }
    page.into()
}

/// What this PC checked, then what the receipt states: the verdict apart from the issuer's own declaration.
fn verdict_card<'a>(v: &'a trust::Verdict, s: &'a trust::Stated) -> El<'a> {
    let (headline, color, sub) = match (v.ok, v.refused) {
        (true, true) => ("A valid refusal", theme::POSITIVE, "The issuer refused, and the refusal is genuine. It is evidence of refusal, not a certified number."),
        (true, false) => ("Verified", theme::POSITIVE, "Every check this receipt needs passed against the key you pinned."),
        (false, _) => ("Not verified", theme::DANGER, "At least one check failed or could not be performed. The reasons are below."),
    };
    let card = Column::new()
        .spacing(10)
        .push(row![dot(color), strong(headline, 20.0, color)].spacing(10).align_y(Alignment::Center))
        .push(label(sub, 13.0, theme::TEXT_DIM));
    container(trust::verdict_body(card, v, s)).padding(16).width(Length::Fill).style(theme::panel).into()
}

// ------------------------------------------------------------------- account and platform

/// Account and platform: Settings (a panel over the window, from here or the account menu), then what is planned.
fn account_page(app: &App) -> El<'_> {
    let section = Section::Account;
    let open = button(label("Open Settings", 13.0, theme::TEXT))
        .padding([7.0, 14.0])
        .on_press(Message::SignIn(crate::signin::Msg::Settings))
        .style(theme::ghost_button);
    let mut page = Column::new()
        .spacing(14)
        .push(heading("Settings", section.purpose()))
        .push(
            row![
                label("Settings open as a panel over whichever page you are on, from here or the account menu.", 13.0, theme::TEXT_DIM),
                space().width(Length::Fill),
                open,
            ]
            .align_y(Alignment::Center),
        )
        .push(space().height(8));
    // friends are hosted: without them (signed out, or a build that does not sign in) the page says how they are had
    if !app.can(Capability::Friends) {
        page = page.push(ui::absent(crate::capability::absent(Capability::Friends))).push(space().height(8));
    }
    page = page
        .push(subheading("Planned for this section"))
        .push(label("Each says where it is today, and the step of the plan it arrives in.", 13.0, theme::TEXT_FAINT));
    for feature in section.features() {
        page = page.push(feature_row(feature));
    }
    page.into()
}

/// A page whose part this build, or this person, does not have: its title and purpose, and the empty state saying why.
fn absent_page<'a>(section: Section, title: &'a str, capability: Capability) -> El<'a> {
    Column::new().spacing(18).push(heading(title, section.purpose())).push(ui::absent(crate::capability::absent(capability))).into()
}

// ------------------------------------------------------------------- planned features

fn feature_row<'a>(feature: &'a Feature) -> El<'a> {
    let mut chips = Row::new().spacing(6);
    chips = chips.push(match feature.here() {
        Here::Now => chip("Here now", theme::POSITIVE),
        Here::InPart => chip("Here in part", theme::GOLD),
        Here::Later if feature.step == "Later" => chip("Later", theme::TEXT_FAINT),
        Here::Later => chip(format!("Step {}", feature.step), theme::GOLD_DIM),
    });
    if feature.no_screen && feature.here() == Here::Later {
        chips = chips.push(chip("No screen today", theme::CAUTION));
    }
    container(
        column![
            row![strong(feature.name, 14.5, theme::TEXT), space().width(Length::Fill), chips].align_y(Alignment::Center),
            label(feature.what, 13.0, theme::TEXT_DIM),
            label(format!("Today: {}. Plan: {}.", feature.today, feature.plan), 12.0, theme::TEXT_FAINT),
        ]
        .spacing(5),
    )
    .padding([12.0, 16.0])
    .width(Length::Fill)
    .style(theme::panel)
    .into()
}
