//! Friends: the social panel, after the Riot Client's: friends online and offline, adding one by
//! username, requests in and out, and one-to-one chat. It talks to the identity service's social routes (contract v1, "Social") with the session the sign-in holds, and only while signed in.
//!
//! The service runs nothing on a timer; this side polls, and only while the window is open and signed in: presence
//! every 60 s, the friends list every 30 s (10 s while the panel is open), and the open conversation every 5 s.
//! Presence is "online" while the app is open and "offline" when it exits from the menu; "away" is not sent yet.

use std::future::Future;
use std::time::Duration;

use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Length, Subscription, Task};
use crate::theme::{self, fonts};

use crate::signin::client::Client;

// ------------------------------------------------------------------- what the service says

// The people, friends, requests, messages and refusals, and the calls that fetch them, live in the
// alelyon-identity-client crate (its social module, with its parsing tests and docs/social-contract.md);
// this module keeps the panel's state and drawing.
use alelyon_identity_client::social;
pub use alelyon_identity_client::social::{Friend, Friends, Message, Person, Presence, Problem};

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
}

/// An open conversation.
pub struct Chat {
    pub with: Person,
    pub messages: Vec<Message>,
    pub draft: String,
    pub sending: bool,
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
            tasks.push(self.refresh());
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

    /// The open conversation again, every 5 s while it is open; as with the list, an answer that changes nothing on show
    /// (the same messages, and nothing unread to clear) makes no message.
    fn read_chat(&self) -> Task<Msg> {
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
                self.chat = Some(Chat { with: person, messages: Vec::new(), draft: String::new(), sending: false });
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
        let bubble = container(words(m.text.as_str(), 13.0, theme::TEXT)).padding([6.0, 10.0]).max_width(240.0).style(if mine {
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
