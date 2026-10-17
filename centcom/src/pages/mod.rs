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

mod icons;
mod keys;
mod kit;
mod look;
mod photos;
mod popover;
#[cfg(test)]
mod snapshots;
mod view;

pub use keys::{SHEET as KEYS, keys};

use std::collections::{HashMap, HashSet};
use std::future::Future;

use alelyon_identity_client::pages::{
    self, About, Applicant, Community, CommunityPage, CommunitySummary, Draft, Education, Employment, Experience, FeedbackCategory,
    FeedbackSort, FeedbackStatus, Found, Joining, JobsQuery, Kind, NewProduct, ProductChanges, ProductList, ProductMaker, ProductPage,
    ProductShow, Products, Stage, Tester,
    ListMembers, ListPage, ListTarget, MediaMatch, Me, Member, ModAct, ModAction, ModReport, PageList,
    NewEducation, NewExperience, NewJob, NewPhoto, NewPoll, NewPost, Notification, Notifications, Org, OrgPage, Person, PersonPage, PersonShow, Post, Posts,
    Reaction, Reason, ReplyRule, SearchKind, Suggestions, Tagged, Target, Thread, Trending, Upload, Workplace,
};
pub use alelyon_identity_client::pages::Problem;
use alelyon_identity_client::social;
use iced::Task;
use iced::widget::text_editor;

use crate::signin::client::Client;
use photos::{Picture, Size};

pub use view::view;

/// How many photos are kept before those no screen on show draws are forgotten.
const PHOTOS_KEPT: usize = 48;

/// What the page shows. The first four are its tabs; the rest are reached from them, with a way back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    Explore,
    Notifications,
    Bookmarks,
    Mine,
    Orgs,
    Person(String),
    Org(String),
    Thread(String),
    Members(String),
    /// Owners and admins only: the posts made as the organization whose author is gone.
    Flagged(String),
    /// The posts carrying a hashtag (lower case, without its #).
    Tag(String),
    /// What a search finds: people, organizations and posts.
    Search(String),
    /// Owners and admins only: the experience entries that name the organization, to confirm.
    OrgExperience(String),
    /// Open jobs, with what narrows them.
    Jobs,
    /// Owners and admins only: who applied for a job (its post's id).
    Applicants(String),
    /// Moderators only: the reports, and the moderation log.
    Moderation,
    /// Communities (ADR-0053): the reader's own, those with the most members, and starting one.
    Communities,
    /// A community, by its handle.
    Community(String),
    /// A community's members; its owners and moderators also see requests and bans.
    CommunityMembers(String),
    /// Lists (ADR-0054): the reader's private lists, and one list (by id) read as a stream.
    Lists,
    List(String),
    /// Drafts and scheduled posts (ADR-0055): the reader's own.
    Drafts,
    /// Builders (ADR-0056): the day's and the week's launches, the newest products, the reader's own and those they
    /// follow; and one product, by its handle.
    Products,
    Product(String),
}

impl Screen {
    pub const TABS: [Screen; 10] = [
        Screen::Home,
        Screen::Explore,
        Screen::Products,
        Screen::Communities,
        Screen::Lists,
        Screen::Jobs,
        Screen::Notifications,
        Screen::Bookmarks,
        Screen::Mine,
        Screen::Orgs,
    ];

    pub fn title(&self) -> String {
        match self {
            Screen::Home => "Home".into(),
            Screen::Explore => "Explore".into(),
            Screen::Notifications => "Notifications".into(),
            Screen::Bookmarks => "Bookmarks".into(),
            Screen::Mine => "My page".into(),
            Screen::Orgs => "Organizations".into(),
            Screen::Person(u) => format!("@{u}"),
            Screen::Org(t) => format!("({t})"),
            Screen::Thread(_) => "Post".into(),
            Screen::Members(t) => format!("({t}) members"),
            Screen::Flagged(t) => format!("({t}) flagged posts"),
            Screen::Tag(t) => format!("#{t}"),
            Screen::Search(q) => format!("Search: {q}"),
            Screen::OrgExperience(t) => format!("({t}) experience to confirm"),
            Screen::Jobs => "Jobs".into(),
            Screen::Applicants(_) => "Applicants".into(),
            Screen::Moderation => "Moderation".into(),
            Screen::Communities => "Communities".into(),
            Screen::Community(h) => format!("c/{h}"),
            Screen::CommunityMembers(h) => format!("c/{h} members"),
            Screen::Lists => "Lists".into(),
            Screen::List(_) => "List".into(),
            Screen::Drafts => "Drafts".into(),
            Screen::Products => "Products".into(),
            Screen::Product(h) => format!("p/{h}"),
        }
    }

    fn is_tab(&self) -> bool {
        Screen::TABS.contains(self) || *self == Screen::Moderation
    }
}

/// Which part of the moderation screen is on show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ModView {
    #[default]
    Open,
    Closed,
    Log,
    /// Uploads a screen matched to known illegal imagery (ADR-0050).
    Matches,
}

impl ModView {
    pub const ALL: [ModView; 4] = [ModView::Open, ModView::Closed, ModView::Log, ModView::Matches];

    pub fn title(self) -> &'static str {
        match self {
            ModView::Open => "Open reports",
            ModView::Closed => "Closed",
            ModView::Log => "Log",
            ModView::Matches => "Photo matches",
        }
    }
}

/// What a chosen photo is for: the post being written, the reader's page, or an organization they run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhotoFor {
    Post,
    /// The page's `avatar` or `banner`.
    Page(&'static str),
    /// An organization's `logo` or `banner`, by its ticker.
    Org(String, &'static str),
    /// A product's `logo`, or its screenshot (`shot`) at a place: its handle, the slot and the place.
    Product(String, &'static str, usize),
}

impl PhotoFor {
    /// What was done, in words, once the photo is set (or cleared).
    fn done_words(&self, cleared: bool) -> String {
        let done = if cleared { "removed" } else { "changed" };
        match self {
            PhotoFor::Post => String::new(),
            PhotoFor::Page("avatar") => format!("Your photo is {done}."),
            PhotoFor::Page(kind) => format!("Your {kind} is {done}."),
            PhotoFor::Org(ticker, kind) => format!("({ticker})'s {kind} is {done}."),
            PhotoFor::Product(handle, "logo", _) => format!("p/{handle}'s logo is {done}."),
            PhotoFor::Product(handle, _, _) => format!("A screenshot of p/{handle} is {}.", if cleared { "removed" } else { "added" }),
        }
    }
}

/// The files chosen in the picker, each read: its name, and its bytes or why it cannot be used; or why the picker
/// failed.
pub type Chosen = Result<Vec<(String, Result<Vec<u8>, String>)>, String>;

/// A photo attached to the post being written: one of the reader's uploads, with its description.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Attached {
    pub id: String,
    pub url: String,
    /// The file's name, as the composer shows it.
    pub name: String,
    pub alt: String,
}

/// Which notifications show: all of them, or those that answer or name the reader (X's Mentions).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NotifView {
    #[default]
    All,
    Mentions,
}

impl NotifView {
    pub const ALL: [NotifView; 2] = [NotifView::All, NotifView::Mentions];

    pub fn title(self) -> &'static str {
        match self {
            NotifView::All => "All",
            NotifView::Mentions => "Mentions",
        }
    }

    /// The notification is one this view shows.
    pub fn shows(self, kind: &str) -> bool {
        self == NotifView::All || matches!(kind, "mention" | "reply" | "quote")
    }
}

/// Notifications as the social sites list them: the reactions to one post, its reposts, and new followers, each as
/// one row where the newest of them stands, its people newest first; everything else a row of its own. `items`
/// come newest first.
pub fn grouped<'a>(items: impl IntoIterator<Item = &'a Notification>) -> Vec<Vec<&'a Notification>> {
    let mut groups: Vec<Vec<&Notification>> = Vec::new();
    let mut at: HashMap<(&str, Option<&str>), usize> = HashMap::new();
    for n in items {
        let key = match n.kind.as_str() {
            "like" | "repost" => Some((n.kind.as_str(), n.post.as_ref().map(|p| p.id.as_str()))),
            "follow" => Some(("follow", None)),
            _ => None,
        };
        if let Some(k) = key {
            if let Some(&i) = at.get(&k) {
                groups[i].push(n);
                continue;
            }
            at.insert(k, groups.len());
        }
        groups.push(vec![n]);
    }
    groups
}

/// A person's page's tabs: what the service lists (posts, replies, photos, articles), and their professional
/// profile (experience, education, skills), as LinkedIn shows one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PersonTab {
    #[default]
    Posts,
    Replies,
    Media,
    Articles,
    Profile,
}

impl PersonTab {
    pub const ALL: [PersonTab; 5] = [PersonTab::Posts, PersonTab::Replies, PersonTab::Media, PersonTab::Articles, PersonTab::Profile];

    pub fn title(self) -> &'static str {
        match self {
            PersonTab::Posts => "Posts",
            PersonTab::Replies => "Replies",
            PersonTab::Media => "Media",
            PersonTab::Articles => "Articles",
            PersonTab::Profile => "Profile",
        }
    }

    /// What the page is read with; the profile comes with any of them.
    pub fn show(self) -> PersonShow {
        match self {
            PersonTab::Replies => PersonShow::Replies,
            PersonTab::Media => PersonShow::Media,
            PersonTab::Articles => PersonShow::Articles,
            PersonTab::Posts | PersonTab::Profile => PersonShow::Posts,
        }
    }
}

/// The reader's own page's tabs: how the page reads (name, photos, title, biography, organizations), the
/// background (positions, schools, skills), privacy (AI training, muted people), and job applications.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MineTab {
    #[default]
    Profile,
    Background,
    Privacy,
    Applications,
}

impl MineTab {
    pub const ALL: [MineTab; 4] = [MineTab::Profile, MineTab::Background, MineTab::Privacy, MineTab::Applications];
}

/// An organization's page's tabs, as LinkedIn's: its posts, its open jobs, its people, and about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OrgTab {
    #[default]
    Posts,
    Products,
    Jobs,
    People,
    About,
}

impl OrgTab {
    pub const ALL: [OrgTab; 5] = [OrgTab::Posts, OrgTab::Products, OrgTab::Jobs, OrgTab::People, OrgTab::About];

    pub fn title(self) -> &'static str {
        match self {
            OrgTab::Posts => "Posts",
            OrgTab::Products => "Products",
            OrgTab::Jobs => "Jobs",
            OrgTab::People => "People",
            OrgTab::About => "About",
        }
    }
}

/// Which feed Home shows: the ranked one, or that of the people and organizations followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HomeFeed {
    ForYou,
    #[default]
    Following,
}

impl HomeFeed {
    pub const ALL: [HomeFeed; 2] = [HomeFeed::ForYou, HomeFeed::Following];

    pub fn title(self) -> &'static str {
        match self {
            HomeFeed::ForYou => "For you",
            HomeFeed::Following => "Following",
        }
    }
}

/// The reader's profile being written on My page: the entry in each form (an id set when changing one), whether each
/// form is open, and the skills as chips.
#[derive(Default)]
pub struct ProfileForm {
    pub experience: NewExperience,
    pub education: NewEducation,
    /// The forms that add a position or a school are open (one changing an entry is open while it has its id).
    pub exp_open: bool,
    pub edu_open: bool,
    /// The position in the form is held now: it has no end.
    pub exp_now: bool,
    /// The entry whose Delete was pressed once: a second press deletes it.
    pub confirm_delete: Option<String>,
    pub skills: Vec<String>,
    /// The skill being typed, added on Enter (several between commas).
    pub skill_new: String,
    /// The skills differ from what the service last said.
    pub skills_dirty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpField {
    Title,
    Ticker,
    OrgName,
    Started,
    Ended,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EduField {
    School,
    Degree,
    Field,
    Started,
    Ended,
}

/// Which part of the profile a save or a delete answered for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilePart {
    Experience,
    Education,
    Skills,
}

/// `#Tag` typed into the finder, as the service spells a hashtag (lower case, without the #), when it is one.
fn as_tag(typed: &str) -> Option<String> {
    let tag = typed.strip_prefix('#')?;
    let ok = (1..=50).contains(&tag.len())
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && tag.bytes().any(|b| b.is_ascii_alphabetic());
    ok.then(|| tag.to_ascii_lowercase())
}

/// A mention being typed in the composer (`@ad`): where it stands, and the people the service found for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mentioning {
    /// The line it is on, and the byte columns of its `@` and of the cursor after it.
    pub line: usize,
    pub start: usize,
    pub end: usize,
    /// What follows the `@`, as typed.
    pub word: String,
    /// The people with a page who match it, and the one Enter or Tab takes.
    pub found: Vec<Person>,
    pub at: usize,
}

/// The mention being typed at the composer's cursor: its line, the byte columns of its `@` and of the cursor, and what
/// follows the `@` (1 to 39 letters, digits, `.`, `_` or `-`), when the `@` begins a word, as the service reads a
/// mention. None with a selection, or anywhere else.
pub(crate) fn typed_mention(body: &text_editor::Content) -> Option<(usize, usize, usize, String)> {
    let cursor = body.cursor();
    if cursor.selection.is_some() {
        return None;
    }
    let at = cursor.position;
    let line = body.line(at.line)?;
    let before = line.text.get(..at.column)?;
    let start = before.rfind('@')?;
    let word = &before[start + 1..];
    let fits = (1..=39).contains(&word.len()) && word.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let free = before[..start].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '@' | '.')));
    (fits && free).then(|| (at.line, start, at.column, word.to_string()))
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

/// Where a new post goes: the reader's own page, or a community they are a member of (its handle).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InWhere(pub Option<String>);

impl std::fmt::Display for InWhere {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => write!(f, "My page"),
            Some(handle) => write!(f, "c/{handle}"),
        }
    }
}

/// A field of the form that starts a community, or of the form its owners edit it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommunityField {
    Handle,
    Name,
    About,
    Rules,
}

/// A community being started (or, without its handle, edited by an owner).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewCommunity {
    pub handle: String,
    pub name: String,
    pub about: String,
    pub rules: String,
    pub joining: Joining,
}

impl NewCommunity {
    /// As the service reads a handle: trimmed, lower case, without a typed `c/`, letters, digits and `_` only.
    fn tidy_handle(typed: &str) -> String {
        let lower = typed.trim().to_ascii_lowercase();
        let bare = lower.strip_prefix("c/").unwrap_or(&lower);
        bare.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(30).collect()
    }

    /// Ready to start: a handle of 3-30 beginning with a letter, and a name.
    pub fn ready(&self) -> bool {
        (3..=30).contains(&self.handle.len())
            && self.handle.starts_with(|c: char| c.is_ascii_lowercase())
            && !self.name.trim().is_empty()
    }
}

/// What the Products tab shows: the day's or the week's launches, the newest products, the reader's own, or those
/// they follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProductsView {
    #[default]
    Today,
    Week,
    Newest,
    Yours,
    Following,
}

impl ProductsView {
    pub const ALL: [ProductsView; 5] = [ProductsView::Today, ProductsView::Week, ProductsView::Newest, ProductsView::Yours, ProductsView::Following];

    pub fn title(self) -> &'static str {
        match self {
            ProductsView::Today => "Today",
            ProductsView::Week => "This week",
            ProductsView::Newest => "Newest",
            ProductsView::Yours => "Yours",
            ProductsView::Following => "Following",
        }
    }

    /// Launches, or products.
    pub fn launches(self) -> bool {
        matches!(self, ProductsView::Today | ProductsView::Week)
    }

    fn list(self) -> ProductList {
        match self {
            ProductsView::Yours => ProductList::Mine,
            ProductsView::Following => ProductList::Following,
            _ => ProductList::Newest,
        }
    }
}

/// A field of the form that starts a product, or of the form its keepers edit it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductField {
    Handle,
    Name,
    Tagline,
    About,
    Website,
    Source,
    /// "Built with", comma-separated.
    Tech,
}

/// A product being started (or, without its handle and owner, edited by its keepers).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProductForm {
    pub handle: String,
    pub name: String,
    pub tagline: String,
    pub about: String,
    pub website: String,
    pub source: String,
    pub tech: String,
    pub platforms: Vec<&'static str>,
    pub stage: Stage,
    /// The organization that owns it (a ticker), else the reader.
    pub as_org: Option<String>,
}

impl ProductForm {
    /// As the service reads a handle: trimmed, lower case, without a typed `p/`, letters, digits and `-` only.
    fn tidy_handle(typed: &str) -> String {
        let lower = typed.trim().to_ascii_lowercase();
        let bare = lower.strip_prefix("p/").unwrap_or(&lower);
        bare.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect()
    }

    /// Its technologies, as typed between commas, each once.
    pub fn tech_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for part in self.tech.split(',').map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|t| !t.is_empty()) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&part)) {
                out.push(part.chars().take(pages::PRODUCT_TECH_MAX_CHARS).collect());
            }
        }
        out
    }

    /// Why it cannot be sent yet, in words; None when it can.
    pub fn why_not(&self, starting: bool) -> Option<String> {
        let link = |l: &str| l.trim().is_empty() || l.trim().starts_with("https://");
        if starting
            && !((3..=40).contains(&self.handle.len()) && !self.handle.starts_with('-') && !self.handle.ends_with('-'))
        {
            Some("A handle is 3-40 letters, digits or -, beginning and ending with a letter or digit".into())
        } else if self.name.trim().is_empty() {
            Some("Name it".into())
        } else if !link(&self.website) || !link(&self.source) {
            Some("A link is an https address".into())
        } else if self.tech_list().len() > pages::MAX_PRODUCT_TECH {
            Some(format!("At most {} technologies", pages::MAX_PRODUCT_TECH))
        } else {
            None
        }
    }

    fn of(page: &ProductPage) -> ProductForm {
        ProductForm {
            name: page.summary.product.name.clone(),
            tagline: page.summary.tagline.clone(),
            about: page.about.clone(),
            website: page.website.clone(),
            source: page.source.clone(),
            tech: page.summary.tech.join(", "),
            platforms: pages::PLATFORMS.into_iter().filter(|p| page.platforms.iter().any(|q| q == p)).collect(),
            stage: page.summary.stage,
            ..ProductForm::default()
        }
    }

    fn new_product(&self) -> NewProduct {
        NewProduct {
            handle: self.handle.clone(),
            name: self.name.trim().to_string(),
            tagline: self.tagline.trim().to_string(),
            about: self.about.trim().to_string(),
            website: self.website.trim().to_string(),
            source: self.source.trim().to_string(),
            platforms: self.platforms.iter().map(|p| p.to_string()).collect(),
            tech: self.tech_list(),
            stage: self.stage,
            as_org: self.as_org.clone(),
        }
    }

    fn changes(&self) -> ProductChanges {
        ProductChanges {
            name: Some(self.name.trim().to_string()),
            tagline: Some(self.tagline.trim().to_string()),
            about: Some(self.about.trim().to_string()),
            website: Some(self.website.trim().to_string()),
            source: Some(self.source.trim().to_string()),
            platforms: Some(self.platforms.iter().map(|p| p.to_string()).collect()),
            tech: Some(self.tech_list()),
            stage: Some(self.stage),
        }
    }
}

/// What is being written on the product on show: feedback (with its category) or a review (with its stars).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Giving {
    Feedback(FeedbackCategory, String),
    Review(u8, String),
}

impl Giving {
    pub fn text(&self) -> &str {
        match self {
            Giving::Feedback(_, text) | Giving::Review(_, text) => text,
        }
    }

    fn about(&self) -> About {
        match self {
            Giving::Feedback(category, _) => About::Feedback(*category),
            Giving::Review(stars, _) => About::Review(*stars),
        }
    }
}

/// A product the reader keeps, as the composer's About offers it; None is nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AboutProduct(pub Option<(String, String)>);

impl std::fmt::Display for AboutProduct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => write!(f, "No product"),
            Some((_, name)) => write!(f, "{name}"),
        }
    }
}

/// Choose a platform (true) or take it back, keeping the service's order.
fn toggle_platform(chosen: &mut Vec<&'static str>, platform: &'static str, on: bool) {
    chosen.retain(|p| *p != platform);
    if on {
        chosen.push(platform);
    }
    chosen.sort_by_key(|p| pages::PLATFORMS.iter().position(|q| q == p));
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
    /// Who may reply to the post.
    pub replies: ReplyRule,
    /// A poll is being written: up to four options, and how long it stays open.
    pub poll_on: bool,
    pub poll_options: [String; 4],
    pub poll_length: PollLength,
    /// The article is a job opening (posted as an organization).
    pub job_on: bool,
    pub job: NewJob,
    pub job_days: JobDays,
    /// Up to four photos, uploaded as they were chosen; never with a poll.
    pub photos: Vec<Attached>,
    /// The community to post in (a handle), when not on the reader's own page.
    pub in_community: Option<String>,
    /// The draft being written again (ADR-0055): Post posts it, and Save draft rewrites it.
    pub draft_id: Option<String>,
    /// The post that draft quotes, until it is read into `quoting` (or when it is not there to read).
    pub draft_quote: Option<String>,
    /// The schedule is being chosen: a day and a time, in UTC as CENTCOM shows every time.
    pub schedule_on: bool,
    pub schedule_day: String,
    pub schedule_time: String,
    /// The product it is about, one the reader keeps (its handle) (ADR-0056); and whether it launches a version, with
    /// the version.
    pub product: Option<String>,
    pub launch_on: bool,
    pub launch_version: String,
}

/// How long a poll stays open, as the composer offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollLength(pub u32);

impl Default for PollLength {
    fn default() -> Self {
        PollLength(24)
    }
}

impl PollLength {
    pub const ALL: [PollLength; 4] = [PollLength(1), PollLength(24), PollLength(72), PollLength(168)];
}

impl std::fmt::Display for PollLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            1 => write!(f, "1 hour"),
            24 => write!(f, "1 day"),
            h if h % 24 == 0 => write!(f, "{} days", h / 24),
            h => write!(f, "{h} hours"),
        }
    }
}

/// How long a job stays open, as the composer offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobDays(pub u32);

impl Default for JobDays {
    fn default() -> Self {
        JobDays(30)
    }
}

impl JobDays {
    pub const ALL: [JobDays; 5] = [JobDays(7), JobDays(14), JobDays(30), JobDays(60), JobDays(90)];
}

impl std::fmt::Display for JobDays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            7 => write!(f, "1 week"),
            14 => write!(f, "2 weeks"),
            d => write!(f, "{d} days"),
        }
    }
}

/// The Jobs tab's workplace filter: one workplace, or any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkplacePick(pub Option<Workplace>);

impl WorkplacePick {
    pub const ALL: [WorkplacePick; 4] = [
        WorkplacePick(None),
        WorkplacePick(Some(Workplace::OnSite)),
        WorkplacePick(Some(Workplace::Hybrid)),
        WorkplacePick(Some(Workplace::Remote)),
    ];
}

impl std::fmt::Display for WorkplacePick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.map_or("Any workplace", Workplace::label))
    }
}

/// The Jobs tab's filter on the kind of employment: one kind, or any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmploymentPick(pub Option<Employment>);

impl EmploymentPick {
    pub const ALL: [EmploymentPick; 6] = [
        EmploymentPick(None),
        EmploymentPick(Some(Employment::FullTime)),
        EmploymentPick(Some(Employment::PartTime)),
        EmploymentPick(Some(Employment::Contract)),
        EmploymentPick(Some(Employment::Temporary)),
        EmploymentPick(Some(Employment::Internship)),
    ];
}

impl std::fmt::Display for EmploymentPick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.map_or("Any kind", Employment::label))
    }
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
            && (!self.poll_on || self.poll_choices().len() >= 2)
            && (!self.job_wanted() || self.job.workplace == Workplace::Remote || !self.job.location.trim().is_empty())
    }

    /// Why the composer cannot post yet, as its Post button says under the pointer (UX rule C5); None when it can
    /// (`ready`).
    pub fn why_not(&self) -> Option<String> {
        let n = self.chars();
        let titled = (1..=pages::ARTICLE_TITLE_MAX_CHARS).contains(&self.title.trim().chars().count());
        if self.sending {
            Some("Posting\u{2026}".into())
        } else if n == 0 {
            Some("Write something first".into())
        } else if n > self.limit() {
            Some(format!("{} characters over the limit of {}", n - self.limit(), self.limit()))
        } else if self.kind == Kind::Article && !titled {
            Some(format!("Give the article a title of at most {} characters", pages::ARTICLE_TITLE_MAX_CHARS))
        } else if self.poll_on && self.poll_choices().len() < 2 {
            Some("A poll needs at least two options".into())
        } else if self.job_wanted() && self.job.workplace != Workplace::Remote && self.job.location.trim().is_empty() {
            Some("Say where the job is, or make it remote".into())
        } else if self.launches() && self.launch_version.trim().is_empty() {
            Some("Say which version it launches".into())
        } else {
            None
        }
    }

    /// The poll's options as typed, without the empty ones.
    fn poll_choices(&self) -> Vec<String> {
        self.poll_options.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect()
    }

    /// A launch goes with a post about a product, not a quote.
    pub fn launches(&self) -> bool {
        self.launch_on && self.product.is_some() && !self.quotes()
    }

    /// A job opening goes with an article posted as an organization only.
    fn job_wanted(&self) -> bool {
        self.job_on && self.kind == Kind::Article && self.as_who.is_some()
    }

    /// A poll goes with a post of its own (not an article, not a quote) only.
    fn poll_wanted(&self) -> bool {
        self.poll_on && self.kind == Kind::Post && !self.quotes()
    }

    /// It quotes a post: one on show, or the one a draft being written again names.
    pub fn quotes(&self) -> bool {
        self.quoting.is_some() || self.draft_quote.is_some()
    }

    /// Ready to keep as a draft: as for posting, but not a job opening, which is posted at once.
    pub fn draftable(&self) -> bool {
        self.ready() && !self.job_wanted()
    }

    /// The time the schedule names, as RFC 3339 and in seconds: a day as 2026-10-15 and a time as 09:30, in UTC.
    pub fn schedule_at(&self) -> Option<(String, f64)> {
        let (h, m) = self.schedule_time.trim().split_once(':')?;
        let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
        let day = self.schedule_day.trim();
        if h > 23 || m > 59 || day.len() != 10 {
            return None;
        }
        let at = format!("{day}T{h:02}:{m:02}:00Z");
        // A day that does not exist (the 31st of a short month) reads as another: refused.
        crate::utc::parse(&at).filter(|s| crate::utc::full(*s).starts_with(day)).map(|s| (at, s))
    }

    /// The composer holding a draft again, to post, rewrite or schedule.
    fn from_draft(draft: &Draft) -> Composer {
        let p = &draft.post;
        let mut composer = Composer {
            kind: p.kind,
            title: p.title.clone().unwrap_or_default(),
            body: text_editor::Content::with_text(&p.text),
            as_who: p.as_org.clone(),
            replies: p.replies,
            photos: draft
                .photos
                .iter()
                .enumerate()
                .map(|(i, photo)| Attached {
                    id: photo.id.clone(),
                    url: photo.url.clone().unwrap_or_default(),
                    name: format!("Photo {}", i + 1),
                    alt: photo.alt.clone(),
                })
                .collect(),
            in_community: p.community.clone(),
            draft_id: Some(draft.id.clone()),
            draft_quote: p.repost_of.clone(),
            product: p.product.clone(),
            launch_on: matches!(p.about, About::Launch(_)),
            launch_version: match &p.about {
                About::Launch(version) => version.clone(),
                _ => String::new(),
            },
            ..Composer::default()
        };
        if let Some(poll) = &p.poll {
            composer.poll_on = true;
            for (slot, option) in composer.poll_options.iter_mut().zip(&poll.options) {
                slot.clone_from(option);
            }
            composer.poll_length = PollLength(poll.hours);
        }
        if let Some(at) = draft.publish_at.as_deref().and_then(crate::utc::parse) {
            let full = crate::utc::full(at);
            composer.schedule_on = true;
            composer.schedule_day = full[..10].to_string();
            composer.schedule_time = full[11..16].to_string();
        }
        composer
    }

    fn new_post(&self) -> NewPost {
        let poll = self.poll_wanted().then(|| NewPoll { options: self.poll_choices(), hours: self.poll_length.0 });
        let media = if poll.is_some() {
            Vec::new()
        } else {
            self.photos.iter().map(|p| NewPhoto { id: p.id.clone(), alt: p.alt.trim().to_string() }).collect()
        };
        NewPost {
            kind: self.kind,
            text: self.body.text().trim().to_string(),
            title: (self.kind == Kind::Article).then(|| self.title.trim().to_string()),
            reply_to: None,
            repost_of: self.quoting.as_ref().map(|p| p.id.clone()).or_else(|| self.draft_quote.clone()),
            as_org: self.as_who.clone(),
            replies: self.replies,
            poll,
            job: self.job_wanted().then(|| NewJob { days: self.job_days.0, ..self.job.clone() }),
            media,
            // A community post is one's own, not an organization's, and not a quote.
            community: self.in_community.clone().filter(|_| self.as_who.is_none() && !self.quotes()),
            // A post about a product is one's own top-level post or article: not a quote, a community's or a job.
            product: self.product.clone().filter(|_| !self.quotes() && self.in_community.is_none() && !self.job_wanted()),
            about: if self.launches() { About::Launch(self.launch_version.trim().to_string()) } else { About::Update },
        }
    }
}

/// The reader's own page as it is being edited.
#[derive(Default)]
pub struct Mine {
    pub name: String,
    pub title: String,
    pub title_org: Option<Org>,
    pub bio: text_editor::Content,
    /// Something differs from what the service last said.
    pub dirty: bool,
    pub saving: bool,
    /// "Close my page" was pressed once: the page asks before it closes.
    pub confirm_close: bool,
    /// The tab on show.
    pub tab: MineTab,
    /// The reader's page as others see it, for the top drawn over the fields (its banner and totals).
    pub top: Option<PersonPage>,
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
    pub flagged: Option<(String, Posts)>,
    pub notifications: Option<Notifications>,
    pub bookmarks: Option<Posts>,
    /// Home's feed, and the ranked one with the number of its last page read.
    pub home_feed: HomeFeed,
    pub for_you: Option<(u32, Posts)>,
    /// Explore's side: the trending hashtags, and who to follow.
    pub trending: Option<Trending>,
    pub suggestions: Option<Suggestions>,
    /// The hashtag on show and its posts.
    pub tagged: Option<Tagged>,
    /// Which part of a search to read, and what the search on show found (its query and part).
    pub search_kind: SearchKind,
    pub found: Option<(String, SearchKind, Found)>,
    /// My page's profile forms.
    pub profile_form: ProfileForm,
    /// Owners and admins only: an organization's experience entries to confirm.
    pub org_experience: Option<(String, Vec<(Person, Experience)>)>,
    /// Whom the reader muted (read with My page).
    pub muted: Option<Vec<Person>>,
    /// The Jobs tab: open jobs, and what narrows them.
    pub jobs: Option<Posts>,
    pub jobs_query: JobsQuery,
    /// The job being applied for (its post's id) and the note.
    pub applying: Option<(String, String)>,
    /// Owners and admins only: who applied for a job (its post's id).
    pub applicants: Option<(String, Vec<Applicant>)>,
    /// The jobs the reader applied for (read with My page).
    pub applications: Option<Vec<Post>>,
    pub composer: Composer,
    pub reply: String,
    pub find: String,
    pub mine: Mine,
    pub new_org: NewOrg,
    pub invite: String,
    /// A report whose reason is being chosen.
    pub reporting: Option<Target>,
    /// Moderators only: the part of the moderation screen on show, the reports or the log it holds (and whether more
    /// remain), and the note the next act carries into the log.
    pub mod_view: ModView,
    pub mod_reports: Option<(Vec<ModReport>, bool)>,
    pub mod_log: Option<(Vec<ModAction>, bool)>,
    pub mod_note: String,
    /// Moderators only: the screens' matches not yet cleared.
    pub mod_matches: Option<Vec<MediaMatch>>,
    /// The photos fetched for what the screens hold, by address.
    pub photos: HashMap<String, Picture>,
    /// Photo uploads under way.
    pub uploading: usize,
    /// Communities (ADR-0053): those with the most members, and the reader's own (with those they asked to join).
    pub communities: Option<Vec<CommunitySummary>>,
    pub my_communities: Option<Vec<CommunitySummary>>,
    /// The community on show, and a members list (its handle with it).
    pub community: Option<CommunityPage>,
    pub community_members: Option<(String, Vec<Member>)>,
    /// A community being started, and the edit its owners make of the one on show (Some while that form is open).
    pub new_community: NewCommunity,
    pub edit_community: Option<NewCommunity>,
    /// Lists (ADR-0054): the reader's own, the one on show with its members (when asked for), a new one's name
    /// and line, the edit of the one on show (its id, name and line; Some while that form is open), the list a press
    /// of Delete waits to confirm, and the lists offered for a person or organization.
    pub lists: Option<Vec<PageList>>,
    pub list: Option<ListPage>,
    pub list_members: Option<(String, ListMembers)>,
    pub new_list: (String, String),
    pub edit_list: Option<(String, String, String)>,
    pub confirm_delete_list: Option<String>,
    pub list_picker: Option<(ListTarget, Vec<PageList>)>,
    /// Drafts (ADR-0055): the reader's own, and the one a press of Delete waits to confirm.
    pub drafts: Option<Vec<Draft>>,
    pub confirm_delete_draft: Option<String>,
    /// The window's width, as the app last saw it: wide enough, the page is three columns (navigation, the feed,
    /// and search with what is trending), as the social sites lay theirs out. None until the app tells it.
    pub width: Option<f32>,
    /// The post whose ⋯ menu, repost choice or reactions are open (its id): one at a time.
    pub post_menu: Option<String>,
    pub repost_menu: Option<String>,
    pub reacting: Option<String>,
    /// All notifications, or the mentions.
    pub notif_view: NotifView,
    /// The page last asked for by scrolling to the end (its key, `more_key`): asked once.
    pub more_asked: Option<String>,
    /// The tab on show on a person's and an organization's page, the organization's open jobs (its ticker with
    /// them), and whether the page's own ⋯ menu is open.
    pub person_tab: PersonTab,
    pub org_tab: OrgTab,
    pub org_jobs: Option<(String, Posts)>,
    /// The products of the organization on show (ADR-0056), with its ticker.
    pub org_products: Option<(String, Products)>,
    pub page_menu: bool,
    /// The post the keyboard is on, by id (`keys`: j and k move it); none until j or k is pressed on this screen.
    pub selected: Option<String>,
    /// r opened a conversation: its reply field takes the keyboard once it is read.
    pub reply_focus: bool,
    /// The photo open over the page: its post's id and its place among the post's photos.
    pub viewing: Option<(String, usize)>,
    /// A mention being typed in the composer, with the people who match it (`typed_mention`).
    pub mentioning: Option<Mentioning>,
    /// Builders (ADR-0056): the Products tab's part on show; the day's launches (the rail's too) and the week's; the
    /// products a list part holds (with the part they answer); the products the reader keeps (the composer's About);
    /// the product on show, the part of its page on show, its feedback board's order and status, its makers and
    /// testers when asked for; a product being started, the edit of the one on show, and what is being written on it;
    /// the note of an ask to test, the username of a maker being listed, the note of an invitation to test; the
    /// product a press of Delete waits to confirm.
    pub products_view: ProductsView,
    pub launches_today: Option<Vec<Post>>,
    pub launches_week: Option<Vec<Post>>,
    pub product_list: Option<(ProductsView, Products)>,
    pub my_products: Option<Vec<pages::ProductSummary>>,
    pub product: Option<ProductPage>,
    pub product_show: ProductShow,
    pub feedback_sort: FeedbackSort,
    pub feedback_status: Option<FeedbackStatus>,
    pub product_makers: Option<(String, Vec<ProductMaker>)>,
    pub product_testers: Option<(String, Vec<Tester>)>,
    pub new_product: ProductForm,
    pub edit_product: Option<ProductForm>,
    pub giving: Option<Giving>,
    pub test_note: String,
    pub maker_name: String,
    pub tester_note: String,
    pub confirm_delete_product: Option<String>,
}

/// The public website's address of a post, as Copy link copies it.
pub fn post_link(post_id: &str) -> String {
    format!("https://www.alelyon.com/pages/?post={post_id}")
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
    Flagged(String, bool, Result<Posts, Problem>),
    /// A page of notifications; `append` adds it under those on show.
    Notifications(bool, Result<Notifications, Problem>),
    /// How many notifications stay unread after some were marked read.
    NotificationsRead(Result<u64, Problem>),
    /// A page of the reader's bookmarks; `append` adds it under those on show.
    Bookmarks(bool, Result<Posts, Problem>),
    /// Save (true) or forget a post.
    Bookmark(String, bool),
    Pin(String),
    Unpin,
    /// Home shows the ranked feed or the followed one.
    HomeFeed(HomeFeed),
    /// A page of the ranked feed and its number; a page after the first adds under those on show.
    ForYou(u32, Result<Posts, Problem>),
    Trending(Result<Trending, Problem>),
    Suggestions(Result<Suggestions, Problem>),
    /// A page of a hashtag's posts; `append` adds it under those on show.
    Tagged(String, bool, Result<Tagged, Problem>),
    /// Which part of the search on show to read.
    SearchKind(SearchKind),
    /// What a search found, with the query and part it answers; `append` adds its posts under those on show.
    Found(String, SearchKind, bool, Result<Found, Problem>),
    // the professional profile
    ExpField(ExpField, String),
    /// Write this entry into the experience form, to change it.
    ExpEdit(Experience),
    EduField(EduField, String),
    EduEdit(Education),
    /// Empty both forms.
    ProfileClear,
    SaveExperience,
    DeleteExperience(String),
    SaveEducation,
    DeleteEducation(String),
    /// The skill being typed.
    SkillNew(String),
    /// Add what is typed to the skills (several between commas).
    SkillAdd,
    /// Take off the skill at this place.
    SkillRemove(usize),
    SaveSkills,
    /// Open the form that adds a position (true) or a school (false).
    EntryOpen(bool),
    /// The position in the form is held now (true): it has no end.
    ExpNow(bool),
    /// A save or delete of that part answered.
    ProfileSaved(ProfilePart, Result<(), Problem>),
    /// Endorse (true) another's skill, or take that back: username, skill.
    Endorse(String, String, bool),
    /// Confirm (true) an entry naming the organization, or take that back: ticker, entry.
    ConfirmExperience(String, String, bool),
    OrgExperience(String, Result<Vec<(Person, Experience)>, Problem>),
    // X's controls
    Replies(ReplyRule),
    PollToggle,
    PollOption(usize, String),
    PollHours(PollLength),
    /// Vote for an option (0 first) in a post's poll.
    Vote(String, usize),
    /// Mute (true) or unmute someone, by username.
    Mute(String, bool),
    Muted(Result<Vec<Person>, Problem>),
    // Jobs
    Jobs(bool, Result<Posts, Problem>),
    JobsWords(String),
    JobsWorkplace(WorkplacePick),
    JobsEmployment(EmploymentPick),
    JobsSearch,
    JobToggle,
    JobLocation(String),
    JobWorkplace(Workplace),
    JobEmployment(Employment),
    JobPay(String),
    JobDays(JobDays),
    /// Open (a job's post id) or close the note box for applying.
    Applying(Option<String>),
    ApplyNote(String),
    Apply,
    /// Withdraw an application (a job's post id).
    Withdraw(String),
    Applicants(String, Result<Vec<Applicant>, Problem>),
    CloseJob(String),
    Applications(Result<Vec<Post>, Problem>),
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
    /// React to a post another way (or as a like).
    React(String, Reaction),
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
    MineName(String),
    /// A tab of the reader's own page chosen.
    MineTab(MineTab),
    /// Allow (true) or opt out of the use of one's public posts to improve Alelyon, AI models included.
    AiTraining(bool),
    MineTitle(String),
    MineTitleOrg(TitleOrg),
    MineBio(text_editor::Action),
    SaveMine,
    /// Put back in My page's fields what the service last said.
    DiscardMine,
    /// The reader's page as others see it, for My page's top.
    MineTop(Result<PersonPage, Problem>),
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
    // moderation (moderators only)
    ModView(ModView),
    /// A page of reports for that view; `append` adds it under those on show.
    ModReports(ModView, bool, Result<(Vec<ModReport>, bool), Problem>),
    ModLog(bool, Result<(Vec<ModAction>, bool), Problem>),
    ModNote(String),
    /// Act on a target, named directly, through a report, or both.
    ModAct(ModAct, Option<Target>, Option<String>),
    ModDismiss(String),
    ModDone(Result<ModAction, Problem>),
    /// The screens' matches, and marking one reported or clearing it (match id, reported, cleared).
    MediaMatches(Result<Vec<MediaMatch>, Problem>),
    MarkMatch(String, bool, bool),
    // photos
    /// Choose photos: to attach to the post being written, or as a page's or an organization's photo or banner.
    PickPhotos(PhotoFor),
    /// The files chosen, each read.
    PhotosChosen(PhotoFor, Chosen),
    /// An upload answered: what it is for, the file's name, and the upload with its picture.
    Uploaded(PhotoFor, String, Result<(Upload, Picture), Problem>),
    /// The description of a photo attached to the post being written.
    PhotoAlt(usize, String),
    RemovePhoto(usize),
    /// Clear a page's or an organization's photo or banner.
    ClearPhoto(PhotoFor),
    /// A photo arrived for its address.
    Photo(String, Picture),
    // communities (ADR-0053)
    Communities(Result<Vec<CommunitySummary>, Problem>),
    MyCommunities(Result<Vec<CommunitySummary>, Problem>),
    /// A page of a community; `append` adds its posts under those on show.
    Community(String, bool, Result<CommunityPage, Problem>),
    CommunityMembers(String, Result<Vec<Member>, Problem>),
    NewCommunityField(CommunityField, String),
    NewCommunityJoining(Joining),
    CreateCommunity,
    CommunityCreated(Result<Community, Problem>),
    JoinCommunity(String),
    LeaveCommunity(String),
    /// Approve (true) or refuse a request: handle, member.
    DecideRequest(String, String, bool),
    /// Owners: make a member `member`, `moderator` or `owner`: handle, member, role.
    CommunityRole(String, String, &'static str),
    /// Remove a member, or ban them too (true), or lift a ban (a banned member, false): handle, member, ban.
    RemoveFromCommunity(String, String, bool),
    /// Take a conversation out of its community (its first post).
    TakeOut(String),
    InCommunity(InWhere),
    /// Open (true) or close the edit form of the community on show (for its owners).
    EditCommunity(bool),
    EditCommunityField(CommunityField, String),
    EditCommunityJoining(Joining),
    SaveCommunity,
    /// Moderators: suspend (true) or restore a community, by handle.
    ModCommunity(String, bool),
    // lists (ADR-0054)
    Lists(Result<Vec<PageList>, Problem>),
    /// A page of a list's stream; `append` adds its posts under those on show.
    ListPage(String, bool, Result<ListPage, Problem>),
    /// Show (true) or hide who is on the list on show.
    ShowListMembers(bool),
    ListMembers(String, Result<ListMembers, Problem>),
    NewListName(String),
    NewListAbout(String),
    CreateList,
    ListCreated(Result<PageList, Problem>),
    /// Open (true) or close the edit of the list on show.
    EditList(bool),
    EditListName(String),
    EditListAbout(String),
    SaveList,
    /// Delete a list: the first press asks, the second deletes.
    DeleteList(String),
    /// Offer the reader's lists for a person or organization (Some), or close the offer.
    PickLists(Option<ListTarget>),
    ListsFor(ListTarget, Result<Vec<PageList>, Problem>),
    /// Put the target on the list (true) or take it off: list id, target, add.
    ListAdd(String, ListTarget, bool),
    /// Take someone off the list on show (its stream and members are read again).
    ListTakeOff(String, ListTarget),
    // drafts and scheduled posts (ADR-0055)
    /// Keep what the composer holds as a draft (rewriting the draft it holds).
    SaveDraft,
    /// Show (true) or hide the schedule's day and time.
    ScheduleOn(bool),
    ScheduleDay(String),
    ScheduleTime(String),
    /// Keep what the composer holds as a draft to be posted at the schedule's time.
    Schedule,
    DraftSaved(Result<Draft, Problem>),
    Drafts(Result<Vec<Draft>, Problem>),
    /// Write a draft again in the composer, on Home.
    OpenDraft(String),
    /// The post a draft written again quotes, read for the composer: the draft's id with it.
    DraftQuote(String, Result<Thread, Problem>),
    /// Post a draft now, from Drafts.
    PostDraft(String),
    /// Delete a draft: the first press asks, the second deletes.
    DeleteDraft(String),
    /// Stop writing the draft again: the composer empties, and the saved draft stays.
    CloseDraft,
    // builders (ADR-0056)
    ProductsView(ProductsView),
    /// The day's (false) or the week's (true) launches.
    Launches(bool, Result<Vec<Post>, Problem>),
    /// A page of products for that part; `append` adds it under those on show.
    ProductList(ProductsView, bool, Result<Products, Problem>),
    MyProducts(Result<Products, Problem>),
    /// A page of a product; `append` adds its posts under those on show.
    Product(String, bool, Result<ProductPage, Problem>),
    ProductShow(ProductShow),
    FeedbackSort(FeedbackSort),
    FeedbackFilter(Option<FeedbackStatus>),
    NewProductField(ProductField, String),
    NewProductStage(Stage),
    NewProductPlatform(&'static str, bool),
    NewProductOwner(AsWho),
    CreateProduct,
    ProductCreated(Result<pages::Product, Problem>),
    /// Open (true) or close the edit form of the product on show (for its keepers).
    EditProduct(bool),
    EditProductField(ProductField, String),
    EditProductStage(Stage),
    EditProductPlatform(&'static str, bool),
    SaveProduct,
    /// Delete a product: the first press asks, the second deletes.
    DeleteProduct(String),
    /// The product on show is gone (deleted, or suspended by a moderator): what to say, on the Products tab.
    ProductGone(Result<String, Problem>),
    FollowProduct(String, bool),
    /// Support a launch (true), or take the support back: its post's id.
    SupportLaunch(String, bool),
    /// Vote for a piece of feedback (true), or take the vote back.
    VoteFeedback(String, bool),
    SetFeedbackStatus(String, FeedbackStatus),
    /// Change one's review's stars.
    ReviewStars(String, u8),
    /// Start writing feedback or a review on the product on show (Some), or stop.
    Give(Option<Giving>),
    GiveText(String),
    GiveCategory(FeedbackCategory),
    GiveStars(u8),
    SendGiving,
    /// Ask to test the product on show (true, with the note), or withdraw: its handle.
    AskToTest(String, bool),
    TestNote(String),
    /// Show (Some, its handle) or hide the product's makers, and its testers for its keepers.
    ShowTeam(Option<String>),
    ProductMakers(String, Result<Vec<ProductMaker>, Problem>),
    ProductTesters(String, Result<Vec<Tester>, Problem>),
    MakerName(String),
    InviteMaker(String),
    /// Confirm (true) or decline being listed as a maker: the product's handle.
    AnswerMaker(String, bool),
    /// Take a maker off, or leave: handle, member.
    RemoveMaker(String, String),
    TesterNote(String),
    /// Invite (true) someone who asked to test, or decline the ask: handle, member.
    AnswerTester(String, String, bool),
    /// The composer's product (for its keepers' updates and launches), launching, and the version.
    PostAbout(AboutProduct),
    LaunchOn(bool),
    LaunchVersion(String),
    /// Moderators: suspend (true) or restore a product, by handle.
    ModProduct(String, bool),
    /// The products of an organization, by its ticker.
    OrgProducts(String, Result<Products, Problem>),
    /// Open a product's website or source in the browser.
    OpenLink(String),
    // a post's menus and the navigation's Post button
    /// Open (Some post id) or close a post's ⋯ menu.
    PostMenu(Option<String>),
    /// Open or close a post's choice between Repost and Quote.
    RepostMenu(Option<String>),
    /// Open or close a post's reactions.
    Reacting(Option<String>),
    /// Copy a post's public link.
    CopyLink(String),
    /// The navigation's Post button: Home, with the composer.
    Compose,
    NotifView(NotifView),
    /// The page was scrolled: how far down, from 0 at the top to 1 at the end.
    Scrolled(f32),
    /// A key of the page's (`keys`).
    Key(keys::PageKey),
    /// Open a post's photo over the page (its post and place), or close it.
    ViewPhoto(Option<(String, usize)>),
    /// The open photo's neighbour among its post's photos: -1 the one before, 1 the one after, round the ends.
    PhotoStep(i32),
    /// The people a mention being typed may name, as the service found them for what it read.
    MentionFound(String, Result<Found, Problem>),
    /// The person after (1) or before (-1) the chosen one among them, round the ends.
    MentionMove(i32),
    /// Write the chosen person's @username in place of what was typed (None: the one the keyboard is on).
    MentionPick(Option<usize>),
    /// Leave the mention as typed.
    MentionClose,
    PersonTab(PersonTab),
    OrgTab(OrgTab),
    /// An organization's open jobs, for its Jobs tab: its ticker with them.
    OrgJobs(String, Result<Posts, Problem>),
    /// Open (true) or close the ⋯ menu of the person or organization on show.
    PageMenu(bool),
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
            Screen::Home if self.home_feed == HomeFeed::ForYou => self.read_for_you(0),
            Screen::Home => self.run(move |c, t| async move { pages::feed(&c, &t, before.as_deref()).await }, move |r| Msg::Feed(true, append, r)),
            Screen::Explore if append => {
                self.run(move |c, t| async move { pages::explore(&c, &t, before.as_deref()).await }, move |r| Msg::Feed(false, append, r))
            }
            Screen::Explore => Task::batch([
                self.run(|c, t| async move { pages::explore(&c, &t, None).await }, |r| Msg::Feed(false, false, r)),
                self.run(|c, t| async move { pages::trending(&c, &t).await }, Msg::Trending),
                self.run(|c, t| async move { pages::suggestions(&c, &t).await }, Msg::Suggestions),
            ]),
            Screen::Tag(tag) => {
                let key = tag.clone();
                self.run(move |c, t| async move { pages::tag(&c, &t, &tag, before.as_deref()).await }, move |r| Msg::Tagged(key.clone(), append, r))
            }
            Screen::OrgExperience(ticker) => {
                let key = ticker.clone();
                self.run(move |c, t| async move { pages::org_experience(&c, &t, &ticker).await }, move |r| Msg::OrgExperience(key.clone(), r))
            }
            Screen::Search(query) => {
                let (key, kind) = (query.clone(), self.search_kind);
                self.run(move |c, t| async move { pages::search(&c, &t, &query, kind, before.as_deref()).await }, move |r| {
                    Msg::Found(key.clone(), kind, append, r)
                })
            }
            Screen::Orgs => self.fetch_me(),
            Screen::Mine => Task::batch([
                self.fetch_me(),
                self.run(|c, t| async move { pages::mutes(&c, &t).await }, Msg::Muted),
                self.run(|c, t| async move { pages::applications(&c, &t).await }, Msg::Applications),
                self.fetch_mine_top(),
            ]),
            Screen::Notifications => self.run(
                move |c, t| async move { pages::notifications(&c, &t, before.as_deref()).await },
                move |r| Msg::Notifications(append, r),
            ),
            Screen::Bookmarks => {
                self.run(move |c, t| async move { pages::bookmarks(&c, &t, before.as_deref()).await }, move |r| Msg::Bookmarks(append, r))
            }
            Screen::Jobs => {
                let query = self.jobs_query.clone();
                self.run(move |c, t| async move { pages::jobs(&c, &t, &query, before.as_deref()).await }, move |r| Msg::Jobs(append, r))
            }
            Screen::Applicants(id) => {
                let key = id.clone();
                self.run(move |c, t| async move { pages::job_applicants(&c, &t, &id).await }, move |r| Msg::Applicants(key.clone(), r))
            }
            Screen::Person(u) => {
                let name = u.clone();
                let show = self.person_tab.show();
                self.run(move |c, t| async move { pages::person(&c, &t, &u, before.as_deref(), show).await }, move |r| Msg::Person(name.clone(), append, r))
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
            Screen::Flagged(ticker) => {
                let key = ticker.clone();
                self.run(move |c, t| async move { pages::org_flagged(&c, &t, &ticker, before.as_deref()).await }, move |r| {
                    Msg::Flagged(key.clone(), append, r)
                })
            }
            Screen::Moderation => self.read_mod(before),
            Screen::Communities => self.run(|c, t| async move { pages::communities(&c, &t, false).await }, Msg::Communities),
            Screen::Community(handle) => {
                let key = handle.clone();
                self.run(move |c, t| async move { pages::community(&c, &t, &handle, before.as_deref()).await }, move |r| {
                    Msg::Community(key.clone(), append, r)
                })
            }
            Screen::CommunityMembers(handle) => {
                let key = handle.clone();
                self.run(move |c, t| async move { pages::community_members(&c, &t, &handle).await }, move |r| {
                    Msg::CommunityMembers(key.clone(), r)
                })
            }
            Screen::Lists => self.run(|c, t| async move { pages::lists(&c, &t, None).await }, Msg::Lists),
            Screen::List(id) => {
                let key = id.clone();
                self.run(move |c, t| async move { pages::list(&c, &t, &id, before.as_deref()).await }, move |r| {
                    Msg::ListPage(key.clone(), append, r)
                })
            }
            Screen::Drafts => self.run(|c, t| async move { pages::drafts(&c, &t).await }, Msg::Drafts),
            Screen::Products => self.read_products(self.products_view, before),
            Screen::Product(handle) => {
                let key = handle.clone();
                let (show, sort, status) = (self.product_show, self.feedback_sort, self.feedback_status);
                self.run(move |c, t| async move { pages::product(&c, &t, &handle, show, sort, status, before.as_deref()).await }, move |r| {
                    Msg::Product(key.clone(), append, r)
                })
            }
        }
    }

    /// The Products tab's part: the day's or the week's launches, or a page of products after `before` (a handle).
    fn read_products(&self, view: ProductsView, before: Option<String>) -> Task<Msg> {
        if view.launches() {
            let week = view == ProductsView::Week;
            return self.run(move |c, t| async move { pages::launches(&c, &t, week, None).await }, move |r| Msg::Launches(week, r));
        }
        let append = before.is_some();
        self.run(
            move |c, t| async move { pages::products(&c, &t, view.list(), None, None, None, before.as_deref()).await },
            move |r| Msg::ProductList(view, append, r),
        )
    }

    /// A product's makers, and for its keepers who asked to test it.
    fn read_team(&self, handle: String) -> Task<Msg> {
        let (h, key) = (handle.clone(), handle.clone());
        let mut tasks = vec![self.run(move |c, t| async move { pages::product_makers(&c, &t, &h).await }, move |r| {
            Msg::ProductMakers(key.clone(), r)
        })];
        if self.product.as_ref().is_some_and(|p| p.summary.product.handle == handle && p.you_keep()) {
            let key = handle.clone();
            tasks.push(self.run(move |c, t| async move { pages::product_testers(&c, &t, &handle).await }, move |r| {
                Msg::ProductTesters(key.clone(), r)
            }));
        }
        Task::batch(tasks)
    }

    /// The moderation screen's view: the open or closed reports, the log, or the photo matches.
    fn read_mod(&self, before: Option<String>) -> Task<Msg> {
        let view = self.mod_view;
        let append = before.is_some();
        if view == ModView::Matches {
            return self.run(|c, t| async move { pages::media_matches(&c, &t, false).await }, Msg::MediaMatches);
        }
        if view == ModView::Log {
            return self.run(move |c, t| async move { pages::mod_history(&c, &t, before.as_deref()).await }, move |r| Msg::ModLog(append, r));
        }
        let closed = view == ModView::Closed;
        self.run(
            move |c, t| async move { pages::mod_queue(&c, &t, closed, before.as_deref()).await },
            move |r| Msg::ModReports(view, append, r),
        )
    }

    /// One page of the ranked feed: 0 is the first.
    fn read_for_you(&self, page: u32) -> Task<Msg> {
        self.run(move |c, t| async move { pages::for_you(&c, &t, page).await }, move |r| Msg::ForYou(page, r))
    }

    fn load(&mut self, screen: &Screen, keep: bool) -> Task<Msg> {
        if !keep {
            self.loading = true;
        }
        let mut tasks = vec![self.read(screen, None)];
        // The reader's own communities: the composer's choices on Home, and what they keep anywhere.
        if matches!(screen, Screen::Home | Screen::Communities | Screen::Community(_)) {
            tasks.push(self.run(|c, t| async move { pages::communities(&c, &t, true).await }, Msg::MyCommunities));
        }
        // The products the reader keeps: what the composer's About offers (ADR-0056).
        if matches!(screen, Screen::Home | Screen::Products | Screen::Product(_)) {
            tasks.push(self.run(|c, t| async move { pages::products(&c, &t, ProductList::Mine, None, None, None, None).await }, Msg::MyProducts));
        }
        // The day's launches beside the feed: read once, and again on Explore (the Products tab's Today reads them).
        if *screen == Screen::Explore || (self.launches_today.is_none() && !(*screen == Screen::Products && self.products_view == ProductsView::Today)) {
            tasks.push(self.run(|c, t| async move { pages::launches(&c, &t, false, None).await }, |r| Msg::Launches(false, r)));
        }
        // What is trending and who to follow, beside the feed: read once, and again on Explore.
        if *screen == Screen::Home && self.trending.is_none() {
            tasks.push(self.run(|c, t| async move { pages::trending(&c, &t).await }, Msg::Trending));
        }
        if *screen == Screen::Home && self.suggestions.is_none() {
            tasks.push(self.run(|c, t| async move { pages::suggestions(&c, &t).await }, Msg::Suggestions));
        }
        Task::batch(tasks)
    }

    /// The reader keeps the community (an active owner or moderator), as their own list says.
    pub fn keeps(&self, handle: &str) -> bool {
        self.my_communities.iter().flatten().any(|c| c.community.handle == handle && c.you_keep())
    }

    /// The person or organization whose page is on show, as a list names it.
    fn list_target_on_show(&self) -> Option<ListTarget> {
        match self.screen.as_ref()? {
            Screen::Person(_) => self.person.as_ref()?.person.username.clone().map(ListTarget::Person),
            Screen::Org(_) => self.org.as_ref().map(|o| ListTarget::Org(o.org.ticker.clone())),
            _ => None,
        }
    }

    /// Keep what the composer holds as a draft, rewriting the one it holds; `publish_at` (RFC 3339) schedules it.
    fn save_draft(&mut self, publish_at: Option<String>) -> Task<Msg> {
        if !self.composer.draftable() || self.uploading > 0 {
            return Task::none();
        }
        self.composer.sending = true;
        let (new, id) = (self.composer.new_post(), self.composer.draft_id.clone());
        self.run(move |c, t| async move { pages::draft_save(&c, &t, id.as_deref(), &new, publish_at.as_deref()).await }, Msg::DraftSaved)
    }

    /// The screen on show, read again (after a change made here).
    fn reload(&mut self) -> Task<Msg> {
        let Some(screen) = self.screen.clone() else { return Task::none() };
        let mut tasks = vec![self.load(&screen, true), self.fetch_me()];
        // An open list of who is on the list on show is read again too.
        if let (Screen::List(id), Some((shown, _))) = (&screen, &self.list_members)
            && shown == id
        {
            let (id, key) = (id.clone(), id.clone());
            tasks.push(self.run(move |c, t| async move { pages::list_members(&c, &t, &id).await }, move |r| Msg::ListMembers(key.clone(), r)));
        }
        // So are the makers and testers of the product on show, while they are open.
        if let (Screen::Product(handle), Some((shown, _))) = (&screen, &self.product_makers)
            && shown == handle
        {
            tasks.push(self.read_team(handle.clone()));
        }
        Task::batch(tasks)
    }

    /// After an edit or a move in the composer: the mention being typed at the cursor, if one is, and the people who
    /// match it, asked for again only when its words changed.
    fn track_mention(&mut self) -> Task<Msg> {
        let Some((line, start, end, word)) = typed_mention(&self.composer.body) else {
            self.mentioning = None;
            return Task::none();
        };
        let (found, at, same) = match self.mentioning.take() {
            Some(m) if m.word == word => (m.found, m.at, true),
            _ => (Vec::new(), 0, false),
        };
        self.mentioning = Some(Mentioning { line, start, end, word: word.clone(), found, at });
        if same {
            return Task::none();
        }
        let asked = word.clone();
        self.run(move |c, t| async move { pages::search(&c, &t, &asked, SearchKind::People, None).await }, move |r| {
            Msg::MentionFound(word.clone(), r)
        })
    }

    /// The chosen person's @username in place of the mention as typed, and a space after it unless one follows already;
    /// the composer keeps the keyboard.
    fn pick_mention(&mut self, which: Option<usize>) -> Task<Msg> {
        let Some(m) = self.mentioning.take() else { return Task::none() };
        let Some(username) = m.found.get(which.unwrap_or(m.at)).and_then(|p| p.username.clone()) else { return Task::none() };
        let spaced = self.composer.body.line(m.line).and_then(|l| l.text.get(m.end..).map(|rest| rest.starts_with(char::is_whitespace))).unwrap_or(false);
        let words = if spaced { format!("@{username}") } else { format!("@{username} ") };
        let (from, to) = (text_editor::Position { line: m.line, column: m.start }, text_editor::Position { line: m.line, column: m.end });
        self.composer.body.move_to(text_editor::Cursor { position: to, selection: Some(from) });
        self.composer.body.perform(text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(words))));
        iced::widget::operation::focus(keys::COMPOSER)
    }

    /// The posts the column shows, in its order, each as its row shows it (a plain repost as the post it reposts):
    /// what j and k move between.
    pub(crate) fn shown_posts(&self) -> Vec<&Post> {
        let mut out: Vec<&Post> = Vec::new();
        let lists: Vec<&Posts> = match self.screen.clone().unwrap_or(Screen::Home) {
            Screen::Home => match self.home_feed {
                HomeFeed::ForYou => self.for_you.iter().map(|(_, p)| p).collect(),
                HomeFeed::Following => self.home.iter().collect(),
            },
            Screen::Explore => self.explore.iter().collect(),
            Screen::Bookmarks => self.bookmarks.iter().collect(),
            Screen::Jobs => self.jobs.iter().collect(),
            Screen::Tag(_) => self.tagged.iter().map(|t| &t.posts).collect(),
            Screen::Flagged(_) => self.flagged.iter().map(|(_, p)| p).collect(),
            Screen::List(_) => self.list.iter().map(|l| &l.posts).collect(),
            Screen::Community(_) => self.community.iter().map(|c| &c.posts).collect(),
            Screen::Product(_) => self.product.iter().map(|p| &p.posts).collect(),
            Screen::Search(q) => self
                .found
                .iter()
                .filter(|(found_q, kind, _)| *found_q == q && matches!(kind, SearchKind::All | SearchKind::Posts) && *kind == self.search_kind)
                .map(|(_, _, found)| &found.posts)
                .collect(),
            Screen::Org(t) => match (self.org.as_ref().filter(|page| page.org.ticker == t), self.org_tab) {
                (Some(page), OrgTab::Posts) => vec![&page.posts],
                (Some(_), OrgTab::Jobs) => self.org_jobs.iter().filter(|(of, _)| *of == t).map(|(_, p)| p).collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        for list in lists {
            out.extend(list.posts.iter());
        }
        match self.screen.clone().unwrap_or(Screen::Home) {
            Screen::Person(_) => {
                if let Some(page) = &self.person
                    && self.person_tab != PersonTab::Profile
                {
                    if self.person_tab == PersonTab::Posts
                        && let Some(pinned) = &page.pinned
                    {
                        out.push(pinned);
                    }
                    out.extend(page.posts.posts.iter());
                }
            }
            Screen::Thread(_) => {
                if let Some(t) = &self.thread {
                    out.extend(t.parent.as_deref());
                    out.push(&t.post);
                    out.extend(t.replies.posts.iter());
                }
            }
            _ => {}
        }
        out.into_iter().map(|p| if p.is_plain_repost() { p.repost_of.as_deref().unwrap_or(p) } else { p }).collect()
    }

    /// What a key of the page's does (`keys`): j and k choose the next or previous post on show and scroll the column
    /// to it; l, r, t, b and o act on the chosen post; n, / and . on the page.
    fn key(&mut self, key: keys::PageKey) -> Task<Msg> {
        use keys::PageKey;
        match key {
            PageKey::NewPost => return self.update(Msg::Compose),
            PageKey::Search => return iced::widget::operation::focus(keys::SEARCH),
            PageKey::Refresh => return self.update(Msg::Refresh),
            PageKey::PhotoPrevious => return self.update(Msg::PhotoStep(-1)),
            PageKey::PhotoNext => return self.update(Msg::PhotoStep(1)),
            _ => {}
        }
        let shown: Vec<(String, bool, bool)> = self.shown_posts().iter().map(|p| (p.id.clone(), p.liked, p.bookmarked)).collect();
        let at = self.selected.as_ref().and_then(|id| shown.iter().position(|(i, _, _)| i == id));
        if matches!(key, PageKey::Next | PageKey::Previous) {
            if shown.is_empty() {
                return Task::none();
            }
            let next = match (at, key) {
                (None, _) => 0,
                (Some(i), PageKey::Next) => (i + 1).min(shown.len() - 1),
                (Some(i), _) => i.saturating_sub(1),
            };
            let id = shown[next].0.clone();
            self.selected = Some(id.clone());
            return iced::advanced::widget::operate(keys::Reveal::new(keys::row_id(&id))).then(|offset| match offset {
                Some(y) => iced::widget::operation::scroll_to(keys::FEED, iced::widget::operation::AbsoluteOffset { x: None, y: Some(y) }),
                None => Task::none(),
            });
        }
        let Some((id, liked, saved)) = at.map(|i| shown[i].clone()) else { return Task::none() };
        match key {
            PageKey::Like => self.update(Msg::Like(id, !liked)),
            PageKey::Bookmark => self.update(Msg::Bookmark(id, !saved)),
            PageKey::Repost => self.update(Msg::RepostMenu(Some(id))),
            PageKey::Open => self.update(Msg::Go(Screen::Thread(id))),
            PageKey::Reply => {
                self.reply_focus = true;
                self.update(Msg::Go(Screen::Thread(id)))
            }
            _ => Task::none(),
        }
    }

    fn go(&mut self, screen: Screen) -> Task<Msg> {
        // Before the screen on show is taken: going to the page already on show keeps its tab.
        self.fresh_tabs(&screen);
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
        self.selected = None;
        self.viewing = None;
        self.problem = None;
        self.edit_community = None;
        self.list_picker = None;
        self.list_members = None;
        self.edit_list = None;
        self.confirm_delete_list = None;
        self.confirm_delete_draft = None;
        self.edit_product = None;
        self.giving = None;
        self.confirm_delete_product = None;
        self.page_menu = false;
        if self.composer.chars() == 0 && !matches!(&screen, Screen::Product(h) if self.composer.product.as_ref() == Some(h)) {
            self.composer.product = None;
            self.composer.launch_on = false;
        }
        self.screen = Some(screen.clone());
        self.load(&screen, false)
    }

    /// A person's or an organization's page opens at its first tab, unless it is the page already on show.
    fn fresh_tabs(&mut self, screen: &Screen) {
        if self.screen.as_ref() == Some(screen) {
            return;
        }
        match screen {
            Screen::Person(_) => self.person_tab = PersonTab::Posts,
            Screen::Org(_) => {
                self.org_tab = OrgTab::Posts;
                self.org_jobs = None;
                self.org_products = None;
            }
            Screen::Product(_) => {
                self.product = None;
                self.product_show = ProductShow::Updates;
                self.feedback_sort = FeedbackSort::Top;
                self.feedback_status = None;
                self.product_makers = None;
                self.product_testers = None;
            }
            _ => {}
        }
    }

    /// What the next page of the screen on show is asked by: For you's page number, or another feed's `before`.
    fn more_key(&self) -> Option<String> {
        if self.screen == Some(Screen::Home) && self.home_feed == HomeFeed::ForYou {
            return self.for_you.as_ref().filter(|(_, shown)| shown.more).map(|(page, _)| format!("for-you:{page}"));
        }
        self.next_page()
    }

    /// The `before` for the next page of the screen on show, when it has more.
    fn next_page(&self) -> Option<String> {
        match self.screen.as_ref()? {
            Screen::Home => self.home.as_ref()?.next(),
            Screen::Explore => self.explore.as_ref()?.next(),
            Screen::Person(_) => self.person.as_ref()?.posts.next(),
            Screen::Org(_) => self.org.as_ref()?.posts.next(),
            Screen::Flagged(_) => self.flagged.as_ref()?.1.next(),
            Screen::Notifications => self.notifications.as_ref()?.next(),
            Screen::Bookmarks => self.bookmarks.as_ref()?.next(),
            Screen::Jobs => self.jobs.as_ref()?.next(),
            Screen::Community(_) => self.community.as_ref()?.posts.next(),
            Screen::Product(_) => self.product.as_ref()?.posts.next(),
            Screen::Products if !self.products_view.launches() => {
                self.product_list.as_ref().filter(|(view, _)| *view == self.products_view)?.1.next()
            }
            Screen::List(_) => self.list.as_ref()?.posts.next(),
            Screen::Tag(_) => self.tagged.as_ref()?.posts.next(),
            Screen::Search(_) if self.search_kind == SearchKind::Posts => self.found.as_ref()?.2.posts.next(),
            Screen::Moderation if self.mod_view == ModView::Matches => None,
            Screen::Moderation if self.mod_view == ModView::Log => {
                let (actions, more) = self.mod_log.as_ref()?;
                more.then(|| actions.last().map(|a| a.id.clone())).flatten()
            }
            Screen::Moderation => {
                let (reports, more) = self.mod_reports.as_ref()?;
                more.then(|| reports.last().map(|r| r.id.clone())).flatten()
            }
            _ => None,
        }
    }

    /// A change with nothing to read back: `words` say what it did.
    fn done<F: Future<Output = Result<(), Problem>> + Send + 'static>(&self, call: impl FnOnce(Client, String) -> F, words: String) -> Task<Msg> {
        self.run(call, move |r| Msg::Done(r.map(|()| words.clone())))
    }

    /// The page's form, refilled from what the service said.
    /// The reader's page as others see it, read for My page's top; nothing until the reader's username is known.
    fn fetch_mine_top(&self) -> Task<Msg> {
        let Some(username) = self.me.as_ref().and_then(|m| m.me.username.clone()) else { return Task::none() };
        self.run(move |c, t| async move { pages::person(&c, &t, &username, None, PersonShow::Posts).await }, Msg::MineTop)
    }

    fn refill_mine(&mut self) {
        let Some(me) = &self.me else { return };
        self.mine.name = me.me.display_name.clone();
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

    /// Uploads a photo the reader chose. Its picture is made from the bytes at hand, so it shows without a fetch.
    fn upload(&self, target: PhotoFor, name: String, bytes: Vec<u8>) -> Task<Msg> {
        self.run(
            move |c, t| async move {
                let upload = pages::upload(&c, &t, &bytes).await?;
                let picture = crate::lattice::off_thread(move || photos::picture(&bytes, Size::Large)).await.unwrap_or(Picture::Failed);
                Ok((upload, picture))
            },
            move |r| Msg::Uploaded(target.clone(), name.clone(), r),
        )
    }

    /// The photos the screen on show draws, each address once, with the size it is drawn at.
    fn photos_on_show(&self) -> Vec<(String, Size)> {
        let mut want = Wanted::default();
        match self.screen.as_ref() {
            Some(Screen::Home) => {
                for p in &self.composer.photos {
                    want.add(Some(&p.url), Size::Large);
                }
                match self.home_feed {
                    HomeFeed::ForYou => want.posts(self.for_you.as_ref().map(|(_, p)| p)),
                    HomeFeed::Following => want.posts(self.home.as_ref()),
                }
            }
            Some(Screen::Explore) => want.posts(self.explore.as_ref()),
            Some(Screen::Bookmarks) => want.posts(self.bookmarks.as_ref()),
            Some(Screen::Jobs) => want.posts(self.jobs.as_ref()),
            Some(Screen::Community(_)) => want.posts(self.community.as_ref().map(|c| &c.posts)),
            Some(Screen::List(_)) => want.posts(self.list.as_ref().map(|l| &l.posts)),
            Some(Screen::Flagged(_)) => want.posts(self.flagged.as_ref().map(|(_, p)| p)),
            Some(Screen::Tag(_)) => want.posts(self.tagged.as_ref().map(|t| &t.posts)),
            Some(Screen::Search(_)) => {
                if let Some((_, _, found)) = &self.found {
                    want.summaries(&found.products);
                    want.posts(Some(&found.posts));
                }
            }
            Some(Screen::Thread(_)) => {
                if let Some(t) = &self.thread {
                    if let Some(parent) = &t.parent {
                        want.post(parent);
                    }
                    want.post(&t.post);
                    want.posts(Some(&t.replies));
                }
            }
            Some(Screen::Person(_)) => {
                if let Some(page) = &self.person {
                    want.add(page.banner.as_ref(), Size::Large);
                    want.add(page.person.avatar.as_ref(), Size::Small);
                    if let Some(p) = &page.pinned {
                        want.post(p);
                    }
                    want.posts(Some(&page.posts));
                }
            }
            Some(Screen::Org(_)) => {
                if let Some(page) = &self.org {
                    want.add(page.banner.as_ref(), Size::Large);
                    want.add(page.org.logo.as_ref(), Size::Small);
                    want.posts(Some(&page.posts));
                }
                if self.org_tab == OrgTab::Products
                    && let Some((_, list)) = &self.org_products
                {
                    want.summaries(&list.products);
                }
            }
            Some(Screen::Products) => match self.products_view {
                ProductsView::Today => want.launches(self.launches_today.as_deref()),
                ProductsView::Week => want.launches(self.launches_week.as_deref()),
                _ => want.summaries(self.product_list.as_ref().map(|(_, list)| list.products.as_slice()).unwrap_or_default()),
            },
            Some(Screen::Product(_)) => {
                if let Some(page) = &self.product {
                    want.summaries(std::slice::from_ref(&page.summary));
                    for shot in &page.shots {
                        want.add(Some(&shot.url), Size::Large);
                    }
                    for m in &page.makers {
                        want.add(m.person.avatar.as_ref(), Size::Small);
                    }
                    if let Some(launch) = &page.latest_launch {
                        want.post(launch);
                    }
                    want.posts(Some(&page.posts));
                }
            }
            Some(Screen::Mine) => {
                if let Some(me) = &self.me {
                    want.add(me.me.avatar.as_ref(), Size::Small);
                }
            }
            _ => {}
        }
        // the day's first launches, beside the feed
        want.launches(self.launches_today.as_deref().map(|l| &l[..l.len().min(3)]));
        want.0
    }

    /// Fetches the photos the screen on show draws that are not held yet; once many are held, forgets those it does not
    /// draw.
    fn fetch_photos(&mut self) -> Task<Msg> {
        let Some(client) = self.session.as_ref().map(|(c, _)| c.clone()) else { return Task::none() };
        let wanted = self.photos_on_show();
        if self.photos.len() > PHOTOS_KEPT {
            let drawn: HashSet<&str> = wanted.iter().map(|(u, _)| u.as_str()).collect();
            self.photos.retain(|u, _| drawn.contains(u.as_str()));
        }
        let mut fetches = Vec::new();
        for (url, size) in wanted {
            if self.photos.contains_key(&url) {
                continue;
            }
            self.photos.insert(url.clone(), Picture::Loading);
            let client = client.clone();
            fetches.push(Task::perform(
                async move {
                    let picture = match client.photo(&url).await {
                        Ok(bytes) => crate::lattice::off_thread(move || photos::picture(&bytes, size)).await.unwrap_or(Picture::Failed),
                        Err(_) => Picture::Failed,
                    };
                    (url, picture)
                },
                |(url, picture)| Msg::Photo(url, picture),
            ));
        }
        Task::batch(fetches)
    }

    /// Handles a message, then fetches the photos the screen on show now draws.
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        if matches!(
            msg,
            Msg::Repost(_)
                | Msg::Quote(_)
                | Msg::React(..)
                | Msg::Like(..)
                | Msg::Bookmark(..)
                | Msg::Delete(_)
                | Msg::Pin(_)
                | Msg::Mute(..)
                | Msg::Block(..)
                | Msg::Reporting(_)
                | Msg::ModAct(..)
                | Msg::TakeOut(_)
                | Msg::CopyLink(_)
                | Msg::Go(_)
                | Msg::Back
                | Msg::Compose
                | Msg::PickLists(_)
        ) {
            self.post_menu = None;
            self.repost_menu = None;
            self.reacting = None;
            self.page_menu = false;
        }
        let task = self.step(msg);
        Task::batch([task, self.fetch_photos()])
    }

    fn step(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Go(screen) => return self.go(screen),
            Msg::Back => {
                if let Some(previous) = self.history.pop() {
                    self.screen = Some(previous.clone());
                    self.reporting = None;
                    self.selected = None;
                    self.viewing = None;
                    self.list_picker = None;
                    self.list_members = None;
                    self.edit_list = None;
                    self.confirm_delete_list = None;
                    self.confirm_delete_draft = None;
                    self.page_menu = false;
                    self.fresh_tabs(&previous);
                    return self.load(&previous, true);
                }
            }
            Msg::Refresh => {
                // a photo that failed is fetched again
                self.photos.retain(|_, p| !matches!(p, Picture::Failed));
                return self.reload();
            }
            Msg::Dismiss => {
                self.problem = None;
                self.note = None;
            }
            Msg::Me(Ok(me)) => {
                let first = self.me.is_none();
                if !self.profile_form.skills_dirty {
                    self.profile_form.skills = me.profile.skills.iter().map(|s| s.skill.clone()).collect();
                }
                self.me = Some(me);
                if first || !self.mine.dirty {
                    self.refill_mine();
                }
                // My page opened before the reader's username was known: its top is read now.
                if self.screen == Some(Screen::Mine) && self.mine.top.is_none() {
                    return self.fetch_mine_top();
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
                if self.screen == Some(Screen::Home) && self.home_feed == HomeFeed::ForYou {
                    return match &self.for_you {
                        Some((page, shown)) if shown.more => self.read_for_you(page + 1),
                        _ => Task::none(),
                    };
                }
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
                    Ok(thread) => {
                        self.thread = Some(thread);
                        if std::mem::take(&mut self.reply_focus) {
                            return iced::widget::operation::focus(keys::REPLY);
                        }
                    }
                    Err(p) => {
                        self.thread = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::Notifications(append, result) => {
                if self.screen != Some(Screen::Notifications) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => {
                        // What is on show has been seen: mark read up to the newest unread one shown.
                        let newest_unread = page.items.iter().find(|n| !n.read).map(|n| n.id.clone()).filter(|_| !append);
                        match self.notifications.as_mut().filter(|_| append) {
                            Some(shown) => {
                                shown.items.extend(page.items);
                                shown.more = page.more;
                                shown.unread = page.unread;
                            }
                            None => self.notifications = Some(page),
                        }
                        if let Some(up_to) = newest_unread {
                            return self.run(
                                move |c, t| async move { pages::notifications_read(&c, &t, Some(&up_to)).await },
                                Msg::NotificationsRead,
                            );
                        }
                    }
                    Err(p) => {
                        self.notifications = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::Jobs(append, result) => {
                if self.screen != Some(Screen::Jobs) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.jobs.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.extend(page.posts);
                            shown.more = page.more;
                        }
                        None => self.jobs = Some(page),
                    },
                    Err(p) => {
                        self.jobs = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::JobsWords(words) => self.jobs_query.words = words,
            Msg::JobsWorkplace(pick) => {
                self.jobs_query.workplace = pick.0;
                return self.load(&Screen::Jobs, false);
            }
            Msg::JobsEmployment(pick) => {
                self.jobs_query.employment = pick.0;
                return self.load(&Screen::Jobs, false);
            }
            Msg::JobsSearch => return self.load(&Screen::Jobs, false),
            Msg::JobToggle => self.composer.job_on = !self.composer.job_on,
            Msg::JobLocation(text) => self.composer.job.location = text,
            Msg::JobWorkplace(workplace) => self.composer.job.workplace = workplace,
            Msg::JobEmployment(employment) => self.composer.job.employment = employment,
            Msg::JobPay(text) => self.composer.job.pay = text,
            Msg::JobDays(days) => self.composer.job_days = days,
            Msg::Applying(post_id) => self.applying = post_id.map(|id| (id, String::new())),
            Msg::ApplyNote(text) => {
                if let Some((_, note)) = self.applying.as_mut() {
                    *note = text;
                }
            }
            Msg::Apply => {
                if let Some((id, note)) = self.applying.take() {
                    let words = "Applied. The organization's owners and admins see your page and your note.".to_string();
                    return self.done(move |c, t| async move { pages::apply_job(&c, &t, &id, note.trim()).await }, words);
                }
            }
            Msg::Withdraw(id) => {
                return self.done(move |c, t| async move { pages::withdraw_application(&c, &t, &id).await }, "Application withdrawn.".into());
            }
            Msg::Applicants(id, result) => {
                if self.screen != Some(Screen::Applicants(id.clone())) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(list) => self.applicants = Some((id, list)),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::CloseJob(id) => {
                return self.done(move |c, t| async move { pages::close_job(&c, &t, &id).await }, "The job is closed.".into());
            }
            Msg::Applications(result) => match result {
                Ok(posts) => self.applications = Some(posts),
                Err(p) => self.problem = Some(p),
            },
            Msg::Bookmarks(append, result) => {
                if self.screen != Some(Screen::Bookmarks) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.bookmarks.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.extend(page.posts);
                            shown.more = page.more;
                        }
                        None => self.bookmarks = Some(page),
                    },
                    Err(p) => {
                        self.bookmarks = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::HomeFeed(feed) => {
                if self.home_feed != feed {
                    self.home_feed = feed;
                    if self.screen == Some(Screen::Home) {
                        return self.load(&Screen::Home, false);
                    }
                }
            }
            Msg::ForYou(page, result) => {
                if self.screen != Some(Screen::Home) || self.home_feed != HomeFeed::ForYou {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(posts) => match self.for_you.as_mut().filter(|_| page > 0) {
                        Some((shown_page, shown)) => {
                            shown.posts.extend(posts.posts);
                            shown.more = posts.more;
                            *shown_page = page;
                        }
                        None => self.for_you = Some((page, posts)),
                    },
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::Trending(result) => match result {
                Ok(t) => self.trending = Some(t),
                Err(p) => self.problem = Some(p),
            },
            Msg::Suggestions(result) => match result {
                Ok(found) => self.suggestions = Some(found),
                Err(p) => self.problem = Some(p),
            },
            Msg::Tagged(tag, append, result) => {
                if self.screen != Some(Screen::Tag(tag)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.tagged.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.posts.extend(page.posts.posts);
                            shown.posts.more = page.posts.more;
                        }
                        None => self.tagged = Some(page),
                    },
                    Err(p) => {
                        self.tagged = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::SearchKind(kind) => {
                if self.search_kind != kind {
                    self.search_kind = kind;
                    if let Some(screen @ Screen::Search(_)) = self.screen.clone() {
                        return self.load(&screen, false);
                    }
                }
            }
            Msg::Found(query, kind, append, result) => {
                if self.screen.as_ref() != Some(&Screen::Search(query.clone())) || self.search_kind != kind {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(found) => match self.found.as_mut().filter(|_| append) {
                        Some((_, _, shown)) => {
                            shown.posts.posts.extend(found.posts.posts);
                            shown.posts.more = found.posts.more;
                        }
                        None => self.found = Some((query, kind, found)),
                    },
                    Err(p) => {
                        self.found = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::ExpField(field, text) => {
                let e = &mut self.profile_form.experience;
                *match field {
                    ExpField::Title => &mut e.title,
                    ExpField::Ticker => &mut e.ticker,
                    ExpField::OrgName => &mut e.org_name,
                    ExpField::Started => &mut e.started,
                    ExpField::Ended => &mut e.ended,
                    ExpField::About => &mut e.about,
                } = text;
            }
            Msg::ExpEdit(entry) => {
                self.profile_form.exp_open = true;
                self.profile_form.exp_now = entry.ended.is_none();
                let ticker = entry.org.as_ref().map(|o| o.ticker.clone()).unwrap_or_default();
                self.profile_form.experience = NewExperience {
                    org_name: if ticker.is_empty() { entry.org_name } else { String::new() },
                    ticker,
                    id: entry.id,
                    title: entry.title,
                    started: entry.started,
                    ended: entry.ended.unwrap_or_default(),
                    about: entry.about,
                };
            }
            Msg::EduField(field, text) => {
                let e = &mut self.profile_form.education;
                *match field {
                    EduField::School => &mut e.school,
                    EduField::Degree => &mut e.degree,
                    EduField::Field => &mut e.field,
                    EduField::Started => &mut e.started,
                    EduField::Ended => &mut e.ended,
                } = text;
            }
            Msg::EduEdit(entry) => {
                self.profile_form.edu_open = true;
                self.profile_form.education = NewEducation {
                    id: entry.id,
                    school: entry.school,
                    degree: entry.degree,
                    field: entry.field,
                    started: entry.started.unwrap_or_default(),
                    ended: entry.ended.unwrap_or_default(),
                };
            }
            Msg::ProfileClear => {
                let f = &mut self.profile_form;
                f.experience = NewExperience::default();
                f.education = NewEducation::default();
                (f.exp_open, f.edu_open, f.exp_now, f.confirm_delete) = (false, false, false, None);
            }
            Msg::EntryOpen(position) => {
                let f = &mut self.profile_form;
                if position {
                    (f.experience, f.exp_open, f.exp_now) = (NewExperience::default(), true, false);
                } else {
                    (f.education, f.edu_open) = (NewEducation::default(), true);
                }
            }
            Msg::ExpNow(now) => {
                self.profile_form.exp_now = now;
                if now {
                    self.profile_form.experience.ended.clear();
                }
            }
            Msg::SaveExperience => {
                let entry = self.profile_form.experience.clone();
                return self.run(move |c, t| async move { pages::set_experience(&c, &t, &entry).await.map(|_| ()) }, |r| {
                    Msg::ProfileSaved(ProfilePart::Experience, r)
                });
            }
            Msg::DeleteExperience(id) => {
                // Deleting takes an organization's confirmation with it: the first press asks.
                if self.profile_form.confirm_delete.as_deref() != Some(id.as_str()) {
                    self.profile_form.confirm_delete = Some(id);
                    return Task::none();
                }
                self.profile_form.confirm_delete = None;
                return self.run(move |c, t| async move { pages::delete_experience(&c, &t, &id).await.map(|_| ()) }, |r| {
                    Msg::ProfileSaved(ProfilePart::Experience, r)
                });
            }
            Msg::SaveEducation => {
                let entry = self.profile_form.education.clone();
                return self.run(move |c, t| async move { pages::set_education(&c, &t, &entry).await.map(|_| ()) }, |r| {
                    Msg::ProfileSaved(ProfilePart::Education, r)
                });
            }
            Msg::DeleteEducation(id) => {
                if self.profile_form.confirm_delete.as_deref() != Some(id.as_str()) {
                    self.profile_form.confirm_delete = Some(id);
                    return Task::none();
                }
                self.profile_form.confirm_delete = None;
                return self.run(move |c, t| async move { pages::delete_education(&c, &t, &id).await.map(|_| ()) }, |r| {
                    Msg::ProfileSaved(ProfilePart::Education, r)
                });
            }
            Msg::SkillNew(text) => self.profile_form.skill_new = text,
            Msg::SkillAdd => {
                let f = &mut self.profile_form;
                let typed = std::mem::take(&mut f.skill_new);
                for skill in typed.split(',').map(|w| w.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|w| !w.is_empty()) {
                    if f.skills.len() < MAX_SKILLS && !f.skills.iter().any(|k| k.to_lowercase() == skill.to_lowercase()) {
                        f.skills.push(skill.chars().take(SKILL_MAX_CHARS).collect());
                        f.skills_dirty = true;
                    }
                }
            }
            Msg::SkillRemove(at) => {
                if at < self.profile_form.skills.len() {
                    self.profile_form.skills.remove(at);
                    self.profile_form.skills_dirty = true;
                }
            }
            Msg::SaveSkills => {
                let skills = self.profile_form.skills.clone();
                return self.run(move |c, t| async move { pages::set_skills(&c, &t, &skills).await.map(|_| ()) }, |r| {
                    Msg::ProfileSaved(ProfilePart::Skills, r)
                });
            }
            Msg::ProfileSaved(part, result) => match result {
                Ok(()) => {
                    let f = &mut self.profile_form;
                    match part {
                        ProfilePart::Experience => (f.experience, f.exp_open, f.exp_now) = (NewExperience::default(), false, false),
                        ProfilePart::Education => (f.education, f.edu_open) = (NewEducation::default(), false),
                        ProfilePart::Skills => f.skills_dirty = false,
                    }
                    self.note = Some("Saved.".into());
                    return self.fetch_me();
                }
                Err(p) => self.problem = Some(p),
            },
            Msg::Endorse(username, skill, endorse) => {
                let words = if endorse { "Endorsed." } else { "Endorsement taken back." };
                return self.done(move |c, t| async move { pages::endorse(&c, &t, &username, &skill, endorse).await }, words.into());
            }
            Msg::ConfirmExperience(ticker, entry, confirm) => {
                let words = if confirm { "Confirmed." } else { "Confirmation taken back." };
                return self.done(
                    move |c, t| async move { pages::confirm_experience(&c, &t, &ticker, &entry, confirm).await },
                    words.into(),
                );
            }
            Msg::OrgExperience(ticker, result) => {
                if self.screen != Some(Screen::OrgExperience(ticker.clone())) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(list) => self.org_experience = Some((ticker, list)),
                    Err(p) => {
                        self.org_experience = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::Replies(rule) => self.composer.replies = rule,
            // a post carries a poll or photos, not both
            Msg::PollToggle if self.composer.photos.is_empty() || self.composer.poll_on => self.composer.poll_on = !self.composer.poll_on,
            Msg::PollToggle => {}
            Msg::PollOption(i, text) => {
                if let Some(option) = self.composer.poll_options.get_mut(i) {
                    *option = text;
                }
            }
            Msg::PollHours(length) => self.composer.poll_length = length,
            Msg::Vote(id, option) => {
                return self.done(move |c, t| async move { pages::vote(&c, &t, &id, option).await }, "Voted.".into());
            }
            Msg::Mute(username, mute) => {
                let words = if mute { format!("Muted @{username}.") } else { format!("Unmuted @{username}.") };
                return self.done(move |c, t| async move { pages::mute(&c, &t, &username, mute).await }, words);
            }
            Msg::Muted(result) => match result {
                Ok(people) => self.muted = Some(people),
                Err(p) => self.problem = Some(p),
            },
            Msg::Bookmark(id, save) => {
                let words = if save { "Saved to your bookmarks." } else { "Removed from your bookmarks." };
                return self.done(move |c, t| async move { pages::bookmark(&c, &t, &id, save).await }, words.into());
            }
            Msg::Pin(id) => {
                return self.done(move |c, t| async move { pages::pin(&c, &t, &id).await }, "Pinned to the top of your page.".into());
            }
            Msg::Unpin => {
                return self.done(|c, t| async move { pages::unpin(&c, &t).await }, "Unpinned.".into());
            }
            Msg::NotificationsRead(result) => {
                if let (Ok(unread), Some(me)) = (result, self.me.as_mut()) {
                    me.unread_notifications = unread;
                }
            }
            Msg::Flagged(ticker, append, result) => {
                if self.screen != Some(Screen::Flagged(ticker.clone())) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.flagged.as_mut().filter(|_| append) {
                        Some((_, shown)) => {
                            shown.posts.extend(page.posts);
                            shown.more = page.more;
                        }
                        None => self.flagged = Some((ticker, page)),
                    },
                    Err(p) => {
                        self.flagged = None;
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
                } else if let Some(tag) = as_tag(typed) {
                    Screen::Tag(tag)
                } else if !typed.is_empty() && typed.len() <= 5 && typed.chars().all(|c| c.is_ascii_uppercase()) {
                    Screen::Org(typed.to_string())
                } else if !typed.is_empty() {
                    self.search_kind = SearchKind::All;
                    Screen::Search(typed.to_string())
                } else {
                    return Task::none();
                };
                self.find.clear();
                return self.go(screen);
            }
            Msg::Kind(kind) => self.composer.kind = kind,
            Msg::Title(s) => self.composer.title = s,
            Msg::Body(action) => {
                self.composer.body.perform(action);
                return self.track_mention();
            }
            Msg::MentionFound(word, result) => {
                if let Some(m) = self.mentioning.as_mut().filter(|m| m.word == word) {
                    m.found = result.map(|f| f.people.into_iter().filter(|p| p.has_page && p.username.is_some()).take(6).collect()).unwrap_or_default();
                    m.at = 0;
                }
            }
            Msg::MentionMove(by) => {
                if let Some(m) = self.mentioning.as_mut().filter(|m| !m.found.is_empty()) {
                    m.at = (m.at as i64 + i64::from(by)).rem_euclid(m.found.len() as i64) as usize;
                }
            }
            Msg::MentionPick(which) => return self.pick_mention(which),
            Msg::MentionClose => self.mentioning = None,
            Msg::AsWho(who) => self.composer.as_who = who.0,
            Msg::Quote(post) => {
                self.composer.kind = Kind::Post;
                self.composer.quoting = Some(post);
                if self.screen != Some(Screen::Home) {
                    return self.go(Screen::Home);
                }
            }
            Msg::Unquote => {
                self.composer.quoting = None;
                self.composer.draft_quote = None;
            }
            Msg::Submit => {
                if !self.composer.ready() || self.uploading > 0 {
                    return Task::none();
                }
                self.composer.sending = true;
                let new = self.composer.new_post();
                if let Some(id) = self.composer.draft_id.clone() {
                    // The draft is rewritten as it stands, then posted: its photos are already its own.
                    return self.run(
                        move |c, t| async move {
                            let saved = pages::draft_save(&c, &t, Some(&id), &new, None).await?;
                            pages::draft_publish(&c, &t, &saved.id).await
                        },
                        Msg::Posted,
                    );
                }
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
            Msg::React(id, reaction) => {
                return self.done(move |c, t| async move { pages::react(&c, &t, &id, reaction).await }, String::new());
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
                // Enter in a field asks too: nothing is sent until there is a change the service would take.
                let name_ok = self.me.as_ref().is_some_and(|m| social::tidy_display_name(&self.mine.name) == m.me.display_name)
                    || social::valid_display_name(&self.mine.name);
                if self.mine.saving || !self.mine.dirty || !name_ok || title_problem(&self.mine.title).is_some() {
                    return Task::none();
                }
                self.mine.saving = true;
                let title = self.mine.title.clone();
                let org = self.mine.title_org.as_ref().map(|o| o.ticker.clone()).unwrap_or_default();
                let bio = self.mine.bio.text().trim_end().to_string();
                // A changed name goes first, so the page's answer reads it.
                let name = social::tidy_display_name(&self.mine.name);
                let rename = (Some(name.as_str()) != self.me.as_ref().map(|m| m.me.display_name.as_str())).then_some(name);
                return self.run(
                    move |c, t| async move {
                        if let Some(name) = rename {
                            social::rename(&c, &t, &name).await?;
                        }
                        pages::update(&c, &t, Some(&title), Some(&org), Some(&bio)).await
                    },
                    Msg::Saved,
                );
            }
            Msg::MineTab(tab) => self.mine.tab = tab,
            Msg::DiscardMine => self.refill_mine(),
            Msg::MineTop(result) => {
                if let Ok(page) = result {
                    self.mine.top = Some(page);
                }
            }
            Msg::MineName(name) => {
                self.mine.name = name;
                self.mine.dirty = true;
            }
            Msg::AiTraining(allow) => {
                return self.run(move |c, t| async move { pages::ai_training(&c, &t, allow).await }, Msg::Me);
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
            Msg::ModView(view) => {
                self.mod_view = view;
                self.mod_reports = None;
                self.mod_log = None;
                self.mod_matches = None;
                if self.screen == Some(Screen::Moderation) {
                    self.loading = true;
                    return self.read_mod(None);
                }
            }
            Msg::ModReports(view, append, result) => {
                if view != self.mod_view {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok((reports, more)) => match self.mod_reports.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.0.extend(reports);
                            shown.1 = more;
                        }
                        None => self.mod_reports = Some((reports, more)),
                    },
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::ModLog(append, result) => {
                if self.mod_view != ModView::Log {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok((actions, more)) => match self.mod_log.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.0.extend(actions);
                            shown.1 = more;
                        }
                        None => self.mod_log = Some((actions, more)),
                    },
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::ModNote(s) => self.mod_note = s,
            Msg::ModAct(act, target, report) => {
                let note = self.mod_note.trim().to_string();
                return self.run(
                    move |c, t| async move { pages::mod_act(&c, &t, act, target.as_ref(), report.as_deref(), &note).await },
                    Msg::ModDone,
                );
            }
            Msg::ModDismiss(report) => {
                let note = self.mod_note.trim().to_string();
                return self.run(move |c, t| async move { pages::mod_dismiss(&c, &t, &report, &note).await }, Msg::ModDone);
            }
            Msg::ModDone(Ok(action)) => {
                self.mod_note.clear();
                self.note = Some(format!("You {}.", action.words()));
                return self.reload();
            }
            Msg::ModDone(Err(p)) => self.problem = Some(p),
            Msg::MediaMatches(result) => {
                if self.mod_view != ModView::Matches {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(list) => self.mod_matches = Some(list),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::MarkMatch(id, reported, cleared) => {
                let words = if cleared { "Cleared: the account may upload photos again." } else { "Marked as reported." };
                return self.done(move |c, t| async move { pages::mark_media_match(&c, &t, &id, reported, cleared).await }, words.into());
            }
            Msg::PickPhotos(target) => {
                if self.session.is_none() {
                    return Task::none();
                }
                let many = target == PhotoFor::Post;
                return Task::perform(
                    crate::lattice::off_thread(move || {
                        crate::lattice::ide::picker::pick_photos(many).map(|paths| paths.iter().map(|p| photos::read(p)).collect())
                    }),
                    move |r| Msg::PhotosChosen(target, r.unwrap_or_else(|| Err("Windows' file picker could not start.".into()))),
                );
            }
            Msg::PhotosChosen(target, chosen) => {
                // signed out while the picker was open: nothing is uploaded, so nothing is counted as uploading
                if self.session.is_none() {
                    return Task::none();
                }
                let files = match chosen {
                    Ok(files) => files,
                    Err(why) => {
                        self.problem = Some(Problem::Refused("photo-file".into(), why));
                        return Task::none();
                    }
                };
                let room = match target {
                    PhotoFor::Post if self.composer.poll_on => 0,
                    PhotoFor::Post => pages::POST_MAX_PHOTOS.saturating_sub(self.composer.photos.len() + self.uploading),
                    _ => 1,
                };
                let (mut uploads, mut refused) = (Vec::new(), Vec::new());
                for (name, bytes) in files {
                    match bytes {
                        Err(why) => refused.push(why),
                        Ok(_) if uploads.len() >= room => refused.push(format!("{name} was left out: a post carries four photos at most.")),
                        Ok(bytes) => uploads.push(self.upload(target.clone(), name, bytes)),
                    }
                }
                if !refused.is_empty() {
                    self.problem = Some(Problem::Refused("photo-file".into(), refused.join(" ")));
                }
                self.uploading += uploads.len();
                return Task::batch(uploads);
            }
            Msg::Uploaded(target, name, result) => {
                self.uploading = self.uploading.saturating_sub(1);
                let (upload, picture) = match result {
                    Ok(done) => done,
                    Err(p) => {
                        self.problem = Some(p);
                        return Task::none();
                    }
                };
                if !upload.url.is_empty() {
                    self.photos.insert(upload.url.clone(), picture);
                }
                let id = upload.id;
                let words = target.done_words(false);
                match target {
                    // the poll may have been chosen, or the photos filled, while it uploaded: it then expires unused
                    PhotoFor::Post => {
                        if !self.composer.poll_on && self.composer.photos.len() < pages::POST_MAX_PHOTOS {
                            self.composer.photos.push(Attached { id, url: upload.url, name, alt: String::new() });
                        }
                    }
                    PhotoFor::Page(kind) => {
                        return self.done(move |c, t| async move { pages::set_page_media(&c, &t, kind, &id).await }, words);
                    }
                    PhotoFor::Org(ticker, kind) => {
                        return self.done(move |c, t| async move { pages::set_org_media(&c, &t, &ticker, kind, &id).await }, words);
                    }
                    PhotoFor::Product(handle, slot, at) => {
                        return self.done(
                            move |c, t| async move { pages::product_media(&c, &t, &handle, slot == "logo", at, Some(&id), "").await },
                            words,
                        );
                    }
                }
            }
            Msg::PhotoAlt(i, alt) => {
                if let Some(photo) = self.composer.photos.get_mut(i) {
                    photo.alt = alt.chars().take(pages::ALT_MAX_CHARS).collect();
                }
            }
            Msg::RemovePhoto(i) => {
                if i < self.composer.photos.len() {
                    self.composer.photos.remove(i);
                }
            }
            Msg::ClearPhoto(target) => {
                let words = target.done_words(true);
                match target {
                    PhotoFor::Post => {}
                    PhotoFor::Page(kind) => {
                        return self.done(move |c, t| async move { pages::set_page_media(&c, &t, kind, "").await }, words);
                    }
                    PhotoFor::Org(ticker, kind) => {
                        return self.done(move |c, t| async move { pages::set_org_media(&c, &t, &ticker, kind, "").await }, words);
                    }
                    PhotoFor::Product(handle, slot, at) => {
                        return self.done(
                            move |c, t| async move { pages::product_media(&c, &t, &handle, slot == "logo", at, None, "").await },
                            words,
                        );
                    }
                }
            }
            Msg::Photo(url, picture) => {
                self.photos.insert(url, picture);
            }
            // builders (ADR-0056)
            Msg::ProductsView(view) => {
                self.products_view = view;
                if self.screen == Some(Screen::Products) {
                    self.loading = true;
                    return self.read_products(view, None);
                }
            }
            Msg::Launches(week, result) => {
                // the day's are the rail's too: a read for the rail that fails says nothing
                let shown = self.screen == Some(Screen::Products) && self.products_view == if week { ProductsView::Week } else { ProductsView::Today };
                if shown {
                    self.loading = false;
                }
                match result {
                    Ok(posts) if week => self.launches_week = Some(posts),
                    Ok(posts) => self.launches_today = Some(posts),
                    Err(p) if shown => self.problem = Some(p),
                    Err(_) => {}
                }
            }
            Msg::ProductList(view, append, result) => {
                if self.screen != Some(Screen::Products) || self.products_view != view {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.product_list.as_mut().filter(|(shown, _)| append && *shown == view) {
                        Some((_, shown)) => {
                            shown.products.extend(page.products);
                            shown.more = page.more;
                        }
                        None => self.product_list = Some((view, page)),
                    },
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::MyProducts(result) => {
                if let Ok(page) = result {
                    let kept: Vec<pages::ProductSummary> = page.products.into_iter().filter(|p| p.you_keep).collect();
                    // a product the reader no longer keeps is not offered to post about
                    if self.composer.product.as_ref().is_some_and(|h| !kept.iter().any(|p| &p.product.handle == h)) {
                        self.composer.product = None;
                        self.composer.launch_on = false;
                    }
                    self.my_products = Some(kept);
                }
            }
            Msg::Product(handle, append, result) => {
                if self.screen != Some(Screen::Product(handle)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.product.as_mut().filter(|shown| append && shown.summary.product.id == page.summary.product.id) {
                        Some(shown) => {
                            shown.posts.posts.extend(page.posts.posts);
                            shown.posts.more = page.posts.more;
                        }
                        None => {
                            // a keeper writes the product's updates here: the composer is about it
                            if page.you_keep() && self.composer.product.is_none() && self.composer.chars() == 0 {
                                self.composer.product = Some(page.summary.product.handle.clone());
                            }
                            self.product = Some(page);
                        }
                    },
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::ProductShow(show) => {
                self.product_show = show;
                self.giving = None;
                return self.reload();
            }
            Msg::FeedbackSort(sort) => {
                self.feedback_sort = sort;
                return self.reload();
            }
            Msg::FeedbackFilter(status) => {
                self.feedback_status = status;
                return self.reload();
            }
            Msg::NewProductField(field, text) => {
                let form = &mut self.new_product;
                match field {
                    ProductField::Handle => form.handle = ProductForm::tidy_handle(&text),
                    ProductField::Name => form.name = text,
                    ProductField::Tagline => form.tagline = text,
                    ProductField::About => form.about = text,
                    ProductField::Website => form.website = text,
                    ProductField::Source => form.source = text,
                    ProductField::Tech => form.tech = text,
                }
            }
            Msg::NewProductStage(stage) => self.new_product.stage = stage,
            Msg::NewProductPlatform(platform, on) => toggle_platform(&mut self.new_product.platforms, platform, on),
            Msg::NewProductOwner(owner) => self.new_product.as_org = owner.0,
            Msg::CreateProduct => {
                if self.new_product.why_not(true).is_some() {
                    return Task::none();
                }
                let new = self.new_product.new_product();
                return self.run(move |c, t| async move { pages::product_create(&c, &t, &new).await }, Msg::ProductCreated);
            }
            Msg::ProductCreated(Ok(product)) => {
                self.new_product = ProductForm::default();
                self.note = Some(format!("p/{} is made. Add its logo and screenshots, and list its makers.", product.handle));
                return self.go(Screen::Product(product.handle));
            }
            Msg::ProductCreated(Err(p)) => self.problem = Some(p),
            Msg::EditProduct(open) => {
                self.edit_product = if open { self.product.as_ref().map(ProductForm::of) } else { None };
            }
            Msg::EditProductField(field, text) => {
                if let Some(form) = self.edit_product.as_mut() {
                    match field {
                        ProductField::Handle => {}
                        ProductField::Name => form.name = text,
                        ProductField::Tagline => form.tagline = text,
                        ProductField::About => form.about = text,
                        ProductField::Website => form.website = text,
                        ProductField::Source => form.source = text,
                        ProductField::Tech => form.tech = text,
                    }
                }
            }
            Msg::EditProductStage(stage) => {
                if let Some(form) = self.edit_product.as_mut() {
                    form.stage = stage;
                }
            }
            Msg::EditProductPlatform(platform, on) => {
                if let Some(form) = self.edit_product.as_mut() {
                    toggle_platform(&mut form.platforms, platform, on);
                }
            }
            Msg::SaveProduct => {
                let (Some(form), Some(page)) = (self.edit_product.as_ref(), self.product.as_ref()) else { return Task::none() };
                if form.why_not(false).is_some() {
                    return Task::none();
                }
                let (handle, changes) = (page.summary.product.handle.clone(), form.changes());
                self.edit_product = None;
                return self.done(move |c, t| async move { pages::product_update(&c, &t, &handle, &changes).await.map(|_| ()) }, "Saved.".into());
            }
            Msg::DeleteProduct(handle) => {
                if self.confirm_delete_product.as_ref() != Some(&handle) {
                    self.confirm_delete_product = Some(handle);
                    return Task::none();
                }
                self.confirm_delete_product = None;
                let shown = handle.clone();
                return self.run(
                    move |c, t| async move { pages::product_delete(&c, &t, &handle).await },
                    move |r| Msg::ProductGone(r.map(|()| format!("p/{shown} is deleted. The posts about it stay their authors'."))),
                );
            }
            Msg::ProductGone(Ok(words)) => {
                self.note = Some(words);
                self.product = None;
                self.my_products = None;
                return self.go(Screen::Products);
            }
            Msg::ProductGone(Err(p)) => self.problem = Some(p),
            Msg::FollowProduct(handle, follow) => {
                return self.done(move |c, t| async move { pages::product_follow(&c, &t, &handle, follow).await }, String::new());
            }
            Msg::SupportLaunch(id, support) => {
                return self.done(move |c, t| async move { pages::launch_support(&c, &t, &id, support).await }, String::new());
            }
            Msg::VoteFeedback(id, vote) => {
                return self.done(move |c, t| async move { pages::feedback_vote(&c, &t, &id, vote).await }, String::new());
            }
            Msg::SetFeedbackStatus(id, status) => {
                let words = format!("Marked {}. Its author and voters are told.", status.label().to_lowercase());
                return self.done(move |c, t| async move { pages::feedback_status(&c, &t, &id, status).await }, words);
            }
            Msg::ReviewStars(id, stars) => {
                return self.done(move |c, t| async move { pages::review_stars(&c, &t, &id, stars).await }, String::new());
            }
            Msg::Give(giving) => self.giving = giving,
            Msg::GiveText(text) => match self.giving.as_mut() {
                Some(Giving::Feedback(_, words) | Giving::Review(_, words)) => *words = text,
                None => {}
            },
            Msg::GiveCategory(category) => {
                if let Some(Giving::Feedback(chosen, _)) = self.giving.as_mut() {
                    *chosen = category;
                }
            }
            Msg::GiveStars(stars) => {
                if let Some(Giving::Review(chosen, _)) = self.giving.as_mut() {
                    *chosen = stars.clamp(1, 5);
                }
            }
            Msg::SendGiving => {
                let (Some(giving), Some(page)) = (self.giving.clone(), self.product.as_ref()) else { return Task::none() };
                let text = giving.text().trim().to_string();
                if text.is_empty() || text.chars().count() > pages::POST_MAX_CHARS {
                    return Task::none();
                }
                let new = NewPost { text, product: Some(page.summary.product.handle.clone()), about: giving.about(), ..NewPost::default() };
                let words = if matches!(giving, Giving::Review(..)) { "Your review is posted." } else { "Your feedback is posted. Its makers are told." };
                self.giving = None;
                return self.run(move |c, t| async move { pages::post(&c, &t, &new).await }, move |r| Msg::Done(r.map(|_| words.to_string())));
            }
            Msg::AskToTest(handle, ask) => {
                let note = std::mem::take(&mut self.test_note);
                let words = if ask { "Asked. Its makers decide, and tell you how to join." } else { "Withdrawn." };
                return self.done(move |c, t| async move { pages::product_test(&c, &t, &handle, ask, note.trim()).await.map(|_| ()) }, words.into());
            }
            Msg::TestNote(note) => self.test_note = note.chars().take(pages::TESTER_NOTE_MAX_CHARS).collect(),
            Msg::ShowTeam(Some(handle)) => return self.read_team(handle),
            Msg::ShowTeam(None) => {
                self.product_makers = None;
                self.product_testers = None;
            }
            Msg::ProductMakers(handle, result) => match result {
                Ok(list) if self.screen == Some(Screen::Product(handle.clone())) => self.product_makers = Some((handle, list)),
                Ok(_) => {}
                Err(p) => self.problem = Some(p),
            },
            Msg::ProductTesters(handle, result) => match result {
                Ok(list) if self.screen == Some(Screen::Product(handle.clone())) => self.product_testers = Some((handle, list)),
                Ok(_) => {}
                Err(p) => self.problem = Some(p),
            },
            Msg::MakerName(name) => self.maker_name = name.trim().trim_start_matches('@').to_string(),
            Msg::InviteMaker(handle) => {
                let username = std::mem::take(&mut self.maker_name);
                if username.is_empty() {
                    return Task::none();
                }
                let words = format!("@{username} is listed. The listing shows once they confirm it.");
                return self.done(move |c, t| async move { pages::maker_invite(&c, &t, &handle, &username).await.map(|_| ()) }, words);
            }
            Msg::AnswerMaker(handle, accept) => {
                let words = if accept { "You are listed as one of its makers." } else { "Declined." };
                return self.done(move |c, t| async move { pages::maker_answer(&c, &t, &handle, accept).await }, words.into());
            }
            Msg::RemoveMaker(handle, member) => {
                let me = self.me.as_ref().is_some_and(|m| m.me.id == member);
                let words = if me { "You left its makers." } else { "Taken off its makers." };
                return self.done(move |c, t| async move { pages::maker_remove(&c, &t, &handle, &member).await }, words.into());
            }
            Msg::TesterNote(note) => self.tester_note = note.chars().take(pages::TESTER_INVITE_MAX_CHARS).collect(),
            Msg::AnswerTester(handle, member, invite) => {
                let note = if invite { std::mem::take(&mut self.tester_note) } else { String::new() };
                let words = if invite { "Invited. Only they see your note." } else { "Declined." };
                return self.done(
                    move |c, t| async move { pages::tester_answer(&c, &t, &handle, &member, invite, note.trim()).await },
                    words.into(),
                );
            }
            Msg::PostAbout(choice) => {
                self.composer.product = choice.0.map(|(handle, _)| handle);
                if self.composer.product.is_none() {
                    self.composer.launch_on = false;
                }
            }
            Msg::LaunchOn(on) => self.composer.launch_on = on && self.composer.product.is_some(),
            Msg::LaunchVersion(version) => self.composer.launch_version = version.chars().take(pages::LAUNCH_VERSION_MAX_CHARS).collect(),
            Msg::ModProduct(handle, suspend) => {
                let note = self.mod_note.trim().to_string();
                let words = format!("p/{handle} is {}.", if suspend { "suspended" } else { "restored" });
                return self.run(move |c, t| async move { pages::mod_product(&c, &t, suspend, &handle, &note).await }, move |r| {
                    let words = words.clone();
                    if suspend { Msg::ProductGone(r.map(|_| words)) } else { Msg::Done(r.map(|_| words)) }
                });
            }
            Msg::Communities(result) => {
                if self.screen != Some(Screen::Communities) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(list) => self.communities = Some(list),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::MyCommunities(result) => match result {
                Ok(list) => {
                    // A community the reader no longer belongs to is not offered to post in.
                    if let Some(handle) = &self.composer.in_community
                        && !list.iter().any(|c| c.you_belong() && &c.community.handle == handle)
                    {
                        self.composer.in_community = None;
                    }
                    self.my_communities = Some(list);
                }
                Err(p) => self.problem = Some(p),
            },
            Msg::Community(handle, append, result) => {
                if self.screen != Some(Screen::Community(handle)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.community.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.posts.extend(page.posts.posts);
                            shown.posts.more = page.posts.more;
                        }
                        None => {
                            // The composer on a community's page posts in it, while nothing is written yet.
                            let c = &self.composer;
                            if page.summary.you_belong() && c.chars() == 0 && c.photos.is_empty() && c.as_who.is_none() {
                                self.composer.in_community = Some(page.summary.community.handle.clone());
                            }
                            self.community = Some(page);
                        }
                    },
                    Err(p) => {
                        self.community = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::CommunityMembers(handle, result) => {
                if self.screen != Some(Screen::CommunityMembers(handle.clone())) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(list) => self.community_members = Some((handle, list)),
                    Err(p) => {
                        self.community_members = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::NewCommunityField(field, text) => {
                let form = &mut self.new_community;
                match field {
                    CommunityField::Handle => form.handle = NewCommunity::tidy_handle(&text),
                    CommunityField::Name => form.name = text,
                    CommunityField::About => form.about = text,
                    CommunityField::Rules => form.rules = text,
                }
            }
            Msg::NewCommunityJoining(joining) => self.new_community.joining = joining,
            Msg::CreateCommunity => {
                if !self.new_community.ready() {
                    return Task::none();
                }
                let NewCommunity { handle, name, about, rules, joining } = self.new_community.clone();
                return self.run(
                    move |c, t| async move { pages::community_create(&c, &t, &handle, name.trim(), about.trim(), rules.trim(), joining).await },
                    Msg::CommunityCreated,
                );
            }
            Msg::CommunityCreated(Ok(community)) => {
                self.new_community = NewCommunity::default();
                self.note = Some(format!("c/{} is yours.", community.handle));
                return self.go(Screen::Community(community.handle));
            }
            Msg::CommunityCreated(Err(p)) => self.problem = Some(p),
            Msg::JoinCommunity(handle) => {
                return self.run(
                    move |c, t| async move { pages::community_join(&c, &t, &handle).await },
                    |r| {
                        Msg::Done(r.map(|state| {
                            if state == "member" { "You are a member.".to_string() } else { "Asked to join. Its moderators decide.".to_string() }
                        }))
                    },
                );
            }
            Msg::LeaveCommunity(handle) => {
                return self.done(move |c, t| async move { pages::community_leave(&c, &t, &handle).await }, "Done.".into());
            }
            Msg::DecideRequest(handle, member, approve) => {
                let words = if approve { "Approved." } else { "Refused." };
                return self.done(move |c, t| async move { pages::community_decide(&c, &t, &handle, &member, approve).await }, words.into());
            }
            Msg::CommunityRole(handle, member, role) => {
                return self.done(move |c, t| async move { pages::community_role(&c, &t, &handle, &member, role).await }, "Role changed.".into());
            }
            Msg::RemoveFromCommunity(handle, member, ban) => {
                let words = if ban { "Removed, and kept from joining again." } else { "Done." };
                return self.done(move |c, t| async move { pages::community_remove(&c, &t, &handle, &member, ban).await }, words.into());
            }
            Msg::TakeOut(post_id) => {
                return self.done(
                    move |c, t| async move { pages::community_remove_post(&c, &t, &post_id).await },
                    "Taken out of the community, and closed to replies.".into(),
                );
            }
            Msg::InCommunity(choice) => self.composer.in_community = choice.0,
            Msg::EditCommunity(open) => {
                self.edit_community = if open {
                    self.community.as_ref().map(|c| NewCommunity {
                        handle: c.summary.community.handle.clone(),
                        name: c.summary.community.name.clone(),
                        about: c.summary.about.clone(),
                        rules: c.rules.clone(),
                        joining: c.summary.joining,
                    })
                } else {
                    None
                };
            }
            Msg::EditCommunityField(field, text) => {
                if let Some(form) = self.edit_community.as_mut() {
                    match field {
                        CommunityField::Handle => {}
                        CommunityField::Name => form.name = text,
                        CommunityField::About => form.about = text,
                        CommunityField::Rules => form.rules = text,
                    }
                }
            }
            Msg::EditCommunityJoining(joining) => {
                if let Some(form) = self.edit_community.as_mut() {
                    form.joining = joining;
                }
            }
            Msg::SaveCommunity => {
                let Some(form) = self.edit_community.take().filter(|f| !f.name.trim().is_empty()) else { return Task::none() };
                return self.run(
                    move |c, t| async move {
                        pages::community_update(&c, &t, &form.handle, Some(form.name.trim()), Some(form.about.trim()), Some(form.rules.trim()), Some(form.joining))
                            .await
                    },
                    |r| Msg::Done(r.map(|_| "Saved.".to_string())),
                );
            }
            Msg::ModCommunity(handle, suspend) => {
                let note = self.mod_note.trim().to_string();
                return self.run(move |c, t| async move { pages::mod_community(&c, &t, suspend, &handle, &note).await }, Msg::ModDone);
            }
            Msg::Lists(result) => {
                if self.screen != Some(Screen::Lists) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(found) => self.lists = Some(found),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::ListPage(id, append, result) => {
                if self.screen != Some(Screen::List(id)) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(page) => match self.list.as_mut().filter(|_| append) {
                        Some(shown) => {
                            shown.posts.posts.extend(page.posts.posts);
                            shown.posts.more = page.posts.more;
                        }
                        None => self.list = Some(page),
                    },
                    Err(p) => {
                        self.list = None;
                        self.problem = Some(p);
                    }
                }
            }
            Msg::ShowListMembers(show) => {
                let Some(id) = self.list.as_ref().map(|l| l.list.id.clone()).filter(|_| show) else {
                    self.list_members = None;
                    return Task::none();
                };
                let key = id.clone();
                return self.run(move |c, t| async move { pages::list_members(&c, &t, &id).await }, move |r| Msg::ListMembers(key.clone(), r));
            }
            Msg::ListMembers(id, result) => {
                if self.screen != Some(Screen::List(id.clone())) {
                    return Task::none();
                }
                match result {
                    Ok(members) => self.list_members = Some((id, members)),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::NewListName(text) => self.new_list.0 = text.chars().take(pages::LIST_NAME_MAX_CHARS).collect(),
            Msg::NewListAbout(text) => self.new_list.1 = text.chars().take(pages::LIST_ABOUT_MAX_CHARS).collect(),
            Msg::CreateList => {
                let (name, about) = (self.new_list.0.trim().to_string(), self.new_list.1.trim().to_string());
                if name.is_empty() {
                    return Task::none();
                }
                return self.run(move |c, t| async move { pages::list_create(&c, &t, &name, &about).await }, Msg::ListCreated);
            }
            Msg::ListCreated(Ok(made)) => {
                self.new_list = (String::new(), String::new());
                self.note = Some(format!("{} is made. Add people and organizations from their pages.", made.name));
                return self.go(Screen::List(made.id));
            }
            Msg::ListCreated(Err(p)) => self.problem = Some(p),
            Msg::EditList(open) => {
                self.edit_list = if open { self.list.as_ref().map(|l| (l.list.id.clone(), l.list.name.clone(), l.list.about.clone())) } else { None };
            }
            Msg::EditListName(text) => {
                if let Some(form) = self.edit_list.as_mut() {
                    form.1 = text.chars().take(pages::LIST_NAME_MAX_CHARS).collect();
                }
            }
            Msg::EditListAbout(text) => {
                if let Some(form) = self.edit_list.as_mut() {
                    form.2 = text.chars().take(pages::LIST_ABOUT_MAX_CHARS).collect();
                }
            }
            Msg::SaveList => {
                // A list needs a name: the form stays open until it has one.
                let Some((id, name, about)) = self.edit_list.clone().filter(|f| !f.1.trim().is_empty()) else { return Task::none() };
                self.edit_list = None;
                return self.run(
                    move |c, t| async move { pages::list_update(&c, &t, &id, Some(name.trim()), Some(about.trim())).await },
                    |r| Msg::Done(r.map(|_| "Saved.".to_string())),
                );
            }
            Msg::DeleteList(id) => {
                if self.confirm_delete_list.as_deref() != Some(id.as_str()) {
                    self.confirm_delete_list = Some(id);
                    return Task::none();
                }
                self.confirm_delete_list = None;
                return self.run(
                    move |c, t| async move { pages::list_delete(&c, &t, &id).await },
                    |r| match r {
                        Ok(()) => Msg::Go(Screen::Lists),
                        Err(p) => Msg::Done(Err(p)),
                    },
                );
            }
            Msg::PickLists(target) => {
                let Some(target) = target else {
                    self.list_picker = None;
                    return Task::none();
                };
                let key = target.clone();
                return self.run(move |c, t| async move { pages::lists(&c, &t, Some(&target)).await }, move |r| Msg::ListsFor(key.clone(), r));
            }
            Msg::ListsFor(target, result) => match result {
                Ok(found) => {
                    if self.list_target_on_show().as_ref() == Some(&target) {
                        self.list_picker = Some((target, found));
                    }
                }
                Err(p) => self.problem = Some(p),
            },
            Msg::ListTakeOff(list_id, target) => {
                return self.done(move |c, t| async move { pages::list_add(&c, &t, &list_id, &target, false).await }, "Taken off the list.".into());
            }
            Msg::SaveDraft => return self.save_draft(None),
            Msg::ScheduleOn(on) => {
                self.composer.schedule_on = on;
                if on && self.composer.schedule_day.is_empty() {
                    // An hour from now, on the next quarter hour.
                    let at = ((crate::utc::now() / 900.0).ceil() * 900.0 + 3600.0).floor();
                    let full = crate::utc::full(at);
                    self.composer.schedule_day = full[..10].to_string();
                    self.composer.schedule_time = full[11..16].to_string();
                }
            }
            Msg::ScheduleDay(day) => self.composer.schedule_day = day.chars().take(10).collect(),
            Msg::ScheduleTime(time) => self.composer.schedule_time = time.chars().take(5).collect(),
            Msg::Schedule => {
                if let Some((at, seconds)) = self.composer.schedule_at()
                    && seconds >= crate::utc::now() + 60.0
                {
                    return self.save_draft(Some(at));
                }
                self.note = Some("Choose a day as 2026-10-15 and a time as 09:30, in UTC, at least a minute from now.".into());
            }
            Msg::DraftSaved(result) => {
                self.composer.sending = false;
                match result {
                    Ok(draft) => {
                        self.note = Some(match draft.publish_at.as_deref().and_then(crate::utc::parse) {
                            Some(at) => format!("Scheduled for {} UTC. It is in Drafts until then.", &crate::utc::full(at)[..16]),
                            None => "Saved to Drafts.".into(),
                        });
                        self.composer = Composer { as_who: self.composer.as_who.take(), ..Composer::default() };
                        return self.fetch_me();
                    }
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::Drafts(result) => {
                if self.screen != Some(Screen::Drafts) {
                    return Task::none();
                }
                self.loading = false;
                match result {
                    Ok(found) => self.drafts = Some(found),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::OpenDraft(id) => {
                let Some(draft) = self.drafts.iter().flatten().find(|d| d.id == id).cloned() else { return Task::none() };
                self.composer = Composer::from_draft(&draft);
                let show = self.go(Screen::Home);
                let Some(post_id) = draft.post.repost_of else { return show };
                let read = self.run(move |c, t| async move { pages::thread(&c, &t, &post_id, None).await }, move |r| Msg::DraftQuote(id.clone(), r));
                return Task::batch([show, read]);
            }
            Msg::DraftQuote(id, result) => {
                if self.composer.draft_id.as_deref() != Some(id.as_str()) || self.composer.draft_quote.is_none() {
                    return Task::none();
                }
                match result {
                    Ok(thread) => {
                        self.composer.quoting = Some(thread.post);
                        self.composer.draft_quote = None;
                    }
                    Err(_) => self.note = Some("The post this draft quotes could not be read. Cancel the quote to post it without.".into()),
                }
            }
            Msg::PostDraft(id) => {
                return self.run(move |c, t| async move { pages::draft_publish(&c, &t, &id).await }, |r| Msg::Done(r.map(|_| "Posted.".to_string())));
            }
            Msg::DeleteDraft(id) => {
                if self.confirm_delete_draft.as_deref() != Some(id.as_str()) {
                    self.confirm_delete_draft = Some(id);
                    return Task::none();
                }
                self.confirm_delete_draft = None;
                if self.composer.draft_id.as_deref() == Some(id.as_str()) {
                    // What the composer holds stays, as a new post.
                    self.composer.draft_id = None;
                }
                return self.done(move |c, t| async move { pages::draft_delete(&c, &t, &id).await }, "Draft deleted.".into());
            }
            Msg::CloseDraft => self.composer = Composer { as_who: self.composer.as_who.take(), ..Composer::default() },
            Msg::PostMenu(id) => {
                self.post_menu = id;
                self.repost_menu = None;
                self.reacting = None;
            }
            Msg::RepostMenu(id) => {
                self.repost_menu = id;
                self.post_menu = None;
                self.reacting = None;
            }
            Msg::Reacting(id) => {
                self.reacting = id;
                self.post_menu = None;
                self.repost_menu = None;
            }
            Msg::CopyLink(id) => {
                self.note = Some("Link copied.".into());
                return iced::clipboard::write(post_link(&id));
            }
            Msg::Compose => {
                let home = self.go(Screen::Home);
                return Task::batch([home, iced::widget::operation::focus(keys::COMPOSER)]);
            }
            Msg::Key(key) => return self.key(key),
            Msg::ViewPhoto(at) => self.viewing = at,
            Msg::PhotoStep(by) => {
                if let Some((id, at)) = self.viewing.clone() {
                    let count = self.shown_posts().into_iter().find(|p| p.id == id).map_or(0, |p| p.media.len());
                    if count > 0 {
                        self.viewing = Some((id, (at as i64 + i64::from(by)).rem_euclid(count as i64) as usize));
                    }
                }
            }
            Msg::NotifView(view) => self.notif_view = view,
            Msg::PersonTab(tab) => {
                let read_again = tab.show() != self.person_tab.show();
                self.person_tab = tab;
                if read_again && let Some(screen) = self.screen.clone() {
                    return self.load(&screen, true);
                }
            }
            Msg::OrgTab(tab) => {
                self.org_tab = tab;
                let Some(Screen::Org(ticker)) = self.screen.clone() else { return Task::none() };
                if tab == OrgTab::Jobs && self.org_jobs.as_ref().is_none_or(|(of, _)| *of != ticker) {
                    let query = JobsQuery { ticker: Some(ticker.clone()), ..JobsQuery::default() };
                    return self.run(move |c, t| async move { pages::jobs(&c, &t, &query, None).await }, move |r| Msg::OrgJobs(ticker.clone(), r));
                }
                if tab == OrgTab::Products && self.org_products.as_ref().is_none_or(|(of, _)| *of != ticker) {
                    let key = ticker.clone();
                    return self.run(
                        move |c, t| async move { pages::products(&c, &t, ProductList::Newest, None, None, Some(&ticker), None).await },
                        move |r| Msg::OrgProducts(key.clone(), r),
                    );
                }
            }
            Msg::OrgProducts(ticker, result) => {
                if self.screen != Some(Screen::Org(ticker.clone())) {
                    return Task::none();
                }
                match result {
                    Ok(page) => self.org_products = Some((ticker, page)),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::OpenLink(url) => {
                // Only a web address the service kept: a product's https link.
                if url.starts_with("https://")
                    && let Err(why) = crate::signin::loopback::open_in_browser(&url)
                {
                    self.note = Some(why);
                }
            }
            Msg::OrgJobs(ticker, result) => {
                if self.screen != Some(Screen::Org(ticker.clone())) {
                    return Task::none();
                }
                match result {
                    Ok(jobs) => self.org_jobs = Some((ticker, jobs)),
                    Err(p) => self.problem = Some(p),
                }
            }
            Msg::PageMenu(open) => self.page_menu = open,
            Msg::Scrolled(y) => {
                // Near the end, the next page is read once, as the social sites do; Load more stays for the keys.
                if y.is_nan() || y < 0.9 || self.loading {
                    return Task::none();
                }
                let Some(key) = self.more_key() else { return Task::none() };
                if self.more_asked.as_deref() == Some(key.as_str()) {
                    return Task::none();
                }
                self.more_asked = Some(key);
                return self.update(Msg::More);
            }
            Msg::ListAdd(list_id, target, add) => {
                // The offer is read again after the change, so it shows what is so.
                let key = target.clone();
                return self.run(
                    move |c, t| async move {
                        pages::list_add(&c, &t, &list_id, &target, add).await?;
                        pages::lists(&c, &t, Some(&target)).await
                    },
                    move |r| Msg::ListsFor(key.clone(), r),
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

/// The photos a screen draws, in order, each address once, with the size it is drawn at.
#[derive(Default)]
struct Wanted(Vec<(String, Size)>);

impl Wanted {
    fn add(&mut self, url: Option<&String>, size: Size) {
        if let Some(url) = url
            && !self.0.iter().any(|(u, _)| u == url)
        {
            self.0.push((url.clone(), size));
        }
    }

    /// As its card draws it: the organization's logo or the author's photo, and its photos; a plain repost draws the
    /// post it reposts (a quote only names it).
    fn post(&mut self, p: &Post) {
        if p.is_plain_repost() {
            if let Some(inner) = &p.repost_of {
                self.post(inner);
            }
            return;
        }
        match (&p.org, &p.author) {
            (Some(o), _) => self.add(o.logo.as_ref(), Size::Small),
            (None, Some(a)) => self.add(a.avatar.as_ref(), Size::Small),
            (None, None) => {}
        }
        if let Some(product) = &p.product {
            self.add(product.logo.as_ref(), Size::Small);
        }
        for photo in &p.media {
            self.add(Some(&photo.url), Size::Large);
        }
    }

    /// Launches as a list draws them: each product's logo, and its post's.
    fn launches(&mut self, posts: Option<&[Post]>) {
        for p in posts.unwrap_or_default() {
            self.post(p);
        }
    }

    /// Products as a list draws them: each logo, and the picture of the person or organization that owns it.
    fn summaries(&mut self, list: &[pages::ProductSummary]) {
        for summary in list {
            self.add(summary.product.logo.as_ref(), Size::Small);
            match (&summary.org, &summary.owner) {
                (Some(o), _) => self.add(o.logo.as_ref(), Size::Small),
                (None, Some(a)) => self.add(a.avatar.as_ref(), Size::Small),
                (None, None) => {}
            }
        }
    }

    fn posts(&mut self, page: Option<&Posts>) {
        for p in page.map(|p| p.posts.as_slice()).unwrap_or_default() {
            self.post(p);
        }
    }
}

/// How many skills a page keeps, and how long one may be (pages.py `MAX_SKILLS`, `SKILL_MAX_CHARS`).
const MAX_SKILLS: usize = 50;
const SKILL_MAX_CHARS: usize = 60;

/// What is wrong with a title as typed, as the service would refuse it (pages.py `normalize_title`), or nothing.
pub(super) fn title_problem(text: &str) -> Option<&'static str> {
    let raw = unbracketed(text);
    if raw.is_empty() {
        return None;
    }
    if raw.contains(['[', ']', '(', ')']) {
        return Some("A title may not carry brackets or parentheses: the name line draws them.");
    }
    let parts: Vec<String> = raw.split('|').map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    if parts.iter().any(String::is_empty) {
        return Some("A title part may not be empty: one | between two parts.");
    }
    if parts.len() > 4 {
        return Some("A title has at most 4 parts.");
    }
    if parts.iter().any(|p| p.chars().count() > 30) {
        return Some("Each part of a title is at most 30 characters.");
    }
    if parts.join(" | ").chars().count() > pages::TITLE_MAX_CHARS {
        return Some("A title is at most 60 characters.");
    }
    None
}

/// A title as the page will show it: its parts tidied, one " | " between them.
pub(super) fn tidy_title(text: &str) -> String {
    let raw = unbracketed(text);
    if raw.is_empty() {
        return String::new();
    }
    raw.split('|').map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" | ")
}

/// A title without the one pair of brackets round it that the service drops.
fn unbracketed(text: &str) -> &str {
    let raw = text.trim();
    match raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        Some(inner) => inner.trim(),
        None => raw,
    }
}

/// A month as the service keeps one (YYYY-MM, 1950 to 2099), and a year.
pub(super) fn is_month(text: &str) -> bool {
    let (year, month) = text.split_once('-').unwrap_or(("", ""));
    is_year(year) && month.len() == 2 && month.bytes().all(|b| b.is_ascii_digit()) && (1..=12).contains(&month.parse::<u32>().unwrap_or(0))
}

pub(super) fn is_year(text: &str) -> bool {
    text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit()) && (1950..=2099).contains(&text.parse::<u32>().unwrap_or(0))
}

/// What is wrong with the position in the form, said before anything is sent, or nothing.
pub(super) fn position_problem(x: &NewExperience, now: bool) -> Option<&'static str> {
    let (ticker, started, ended) = (x.ticker.trim(), x.started.trim(), x.ended.trim());
    if x.title.trim().is_empty() {
        return Some("Name the position.");
    }
    if !ticker.is_empty() && !((2..=5).contains(&ticker.len()) && ticker.bytes().all(|b| b.is_ascii_alphabetic())) {
        return Some("A ticker is 2-5 letters.");
    }
    if !is_month(started) {
        return Some("Say when it began: YYYY-MM, between 1950 and 2099.");
    }
    if !now && !ended.is_empty() && !is_month(ended) {
        return Some("Say when it ended as YYYY-MM, or tick that you hold it now.");
    }
    if !now && !ended.is_empty() && ended < started {
        return Some("It cannot end before it began.");
    }
    None
}

/// What is wrong with the school in the form, or nothing.
pub(super) fn school_problem(y: &NewEducation) -> Option<&'static str> {
    let (from, to) = (y.started.trim(), y.ended.trim());
    if y.school.trim().is_empty() {
        return Some("Name the school.");
    }
    if (!from.is_empty() && !is_year(from)) || (!to.is_empty() && !is_year(to)) {
        return Some("A year is YYYY, between 1950 and 2099.");
    }
    if !from.is_empty() && !to.is_empty() && to < from {
        return Some("It cannot end before it began.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use alelyon_identity_client::pages::{Membership, Person};

    fn org(ticker: &str) -> Org {
        Org { id: format!("org_{ticker}"), ticker: ticker.into(), name: ticker.into(), ..Org::default() }
    }

    fn me_showing(tickers: &[&str]) -> Me {
        Me {
            me: Person { id: "me".into(), username: Some("me".into()), tickers: tickers.iter().map(|t| org(t)).collect(), ..Person::default() },
            bio: "Bio".into(),
            memberships: ["AAA", "BBB", "CCC"]
                .iter()
                .map(|t| Membership { org: org(t), role: "member".into(), state: "active".into(), shown: tickers.contains(t), confirmed_title: None })
                .collect(),
            ..Me::default()
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
    fn find_opens_a_person_an_organization_a_hashtag_or_a_search() {
        let mut s = State::default();
        for (typed, screen) in [
            ("@Ada", Screen::Person("ada".into())),
            ("ada lovelace", Screen::Search("ada lovelace".into())),
            ("#2026", Screen::Search("#2026".into())),
            ("#Looms", Screen::Tag("looms".into())),
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
        assert_eq!(s.screen, Some(Screen::Tag("looms".into())));
        let _ = s.update(Msg::Go(Screen::Explore));
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Explore), "a tab starts the way back afresh");
    }

    #[test]
    fn home_shows_for_you_or_following_and_pages_the_ranked_feed_by_number() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Home));
        assert_eq!(s.home_feed, HomeFeed::Following);
        let _ = s.update(Msg::HomeFeed(HomeFeed::ForYou));
        let post = |id: &str| Post { id: id.into(), ..Post::default() };
        let _ = s.update(Msg::ForYou(0, Ok(Posts { posts: vec![post("pst_a")], more: true })));
        let _ = s.update(Msg::ForYou(1, Ok(Posts { posts: vec![post("pst_b")], more: false })));
        let (page, shown) = s.for_you.as_ref().expect("the ranked feed");
        assert_eq!((*page, shown.posts.len(), shown.more), (1, 2, false));
        let _ = s.update(Msg::HomeFeed(HomeFeed::Following));
        let _ = s.update(Msg::ForYou(0, Ok(Posts::default())));
        assert_eq!(s.for_you.as_ref().map(|(_, p)| p.posts.len()), Some(2), "an answer for the other feed is dropped");
    }

    #[test]
    fn a_hashtag_and_a_search_page_on_and_drop_answers_for_another() {
        let mut s = State::default();
        let post = |id: &str| Post { id: id.into(), ..Post::default() };
        let page = |ids: &[&str], more: bool| Posts { posts: ids.iter().map(|i| post(i)).collect(), more };
        let _ = s.update(Msg::Go(Screen::Tag("looms".into())));
        assert_eq!(s.screen.as_ref().map(Screen::title).as_deref(), Some("#looms"));
        let _ = s.update(Msg::Tagged("mills".into(), false, Ok(Tagged { tag: "mills".into(), posts: page(&["pst_0"], false) })));
        assert!(s.tagged.is_none(), "another tag's answer is dropped");
        let _ = s.update(Msg::Tagged("looms".into(), false, Ok(Tagged { tag: "looms".into(), posts: page(&["pst_1"], true) })));
        let _ = s.update(Msg::Tagged("looms".into(), true, Ok(Tagged { tag: "looms".into(), posts: page(&["pst_2"], false) })));
        assert_eq!(s.tagged.as_ref().map(|t| t.posts.posts.len()), Some(2));
        let _ = s.update(Msg::Go(Screen::Search("ada".into())));
        let _ = s.update(Msg::SearchKind(SearchKind::Posts));
        let _ = s.update(Msg::Found("ada".into(), SearchKind::All, false, Ok(Found::default())));
        assert!(s.found.is_none(), "an answer for another part is dropped");
        let _ = s.update(Msg::Found("ada".into(), SearchKind::Posts, false, Ok(Found { posts: page(&["pst_3"], true), ..Found::default() })));
        assert_eq!(s.next_page().as_deref(), Some("pst_3"));
        let _ = s.update(Msg::SearchKind(SearchKind::People));
        assert_eq!(s.next_page(), None, "only a posts search pages on");
    }

    #[test]
    fn a_new_article_as_an_organization_carries_a_job_only_when_asked() {
        let mut s = State::default();
        s.composer.kind = Kind::Article;
        s.composer.title = "Engineer".into();
        s.composer.body = text_editor::Content::with_text("Build the engine with us.");
        s.composer.as_who = Some("BABG".into());
        assert_eq!(s.composer.new_post().job, None);
        let _ = s.update(Msg::JobToggle);
        assert!(!s.composer.ready(), "an on-site job needs a place");
        let _ = s.update(Msg::JobLocation("London".into()));
        let _ = s.update(Msg::JobEmployment(Employment::Contract));
        let _ = s.update(Msg::JobDays(JobDays(60)));
        assert!(s.composer.ready());
        let job = s.composer.new_post().job.expect("a job");
        assert_eq!((job.location.as_str(), job.workplace, job.employment, job.days), ("London", Workplace::OnSite, Employment::Contract, 60));
        let _ = s.update(Msg::JobWorkplace(Workplace::Remote));
        let _ = s.update(Msg::JobLocation(String::new()));
        assert!(s.composer.ready(), "a remote job needs no place");
        s.composer.as_who = None;
        assert_eq!(s.composer.new_post().job, None, "only an organization opens a job");
        assert_eq!(JobDays::ALL.map(|d| d.to_string()), ["1 week", "2 weeks", "30 days", "60 days", "90 days"]);
        assert_eq!(WorkplacePick::ALL[0].to_string(), "Any workplace");
        let _ = s.update(Msg::JobsWorkplace(WorkplacePick(Some(Workplace::Hybrid))));
        let _ = s.update(Msg::JobsWords("engine".into()));
        assert_eq!((s.jobs_query.workplace, s.jobs_query.words.as_str()), (Some(Workplace::Hybrid), "engine"));
        let _ = s.update(Msg::Applying(Some("pst_1".into())));
        let _ = s.update(Msg::ApplyNote("I built the first one.".into()));
        assert_eq!(s.applying, Some(("pst_1".into(), "I built the first one.".into())));
        let _ = s.update(Msg::Apply);
        assert_eq!(s.applying, None, "sent, the note box closes");
        assert!(Screen::TABS.contains(&Screen::Jobs));
    }

    #[test]
    fn a_new_post_carries_its_reply_rule_and_a_poll_only_when_complete() {
        let mut s = State::default();
        s.composer.body = text_editor::Content::with_text("Which engine?");
        let _ = s.update(Msg::Replies(ReplyRule::Followers));
        let _ = s.update(Msg::PollToggle);
        let _ = s.update(Msg::PollOption(0, "Difference".into()));
        assert!(!s.composer.ready(), "a poll needs two options");
        let _ = s.update(Msg::PollOption(1, " Analytical ".into()));
        let _ = s.update(Msg::PollHours(PollLength(72)));
        assert!(s.composer.ready());
        let new = s.composer.new_post();
        assert_eq!(new.replies, ReplyRule::Followers);
        assert_eq!(new.poll, Some(NewPoll { options: vec!["Difference".into(), "Analytical".into()], hours: 72 }));
        let _ = s.update(Msg::PollToggle);
        assert_eq!(s.composer.new_post().poll, None);
        assert_eq!(PollLength::ALL.map(|l| l.to_string()), ["1 hour", "1 day", "3 days", "7 days"]);
        let _ = s.update(Msg::Posted(Ok(Post::default())));
        assert_eq!((s.composer.replies, s.composer.poll_on), (ReplyRule::Everyone, false), "a post sent starts the next afresh");
    }

    #[test]
    fn a_title_is_held_to_the_services_rules_before_it_is_sent() {
        assert_eq!(title_problem(""), None);
        assert_eq!(title_problem("[ Founder | CEO ]"), None, "one pair of brackets round it is dropped, as the service drops it");
        assert_eq!(tidy_title("[ Founder|  CEO ]"), "Founder | CEO");
        assert!(title_problem("Founder (CEO)").is_some_and(|w| w.contains("brackets")));
        assert!(title_problem("A | | B").is_some_and(|w| w.contains("empty")));
        assert!(title_problem("A | B | C | D | E").is_some_and(|w| w.contains("4 parts")));
        assert!(title_problem(&"x".repeat(31)).is_some_and(|w| w.contains("30 characters")));
        assert!(title_problem(&["abcdefghijklmn"; 4].join(" | ")).is_some_and(|w| w.contains("60 characters")));
        assert!(is_month("1950-01") && is_month("2099-12") && !is_month("1949-12") && !is_month("2021-13") && !is_month("2021-3"));
        assert!(is_year("2016") && !is_year("16") && !is_year("2100"));
    }

    #[test]
    fn my_page_saves_only_a_change_the_service_would_take_and_can_put_it_back() {
        let mut s = State::default();
        let me = Me { me: Person { display_name: "Ada".into(), username: Some("ada".into()), has_page: true, ..Person::default() }, ..Me::default() };
        let _ = s.update(Msg::Me(Ok(me)));
        let _ = s.update(Msg::SaveMine);
        assert!(!s.mine.saving, "nothing changed: nothing is sent");
        let _ = s.update(Msg::MineTitle("A | B | C | D | E".into()));
        let _ = s.update(Msg::SaveMine);
        assert!(!s.mine.saving, "a title the service would refuse is not sent");
        let _ = s.update(Msg::MineTitle("Founder | CEO".into()));
        let _ = s.update(Msg::DiscardMine);
        assert_eq!((s.mine.title.as_str(), s.mine.dirty), ("", false), "Discard puts back what the service said");
        let _ = s.update(Msg::MineTitle("Founder".into()));
        let _ = s.update(Msg::SaveMine);
        assert!(s.mine.saving);
        let _ = s.update(Msg::MineTop(Ok(PersonPage { banner: Some("https://media.alelyon.com/b.webp".into()), followers: 3, ..PersonPage::default() })));
        assert_eq!(s.mine.top.as_ref().map(|t| t.followers), Some(3), "the top is drawn from the page as others see it");
    }

    #[test]
    fn the_background_forms_open_on_demand_say_a_bad_month_and_delete_on_a_second_press() {
        let mut s = State::default();
        assert!(!s.profile_form.exp_open);
        let _ = s.update(Msg::EntryOpen(true));
        assert!(s.profile_form.exp_open);
        let _ = s.update(Msg::ExpField(ExpField::Title, "Founder".into()));
        let _ = s.update(Msg::ExpField(ExpField::Started, "2021-13".into()));
        assert!(position_problem(&s.profile_form.experience, false).is_some_and(|w| w.contains("began")));
        let _ = s.update(Msg::ExpField(ExpField::Started, "2021-03".into()));
        let _ = s.update(Msg::ExpField(ExpField::Ended, "2020-01".into()));
        assert!(position_problem(&s.profile_form.experience, false).is_some_and(|w| w.contains("before")));
        let _ = s.update(Msg::ExpNow(true));
        assert_eq!((s.profile_form.experience.ended.as_str(), position_problem(&s.profile_form.experience, true)), ("", None), "held now: no end");
        let _ = s.update(Msg::ProfileSaved(ProfilePart::Experience, Ok(())));
        assert!(!s.profile_form.exp_open, "a saved position closes its form");
        let _ = s.update(Msg::EntryOpen(false));
        let _ = s.update(Msg::EduField(EduField::School, "UCL".into()));
        let _ = s.update(Msg::EduField(EduField::Started, "16".into()));
        assert!(school_problem(&s.profile_form.education).is_some_and(|w| w.contains("YYYY")));
        let _ = s.update(Msg::ProfileClear);
        assert!(!s.profile_form.edu_open);
        let _ = s.update(Msg::DeleteExperience("exp_1".into()));
        assert_eq!(s.profile_form.confirm_delete.as_deref(), Some("exp_1"), "the first press asks");
        let _ = s.update(Msg::DeleteExperience("exp_1".into()));
        assert!(s.profile_form.confirm_delete.is_none(), "the second deletes");
    }

    #[test]
    fn the_profile_forms_fill_from_an_entry_and_empty_only_once_saved() {
        let mut s = State::default();
        let entry = Experience {
            id: "exp_1".into(),
            title: "Founder".into(),
            org: Some(Org { id: "org_1".into(), ticker: "LACE".into(), name: "Lace Labs".into(), ..Org::default() }),
            org_name: "Lace Labs".into(),
            started: "2024-01".into(),
            ..Experience::default()
        };
        let _ = s.update(Msg::ExpEdit(entry));
        let f = &s.profile_form.experience;
        assert_eq!((f.id.as_str(), f.ticker.as_str(), f.org_name.as_str(), f.ended.as_str()), ("exp_1", "LACE", "", ""));
        let _ = s.update(Msg::ExpField(ExpField::Ended, "2025-06".into()));
        let _ = s.update(Msg::ProfileSaved(ProfilePart::Experience, Err(Problem::Unreachable("down".into()))));
        assert_eq!(s.profile_form.experience.ended, "2025-06", "a failed save keeps what was typed");
        let _ = s.update(Msg::ProfileSaved(ProfilePart::Experience, Ok(())));
        assert_eq!(s.profile_form.experience, NewExperience::default());
        let _ = s.update(Msg::EduField(EduField::School, "UCL".into()));
        let _ = s.update(Msg::ProfileClear);
        assert_eq!(s.profile_form.education, NewEducation::default());
    }

    #[test]
    fn the_skills_line_follows_the_service_until_it_is_edited() {
        let mut s = State::default();
        let me = |skills: &[&str]| Me {
            profile: pages::Profile {
                skills: skills.iter().map(|k| pages::Skill { skill: k.to_string(), ..pages::Skill::default() }).collect(),
                ..pages::Profile::default()
            },
            ..Me::default()
        };
        let _ = s.update(Msg::Me(Ok(me(&["Mathematics", "Engines"]))));
        assert_eq!(s.profile_form.skills, ["Mathematics", "Engines"]);
        let _ = s.update(Msg::SkillRemove(0));
        let _ = s.update(Msg::SkillNew("Looms, engines,  Steam   power".into()));
        let _ = s.update(Msg::SkillAdd);
        assert_eq!(s.profile_form.skills, ["Engines", "Looms", "Steam power"], "a comma parts them; one already there is not added twice");
        assert!(s.profile_form.skill_new.is_empty());
        let _ = s.update(Msg::Me(Ok(me(&["Mathematics"]))));
        assert_eq!(s.profile_form.skills, ["Engines", "Looms", "Steam power"], "what is being changed stays");
        let _ = s.update(Msg::ProfileSaved(ProfilePart::Skills, Ok(())));
        let _ = s.update(Msg::Me(Ok(me(&["Looms"]))));
        assert_eq!(s.profile_form.skills, ["Looms"]);
        let _ = s.update(Msg::Go(Screen::OrgExperience("LACE".into())));
        let _ = s.update(Msg::OrgExperience("ABCD".into(), Ok(vec![])));
        assert!(s.org_experience.is_none(), "another organization's answer is dropped");
        let _ = s.update(Msg::OrgExperience("LACE".into(), Ok(vec![])));
        assert_eq!(s.org_experience.as_ref().map(|(t, l)| (t.as_str(), l.len())), Some(("LACE", 0)));
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
    fn the_flagged_posts_page_on_and_drops_an_answer_for_another_organization() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Org("LACE".into())));
        let _ = s.update(Msg::Go(Screen::Flagged("LACE".into())));
        assert_eq!(s.screen.as_ref().map(Screen::title).as_deref(), Some("(LACE) flagged posts"));
        let post = |id: &str| Post { id: id.into(), ..Post::default() };
        let _ = s.update(Msg::Flagged("ABCD".into(), false, Ok(Posts { posts: vec![post("pst_0")], more: false })));
        assert!(s.flagged.is_none(), "an answer for another organization is dropped");
        let _ = s.update(Msg::Flagged("LACE".into(), false, Ok(Posts { posts: vec![post("pst_1")], more: true })));
        assert_eq!(s.next_page().as_deref(), Some("pst_1"), "load more starts after the last post");
        let _ = s.update(Msg::Flagged("LACE".into(), true, Ok(Posts { posts: vec![post("pst_2")], more: false })));
        assert_eq!(s.flagged.as_ref().map(|(_, p)| (p.posts.len(), p.more)), Some((2, false)));
        assert!(!s.loading);
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Org("LACE".into())));
    }

    #[test]
    fn the_my_page_form_reads_the_name_and_marks_an_edit_to_it() {
        let mut s = State::default();
        let mut me = me_showing(&[]);
        me.me.display_name = "[ Founder | CEO ] Thomas Lacy".into();
        let _ = s.update(Msg::Me(Ok(me)));
        assert_eq!(s.mine.name, "[ Founder | CEO ] Thomas Lacy");
        assert!(!s.mine.dirty);
        let _ = s.update(Msg::MineName("Thomas Lacy".into()));
        assert!(s.mine.dirty && s.mine.name == "Thomas Lacy");
    }

    #[test]
    fn notifications_are_a_tab_that_marks_what_it_shows_read() {
        let mut s = State::default();
        let mut me = me_showing(&[]);
        me.unread_notifications = 2;
        let _ = s.update(Msg::Me(Ok(me)));
        let _ = s.update(Msg::Go(Screen::Notifications));
        assert!(Screen::TABS.contains(&Screen::Notifications));
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Notifications), "a tab: nothing to go back to");
        let unread = |id: &str| pages::Notification { id: id.into(), kind: "like".into(), ..pages::Notification::default() };
        let page = Notifications { items: vec![unread("ntf_2"), unread("ntf_1")], more: true, unread: 2 };
        let _ = s.update(Msg::Notifications(false, Ok(page)));
        assert_eq!(s.next_page().as_deref(), Some("ntf_1"));
        let _ = s.update(Msg::NotificationsRead(Ok(0)));
        assert_eq!(s.me.as_ref().map(|m| m.unread_notifications), Some(0));
        let _ = s.update(Msg::Notifications(true, Ok(Notifications { items: vec![unread("ntf_0")], more: false, unread: 0 })));
        assert_eq!(s.notifications.as_ref().map(|n| (n.items.len(), n.more)), Some((3, false)));
        let _ = s.update(Msg::Go(Screen::Home));
        let _ = s.update(Msg::Notifications(false, Ok(Notifications::default())));
        assert_eq!(s.notifications.as_ref().map(|n| n.items.len()), Some(3), "an answer for another screen is dropped");
    }

    #[test]
    fn bookmarks_are_a_tab_that_pages_on() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Bookmarks));
        assert!(Screen::TABS.contains(&Screen::Bookmarks));
        let post = |id: &str| Post { id: id.into(), bookmarked: true, ..Post::default() };
        let _ = s.update(Msg::Bookmarks(false, Ok(Posts { posts: vec![post("pst_2")], more: true })));
        assert_eq!(s.next_page().as_deref(), Some("pst_2"));
        let _ = s.update(Msg::Bookmarks(true, Ok(Posts { posts: vec![post("pst_1")], more: false })));
        assert_eq!(s.bookmarks.as_ref().map(|b| (b.posts.len(), b.more)), Some((2, false)));
        let _ = s.update(Msg::Go(Screen::Home));
        let _ = s.update(Msg::Bookmarks(false, Ok(Posts::default())));
        assert_eq!(s.bookmarks.as_ref().map(|b| b.posts.len()), Some(2), "an answer for another screen is dropped");
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

    fn report(id: &str) -> ModReport {
        ModReport { id: id.into(), state: "open".into(), ..ModReport::default() }
    }

    #[test]
    fn the_moderation_screen_keeps_only_the_answer_for_the_view_on_show() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Person("ada".into())));
        let _ = s.update(Msg::Go(Screen::Moderation));
        assert_eq!(s.screen, Some(Screen::Moderation));
        let _ = s.update(Msg::Back);
        assert_eq!(s.screen, Some(Screen::Moderation), "a tab: nothing to go back to");
        let _ = s.update(Msg::ModReports(ModView::Open, false, Ok((vec![report("rpt_1")], true))));
        assert_eq!(s.mod_reports.as_ref().map(|(r, more)| (r.len(), *more)), Some((1, true)));
        assert_eq!(s.next_page().as_deref(), Some("rpt_1"), "load more starts after the last report");
        let _ = s.update(Msg::ModReports(ModView::Open, true, Ok((vec![report("rpt_2")], false))));
        assert_eq!(s.mod_reports.as_ref().map(|(r, more)| (r.len(), *more)), Some((2, false)));
        let _ = s.update(Msg::ModView(ModView::Log));
        assert!(s.mod_reports.is_none() && s.loading);
        let _ = s.update(Msg::ModReports(ModView::Open, false, Ok((vec![report("rpt_3")], false))));
        assert!(s.mod_reports.is_none(), "an answer for another view is dropped");
        let _ = s.update(Msg::ModLog(false, Ok((vec![ModAction { id: "mod_1".into(), ..ModAction::default() }], false))));
        assert!(!s.loading && s.mod_log.as_ref().is_some_and(|(a, _)| a.len() == 1));
    }

    #[test]
    fn a_finished_act_clears_the_note_and_says_what_was_done() {
        let mut s = State::default();
        let _ = s.update(Msg::ModNote("Spam link.".into()));
        let _ = s.update(Msg::ModDone(Ok(ModAction { action: "hide-post".into(), ..ModAction::default() })));
        assert!(s.mod_note.is_empty());
        assert_eq!(s.note.as_deref(), Some("You hid a post."));
        let _ = s.update(Msg::ModDone(Err(Problem::Refused("not-a-moderator".into(), "Only moderators.".into()))));
        assert!(s.problem.is_some());
    }

    #[test]
    fn a_new_ticker_is_letters_only_upper_case_and_at_most_five() {
        let mut s = State::default();
        let _ = s.update(Msg::NewTicker("la-ce99xyz".into()));
        assert_eq!(s.new_org.ticker, "LACEX");
    }

    fn photo_url(id: &str, ext: &str) -> String {
        format!("https://media.alelyon.com/m/med_{id}abcdefgh.{ext}")
    }

    fn uploaded(id: &str) -> Upload {
        Upload { id: format!("med_{id}"), url: photo_url(id, "png"), width: 4, height: 3 }
    }

    fn signed_in() -> State {
        // the tasks a test's updates return are never run: nothing is sent to this address
        let c = Client::new("http://127.0.0.1:9".into()).unwrap();
        State { session: Some((c, "t".into())), ..State::default() }
    }

    #[test]
    fn a_post_carries_up_to_four_photos_with_their_descriptions_and_never_a_poll() {
        let mut s = State::default();
        for i in 0..5 {
            s.uploading += 1;
            let _ = s.update(Msg::Uploaded(PhotoFor::Post, format!("{i}.png"), Ok((uploaded(&i.to_string()), Picture::Failed))));
        }
        assert_eq!((s.composer.photos.len(), s.uploading), (4, 0), "the fifth is left to expire unused");
        assert!(s.photos.contains_key(&photo_url("0", "png")), "an upload's picture is held without a fetch");
        let _ = s.update(Msg::PhotoAlt(1, "x".repeat(1_200)));
        assert_eq!(s.composer.photos[1].alt.chars().count(), pages::ALT_MAX_CHARS);
        let _ = s.update(Msg::PhotoAlt(0, " A red card ".into()));
        let _ = s.update(Msg::RemovePhoto(3));
        let _ = s.update(Msg::PollToggle);
        assert!(!s.composer.poll_on, "no poll while photos are attached");
        s.composer.body = text_editor::Content::with_text("Cards");
        let new = s.composer.new_post();
        assert_eq!(new.media.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["med_0", "med_1", "med_2"]);
        assert_eq!(new.media[0].alt, "A red card");
        s.uploading = 1;
        let _ = s.update(Msg::Submit);
        assert!(!s.composer.sending, "not sent while a photo uploads");
        s.uploading = 0;
        let _ = s.update(Msg::Submit);
        assert!(s.composer.sending);
        s.composer.sending = false;
        for _ in 0..3 {
            let _ = s.update(Msg::RemovePhoto(0));
        }
        let _ = s.update(Msg::PollToggle);
        assert!(s.composer.poll_on);
        let _ = s.update(Msg::Uploaded(PhotoFor::Post, "late.png".into(), Ok((uploaded("9"), Picture::Failed))));
        assert!(s.composer.photos.is_empty(), "a photo that arrives once a poll is chosen is not attached");
    }

    #[test]
    fn chosen_files_are_refused_with_their_reasons_and_only_as_many_as_fit_are_uploaded() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let mut files: Vec<(String, Result<Vec<u8>, String>)> = (0..6).map(|i| (format!("{i}.png"), Ok(png.clone()))).collect();
        files.push(("x.gif".into(), Err("x.gif is not a JPEG, PNG or WebP image.".into())));
        let mut s = State::default();
        let _ = s.update(Msg::PhotosChosen(PhotoFor::Post, Ok(files.clone())));
        assert_eq!(s.uploading, 0, "signed out: nothing uploads");
        let mut s = signed_in();
        s.composer.photos.push(Attached { id: "med_a".into(), ..Attached::default() });
        let _ = s.update(Msg::PhotosChosen(PhotoFor::Post, Ok(files)));
        assert_eq!(s.uploading, 3, "room for three beside the one attached");
        let Some(Problem::Refused(_, words)) = &s.problem else { panic!("{:?}", s.problem) };
        assert!(words.contains("3.png was left out") && words.contains("x.gif is not a JPEG"), "{words}");
        let mut s = signed_in();
        let _ = s.update(Msg::PhotosChosen(PhotoFor::Page("avatar"), Ok(vec![("a.png".into(), Ok(png))])));
        assert_eq!(s.uploading, 1);
        assert_eq!(PhotoFor::Page("avatar").done_words(false), "Your photo is changed.");
        assert_eq!(PhotoFor::Org("LACE".into(), "logo").done_words(true), "(LACE)'s logo is removed.");
    }

    #[test]
    fn the_screen_on_show_fetches_each_of_its_photos_once() {
        let photo = |url: String| pages::Photo { url, width: 4, height: 3, alt: String::new() };
        let ada = Person { id: "a".into(), avatar: Some(photo_url("face", "png")), ..Person::default() };
        let post = Post {
            id: "p1".into(),
            author: Some(ada),
            media: vec![photo(photo_url("one", "png")), photo(photo_url("two", "jpg"))],
            ..Post::default()
        };
        let repost = Post {
            id: "p2".into(),
            author: Some(Person { avatar: Some(photo_url("reposter", "png")), ..Person::default() }),
            repost_of_id: Some("p1".into()),
            repost_of: Some(Box::new(post.clone())),
            ..Post::default()
        };
        let mut s = signed_in();
        s.screen = Some(Screen::Home);
        s.home = Some(Posts { posts: vec![post, repost], more: false });
        assert_eq!(
            s.photos_on_show(),
            [(photo_url("face", "png"), Size::Small), (photo_url("one", "png"), Size::Large), (photo_url("two", "jpg"), Size::Large)],
            "a reposted post's photos once; a plain repost's own author is not drawn"
        );
        let _ = s.update(Msg::Dismiss);
        assert!(matches!(s.photos.get(&photo_url("one", "png")), Some(Picture::Loading)));
        assert!(matches!(s.photos.get(&photo_url("two", "jpg")), Some(Picture::Loading)), "a JPEG is fetched too");
        let _ = s.update(Msg::Photo(photo_url("one", "png"), Picture::Failed));
        assert!(matches!(s.photos.get(&photo_url("one", "png")), Some(Picture::Failed)), "not fetched again on its own");
        let _ = s.update(Msg::Refresh);
        assert!(matches!(s.photos.get(&photo_url("one", "png")), Some(Picture::Loading)), "Refresh fetches it again");
        for i in 0..=PHOTOS_KEPT {
            s.photos.insert(format!("old{i}"), Picture::Failed);
        }
        let _ = s.update(Msg::Dismiss);
        assert!(s.photos.keys().all(|u| !u.starts_with("old")), "once many are held, those not drawn are forgotten");
        assert!(s.photos.contains_key(&photo_url("one", "png")));
    }

    fn summary(handle: &str, role: Option<&str>, state: Option<&str>) -> CommunitySummary {
        CommunitySummary {
            community: Community { id: format!("com_{handle}"), handle: handle.into(), name: handle.into() },
            your_role: role.map(str::to_string),
            your_state: state.map(str::to_string),
            ..CommunitySummary::default()
        }
    }

    #[test]
    fn a_composer_posts_in_a_community_only_as_oneself_and_never_a_quote() {
        let mut s = State::default();
        let _ = s.update(Msg::InCommunity(InWhere(Some("engines".into()))));
        assert_eq!(s.composer.new_post().community.as_deref(), Some("engines"));
        s.composer.as_who = Some("LACE".into());
        assert_eq!(s.composer.new_post().community, None, "an organization's post is not a community's");
        s.composer.as_who = None;
        s.composer.quoting = Some(Post::default());
        assert_eq!(s.composer.new_post().community, None, "a quote stays outside");
        assert_eq!(InWhere(Some("engines".into())).to_string(), "c/engines");
        assert_eq!(InWhere(None).to_string(), "My page");
    }

    #[test]
    fn a_communitys_screen_keeps_only_its_own_answer_and_its_composer_posts_in_it() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Community("engines".into())));
        let page = |handle: &str| CommunityPage { summary: summary(handle, Some("member"), Some("active")), ..CommunityPage::default() };
        let _ = s.update(Msg::Community("looms".into(), false, Ok(page("looms"))));
        assert!(s.community.is_none(), "an answer for another community is dropped");
        let _ = s.update(Msg::Community("engines".into(), false, Ok(page("engines"))));
        assert_eq!(s.community.as_ref().map(|c| c.summary.community.handle.as_str()), Some("engines"));
        assert_eq!(s.composer.in_community.as_deref(), Some("engines"), "a member's composer posts here");
        let _ = s.update(Msg::MyCommunities(Ok(vec![summary("looms", Some("moderator"), Some("active"))])));
        assert_eq!(s.composer.in_community, None, "a community the reader left is not offered");
        assert!(s.keeps("looms") && !s.keeps("engines"));
        let _ = s.update(Msg::MyCommunities(Ok(vec![summary("looms", Some("moderator"), Some("requested"))])));
        assert!(!s.keeps("looms"), "only an active owner or moderator keeps one");
    }

    #[test]
    fn a_new_communitys_handle_is_tidied_as_the_service_reads_it() {
        let mut s = State::default();
        let _ = s.update(Msg::NewCommunityField(CommunityField::Handle, " C/Rust Aceans! ".into()));
        assert_eq!(s.new_community.handle, "rustaceans");
        let _ = s.update(Msg::NewCommunityField(CommunityField::Name, "Rustaceans".into()));
        assert!(s.new_community.ready());
        for (typed, ok) in [("ab", false), ("1engines", false), ("engines", true), (&"x".repeat(40)[..], true)] {
            let _ = s.update(Msg::NewCommunityField(CommunityField::Handle, typed.into()));
            assert_eq!(s.new_community.ready(), ok, "{typed}");
        }
        assert_eq!(s.new_community.handle.len(), 30, "cut to the longest a handle may be");
    }

    #[test]
    fn the_photo_matches_are_kept_only_while_their_view_is_on_show() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Moderation));
        let _ = s.update(Msg::ModView(ModView::Matches));
        let _ = s.update(Msg::MediaMatches(Ok(vec![MediaMatch { id: "mm_1".into(), ..MediaMatch::default() }])));
        assert_eq!(s.mod_matches.as_ref().map(Vec::len), Some(1));
        assert_eq!(s.next_page(), None, "the matches come in one page");
        let _ = s.update(Msg::ModView(ModView::Open));
        assert!(s.mod_matches.is_none());
        let _ = s.update(Msg::MediaMatches(Ok(vec![MediaMatch::default()])));
        assert!(s.mod_matches.is_none(), "an answer for another view is dropped");
    }

    fn a_list(id: &str, name: &str) -> PageList {
        PageList { id: id.into(), name: name.into(), ..PageList::default() }
    }

    fn person(username: &str) -> Person {
        Person { id: format!("acc_{username}"), username: Some(username.into()), ..Person::default() }
    }

    fn a_draft(id: &str, text: &str) -> Draft {
        Draft { id: id.into(), post: NewPost { text: text.into(), ..NewPost::default() }, ..Draft::default() }
    }

    #[test]
    fn a_draft_opens_in_the_composer_as_it_was_kept_and_posting_it_posts_the_draft() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Drafts));
        let mut kept = a_draft("drf_1", "Which loom?");
        kept.post.poll = Some(pages::NewPoll { options: vec!["Jacquard".into(), "Hand".into()], hours: 72 });
        kept.post.replies = ReplyRule::Followers;
        kept.publish_at = Some("2026-10-15T09:30:00Z".into());
        let mut quoting = a_draft("drf_2", "Well put");
        quoting.post.repost_of = Some("pst_9".into());
        let _ = s.update(Msg::Drafts(Ok(vec![kept, quoting])));
        let _ = s.update(Msg::OpenDraft("drf_1".into()));
        assert_eq!(s.screen, Some(Screen::Home), "a draft is written again on Home");
        let c = &s.composer;
        assert_eq!((c.draft_id.as_deref(), c.body.text().trim(), c.replies), (Some("drf_1"), "Which loom?", ReplyRule::Followers));
        assert!(c.poll_on && c.poll_options[..2] == ["Jacquard".to_string(), "Hand".to_string()] && c.poll_length == PollLength(72));
        assert_eq!((c.schedule_on, c.schedule_day.as_str(), c.schedule_time.as_str()), (true, "2026-10-15", "09:30"));
        assert_eq!(c.new_post().poll.map(|p| p.options.len()), Some(2));
        let _ = s.update(Msg::CloseDraft);
        assert!(s.composer.draft_id.is_none() && s.composer.body.text().trim().is_empty(), "closing empties the composer");
        let _ = s.update(Msg::OpenDraft("drf_2".into()));
        assert_eq!(s.composer.new_post().repost_of.as_deref(), Some("pst_9"), "its quote is kept before the post is read");
        let _ = s.update(Msg::DraftQuote("drf_2".into(), Err(Problem::Expired)));
        assert_eq!(s.composer.new_post().repost_of.as_deref(), Some("pst_9"), "and when it cannot be read");
        let _ = s.update(Msg::Unquote);
        assert_eq!(s.composer.new_post().repost_of, None, "until the quote is cancelled");
    }

    #[test]
    fn a_schedule_reads_only_a_real_day_and_time_in_utc() {
        let mut c = Composer { schedule_day: "2026-10-15".into(), schedule_time: "9:05".into(), ..Composer::default() };
        assert_eq!(c.schedule_at().map(|(at, _)| at), Some("2026-10-15T09:05:00Z".to_string()));
        for (day, time) in [("2026-02-31", "09:00"), ("2026-10-15", "24:00"), ("2026-10-15", "09:60"), ("15-10-2026", "09:00"), ("2026-10-15", "nine")] {
            c.schedule_day = day.into();
            c.schedule_time = time.into();
            assert_eq!(c.schedule_at(), None, "{day} {time}");
        }
        let mut s = State::default();
        let _ = s.update(Msg::ScheduleOn(true));
        let (_, at) = s.composer.schedule_at().expect("a default time an hour on");
        assert!(at > crate::utc::now() + 3000.0 && at < crate::utc::now() + 4600.0, "{at}");
        s.composer.kind = Kind::Article;
        s.composer.title = "Engineer".into();
        s.composer.body = text_editor::Content::with_text("Build the engine");
        s.composer.as_who = Some("LACE".into());
        assert!(s.composer.draftable(), "an article is drafted");
        s.composer.job_on = true;
        s.composer.job.workplace = Workplace::Remote;
        assert!(s.composer.ready() && !s.composer.draftable(), "a job opening is posted at once, not drafted");
    }

    #[test]
    fn deleting_a_draft_asks_first_and_frees_the_composer_writing_it() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Drafts));
        let _ = s.update(Msg::Drafts(Ok(vec![a_draft("drf_1", "One")])));
        let _ = s.update(Msg::OpenDraft("drf_1".into()));
        let _ = s.update(Msg::Go(Screen::Drafts));
        let _ = s.update(Msg::DeleteDraft("drf_1".into()));
        assert_eq!(s.confirm_delete_draft.as_deref(), Some("drf_1"), "the first press asks");
        let _ = s.update(Msg::DeleteDraft("drf_1".into()));
        assert_eq!((s.confirm_delete_draft.as_ref(), s.composer.draft_id.as_ref()), (None, None));
        assert_eq!(s.composer.body.text().trim(), "One", "the words stay, as a new post");
        let _ = s.update(Msg::Drafts(Ok(Vec::new())));
        let _ = s.update(Msg::Go(Screen::Home));
        let _ = s.update(Msg::Drafts(Ok(vec![a_draft("drf_2", "Two")])));
        assert_eq!(s.drafts.as_ref().map(Vec::len), Some(0), "an answer for Drafts off screen is dropped");
    }

    fn told(id: &str, kind: &str, who: &str, post: Option<&str>) -> Notification {
        Notification {
            id: id.into(),
            kind: kind.into(),
            actor: Some(Person { id: format!("acc_{who}"), username: Some(who.into()), ..Person::default() }),
            post: post.map(|p| Post { id: p.into(), ..Post::default() }),
            ..Notification::default()
        }
    }

    #[test]
    fn the_composer_says_why_it_cannot_post_exactly_when_it_cannot() {
        let mut c = Composer::default();
        assert_eq!(c.why_not().as_deref(), Some("Write something first"));
        c.body = text_editor::Content::with_text("Shipped it");
        assert!(c.ready() && c.why_not().is_none());
        c.poll_on = true;
        assert!(!c.ready());
        assert_eq!(c.why_not().as_deref(), Some("A poll needs at least two options"));
        c.poll_options = ["Yes".into(), "No".into(), String::new(), String::new()];
        assert!(c.ready() && c.why_not().is_none());
        c.kind = Kind::Article;
        c.poll_on = false;
        assert!(!c.ready() && c.why_not().is_some_and(|w| w.starts_with("Give the article a title")));
        c.title = "Notes".into();
        assert!(c.ready() && c.why_not().is_none());
        c.body = text_editor::Content::with_text(&"x".repeat(c.limit() + 3));
        assert_eq!(c.why_not(), Some(format!("3 characters over the limit of {}", c.limit())));
    }

    #[test]
    fn j_and_k_choose_the_posts_on_show_in_their_order_and_l_likes_the_chosen_one() {
        let post = |id: &str| Post { id: id.into(), ..Post::default() };
        let mut s = State {
            screen: Some(Screen::Home),
            home_feed: HomeFeed::Following,
            home: Some(Posts { posts: vec![post("p1"), post("p2"), post("p3")], more: false }),
            ..State::default()
        };
        let _ = s.update(Msg::Key(keys::PageKey::Previous));
        assert_eq!(s.selected.as_deref(), Some("p1"), "the first key chooses the first post");
        let _ = s.update(Msg::Key(keys::PageKey::Next));
        let _ = s.update(Msg::Key(keys::PageKey::Next));
        let _ = s.update(Msg::Key(keys::PageKey::Next));
        assert_eq!(s.selected.as_deref(), Some("p3"), "and stays on the last");
        let _ = s.update(Msg::Key(keys::PageKey::Previous));
        assert_eq!(s.selected.as_deref(), Some("p2"));
        let _ = s.update(Msg::Go(Screen::Explore));
        assert_eq!(s.selected, None, "another screen starts with none chosen");
    }

    #[test]
    fn a_plain_repost_is_chosen_as_the_post_it_reposts_and_a_thread_lists_its_parent_post_and_replies() {
        let post = |id: &str| Post { id: id.into(), ..Post::default() };
        let repost = Post { id: "r1".into(), repost_of_id: Some("p9".into()), repost_of: Some(Box::new(post("p9"))), ..Post::default() };
        let s = State { screen: Some(Screen::Home), home_feed: HomeFeed::Following, home: Some(Posts { posts: vec![repost], more: false }), ..State::default() };
        assert_eq!(s.shown_posts().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["p9"]);
        let thread = Thread { parent: Some(Box::new(post("up"))), post: post("it"), replies: Posts { posts: vec![post("a"), post("b")], more: false } };
        let s = State { screen: Some(Screen::Thread("it".into())), thread: Some(thread), ..State::default() };
        assert_eq!(s.shown_posts().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["up", "it", "a", "b"]);
    }

    #[test]
    fn a_photo_opens_over_the_page_steps_round_its_posts_photos_and_closes_on_another_screen() {
        let photo = |n: u32| pages::Photo { url: format!("https://media.alelyon.com/p{n}.webp"), ..pages::Photo::default() };
        let post = Post { id: "pst_1".into(), media: vec![photo(1), photo(2), photo(3)], ..Post::default() };
        let mut s = State {
            screen: Some(Screen::Home),
            home_feed: HomeFeed::Following,
            home: Some(Posts { posts: vec![post], more: false }),
            ..State::default()
        };
        let _ = s.update(Msg::ViewPhoto(Some(("pst_1".into(), 0))));
        let _ = s.update(Msg::Key(keys::PageKey::PhotoPrevious));
        assert_eq!(s.viewing, Some(("pst_1".into(), 2)), "round the start to the last");
        let _ = s.update(Msg::Key(keys::PageKey::PhotoNext));
        assert_eq!(s.viewing, Some(("pst_1".into(), 0)), "and round the end to the first");
        let _ = s.update(Msg::Go(Screen::Explore));
        assert_eq!(s.viewing, None, "another screen closes it");
        let _ = s.update(Msg::Key(keys::PageKey::PhotoNext));
        assert_eq!(s.viewing, None, "the arrows open nothing");
    }

    #[test]
    fn a_mention_is_read_where_the_service_reads_one() {
        let at = |text: &str| {
            let mut body = text_editor::Content::with_text(text);
            body.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
            typed_mention(&body).map(|(_, start, end, word)| (start, end, word))
        };
        assert_eq!(at("Thanks @ad"), Some((7, 10, "ad".to_string())));
        assert_eq!(at("@ada.l"), Some((0, 6, "ada.l".to_string())), "at the start, with a dot");
        assert_eq!(at("mail ada@ex"), None, "inside an address");
        assert_eq!(at("Thanks @"), None, "nothing typed after it yet");
        assert_eq!(at("Thanks @ada "), None, "a space ends it");
        assert_eq!(at("Zoë: @bo"), Some((6, 9, "bo".to_string())), "byte columns after a two-byte letter");
    }

    #[test]
    fn a_picked_person_replaces_the_mention_as_typed_and_the_list_steps_round() {
        let who = |u: &str| Person { username: Some(u.into()), has_page: true, ..Person::default() };
        let mut s = State::default();
        s.composer.body = text_editor::Content::with_text("Thanks @ad for the review");
        s.composer.body.perform(text_editor::Action::Move(text_editor::Motion::DocumentStart));
        for _ in 0.."Thanks @ad".len() {
            s.composer.body.perform(text_editor::Action::Move(text_editor::Motion::Right));
        }
        let _ = s.track_mention();
        assert_eq!(s.mentioning.as_ref().map(|m| m.word.as_str()), Some("ad"));
        let found = Found { people: vec![who("ada"), who("adam"), Person { username: Some("nopage".into()), ..Person::default() }], ..Found::default() };
        let _ = s.update(Msg::MentionFound("ad".into(), Ok(found)));
        assert_eq!(s.mentioning.as_ref().map(|m| m.found.len()), Some(2), "only people with a page");
        let _ = s.update(Msg::MentionMove(-1));
        assert_eq!(s.mentioning.as_ref().map(|m| m.at), Some(1), "round the start to the last");
        let _ = s.update(Msg::MentionMove(1));
        let _ = s.update(Msg::MentionPick(None));
        assert_eq!(s.composer.body.text().trim_end(), "Thanks @ada for the review", "no second space before one already there");
        assert!(s.mentioning.is_none());
        let _ = s.update(Msg::MentionFound("ad".into(), Ok(Found::default())));
        assert!(s.mentioning.is_none(), "a late answer opens nothing");
    }

    #[test]
    fn counts_are_written_as_x_writes_them() {
        for (n, words) in [(0, "0"), (7, "7"), (999, "999"), (1_284, "1,284"), (9_999, "9,999"), (10_000, "10K"), (12_549, "12.5K"), (999_999, "999.9K"), (1_000_000, "1M"), (1_250_000, "1.2M")] {
            assert_eq!(view::compact(n), words, "{n}");
        }
    }

    #[test]
    fn a_page_opens_at_its_first_tab_and_a_tab_reads_only_what_it_changes() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Person("ada".into())));
        let _ = s.update(Msg::PersonTab(PersonTab::Replies));
        assert_eq!((s.person_tab, s.person_tab.show()), (PersonTab::Replies, PersonShow::Replies));
        let _ = s.update(Msg::PersonTab(PersonTab::Profile));
        assert_eq!(s.person_tab.show(), PersonShow::Posts, "the profile comes with the posts");
        let _ = s.update(Msg::Go(Screen::Person("ada".into())));
        assert_eq!(s.person_tab, PersonTab::Profile, "the page on show keeps its tab");
        let _ = s.update(Msg::Go(Screen::Person("bob".into())));
        assert_eq!(s.person_tab, PersonTab::Posts, "another person's page opens at Posts");
        let _ = s.update(Msg::Go(Screen::Org("LACE".into())));
        let _ = s.update(Msg::OrgTab(OrgTab::Jobs));
        let _ = s.update(Msg::OrgJobs("WEFT".into(), Ok(Posts::default())));
        assert!(s.org_jobs.is_none(), "another organization's jobs are dropped");
        let _ = s.update(Msg::OrgJobs("LACE".into(), Ok(Posts::default())));
        assert_eq!(s.org_jobs.as_ref().map(|(t, _)| t.as_str()), Some("LACE"));
        let _ = s.update(Msg::PageMenu(true));
        let _ = s.update(Msg::Reporting(Some(Target::Org("LACE".into()))));
        assert!(!s.page_menu, "acting from the menu closes it");
        let _ = s.update(Msg::Go(Screen::Org("WEFT".into())));
        assert_eq!((s.org_tab, s.org_jobs.as_ref()), (OrgTab::Posts, None), "another organization opens at Posts");
    }

    #[test]
    fn reactions_reposts_and_follows_are_grouped_where_the_newest_stands() {
        let items = vec![
            told("n1", "like", "ada", Some("pst_1")),
            told("n2", "reply", "bob", Some("pst_1")),
            told("n3", "like", "carol", Some("pst_1")),
            told("n4", "follow", "dan", None),
            told("n5", "like", "erin", Some("pst_2")),
            told("n6", "follow", "fay", None),
            told("n7", "repost", "gus", Some("pst_1")),
        ];
        let ids: Vec<Vec<&str>> = grouped(&items).iter().map(|g| g.iter().map(|n| n.id.as_str()).collect()).collect();
        assert_eq!(ids, [vec!["n1", "n3"], vec!["n2"], vec!["n4", "n6"], vec!["n5"], vec!["n7"]]);
        assert!(NotifView::Mentions.shows("reply") && NotifView::Mentions.shows("mention") && !NotifView::Mentions.shows("like"));
        assert!(NotifView::All.shows("follow"));
    }

    #[test]
    fn scrolling_near_the_end_asks_for_the_next_page_once() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Explore));
        s.loading = false;
        s.explore = Some(Posts { posts: vec![Post { id: "pst_9".into(), ..Post::default() }], more: true });
        let _ = s.update(Msg::Scrolled(0.5));
        assert_eq!(s.more_asked, None, "not near the end");
        let _ = s.update(Msg::Scrolled(0.95));
        assert_eq!(s.more_asked.as_deref(), Some("pst_9"));
        s.more_asked = Some("pst_9".into());
        let _ = s.update(Msg::Scrolled(0.99));
        assert_eq!(s.more_asked.as_deref(), Some("pst_9"), "the same page is not asked twice");
        s.explore = Some(Posts { posts: vec![Post { id: "pst_9".into(), ..Post::default() }], more: false });
        s.more_asked = None;
        let _ = s.update(Msg::Scrolled(1.0));
        assert_eq!(s.more_asked, None, "nothing more to ask for");
        let _ = s.update(Msg::Scrolled(f32::NAN));
        assert_eq!(s.more_asked, None, "a page shorter than the window scrolls nowhere");
    }

    #[test]
    fn a_post_shows_one_menu_at_a_time_and_acting_closes_it() {
        let mut s = State::default();
        let _ = s.update(Msg::PostMenu(Some("pst_1".into())));
        assert_eq!(s.post_menu.as_deref(), Some("pst_1"));
        let _ = s.update(Msg::RepostMenu(Some("pst_2".into())));
        assert_eq!((s.post_menu.as_deref(), s.repost_menu.as_deref()), (None, Some("pst_2")), "one menu at a time");
        let _ = s.update(Msg::Reacting(Some("pst_2".into())));
        assert_eq!((s.repost_menu.as_deref(), s.reacting.as_deref()), (None, Some("pst_2")));
        let _ = s.update(Msg::React("pst_2".into(), Reaction::Celebrate));
        assert_eq!(s.reacting, None, "reacting closes the choices");
        let _ = s.update(Msg::PostMenu(Some("pst_3".into())));
        let _ = s.update(Msg::CopyLink("pst_3".into()));
        assert_eq!((s.post_menu.as_ref(), s.note.as_deref()), (None, Some("Link copied.")));
        assert_eq!(post_link("pst_3"), "https://www.alelyon.com/pages/?post=pst_3");
        let _ = s.update(Msg::PostMenu(Some("pst_4".into())));
        let _ = s.update(Msg::Go(Screen::Explore));
        assert_eq!(s.post_menu, None, "going elsewhere closes it");
    }

    #[test]
    fn a_lists_screen_keeps_only_its_own_answers_and_who_is_on_it_goes_with_it() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::List("lst_a".into())));
        let page = |id: &str, post: &str| ListPage {
            list: a_list(id, "Engines"),
            posts: Posts { posts: vec![Post { id: post.into(), ..Post::default() }], more: true },
        };
        let _ = s.update(Msg::ListPage("lst_b".into(), false, Ok(page("lst_b", "p1"))));
        assert!(s.list.is_none(), "an answer for another list is dropped");
        let _ = s.update(Msg::ListPage("lst_a".into(), false, Ok(page("lst_a", "p1"))));
        let _ = s.update(Msg::ListPage("lst_a".into(), true, Ok(page("lst_a", "p2"))));
        assert_eq!(s.list.as_ref().map(|l| l.posts.posts.len()), Some(2), "a further page goes under the first");
        assert_eq!(s.next_page().as_deref(), Some("p2"));
        let on_it = ListMembers { people: vec![person("ada")], orgs: vec![org("LACE")] };
        let _ = s.update(Msg::ListMembers("lst_b".into(), Ok(on_it.clone())));
        assert!(s.list_members.is_none(), "who is on another list is dropped");
        let _ = s.update(Msg::ListMembers("lst_a".into(), Ok(on_it.clone())));
        assert!(s.list_members.as_ref().is_some_and(|(id, m)| id == "lst_a" && m.people.len() == 1 && m.orgs.len() == 1));
        let _ = s.update(Msg::ShowListMembers(false));
        assert!(s.list_members.is_none(), "hiding them forgets them");
        let _ = s.update(Msg::ListMembers("lst_a".into(), Ok(on_it)));
        let _ = s.update(Msg::Go(Screen::Lists));
        assert!(s.list_members.is_none(), "leaving the list forgets them");
    }

    #[test]
    fn a_new_list_is_cut_to_what_the_service_keeps_and_deleting_one_asks_first() {
        let mut s = State::default();
        let _ = s.update(Msg::NewListName("x".repeat(40)));
        let _ = s.update(Msg::NewListAbout("y".repeat(150)));
        assert_eq!((s.new_list.0.chars().count(), s.new_list.1.chars().count()), (pages::LIST_NAME_MAX_CHARS, pages::LIST_ABOUT_MAX_CHARS));
        let _ = s.update(Msg::ListCreated(Ok(a_list("lst_n", "Engines"))));
        assert_eq!(s.new_list, (String::new(), String::new()), "the form is emptied");
        assert_eq!(s.screen, Some(Screen::List("lst_n".into())), "the new list opens");
        let _ = s.update(Msg::DeleteList("lst_n".into()));
        assert_eq!(s.confirm_delete_list.as_deref(), Some("lst_n"), "the first press asks");
        let _ = s.update(Msg::DeleteList("lst_o".into()));
        assert_eq!(s.confirm_delete_list.as_deref(), Some("lst_o"), "a press for another list asks about that one");
        let _ = s.update(Msg::DeleteList("lst_o".into()));
        assert_eq!(s.confirm_delete_list, None, "the second press deletes");
    }

    #[test]
    fn a_lists_edit_starts_from_what_is_on_show_and_needs_a_name_to_save() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::List("lst_a".into())));
        let _ = s.update(Msg::EditList(true));
        assert!(s.edit_list.is_none(), "nothing to edit before the list is read");
        let shown = ListPage { list: PageList { about: "Who builds them".into(), ..a_list("lst_a", "Engines") }, ..ListPage::default() };
        let _ = s.update(Msg::ListPage("lst_a".into(), false, Ok(shown)));
        let _ = s.update(Msg::EditList(true));
        assert_eq!(s.edit_list, Some(("lst_a".into(), "Engines".into(), "Who builds them".into())));
        let _ = s.update(Msg::EditListName("z".repeat(30)));
        let _ = s.update(Msg::EditListAbout("w".repeat(120)));
        let form = s.edit_list.clone().unwrap_or_default();
        assert_eq!((form.1.chars().count(), form.2.chars().count()), (pages::LIST_NAME_MAX_CHARS, pages::LIST_ABOUT_MAX_CHARS));
        let _ = s.update(Msg::EditListName("   ".into()));
        let _ = s.update(Msg::SaveList);
        assert!(s.edit_list.is_some(), "a blank name is not saved, and the form stays open");
        let _ = s.update(Msg::EditListName("Looms".into()));
        let _ = s.update(Msg::SaveList);
        assert!(s.edit_list.is_none(), "saving closes the form");
        let _ = s.update(Msg::EditList(true));
        let _ = s.update(Msg::Go(Screen::Lists));
        assert!(s.edit_list.is_none(), "leaving the list closes it");
    }

    #[test]
    fn the_lists_offered_for_a_page_are_kept_only_while_that_page_is_on_show() {
        let mut s = State::default();
        let _ = s.update(Msg::Go(Screen::Person("ada".into())));
        let ada = ListTarget::Person("ada".into());
        let offered = vec![PageList { has: Some(true), ..a_list("lst_a", "Engines") }];
        let _ = s.update(Msg::ListsFor(ada.clone(), Ok(offered.clone())));
        assert!(s.list_picker.is_none(), "nothing is offered before the page is read");
        s.person = Some(PersonPage { person: person("ada"), ..PersonPage::default() });
        let _ = s.update(Msg::ListsFor(ada.clone(), Ok(offered.clone())));
        assert_eq!(s.list_picker.as_ref().map(|(t, l)| (t, l.len())), Some((&ada, 1)));
        let _ = s.update(Msg::ListsFor(ListTarget::Person("bob".into()), Ok(Vec::new())));
        assert_eq!(s.list_picker.as_ref().map(|(t, _)| t), Some(&ada), "an offer for another page is dropped");
        let _ = s.update(Msg::ListsFor(ListTarget::Org("LACE".into()), Ok(Vec::new())));
        assert_eq!(s.list_picker.as_ref().map(|(t, _)| t), Some(&ada), "an organization's offer is not a person's");
        let _ = s.update(Msg::PickLists(None));
        assert!(s.list_picker.is_none(), "Close lists closes it");
        let _ = s.update(Msg::ListsFor(ada, Ok(offered)));
        let _ = s.update(Msg::Go(Screen::Lists));
        assert!(s.list_picker.is_none(), "leaving the page closes it");
    }
}
