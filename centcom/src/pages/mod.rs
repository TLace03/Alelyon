//! Pages: public profiles, organizations and posts, after X and LinkedIn. It talks to the identity service's pages
//! routes (the alelyon-identity-client crate's `pages` module) with the session the sign-in holds, and only while
//! signed in.
//!
//! A person's name line reads `[ Founder | CEO ] Ada Lovelace (LACE)`: the bracket is the title they wrote, each
//! parenthesis an organization they belong to and chose to show. A title an organization's owner or admin confirmed
//! carries a gold mark, drawn from the service's `title_verified`, never by reading the line.
//!
//! Nothing here runs on a timer: a screen is read when it is opened, when "Refresh" is pressed, and after each change
//! made from this page. A page is public: the page says so before a person opens theirs.

mod view;

use std::future::Future;

use alelyon_identity_client::pages::{self, Kind, Me, Member, NewPost, Org, OrgPage, PersonPage, Post, Posts, Reason, Target, Thread};
pub use alelyon_identity_client::pages::Problem;
use iced::Task;
use iced::widget::text_editor;

use crate::signin::client::Client;

pub use view::view;

/// What the page shows. The first four are its tabs; the rest are reached from them, with a way back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    Explore,
    Mine,
    Orgs,
    Person(String),
    Org(String),
    Thread(String),
    Members(String),
}

impl Screen {
    pub const TABS: [Screen; 4] = [Screen::Home, Screen::Explore, Screen::Mine, Screen::Orgs];

    pub fn title(&self) -> String {
        match self {
            Screen::Home => "Home".into(),
            Screen::Explore => "Explore".into(),
            Screen::Mine => "My page".into(),
            Screen::Orgs => "Organizations".into(),
            Screen::Person(u) => format!("@{u}"),
            Screen::Org(t) => format!("({t})"),
            Screen::Thread(_) => "Post".into(),
            Screen::Members(t) => format!("({t}) members"),
        }
    }

    fn is_tab(&self) -> bool {
        Screen::TABS.contains(self)
    }
}

/// Who a new post is from: the person, or an organization they run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsWho(pub Option<String>);

impl std::fmt::Display for AsWho {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => write!(f, "Myself"),
            Some(ticker) => write!(f, "({ticker})"),
        }
    }
}

/// The organization a title names, as the picker offers it.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleOrg(pub Option<Org>);

impl std::fmt::Display for TitleOrg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => write!(f, "No organization"),
            Some(o) => write!(f, "{} ({})", o.name, o.ticker),
        }
    }
}

/// A post being written.
#[derive(Default)]
pub struct Composer {
    pub kind: Kind,
    pub title: String,
    pub body: text_editor::Content,
    pub as_who: Option<String>,
    /// The post being quoted.
    pub quoting: Option<Post>,
    pub sending: bool,
}

impl Composer {
    pub fn chars(&self) -> usize {
        self.body.text().trim().chars().count()
    }

    pub fn limit(&self) -> usize {
        match self.kind {
            Kind::Post => pages::POST_MAX_CHARS,
            Kind::Article => pages::ARTICLE_MAX_CHARS,
        }
    }

    /// Ready to send: words within the limit (an article with its title too).
    pub fn ready(&self) -> bool {
        let n = self.chars();
        !self.sending
            && (1..=self.limit()).contains(&n)
            && (self.kind == Kind::Post || (1..=pages::ARTICLE_TITLE_MAX_CHARS).contains(&self.title.trim().chars().count()))
    }

    fn new_post(&self) -> NewPost {
        NewPost {
            kind: self.kind,
            text: self.body.text().trim().to_string(),
            title: (self.kind == Kind::Article).then(|| self.title.trim().to_string()),
            reply_to: None,
            repost_of: self.quoting.as_ref().map(|p| p.id.clone()),
            as_org: self.as_who.clone(),
        }
    }
}

/// The reader's own page as it is being edited.
#[derive(Default)]
pub struct Mine {
    pub title: String,
    pub title_org: Option<Org>,
    pub bio: text_editor::Content,
    /// Something differs from what the service last said.
    pub dirty: bool,
    pub saving: bool,
    /// "Close my page" was pressed once: the page asks before it closes.
    pub confirm_close: bool,
}

/// A new organization being named.
#[derive(Default)]
pub struct NewOrg {
    pub ticker: String,
    pub name: String,
    pub about: String,
}

#[derive(Default)]
pub struct State {
    /// The service and the session token, while signed in; None and nothing happens otherwise.
    session: Option<(Client, String)>,
    pub screen: Option<Screen>,
    history: Vec<Screen>,
    pub me: Option<Me>,
    pub problem: Option<Problem>,
    pub note: Option<String>,
    /// A read is under way for the screen on show.
    pub loading: bool,
    pub home: Option<Posts>,
    pub explore: Option<Posts>,
    pub person: Option<PersonPage>,
    pub org: Option<OrgPage>,
    pub thread: Option<Thread>,
    pub members: Option<(String, Vec<Member>)>,
    pub composer: Composer,
    pub reply: String,
    pub find: String,
    pub mine: Mine,
    pub new_org: NewOrg,
    pub invite: String,
    /// A report whose reason is being chosen.
    pub reporting: Option<Target>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Go(Screen),
    Back,
    Refresh,
    Dismiss,
    Me(Result<Me, Problem>),
    /// A page of posts for Home (true) or Explore (false); `append` adds it under the posts on show.
    Feed(bool, bool, Result<Posts, Problem>),
    More,
    Person(String, bool, Result<PersonPage, Problem>),
    Org(String, bool, Result<OrgPage, Problem>),
    Thread(String, Result<Thread, Problem>),
    Members(String, Result<Vec<Member>, Problem>),
    FindText(String),
    Find,
    // writing
    Kind(Kind),
    Title(String),
    Body(text_editor::Action),
    AsWho(AsWho),
    Quote(Post),
    Unquote,
    Submit,
    Posted(Result<Post, Problem>),
    ReplyText(String),
    SendReply,
    Replied(Result<Post, Problem>),
    // acting on posts and people
    Like(String, bool),
    Repost(String),
    Delete(String),
    FollowPerson(String, bool),
    FollowOrg(String, bool),
    Block(String, bool),
    Reporting(Option<Target>),
    Report(Target, Reason),
    Reported(Result<String, Problem>),
    /// A change was made: what it means in words, or why it failed. The screen on show is read again.
    Done(Result<String, Problem>),
    // my page
    OpenPage,
    ClosePage,
    MineTitle(String),
    MineTitleOrg(TitleOrg),
    MineBio(text_editor::Action),
    SaveMine,
    Saved(Result<Me, Problem>),
    ShowTicker(String, bool),
    MoveTicker(String, bool),
    // organizations
    NewTicker(String),
    NewName(String),
    NewAbout(String),
    CreateOrg,
    Created(Result<Org, Problem>),
    Join(String),
    Leave(String),
    InviteText(String),
    Invite(String, String),
    Remove(String, String),
    Role(String, String, &'static str),
    Confirm(String, String, bool),
}

impl State {
    /// Called whenever the sign-in may have changed: starts afresh on signing in, forgets everything on signing out.
    /// `on_show` says the page is on show, so a new session reads its screen at once.
    pub fn attach(&mut self, session: Option<(Client, String)>, on_show: bool) -> Task<Msg> {
        let was = self.session.as_ref().map(|(_, t)| t.clone());
        let now = session.as_ref().map(|(_, t)| t.clone());
        if was == now {
            return Task::none();
        }
        let screen = self.screen.clone().filter(|_| now.is_some()).filter(Screen::is_tab);
        *self = State { session, screen, ..State::default() };
        if self.session.is_none() {
            return Task::none();
        }
        if on_show {
            return self.open();
        }
        self.fetch_me()
    }

    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// The page was opened: its first screen, or the one it was left on, read again.
    pub fn open(&mut self) -> Task<Msg> {
        let screen = self.screen.clone().unwrap_or(Screen::Home);
        self.screen = Some(screen.clone());
        Task::batch([self.fetch_me(), self.load(&screen, false)])
    }

    pub fn busy(&self) -> bool {
        self.loading || self.composer.sending || self.mine.saving
    }

    /// Runs one pages call with the session's service and token; nothing while signed out.
    fn run<T: Send + 'static, F: Future<Output = Result<T, Problem>> + Send + 'static>(
        &self,
        call: impl FnOnce(Client, String) -> F,
        done: impl Fn(Result<T, Problem>) -> Msg + Send + 'static,
    ) -> Task<Msg> {
        let Some((c, t)) = self.session.clone() else { return Task::none() };
        Task::perform(call(c, t), done)
    }

    fn fetch_me(&self) -> Task<Msg> {
        self.run(|c, t| async move { pages::me(&c, &t).await }, Msg::Me)
    }

    /// Reads what `screen` shows; `before` asks for the page after that post.
    fn read(&self, screen: &Screen, before: Option<String>) -> Task<Msg> {
        let append = before.is_some();
        match screen.clone() {
            Screen::Home => self.run(move |c, t| async move { pages::feed(&c, &t, before.as_deref()).await }, move |r| Msg::Feed(true, append, r)),
            Screen::Explore => {
                self.run(move |c, t| async move { pages::explore(&c, &t, before.as_deref()).await }, move |r| Msg::Feed(false, append, r))
            }
            Screen::Mine | Screen::Orgs => self.fetch_me(),
            Screen::Person(u) => {
                let name = u.clone();
                self.run(move |c, t| async move { pages::person(&c, &t, &u, before.as_deref()).await }, move |r| Msg::Person(name.clone(), append, r))
            }
            Screen::Org(ticker) => {
                let name = ticker.clone();
                self.run(move |c, t| async move { pages::org(&c, &t, &ticker, before.as_deref()).await }, move |r| Msg::Org(name.clone(), append, r))
            }
            Screen::Thread(id) => {
                let key = id.clone();
                self.run(move |c, t| async move { pages::thread(&c, &t, &id, None).await }, move |r| Msg::Thread(key.clone(), r))
            }
            Screen::Members(ticker) => {
                let key = ticker.clone();
                self.run(move |c, t| async move { pages::org_members(&c, &t, &ticker).await }, move |r| Msg::Members(key.clone(), r))
            }
        }
    }

    fn load(&mut self, screen: &Screen, keep: bool) -> Task<Msg> {
        if !keep {
            self.loading = true;
        }
        self.read(screen, None)
    }

    /// The screen on show, read again (after a change made here).
    fn reload(&mut self) -> Task<Msg> {
        let Some(screen) = self.screen.clone() else { return Task::none() };
        Task::batch([self.load(&screen, true), self.fetch_me()])
    }

    fn go(&mut self, screen: Screen) -> Task<Msg> {
        if let Some(current) = self.screen.take()
            && !screen.is_tab()
            && current != screen
        {
            self.history.push(current);
        }
        if screen.is_tab() {
            self.history.clear();
        }
        self.reporting = None;
        self.problem = None;
        self.screen = Some(screen.clone());
        self.load(&screen, false)
    }

    /// The `before` for the next page of the screen on show, when it has more.
    fn next_page(&self) -> Option<String> {
        match self.screen.as_ref()? {
            Screen::Home => self.home.as_ref()?.next(),
            Screen::Explore => self.explore.as_ref()?.next(),
            Screen::Person(_) => self.person.as_ref()?.posts.next(),
            Screen::Org(_) => self.org.as_ref()?.posts.next(),
            _ => None,
        }
    }

    /// A change with nothing to read back: `words` say what it did.
    fn done<F: Future<Output = Result<(), Problem>> + Send + 'static>(&self, call: impl FnOnce(Client, String) -> F, words: String) -> Task<Msg> {
        self.run(call, move |r| Msg::Done(r.map(|()| words.clone())))
    }

    /// The page's form, refilled from what the service said.
    fn refill_mine(&mut self) {
        let Some(me) = &self.me else { return };
        self.mine.title = me.me.title.clone();
        self.mine.title_org = me.me.title_org.clone();
        self.mine.bio = text_editor::Content::with_text(&me.bio);
        self.mine.dirty = false;
    }

    /// The tickers to show after a change: the ones on show, in their order, with `ticker` added at the end or taken
    /// out, or moved one place up or down.
    fn tickers_after(&self, ticker: &str, change: TickerChange) -> Vec<String> {
        let mut shown: Vec<String> = self.me.as_ref().map(|m| m.me.tickers.iter().map(|o| o.ticker.clone()).collect()).unwrap_or_default();
        let at = shown.iter().position(|t| t == ticker);
        match (change, at) {
            (TickerChange::Show, None) => shown.push(ticker.to_string()),
            (TickerChange::Hide, Some(i)) => {
                shown.remove(i);
            }
            (TickerChange::Up, Some(i)) if i > 0 => shown.swap(i, i - 1),
            (TickerChange::Down, Some(i)) if i + 1 < shown.len() => shown.swap(i, i + 1),
            _ => {}
        }
        shown
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Go(screen) => return self.go(screen),
            Msg::Back => {
                if let Some(previous) = self.history.pop() {
                    self.screen = Some(previous.clone());
                    self.reporting = None;
                    return self.load(&previous, true);
                }
            }
            Msg::Refresh => return self.reload(),
            Msg::Dismiss => {
                self.problem = None;
                self.note = None;
            }
            Msg::Me(Ok(me)) => {
                let first = self.me.is_none();
                self.me = Some(me);
                if first || !self.mine.dirty {
                    self.refill_mine();
                }
            }
            Msg::Me(Err(p)) => self.problem = Some(p),
            Msg::Feed(home, append, result) => {
                self.loading = false;
                match result {
                    Ok(page) => {
                        let slot = if home { &mut self.home } else { &mut self.explore };
                        match slot.as_mut().filter(|_| append) {
                            Some(shown) => {
                                shown.posts.extend(page.posts);
                                shown.more = page.more;
                            }
                            None => *slot = Some(page),
                        }
                    }
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::More => {
                if let (Some(screen), Some(before)) = (self.screen.clone(), self.next_page()) {
                    return self.read(&screen, Some(before));
                }
            }
            Msg::Person(username, append, result) => {
                if self.screen != Some(Screen::Person(username)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.person.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.posts.extend(page.posts.posts);
                            shown.posts.more = page.posts.more;
                        }
                        None => self.person = Some(page),
                    },
                    Err(p) => {
                        self.person = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::Org(ticker, append, result) => {
                if self.screen != Some(Screen::Org(ticker)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.org.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.posts.extend(page.posts.posts);
                            shown.posts.more = page.posts.more;
                        }
                        None => self.org = Some(page),
                    },
                    Err(p) => {
                        self.org = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::Thread(id, result) => {
                if self.screen != Some(Screen::Thread(id)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(thread) => self.thread = Some(thread),
                    Err(p) => {
                        self.thread = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::Members(ticker, result) => {
                if self.screen != Some(Screen::Members(ticker.clone())) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(list) => self.members = Some((ticker, list)),
                    Err(p) => {
                        self.members = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::FindText(s) => self.find = s,
            Msg::Find => {
                let typed = self.find.trim();
                let screen = if let Some(ticker) = typed.strip_prefix('(').map(|t| t.trim_end_matches(')')).or_else(|| typed.strip_prefix('$')) {
                    Screen::Org(ticker.trim().to_uppercase())
                } else if let Some(name) = typed.strip_prefix('@') {
                    Screen::Person(name.trim().to_lowercase())
                } else if !typed.is_empty() && typed.len() <= 5 && typed.chars().all(|c| c.is_ascii_uppercase()) {
                    Screen::Org(typed.to_string())
                } else if !typed.is_empty() {
                    Screen::Person(typed.to_lowercase())
                } else {
                    return Task::none();
                };
                self.find.clear();
                return self.go(screen);
            }
            Msg::Kind(kind) => self.composer.kind = kind,
            Msg::Title(s) => self.composer.title = s,
            Msg::Body(action) => self.composer.body.perform(action),
            Msg::AsWho(who) => self.composer.as_who = who.0,
            Msg::Quote(post) => {
                self.composer.kind = Kind::Post;
                self.composer.quoting = Some(post);
                if self.screen != Some(Screen::Home) {
                    return self.go(Screen::Home);
                }
            }
            Msg::Unquote => self.composer.quoting = None,
            Msg::Submit => {
                if !self.composer.ready() {
                    return Task::none();
                }
                self.composer.sending = true;
                let new = self.composer.new_post();
                return self.run(move |c, t| async move { pages::post(&c, &t, &new).await }, Msg::Posted);
            }
            Msg::Posted(result) => {
                self.composer.sending = false;
                match result {
                    Ok(_) => {
                        self.composer = Composer { as_who: self.composer.as_who.take(), ..Composer::default() };
                        self.note = Some("Posted.".into());
                        return self.reload();
                    }
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::ReplyText(s) => self.reply = s,
            Msg::SendReply => {
                let Some(Screen::Thread(id)) = self.screen.clone() else { return Task::none() };
                let text = self.reply.trim().to_string();
                if text.is_empty() || self.composer.sending {
                    return Task::none();
                }
                self.composer.sending = true;
                let new = NewPost { text, reply_to: Some(id), ..NewPost::default() };
                return self.run(move |c, t| async move { pages::post(&c, &t, &new).await }, Msg::Replied);
            }
            Msg::Replied(result) => {
                self.composer.sending = false;
                match result {
                    Ok(_) => {
                        self.reply.clear();
                        return self.reload();
                    }
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::Like(id, like) => {
                return self.done(move |c, t| async move { pages::like(&c, &t, &id, like).await }, String::new());
            }
            Msg::Repost(id) => {
                let new = NewPost { repost_of: Some(id), ..NewPost::default() };
                return self.run(
                    move |c, t| async move { pages::post(&c, &t, &new).await },
                    |r| Msg::Done(r.map(|_| "Reposted.".to_string())),
                );
            }
            Msg::Delete(id) => {
                return self.done(move |c, t| async move { pages::delete(&c, &t, &id).await }, "Deleted.".into());
            }
            Msg::FollowPerson(username, follow) => {
                let words = if follow { format!("You follow @{username}.") } else { format!("You no longer follow @{username}.") };
                return self.done(move |c, t| async move { pages::follow_person(&c, &t, &username, follow).await }, words);
            }
            Msg::FollowOrg(ticker, follow) => {
                let words = if follow { format!("You follow ({ticker}).") } else { format!("You no longer follow ({ticker}).") };
                return self.done(move |c, t| async move { pages::follow_org(&c, &t, &ticker, follow).await }, words);
            }
            Msg::Block(username, block) => {
                let words = if block { format!("You blocked @{username}.") } else { format!("You unblocked @{username}.") };
                return self.done(move |c, t| async move { pages::block(&c, &t, &username, block).await }, words);
            }
            Msg::Reporting(target) => self.reporting = target,
            Msg::Report(target, reason) => {
                self.reporting = None;
                return self.run(move |c, t| async move { pages::report(&c, &t, reason, "", &target).await }, Msg::Reported);
            }
            Msg::Reported(Ok(_)) => self.note = Some("Reported. Thank you.".into()),
            Msg::Reported(Err(p)) => self.problem = Some(p),
            Msg::Done(Ok(words)) => {
                if !words.is_empty() {
                    self.note = Some(words);
                }
                return self.reload();
            }
            Msg::Done(Err(p)) => self.problem = Some(p),
            Msg::OpenPage => return self.run(|c, t| async move { pages::open(&c, &t).await }, Msg::Saved),
            Msg::ClosePage => {
                if !self.mine.confirm_close {
                    self.mine.confirm_close = true;
                    return Task::none();
                }
                self.mine.confirm_close = false;
                return self.run(|c, t| async move { pages::close(&c, &t).await }, Msg::Saved);
            }
            Msg::MineTitle(s) => {
                self.mine.title = s;
                self.mine.dirty = true;
            }
            Msg::MineTitleOrg(choice) => {
                self.mine.title_org = choice.0;
                self.mine.dirty = true;
            }
            Msg::MineBio(action) => {
                let edits = action.is_edit();
                self.mine.bio.perform(action);
                self.mine.dirty |= edits;
            }
            Msg::SaveMine => {
                if self.mine.saving {
                    return Task::none();
                }
                self.mine.saving = true;
                let title = self.mine.title.clone();
                let org = self.mine.title_org.as_ref().map(|o| o.ticker.clone()).unwrap_or_default();
                let bio = self.mine.bio.text().trim_end().to_string();
                return self.run(move |c, t| async move { pages::update(&c, &t, Some(&title), Some(&org), Some(&bio)).await }, Msg::Saved);
            }
            Msg::Saved(result) => {
                self.mine.saving = false;
                match result {
                    Ok(me) => {
                        self.me = Some(me);
                        self.refill_mine();
                        self.note = Some("Saved.".into());
                    }
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::ShowTicker(ticker, show) => {
                let tickers = self.tickers_after(&ticker, if show { TickerChange::Show } else { TickerChange::Hide });
                return self.run(move |c, t| async move { pages::set_tickers(&c, &t, &tickers).await }, Msg::Saved);
            }
            Msg::MoveTicker(ticker, up) => {
                let tickers = self.tickers_after(&ticker, if up { TickerChange::Up } else { TickerChange::Down });
                return self.run(move |c, t| async move { pages::set_tickers(&c, &t, &tickers).await }, Msg::Saved);
            }
            Msg::NewTicker(s) => self.new_org.ticker = s.trim().to_uppercase().chars().filter(char::is_ascii_alphabetic).take(5).collect(),
            Msg::NewName(s) => self.new_org.name = s,
            Msg::NewAbout(s) => self.new_org.about = s,
            Msg::CreateOrg => {
                let NewOrg { ticker, name, about } = &self.new_org;
                let (ticker, name, about) = (ticker.clone(), name.trim().to_string(), about.trim().to_string());
                if ticker.len() < 2 || name.is_empty() {
                    return Task::none();
                }
                return self.run(move |c, t| async move { pages::org_create(&c, &t, &ticker, &name, &about).await }, Msg::Created);
            }
            Msg::Created(Ok(org)) => {
                self.new_org = NewOrg::default();
                self.note = Some(format!("{} ({}) is yours.", org.name, org.ticker));
                return self.reload();
            }
            Msg::Created(Err(p)) => self.problem = Some(p),
            Msg::Join(ticker) => {
                return self.run(
                    move |c, t| async move { pages::org_join(&c, &t, &ticker).await },
                    |r| {
                        Msg::Done(r.map(|state| {
                            if state == "member" { "You are a member.".to_string() } else { "Asked to join. Its admins decide.".to_string() }
                        }))
                    },
                );
            }
            Msg::Leave(ticker) => {
                return self.done(move |c, t| async move { pages::org_leave(&c, &t, &ticker).await }, "Done.".into());
            }
            Msg::InviteText(s) => self.invite = s,
            Msg::Invite(ticker, username) => {
                let username = username.trim().trim_start_matches('@').to_lowercase();
                if username.is_empty() {
                    return Task::none();
                }
                self.invite.clear();
                return self.run(
                    move |c, t| async move { pages::org_invite(&c, &t, &ticker, &username).await },
                    |r| Msg::Done(r.map(|state| if state == "member" { "Now a member.".to_string() } else { "Invited.".to_string() })),
                );
            }
            Msg::Remove(ticker, member) => {
                return self.done(move |c, t| async move { pages::org_remove(&c, &t, &ticker, &member).await }, "Removed.".into());
            }
            Msg::Role(ticker, member, role) => {
                return self.done(move |c, t| async move { pages::org_role(&c, &t, &ticker, &member, role).await }, "Role changed.".into());
            }
            Msg::Confirm(ticker, member, confirm) => {
                return self.run(
                    move |c, t| async move { pages::confirm_title(&c, &t, &ticker, &member, confirm).await },
                    |r| Msg::Done(r.map(|title| match title {
                        Some(title) => format!("Confirmed \u{201C}{title}\u{201D}."),
                        None => "Confirmation withdrawn.".to_string(),
                    })),
                );
            }
        }
        Task::none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TickerChange {
    Show,
    Hide,
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alelyon_identity_client::pages::{Membership, Person};

    fn org(ticker: &str) -> Org {
        Org { id: format!("org_{ticker}"), ticker: ticker.into(), name: ticker.into() }
    }

    fn me_showing(tickers: &[&str]) -> Me {
        Me {
            me: Person { id: "me".into(), username: Some("me".into()), tickers: tickers.iter().map(|t| org(t)).collect(), ..Person::default() },
            bio: "Bio".into(),
            memberships: ["AAA", "BBB", "CCC"]
                .iter()
                .map(|t| Membership { org: org(t), role: "member".into(), state: "active".into(), shown: tickers.contains(t), confirmed_title: None })
                .collect(),
        }
    }

    #[test]
    fn nothing_runs_while_signed_out_and_signing_out_forgets_everything() {
        let mut s = State::default();
        assert!(!s.active());
        let _ = s.update(Msg::Me(Ok(me_showing(&[]))));
        let _ = s.attach(None, false);
        assert!(s.me.is_some(), "no change, nothing forgotten");
        let c = Client::new("http://127.0.0.1:9".into()).unwrap();
        let _ = s.attach(Some((c.clone(), "t1".into())), false);
        assert!(s.active() && s.me.is_none() && s.screen.is_none(), "a new session starts afresh, reading only who it is");
        s.screen = Some(Screen::Person("ada".into()));
        let _ = s.attach(None, true);
        assert!(!s.active() && s.screen.is_none(), "signing out forgets what was on show");
        let _ = s.attach(Some((c, "t2".into())), true);
        assert_eq!(s.screen, Some(Screen::Home), "signing in while the page is on show opens its first screen");
    }

    #[test]
    fn the_shown_tickers_change_one_at_a_time_and_keep_their_order() {
        let mut s = State { me: Some(me_showing(&["AAA", "BBB"])), ..State::default() };
        assert_eq!(s.tickers_after("CCC", TickerChange::Show), ["AAA", "BBB", "CCC"]);
        assert_eq!(s.tickers_after("AAA", TickerChange::Hide), ["BBB"]);
        assert_eq!(s.tickers_after("BBB", TickerChange::Up), ["BBB", "AAA"]);
        assert_eq!(s.tickers_after("BBB", TickerChange::Down), ["AAA", "BBB"], "the last cannot move down");
        assert_eq!(s.tickers_after("AAA", TickerChange::Up), ["AAA", "BBB"], "the first cannot move up");
        s.me = Some(me_showing(&[]));
        assert!(s.tickers_after("AAA", TickerChange::Hide).is_empty());
    }

    #[test]
    fn find_opens_a_person_or_an_organization() {
        let mut s = State::default();
        for (typed, screen) in [
            ("@Ada", Screen::Person("ada".into())),
            ("ada", Screen::Person("ada".into())),
            ("(lace)", Screen::Org("LACE".into())),
            ("$lace", Screen::Org("LACE".into())),
            ("LACE", Screen::Org("LACE".into())),
        ] {
            let _ = s.update(Msg::FindText(typed.into()));
            let _ = s.update(Msg::Find);
            assert_eq!(s.screen, Some(screen), "{typed}");
            assert!(s.find.is_empty());
        }
        let _ = s.update(Msg::Go(Screen::Thread("pst_1".into())));
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Org("LACE".into())), "back goes to the screen before");
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Person("ada".into())));
        let _ = s.update(Msg::Go(Screen::Explore));
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Explore), "a tab starts the way back afresh");
    }

    #[test]
    fn an_answer_for_a_screen_no_longer_on_show_is_dropped() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Person("ada".into())));
        let _ = s.update(Msg::Go(Screen::Person("bob".into())));
        let _ = s.update(Msg::Person("ada".into(), false, Ok(PersonPage::default())));
        assert!(s.person.is_none());
        let _ = s.update(Msg::Person("bob".into(), false, Ok(PersonPage::default())));
        assert!(s.person.is_some() && !s.loading);
    }

    #[test]
    fn a_composer_is_ready_only_within_its_limits() {
        let mut c = Composer::default();
        assert!(!c.ready(), "empty");
        c.body = text_editor::Content::with_text("Hello");
        assert!(c.ready());
        c.body = text_editor::Content::with_text(&"x".repeat(pages::POST_MAX_CHARS + 1));
        assert!(!c.ready(), "too long for a post");
        c.kind = Kind::Article;
        assert!(!c.ready(), "an article needs its title");
        c.title = "On Engines".into();
        assert!(c.ready());
        let new = c.new_post();
        assert_eq!((new.kind, new.title.as_deref(), new.repost_of), (Kind::Article, Some("On Engines"), None));
    }

    #[test]
    fn the_my_page_form_keeps_unsaved_edits_when_the_service_answers() {
        let mut s = State::default();
        let _ = s.update(Msg::Me(Ok(me_showing(&[]))));
        assert_eq!(s.mine.bio.text().trim_end(), "Bio");
        let _ = s.update(Msg::MineTitle("Founder | CEO".into()));
        let _ = s.update(Msg::Me(Ok(me_showing(&["AAA"]))));
        assert_eq!(s.mine.title, "Founder | CEO", "an edit in progress is kept");
        let _ = s.update(Msg::Saved(Ok(me_showing(&["AAA"]))));
        assert!(!s.mine.dirty && s.mine.title.is_empty(), "a save refills the form from the service");
    }

    #[test]
    fn a_new_ticker_is_letters_only_upper_case_and_at_most_five() {
        let mut s = State::default();
        let _ = s.update(Msg::NewTicker("la-ce99xyz".into()));
        assert_eq!(s.new_org.ticker, "LACEX");
    }
}
