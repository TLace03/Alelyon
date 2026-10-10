//! Settings: a panel over the window (2026-10-08; similar to Claude Code's, a panel and not a separate tab),
//! opened from the account menu, so the page behind it stays on show and live. A list of its parts on the left with a search above it, the part chosen on the right; Esc, a click beside
//! the panel or its close button closes it.
//!
//! Only what works is here: who is signed in and their account pages; whether this PC keeps a session, and
//! forgetting it, and which sign-in service Alelyon uses and what it says now (its state, ways of signing in,
//! notices); where web pages open (Lattice's own browser or yours, `web`); appearance (whether the sign-in background
//! moves, the same switch as the sign-in screen's Motion button); and about Alelyon (the version, What's New, the
//! Terms and the Privacy Notice). Nothing on it is a switch that does nothing yet. Each web page's link has a small
//! button for your own browser, this once.

use iced::widget::{Column, Space, button, column, container, row, scrollable, text_input};
use iced::{Alignment, Length};
use crate::theme;

use super::web::WebPages;
use super::{El, Gate, Msg, State, client, words, yours};

const TERMS_URL: &str = "https://www.alelyon.com/legal/terms/";
const PRIVACY_URL: &str = "https://www.alelyon.com/legal/privacy/";

/// The panel's size, when the window has room for it.
const WIDTH: f32 = 940.0;
const HEIGHT: f32 = 640.0;

/// A part of the Settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Account,
    Security,
    WebPages,
    Appearance,
    About,
}

impl Pane {
    pub const ALL: [Pane; 5] = [Pane::Account, Pane::Security, Pane::WebPages, Pane::Appearance, Pane::About];

    pub fn title(self) -> &'static str {
        match self {
            Pane::Account => "Account",
            Pane::Security => "Sign-in & security",
            Pane::WebPages => "Web pages",
            Pane::Appearance => "Appearance",
            Pane::About => "About",
        }
    }

    /// One line under the part's title.
    pub fn description(self) -> &'static str {
        match self {
            Pane::Account => "Who is signed in, and your account's pages.",
            Pane::Security => "Staying signed in on this PC, and the sign-in service Alelyon uses.",
            Pane::WebPages => "Where the account pages, the Terms and the Privacy Notice open.",
            Pane::Appearance => "How the sign-in screen looks.",
            Pane::About => "This version of Alelyon, what is new, and its terms.",
        }
    }

    /// Every word the part's settings say about themselves, for the search.
    fn says(self) -> &'static str {
        match self {
            Pane::Account => "account signed in sign out name email ways to sign in account details account security offline",
            Pane::Security => {
                "stay signed in kept session forget session sealed windows user sign-in service address state notice \
                 providers security password"
            }
            Pane::WebPages => "web pages open links browser lattice's browser your browser agent default browser",
            Pane::Appearance => "appearance motion background fog animation still sign-in screen theme",
            Pane::About => "about version what's new terms of service privacy notice legal",
        }
    }

    /// Whether the part answers `query` (every word of it, in any case, in its title, its description or what its
    /// settings say); an empty query matches every part.
    pub fn matches(self, query: &str) -> bool {
        let said = format!("{} {} {}", self.title(), self.description(), self.says()).to_lowercase();
        query.split_whitespace().all(|word| said.contains(&word.to_lowercase()))
    }
}

/// What the sign-in service is, in words: (headline, colour).
pub fn service_state(s: &State) -> (String, iced::Color) {
    if s.client.is_none() {
        return ("Switched off on this PC (ALELYON_IDENTITY_URL=off)".into(), theme::TEXT_FAINT);
    }
    match (&s.status, &s.status_error) {
        (_, Some(why)) => (why.clone(), theme::CAUTION),
        (Some(st), None) if s.notice().is_some() => (format!("Up, with a notice ({})", st.service), theme::CAUTION),
        (Some(st), None) if st.service == "up" => ("Up".into(), theme::POSITIVE),
        (Some(st), None) => (format!("Answering ({})", st.service), theme::CAUTION),
        (None, None) => ("Asking…".into(), theme::TEXT_DIM),
    }
}

/// Where the address came from: the setting, or the default (the deployed service).
fn address() -> String {
    let set = std::env::var("ALELYON_IDENTITY_URL").ok().filter(|v| !v.trim().is_empty()).is_some();
    match client::base_url() {
        Some(url) if set => format!("{url} (set by ALELYON_IDENTITY_URL)"),
        Some(url) => format!("{url} (the default)"),
        None => "none".into(),
    }
}

/// Whether a session is kept where this window keeps it (`State::session_file`).
pub fn kept_on_this_pc(s: &State) -> bool {
    s.session_file.as_ref().is_some_and(|p| p.exists())
}

fn group<'a>(title: &'a str, body: El<'a>) -> El<'a> {
    container(column![crate::ui::subheading(title), body].spacing(10))
        .padding([14.0, 16.0])
        .width(Length::Fill)
        .style(theme::panel)
        .into()
}

fn line<'a>(name: &'a str, value: impl Into<String>, color: iced::Color) -> El<'a> {
    row![container(words(name, 13.0, theme::TEXT_DIM)).width(180), words(value.into(), 13.0, color)]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
}

fn action<'a>(label: &'a str, msg: Option<Msg>) -> El<'a> {
    button(words(label, 12.5, theme::TEXT)).padding([6.0, 12.0]).on_press_maybe(msg).style(theme::ghost_button).into()
}

fn account<'a>(s: &'a State) -> El<'a> {
    let urls = s.status.clone().unwrap_or_default();
    let open = |u: &str| (!u.is_empty()).then(|| Msg::Open(u.to_string()));
    match &s.gate {
        Gate::SignedIn(session) => {
            let a = &session.account;
            let ways = if a.providers.is_empty() { "—".to_string() } else { a.providers.join(", ") };
            column![
                line("Signed in as", a.name().to_string(), theme::TEXT),
                line("Email", if a.email.is_empty() { "none on the account".to_string() } else { a.email.clone() }, theme::TEXT),
                line("Ways to sign in", ways, theme::TEXT),
                row![
                    action("Account details ↗", open(&urls.account_url)),
                    yours(&urls.account_url),
                    action("Account security ↗", open(&urls.security_url)),
                    yours(&urls.security_url),
                    Space::new().width(Length::Fill),
                    action("Sign out", Some(Msg::SignOut)),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            ]
            .spacing(8)
            .into()
        }
        _ => column![
            line("Signed in as", "nobody: Alelyon is offline", theme::TEXT_DIM),
            words("Sinai, transcription, the fleet and your data work without an account.", 12.5, theme::TEXT_FAINT),
            row![action("Sign in", Some(Msg::SignInFromOffline))],
        ]
        .spacing(8)
        .into(),
    }
}

fn this_pc<'a>(s: &'a State) -> El<'a> {
    let kept = kept_on_this_pc(s);
    let mut this_pc = column![line(
        "Stay signed in",
        if kept {
            "A session is kept on this PC, sealed to your Windows user"
        } else {
            "No session is kept: you sign in each time Alelyon starts"
        },
        theme::TEXT,
    )]
    .spacing(8);
    if kept {
        this_pc = this_pc.push(row![action("Forget the kept session", Some(Msg::ForgetKept))]).push(words(
            "Forgetting it does not sign you out now; the next start asks you to sign in.",
            12.0,
            theme::TEXT_FAINT,
        ));
    }
    this_pc.into()
}

fn service<'a>(s: &'a State) -> El<'a> {
    let (state_words, state_color) = service_state(s);
    let on: Vec<&str> = s.status.iter().flat_map(|st| st.providers.iter().filter(|p| p.enabled).map(|p| p.name.as_str())).collect();
    let mut service = column![
        line("Address", address(), theme::TEXT),
        line("State", state_words, state_color),
        line(
            "Ways to sign in",
            match &s.status {
                None => "—".to_string(),
                Some(_) if on.is_empty() => "none switched on yet".to_string(),
                Some(_) => on.join(", "),
            },
            theme::TEXT,
        ),
    ]
    .spacing(8);
    if let Some(st) = &s.status {
        for n in &st.notices {
            let when = match (n.starts_at.is_empty(), n.ends_at.is_empty()) {
                (false, false) => format!(" ({} to {})", n.starts_at, n.ends_at),
                (false, true) => format!(" (from {})", n.starts_at),
                _ => String::new(),
            };
            service = service.push(line("Notice", format!("{}{when}", n.title), theme::CAUTION));
        }
    }
    // No Check again: the status is watched while this panel shows it, and a change is on show when it comes.
    if s.client.is_some() {
        service = service.push(crate::live::badge(&s.status_fresh));
    }
    service.into()
}

/// Where web pages open: Lattice's own browser (each in a tab of its own) or yours, kept on this PC (`web`).
fn web_pages<'a>(s: &'a State) -> El<'a> {
    let choice = |label: &'a str, pages: WebPages| -> El<'a> {
        button(words(label, 12.5, theme::TEXT))
            .padding([6.0, 12.0])
            .on_press(Msg::WebPages(pages))
            .style(theme::segment_button(s.web == pages))
            .into()
    };
    let mut col = column![
        row![
            container(words("Open web pages in", 13.0, theme::TEXT_DIM)).width(180),
            row![choice("Lattice's browser", WebPages::Lattice), choice("Your browser", WebPages::Yours)].spacing(4),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        words(
            "The account pages, the Terms and the Privacy Notice; the \u{21D7} beside a link opens it in your own browser, \
             once. Lattice's browser is the one its agent uses when you switch that on, so what you sign in to there, the \
             agent can reach. Signing in to Alelyon with a provider always uses your own browser.",
            12.0,
            theme::TEXT_FAINT,
        ),
    ]
    .spacing(6);
    if let Some(why) = &s.web_said {
        col = col.push(words(why.as_str(), 12.0, theme::CAUTION));
    }
    col.into()
}

fn appearance<'a>(s: &'a State) -> El<'a> {
    let m = &s.motion;
    let mut appearance = column![
        row![
            line(
                "Sign-in background",
                match (m.on, m.forced_still) {
                    (_, true) => "Still (kept still by CENTCOM_STILL_BACKDROP)",
                    (true, false) => "Animated: the fog rolls and your cursor stirs it",
                    (false, false) => "Still: one frame, no animation",
                },
                theme::TEXT,
            ),
            Space::new().width(Length::Fill),
            action(if m.on { "Turn motion off" } else { "Turn motion on" }, (!m.forced_still).then_some(Msg::Motion(!m.on))),
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(8);
    if let Some(note) = &m.note {
        appearance = appearance.push(words(note.as_str(), 12.0, theme::CAUTION));
    }
    appearance.into()
}

fn about<'a>() -> El<'a> {
    column![
        line("Version", env!("CARGO_PKG_VERSION"), theme::TEXT),
        row![
            action("What's New", Some(Msg::WhatsNew(true))),
            action("Terms of Service ↗", Some(Msg::Open(TERMS_URL.into()))),
            yours(TERMS_URL),
            action("Privacy Notice ↗", Some(Msg::Open(PRIVACY_URL.into()))),
            yours(PRIVACY_URL),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    ]
    .spacing(8)
    .into()
}

/// One part's groups.
fn pane<'a>(s: &'a State, pane: Pane) -> El<'a> {
    match pane {
        Pane::Account => group("Account", account(s)),
        Pane::Security => column![group("This PC", this_pc(s)), group("Sign-in service", service(s))].spacing(12).into(),
        Pane::WebPages => group("Web pages", web_pages(s)),
        Pane::Appearance => group("Appearance", appearance(s)),
        Pane::About => group("About Alelyon", about()),
    }
}

/// The parts the search finds now, in the list's order.
pub fn found(query: &str) -> Vec<Pane> {
    Pane::ALL.into_iter().filter(|p| p.matches(query)).collect()
}

/// The panel: the list of parts with the search above it on the left, and on the right the part chosen, or, while
/// something is typed in the search, every part it finds.
pub fn panel<'a>(s: &'a State) -> El<'a> {
    let chosen = s.settings.unwrap_or(Pane::Account);
    let query = s.settings_query.trim();
    let shown = found(query);
    let mut nav = Column::new().spacing(2).width(220);
    nav = nav.push(words("Settings", 18.0, theme::TEXT));
    nav = nav.push(Space::new().height(8));
    nav = nav.push(
        text_input("Search settings", &s.settings_query)
            .on_input(Msg::SettingsQuery)
            .padding([7.0, 10.0])
            .size(13.0),
    );
    nav = nav.push(Space::new().height(8));
    let query_empty = query.is_empty();
    for p in Pane::ALL {
        // While searching, a part with nothing found is dimmed and still opens.
        let lit = query.is_empty() || shown.contains(&p);
        let color = if lit { theme::TEXT } else { theme::TEXT_FAINT };
        nav = nav.push(
            button(words(p.title(), 13.5, color))
                .width(Length::Fill)
                .padding([7.0, 10.0])
                .on_press(Msg::SettingsPane(p))
                .style(move |th: &iced::Theme, status| {
                    if query_empty && p == chosen { theme::segment_button(true)(th, status) } else { theme::ghost_button(th, status) }
                }),
        );
    }
    let mut right = Column::new().spacing(14).padding([4.0, 8.0]);
    if query.is_empty() {
        right = right
            .push(column![words(chosen.title(), 20.0, theme::TEXT), words(chosen.description(), 13.0, theme::TEXT_DIM)].spacing(4))
            .push(pane(s, chosen));
    } else if shown.is_empty() {
        right = right.push(words(format!("Nothing in Settings matches \u{201C}{query}\u{201D}."), 13.5, theme::TEXT_DIM));
    } else {
        for p in shown {
            right = right
                .push(column![words(p.title(), 17.0, theme::TEXT), words(p.description(), 12.5, theme::TEXT_DIM)].spacing(3))
                .push(pane(s, p));
        }
    }
    let close = button(words("\u{2715}", 14.0, theme::TEXT_DIM)).padding([4.0, 10.0]).on_press(Msg::SettingsClose).style(theme::ghost_button);
    let body = row![
        nav,
        container(Space::new().width(1)).height(Length::Fill).style(theme::line),
        column![
            row![Space::new().width(Length::Fill), close],
            scrollable(right).style(theme::scrollbars).height(Length::Fill),
        ]
        .width(Length::Fill),
    ]
    .spacing(16);
    container(body)
        .padding(18)
        .width(Length::Fixed(WIDTH))
        .height(Length::Fixed(HEIGHT))
        .style(theme::panel)
        .into()
}
