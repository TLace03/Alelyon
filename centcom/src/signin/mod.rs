//! Signing in to Alelyon: the screen before the main window, after the Riot Client's (2026-10-07): the form down the left (a Sign-in tab with email or username, password, the ways
//! to sign in with another account, Stay signed in and the arrow; a QR Code tab), the backdrop on the right (the mark is the
//! small one above the form; the large one was taken out, 2026-10-08) with the service's notices (a scheduled
//! maintenance first), and Use offline for everything that does not need an account.
//!
//! The decisions of 2026-10-07: our own identity service (`/identity`, the contract in this crate's README);
//! providers GitHub, Google, Microsoft, Apple, ORCID, Hugging Face and LinkedIn; signing in is required, with Use
//! offline; QR sign-in now. The service runs at `client::DEFAULT_URL` (deployed 2026-10-07);
//! `ALELYON_IDENTITY_URL` names another, or `off` for none, when the screen says so and Use offline is the way in.
//!
//! What is kept: with Stay signed in, the refresh token, sealed for this Windows user (`vault`); without it, nothing
//! outlives the window. A password is sent once, over TLS, and never kept.

pub mod client;
// Opens the loopback listener and the browser, and nothing else; no unsafe code.
pub mod loopback;
pub mod qr;
// Settings: the account menu's page (drawn at the top of Account and platform).
pub mod settings;
// Seals the kept session with Windows' DPAPI, so it may use unsafe code.
#[allow(unsafe_code)]
pub mod vault;
// Where the panel's web pages open: Lattice's own browser or the person's own (Settings), and the latter's checks.
pub mod web;

use std::time::Duration;

use iced::widget::{
    Space, button, canvas, checkbox, column, container, image, row, scrollable, stack, text, text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Subscription, Task};

use crate::theme::{self, fonts};

use client::{Client, Failure, Pairing, Session, Status};

/// The seven ways to sign in with another account, in the order shown: (id, name, tile colour, text colour).
pub const PROVIDERS: [(&str, &str, Color, Color); 7] = [
    ("github", "GitHub", Color::from_rgb8(0x24, 0x29, 0x2f), Color::WHITE),
    ("google", "Google", Color::WHITE, Color::from_rgb8(0x1f, 0x1f, 0x1f)),
    ("microsoft", "Microsoft", Color::from_rgb8(0x2f, 0x2f, 0x2f), Color::WHITE),
    ("apple", "Apple", Color::BLACK, Color::WHITE),
    ("orcid", "ORCID", Color::from_rgb8(0xa6, 0xce, 0x39), Color::from_rgb8(0x1f, 0x1f, 0x1f)),
    ("huggingface", "Hugging Face", Color::from_rgb8(0xff, 0xd2, 0x1e), Color::from_rgb8(0x1f, 0x1f, 0x1f)),
    ("linkedin", "LinkedIn", Color::from_rgb8(0x0a, 0x66, 0xc2), Color::WHITE),
];

/// How long a provider's browser sign-in is waited for.
const BROWSER_WAIT: Duration = Duration::from_secs(300);

/// Where the person stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    /// A kept session is being renewed.
    Checking,
    SignedOut,
    SignedIn(Session),
    /// Using Alelyon without an account: the local features only.
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Password,
    Qr,
}

pub struct State {
    pub gate: Gate,
    client: Option<Client>,
    pub status: Option<Status>,
    pub tab: Tab,
    pub login: String,
    pub password: String,
    pub stay: bool,
    /// Where Stay signed in keeps the session: `vault::path()` at start, None for nowhere (a test build, or no home
    /// folder). Every keep, forget and look goes here, so a test's scratch file stands in for the person's own.
    pub session_file: Option<std::path::PathBuf>,
    /// What is under way, in words; None when nothing is.
    pub busy: Option<&'static str>,
    pub error: Option<String>,
    pub pairing: Option<(Pairing, Vec<bool>, usize)>,
    /// The maintenance notice opened to its full text.
    pub notice_open: bool,
    /// The account menu over the main window.
    pub menu_open: bool,
    /// The What's New list over the main window.
    pub whats_new_open: bool,
    /// The Settings panel over the main window, and the part of it shown (`settings::Pane`); `None` when closed.
    pub settings: Option<settings::Pane>,
    /// What is typed in the Settings panel's search.
    pub settings_query: String,
    /// Why the service's status could not be read, when it could not.
    pub status_error: Option<String>,
    /// When the service's status last changed, as its watch said (`status_watch`).
    pub status_fresh: crate::live::Freshness,
    /// A provider whose email already has an account: its ticket rides on the next password sign-in, which links it.
    pub link_ticket: Option<String>,
    /// Where the panel's web pages open (Settings; kept on this PC). app.rs sends a link to Lattice's browser itself.
    pub web: web::WebPages,
    /// Where that choice is kept (preferences.json); None without a home folder, when it lasts until Alelyon closes.
    pub web_file: Option<std::path::PathBuf>,
    /// Why the last change of that choice was not kept, when it was not.
    pub web_said: Option<String>,
    /// Whether the backdrop moves (the sign-in screen's Motion button and Settings' Appearance line).
    pub motion: crate::backdrop::Motion,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Status(Result<Status, Failure>),
    Resumed(Result<Session, Failure>),
    Tab(Tab),
    Login(String),
    Password(String),
    Stay(bool),
    Submit,
    Provider(&'static str),
    Done(Result<Session, Failure>),
    Paired(Result<Pairing, Failure>),
    NewCode,
    Poll,
    Polled(Result<Option<Session>, Failure>),
    UseOffline,
    SignInFromOffline,
    /// A link's web page, where Settings says (app.rs takes it to Lattice's browser when that is the choice).
    Open(String),
    /// A link's small button: the web page in the person's own browser, this once.
    OpenYours(String),
    /// Settings: where web pages open, kept on this PC.
    WebPages(web::WebPages),
    Notice(bool),
    Menu(bool),
    SignOut,
    /// The menu's Settings: the Settings panel opens over the page, at its Account part.
    Settings,
    /// The Settings panel: show one of its parts, search it, or close it (Esc, a click beside it, its close button).
    SettingsPane(settings::Pane),
    SettingsQuery(String),
    SettingsClose,
    WhatsNew(bool),
    /// Settings: forget the session kept on this PC (the one signed in now carries on).
    ForgetKept,
    /// The menu's Exit: the app closes (app.rs acts on it).
    Exit,
    /// The backdrop's motion on or off (the sign-in screen's corner button, or Settings), kept in preferences.json.
    Motion(bool),
}

impl State {
    /// The gate at start. `gate` false (a `--screenshot` run of another page) opens straight onto Alelyon, offline.
    pub fn boot(gate: bool) -> (State, Task<Msg>) {
        State::boot_at(gate, vault::path())
    }

    /// [`State::boot`], with the kept session's file given: `vault::path()` there, a test's scratch file here.
    fn boot_at(gate: bool, session_file: Option<std::path::PathBuf>) -> (State, Task<Msg>) {
        let client = client::base_url().and_then(|u| Client::new(u).ok());
        let kept = session_file.as_deref().and_then(vault::open);
        let mut state = State {
            gate: if !gate {
                Gate::Offline
            } else if kept.is_some() && client.is_some() {
                Gate::Checking
            } else {
                Gate::SignedOut
            },
            client,
            status: None,
            tab: Tab::Password,
            login: String::new(),
            password: String::new(),
            stay: kept.is_some(),
            session_file,
            busy: None,
            error: None,
            pairing: None,
            notice_open: false,
            menu_open: false,
            whats_new_open: false,
            settings: None,
            settings_query: String::new(),
            status_error: None,
            status_fresh: crate::live::Freshness::waiting(),
            link_ticket: None,
            web_file: crate::prefs::path(),
            web: crate::prefs::path().map(|file| web::load(&file)).unwrap_or_default(),
            web_said: None,
            motion: crate::backdrop::Motion::at_start(),
        };
        let mut tasks = Vec::new();
        if let Some(c) = state.client.clone() {
            tasks.push(Task::perform(async move { c.status().await }, Msg::Status));
            if let (Gate::Checking, Some(token)) = (&state.gate, kept) {
                let c = state.client.clone().expect("client");
                state.busy = Some("Signing you in");
                tasks.push(Task::perform(async move { c.refresh(&token).await }, Msg::Resumed));
            }
        }
        (state, Task::batch(tasks))
    }

    /// The main window is open: signed in, or offline.
    pub fn through(&self) -> bool {
        matches!(self.gate, Gate::SignedIn(_) | Gate::Offline)
    }

    /// The service and the session's token, while signed in (the friends panel calls the service with them).
    pub fn session(&self) -> Option<(Client, String)> {
        match (&self.gate, &self.client) {
            (Gate::SignedIn(s), Some(c)) => Some((c.clone(), s.refresh_token.clone())),
            _ => None,
        }
    }

    pub fn account(&self) -> Option<&client::Account> {
        match &self.gate {
            Gate::SignedIn(s) => Some(&s.account),
            _ => None,
        }
    }

    /// The service's notice to show now: a maintenance first.
    pub fn notice(&self) -> Option<&client::Notice> {
        let st = self.status.as_ref()?;
        st.notices.iter().find(|n| n.kind == "maintenance").or(st.notices.first())
    }

    fn signed_in(&mut self, session: Session) {
        if let Some(p) = &self.session_file {
            if self.stay {
                if let Err(why) = vault::keep(p, &session.refresh_token) {
                    self.error = Some(format!("You are signed in, but {why}."));
                }
            } else {
                vault::forget(p);
            }
        }
        self.password.clear();
        self.busy = None;
        self.pairing = None;
        self.gate = Gate::SignedIn(session);
    }

    fn failed(&mut self, f: Failure) {
        self.busy = None;
        self.error = Some(f.words());
    }

    fn start_pairing(&mut self) -> Task<Msg> {
        let Some(c) = self.client.clone() else { return Task::none() };
        self.pairing = None;
        self.error = None;
        let stay = self.stay;
        Task::perform(async move { c.pair_start(stay).await }, Msg::Paired)
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Status(Ok(s)) => {
                self.status = Some(s);
                self.status_error = None;
                self.status_fresh.heard(crate::utc::now(), None);
            }
            Msg::Status(Err(f)) => {
                self.status_fresh.heard(crate::utc::now(), Some(f.words()));
                self.status_error = Some(f.words());
            }
            Msg::ForgetKept => {
                if let Some(p) = &self.session_file {
                    vault::forget(p);
                }
                self.stay = false;
            }
            Msg::Resumed(Ok(session)) => self.signed_in(session),
            Msg::Resumed(Err(f)) => {
                self.busy = None;
                self.gate = Gate::SignedOut;
                // a refused token is forgotten; an unreachable service keeps it for the next start
                if matches!(f, Failure::Expired) {
                    if let Some(p) = &self.session_file {
                        vault::forget(p);
                    }
                }
                self.error = Some(f.words());
            }
            Msg::Tab(tab) => {
                self.tab = tab;
                self.error = None;
                if tab == Tab::Qr && self.pairing.is_none() {
                    return self.start_pairing();
                }
            }
            Msg::Login(s) => self.login = s,
            Msg::Password(s) => self.password = s,
            Msg::Stay(on) => self.stay = on,
            Msg::Submit => {
                let (Some(c), false) = (self.client.clone(), self.busy.is_some()) else { return Task::none() };
                if self.login.trim().is_empty() || self.password.is_empty() {
                    return Task::none();
                }
                self.busy = Some("Signing in");
                self.error = None;
                let (login, password, stay, ticket) =
                    (self.login.trim().to_string(), self.password.clone(), self.stay, self.link_ticket.clone());
                return Task::perform(async move { c.sign_in(&login, &password, stay, ticket.as_deref()).await }, Msg::Done);
            }
            Msg::Provider(id) => {
                let (Some(c), false) = (self.client.clone(), self.busy.is_some()) else { return Task::none() };
                self.busy = Some("Finish signing in in your browser");
                self.error = None;
                let stay = self.stay;
                return Task::perform(
                    async move {
                        let pkce = loopback::Pkce::new().map_err(Failure::Other)?;
                        let listener = loopback::Listener::bind().map_err(Failure::Other)?;
                        let redirect = listener.redirect_uri.clone();
                        loopback::open_in_browser(&c.provider_start_url(id, &redirect, &pkce.state, &pkce.challenge, stay))
                            .map_err(Failure::Other)?;
                        let state = pkce.state.clone();
                        let answer = tokio::task::spawn_blocking(move || listener.wait(&state, BROWSER_WAIT))
                            .await
                            .map_err(|_| Failure::Other("the wait for the browser stopped".into()))?
                            .map_err(Failure::Other)?;
                        match answer {
                            loopback::Answer::Code(code) => c.exchange(&redirect, &code, &pkce.verifier).await,
                            loopback::Answer::Link(ticket) => Err(Failure::LinkNeeded(ticket)),
                        }
                    },
                    Msg::Done,
                );
            }
            Msg::Done(Ok(session)) => {
                self.link_ticket = None;
                self.signed_in(session)
            }
            Msg::Done(Err(Failure::LinkNeeded(ticket))) => {
                self.busy = None;
                self.error = Some(Failure::LinkNeeded(String::new()).words());
                self.link_ticket = Some(ticket);
                self.tab = Tab::Password;
            }
            Msg::Done(Err(f)) => self.failed(f),
            Msg::Paired(Ok(p)) => match qr::modules(&p.approve_url) {
                Some((dark, width)) => self.pairing = Some((p, dark, width)),
                None => self.error = Some("The pairing address does not fit a QR code.".into()),
            },
            Msg::Paired(Err(f)) => self.failed(f),
            Msg::NewCode => return self.start_pairing(),
            Msg::Poll => {
                if let (Some(c), Some((p, ..)), Gate::SignedOut) = (self.client.clone(), &self.pairing, &self.gate) {
                    let id = p.pair_id.clone();
                    return Task::perform(async move { c.pair_poll(&id).await }, Msg::Polled);
                }
            }
            Msg::Polled(Ok(Some(session))) => self.signed_in(session),
            Msg::Polled(Ok(None)) => {}
            Msg::Polled(Err(f)) => {
                self.pairing = None;
                self.failed(f);
            }
            Msg::UseOffline => {
                self.busy = None;
                self.gate = Gate::Offline;
            }
            Msg::SignInFromOffline => {
                self.menu_open = false;
                self.error = None;
                self.gate = Gate::SignedOut;
            }
            // The person's own browser: the choice in Settings, or a link's own button. (A link whose page goes to
            // Lattice's browser never reaches here: app.rs takes it to the Lattice page.)
            Msg::Open(url) | Msg::OpenYours(url) => {
                if let Err(why) = web::open_yours(&url) {
                    self.error = Some(why);
                }
            }
            // As the backdrop's Motion: the choice holds at once, and is kept with the other preferences when it can be.
            Msg::WebPages(pages) => {
                self.web = pages;
                self.web_said = match &self.web_file {
                    Some(file) => web::keep(file, pages).err().map(|why| format!("{why}, so the choice lasts until Alelyon closes")),
                    None => Some("USERPROFILE is not set, so the choice lasts until Alelyon closes".into()),
                };
            }
            Msg::Notice(open) => self.notice_open = open,
            Msg::Motion(on) => self.motion.set(on),
            Msg::Menu(open) => self.menu_open = open,
            Msg::Settings => {
                self.menu_open = false;
                self.settings = Some(settings::Pane::Account);
            }
            Msg::SettingsPane(pane) => self.settings = Some(pane),
            Msg::SettingsQuery(query) => self.settings_query = query,
            Msg::SettingsClose => {
                self.settings = None;
                self.settings_query.clear();
            }
            Msg::Exit => self.menu_open = false,
            Msg::WhatsNew(open) => {
                self.menu_open = false;
                self.whats_new_open = open;
            }
            Msg::SignOut => {
                self.menu_open = false;
                if let Gate::SignedIn(s) = std::mem::replace(&mut self.gate, Gate::SignedOut) {
                    if let Some(p) = &self.session_file {
                        vault::forget(p);
                    }
                    self.stay = false;
                    if let Some(c) = self.client.clone() {
                        // the service revokes the token; a failure here leaves it to lapse on its own
                        return Task::perform(async move { c.sign_out(&s.refresh_token).await }, |_| Msg::Notice(false));
                    }
                }
            }
        }
        Task::none()
    }

    /// The service's status (its state, its ways to sign in, its notices) while the sign-in screen or the Account page
    /// shows it: asked every `STATUS_EVERY` on a task of its own, and said to the window only when the answer differs
    /// from the last one said, so there is no Check again and an unchanged answer draws nothing. The service pushes
    /// nothing yet; a server-sent stream would make this a push (a follow-up on the service's side).
    pub fn status_watch(&self) -> Subscription<Msg> {
        match &self.client {
            Some(c) => Subscription::run_with(StatusWatch(c.clone()), status_stream),
            None => Subscription::none(),
        }
    }

    /// The QR tab asks whether the code was approved, at the service's pace, only while it is on show.
    pub fn subscription(&self) -> Subscription<Msg> {
        match (&self.gate, self.tab, &self.pairing) {
            (Gate::SignedOut, Tab::Qr, Some((p, ..))) => iced::time::every(Duration::from_secs(p.interval_s)).map(|_| Msg::Poll),
            _ => Subscription::none(),
        }
    }
}

/// How often the status is asked while a page shows it.
pub const STATUS_EVERY: Duration = Duration::from_secs(60);

/// The status watch, known to iced by the service's address.
struct StatusWatch(Client);

impl std::hash::Hash for StatusWatch {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "identity-status".hash(state);
        self.0.base().hash(state);
    }
}

fn status_stream(watch: &StatusWatch) -> impl futures::Stream<Item = Msg> + use<> {
    let c = watch.0.clone();
    iced::stream::channel(1, async move |mut output: futures::channel::mpsc::Sender<Msg>| {
        let mut said: Option<Result<Status, Failure>> = None;
        loop {
            let now = c.status().await;
            if said.as_ref() != Some(&now) {
                said = Some(now.clone());
                if futures::SinkExt::send(&mut output, Msg::Status(now)).await.is_err() {
                    return;
                }
            }
            tokio::time::sleep(STATUS_EVERY).await;
        }
    })
}

// ------------------------------------------------------------------- what it draws

type El<'a> = Element<'a, Msg>;

fn words<'a>(s: impl text::IntoFragment<'a>, size: f32, color: Color) -> iced::widget::Text<'a> {
    text(s).size(size).color(color).font(fonts().ui)
}

fn link<'a>(s: &'a str, msg: Option<Msg>) -> El<'a> {
    button(words(s, 11.0, theme::TEXT_DIM)).padding([2.0, 4.0]).on_press_maybe(msg).style(theme::ghost_button).into()
}

/// A link's small button: its page in the person's own browser, this once, with what it does
/// on hover. Inert when the service named no address.
fn yours<'a>(url: &str) -> El<'a> {
    let msg = (!url.is_empty()).then(|| Msg::OpenYours(url.to_string()));
    let glyph = words("\u{21D7}", 13.0, if msg.is_some() { theme::TEXT_DIM } else { theme::TEXT_FAINT });
    let b = button(glyph).padding([2.0, 6.0]).on_press_maybe(msg).style(theme::ghost_button);
    tooltip(b, container(words("Open in your own browser", 12.0, theme::TEXT)).padding([4.0, 8.0]).style(theme::card), tooltip::Position::Bottom)
        .into()
}

/// One provider's tile: its name on its brand colour; dimmed and inert when the service has it switched off.
fn tile<'a>(id: &'static str, name: &'static str, bg: Color, fg: Color, enabled: bool) -> El<'a> {
    // Switched off: no brand colour at all, on a solid dark fill, so every name stays readable over the moving
    // backdrop (a dimmed brand colour hid ORCID's dark green name; a see-through fill let the fog wash names out).
    let (bg, fg) = if enabled { (bg, fg) } else { (theme::RAISED, theme::TEXT_FAINT) };
    button(container(words(name, 12.0, fg)).center_x(Length::Fill))
        .width(Length::FillPortion(1))
        .padding([9.0, 4.0])
        .on_press_maybe(enabled.then_some(Msg::Provider(id)))
        .style(move |_, status| button::Style {
            background: Some(Background::Color(if matches!(status, button::Status::Hovered) { theme::with_alpha(bg, 0.85) } else { bg })),
            text_color: fg,
            border: Border { color: theme::LINE, width: 1.0, radius: 6.0.into() },
            ..button::Style::default()
        })
        .into()
}

/// The screen.
pub fn view<'a>(s: &'a State, mark: Option<&'a image::Handle>, spinner: El<'a>) -> El<'a> {
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    if s.gate == Gate::Checking {
        return container(column![spinner, words("Signing you in…", 14.0, theme::TEXT_DIM)].spacing(12).align_x(Alignment::Center))
            .center(Length::Fill)
            .style(theme::canvas_bg)
            .into();
    }
    // ---- the left: the form
    let brand: El<'a> = match mark {
        Some(h) => {
            row![image(h.clone()).width(40).height(40), words("ALELYON", 22.0, theme::GOLD)].spacing(10).align_y(Alignment::Center).into()
        }
        None => words("ALELYON", 22.0, theme::GOLD).into(),
    };
    let tab = |label: &'a str, t: Tab| -> El<'a> {
        button(container(words(label, 13.0, if s.tab == t { theme::TEXT } else { theme::TEXT_DIM })).center_x(Length::Fill))
            .width(Length::FillPortion(1))
            .padding([8.0, 8.0])
            .on_press(Msg::Tab(t))
            .style(theme::segment_button(s.tab == t))
            .into()
    };
    let tabs = container(row![tab("Sign-in", Tab::Password), tab("QR Code", Tab::Qr)].spacing(4)).padding(4).style(theme::well);
    let reachable = s.client.is_some();
    let enabled = |id: &str| s.status.as_ref().map(|st| st.providers.iter().any(|p| p.id == id && p.enabled)).unwrap_or(false);
    let mut form = column![brand, Space::new().height(28), tabs, Space::new().height(18)].spacing(0);
    match s.tab {
        Tab::Password => {
            let busy = s.busy.is_some();
            let field = |placeholder: &'a str, value: &'a str, msg: fn(String) -> Msg, secure: bool| -> El<'a> {
                text_input(placeholder, value)
                    .on_input_maybe((!busy).then_some(msg))
                    .on_submit(Msg::Submit)
                    .secure(secure)
                    .padding(14)
                    .size(14)
                    .into()
            };
            form = form
                .push(field("EMAIL OR USERNAME", &s.login, Msg::Login, false))
                .push(Space::new().height(10))
                .push(field("PASSWORD", &s.password, Msg::Password, true))
                .push(Space::new().height(14));
            let mut rows = column![].spacing(6);
            for chunk in PROVIDERS.chunks(4) {
                let mut r = row![].spacing(6);
                for (id, name, bg, fg) in chunk {
                    r = r.push(tile(id, name, *bg, *fg, reachable && enabled(id) && !busy));
                }
                rows = rows.push(r);
            }
            form = form
                .push(rows)
                .push(Space::new().height(12))
                .push(checkbox(s.stay).label("Stay signed in").on_toggle_maybe((!busy).then_some(Msg::Stay)).size(16).text_size(13.0));
        }
        Tab::Qr => {
            form = form.push(match &s.pairing {
                Some((p, dark, width)) => Element::from(
                    column![
                        canvas(qr::Drawing { dark: dark.clone(), width: *width }).width(220).height(220),
                        words(format!("Code {}", p.user_code), 16.0, theme::TEXT),
                        words(
                            "Scan it with your phone where you are signed in to Alelyon, check the code matches, and approve.",
                            12.5,
                            theme::TEXT_DIM
                        ),
                        link("Get a new code", Some(Msg::NewCode)),
                    ]
                    .spacing(10)
                    .align_x(Alignment::Center),
                ),
                None if reachable && s.error.is_none() => {
                    Element::from(column![spinner, words("Getting a code…", 12.5, theme::TEXT_DIM)].spacing(8).align_x(Alignment::Center))
                }
                None => link("Get a code", reachable.then_some(Msg::NewCode)),
            });
        }
    }
    if !reachable {
        form = form.push(Space::new().height(12)).push(
            container(words(
                "Signing in is switched off on this PC (ALELYON_IDENTITY_URL=off). Use Alelyon offline: Sinai, transcription, the fleet and your data all work without an account.",
                12.0,
                theme::TEXT,
            ))
            .padding(10)
            .style(theme::notice(theme::CAUTION)),
        );
    }
    if let Some(e) = &s.error {
        form = form.push(Space::new().height(10)).push(words(e.as_str(), 12.5, theme::DANGER));
    }
    if let Some(b) = s.busy {
        form = form.push(Space::new().height(10)).push(row![words(format!("{b}…"), 12.5, theme::TEXT_DIM)].spacing(8));
    }
    let ready = reachable && s.tab == Tab::Password && s.busy.is_none() && !s.login.trim().is_empty() && !s.password.is_empty();
    let arrow = button(container(words("→", 26.0, if ready { theme::ON_GOLD } else { theme::TEXT_FAINT })).center(Length::Fill))
        .width(64)
        .height(64)
        .on_press_maybe(ready.then_some(Msg::Submit))
        .style(move |_, status| button::Style {
            background: Some(Background::Color(match (ready, status) {
                (true, button::Status::Hovered) => theme::GOLD_STRONG,
                (true, _) => theme::GOLD,
                _ => theme::RAISED,
            })),
            border: Border { radius: 20.0.into(), ..Border::default() },
            ..button::Style::default()
        });
    let urls = s.status.clone().unwrap_or_default();
    let open = |u: &str| (!u.is_empty()).then(|| Msg::Open(u.to_string()));
    let foot = column![
        row![link("CAN'T SIGN IN?", open(&urls.reset_url)), yours(&urls.reset_url)].spacing(2).align_y(Alignment::Center),
        row![
            link("CREATE ACCOUNT", open(&urls.sign_up_url)),
            yours(&urls.sign_up_url),
            Space::new().width(Length::Fill),
            words(version, 11.0, theme::TEXT_FAINT)
        ]
        .spacing(2)
        .align_y(Alignment::Center),
        Space::new().height(6),
        button(container(words("Use Alelyon offline", 13.0, theme::TEXT)).center_x(Length::Fill))
            .width(Length::Fill)
            .padding([9.0, 10.0])
            .on_press(Msg::UseOffline)
            .style(theme::secondary_button),
    ]
    .spacing(2);
    let left = container(
        column![form, Space::new().height(Length::Fill), container(arrow).center_x(Length::Fill), Space::new().height(Length::Fill), foot]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    // no panel of its own: the form sits on the same background as the mark
    .width(400)
    .height(Length::Fill)
    .padding([40.0, 48.0]);

    // ---- the right: the notices, over the backdrop
    // clear of the window's own title bar, which lies over this screen's top edge
    let mut overlay = column![].spacing(10).padding(iced::Padding { top: 44.0, right: 28.0, bottom: 28.0, left: 28.0 });
    if let Some(n) = s.notice() {
        let title = if n.title.is_empty() { "Maintenance Notification" } else { n.title.as_str() };
        overlay = overlay.push(
            button(row![words("◆", 13.0, theme::GOLD), words(title, 15.0, theme::TEXT)].spacing(8).align_y(Alignment::Center))
                .padding([4.0, 6.0])
                .on_press(Msg::Notice(!s.notice_open))
                .style(theme::ghost_button),
        );
        if s.notice_open {
            let when = match (n.starts_at.as_str(), n.ends_at.as_str()) {
                ("", "") => String::new(),
                (a, "") => format!("From {a}."),
                (a, b) => format!("From {a} to {b}."),
            };
            overlay = overlay.push(
                container(column![words(n.body.as_str(), 13.0, theme::TEXT), words(when, 12.0, theme::TEXT_DIM)].spacing(4))
                    .padding(12)
                    .max_width(520)
                    .style(theme::notice(theme::GOLD)),
            );
        }
    }
    if let Some(st) = &s.status {
        if st.in_maintenance() {
            overlay = overlay.push(words(
                "Sign-in is unavailable during the maintenance. You can still use Alelyon offline.",
                13.0,
                theme::CAUTION,
            ));
        }
    }
    // the art: a carbon-fibre plain under a rolling gold fog the cursor stirs, over the whole window and under the form
    // and the notices; where the card draws nothing, the old diagonal gradient under it shows (`backdrop`)
    let corner = row![Space::new().width(Length::Fill), motion_button(&s.motion)].padding([0.0, 12.0]);
    let right = container(
        column![overlay, Space::new().height(Length::Fill), corner].padding(iced::Padding { bottom: 10.0, ..Default::default() }),
    )
    .width(Length::Fill)
    .height(Length::Fill);
    stack![crate::backdrop::view(s.motion.on), row![left, right]].into()
}

/// The backdrop's Motion switch, small in the corner: "Motion: on" or "Motion: off", with what it is about on hover.
pub fn motion_button<'a>(m: &crate::backdrop::Motion) -> El<'a> {
    let label = if m.on { "Motion: on" } else { "Motion: off" };
    let tip = if m.forced_still { "Animated background (kept off by CENTCOM_STILL_BACKDROP)" } else { "Animated background" };
    iced::widget::tooltip(
        button(words(label, 11.0, theme::TEXT_FAINT))
            .padding([3.0, 8.0])
            .on_press_maybe((!m.forced_still).then_some(Msg::Motion(!m.on)))
            .style(theme::ghost_button),
        container(words(tip, 11.0, theme::TEXT_DIM)).padding([4.0, 8.0]).style(theme::card),
        iced::widget::tooltip::Position::Top,
    )
    .into()
}

/// The account menu, over the main window's corner: who is signed in, and the account's pages, settings, sign out
/// and exit (after the Riot Client's).
pub fn menu<'a>(s: &'a State) -> El<'a> {
    let item = |label: &'a str, msg: Option<Msg>| -> El<'a> {
        button(words(label, 13.0, theme::TEXT))
            .width(Length::Fill)
            .padding([8.0, 12.0])
            .on_press_maybe(msg)
            .style(theme::ghost_button)
            .into()
    };
    let urls = s.status.clone().unwrap_or_default();
    let open = |u: &str| (!u.is_empty()).then(|| Msg::Open(u.to_string()));
    let mut col = column![].spacing(2).width(240);
    match &s.gate {
        Gate::SignedIn(session) => {
            let a = &session.account;
            col = col
                .push(
                    column![
                        words(a.name(), 15.0, theme::TEXT),
                        words(if a.email.is_empty() { "Signed in" } else { a.email.as_str() }, 12.0, theme::TEXT_DIM),
                        row![
                            container(Space::new()).width(8).height(8).style(theme::dot(theme::POSITIVE)),
                            words("Online", 12.0, theme::TEXT_DIM)
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(3)
                    .padding([10.0, 12.0]),
                )
                .push(row![item("Account details ↗", open(&urls.account_url)), yours(&urls.account_url)].align_y(Alignment::Center))
                .push(row![item("Account security ↗", open(&urls.security_url)), yours(&urls.security_url)].align_y(Alignment::Center))
                .push(item("Settings", Some(Msg::Settings)))
                .push(item("What's New", Some(Msg::WhatsNew(true))))
                .push(item("Sign out", Some(Msg::SignOut)))
                .push(item("Exit", Some(Msg::Exit)));
        }
        _ => {
            col = col
                .push(
                    column![words("Offline", 15.0, theme::TEXT), words("Local features only", 12.0, theme::TEXT_DIM)]
                        .spacing(3)
                        .padding([10.0, 12.0]),
                )
                .push(item("Sign in", Some(Msg::SignInFromOffline)))
                .push(item("Settings", Some(Msg::Settings)))
                .push(item("What's New", Some(Msg::WhatsNew(true))))
                .push(item("Exit", Some(Msg::Exit)));
        }
    }
    container(col).padding(6).style(theme::card).into()
}

/// The release notes the app was built with (WHATS-NEW.md), newest first: each release's heading and its lines.
pub fn release_notes() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut notes: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    for line in include_str!("../../WHATS-NEW.md").lines() {
        if let Some(h) = line.strip_prefix("## ") {
            notes.push((h.trim(), Vec::new()));
        } else if let (Some(item), Some(last)) = (line.strip_prefix("- "), notes.last_mut()) {
            last.1.push(item.trim());
        }
    }
    notes
}

/// What's New: the release notes in a card over the main window.
pub fn whats_new<'a>() -> El<'a> {
    let mut col = column![
        row![
            words("What's New", 18.0, theme::TEXT),
            Space::new().width(Length::Fill),
            button(words("Close", 12.0, theme::TEXT_DIM)).on_press(Msg::WhatsNew(false)).style(theme::ghost_button),
        ]
        .align_y(Alignment::Center)
    ]
    .spacing(14);
    for (heading, items) in release_notes() {
        let mut block = column![words(heading, 14.0, theme::GOLD)].spacing(6);
        for item in items {
            block = block.push(row![words("•", 13.0, theme::TEXT_DIM), words(item, 13.0, theme::TEXT)].spacing(8));
        }
        col = col.push(block);
    }
    container(scrollable(col.padding(iced::Padding { top: 4.0, right: 14.0, bottom: 4.0, left: 4.0 })).style(theme::scrollbars).height(Length::Shrink))
        .padding(18)
        .max_width(560.0)
        .max_height(520.0)
        .style(theme::card)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_notes_are_newest_first_and_every_release_says_something() {
        let notes = release_notes();
        assert!(!notes.is_empty());
        assert!(notes.iter().all(|(h, items)| h.len() >= 10 && h[..10].chars().filter(|c| *c == '-').count() == 2 && !items.is_empty()));
        let dates: Vec<&str> = notes.iter().map(|(h, _)| &h[..10]).collect();
        assert!(dates.windows(2).all(|w| w[0] >= w[1]), "newest first: {dates:?}");
    }

    #[test]
    fn settings_say_when_the_service_cannot_be_read_and_the_watchs_next_answer_replaces_it() {
        let mut s = state(Gate::Offline);
        assert_eq!(settings::service_state(&s).0, "Switched off on this PC (ALELYON_IDENTITY_URL=off)", "no client");
        s.client = Client::new("http://127.0.0.1:9".into()).ok();
        assert_eq!(settings::service_state(&s).0, "Asking…");
        assert_eq!(s.status_fresh.words(0.0), "Connecting to the source…");
        let _ = s.update(Msg::Status(Err(Failure::Unreachable("connection refused".into()))));
        assert!(settings::service_state(&s).0.contains("could not be reached (connection refused)"));
        assert!(s.status_fresh.unavailable && s.status_fresh.words(0.0).starts_with("Source unavailable: "));
        let _ = s.update(Msg::Status(Ok(Status::from_json(&serde_json::json!({"service": "up", "notices": []})))));
        assert_eq!(settings::service_state(&s).0, "Up");
        assert!(s.status_error.is_none() && !s.status_fresh.unavailable, "the next answer clears the old one");
    }

    #[test]
    fn the_menu_opens_whats_new_and_closes_itself_for_settings_and_exit() {
        let mut s = state(Gate::Offline);
        s.menu_open = true;
        let _ = s.update(Msg::WhatsNew(true));
        assert!(s.whats_new_open && !s.menu_open);
        let _ = s.update(Msg::WhatsNew(false));
        assert!(!s.whats_new_open);
        for m in [Msg::Settings, Msg::Exit] {
            s.menu_open = true;
            let _ = s.update(m);
            assert!(!s.menu_open);
        }
    }

    /// The Settings panel: its parts chosen and searched; a search finds parts by their titles, descriptions and
    /// settings, every word of it, in any case; closing forgets the search.
    #[test]
    fn the_settings_panel_switches_searches_and_closes() {
        use settings::{Pane, found};
        let mut s = state(Gate::Offline);
        let _ = s.update(Msg::Settings);
        assert_eq!(s.settings, Some(Pane::Account));
        let _ = s.update(Msg::SettingsPane(Pane::Appearance));
        assert_eq!(s.settings, Some(Pane::Appearance));
        assert_eq!(found(""), Pane::ALL);
        assert_eq!(found("MOTION"), [Pane::Appearance]);
        assert_eq!(found("forget session"), [Pane::Security]);
        assert_eq!(found("your browser"), [Pane::WebPages]);
        assert_eq!(found("privacy"), [Pane::WebPages, Pane::About], "the Web pages part names the Privacy Notice too");
        assert!(found("quantum toaster").is_empty());
        let _ = s.update(Msg::SettingsQuery("privacy".into()));
        let _ = settings::panel(&s);
        let _ = s.update(Msg::SettingsQuery("quantum toaster".into()));
        let _ = settings::panel(&s);
        let _ = s.update(Msg::SettingsClose);
        assert_eq!((s.settings, s.settings_query.as_str()), (None, ""));
        for pane in Pane::ALL {
            let _ = s.update(Msg::SettingsPane(pane));
            let _ = settings::panel(&s);
        }
    }

    fn state(gate: Gate) -> State {
        State {
            gate,
            client: None,
            status: None,
            tab: Tab::Password,
            login: String::new(),
            password: String::new(),
            stay: false,
            session_file: None,
            busy: None,
            error: None,
            pairing: None,
            notice_open: false,
            menu_open: false,
            whats_new_open: false,
            settings: None,
            settings_query: String::new(),
            status_error: None,
            status_fresh: crate::live::Freshness::waiting(),
            link_ticket: None,
            web: web::WebPages::Lattice,
            web_file: None,
            web_said: None,
            motion: crate::backdrop::Motion { on: true, forced_still: false, file: None, note: None },
        }
    }

    #[test]
    fn the_motion_button_switches_the_backdrop_and_keeps_the_choice() {
        let dir = crate::sqlite_ro::tests::scratch("signin-motion");
        let file = dir.join("preferences.json");
        let mut s = state(Gate::SignedOut);
        s.motion.file = Some(file.clone());
        let _ = s.update(Msg::Motion(false));
        assert!(!s.motion.on && s.motion.note.is_none());
        assert_eq!(crate::prefs::load(&file).motion, Some(false), "kept");
        let _ = s.update(Msg::Motion(true));
        assert!(s.motion.on);
        assert_eq!(crate::prefs::load(&file).motion, Some(true));
        // nowhere to keep it: it still switches, and says it will not last
        s.motion.file = None;
        let _ = s.update(Msg::Motion(false));
        assert!(!s.motion.on && s.motion.note.as_deref().is_some_and(|n| n.contains("until Alelyon closes")));
        // forced still by CENTCOM_STILL_BACKDROP: the button changes nothing
        s.motion = crate::backdrop::Motion { on: false, forced_still: true, file: Some(file.clone()), note: None };
        let _ = s.update(Msg::Motion(true));
        assert!(!s.motion.on);
        assert_eq!(crate::prefs::load(&file).motion, Some(true), "and writes nothing");
        // the screen is drawn with the button in either state
        let _ = view(&s, None, Space::new().into());
        s.motion = crate::backdrop::Motion { on: true, forced_still: false, file: None, note: None };
        let _ = view(&s, None, Space::new().into());
    }

    /// Settings' Open web pages in: the choice holds at once and is kept with the other preferences (the backdrop's
    /// motion stays); with nowhere to keep it, it still holds and says it will not last. The screen, the menu and the
    /// Settings page draw in either state.
    #[test]
    fn the_web_pages_choice_holds_and_is_kept_with_the_other_preferences() {
        let dir = crate::sqlite_ro::tests::scratch("signin-web-pages");
        let file = dir.join("preferences.json");
        crate::prefs::save(&file, &crate::prefs::Prefs { motion: Some(false) }).unwrap();
        let mut s = state(Gate::Offline);
        s.web_file = Some(file.clone());
        let _ = s.update(Msg::WebPages(web::WebPages::Yours));
        assert_eq!((s.web, s.web_said.as_deref()), (web::WebPages::Yours, None));
        assert_eq!(web::load(&file), web::WebPages::Yours, "kept");
        assert_eq!(crate::prefs::load(&file).motion, Some(false), "the motion choice stays");
        let _ = view(&s, None, Space::new().into());
        let _ = menu(&s);
        let _ = settings::panel(&s);
        let _ = s.update(Msg::WebPages(web::WebPages::Lattice));
        assert_eq!(web::load(&file), web::WebPages::Lattice);
        s.web_file = None;
        let _ = s.update(Msg::WebPages(web::WebPages::Yours));
        assert_eq!(s.web, web::WebPages::Yours);
        assert!(s.web_said.as_deref().is_some_and(|n| n.contains("until Alelyon closes")), "{:?}", s.web_said);
        let _ = settings::panel(&s);
    }

    #[test]
    fn use_offline_opens_alelyon_and_sign_in_brings_the_screen_back() {
        let mut s = state(Gate::SignedOut);
        assert!(!s.through());
        let _ = s.update(Msg::UseOffline);
        assert!(s.through() && s.account().is_none());
        let _ = s.update(Msg::SignInFromOffline);
        assert_eq!(s.gate, Gate::SignedOut);
    }

    #[test]
    fn nothing_is_sent_without_a_service_or_with_an_empty_field() {
        let mut s = state(Gate::SignedOut);
        s.login = "t@example.com".into();
        s.password = "pw".into();
        let _ = s.update(Msg::Submit);
        assert!(s.busy.is_none(), "no service, no request");
        let _ = s.update(Msg::Provider("github"));
        assert!(s.busy.is_none());
    }

    #[test]
    fn a_session_signs_in_clears_the_password_and_sign_out_returns() {
        let dir = crate::sqlite_ro::tests::scratch("signin-gate");
        let mut s = state(Gate::SignedOut);
        // the kept-session file in a scratch folder of this test's own (never the person's, nor another test's)
        s.session_file = Some(dir.join("session.sealed"));
        s.password = "secret".into();
        s.stay = true;
        let session = Session {
            refresh_token: "tok".into(),
            expires_at: String::new(),
            account: client::Account { id: "a1".into(), email: "t@example.com".into(), ..Default::default() },
        };
        let _ = s.update(Msg::Done(Ok(session)));
        assert!(s.through() && s.password.is_empty());
        #[cfg(windows)]
        assert_eq!(vault::open(&dir.join("session.sealed")).as_deref(), Some("tok"), "kept, sealed");
        let _ = s.update(Msg::SignOut);
        assert_eq!(s.gate, Gate::SignedOut);
        assert!(!dir.join("session.sealed").exists(), "forgotten at sign-out");
        let _ = s.update(Msg::Done(Err(Failure::Credentials)));
        assert!(s.error.as_deref().is_some_and(|e| e.contains("do not match")));
    }

    /// Every forget reaches the state's own file (a scratch one here): a kept session the service refused, Settings'
    /// Forget, and a sign-in without Stay signed in; a service that could not be reached forgets nothing. Settings says
    /// what that file holds.
    #[test]
    fn each_forget_reaches_the_states_own_session_file() {
        let dir = crate::sqlite_ro::tests::scratch("signin-forget");
        let file = dir.join("session.sealed");
        let kept_before = || std::fs::write(&file, b"kept by an earlier start").unwrap();
        let mut s = state(Gate::Checking);
        s.session_file = Some(file.clone());
        kept_before();
        assert!(settings::kept_on_this_pc(&s));
        let _ = s.update(Msg::Resumed(Err(Failure::Unreachable("connection refused".into()))));
        assert!(file.exists(), "a service that could not be reached keeps it for the next start");
        let _ = s.update(Msg::Resumed(Err(Failure::Expired)));
        assert!(!file.exists() && !settings::kept_on_this_pc(&s), "a refused one is forgotten");
        kept_before();
        s.stay = true;
        let _ = s.update(Msg::ForgetKept);
        assert!(!file.exists() && !s.stay);
        kept_before();
        let _ = s.update(Msg::Done(Ok(Session { refresh_token: "t".into(), expires_at: String::new(), account: Default::default() })));
        assert!(s.through() && !file.exists(), "signed in without Stay signed in: nothing outlives the window");
    }

    /// The window's start opens the session kept in the file it is given and holds that file for what follows: Stay
    /// signed in stays ticked, and the session is renewed when there is a service. With no file, nothing is read.
    #[test]
    fn the_start_opens_the_session_kept_in_the_file_it_is_given() {
        let (s, _) = State::boot_at(true, None);
        assert_eq!((s.session_file.as_deref(), s.stay, &s.gate), (None, false, &Gate::SignedOut));
        #[cfg(windows)]
        {
            let file = crate::sqlite_ro::tests::scratch("signin-boot").join("session.sealed");
            vault::keep(&file, "tok").unwrap();
            let (s, _) = State::boot_at(true, Some(file.clone()));
            assert_eq!((s.session_file.as_deref(), s.stay), (Some(file.as_path()), true));
            assert_eq!(s.gate == Gate::Checking, s.client.is_some(), "renewed at start when there is a service: {:?}", s.gate);
        }
    }

    #[test]
    fn an_existing_account_asks_for_its_password_and_keeps_the_ticket_for_it() {
        let mut s = state(Gate::SignedOut);
        s.tab = Tab::Qr;
        let _ = s.update(Msg::Done(Err(Failure::LinkNeeded("t1".into()))));
        assert_eq!((s.link_ticket.as_deref(), s.tab), (Some("t1"), Tab::Password));
        assert!(s.error.as_deref().is_some_and(|e| e.contains("already uses that email")) && s.through() == false);
        let _ = s.update(Msg::Done(Ok(Session { refresh_token: "x".into(), expires_at: String::new(), account: Default::default() })));
        assert!(s.link_ticket.is_none(), "spent once signed in");
    }

    #[test]
    fn the_maintenance_notice_comes_first() {
        let mut s = state(Gate::SignedOut);
        let _ = s.update(Msg::Status(Ok(Status::from_json(&serde_json::json!({"service": "up", "notices": [
            {"id": "1", "kind": "info", "title": "New"}, {"id": "2", "kind": "maintenance", "title": "Scheduled update"}]})))));
        assert_eq!(s.notice().map(|n| n.title.as_str()), Some("Scheduled update"));
    }
}
