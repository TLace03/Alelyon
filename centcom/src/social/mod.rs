//! Friends: the social panel, after the Riot Client's: friends online and offline, adding one by
//! username, requests in and out, and one-to-one chat. It talks to the identity service's social routes (contract v1, "Social") with the session the sign-in holds, and only while signed in.
//!
//! The service runs nothing on a timer; this side polls, and only while the window is open and signed in: presence
//! every 60 s, the friends list every 30 s (10 s while the panel is open), and the open conversation every 5 s.
//! Presence is "online" while the app is open and "offline" when it exits from the menu; "away" is not sent yet.
//! The person chooses their own status in the panel (online, away, do not disturb, appear offline); the service keeps
//! it apart from presence and combines the two for friends, so the heartbeat never overwrites it.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Length, Subscription, Task};
use crate::theme::{self, fonts};

use crate::signin::client::Client;

// ------------------------------------------------------------------- what the service says

// The people, friends, requests, messages and refusals, and the calls that fetch them, live in the
// alelyon-identity-client crate (its social module, with its parsing tests and docs/social-contract.md);
// this module keeps the panel's state and drawing.
use alelyon_identity_client::sealed::DeviceKey;
use alelyon_identity_client::sealed_chat::{self, Device, Entry, Opened};
use alelyon_identity_client::social;
pub use alelyon_identity_client::social::{Friend, Friends, Message, Person, Presence, Problem, Status};

// ------------------------------------------------------------------- state

#[derive(Clone, Debug)]
pub enum Msg {
    /// Open or close the panel.
    Panel(bool),
    Refresh,
    Listed(Result<Friends, Problem>),
    Me(Result<Person, Problem>),
    Heartbeat,
    /// Presence was reported (or not: a missed heartbeat is retried by the next one).
    PresenceSent,
    /// Show or hide the status choices.
    StatusMenu(bool),
    /// The person picked a status.
    ChooseStatus(Status),
    /// The service's answer: the status read on signing in, or the one just chosen.
    StatusIs(Result<Status, Problem>),
    AddText(String),
    Add,
    Added(Result<String, Problem>),
    Respond(String, bool),
    Cancel(String),
    Remove(String),
    Done(Result<(), Problem>),
    UsernameText(String),
    SaveUsername,
    Open(Person),
    Close,
    Poll,
    Messages(String, Result<Vec<Message>, Problem>),
    Draft(String),
    Send,
    Sent(String, Result<Message, Problem>),
    /// This PC's friends device key, loaded (or made) and registered: sealed chat can start (ADR-0049 Phase 3).
    DeviceReady(Result<Arc<DeviceKey>, String>),
    /// A conversation page read and opened on this device.
    Page(String, Result<ChatPage, Problem>),
    /// A sealed send's answer: the message as shown, and whether it went sealed.
    SentSealed(String, Result<(Message, bool), Problem>),
}

/// A conversation page as this device shows it: sealed entries already opened (or explained), which entries were
/// sealed, and "added a new device" notices.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChatPage {
    pub messages: Vec<Message>,
    pub locked: Vec<String>,
    pub notices: Vec<String>,
    /// The device lists this page used, when it fetched them (None when it used the cached ones).
    pub lists: Option<(Vec<Device>, Vec<Device>)>,
}

/// What the chat says when a device was added to this account since it last looked.
const OWN_DEVICE_NOTICE: &str = "A new device was added to your account, and messages are now sealed for it too. If you did not add it, change your password.";

/// How long the friend's and this account's device lists are reused between polls of an open chat. A send always reads
/// them fresh, and the service refuses one that misses a device, so a stale list never drops a recipient.
const DEVICE_LISTS_FOR: Duration = Duration::from_secs(60);

/// Sealed chat is on since the switch-over (ADR-0049 Phase 4); `ALELYON_FRIENDS_SEALED=0` turns it off on this PC.
pub fn sealed_switch() -> bool {
    std::env::var("ALELYON_FRIENDS_SEALED").as_deref() != Ok("0")
}

/// What a sealed entry that did not open says in the chat.
fn explained(opened: Opened) -> String {
    match opened {
        Opened::Text(t) => t,
        Opened::NotForThisDevice => "This message was sealed before this device could read it.".into(),
        Opened::UnknownSender => "This message came from a device that was removed, so it cannot be checked.".into(),
        Opened::Unopenable => "This message could not be opened.".into(),
    }
}

/// An open conversation.
pub struct Chat {
    pub with: Person,
    pub messages: Vec<Message>,
    pub draft: String,
    pub sending: bool,
    /// The ids of messages that were sealed (shown with a lock).
    pub locked: Vec<String>,
    /// "Added a new device" notices, newest last.
    pub notices: Vec<String>,
}

#[derive(Default)]
pub struct State {
    /// The service and the session token, while signed in; None and nothing happens otherwise.
    session: Option<(Client, String)>,
    pub open: bool,
    pub me: Option<Person>,
    pub list: Option<Friends>,
    pub problem: Option<Problem>,
    pub add: String,
    pub note: Option<String>,
    pub username: String,
    pub chat: Option<Chat>,
    /// The status this person chose, as the service last said.
    pub status: Status,
    pub status_menu: bool,
    /// This PC's friends device key, once loaded and registered (with the sealed-chat switch on).
    pub device: Option<Arc<DeviceKey>>,
    /// The open chat's friend id, when its device lists were read, the friend's and this account's.
    device_lists: Option<(String, Instant, Vec<Device>, Vec<Device>)>,
}

impl State {
    /// Called whenever the sign-in may have changed: starts on signing in, forgets everything on signing out.
    pub fn attach(&mut self, session: Option<(Client, String)>) -> Task<Msg> {
        let was = self.session.as_ref().map(|(_, t)| t.clone());
        let now = session.as_ref().map(|(_, t)| t.clone());
        if was == now {
            return Task::none();
        }
        let leaving = self.session.take();
        *self = State { session, open: self.open && now.is_some(), ..State::default() };
        let mut tasks = Vec::new();
        if let Some((c, t)) = leaving {
            tasks.push(Task::perform(async move { social::presence(&c, &t, Presence::Offline).await }, |_| Msg::PresenceSent));
        }
        if self.session.is_some() {
            tasks.push(self.presence("online"));
            tasks.push(self.fetch_me());
            tasks.push(self.run(|c, t| async move { social::status(&c, &t).await }, Msg::StatusIs));
            tasks.push(self.refresh());
            if sealed_switch() {
                tasks.push(self.ready_device());
            }
        }
        Task::batch(tasks)
    }

    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// Runs one social call with the session's service and token; nothing while signed out.
    fn run<T: Send + 'static, F: Future<Output = Result<T, Problem>> + Send + 'static>(
        &self,
        call: impl FnOnce(Client, String) -> F,
        done: impl Fn(Result<T, Problem>) -> Msg + Send + 'static,
    ) -> Task<Msg> {
        let Some((c, t)) = self.session.clone() else { return Task::none() };
        Task::perform(call(c, t), done)
    }

    /// The friends list again. An answer that is the list on show, with nothing to clear, makes no message: the list is
    /// polled every 30 s, and each message redraws the whole window.
    fn refresh(&self) -> Task<Msg> {
        let shown = self.list.clone().filter(|_| matches!(self.problem, None | Some(Problem::UsernameNeeded)));
        self.run(|c, t| async move { social::friends(&c, &t).await }, Msg::Listed)
            .then(move |m| if matches!(&m, Msg::Listed(Ok(list)) if shown.as_ref() == Some(list)) { Task::none() } else { Task::done(m) })
    }

    fn fetch_me(&self) -> Task<Msg> {
        self.run(|c, t| async move { social::profile(&c, &t).await }, Msg::Me)
    }

    /// Tells the service this app is online (or offline, on leaving).
    pub fn presence(&self, state: &'static str) -> Task<Msg> {
        let state = Presence::from_word(Some(state));
        self.run(move |c, t| async move { social::presence(&c, &t, state).await }, |_| Msg::PresenceSent)
    }

    /// Load (or make) this PC's device key, kept with DPAPI, and register it.
    fn ready_device(&self) -> Task<Msg> {
        let Some((c, t)) = self.session.clone() else { return Task::none() };
        Task::perform(
            async move {
                let at = sealed_chat::device_key_path().ok_or("no place to keep a device key here")?;
                let mut key = sealed_chat::load_or_create_device_key(&at)?;
                match sealed_chat::register(&c, &t, &key).await {
                    Err(Problem::Refused(code, _)) if code == "device-revoked" => {
                        key = sealed_chat::replace_device_key(&at)?;
                        sealed_chat::register(&c, &t, &key).await.map_err(|p| p.words())?;
                    }
                    Err(p) => return Err(p.words()),
                    Ok(_) => {}
                }
                Ok(Arc::new(key))
            },
            Msg::DeviceReady,
        )
    }

    /// The open conversation as this device reads it: sealed entries opened against the friend's and this account's
    /// listed devices, and notices for devices the friend added since last time.
    fn read_sealed_chat(&self, device: Arc<DeviceKey>) -> Task<Msg> {
        let Some(chat) = &self.chat else { return Task::none() };
        let friend = chat.with.clone();
        let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        let Some((c, t)) = self.session.clone() else { return Task::none() };
        let cached = self
            .device_lists
            .as_ref()
            .filter(|(f, at, _, _)| *f == friend.id && at.elapsed() < DEVICE_LISTS_FOR)
            .map(|(_, _, theirs, mine)| (theirs.clone(), mine.clone()));
        Task::perform(
            async move {
                let id = friend.id.clone();
                let page = async {
                    let (entries, _) = sealed_chat::conversation(&c, &t, &id, &device.id(), None).await?;
                    let fresh = cached.is_none();
                    let (theirs, mine) = match cached {
                        Some(lists) => lists,
                        None => (sealed_chat::devices(&c, &t, Some(&id)).await?, sealed_chat::devices(&c, &t, None).await?),
                    };
                    let senders = sealed_chat::Senders::new(&theirs, &mine);
                    let mut out = ChatPage::default();
                    if fresh {
                        out.lists = Some((theirs.clone(), mine.clone()));
                    }
                    if let Some(at) = sealed_chat::known_devices_path().filter(|_| fresh) {
                        for d in sealed_chat::remember_devices(&at, &id, &theirs) {
                            out.notices.push(format!("{} added a new device. Messages are now sealed for it too.", friend.name()));
                            let _ = d;
                        }
                        // This account's own devices too (a device added to it is sealed for as well); this PC's own
                        // key, new after a replacement, is not news.
                        for d in sealed_chat::remember_devices(&at, &me, &mine) {
                            if d.id != device.id() {
                                out.notices.push(OWN_DEVICE_NOTICE.into());
                            }
                        }
                    }
                    for entry in &entries {
                        let text = explained(sealed_chat::open(entry, &device, &me, &id, &senders));
                        if let Entry::Sealed(sealed) = entry {
                            out.locked.push(sealed.id.clone());
                            out.messages.push(Message { id: sealed.id.clone(), from: sealed.from.clone(), text, sent_at: sealed.sent_at.clone() });
                        } else if let Entry::Text(m) = entry {
                            out.messages.push(m.clone());
                        }
                    }
                    Ok::<ChatPage, Problem>(out)
                }
                .await;
                (id, page)
            },
            |(id, r)| Msg::Page(id, r),
        )
    }

    /// The open conversation again, every 5 s while it is open; as with the list, an answer that changes nothing on show
    /// (the same messages, and nothing unread to clear) makes no message.
    fn read_chat(&self) -> Task<Msg> {
        if let Some(device) = self.device.clone() {
            return self.read_sealed_chat(device);
        }
        let Some(chat) = &self.chat else { return Task::none() };
        let id = chat.with.id.clone();
        let Some((c, t)) = self.session.clone() else { return Task::none() };
        let unread = self.list.as_ref().and_then(|l| l.friends.iter().find(|f| f.person.id == id)).is_some_and(|f| f.unread > 0);
        let shown = (!unread).then(|| chat.messages.clone());
        Task::perform(
            async move {
                let v = social::messages(&c, &t, &id, None).await;
                (id, v.map(|page| page.messages))
            },
            |(id, r)| Msg::Messages(id, r),
        )
        .then(move |m| if matches!(&m, Msg::Messages(_, Ok(got)) if shown.as_ref() == Some(got)) { Task::none() } else { Task::done(m) })
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Panel(open) => {
                self.open = open;
                if open {
                    return self.refresh();
                }
            }
            Msg::Refresh => return self.refresh(),
            Msg::Listed(Ok(list)) => {
                self.list = Some(list);
                if !matches!(self.problem, Some(Problem::UsernameNeeded)) {
                    self.problem = None;
                }
            }
            Msg::Listed(Err(p)) | Msg::Me(Err(p)) | Msg::Done(Err(p)) => self.problem = Some(p),
            Msg::Me(Ok(me)) => {
                if me.username.is_none() {
                    self.problem = Some(Problem::UsernameNeeded);
                } else if matches!(self.problem, Some(Problem::UsernameNeeded)) {
                    self.problem = None;
                }
                self.me = Some(me);
            }
            // the service's answer changes nothing on show, so it makes no message (and no redraw)
            Msg::Heartbeat => return self.presence("online").discard(),
            Msg::PresenceSent => {}
            Msg::StatusMenu(open) => self.status_menu = open,
            Msg::ChooseStatus(status) => {
                self.status_menu = false;
                if status == self.status {
                    return Task::none();
                }
                return self.run(move |c, t| async move { social::set_status(&c, &t, status).await }, Msg::StatusIs);
            }
            Msg::StatusIs(Ok(status)) => self.status = status,
            Msg::StatusIs(Err(p)) => self.note = Some(p.words()),
            Msg::AddText(s) => self.add = s,
            Msg::Add => {
                let name = self.add.trim().trim_start_matches('@').to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.note = None;
                return self.run(move |c, t| async move { social::request(&c, &t, &name).await }, Msg::Added);
            }
            Msg::Added(Ok(state)) => {
                self.note = Some(if state == "friends" {
                    format!("You and {} are now friends.", self.add.trim())
                } else {
                    format!("Request sent to {}.", self.add.trim())
                });
                self.add.clear();
                return self.refresh();
            }
            Msg::Added(Err(p)) => self.note = Some(p.words()),
            Msg::Respond(id, accept) => {
                return self.run(move |c, t| async move { social::respond(&c, &t, &id, accept).await }, Msg::Done);
            }
            Msg::Cancel(id) => return self.run(move |c, t| async move { social::cancel(&c, &t, &id).await }, Msg::Done),
            Msg::Remove(id) => {
                if self.chat.as_ref().is_some_and(|c| c.with.id == id) {
                    self.chat = None;
                }
                return self.run(move |c, t| async move { social::remove(&c, &t, &id).await }, Msg::Done);
            }
            Msg::Done(Ok(())) => return self.refresh(),
            Msg::UsernameText(s) => self.username = s,
            Msg::SaveUsername => {
                let name = self.username.trim().to_lowercase();
                return self.run(move |c, t| async move { social::set_username(&c, &t, &name).await }, Msg::Me);
            }
            Msg::Open(person) => {
                self.chat = Some(Chat { with: person, messages: Vec::new(), draft: String::new(), sending: false, locked: Vec::new(), notices: Vec::new() });
                return self.read_chat();
            }
            Msg::Close => {
                self.chat = None;
                return self.refresh();
            }
            Msg::Poll => return self.read_chat(),
            Msg::Messages(id, Ok(messages)) => {
                if let Some(chat) = self.chat.as_mut().filter(|c| c.with.id == id) {
                    chat.messages = messages;
                }
                if let Some(f) = self.list.as_mut().and_then(|l| l.friends.iter_mut().find(|f| f.person.id == id)) {
                    f.unread = 0;
                }
            }
            Msg::Messages(_, Err(p)) => self.problem = Some(p),
            Msg::DeviceReady(Ok(device)) => {
                self.device = Some(device);
                return self.read_chat();
            }
            Msg::DeviceReady(Err(why)) => self.note = Some(format!("Sealed chat is not available on this PC: {why}")),
            Msg::Page(id, Ok(page)) => {
                if let Some((theirs, mine)) = page.lists.clone() {
                    self.device_lists = Some((id.clone(), Instant::now(), theirs, mine));
                }
                if let Some(chat) = self.chat.as_mut().filter(|c| c.with.id == id) {
                    chat.messages = page.messages;
                    chat.locked = page.locked;
                    chat.notices.extend(page.notices);
                }
                if let Some(f) = self.list.as_mut().and_then(|l| l.friends.iter_mut().find(|f| f.person.id == id)) {
                    f.unread = 0;
                }
            }
            Msg::Page(_, Err(p)) => self.problem = Some(p),
            Msg::SentSealed(id, result) => {
                if let Some(chat) = self.chat.as_mut().filter(|c| c.with.id == id) {
                    chat.sending = false;
                    match result {
                        Ok((m, sealed)) => {
                            if sealed {
                                chat.locked.push(m.id.clone());
                            }
                            chat.messages.push(m);
                            chat.draft.clear();
                        }
                        Err(p) => self.problem = Some(p),
                    }
                }
            }
            Msg::Draft(s) => {
                if let Some(chat) = self.chat.as_mut() {
                    chat.draft = s;
                }
            }
            Msg::Send => {
                let Some(chat) = self.chat.as_mut() else { return Task::none() };
                let text = chat.draft.trim().to_string();
                if text.is_empty() || chat.sending {
                    return Task::none();
                }
                chat.sending = true;
                let id = chat.with.id.clone();
                let Some((c, t)) = self.session.clone() else { return Task::none() };
                if let Some(device) = self.device.clone() {
                    let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
                    return Task::perform(
                        async move {
                            let r = match sealed_chat::send(&c, &t, &device, &me, &id, &text).await {
                                Ok(entry) => Ok((Message { id: entry.id().to_string(), from: me.clone(), text: text.clone(), sent_at: String::new() }, true)),
                                // The friend has no device yet: plain text, until they do (the service then refuses it).
                                Err(Problem::Refused(code, _)) if code == "no-devices" => social::send(&c, &t, &id, &text).await.map(|m| (m, false)),
                                Err(p) => Err(p),
                            };
                            (id, r)
                        },
                        |(id, r)| Msg::SentSealed(id, r),
                    );
                }
                return Task::perform(
                    async move {
                        let r = social::send(&c, &t, &id, &text).await;
                        (id, r)
                    },
                    |(id, r)| Msg::Sent(id, r),
                );
            }
            Msg::Sent(id, result) => {
                if let Some(chat) = self.chat.as_mut().filter(|c| c.with.id == id) {
                    chat.sending = false;
                    match result {
                        Ok(m) => {
                            chat.messages.push(m);
                            chat.draft.clear();
                        }
                        Err(p) => self.problem = Some(p),
                    }
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        if self.session.is_none() {
            return Subscription::none();
        }
        let list_every = if self.open { 10 } else { 30 };
        let mut subs = vec![
            iced::time::every(Duration::from_secs(60)).map(|_| Msg::Heartbeat),
            iced::time::every(Duration::from_secs(list_every)).map(|_| Msg::Refresh),
        ];
        if self.open && self.chat.is_some() {
            subs.push(iced::time::every(Duration::from_secs(5)).map(|_| Msg::Poll));
        }
        Subscription::batch(subs)
    }
}

// ------------------------------------------------------------------- what it draws

type El<'a> = Element<'a, Msg>;

fn words<'a>(s: impl text::IntoFragment<'a>, size: f32, color: Color) -> iced::widget::Text<'a> {
    text(s).size(size).color(color).font(fonts().ui)
}

fn small_button<'a>(label: &'a str, msg: Option<Msg>) -> El<'a> {
    button(words(label, 12.0, theme::TEXT)).padding([4.0, 10.0]).on_press_maybe(msg).style(theme::ghost_button).into()
}

fn presence_dot<'a>(p: Presence) -> El<'a> {
    let color = match p {
        Presence::Online => theme::POSITIVE,
        Presence::Away => theme::CAUTION,
        Presence::DoNotDisturb => theme::DANGER,
        Presence::Offline => theme::TEXT_FAINT,
    };
    container(Space::new()).width(8).height(8).style(theme::dot(color)).into()
}

fn friend_row<'a>(f: &'a Friend) -> El<'a> {
    let mut line = row![presence_dot(f.presence), words(f.person.name(), 13.5, theme::TEXT)].spacing(10).align_y(Alignment::Center);
    line = line.push(Space::new().width(Length::Fill));
    if f.unread > 0 {
        line = line.push(container(words(f.unread.to_string(), 11.0, theme::ON_GOLD)).padding([1.0, 7.0]).style(theme::dot(theme::GOLD)));
    }
    button(line).width(Length::Fill).padding([7.0, 8.0]).on_press(Msg::Open(f.person.clone())).style(theme::ghost_button).into()
}

/// The person's own status: the current one as a button that opens the four choices.
fn status_picker<'a>(s: &'a State) -> El<'a> {
    let current = button(
        row![presence_dot(s.status.shown()), words(s.status.label(), 12.5, theme::TEXT), words(if s.status_menu { "▴" } else { "▾" }, 11.0, theme::TEXT_DIM)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([4.0, 8.0])
    .on_press(Msg::StatusMenu(!s.status_menu))
    .style(theme::ghost_button);
    if !s.status_menu {
        return current.into();
    }
    let mut choices = column![current].spacing(2);
    for choice in Status::ALL {
        let colour = if choice == s.status { theme::TEXT } else { theme::TEXT_DIM };
        choices = choices.push(
            button(row![presence_dot(choice.shown()), words(choice.label(), 12.5, colour)].spacing(8).align_y(Alignment::Center))
                .width(Length::Fill)
                .padding([4.0, 8.0])
                .on_press(Msg::ChooseStatus(choice))
                .style(theme::ghost_button),
        );
    }
    choices.into()
}

/// The friends bar's height, so the open panel can stop above it.
pub const BAR_HEIGHT: f32 = 31.0;

/// The friends bar along the bottom of every page, while signed in: the person's status, how many friends are online,
/// the unread count, and a button that opens and closes the panel (as the rail's friends button does).
pub fn bar<'a>(s: &'a State) -> El<'a> {
    let (online, unread) = s.list.as_ref().map(|l| (l.online(), l.unread())).unwrap_or((0, 0));
    let mut line = row![
        presence_dot(s.status.shown()),
        words("Friends", 12.0, if s.open { theme::GOLD } else { theme::TEXT }),
        words(format!("{online} online"), 11.5, theme::TEXT_DIM),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if unread > 0 {
        line = line.push(container(words(unread.to_string(), 10.5, theme::ON_GOLD)).padding([0.0, 6.0]).style(theme::dot(theme::GOLD)));
    }
    line = line.push(words(if s.open { "▾" } else { "▴" }, 10.0, theme::TEXT_DIM));
    let toggle = button(line).padding([4.0, 12.0]).on_press(Msg::Panel(!s.open)).style(theme::ghost_button);
    column![
        container(Space::new()).width(Length::Fill).height(1).style(theme::line),
        container(row![Space::new().width(Length::Fill), toggle].align_y(Alignment::Center)).width(Length::Fill).padding([1.0, 8.0]),
    ]
    .height(BAR_HEIGHT)
    .into()
}

fn heading<'a>(title: String) -> El<'a> {
    words(title, 11.0, theme::TEXT_FAINT).into()
}

/// The panel: over the right of the window, like the Riot Client's friends list.
pub fn panel<'a>(s: &'a State) -> El<'a> {
    let body: El<'a> = if let Some(chat) = &s.chat { chat_view(s, chat) } else { list_view(s) };
    container(body).padding(14).width(320).height(Length::Fill).style(theme::card).into()
}

fn list_view<'a>(s: &'a State) -> El<'a> {
    let mut col = column![
        row![words("Friends", 18.0, theme::TEXT), Space::new().width(Length::Fill), small_button("Close", Some(Msg::Panel(false)))]
            .align_y(Alignment::Center)
    ]
    .spacing(12);
    if let Some(me) = &s.me {
        col = col.push(words(
            match &me.username {
                Some(u) => format!("You are @{u}"),
                None => "You have no username yet".into(),
            },
            12.0,
            theme::TEXT_DIM,
        ));
    }
    if !matches!(s.problem, Some(Problem::UsernameNeeded)) && s.me.is_some() {
        col = col.push(status_picker(s));
    }
    if let Some(Problem::UsernameNeeded) = &s.problem {
        col = col
            .push(words("Choose a username so friends can find you: 2-39 lowercase letters, digits, '.', '_' or '-'.", 12.5, theme::TEXT))
            .push(
                row![
                    text_input("username", &s.username).on_input(Msg::UsernameText).on_submit(Msg::SaveUsername).padding(8).size(13),
                    small_button("Save", Some(Msg::SaveUsername)),
                ]
                .spacing(6),
            );
        return col.into();
    }
    if let Some(p) = &s.problem {
        col = col.push(words(p.words(), 12.5, theme::CAUTION));
    }
    col = col.push(
        row![
            text_input("Add a friend by username", &s.add).on_input(Msg::AddText).on_submit(Msg::Add).padding(8).size(13),
            small_button("Add", Some(Msg::Add)),
        ]
        .spacing(6),
    );
    if let Some(note) = &s.note {
        col = col.push(words(note.as_str(), 12.0, theme::TEXT_DIM));
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Some(retiring) = s.list.as_ref().and_then(|l| l.retirement_notice(now)) {
        col = col.push(words(retiring, 12.0, theme::CAUTION));
    }
    let Some(list) = &s.list else {
        return col.push(words("Loading your friends…", 12.5, theme::TEXT_DIM)).into();
    };
    if !list.incoming.is_empty() {
        col = col.push(heading(format!("REQUESTS ({})", list.incoming.len())));
        for r in &list.incoming {
            col = col.push(
                row![
                    words(r.person.name(), 13.0, theme::TEXT),
                    Space::new().width(Length::Fill),
                    small_button("Accept", Some(Msg::Respond(r.id.clone(), true))),
                    small_button("Decline", Some(Msg::Respond(r.id.clone(), false))),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
        }
    }
    let (online, offline): (Vec<&Friend>, Vec<&Friend>) = list.friends.iter().partition(|f| f.presence != Presence::Offline);
    let mut people = column![].spacing(2);
    people = people.push(heading(format!("ONLINE ({})", online.len())));
    for f in online {
        people = people.push(friend_row(f));
    }
    people = people.push(Space::new().height(8)).push(heading(format!("OFFLINE ({})", offline.len())));
    for f in offline {
        people = people.push(friend_row(f));
    }
    if list.friends.is_empty() {
        people = people.push(words("No friends yet. Add one by their username above.", 12.5, theme::TEXT_DIM));
    }
    if !list.outgoing.is_empty() {
        people = people.push(Space::new().height(8)).push(heading(format!("SENT ({})", list.outgoing.len())));
        for r in &list.outgoing {
            people = people.push(
                row![words(r.person.name(), 13.0, theme::TEXT_DIM), Space::new().width(Length::Fill), small_button("Cancel", Some(Msg::Cancel(r.id.clone())))]
                    .align_y(Alignment::Center),
            );
        }
    }
    col.push(scrollable(people).style(theme::scrollbars).height(Length::Fill)).into()
}

fn chat_view<'a>(s: &'a State, chat: &'a Chat) -> El<'a> {
    let me = s.me.as_ref().map(|m| m.id.as_str()).unwrap_or("");
    let mut lines = column![].spacing(8);
    if chat.messages.is_empty() {
        lines = lines.push(words(format!("Say hello to {}.", chat.with.name()), 12.5, theme::TEXT_DIM));
    }
    for m in &chat.messages {
        let mine = m.from == me;
        let mut inside = column![words(m.text.as_str(), 13.0, theme::TEXT)].spacing(2);
        if chat.locked.contains(&m.id) {
            inside = inside.push(words("sealed", 10.0, theme::TEXT_FAINT));
        }
        let bubble = container(inside).padding([6.0, 10.0]).max_width(240.0).style(if mine {
            theme::panel
        } else {
            theme::card
        });
        lines = lines.push(if mine {
            row![Space::new().width(Length::Fill), bubble]
        } else {
            row![bubble, Space::new().width(Length::Fill)]
        });
    }
    let mut col = column![
        row![
            small_button("‹ Friends", Some(Msg::Close)),
            Space::new().width(Length::Fill),
            words(chat.with.name(), 15.0, theme::TEXT),
            Space::new().width(Length::Fill),
            small_button("Remove", Some(Msg::Remove(chat.with.id.clone()))),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(10);
    if let Some(p) = &s.problem {
        col = col.push(words(p.words(), 12.0, theme::CAUTION));
    }
    if s.device.is_some() {
        col = col.push(words("Messages marked sealed are sealed on this PC for your friend's devices and yours before they reach Alelyon. An independent review is pending; no forward secrecy yet.", 11.0, theme::TEXT_FAINT));
    }
    for notice in &chat.notices {
        col = col.push(words(notice.as_str(), 11.5, theme::CAUTION));
    }
    col.push(scrollable(lines).anchor_bottom().style(theme::scrollbars).height(Length::Fill))
        .push(
            row![
                text_input("Message", &chat.draft).on_input(Msg::Draft).on_submit(Msg::Send).padding(8).size(13),
                small_button("Send", (!chat.sending).then_some(Msg::Send)),
            ]
            .spacing(6),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reading the service's answers and refusals is tested with the crate (alelyon-identity-client's social tests).

    #[test]
    fn nothing_runs_while_signed_out_and_signing_out_forgets_everything() {
        let mut s = State::default();
        assert!(!s.active());
        let _ = s.update(Msg::Listed(Ok(Friends::default())));
        let _ = s.attach(None);
        assert!(s.list.is_some(), "no change, nothing forgotten");
        let c = Client::new("http://127.0.0.1:9".into()).unwrap();
        let _ = s.attach(Some((c, "t1".into())));
        assert!(s.active() && s.list.is_none(), "a new session starts afresh");
        s.open = true;
        let _ = s.attach(None);
        assert!(!s.active() && !s.open && s.list.is_none(), "signing out forgets the friends and closes the panel");
    }

    #[test]
    fn a_chosen_status_is_kept_from_the_services_answer_and_a_refusal_keeps_the_old_one() {
        let mut s = State::default();
        assert_eq!(s.status, Status::Online, "online until the service says otherwise");
        let _ = s.update(Msg::StatusMenu(true));
        assert!(s.status_menu);
        let _ = s.update(Msg::ChooseStatus(Status::DoNotDisturb));
        assert!(!s.status_menu, "picking closes the choices");
        assert_eq!(s.status, Status::Online, "the status changes when the service answers, not before");
        let _ = s.update(Msg::StatusIs(Ok(Status::DoNotDisturb)));
        assert_eq!(s.status, Status::DoNotDisturb);
        let _ = s.update(Msg::StatusIs(Err(Problem::RateLimited(30))));
        assert_eq!(s.status, Status::DoNotDisturb, "a refusal leaves the status as it was");
        assert!(s.note.is_some(), "and says why");
    }

    #[test]
    fn a_sealed_page_replaces_the_conversation_marks_its_sealed_messages_and_keeps_notices() {
        let mut s = State::default();
        let ada = Person { id: "a1".into(), username: Some("ada".into()), display_name: "Ada".into() };
        s.list = Some(Friends { friends: vec![Friend { person: ada.clone(), presence: Presence::Online, unread: 2 }], ..Friends::default() });
        s.chat = Some(Chat { with: ada, messages: Vec::new(), draft: String::new(), sending: false, locked: Vec::new(), notices: Vec::new() });
        let m = |id: &str, text: &str| Message { id: id.into(), from: "a1".into(), text: text.into(), sent_at: String::new() };
        let page = ChatPage { messages: vec![m("t1", "plain"), m("s1", "sealed words")], locked: vec!["s1".into()], notices: vec!["Ada added a new device.".into()], lists: None };
        let _ = s.update(Msg::Page("a1".into(), Ok(page.clone())));
        let chat = s.chat.as_ref().unwrap();
        assert_eq!((chat.messages.len(), chat.locked.as_slice(), chat.notices.len()), (2, &["s1".to_string()][..], 1));
        assert_eq!(s.list.as_ref().unwrap().friends[0].unread, 0, "reading clears unread");
        let _ = s.update(Msg::Page("a1".into(), Ok(ChatPage { notices: vec![], ..page })));
        assert_eq!(s.chat.as_ref().unwrap().notices.len(), 1, "a notice stays once shown");
        let _ = s.update(Msg::Page("someone-else".into(), Ok(ChatPage::default())));
        assert_eq!(s.chat.as_ref().unwrap().messages.len(), 2, "another conversation's page is ignored");
    }

    #[test]
    fn a_page_that_fetched_device_lists_caches_them_for_that_friend() {
        let mut s = State::default();
        assert!(s.device_lists.is_none());
        let lists = (vec![], vec![]);
        let _ = s.update(Msg::Page("a1".into(), Ok(ChatPage { lists: Some(lists), ..ChatPage::default() })));
        let (friend, at, _, _) = s.device_lists.as_ref().unwrap();
        assert_eq!(friend, "a1");
        assert!(at.elapsed() < DEVICE_LISTS_FOR);
        let _ = s.update(Msg::Page("a1".into(), Ok(ChatPage::default())));
        assert!(s.device_lists.is_some(), "a page that used the cache keeps it");
    }

    #[test]
    fn a_sealed_send_is_marked_sealed_and_a_plain_fallback_is_not() {
        let mut s = State::default();
        let ada = Person { id: "a1".into(), username: Some("ada".into()), display_name: "Ada".into() };
        s.chat = Some(Chat { with: ada, messages: Vec::new(), draft: "hi".into(), sending: true, locked: Vec::new(), notices: Vec::new() });
        let m = |id: &str| Message { id: id.into(), from: "me".into(), text: "hi".into(), sent_at: String::new() };
        let _ = s.update(Msg::SentSealed("a1".into(), Ok((m("s1"), true))));
        let _ = s.update(Msg::SentSealed("a1".into(), Ok((m("t1"), false))));
        let chat = s.chat.as_ref().unwrap();
        assert_eq!(chat.locked, vec!["s1".to_string()]);
        assert_eq!((chat.messages.len(), chat.draft.as_str(), chat.sending), (2, "", false));
    }

    #[test]
    fn a_device_that_cannot_be_readied_says_why_and_leaves_chat_plain() {
        let mut s = State::default();
        let _ = s.update(Msg::DeviceReady(Err("no DPAPI".into())));
        assert!(s.device.is_none());
        assert!(s.note.as_deref().unwrap_or("").contains("no DPAPI"));
    }

    #[test]
    fn a_missing_username_shows_the_username_form_until_one_is_set() {
        let mut s = State::default();
        let _ = s.update(Msg::Me(Ok(Person { id: "me".into(), username: None, display_name: "Me".into() })));
        assert_eq!(s.problem, Some(Problem::UsernameNeeded));
        let _ = s.update(Msg::Listed(Ok(Friends::default())));
        assert_eq!(s.problem, Some(Problem::UsernameNeeded), "a list does not hide the need");
        let _ = s.update(Msg::Me(Ok(Person { id: "me".into(), username: Some("me".into()), display_name: "Me".into() })));
        assert_eq!(s.problem, None);
    }

    #[test]
    fn reading_a_conversation_clears_its_unread_count_and_a_sent_message_joins_it() {
        let mut s = State::default();
        let ada = Person { id: "a1".into(), username: Some("ada".into()), display_name: "Ada".into() };
        s.list = Some(Friends { friends: vec![Friend { person: ada.clone(), presence: Presence::Online, unread: 3 }], ..Friends::default() });
        let _ = s.update(Msg::Open(ada));
        let m = |id: &str| Message { id: id.into(), from: "a1".into(), text: "hi".into(), sent_at: String::new() };
        let _ = s.update(Msg::Messages("a1".into(), Ok(vec![m("m1")])));
        assert_eq!(s.list.as_ref().unwrap().friends[0].unread, 0);
        let _ = s.update(Msg::Draft("hello".into()));
        s.chat.as_mut().unwrap().sending = true;
        let _ = s.update(Msg::Sent("a1".into(), Ok(m("m2"))));
        let chat = s.chat.as_ref().unwrap();
        assert_eq!((chat.messages.len(), chat.draft.as_str(), chat.sending), (2, "", false));
        let _ = s.update(Msg::Messages("other".into(), Ok(vec![])));
        assert_eq!(s.chat.as_ref().unwrap().messages.len(), 2, "another conversation's answer is ignored");
    }
}
