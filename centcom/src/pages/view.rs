//! What the Pages page draws, as the social sites draw theirs (the design pass of 2026-10-15, `look`): the
//! navigation down the left, one column of posts between hairlines under its header, and beside it search with what
//! is trending and who to follow; a post as a row of that column; a person's and an organization's page; a thread;
//! notifications; the reader's own page and organizations; communities, lists and drafts; products, their launches,
//! feedback boards and reviews.

use alelyon_identity_client::pages::{
    self, Community, CommunitySummary, Draft, Education, Employment, Experience, FeedbackCategory, FeedbackSort, FeedbackStatus, Job,
    Joining, Kind, ListMembers, ListTarget, MediaMatch, Me, Member, Membership, ModAct, ModAction, ModReport, ModTarget, Notification, Org,
    PageList, Person, PersonPage, Photo, Poll, Post, ProductPage, ProductShow, ProductSummary, Profile, Reaction, Reason, ReplyRule,
    SearchKind, Stage, Target, Workplace,
};
use alelyon_identity_client::social;
use iced::widget::{
    Column, Row, button, checkbox, column, container, image, pick_list, rich_text, row, scrollable, space, span, stack, text, text_editor,
    text_input,
};
use iced::{Alignment, Color, ContentFit, Element, Length};

use super::icons::{self, Glyph};
use super::keys;
use super::kit::{self, Pill};
use super::look::{self, BODY, NAV, Palette, SMALL};
use super::photos::Picture;
use super::popover::{Side, popover, popover_at};
use super::Mentioning;
use super::{
    AboutProduct, AsWho, CommunityField, Composer, EduField, EmploymentPick, ExpField, Giving, HomeFeed, InWhere, JobDays, ModView, Msg,
    MineTab, NewCommunity, NotifView, OrgTab, PersonTab, PhotoFor, PollLength, ProductField, ProductForm, ProductsView, Screen, State, TitleOrg,
    WorkplacePick,
};
use crate::theme::fonts;
use crate::ui::{self, label, strong};

type El<'a> = Element<'a, Msg>;

/// The palette on show (`look`).
fn pal() -> &'static Palette {
    look::palette()
}

/// A small outlined button: a quieter act inside a row (Edit, Delete, Approve, Dismiss). Its words take the button's
/// colour, so one that cannot act looks it.
fn small<'a>(words: impl Into<String>, msg: Option<Msg>) -> El<'a> {
    button(text(words.into()).size(13.5).font(fonts().ui_strong)).padding([5.0, 12.0]).on_press_maybe(msg).style(look::outline(pal())).into()
}

/// A name that opens its page: semibold, bare.
fn link<'a>(words: impl Into<String>, size: f32, msg: Msg) -> El<'a> {
    kit::strong_link(words, size, pal().text, Some(msg))
}

fn when(at: &str) -> String {
    crate::utc::parse(at).map(|t| crate::utc::ago(t, crate::utc::now())).unwrap_or_default()
}

/// The page's room: the window less the app's rail (84) and its hairline.
fn room(s: &State) -> f32 {
    s.width.map_or(1280.0, |w| w - 85.0)
}

/// The columns' widths, as X lays out its own: the navigation with its words, or its icons alone when the window is
/// narrower; the feed's column; and, when there is room, search with what is trending and who to follow beside it.
const NAV_W: f32 = 232.0;
const NAV_ICONS_W: f32 = 84.0;
const COLUMN_W: f32 = 600.0;
const RAIL_W: f32 = 304.0;
const GAP: f32 = 24.0;

/// Which columns fit in `room`: the navigation's words, and the rail beside the column.
pub(super) fn columns(room: f32) -> (bool, bool) {
    (room >= NAV_W + COLUMN_W + 2.0 + GAP + RAIL_W, room >= NAV_ICONS_W + COLUMN_W + 2.0 + GAP + RAIL_W)
}

pub fn view(s: &State, phase: f32) -> El<'_> {
    let p = pal();
    if let Some(problem) = s.problem.as_ref().filter(|problem| pages::not_offered(problem)) {
        let notice = container(kit::notice(pages::words(problem), p.accent)).max_width(COLUMN_W);
        return container(notice).center_x(Length::Fill).padding(32).height(Length::Fill).style(look::page(p)).into();
    }
    let screen = s.screen.clone().unwrap_or(Screen::Home);
    let (words, wide) = columns(room(s));
    let feed = container(feed_column(s, &screen, phase))
        .width(if wide { Length::Fixed(COLUMN_W) } else { Length::Fill })
        .max_width(COLUMN_W)
        .height(Length::Fill);
    let mut page = Row::new().height(Length::Fill).push(nav(s, &screen, words)).push(kit::side_line()).push(feed).push(kit::side_line());
    let mut most = (if words { NAV_W } else { NAV_ICONS_W }) + COLUMN_W + 2.0;
    if wide {
        page = page.push(space().width(GAP)).push(rail(s, &screen));
        most += GAP + RAIL_W;
    }
    let page: El<'_> = container(container(page).max_width(most).height(Length::Fill)).center_x(Length::Fill).height(Length::Fill).style(look::page(p)).into();
    match photo_viewer(s) {
        Some(viewer) => stack![page, viewer].into(),
        None => page,
    }
}

/// A post's photo over the whole page, as the social sites open one: as large as the window allows, its description
/// under it, the others a press or an arrow key away. A press beside it, Esc or × closes it; a press on it does not.
fn photo_viewer(s: &State) -> Option<El<'_>> {
    let (id, at) = s.viewing.as_ref()?;
    let post = s.shown_posts().into_iter().find(|p| &p.id == id)?;
    let photo = post.media.get(*at)?;
    let p = pal();
    let words = look::ONYX;
    let picture: El<'_> = match s.photos.get(&photo.url) {
        Some(Picture::Ready(h)) => iced::widget::opaque(image(h.clone()).content_fit(ContentFit::Contain).width(Length::Fill).height(Length::Fill)),
        Some(Picture::Failed) => label("The photo could not be loaded.", BODY, words.text).into(),
        _ => label("Loading the photo\u{2026}", BODY, words.text).into(),
    };
    fn round<'a>(glyph: Glyph, msg: Msg, tip: &'static str) -> El<'a> {
        let icon = icons::icon(glyph, 22.0, look::ONYX.text, false);
        kit::tip(button(icon).padding(10).on_press(msg).style(look::over_photo(pal())), tip)
    }
    let n = post.media.len();
    let mut top = Row::new().spacing(12).align_y(Alignment::Center).push(space().width(Length::Fill));
    if n > 1 {
        top = top.push(label(format!("{} of {n}", at + 1), SMALL, words.dim));
    }
    top = top.push(round(Glyph::Close, Msg::ViewPhoto(None), "Close (Esc)"));
    let mut middle = Row::new().spacing(16).align_y(Alignment::Center).height(Length::Fill);
    if n > 1 {
        middle = middle.push(round(Glyph::Left, Msg::PhotoStep(-1), "The photo before (Left)"));
    }
    middle = middle.push(container(picture).center(Length::Fill));
    if n > 1 {
        middle = middle.push(round(Glyph::Right, Msg::PhotoStep(1), "The photo after (Right)"));
    }
    let mut shown = column![top, middle].spacing(12).padding(24).width(Length::Fill).height(Length::Fill);
    if !photo.alt.is_empty() {
        shown = shown.push(container(label(photo.alt.as_str(), BODY, words.text)).center_x(Length::Fill));
    }
    let backdrop = container(shown).width(Length::Fill).height(Length::Fill).style(look::scrim(p));
    Some(iced::widget::opaque(iced::widget::mouse_area(backdrop).on_press(Msg::ViewPhoto(None))))
}

/// Rows standing for posts while a feed's first read is on its way, as the social sites draw theirs.
fn skeleton<'a>() -> El<'a> {
    let p = pal();
    let bone = |most: f32| -> El<'a> { container(space()).width(Length::Fill).max_width(most).height(12).style(look::bone(p, 6.0)).into() };
    let mut col = Column::new();
    for most in [[160.0, 440.0, 300.0], [120.0, 400.0, 220.0], [190.0, 420.0, 260.0]] {
        let lines = column![bone(most[0]), bone(most[1]), bone(most[2])].spacing(12).width(Length::Fill);
        let face = container(space()).width(40).height(40).style(look::bone(p, 20.0));
        col = col.push(column![container(row![face, lines].spacing(12)).padding([16.0, 16.0]), kit::hairline()]);
    }
    col.into()
}

/// The feed's column: its header, then what the screen shows, scrolling under it.
fn feed_column<'a>(s: &'a State, screen: &Screen, phase: f32) -> El<'a> {
    let p = pal();
    let mut body = Column::new().width(Length::Fill);
    if let Some(problem) = &s.problem {
        let line = row![container(kit::notice(pages::words(problem), p.caution)).width(Length::Fill), small("Dismiss", Some(Msg::Dismiss))];
        body = body.push(container(line.spacing(10).align_y(Alignment::Center)).padding([10.0, 16.0]));
    } else if let Some(n) = &s.note {
        let line = row![label(n.as_str(), 14.0, p.dim).width(Length::Fill), small("Dismiss", Some(Msg::Dismiss))];
        body = body.push(container(line.spacing(10).align_y(Alignment::Center)).padding([10.0, 16.0]));
    }
    // a feed's first read stands as rows of grey where its posts will be (`skeleton`); a read again, or another
    // page's, shows the spinner
    let feed_screen = matches!(
        screen,
        Screen::Home
            | Screen::Explore
            | Screen::Bookmarks
            | Screen::Tag(_)
            | Screen::List(_)
            | Screen::Community(_)
            | Screen::Jobs
            | Screen::Flagged(_)
            | Screen::Products
    );
    if s.loading && !(feed_screen && s.shown_posts().is_empty()) {
        body = body.push(container(ui::working(phase, "Reading\u{2026}")).padding([14.0, 16.0]));
    }
    body = body.push(match screen {
        Screen::Home => home(s),
        Screen::Explore => explore(s),
        Screen::Mine => mine(s),
        Screen::Orgs => orgs(s),
        Screen::Person(_) => person(s),
        Screen::Org(_) => org(s),
        Screen::Thread(_) => thread(s),
        Screen::Members(_) => members(s),
        Screen::Flagged(_) => flagged(s),
        Screen::Notifications => notifications(s),
        Screen::Bookmarks => feed(s, s.bookmarks.as_ref(), "Nothing saved yet. Save a post and it shows here, for you only."),
        Screen::Moderation => moderation(s),
        Screen::Tag(_) => tagged(s),
        Screen::Search(_) => search(s),
        Screen::OrgExperience(_) => org_experience(s),
        Screen::Jobs => jobs(s),
        Screen::Applicants(_) => applicants(s),
        Screen::Communities => communities(s),
        Screen::Community(_) => community(s),
        Screen::CommunityMembers(_) => community_members(s),
        Screen::Lists => lists(s),
        Screen::List(_) => list(s),
        Screen::Drafts => drafts(s),
        Screen::Products => products(s),
        Screen::Product(_) => product(s),
    });
    let scroller = scrollable(body.push(space().height(120)))
        .id(keys::FEED)
        .on_scroll(|v| Msg::Scrolled(v.relative_offset().y))
        .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(4).scroller_width(4).margin(2)))
        .style(look::scrollers(p))
        .height(Length::Fill);
    column![head(s, screen), scroller].height(Length::Fill).into()
}

/// The column's header, as Pages sets one: the screen's title in the display face over a short accent rule, and
/// beside it what changes the view (Home's two feeds, Notifications' two views) as a `switch`; a search's kinds under
/// it; Explore's search in its place; a way back from what was reached from a tab.
fn head<'a>(s: &'a State, screen: &Screen) -> El<'a> {
    let back = (!screen.is_tab()).then_some(Msg::Back);
    let top: El<'a> = match screen {
        Screen::Home => {
            let feeds = kit::switch(&HomeFeed::ALL, s.home_feed, |f| f.title().to_string(), Msg::HomeFeed);
            kit::header(None, "Home", None, Some(feeds))
        }
        Screen::Notifications => {
            let views = kit::switch(&NotifView::ALL, s.notif_view, |v| v.title().to_string(), Msg::NotifView);
            kit::header(None, "Notifications", None, Some(views))
        }
        Screen::Explore => container(row![finder_box(s), refresh_button(s)].spacing(8).align_y(Alignment::Center)).padding([10.0, 16.0]).into(),
        Screen::Search(q) => {
            let kinds = kit::switch(&SearchKind::ALL, s.search_kind, |k| k.label().to_string(), Msg::SearchKind);
            column![kit::header(back, "Search", Some(q.clone()), None), container(kinds).padding(iced::padding::left(6).bottom(4))].into()
        }
        Screen::Person(u) => {
            let name = s.person.as_ref().filter(|page| page.person.username.as_deref() == Some(u.as_str())).map(|page| page.person.name().to_string());
            kit::header(back, name.unwrap_or_else(|| format!("@{u}")), Some(format!("@{u}")), None)
        }
        Screen::Org(t) => {
            let name = s.org.as_ref().filter(|page| page.org.ticker == *t).map(|page| page.org.name.clone());
            kit::header(back, name.unwrap_or_else(|| t.clone()), Some(t.clone()), None)
        }
        Screen::Products => {
            let views = kit::switch(&ProductsView::ALL, s.products_view, |v| v.title().to_string(), Msg::ProductsView);
            kit::header(None, "Products", None, Some(views))
        }
        Screen::Product(h) => {
            let name = s.product.as_ref().filter(|page| page.summary.product.handle == *h).map(|page| page.summary.product.name.clone());
            kit::header(back, name.unwrap_or_else(|| format!("p/{h}")), Some(format!("p/{h}")), None)
        }
        _ => kit::header(back, screen.title(), None, None),
    };
    column![top, kit::hairline()].into()
}

// ------------------------------------------------------------------- the navigation and the right rail

/// The tabs the reader has: everyone's, and Moderation for Alelyon's moderators.
fn tabs_for(s: &State) -> Vec<Screen> {
    let mut tabs = Screen::TABS.to_vec();
    if moderator(s) {
        tabs.push(Screen::Moderation);
    }
    tabs
}

fn glyph_for(tab: &Screen) -> Glyph {
    match tab {
        Screen::Home => Glyph::Home,
        Screen::Explore => Glyph::Explore,
        Screen::Products => Glyph::Rocket,
        Screen::Communities => Glyph::Communities,
        Screen::Lists => Glyph::Lists,
        Screen::Jobs => Glyph::Jobs,
        Screen::Notifications => Glyph::Bell,
        Screen::Bookmarks => Glyph::Bookmark,
        Screen::Mine => Glyph::Person,
        Screen::Orgs => Glyph::Building,
        _ => Glyph::Shield,
    }
}

/// The navigation, in Pages' manner: the mark and the wordmark; each place with its icon, the one on show washed in
/// gold behind a fine gold edge with its icon and words in gold (as CENTCOM's own rail marks its page); Notifications
/// with the count of the unread; New post, outlined in gold; and the reader at its foot. Narrow, the icons alone, each
/// named under the pointer.
fn nav<'a>(s: &'a State, screen: &Screen, words: bool) -> El<'a> {
    let p = pal();
    let mark = container(strong("P", 18.0, p.accent)).center(Length::Fixed(34.0)).style(look::mark(p));
    let brand: El<'a> = if words {
        row![mark, text("Pages").size(23.0).font(fonts().display).color(p.text)].spacing(12).align_y(Alignment::Center).into()
    } else {
        mark.into()
    };
    let mut col = Column::new()
        .spacing(2)
        .width(if words { NAV_W } else { NAV_ICONS_W })
        .height(Length::Fill)
        .padding([10.0, 12.0])
        .align_x(if words { Alignment::Start } else { Alignment::Center })
        .push(container(brand).padding([8.0, 12.0]))
        .push(space().height(6));
    let unread = s.me.as_ref().map(|m| m.unread_notifications).unwrap_or(0);
    for tab in tabs_for(s) {
        let on = tab == *screen;
        let mut icon = icons::icon(glyph_for(&tab), 22.0, if on { p.accent } else { p.dim }, on);
        if tab == Screen::Notifications && unread > 0 {
            icon = with_badge(icon, unread);
        }
        let title = tab.title();
        let inner: El<'a> = if words {
            let name: El<'a> = if on { strong(title.clone(), NAV, p.accent).into() } else { label(title.clone(), NAV, p.text).into() };
            row![icon, name].spacing(14).align_y(Alignment::Center).into()
        } else {
            icon
        };
        let item = button(inner).padding([9.0, 12.0]).width(if words { Length::Fill } else { Length::Shrink }).on_press(Msg::Go(tab)).style(look::nav_item(p, on));
        col = col.push(if words { El::from(item) } else { kit::tip(item, title) });
    }
    col = col.push(space().height(14));
    col = col.push(if words {
        let words = row![icons::icon(Glyph::Pen, 18.0, p.accent, false), strong("New post", 15.5, p.accent)].spacing(10).align_y(Alignment::Center);
        El::from(button(container(words).center_x(Length::Fill)).padding([12.0, 0.0]).width(Length::Fill).on_press(Msg::Compose).style(look::outline_in(p, p.accent)))
    } else {
        kit::tip(button(icons::icon(Glyph::Pen, 22.0, p.accent, false)).padding(14).on_press(Msg::Compose).style(look::outline_in(p, p.accent)), "New post")
    });
    col = col.push(space().height(Length::Fill));
    if let Some(me) = &s.me {
        let face = avatar(s, me.me.avatar.as_ref(), me.me.name(), 40.0);
        let who: El<'a> = if words {
            let names = column![strong(me.me.name(), BODY, p.text), label(me.me.handle(), BODY, p.dim)].spacing(1).width(Length::Fill);
            row![face, names, icons::icon(Glyph::More, 18.0, p.text, false)].spacing(12).align_y(Alignment::Center).into()
        } else {
            face
        };
        let mine = button(who).padding(12).width(if words { Length::Fill } else { Length::Shrink }).on_press(Msg::Go(Screen::Mine)).style(look::nav(p));
        col = col.push(kit::tip(mine, "My page"));
    }
    col.into()
}

/// An icon with a count over its corner: the unread notifications.
fn with_badge<'a>(icon: El<'a>, n: u64) -> El<'a> {
    let p = pal();
    let words = if n > 99 { "99+".to_string() } else { n.to_string() };
    let badge = container(text(words).size(11.0).font(fonts().ui_strong).color(p.on_fill)).padding([1.0, 5.0]).style(look::badge(p));
    stack![container(icon).padding(iced::padding::top(4).right(8)), container(badge).width(Length::Fixed(34.0)).align_x(Alignment::End)].into()
}

/// Search, as X's pill with its magnifier inside it: Enter searches people, organizations and posts.
fn finder_box(s: &State) -> El<'_> {
    let p = pal();
    let field = text_input("Search", &s.find)
        .id(keys::SEARCH)
        .on_input(Msg::FindText)
        .on_submit(Msg::Find)
        .padding(iced::padding::top(11).bottom(11).left(46).right(16))
        .size(BODY)
        .font(fonts().ui)
        .style(look::search(p))
        .width(Length::Fill);
    stack![field, container(icons::icon(Glyph::Explore, 18.0, p.dim, false)).padding(iced::padding::top(12).left(18))].into()
}

/// Read everything on show again.
fn refresh_button(s: &State) -> El<'_> {
    let p = pal();
    kit::icon_button(Glyph::Refresh, 20.0, p.text, p.accent, (!s.busy()).then_some(Msg::Refresh), "Refresh")
}

/// Beside the column: search with what is trending, the day's launches and who to follow; on Explore, which carries
/// what is trending, the launches and who to follow; on Products, which ranks the launches, the rest.
fn rail<'a>(s: &'a State, screen: &Screen) -> El<'a> {
    let mut col = Column::new().spacing(16).width(RAIL_W).padding(iced::padding::top(8).bottom(24));
    if *screen != Screen::Explore {
        col = col.push(row![finder_box(s), refresh_button(s)].spacing(6).align_y(Alignment::Center));
        if let Some(card) = trending_card(s, Some(5)) {
            col = col.push(card);
        }
    }
    if *screen != Screen::Products
        && let Some(card) = launches_card(s)
    {
        col = col.push(card);
    }
    if let Some(card) = follow_card(s, Some(3)) {
        col = col.push(card);
    }
    scrollable(col).direction(scrollable::Direction::Vertical(scrollable::Scrollbar::hidden())).height(Length::Fill).into()
}

/// What is trending, as X's card: each hashtag with its place and how many posted it; Show more opens Explore.
fn trending_card(s: &State, most: Option<usize>) -> Option<El<'_>> {
    let t = s.trending.as_ref()?;
    let p = pal();
    let mut rows: Vec<El<'_>> = Vec::new();
    if t.tags.is_empty() {
        rows.push(container(kit::note("Nothing is trending yet. A hashtag trends once two or more people use it.")).padding([4.0, 16.0]).into());
    }
    for (i, tag) in t.tags.iter().take(most.unwrap_or(usize::MAX)).enumerate() {
        let posts = if tag.posts == 1 { "post" } else { "posts" };
        let people = if tag.people == 1 { "person" } else { "people" };
        let words = column![
            strong(format!("#{}", tag.tag), BODY, p.text),
            kit::figure(format!("{} {posts} \u{00B7} {} {people}", compact(tag.posts), compact(tag.people)), 12.0, p.dim),
        ]
        .spacing(3);
        let line = row![kit::figure(format!("{:02}", i + 1), 13.0, p.accent), words].spacing(14).align_y(Alignment::Start);
        rows.push(button(line).padding([10.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Tag(tag.tag.clone()))).style(look::card_row(p)).into());
    }
    let more = most.is_some_and(|m| t.tags.len() > m).then_some(Msg::Go(Screen::Explore));
    Some(kit::card(if t.hours > 24 { "Trending this week" } else { "Trending today" }, rows, more))
}

/// Who to follow, as X's card: each picture, name and handle, why they are suggested, and Follow.
fn follow_card(s: &State, most: Option<usize>) -> Option<El<'_>> {
    let found = s.suggestions.as_ref().filter(|f| !f.people.is_empty() || !f.orgs.is_empty())?;
    let p = pal();
    let most = most.unwrap_or(usize::MAX);
    let mut rows: Vec<El<'_>> = Vec::new();
    for sug in found.people.iter().take(most) {
        let who = &sug.person;
        let mut handle = Row::new().spacing(6).align_y(Alignment::Center).push(label(who.handle(), 14.0, p.dim));
        for o in &who.tickers {
            handle = handle.push(kit::ticker(&o.ticker, Some(Msg::Go(Screen::Org(o.ticker.clone())))));
        }
        let names = column![strong(who.name(), BODY, p.text), handle, label(sug.why.words(sug.org.as_ref()), SMALL, p.dim)].spacing(2).width(Length::Fill);
        let mut line = row![avatar(s, who.avatar.as_ref(), who.name(), 40.0), names].spacing(12).align_y(Alignment::Center);
        if let Some(u) = who.username.clone() {
            line = line.push(follow_button(Msg::FollowPerson(u, true)));
        }
        let open = who.username.clone().map(|u| Msg::Go(Screen::Person(u)));
        rows.push(button(line).padding([12.0, 16.0]).width(Length::Fill).on_press_maybe(open).style(look::card_row(p)).into());
    }
    for o in found.orgs.iter().take(most) {
        let line = row![
            logo(s, o.logo.as_ref(), &o.name, 40.0),
            column![strong(o.name.as_str(), BODY, p.text), kit::ticker(&o.ticker, None)].spacing(4).width(Length::Fill),
            follow_button(Msg::FollowOrg(o.ticker.clone(), true)),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        rows.push(button(line).padding([12.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Org(o.ticker.clone()))).style(look::card_row(p)).into());
    }
    Some(kit::card("Builders to follow", rows, None))
}

/// Follow beside a suggestion: outlined in gold, a quieter act than the column's one gold button.
fn follow_button<'a>(msg: Msg) -> El<'a> {
    let p = pal();
    button(text("Follow").size(14.0).font(fonts().ui_strong)).padding([6.0, 14.0]).on_press(msg).style(look::outline_in(p, p.accent)).into()
}

// ------------------------------------------------------------------- a person's name, as every card draws it

/// `[ Founder | CEO ]` with its mark when its organization confirmed it, the name (a link to the page), each shown
/// ticker (a link to its organization) and the handle: the service's name line, in the column's sizes.
fn name_line<'a>(who: &'a Person) -> El<'a> {
    let p = pal();
    let mut line = Row::new().spacing(5).align_y(Alignment::Center);
    if !who.title.is_empty() {
        line = line.push(kit::figure(format!("[ {} ]", who.title), 13.0, p.dim));
        if who.title_verified {
            line = line.push(kit::tip(label("\u{2713}", BODY, p.accent), "Title confirmed by the organization it names"));
        }
    }
    line = line.push(match &who.username {
        Some(u) if who.has_page => kit::strong_link(who.name(), BODY, p.text, Some(Msg::Go(Screen::Person(u.clone())))),
        _ => strong(who.name(), BODY, p.text).into(),
    });
    for o in &who.tickers {
        line = line.push(kit::ticker(&o.ticker, Some(Msg::Go(Screen::Org(o.ticker.clone())))));
    }
    line = line.push(label(who.handle(), 14.0, p.dim));
    line.wrap().into()
}

/// A person as a row of the column: their picture and name line; pressed, their page.
fn person_row<'a>(s: &'a State, who: &'a Person) -> El<'a> {
    let line = row![avatar(s, who.avatar.as_ref(), who.name(), 40.0), name_line(who)].spacing(12).align_y(Alignment::Center);
    let open = who.username.clone().filter(|_| who.has_page).map(|u| Msg::Go(Screen::Person(u)));
    column![button(line).padding([12.0, 16.0]).width(Length::Fill).on_press_maybe(open).style(look::row(pal())), kit::hairline()].into()
}

/// An organization as a row of the column: its logo, name and ticker; pressed, its page.
fn org_row<'a>(s: &'a State, o: &'a Org) -> El<'a> {
    let p = pal();
    let line = row![logo(s, o.logo.as_ref(), &o.name, 40.0), strong(o.name.as_str(), BODY, p.text), kit::ticker(&o.ticker, None)]
        .spacing(10)
        .align_y(Alignment::Center);
    column![button(line).padding([12.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Org(o.ticker.clone()))).style(look::row(p)), kit::hairline()].into()
}

// ------------------------------------------------------------------- photos

/// A picture `px` across, rounded by `radius`: the photo once it is drawn, else the name's first letter on a disc of
/// a colour of its own (`look::initial_color`).
fn picture<'a>(s: &'a State, url: Option<&str>, name: &str, px: f32, radius: f32) -> El<'a> {
    if let Some(Picture::Ready(h)) = url.and_then(|u| s.photos.get(u)) {
        return image(h.clone()).width(px).height(px).content_fit(ContentFit::Cover).border_radius(radius).into();
    }
    let p = pal();
    let color = look::initial_color(p, name);
    let initial: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
    container(strong(initial, (px * 0.42).round(), color)).center(Length::Fixed(px)).style(look::disc(p, color, radius)).into()
}

/// A post's photos: one at its own shape, two side by side, three or four two to a row; each opens over the page.
fn photo_grid<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    if let [one] = post.media.as_slice() {
        return photo_cell(s, post, 0, one, None);
    }
    let height = if post.media.len() == 2 { 260.0 } else { 180.0 };
    let mut grid = Column::new().spacing(4);
    for (row_at, pair) in post.media.chunks(2).enumerate() {
        let cells = pair.iter().enumerate().map(|(i, photo)| photo_cell(s, post, row_at * 2 + i, photo, Some(height)));
        grid = grid.push(Row::with_children(cells).spacing(4));
    }
    grid.into()
}

/// One photo: at its own shape (at most 510 high) when `height` is None, else cropped to fill `height`; rounded, with
/// a hairline round it as X frames a post's photos. A photo not drawn shows its description in a frame.
fn photo_cell<'a>(s: &'a State, post: &'a Post, at: usize, photo: &'a Photo, height: Option<f32>) -> El<'a> {
    let p = pal();
    let state = s.photos.get(&photo.url);
    if let Some(Picture::Ready(h)) = state {
        let shown = image(h.clone()).width(Length::Fill).border_radius(16.0);
        let shown: El<'a> = match height {
            // cropped to its cell: the cell clips it, as not every renderer clips an image to its own bounds
            Some(px) => container(shown.height(px).content_fit(ContentFit::Cover)).width(Length::Fill).height(px).clip(true).into(),
            None => container(shown).max_height(510.0).into(),
        };
        let framed = stack![shown, container(space()).width(Length::Fill).height(Length::Fill).style(look::photo_edge(p, 16.0))];
        let opens = button(framed).padding(0).width(Length::Fill).on_press(Msg::ViewPhoto(Some((post.id.clone(), at)))).style(look::bare(p));
        return kit::tip(opens, if photo.alt.is_empty() { "Open the photo" } else { photo.alt.as_str() });
    }
    let why = match state {
        Some(Picture::Failed) => "The photo could not be loaded.",
        _ => "Loading the photo\u{2026}",
    };
    let alt = if photo.alt.is_empty() { "A photo without a description." } else { photo.alt.as_str() };
    container(column![label(alt, BODY, p.text), label(why, SMALL, p.dim)].spacing(4))
        .padding(14)
        .width(Length::Fill)
        .height(height.unwrap_or(120.0))
        .style(look::frame(p))
        .into()
}

/// A small square of a photo being attached, or a frame until it is drawn.
fn thumb<'a>(s: &'a State, url: &str, px: f32) -> El<'a> {
    match s.photos.get(url) {
        Some(Picture::Ready(h)) => image(h.clone()).width(px).height(px).content_fit(ContentFit::Cover).border_radius(12.0).into(),
        _ => container(label("Photo", 12.0, pal().dim)).center(px).style(look::frame(pal())).into(),
    }
}

/// A page's or an organization's banner across the column, cropped to fill; nothing until it is drawn.
fn banner<'a>(s: &'a State, url: Option<&String>) -> Option<El<'a>> {
    let Some(Picture::Ready(h)) = s.photos.get(url?) else { return None };
    Some(image(h.clone()).width(Length::Fill).height(BANNER).content_fit(ContentFit::Cover).into())
}

/// Change or remove a photo and a banner: each part, what it is called, and whether it is set.
fn photo_controls<'a>(s: &'a State, parts: [(PhotoFor, &'static str, bool); 2]) -> El<'a> {
    let free = s.uploading == 0;
    let mut r = Row::new().spacing(6).align_y(Alignment::Center);
    for (target, what, set) in parts {
        r = r.push(small(format!("Change {what}"), free.then(|| Msg::PickPhotos(target.clone()))));
        if set {
            r = r.push(small(format!("Remove {what}"), Some(Msg::ClearPhoto(target))));
        }
    }
    if !free {
        r = r.push(label("Uploading\u{2026}", 13.0, pal().dim));
    }
    r.wrap().into()
}

/// What a photo becomes when it is uploaded, said where photos are chosen.
const PHOTO_RULE: &str = "Photos are public: JPEG, PNG or WebP, at most 5 MB each. Before one is stored, its location and \
    other details are removed and it is checked against known illegal imagery.";

/// What a confirmed title means, said where the mark is explained.
fn verified_note<'a>(p: &'a Person) -> Option<El<'a>> {
    let org = p.title_org.as_ref()?;
    Some(if p.title_verified {
        kit::note(format!("\u{2713} {} ({}) confirmed this title.", org.name, org.ticker))
    } else {
        kit::note(format!("This title names {} ({}), which has not confirmed it.", org.name, org.ticker))
    })
}

// ------------------------------------------------------------------- posts

fn me_id(s: &State) -> &str {
    s.me.as_ref().map(|m| m.me.id.as_str()).unwrap_or("")
}

/// Who a post is by, in a few words: its author, else the organization it was posted as (whose author is gone).
fn by_name(p: &Post) -> &str {
    match (&p.author, &p.org) {
        (Some(a), _) => a.name(),
        (None, Some(o)) => o.name.as_str(),
        (None, None) => "someone",
    }
}

/// The reader is an active owner or admin of the organization.
fn admin_of(s: &State, ticker: &str) -> bool {
    s.me.as_ref().is_some_and(|m| {
        m.memberships.iter().any(|ms| ms.org.ticker == ticker && ms.state == "active" && matches!(ms.role.as_str(), "owner" | "admin"))
    })
}

/// An organization's name and ticker chip, which open its page.
fn org_button<'a>(o: &'a Org) -> El<'a> {
    let p = pal();
    let opens = Msg::Go(Screen::Org(o.ticker.clone()));
    row![kit::strong_link(o.name.as_str(), BODY, p.text, Some(opens.clone())), kit::ticker(&o.ticker, Some(opens))].spacing(6).align_y(Alignment::Center).into()
}

/// A person's picture, round: the photo once drawn, else their initial on a disc of its own colour.
fn avatar<'a>(s: &'a State, url: Option<&String>, name: &str, px: f32) -> El<'a> {
    picture(s, url.map(String::as_str), name, px, px / 2.0)
}

/// An organization's logo: a rounded square, as X draws an organization's.
fn logo<'a>(s: &'a State, url: Option<&String>, name: &str, px: f32) -> El<'a> {
    picture(s, url.map(String::as_str), name, px, (px * 0.22).round())
}

/// A post's time as the social sites write it: "now", "5m", "2h", "3d", then its date ("Oct 14", with the year
/// when it is not this year).
fn short_when(at: &str) -> String {
    let Some(t) = crate::utc::parse(at) else { return String::new() };
    let now = crate::utc::now();
    let gone = (now - t).max(0.0) as u64;
    match gone {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m", gone / 60),
        3_600..=86_399 => format!("{}h", gone / 3_600),
        86_400..=604_799 => format!("{}d", gone / 86_400),
        _ => {
            let (date, today) = (crate::utc::full(t), crate::utc::full(now));
            let month = date[5..7].parse::<usize>().ok().and_then(|m| MONTHS.get(m.wrapping_sub(1))).copied().unwrap_or("");
            let day = date[8..10].trim_start_matches('0');
            if date[..4] == today[..4] { format!("{month} {day}") } else { format!("{month} {day}, {}", &date[..4]) }
        }
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The time in full, for the pointer's tip: "2026-10-14 20:00 UTC".
fn full_time(at: &str) -> String {
    crate::utc::parse(at).map(|t| format!("{} UTC", &crate::utc::full(t)[..16])).unwrap_or_default()
}

/// A post as a row of the column (`post_row`).
fn post_card<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    post_row(s, post, None)
}

/// A post as a row of the column, as X draws one: the picture of whoever posted it beside its head (name, handle,
/// time, ⋯), its words, photos, quote, poll or job, and its actions; `above` is a line over it (who reposted it,
/// Pinned). The whole row opens the conversation, and a hairline closes it.
fn post_row<'a>(s: &'a State, post: &'a Post, above: Option<El<'a>>) -> El<'a> {
    let p = pal();
    if post.is_plain_repost() {
        let by = row![icons::icon(Glyph::Repost, 16.0, p.dim, false), strong(format!("{} reposted", by_name(post)), SMALL, p.dim)]
            .spacing(6)
            .align_y(Alignment::Center);
        return match post.repost_of.as_deref() {
            Some(inner) => post_row(s, inner, Some(by.into())),
            None => kit::block(column![by, kit::note("The post it reposted is not there any more.")].spacing(6)),
        };
    }
    let (url, who) = post_picture(post);
    let face = if post.org.is_some() { logo(s, url, who, 40.0) } else { avatar(s, url, who, 40.0) };
    let mut col = Column::new();
    if let Some(line) = above {
        col = col.push(container(line).padding(iced::padding::left(24.0).bottom(4.0)));
    }
    col = col.push(row![face, post_parts(s, post).width(Length::Fill)].spacing(12));
    let opens = button(col).padding([12.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Thread(post.id.clone())));
    // the post the keyboard is on (j, k): washed in the accent, with the accent's bar down its left edge
    let row_: El<'a> = if s.selected.as_deref() == Some(post.id.as_str()) {
        let bar = container(space()).width(3).height(Length::Fill).style(look::bar(p));
        stack![opens.style(look::chosen_row(p)), bar].into()
    } else {
        opens.style(look::row(p)).into()
    };
    container(column![row_, kit::hairline()]).id(keys::row_id(&post.id)).into()
}

/// Whose picture a post carries, and their name: the organization it was posted as, else its author.
fn post_picture(post: &Post) -> (Option<&String>, &str) {
    match (&post.org, &post.author) {
        (Some(o), _) => (o.logo.as_ref(), o.name.as_str()),
        (None, Some(a)) => (a.avatar.as_ref(), a.name()),
        (None, None) => (None, "someone"),
    }
}

/// Everything of a post beside its picture: its head, what it holds, its actions, and what opens under them.
fn post_parts<'a>(s: &'a State, post: &'a Post) -> Column<'a, Msg> {
    let p = pal();
    let mine = post.author.as_ref().is_some_and(|a| a.id == me_id(s));
    let admin = post.org.as_ref().is_some_and(|o| admin_of(s, &o.ticker));
    let mut col = Column::new().spacing(4).push(post_head(s, post));
    // Only the organization's owners and admins are told why a post has no author.
    if post.org.is_some()
        && let Some(flag) = post.author_flag
    {
        col = col.push(label(flag.words(), SMALL, p.caution));
    }
    if post.deleted {
        return col.push(kit::note("Deleted."));
    }
    col = col.push(post_body(s, post, BODY, mine, admin)).push(action_bar(s, post));
    open_under(s, post, col)
}

/// A post's head: who posted it (an organization's name and by whom, else the person's name line), when, whether it
/// was edited and in which community; then ⋯ at its end.
fn post_head<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    let p = pal();
    let mut head = Row::new().spacing(5).align_y(Alignment::Center);
    if let Some(o) = &post.org {
        head = head.push(org_button(o));
        if let Some(a) = &post.author {
            head = head.push(label(format!("by {}", a.name()), BODY, p.dim));
        }
    } else if let Some(a) = &post.author {
        head = head.push(name_line(a));
    }
    head = head.push(label("\u{00B7}", BODY, p.dim)).push(kit::tip(kit::figure(short_when(&post.created_at), 13.0, p.dim), full_time(&post.created_at)));
    if post.edited_at.is_some() {
        head = head.push(kit::figure("\u{00B7} edited", 13.0, p.dim));
    }
    // a reply by one of the makers of the product the conversation is about
    if post.maker {
        head = head.push(kit::chip("Maker", p.accent));
    }
    if let Some(c) = &post.community {
        head = head.push(label("in", BODY, p.dim)).push(community_link(c));
    }
    row![head.wrap(), space().width(Length::Fill), more_menu(s, post)].spacing(8).into()
}

/// A post's ⋯, its menu floating under it while it is open.
fn more_menu<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    let p = pal();
    let open = s.post_menu.as_deref() == Some(post.id.as_str());
    let more = kit::icon_button(Glyph::More, 18.0, if open { p.accent } else { p.dim }, p.accent, Some(Msg::PostMenu((!open).then(|| post.id.clone()))), "More");
    let mine = post.author.as_ref().is_some_and(|a| a.id == me_id(s));
    let admin = post.org.as_ref().is_some_and(|o| admin_of(s, &o.ticker));
    popover(more, open.then(|| post_menu(s, post, mine, admin)), Msg::PostMenu(None))
}

/// What a post holds, its words at `size`: its title, words, photos, quote, poll, who may reply, job, and how people
/// reacted when any did otherwise than with a plain like.
fn post_body<'a>(s: &'a State, post: &'a Post, size: f32, mine: bool, admin: bool) -> Column<'a, Msg> {
    let p = pal();
    let mut col = Column::new().spacing(10);
    match &post.product {
        Some(product) => col = col.push(product_line(s, post, product)),
        None => {
            if let Some(kind) = kind_words(post) {
                col = col.push(kit::kind(kind));
            }
        }
    }
    if let Some(title) = &post.title {
        col = col.push(text(title.as_str()).size(size + 4.0).font(fonts().display).color(p.text));
    }
    if !post.text.is_empty() {
        col = col.push(post_text(post.text.as_str(), size));
    }
    if !post.media.is_empty() {
        col = col.push(photo_grid(s, post));
    }
    if post.repost_of_id.is_some() {
        col = col.push(match post.repost_of.as_deref() {
            Some(inner) => quoted(s, inner),
            None => kit::note("The quoted post is not there any more."),
        });
    }
    if let Some(poll) = &post.poll {
        col = col.push(poll_view(post, poll, mine));
    }
    let own = post.product.as_ref().is_some_and(|product| keeps_product(s, &product.handle));
    if let Some(launch) = &post.launch {
        col = col.push(launch_box(post, launch, own));
    }
    if let Some(feedback) = &post.feedback {
        col = col.push(feedback_bar(post, feedback, mine, own));
    }
    if let Some(n) = post.stars.filter(|_| mine) {
        col = col.push(restar(post, n));
    }
    match post.replies_from {
        ReplyRule::Everyone => {}
        ReplyRule::Followers => col = col.push(kit::note("Replies: followers only")),
        ReplyRule::Mentioned => col = col.push(kit::note("Replies: the people mentioned only")),
    }
    if let Some(job) = &post.job {
        col = col.push(job_view(s, post, job, admin));
    }
    if post.reactions.iter().any(|(r, _)| *r != Reaction::Like) {
        let words: Vec<String> = post.reactions.iter().map(|(r, n)| format!("{} {r}", compact(*n))).collect();
        col = col.push(label(words.join(" \u{00B7} "), SMALL, p.dim));
    }
    col
}

/// What opens under a post's actions: the reasons to report it.
fn open_under<'a>(s: &'a State, post: &'a Post, mut col: Column<'a, Msg>) -> Column<'a, Msg> {
    if s.reporting == Some(Target::Post(post.id.clone())) {
        col = col.push(report_reasons(Target::Post(post.id.clone())));
    }
    col
}

/// What kind of post this is, said over its words in the brand's small capitals: a job opening, a product's launch,
/// feedback or review, an article, a poll, or a product's update. A plain post says nothing; Pages is for what people
/// build and hire for, so those are named.
fn kind_words(post: &Post) -> Option<&'static str> {
    if post.job.is_some() {
        Some("Job opening")
    } else if post.launch.is_some() {
        Some("Launch")
    } else if post.feedback.is_some() {
        Some("Feedback")
    } else if post.stars.is_some() {
        Some("Review")
    } else if post.kind == Kind::Article {
        Some("Article")
    } else if post.poll.is_some() {
        Some("Poll")
    } else if post.product.is_some() {
        Some("Update")
    } else {
        None
    }
}

/// The actions under a post, in Pages' manner: reply, repost or quote and like (with the other reactions beside it)
/// kept together at the left with their counts in the mono face, save and copy its link at the right; each lights in
/// its own colour under the pointer.
fn action_bar<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    let p = pal();
    let id = &post.id;
    let open = |which: &Option<String>| which.as_deref() == Some(id.as_str());
    let (reposting, reacting) = (open(&s.repost_menu), open(&s.reacting));
    let mine = post.author.as_ref().is_some_and(|a| a.id == me_id(s));
    let like = if post.liked { "Unlike" } else { "Like" };
    let save = if post.bookmarked { "Remove from Bookmarks" } else { "Save to Bookmarks" };
    row![
        kit::action(Glyph::Reply, post.replies, p.accent, false, Some(Msg::Go(Screen::Thread(id.clone()))), "Reply"),
        space().width(18),
        popover(
            kit::action(Glyph::Repost, post.reposts, p.repost, reposting, Some(Msg::RepostMenu((!reposting).then(|| id.clone()))), "Repost or quote"),
            reposting.then(|| repost_choices(post, mine)),
            Msg::RepostMenu(None),
        ),
        space().width(18),
        row![
            kit::action(Glyph::Heart, post.likes, p.like, post.liked, Some(Msg::Like(id.clone(), !post.liked)), like),
            popover_at(
                kit::icon_button(Glyph::Chevron, 15.0, if reacting { p.like } else { p.dim }, p.like, Some(Msg::Reacting((!reacting).then(|| id.clone()))), "React"),
                reacting.then(|| reaction_choices(post)),
                Msg::Reacting(None),
                Side::Centre,
            ),
        ]
        .align_y(Alignment::Center),
        space().width(Length::Fill),
        kit::action(Glyph::Bookmark, 0, p.accent, post.bookmarked, Some(Msg::Bookmark(id.clone(), !post.bookmarked)), save),
        kit::action(Glyph::Share, 0, p.accent, false, Some(Msg::CopyLink(id.clone())), "Copy link"),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// A tool of the composer: its icon in the accent (faded when it cannot act), washed while it is on, what it does
/// under the pointer.
fn tool<'a>(glyph: Glyph, tip: &'a str, on: bool, msg: Option<Msg>) -> El<'a> {
    let p = pal();
    let color = if msg.is_none() { look::tint(p, p.accent, 0.45) } else { p.accent };
    kit::tip(button(icons::icon(glyph, 18.0, color, false)).padding(7).on_press_maybe(msg).style(look::tool(p, on)), tip)
}

/// How much of a post's room is used, as X's ring: the accent, the caution colour in the last twenty characters and
/// the danger colour past them, with the characters left beside it from twenty down.
fn counter<'a>(count: usize, limit: usize) -> El<'a> {
    let p = pal();
    let left = limit as i64 - count as i64;
    let color = if left < 0 {
        p.danger
    } else if left <= 20 {
        p.caution
    } else {
        p.accent
    };
    let share = (count as f32 / limit.max(1) as f32).min(1.0);
    let ring = iced::widget::canvas(Ring { share, color, track: p.line }).width(22).height(22);
    let mut r = Row::new().spacing(6).align_y(Alignment::Center);
    if left <= 20 {
        r = r.push(label(left.to_string(), SMALL, color));
    }
    kit::tip(r.push(ring), format!("{count} of {limit} characters"))
}

/// The counter's ring: a track, and over it the share used.
struct Ring {
    share: f32,
    color: Color,
    track: Color,
}

impl<M> iced::widget::canvas::Program<M> for Ring {
    type State = crate::ui::Kept;

    fn draw(
        &self,
        kept: &crate::ui::Kept,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{LineCap, Path, Stroke, path::Arc};
        let key = crate::ui::key_of(&((self.share * 1000.0) as u32, self.color.into_rgba8(), self.track.into_rgba8()));
        vec![kept.draw(renderer, bounds.size(), key, |frame| {
            let center = frame.center();
            let radius = frame.width().min(frame.height()) / 2.0 - 2.0;
            frame.stroke(&Path::circle(center, radius), Stroke::default().with_color(self.track).with_width(2.0));
            if self.share > 0.0 {
                let start = -std::f32::consts::FRAC_PI_2;
                let end = start + self.share * std::f32::consts::TAU;
                let arc = Path::new(|b| b.arc(Arc { center, radius, start_angle: iced::Radians(start), end_angle: iced::Radians(end) }));
                frame.stroke(&arc, Stroke::default().with_color(self.color).with_width(2.0).with_line_cap(LineCap::Round));
            }
        })]
    }
}

/// Repost it, or quote it with words of one's own; one's own post is quoted, not reposted.
fn repost_choices<'a>(post: &'a Post, mine: bool) -> El<'a> {
    let items = column![
        kit::menu_item(Glyph::Repost, if mine { "Repost (not your own)" } else { "Repost" }, (!mine).then(|| Msg::Repost(post.id.clone())), false),
        kit::menu_item(Glyph::Pen, "Quote", Some(Msg::Quote(post.clone())), false),
    ];
    kit::menu(items, 260.0)
}

/// LinkedIn's reactions as a row of choices on a floating card, the reader's own washed in the accent.
fn reaction_choices(post: &Post) -> El<'_> {
    let p = pal();
    let mut choices = Row::new().spacing(6);
    for r in Reaction::ALL {
        let on = post.your_reaction == Some(r);
        choices = choices.push(
            button(text(r.label()).size(14.0).font(fonts().ui)).padding([6.0, 14.0]).on_press(Msg::React(post.id.clone(), r)).style(look::choice(p, on)),
        );
    }
    container(choices.wrap().vertical_spacing(6)).padding([8.0, 10.0]).style(look::menu(p)).into()
}

/// A post's ⋯ menu: copy its link; for its author, pin it and delete it; for anyone else, mute or block its author
/// and report it; and what an organization's admins, Alelyon's moderators and a community's keepers may do.
fn post_menu<'a>(s: &'a State, post: &'a Post, mine: bool, admin: bool) -> El<'a> {
    let id = post.id.clone();
    let mut items = Column::new().push(kit::menu_item(Glyph::Link, "Copy link", Some(Msg::CopyLink(id.clone())), false));
    if mine {
        if post.reply_to.is_none() && post.org.is_none() {
            items = items.push(kit::menu_item(Glyph::Pin, "Pin to your page", Some(Msg::Pin(id.clone())), false));
        }
        items = items.push(kit::menu_item(Glyph::Trash, "Delete", Some(Msg::Delete(id.clone())), true));
    } else {
        if admin {
            items = items.push(kit::menu_item(Glyph::Trash, "Delete (admin)", Some(Msg::Delete(id.clone())), true));
        }
        if post.org.is_none()
            && let Some(u) = post.author.as_ref().and_then(|a| a.username.clone())
        {
            items = items.push(kit::menu_item(Glyph::Mute, format!("Mute @{u}"), Some(Msg::Mute(u.clone(), true)), false));
            items = items.push(kit::menu_item(Glyph::Block, format!("Block @{u}"), Some(Msg::Block(u, true)), false));
        }
        items = items.push(kit::menu_item(Glyph::Flag, "Report post", Some(Msg::Reporting(Some(Target::Post(id.clone())))), false));
        if moderator(s) {
            let hide = Msg::ModAct(ModAct::HidePost, Some(Target::Post(id.clone())), None);
            items = items.push(kit::menu_item(Glyph::Shield, "Hide (moderator)", Some(hide), true));
        }
    }
    // The community's owners and moderators take a conversation out of it (its first post).
    if post.community.as_ref().is_some_and(|c| s.keeps(&c.handle)) {
        items = items.push(kit::menu_item(Glyph::Communities, "Take out of the community", Some(Msg::TakeOut(id)), false));
    }
    kit::menu(items, 300.0)
}

/// A job opening in a round frame: where, how and the pay, and until when it is open; Apply (with a note) or
/// Withdraw; and for the organization's owners and admins, how many applied and Close.
fn job_view<'a>(s: &'a State, post: &'a Post, job: &'a Job, admin: bool) -> El<'a> {
    let p = pal();
    let mut parts = Vec::new();
    if !job.location.is_empty() {
        parts.push(job.location.as_str());
    }
    parts.extend([job.workplace.label(), job.employment.label()]);
    if !job.pay.is_empty() {
        parts.push(job.pay.as_str());
    }
    let until = if job.closed {
        "Closed to applications".to_string()
    } else {
        format!("Open until {}", job.closes_at.get(..10).unwrap_or(job.closes_at.as_str()))
    };
    let what = row![icons::icon(Glyph::Jobs, 18.0, p.accent, false), strong(parts.join(" \u{00B7} "), BODY, p.text)].spacing(8).align_y(Alignment::Center);
    let mut col = column![what, label(until, SMALL, p.dim)].spacing(6);
    let mut acts = Row::new().spacing(8).align_y(Alignment::Center);
    if admin {
        acts = acts.push(small(format!("Applicants ({})", job.applicants.unwrap_or(0)), Some(Msg::Go(Screen::Applicants(post.id.clone())))));
        if !job.closed {
            acts = acts.push(small("Close the job", Some(Msg::CloseJob(post.id.clone()))));
        }
    } else if job.applied {
        acts = acts.push(label("\u{2713} Applied", 14.0, p.accent)).push(small("Withdraw", Some(Msg::Withdraw(post.id.clone()))));
    } else if !job.closed && s.me.as_ref().is_some_and(|m| m.me.has_page) {
        acts = acts.push(kit::pill("Apply", Some(Msg::Applying(Some(post.id.clone()))), Pill::Solid));
    }
    col = col.push(acts);
    if let Some((_, words)) = s.applying.as_ref().filter(|(id, _)| id == &post.id) {
        col = col.push(kit::field("A note for the organization (optional)", words, Msg::ApplyNote).on_submit(Msg::Apply));
        col = col.push(
            row![
                container(kit::note("Its owners and admins will see your page and this note.")).width(Length::Fill),
                small("Cancel", Some(Msg::Applying(None))),
                kit::primary("Apply", Some(Msg::Apply)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(look::frame(p)).into()
}

/// A poll as X draws one: once the votes show, each option over a bar as long as its share, the reader's choice in
/// bold over the accent's wash and marked; before, each option a button. Then the total and whether it is open.
fn poll_view<'a>(post: &'a Post, poll: &'a Poll, mine: bool) -> El<'a> {
    let p = pal();
    let mut col = Column::new().spacing(8);
    match poll.shares() {
        Some(shares) => {
            for (i, (option, share)) in poll.options.iter().zip(shares).enumerate() {
                let chosen = poll.you_voted == Some(i);
                let share = share.min(100) as u16;
                let bar: El<'a> = if share > 0 {
                    row![
                        container(space()).width(Length::FillPortion(share)).height(Length::Fill).style(look::share(p, chosen)),
                        space().width(Length::FillPortion(100 - share)),
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    space().into()
                };
                let face = |words: String| -> El<'a> { if chosen { strong(words, BODY, p.text).into() } else { label(words, BODY, p.text).into() } };
                let mut words = Row::new().spacing(8).align_y(Alignment::Center).push(face(option.clone()));
                if chosen {
                    words = words.push(label("\u{2713}", BODY, p.accent));
                }
                words = words.push(space().width(Length::Fill)).push(face(format!("{share}%")));
                col = col.push(stack![
                    container(bar).width(Length::Fill).height(34),
                    container(words).width(Length::Fill).height(34).padding([0.0, 12.0]).align_y(Alignment::Center),
                ]);
            }
        }
        None => {
            for (i, option) in poll.options.iter().enumerate() {
                let words = container(text(option.as_str()).size(BODY).font(fonts().ui_strong)).center_x(Length::Fill);
                let vote = (!poll.closed && !mine).then(|| Msg::Vote(post.id.clone(), i));
                col = col.push(button(words).padding([7.0, 16.0]).width(Length::Fill).on_press_maybe(vote).style(look::outline_in(p, p.accent)));
            }
        }
    }
    let votes = if poll.total == 1 { "1 vote".to_string() } else { format!("{} votes", poll.total) };
    col.push(label(format!("{votes} \u{00B7} {}", if poll.closed { "Final results" } else { "Open" }), SMALL, p.dim)).into()
}

/// A quoted post, as X draws one inside the post that quotes it: in a round frame, its author's small picture, name,
/// handle and time, then its words; pressed, it opens.
fn quoted<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    let p = pal();
    let (url, who) = post_picture(post);
    let mut by = Row::new().spacing(6).align_y(Alignment::Center);
    by = by.push(if post.org.is_some() { logo(s, url, who, 20.0) } else { avatar(s, url, who, 20.0) });
    by = match (&post.author, &post.org) {
        (_, Some(o)) => by.push(strong(o.name.as_str(), BODY, p.text)).push(kit::ticker(&o.ticker, None)),
        (Some(a), None) => by.push(strong(a.name(), BODY, p.text)).push(label(a.handle(), BODY, p.dim)),
        (None, None) => by.push(kit::note("Posted by someone no longer here.")),
    };
    if !post.created_at.is_empty() {
        by = by.push(label(format!("\u{00B7} {}", short_when(&post.created_at)), BODY, p.dim));
    }
    let mut col = column![by].spacing(6);
    if let Some(title) = &post.title {
        col = col.push(strong(title.as_str(), BODY, p.text));
    }
    if !post.text.is_empty() {
        col = col.push(label(ui::cut(&post.text, 400), BODY, p.text));
    }
    match post.media.len() {
        0 => {}
        1 => col = col.push(kit::note("1 photo")),
        n => col = col.push(kit::note(format!("{n} photos"))),
    }
    let framed = container(col).padding([10.0, 12.0]).width(Length::Fill).style(look::frame(p));
    button(framed).padding(0).width(Length::Fill).on_press(Msg::Go(Screen::Thread(post.id.clone()))).style(look::bare(p)).into()
}

/// Why a post, a page or an organization is reported: the reasons as small buttons on a floating card.
fn report_reasons<'a>(target: Target) -> El<'a> {
    let p = pal();
    let mut r = Row::new().spacing(6).align_y(Alignment::Center).push(strong("Why?", 14.0, p.text));
    for reason in Reason::ALL {
        r = r.push(small(reason.label(), Some(Msg::Report(target.clone(), reason))));
    }
    container(r.push(small("Cancel", Some(Msg::Reporting(None)))).wrap().vertical_spacing(6)).padding([10.0, 12.0]).style(look::menu(p)).into()
}

/// Posts as rows of the column; what to say when there are none; Show more when the service has more.
fn posts_list<'a>(s: &'a State, posts: &'a [Post], more: bool, empty: &'a str) -> El<'a> {
    let mut col = Column::new();
    if posts.is_empty() && s.loading {
        col = col.push(skeleton());
    } else if posts.is_empty() {
        col = col.push(container(label(empty, BODY, pal().dim)).padding([28.0, 24.0]).width(Length::Fill));
    }
    for post in posts {
        col = col.push(post_card(s, post));
    }
    if more {
        col = col.push(more_row(Msg::More, "Show more"));
    }
    col.into()
}

/// A row that reads more: the accent's words across the column.
fn more_row<'a>(msg: Msg, words: &'a str) -> El<'a> {
    let p = pal();
    let words = container(label(words, BODY, p.accent)).center_x(Length::Fill);
    column![button(words).padding([16.0, 16.0]).width(Length::Fill).on_press(msg).style(look::row(p)), kit::hairline()].into()
}

fn feed<'a>(s: &'a State, page: Option<&'a pages::Posts>, empty: &'a str) -> El<'a> {
    match page {
        Some(p) => posts_list(s, &p.posts, p.more, empty),
        None if s.loading => skeleton(),
        None => space().into(),
    }
}

// ------------------------------------------------------------------- home: the composer and the feed

/// Home: the composer over the feed the header chose (For you, Following).
fn home(s: &State) -> El<'_> {
    let Some(me) = &s.me else { return space().into() };
    let p = pal();
    let mut col = Column::new();
    if me.suspended {
        col = col.push(container(kit::notice(SUSPENDED, p.caution)).padding([12.0, 16.0]));
    } else if !me.me.has_page {
        col = col.push(container(kit::notice("Open your page to post: My page, in the navigation.", p.accent)).padding([12.0, 16.0]));
    } else {
        if me.drafts.failed > 0 {
            let words = if me.drafts.failed == 1 {
                "A scheduled post could not be posted at its time.".to_string()
            } else {
                format!("{} scheduled posts could not be posted at their time.", me.drafts.failed)
            };
            let line = row![container(kit::notice(words, p.caution)).width(Length::Fill), small("See why in Drafts", Some(Msg::Go(Screen::Drafts)))];
            col = col.push(container(line.spacing(10).align_y(Alignment::Center)).padding([12.0, 16.0]));
        }
        col = col.push(composer(s));
    }
    col.push(match s.home_feed {
        HomeFeed::ForYou => match &s.for_you {
            Some((_, page)) => posts_list(
                s,
                &page.posts,
                page.more,
                "Nothing here yet. For you ranks others' posts of the last three days; Following shows those you follow.",
            ),
            None if s.loading => skeleton(),
            None => space().into(),
        },
        HomeFeed::Following => {
            feed(s, s.home.as_ref(), "Your posts, and those of the people and organizations you follow, show here. Explore shows everyone's.")
        }
    })
    .into()
}

// ------------------------------------------------------------------- discovery: explore, a hashtag, a search

/// Explore: what is trending (and who to follow, when there is no rail to show them) over everyone's latest posts.
fn explore(s: &State) -> El<'_> {
    let (_, wide) = columns(room(s));
    let mut col = Column::new();
    if let Some(card) = trending_card(s, None) {
        col = col.push(container(card).padding(iced::padding::top(12).left(16).right(16)));
    }
    if !wide && let Some(card) = launches_card(s) {
        col = col.push(container(card).padding(iced::padding::top(12).left(16).right(16)));
    }
    if !wide && let Some(card) = follow_card(s, None) {
        col = col.push(container(card).padding(iced::padding::top(12).left(16).right(16)));
    }
    col.push(kit::column_title("Latest"))
        .push(kit::hairline())
        .push(feed(s, s.explore.as_ref(), "Nothing is posted yet."))
        .into()
}

fn tagged(s: &State) -> El<'_> {
    match &s.tagged {
        Some(t) => posts_list(s, &t.posts.posts, t.posts.more, "No post carries this hashtag."),
        None => space().into(),
    }
}

/// What a search found, of the kind its header's tabs chose: people, organizations, posts.
fn search(s: &State) -> El<'_> {
    let shown = s.found.as_ref().filter(|(q, k, _)| s.screen.as_ref() == Some(&Screen::Search(q.clone())) && *k == s.search_kind);
    let Some((_, kind, found)) = shown else { return space().into() };
    let all = *kind == SearchKind::All;
    let mut col = Column::new();
    fn part(words: &str) -> El<'_> {
        kit::column_title(words)
    }
    if matches!(kind, SearchKind::All | SearchKind::People) {
        if all {
            col = col.push(part("People"));
        }
        if found.people.is_empty() {
            col = col.push(kit::block(kit::note("Nobody with a page matches.")));
        }
        for who in &found.people {
            col = col.push(person_row(s, who));
        }
    }
    if matches!(kind, SearchKind::All | SearchKind::Orgs) {
        if all {
            col = col.push(part("Organizations"));
        }
        if found.orgs.is_empty() {
            col = col.push(kit::block(kit::note("No organization matches.")));
        }
        for o in &found.orgs {
            col = col.push(org_row(s, o));
        }
    }
    if matches!(kind, SearchKind::All | SearchKind::Products) {
        if all {
            col = col.push(part("Products"));
        }
        if found.products.is_empty() {
            col = col.push(kit::block(kit::note("No product matches.")));
        }
        for summary in &found.products {
            col = col.push(product_row(s, summary));
        }
    }
    if matches!(kind, SearchKind::All | SearchKind::Posts) {
        if all {
            col = col.push(part("Posts")).push(kit::hairline());
        }
        col = col.push(posts_list(s, &found.posts.posts, found.posts.more && !all, "No post matches."));
        if all && found.posts.more {
            col = col.push(more_row(Msg::SearchKind(SearchKind::Posts), "More posts"));
        }
    }
    col.into()
}

/// The composer as X draws its own: the writer's picture beside a frameless text in large words; who it is posted
/// as and where, as accent pills above it; who may reply under it; a hairline; then the tools in the accent (photos,
/// a poll, a schedule, an article), the ring of the room used, Drafts, Save draft and Post.
fn composer(s: &State) -> El<'_> {
    let p = pal();
    let c = &s.composer;
    let article = c.kind == Kind::Article;
    let mut col = Column::new().spacing(10);
    let mut audience = Row::new().spacing(14).align_y(Alignment::Center);
    let mut any = false;
    let admin = s.me.as_ref().map(|m| m.admin_of()).unwrap_or_default();
    if !admin.is_empty() {
        let mut choices = vec![AsWho(None)];
        choices.extend(admin.into_iter().map(|t| AsWho(Some(t))));
        let list = pick_list(choices, Some(AsWho(c.as_who.clone())), Msg::AsWho)
            .padding([4.0, 12.0])
            .text_size(14.0)
            .font(fonts().ui_strong)
            .style(look::audience(p))
            .menu_style(look::picker_menu(p));
        audience = audience.push(named_pill("As", list.into()));
        any = true;
    }
    // Post in one of the reader's communities: one's own post, not an organization's, and not a quote.
    let joined: Vec<InWhere> =
        s.my_communities.iter().flatten().filter(|m| m.you_belong()).map(|m| InWhere(Some(m.community.handle.clone()))).collect();
    if !joined.is_empty() && c.as_who.is_none() && !c.quotes() {
        let mut choices = vec![InWhere(None)];
        choices.extend(joined);
        let list = pick_list(choices, Some(InWhere(c.in_community.clone())), Msg::InCommunity)
            .padding([4.0, 12.0])
            .text_size(14.0)
            .font(fonts().ui_strong)
            .style(look::audience(p))
            .menu_style(look::picker_menu(p));
        audience = audience.push(named_pill("In", list.into()));
        any = true;
    }
    // About one of the products the reader keeps (ADR-0056): its update, or its launch. One's own top-level post,
    // not a quote or a community's; posted as an organization, about that organization's products only.
    let kept: Vec<AboutProduct> = s
        .my_products
        .iter()
        .flatten()
        .filter(|m| c.as_who.as_ref().is_none_or(|ticker| m.org.as_ref().is_some_and(|o| &o.ticker == ticker)))
        .map(|m| AboutProduct(Some((m.product.handle.clone(), m.product.name.clone()))))
        .collect();
    if !kept.is_empty() && !c.quotes() && c.in_community.is_none() {
        let chosen = kept.iter().find(|a| a.0.as_ref().map(|(h, _)| h) == c.product.as_ref()).cloned().unwrap_or(AboutProduct(None));
        let mut choices = vec![AboutProduct(None)];
        choices.extend(kept);
        let list = pick_list(choices, Some(chosen), Msg::PostAbout)
            .padding([4.0, 12.0])
            .text_size(14.0)
            .font(fonts().ui_strong)
            .style(look::audience(p))
            .menu_style(look::picker_menu(p));
        audience = audience.push(named_pill("About", list.into()));
        any = true;
    }
    if any {
        col = col.push(audience);
    }
    if c.draft_id.is_some() {
        col = col.push(
            row![
                label("Writing a draft again: Post posts it, and Save draft keeps the changes.", SMALL, p.accent).width(Length::Fill),
                small("Close it", Some(Msg::CloseDraft)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    if article {
        col = col.push(text_input("Title", &c.title).on_input(Msg::Title).padding([4.0, 0.0]).size(22).font(fonts().ui_strong).style(look::bare_field(p)));
    }
    // the people a mention being typed may name, under the text; Up and Down choose, Enter or Tab writes, Esc leaves it
    let people = s.mentioning.as_ref().and_then(|m| mention_list(s, m));
    let open = people.is_some();
    let editor = text_editor(&c.body)
        .id(keys::COMPOSER)
        .placeholder(match (article, c.product.is_some(), c.launches()) {
            (true, _, _) => "Write your article",
            (false, _, true) => "What does this version bring?",
            (false, true, false) => "What changed? Its followers see it, and it shows on the product's page.",
            (false, false, false) => "What are you building?",
        })
        .on_action(Msg::Body)
        .key_binding(move |press| mention_keys(open, press))
        .height(Length::Shrink)
        .min_height(if article { 220.0 } else { 56.0 })
        .max_height(if article { 560.0 } else { 360.0 })
        .padding([6.0, 0.0])
        .size(if article { 17.0 } else { 20.0 })
        .font(fonts().ui)
        .style(look::bare_editor(p));
    col = col.push(popover_at(editor, people, Msg::MentionClose, Side::Left));
    if !c.photos.is_empty() {
        let mut list = Column::new().spacing(10);
        for (i, photo) in c.photos.iter().enumerate() {
            let alt = kit::field("Describe the photo for people who cannot see it (optional)", &photo.alt, move |t| Msg::PhotoAlt(i, t));
            list = list.push(
                row![
                    thumb(s, &photo.url, 72.0),
                    column![label(photo.name.as_str(), SMALL, p.dim), alt].spacing(6).width(Length::Fill),
                    small("Remove", Some(Msg::RemovePhoto(i))),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(list.push(kit::note(PHOTO_RULE)));
    }
    if let Some(q) = &c.quoting {
        col = col.push(row![label("Quoting", SMALL, p.dim), space().width(Length::Fill), small("Cancel", Some(Msg::Unquote))].align_y(Alignment::Center));
        col = col.push(quoted(s, q));
    } else if c.draft_quote.is_some() {
        col = col.push(
            row![label("Quoting a post (reading it\u{2026})", SMALL, p.dim), space().width(Length::Fill), small("Cancel", Some(Msg::Unquote))]
                .align_y(Alignment::Center),
        );
    }
    if c.poll_on && c.kind == Kind::Post && !c.quotes() {
        let mut poll = Column::new().spacing(8);
        for (i, option) in c.poll_options.iter().enumerate() {
            let hint = if i < 2 { "An option" } else { "Another option (optional)" };
            poll = poll.push(kit::field(hint, option, move |t| Msg::PollOption(i, t)));
        }
        let length = kit::picked(pick_list(PollLength::ALL, Some(c.poll_length), Msg::PollHours));
        poll = poll.push(row![label("Open for", 14.0, p.dim), length].spacing(8).align_y(Alignment::Center));
        col = col.push(container(poll).padding(12).width(Length::Fill).style(look::frame(p)));
    }
    // A job opening is posted at once: it is not drafted.
    if article && c.as_who.is_some() && c.draft_id.is_none() {
        col = col.push(small(if c.job_on { "Not a job opening" } else { "Make it a job opening" }, Some(Msg::JobToggle)));
        if c.job_on {
            let job = &c.job;
            col = col.push(
                row![
                    kit::picked(pick_list(Workplace::ALL, Some(job.workplace), Msg::JobWorkplace)),
                    kit::picked(pick_list(Employment::ALL, Some(job.employment), Msg::JobEmployment)),
                    label("Open for", 14.0, p.dim),
                    kit::picked(pick_list(JobDays::ALL, Some(c.job_days), Msg::JobDays)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .wrap(),
            );
            let place = if job.workplace == Workplace::Remote { "Location (optional)" } else { "Location" };
            col = col.push(
                row![
                    kit::field(place, &job.location, Msg::JobLocation).width(240),
                    kit::field("Pay (optional), as you would write it", &job.pay, Msg::JobPay),
                ]
                .spacing(8),
            );
        }
    }
    if c.launches() {
        col = col.push(
            row![
                icons::icon(Glyph::Rocket, 16.0, p.accent, false),
                label("Launching", 14.0, p.dim),
                kit::field("Its version: 2.0, v1.4", &c.launch_version, Msg::LaunchVersion).width(190),
                kit::note("Once for each major version. People can support it for a day, and its followers are told."),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap(),
        );
    }
    if c.schedule_on {
        col = col.push(schedule_fields(c));
    }
    // Who may reply, as X sets it: the accent's words under the text.
    let replies = pick_list(ReplyRule::ALL, Some(c.replies), Msg::Replies)
        .padding([4.0, 12.0])
        .text_size(14.0)
        .font(fonts().ui_strong)
        .style(look::audience(p))
        .menu_style(look::picker_menu(p));
    col = col.push(row![icons::icon(Glyph::Globe, 16.0, p.accent, false), replies].spacing(6).align_y(Alignment::Center));
    col = col.push(kit::hairline());
    let room = c.photos.len() + s.uploading < pages::POST_MAX_PHOTOS;
    let mut tools = Row::new().align_y(Alignment::Center).push(tool(
        Glyph::Photo,
        if room { "Add photos" } else { "Four photos is the most a post holds" },
        false,
        (room && !c.poll_on).then_some(Msg::PickPhotos(PhotoFor::Post)),
    ));
    if c.kind == Kind::Post && !c.quotes() {
        // a post carries a poll or photos, not both
        tools = tools.push(tool(
            Glyph::Poll,
            if c.poll_on { "Remove the poll" } else { "Add a poll" },
            c.poll_on,
            (c.poll_on || c.photos.is_empty()).then_some(Msg::PollToggle),
        ));
    }
    tools = tools.push(tool(Glyph::Calendar, if c.schedule_on { "Not scheduled" } else { "Schedule it" }, c.schedule_on, Some(Msg::ScheduleOn(!c.schedule_on))));
    let kind = if article { Some(Msg::Kind(Kind::Post)) } else { (!c.quotes()).then_some(Msg::Kind(Kind::Article)) };
    tools = tools.push(tool(Glyph::Pen, if article { "Back to a post" } else { "Write an article" }, article, kind));
    if c.product.is_some() && !c.quotes() {
        tools = tools.push(tool(Glyph::Rocket, if c.launch_on { "Not a launch" } else { "Launch a version" }, c.launch_on, Some(Msg::LaunchOn(!c.launch_on))));
    }
    if s.uploading > 0 {
        tools = tools.push(label("Uploading\u{2026}", SMALL, p.dim));
    }
    let count = c.chars();
    let mut bottom = row![tools, space().width(Length::Fill)].spacing(10).align_y(Alignment::Center);
    if count > 0 {
        bottom = bottom.push(counter(count, c.limit()));
    }
    let saved = s.me.as_ref().map(|m| m.drafts.saved).unwrap_or(0);
    if saved > 0 {
        bottom = bottom.push(kit::action(Glyph::Drafts, saved, p.accent, false, Some(Msg::Go(Screen::Drafts)), "Drafts"));
    }
    let uploading = (s.uploading > 0).then(|| "Wait for the photos to upload".to_string());
    let why_not = uploading.or_else(|| c.why_not());
    let job = (c.job_on && article && c.as_who.is_some()).then(|| "A job opening is posted at once, not drafted".to_string());
    let save = kit::outline("Save draft", (why_not.is_none() && job.is_none()).then_some(Msg::SaveDraft));
    let post = kit::primary(if c.sending { "Posting\u{2026}" } else { "Post" }, why_not.is_none().then_some(Msg::Submit));
    bottom = bottom.push(match why_not.clone().or(job) {
        Some(why) => kit::tip(save, why),
        None => save,
    });
    bottom = bottom.push(match why_not.filter(|_| !c.sending) {
        Some(why) => kit::tip(post, why),
        None => post,
    });
    col = col.push(bottom);
    // The writer's picture beside what they write, as the social sites draw a composer.
    let me = s.me.as_ref().map(|m| &m.me);
    let face = avatar(s, me.and_then(|m| m.avatar.as_ref()), me.map_or("", Person::name), 40.0);
    column![container(row![face, col.width(Length::Fill)].spacing(12)).padding([12.0, 16.0]), kit::hairline()].into()
}

/// A choice in the composer's accent pill, its name before it.
fn named_pill<'a>(words: &'a str, list: El<'a>) -> El<'a> {
    row![label(words, 14.0, pal().dim), list].spacing(6).align_y(Alignment::Center).into()
}

/// While people are offered for a mention, the composer's Up and Down choose among them, Enter or Tab writes the chosen
/// one and Esc leaves the mention as typed; every other key, and every key otherwise, does what it always does.
fn mention_keys(open: bool, press: text_editor::KeyPress) -> Option<text_editor::Binding<Msg>> {
    use iced::keyboard::{Key, key::Named};
    if open && !press.modifiers.command() && !press.modifiers.alt() {
        match press.key.as_ref() {
            Key::Named(Named::ArrowDown) => return Some(text_editor::Binding::Custom(Msg::MentionMove(1))),
            Key::Named(Named::ArrowUp) => return Some(text_editor::Binding::Custom(Msg::MentionMove(-1))),
            Key::Named(Named::Enter | Named::Tab) => return Some(text_editor::Binding::Custom(Msg::MentionPick(None))),
            Key::Named(Named::Escape) => return Some(text_editor::Binding::Custom(Msg::MentionClose)),
            _ => {}
        }
    }
    text_editor::Binding::from_key_press(press)
}

/// The people a mention may name, as X lists them under the text: each picture, name and handle, the one the
/// keyboard is on tinted. None while there are none.
fn mention_list<'a>(s: &'a State, m: &'a Mentioning) -> Option<El<'a>> {
    if m.found.is_empty() {
        return None;
    }
    let p = pal();
    let mut items = Column::new();
    for (i, who) in m.found.iter().enumerate() {
        let names = column![strong(who.name(), BODY, p.text), label(who.handle(), SMALL, p.dim)].spacing(1);
        let line = row![avatar(s, who.avatar.as_ref(), who.name(), 36.0), names].spacing(10).align_y(Alignment::Center);
        let b = button(line).padding([8.0, 14.0]).width(Length::Fill).on_press(Msg::MentionPick(Some(i)));
        items = items.push(if i == m.at { b.style(look::chosen_menu_row(p)) } else { b.style(look::menu_row(p)) });
    }
    Some(kit::menu(items, 320.0))
}

/// The schedule's day and time, in UTC as CENTCOM shows every time, with when that is from now.
fn schedule_fields(c: &Composer) -> El<'_> {
    let now = crate::utc::now();
    let (hint, ready) = match c.schedule_at() {
        Some((_, at)) if at < now + 60.0 => ("That time has passed, or is under a minute away.".to_string(), false),
        Some((_, at)) if at > now + 365.0 * 86_400.0 => ("A post is scheduled at most a year ahead.".to_string(), false),
        Some((_, at)) => (format!("Posts in {}.", until(at - now)), true),
        None => ("Write the day as 2026-10-15 and the time as 09:30.".to_string(), false),
    };
    row![
        label("Post it on", 13.0, pal().dim),
        text_input("2026-10-15", &c.schedule_day).on_input(Msg::ScheduleDay).on_submit(Msg::Schedule).padding([10.0, 12.0]).size(15).width(120).style(look::field(pal())),
        label("at", 13.0, pal().dim),
        text_input("09:30", &c.schedule_time).on_input(Msg::ScheduleTime).on_submit(Msg::Schedule).padding([10.0, 12.0]).size(15).width(80).style(look::field(pal())),
        label("UTC", 13.0, pal().dim),
        label(hint, 12.0, if ready { pal().dim } else { pal().caution }),
        space().width(Length::Fill),
        kit::primary("Schedule", (ready && c.draftable()).then_some(Msg::Schedule)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .wrap()
    .into()
}

/// "3 d 4 h", "2 h 05 min", "12 min": how long until a time.
fn until(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    match s {
        0..=3599 => format!("{} min", s.div_ceil(60)),
        3600..=86_399 => format!("{} h {:02} min", s / 3600, s % 3600 / 60),
        _ => format!("{} d {} h", s / 86_400, s % 86_400 / 3600),
    }
}

/// "2026-10-15 09:30 UTC", from RFC 3339.
fn utc_time(at: &str) -> String {
    crate::utc::parse(at).map(|t| format!("{} UTC", &crate::utc::full(t)[..16])).unwrap_or_else(|| at.to_string())
}

// ------------------------------------------------------------------- drafts and scheduled posts (ADR-0055)

/// The reader's drafts: the scheduled first, the soonest first, then the rest.
fn drafts(s: &State) -> El<'_> {
    let Some(found) = &s.drafts else { return space().into() };
    if found.is_empty() {
        return if s.loading { space().into() } else { kit::empty("No drafts yet", "Save what you are writing, or schedule it, from the composer on Home.") };
    }
    let mut col = Column::new();
    for d in found {
        col = col.push(draft_card(s, d));
    }
    col.push(kit::padded(kit::note("Only you see your drafts. A scheduled post is posted as you at its time, if it can be then; times are UTC."))).into()
}

fn draft_card<'a>(s: &'a State, d: &'a Draft) -> El<'a> {
    let p = &d.post;
    let state = match (&d.failed, &d.publish_at) {
        (Some(_), _) => kit::chip("Not posted", pal().caution),
        (None, Some(at)) => kit::chip(format!("Scheduled for {}", utc_time(at)), pal().accent),
        (None, None) => kit::chip("Draft", pal().dim),
    };
    let mut head = Row::new().spacing(8).align_y(Alignment::Center).push(state);
    if p.kind == Kind::Article {
        head = head.push(label("Article", 13.0, pal().dim));
    }
    if let Some(t) = &p.as_org {
        head = head.push(label(format!("as ({t})"), 13.0, pal().dim));
    }
    if let Some(h) = &p.community {
        head = head.push(label(format!("in c/{h}"), 13.0, pal().accent));
    }
    if p.repost_of.is_some() {
        head = head.push(label("Quotes a post", 13.0, pal().dim));
    }
    if p.poll.is_some() {
        head = head.push(label("Poll", 13.0, pal().dim));
    }
    match d.photos.len() {
        0 => {}
        1 => head = head.push(label("1 photo", 13.0, pal().dim)),
        n => head = head.push(label(format!("{n} photos"), 13.0, pal().dim)),
    }
    head = head.push(label(format!("changed {}", when(&d.updated_at)), 12.5, pal().dim));
    let mut col = Column::new().spacing(6).push(head.wrap());
    if let Some((_, words)) = &d.failed {
        col = col.push(label(format!("It could not be posted: {words} Edit it, then post or schedule it again."), 13.5, pal().caution));
    }
    if let Some(title) = &p.title {
        col = col.push(strong(title.as_str(), 15.0, pal().text));
    }
    col = col.push(label(ui::cut(&p.text, 280), 14.0, pal().text));
    let confirming = s.confirm_delete_draft.as_deref() == Some(d.id.as_str());
    col = col.push(
        row![
            small("Edit", Some(Msg::OpenDraft(d.id.clone()))),
            small("Post now", Some(Msg::PostDraft(d.id.clone()))),
            small(if confirming { "Delete it: press again" } else { "Delete" }, Some(Msg::DeleteDraft(d.id.clone()))),
        ]
        .spacing(6),
    );
    kit::block(col)
}

// ------------------------------------------------------------------- jobs

fn jobs(s: &State) -> El<'_> {
    let q = &s.jobs_query;
    let filters = row![
        text_input("Search jobs", &q.words).on_input(Msg::JobsWords).on_submit(Msg::JobsSearch).padding([10.0, 12.0]).size(15).width(260).style(look::field(pal())),
        pick_list(WorkplacePick::ALL, Some(WorkplacePick(q.workplace)), Msg::JobsWorkplace)
            .padding([8.0, 12.0])
            .text_size(14.0)
            .font(fonts().ui)
            .style(look::picker(pal()))
            .menu_style(look::picker_menu(pal())),
        pick_list(EmploymentPick::ALL, Some(EmploymentPick(q.employment)), Msg::JobsEmployment)
            .padding([8.0, 12.0])
            .text_size(14.0)
            .font(fonts().ui)
            .style(look::picker(pal()))
            .menu_style(look::picker_menu(pal())),
        kit::outline("Search", Some(Msg::JobsSearch)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let empty = "No open jobs match. An organization's owners and admins open one as an article: Post as the organization, \
                 then Make it a job opening.";
    column![kit::padded(filters.wrap()), kit::hairline(), feed(s, s.jobs.as_ref(), empty)].into()
}

fn applicants(s: &State) -> El<'_> {
    let Some((_, list)) = &s.applicants else { return space().into() };
    let mut col = Column::new();
    if list.is_empty() {
        col = col.push(kit::padded(kit::note("Nobody has applied yet.")));
    }
    for a in list {
        let mut card = column![
            row![name_line(&a.person), space().width(Length::Fill), label(when(&a.applied_at), 12.5, pal().dim)].align_y(Alignment::Center)
        ]
        .spacing(6);
        if !a.note.is_empty() {
            card = card.push(post_text(a.note.as_str(), BODY));
        }
        col = col.push(kit::block(card));
    }
    col.push(kit::padded(kit::note("Only the organization's owners and admins see who applied. An application is deleted 180 days after its job closes.")))
        .into()
}

// ------------------------------------------------------------------- a person's page

/// The height of a page's banner, a third of the column's width as X's are; a page's picture, which rises half its
/// height over the banner's foot.
const BANNER: f32 = 200.0;
const PICTURE: f32 = 134.0;

/// The top of a page: the banner across the column (without one, a band in the page's own colour, the colour of its
/// initial), the picture over its foot in a ring of the page's colour (`radius` rounds the ring as the picture is
/// rounded), and the page's actions at the right, just under the banner.
fn page_top<'a>(s: &'a State, banner_url: Option<&String>, name: &str, picture: El<'a>, actions: El<'a>, radius: f32) -> El<'a> {
    let p = pal();
    let rise = (PICTURE + 8.0) / 2.0;
    let hue = look::initial_color(p, name);
    let band: El<'a> = banner(s, banner_url).unwrap_or_else(|| container(space()).width(Length::Fill).height(BANNER).style(look::band(p, hue)).into());
    let ringed = container(picture).padding(4).style(look::ring(p, radius + 4.0));
    stack![
        column![band, space().height(rise)],
        container(row![ringed, space().width(Length::Fill), column![space().height(rise + 12.0), actions]])
            .padding(iced::padding::top(BANNER - rise).left(16).right(16))
            .width(Length::Fill),
    ]
    .into()
}

/// A page's name, in the display face.
fn page_name<'a>(name: &str) -> El<'a> {
    text(name.to_string()).size(26.0).font(fonts().display).color(pal().text).into()
}

/// The ⋯ of a page's top, a round outline with its menu floating under it while it is open, and beside it the page's
/// main action.
fn page_actions<'a>(s: &'a State, main: El<'a>, menu: Option<El<'a>>) -> El<'a> {
    let p = pal();
    let more = button(icons::icon(Glyph::More, 18.0, p.text, false)).padding(8).on_press(Msg::PageMenu(!s.page_menu)).style(look::outline(p));
    let more = popover(kit::tip(more, "More"), menu.filter(|_| s.page_menu), Msg::PageMenu(false));
    row![more, main].spacing(8).align_y(Alignment::Center).into()
}

/// A person's page: the top with the picture over the banner; their name in the display face, their handle in the
/// mono face, their title and their organizations' tickers, their words, who they follow and who follows them; then
/// the tabs: posts, replies, photos, articles and the professional profile.
fn person(s: &State) -> El<'_> {
    let Some(page) = &s.person else { return space().into() };
    let p = pal();
    let who = &page.person;
    let mine = who.id == me_id(s);
    let main: El<'_> = if mine {
        kit::outline("Edit page", Some(Msg::Go(Screen::Mine)))
    } else {
        match &who.username {
            Some(u) if page.you_follow => kit::following(Msg::FollowPerson(u.clone(), false)),
            Some(u) => kit::primary("Follow", (!page.you_block).then(|| Msg::FollowPerson(u.clone(), true))),
            None => space().into(),
        }
    };
    let menu = match &who.username {
        Some(u) if !mine && s.page_menu => {
            let target = ListTarget::Person(u.clone());
            let mute = if page.you_mute { format!("Unmute @{u}") } else { format!("Mute @{u}") };
            let block = if page.you_block { format!("Unblock @{u}") } else { format!("Block @{u}") };
            let mut items = Column::new()
                .push(kit::menu_item(Glyph::Lists, "Add to or take off a list", (!page.you_block).then(|| Msg::PickLists(Some(target))), false))
                .push(kit::menu_item(Glyph::Mute, mute, Some(Msg::Mute(u.clone(), !page.you_mute)), false))
                .push(kit::menu_item(Glyph::Block, block, Some(Msg::Block(u.clone(), !page.you_block)), !page.you_block))
                .push(kit::menu_item(Glyph::Flag, format!("Report @{u}"), Some(Msg::Reporting(Some(Target::Person(u.clone())))), false));
            if moderator(s) {
                let suspend = Msg::ModAct(ModAct::SuspendPage, Some(Target::Person(u.clone())), None);
                items = items.push(kit::menu_item(Glyph::Shield, "Suspend page (moderator)", Some(suspend), true));
            }
            Some(kit::menu(items, 300.0))
        }
        _ => None,
    };
    let actions = if mine { main } else { page_actions(s, main, menu) };
    let mut about = Column::new().spacing(12).push(column![page_name(who.name()), kit::figure(who.handle(), 13.0, p.dim)].spacing(4));
    if !who.title.is_empty() || !who.tickers.is_empty() {
        let mut line = Row::new().spacing(6).align_y(Alignment::Center);
        if !who.title.is_empty() {
            line = line.push(label(who.title.as_str(), BODY, p.text));
        }
        if who.title_verified {
            line = line.push(kit::tip(label("\u{2713}", BODY, p.accent), "Title confirmed by the organization it names"));
        }
        for t in &who.tickers {
            line = line.push(kit::ticker(&t.ticker, Some(Msg::Go(Screen::Org(t.ticker.clone())))));
        }
        about = about.push(line.wrap());
    }
    if let Some(n) = verified_note(who) {
        about = about.push(n);
    }
    if !page.bio.is_empty() {
        about = about.push(post_text(page.bio.as_str(), BODY));
    }
    about = about.push(row![total(page.following, "Following", "Following"), total(page.followers, "Follower", "Followers")].spacing(20));
    if !mine && let Some(u) = &who.username {
        let target = ListTarget::Person(u.clone());
        if let Some(picker) = list_picker(s, &target) {
            about = about.push(container(column![picker, small("Done", Some(Msg::PickLists(None)))].spacing(8)).padding(14).width(Length::Fill).style(look::frame(p)));
        }
        if s.reporting == Some(Target::Person(u.clone())) {
            about = about.push(report_reasons(Target::Person(u.clone())));
        }
    }
    let picture = avatar(s, who.avatar.as_ref(), who.name(), PICTURE);
    let mut col = column![page_top(s, page.banner.as_ref(), who.name(), picture, actions, PICTURE / 2.0), container(about).padding([12.0, 16.0]).width(Length::Fill)];
    col = col.push(kit::tabs(&PersonTab::ALL, s.person_tab, |t| t.title().to_string(), Msg::PersonTab));
    match s.person_tab {
        PersonTab::Profile => {
            col = col.push(
                page.profile
                    .as_ref()
                    .and_then(|profile| profile_view(s, page, profile))
                    .unwrap_or_else(|| kit::empty("Nothing here yet", "No experience, education or skills listed yet.")),
            );
        }
        tab => {
            if tab == PersonTab::Posts
                && let Some(pinned) = &page.pinned
            {
                let mut over = Row::new().spacing(6).align_y(Alignment::Center).push(icons::icon(Glyph::Pin, 14.0, p.dim, false)).push(strong("Pinned", SMALL, p.dim));
                if mine {
                    over = over.push(small("Unpin", Some(Msg::Unpin)));
                }
                col = col.push(post_row(s, pinned, Some(over.into())));
            }
            let empty = match tab {
                PersonTab::Replies => "No replies yet.",
                PersonTab::Media => "No photos yet.",
                PersonTab::Articles => "No articles yet.",
                _ => "No posts yet.",
            };
            col = col.push(posts_list(s, &page.posts.posts, page.posts.more, empty));
        }
    }
    col.into()
}

/// An organization's page: the top with its logo over the banner; its name in the display face and its ticker as a
/// market writes one, its words, who follows it and who belongs; joining, and its owners' and admins' tools; then the
/// tabs: posts, open jobs, people and about.
fn org(s: &State) -> El<'_> {
    let Some(page) = &s.org else { return space().into() };
    let p = pal();
    let o = &page.org;
    let ticker = o.ticker.clone();
    let follow = if page.you_follow {
        kit::following(Msg::FollowOrg(ticker.clone(), false))
    } else {
        kit::primary("Follow", Some(Msg::FollowOrg(ticker.clone(), true)))
    };
    let target = ListTarget::Org(ticker.clone());
    let menu = s.page_menu.then(|| {
        let mut items = Column::new()
            .push(kit::menu_item(Glyph::Lists, "Add to or take off a list", Some(Msg::PickLists(Some(target.clone()))), false))
            .push(kit::menu_item(Glyph::Flag, format!("Report ({ticker})"), Some(Msg::Reporting(Some(Target::Org(ticker.clone())))), false));
        if moderator(s) {
            let suspend = Msg::ModAct(ModAct::SuspendOrg, Some(Target::Org(ticker.clone())), None);
            items = items.push(kit::menu_item(Glyph::Shield, "Suspend organization (moderator)", Some(suspend), true));
        }
        kit::menu(items, 320.0)
    });
    let mut about = Column::new()
        .spacing(12)
        .push(row![page_name(o.name.as_str()), kit::ticker(&o.ticker, None)].spacing(10).align_y(Alignment::Center));
    if !page.about.is_empty() {
        about = about.push(label(ui::cut(&page.about, 220), BODY, p.text));
    }
    about = about.push(row![total(page.followers, "Follower", "Followers"), total(page.members.len() as u64, "Member", "Members")].spacing(20));
    let join = match page.your_state.as_deref() {
        None => small("Ask to join", Some(Msg::Join(ticker.clone()))),
        Some("requested") => small("Withdraw my request", Some(Msg::Leave(ticker.clone()))),
        Some("invited") => small("Accept the invitation", Some(Msg::Join(ticker.clone()))),
        Some(_) => small("Leave", Some(Msg::Leave(ticker.clone()))),
    };
    let mut tools = Row::new().spacing(8).align_y(Alignment::Center).push(join);
    if page.you_admin() {
        tools = tools
            .push(small("Members", Some(Msg::Go(Screen::Members(ticker.clone())))))
            .push(small("Flagged posts", Some(Msg::Go(Screen::Flagged(ticker.clone())))))
            .push(small("Experience to confirm", Some(Msg::Go(Screen::OrgExperience(ticker.clone())))));
    }
    about = about.push(tools.wrap().vertical_spacing(8));
    if let Some(picker) = list_picker(s, &target) {
        about = about.push(container(column![picker, small("Done", Some(Msg::PickLists(None)))].spacing(8)).padding(14).width(Length::Fill).style(look::frame(p)));
    }
    if page.you_admin() {
        about = about.push(photo_controls(
            s,
            [(PhotoFor::Org(ticker.clone(), "logo"), "logo", o.logo.is_some()), (PhotoFor::Org(ticker.clone(), "banner"), "banner", page.banner.is_some())],
        ));
    }
    if s.reporting == Some(Target::Org(ticker.clone())) {
        about = about.push(report_reasons(Target::Org(ticker.clone())));
    }
    let picture = logo(s, o.logo.as_ref(), &o.name, PICTURE);
    let mut col = column![
        page_top(s, page.banner.as_ref(), &o.name, picture, page_actions(s, follow, menu), (PICTURE * 0.22).round()),
        container(about).padding([12.0, 16.0]).width(Length::Fill),
        kit::tabs(&OrgTab::ALL, s.org_tab, |t| t.title().to_string(), Msg::OrgTab),
    ];
    col = col.push(match s.org_tab {
        OrgTab::Posts => posts_list(s, &page.posts.posts, page.posts.more, "No posts yet."),
        OrgTab::Products => match s.org_products.as_ref().filter(|(of, _)| *of == ticker) {
            Some((_, list)) if list.products.is_empty() => kit::empty("No products yet", "The products this organization owns show here."),
            Some((_, list)) => {
                let mut rows = Column::new();
                for summary in &list.products {
                    rows = rows.push(product_row(s, summary));
                }
                rows.into()
            }
            None => space().into(),
        },
        OrgTab::Jobs => match s.org_jobs.as_ref().filter(|(of, _)| *of == ticker) {
            Some((_, jobs)) => posts_list(s, &jobs.posts, false, "No open jobs."),
            None => space().into(),
        },
        OrgTab::People => {
            let mut people = Column::new();
            if page.members.is_empty() {
                people = people.push(kit::empty("Nobody here yet", "Nobody shows this organization on their page yet."));
            }
            for m in &page.members {
                people = people.push(person_row(s, m));
            }
            people.into()
        }
        OrgTab::About => {
            let mut facts = Column::new();
            facts = facts.push(if page.about.is_empty() { kit::note("No description yet.") } else { label(page.about.as_str(), BODY, p.text).into() });
            facts = facts.push(kit::labeled("Ticker", kit::ticker(&o.ticker, None)));
            let since = full_time(&page.created_at).get(..10).unwrap_or_default().to_string();
            facts = facts.push(kit::labeled("On Pages since", label(since, BODY, p.text)));
            kit::section("About", facts)
        }
    });
    col.into()
}

// ------------------------------------------------------------------- the professional profile

/// Experience, education and skills as a page shows them; another person's skills carry Endorse. None when empty.
fn profile_view<'a>(s: &'a State, page: &'a PersonPage, profile: &'a Profile) -> Option<El<'a>> {
    if profile.experience.is_empty() && profile.education.is_empty() && profile.skills.is_empty() {
        return None;
    }
    let mut col = Column::new().spacing(14);
    if !profile.experience.is_empty() {
        col = col.push(kit::section("Experience", profile.experience.iter().fold(Column::new(), |c, e| c.push(experience_row(e)))));
    }
    if !profile.education.is_empty() {
        col = col.push(kit::section("Education", profile.education.iter().fold(Column::new(), |c, e| c.push(education_row(e)))));
    }
    if !profile.skills.is_empty() {
        let endorser = page.person.id != me_id(s) && s.me.as_ref().is_some_and(|m| m.me.has_page) && !page.you_block;
        let mut list = Column::new();
        for skill in &profile.skills {
            let mut line = row![label(skill.skill.as_str(), 15.0, pal().text), label(endorsement_words(skill.endorsements), 13.0, pal().dim)]
                .spacing(10)
                .align_y(Alignment::Center);
            if endorser && let Some(u) = &page.person.username {
                let endorsed = skill.you_endorsed == Some(true);
                line = line
                    .push(space().width(Length::Fill))
                    .push(small(if endorsed { "Endorsed" } else { "Endorse" }, Some(Msg::Endorse(u.clone(), skill.skill.clone(), !endorsed))));
            }
            list = list.push(line);
        }
        col = col.push(kit::section("Skills", list));
    }
    Some(col.into())
}

/// A position: its title, where (the organization as a link, or the employer as written), when, a confirmation's mark.
fn experience_row(e: &Experience) -> El<'_> {
    let mut at = Row::new().spacing(8).align_y(Alignment::Center);
    match &e.org {
        Some(o) => at = at.push(org_button(o)),
        None if !e.org_name.is_empty() => at = at.push(label(e.org_name.as_str(), 14.0, pal().dim)),
        None => {}
    }
    at = at.push(label(e.span(), 13.0, pal().dim));
    if e.confirmed {
        at = at.push(label("\u{2713} confirmed by the organization", 13.0, pal().accent));
    }
    let mut col = column![strong(e.title.as_str(), 15.0, pal().text), at.wrap()].spacing(2);
    if !e.about.is_empty() {
        col = col.push(label(e.about.as_str(), 14.0, pal().dim));
    }
    col.into()
}

fn education_row(e: &Education) -> El<'_> {
    let years = match (&e.started, &e.ended) {
        (Some(a), Some(b)) => format!("{a} to {b}"),
        (Some(a), None) => format!("from {a}"),
        (None, Some(b)) => b.clone(),
        (None, None) => String::new(),
    };
    let line: Vec<&str> = [e.degree.as_str(), e.field.as_str(), years.as_str()].into_iter().filter(|w| !w.is_empty()).collect();
    column![strong(e.school.as_str(), 15.0, pal().text), label(line.join(" \u{00B7} "), 13.5, pal().dim)].spacing(2).into()
}

fn endorsement_words(n: u64) -> String {
    match n {
        0 => String::new(),
        1 => "1 endorsement".into(),
        n => format!("{n} endorsements"),
    }
}

/// A one-line text field of My page's profile forms.
fn field<'a>(hint: &'a str, value: &'a str, on: impl Fn(String) -> Msg + 'a) -> El<'a> {
    text_input(hint, value).on_input(on).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal())).into()
}

/// My page's experience, education and skills, as LinkedIn keeps a profile: each entry with Edit and Delete (pressed
/// twice), Add opening the form where the list ends, every field named, a bad month or year said before anything is
/// sent; the skills as chips, each with how many endorsed it and its x.
fn profile_editor<'a>(s: &'a State, me: &'a Me) -> El<'a> {
    let f = &s.profile_form;
    let p = pal();
    let delete = |id: &str| if f.confirm_delete.as_deref() == Some(id) { "Delete: yes" } else { "Delete" };
    let framed = |form: Column<'a, Msg>| -> El<'a> { container(form.spacing(8)).padding(12).width(Length::Fill).style(look::frame(pal())).into() };
    let mut exp = Column::new().spacing(6);
    if me.profile.experience.is_empty() {
        exp = exp.push(kit::note("No position yet. An organization on Alelyon named by its ticker can confirm one."));
    }
    for e in &me.profile.experience {
        exp = exp.push(
            row![experience_row(e), space().width(Length::Fill), small("Edit", Some(Msg::ExpEdit(e.clone()))), small(delete(&e.id), Some(Msg::DeleteExperience(e.id.clone())))]
                .spacing(6)
                .align_y(Alignment::Center),
        );
    }
    let x = &f.experience;
    if f.exp_open || !x.id.is_empty() {
        let problem = super::position_problem(x, f.exp_now);
        let to: El<'a> = if f.exp_now {
            text_input("Now", "").padding([10.0, 12.0]).size(15).width(150).style(look::field(pal())).into()
        } else {
            text_input("YYYY-MM", &x.ended).on_input(|t| Msg::ExpField(ExpField::Ended, t)).padding([10.0, 12.0]).size(15).width(150).style(look::field(pal())).into()
        };
        let mut form = Column::new()
            .push(kit::part_title(if x.id.is_empty() { "Add a position" } else { "Change this position" }))
            .push(kit::labeled("Title", field("Founder", &x.title, |t| Msg::ExpField(ExpField::Title, t))))
            .push(
                row![
                    kit::labeled("At (TICKER)", field("An Alelyon organization's ticker", &x.ticker, |t| Msg::ExpField(ExpField::Ticker, t))),
                    kit::labeled("Or employer", field("An employer's name, as written", &x.org_name, |t| Msg::ExpField(ExpField::OrgName, t))),
                ]
                .spacing(12)
                .wrap(),
            )
            .push(
                row![
                    kit::labeled("From", text_input("YYYY-MM", &x.started).on_input(|t| Msg::ExpField(ExpField::Started, t)).padding([10.0, 12.0]).size(15).width(150).style(look::field(pal()))),
                    kit::labeled("To", to),
                    checkbox(f.exp_now).label("I hold it now").on_toggle(Msg::ExpNow).style(look::tick(pal())),
                ]
                .spacing(12)
                .align_y(Alignment::End)
                .wrap(),
            )
            .push(kit::labeled("About", field("What you did (optional)", &x.about, |t| Msg::ExpField(ExpField::About, t))));
        if let Some(words) = problem.filter(|_| !x.title.trim().is_empty() || !x.started.trim().is_empty()) {
            form = form.push(kit::notice(words, p.caution));
        }
        form = form
            .push(
                row![
                    kit::pill(if x.id.is_empty() { "Add the position" } else { "Save position" }, problem.is_none().then_some(Msg::SaveExperience), Pill::Solid),
                    small("Cancel", Some(Msg::ProfileClear)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .push(kit::note("An organization's owners or admins can confirm a position that names it by ticker; a change clears that."));
        exp = exp.push(framed(form));
    } else {
        exp = exp.push(kit::pill("Add a position", Some(Msg::EntryOpen(true)), Pill::Outline));
    }
    let mut edu = Column::new().spacing(6);
    if me.profile.education.is_empty() {
        edu = edu.push(kit::note("No school yet."));
    }
    for e in &me.profile.education {
        edu = edu.push(
            row![education_row(e), space().width(Length::Fill), small("Edit", Some(Msg::EduEdit(e.clone()))), small(delete(&e.id), Some(Msg::DeleteEducation(e.id.clone())))]
                .spacing(6)
                .align_y(Alignment::Center),
        );
    }
    let y = &f.education;
    if f.edu_open || !y.id.is_empty() {
        let problem = super::school_problem(y);
        let year = |hint: &'a str, value: &'a str, which: EduField| -> El<'a> {
            text_input(hint, value).on_input(move |t| Msg::EduField(which, t)).padding([10.0, 12.0]).size(15).width(110).style(look::field(pal())).into()
        };
        let mut form = Column::new()
            .push(kit::part_title(if y.id.is_empty() { "Add a school" } else { "Change this school" }))
            .push(kit::labeled("School", field("University of London", &y.school, |t| Msg::EduField(EduField::School, t))))
            .push(
                row![
                    kit::labeled("Degree", field("BSc (optional)", &y.degree, |t| Msg::EduField(EduField::Degree, t))),
                    kit::labeled("Field", field("Mathematics (optional)", &y.field, |t| Msg::EduField(EduField::Field, t))),
                ]
                .spacing(12)
                .wrap(),
            )
            .push(row![kit::labeled("From", year("YYYY", &y.started, EduField::Started)), kit::labeled("To", year("YYYY", &y.ended, EduField::Ended))].spacing(12));
        if let Some(words) = problem.filter(|_| !y.school.trim().is_empty()) {
            form = form.push(kit::notice(words, p.caution));
        }
        form = form.push(
            row![
                kit::pill(if y.id.is_empty() { "Add the school" } else { "Save school" }, problem.is_none().then_some(Msg::SaveEducation), Pill::Solid),
                small("Cancel", Some(Msg::ProfileClear)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        edu = edu.push(framed(form));
    } else {
        edu = edu.push(kit::pill("Add a school", Some(Msg::EntryOpen(false)), Pill::Outline));
    }
    // The skills as chips: each with how many endorsed it, and its x.
    let mut chips = Row::new().spacing(6).align_y(Alignment::Center);
    for (at, k) in f.skills.iter().enumerate() {
        let n = me.profile.skills.iter().find(|s| s.skill.to_lowercase() == k.to_lowercase()).map_or(0, |s| s.endorsements);
        let words = if n > 0 { format!("{k} \u{00B7} {n}") } else { k.clone() };
        let x = kit::tip(button(label("\u{00D7}", 14.0, p.dim)).padding([0.0, 6.0]).on_press(Msg::SkillRemove(at)).style(look::tool(p, false)), format!("Remove {k}"));
        chips = chips.push(
            container(row![label(words, 14.0, p.text), x].spacing(2).align_y(Alignment::Center))
                .padding(iced::Padding { top: 2.0, right: 2.0, bottom: 2.0, left: 10.0 })
                .style(look::chip(p, p.line)),
        );
    }
    let mut skills = Column::new().spacing(8);
    skills = skills.push(if f.skills.is_empty() { kit::note("No skills yet.") } else { chips.wrap().into() });
    skills = skills
        .push(
            row![
                text_input("Add a skill, then Enter", &f.skill_new).on_input(Msg::SkillNew).on_submit(Msg::SkillAdd).padding([10.0, 12.0]).size(15).width(320).style(look::field(p)),
                small("Add", (!f.skill_new.trim().is_empty()).then_some(Msg::SkillAdd)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .push(
            row![kit::pill("Save skills", f.skills_dirty.then_some(Msg::SaveSkills), Pill::Solid), kit::note("Up to 50. Others can endorse each; only how many is shown.")]
                .spacing(10)
                .align_y(Alignment::Center),
        );
    column![kit::section("Experience", exp), kit::section("Education", edu), kit::section("Skills", skills)].spacing(14).into()
}

/// For an organization's owners and admins: the positions that name it, each with Confirm or Take back.
fn org_experience(s: &State) -> El<'_> {
    let Some((ticker, list)) = &s.org_experience else { return space().into() };
    let mut col = Column::new();
    if list.is_empty() && !s.loading {
        col = col.push(kit::padded(kit::note("Nobody lists a position here yet.")));
    }
    for (person, e) in list {
        let act = if e.confirmed {
            small("Take back", Some(Msg::ConfirmExperience(ticker.clone(), e.id.clone(), false)))
        } else {
            kit::pill("Confirm", Some(Msg::ConfirmExperience(ticker.clone(), e.id.clone(), true)), Pill::Solid)
        };
        col = col.push(kit::block(column![name_line(person), experience_row(e), act].spacing(6)));
    }
    col.into()
}

// ------------------------------------------------------------------- an organization's page

// ------------------------------------------------------------------- a post's words, each @username a link

/// The pieces of a post's text, each with the username it mentions: an `@` not inside a word or an email address,
/// then 2-39 letters, digits, `.`, `_` or `-`, a trailing `.`, `_` or `-` read as punctuation (as the service reads
/// mentions when it notifies).
pub(super) fn mention_pieces(text: &str) -> Vec<(&str, Option<&str>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let before = text[..i].chars().next_back();
            let free = before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '@' || c == '.'));
            let rest = &text[i + 1..];
            let run = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')).take(39).count();
            let name = rest[..run].trim_end_matches(['.', '_', '-']);
            if free && name.len() >= 2 {
                if start < i {
                    out.push((&text[start..i], None));
                }
                out.push((&text[i..i + 1 + name.len()], Some(name)));
                i += 1 + name.len();
                start = i;
                continue;
            }
        }
        i += 1;
    }
    if start < text.len() {
        out.push((&text[start..], None));
    }
    out
}

/// A post's words cut into plain text and hashtags, as the service reads a hashtag: `#` and 1-50 letters, digits or
/// `_` with a letter, not glued to a word, `#`, `&` or `/` before it. Each tag comes with its lower-case name.
pub(super) fn tag_pieces(text: &str) -> Vec<(&str, Option<String>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let before = text[..i].chars().next_back();
            let free = before.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '#' | '&' | '/')));
            let run = bytes[i + 1..].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
            let tag = &text[i + 1..i + 1 + run];
            if free && (1..=50).contains(&run) && tag.bytes().any(|b| b.is_ascii_alphabetic()) {
                if start < i {
                    out.push((&text[start..i], None));
                }
                out.push((&text[i..i + 1 + run], Some(tag.to_ascii_lowercase())));
                i += 1 + run;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    if start < text.len() {
        out.push((&text[start..], None));
    }
    out
}

/// A post's words: each @username a link to that person's page and each #hashtag a link to its posts; what a line of
/// ``` opens (and another closes) is code, set as written in the mono face on a panel.
fn post_text<'a>(text: &'a str, size: f32) -> El<'a> {
    let pieces = code_pieces(text);
    if let [(words, false)] = pieces[..] {
        return prose(words, size);
    }
    let mut col = Column::new().spacing(8);
    for (piece, code) in pieces {
        col = col.push(if code { code_block(piece, size) } else { prose(piece, size) });
    }
    col.into()
}

/// A post's text cut at its fences, the lines that begin with ``` (a language may follow): each piece, and whether it
/// is code. A fence left open runs to the end; the blank lines round a block are left out with it.
pub(super) fn code_pieces(text: &str) -> Vec<(&str, bool)> {
    if !text.contains("```") {
        return vec![(text, false)];
    }
    /// A block's lines as written, without the line end before its closing fence; prose without the blank lines round
    /// it, and none when it is blank.
    fn piece(words: &str, code: bool) -> Option<(&str, bool)> {
        if code {
            Some((words.strip_suffix('\n').unwrap_or(words), true))
        } else {
            let words = words.trim_matches('\n');
            (!words.trim().is_empty()).then_some((words, false))
        }
    }
    let mut out = Vec::new();
    let (mut start, mut at, mut code) = (0, 0, false);
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            out.extend(piece(&text[start..at], code));
            code = !code;
            start = at + line.len();
        }
        at += line.len();
    }
    out.extend(piece(&text[start..], code));
    out
}

/// Code in a post: its lines as written, in the mono face, on a panel.
fn code_block<'a>(code: &'a str, size: f32) -> El<'a> {
    let p = pal();
    container(text(code).size(size - 1.5).font(fonts().mono).color(p.text)).padding([10.0, 12.0]).width(Length::Fill).style(look::card(p)).into()
}

/// Words of a post, its mentions and hashtags as links.
fn prose<'a>(text: &'a str, size: f32) -> El<'a> {
    let mut spans: Vec<iced::widget::text::Span<'a, String, iced::Font>> = Vec::new();
    for (piece, name) in mention_pieces(text) {
        match name {
            Some(user) => spans.push(span(piece).color(pal().accent).link(user.to_lowercase())),
            None => {
                for (words, tag) in tag_pieces(piece) {
                    spans.push(match tag {
                        Some(t) => span(words).color(pal().accent).link(format!("#{t}")),
                        None => span(words).color(pal().text),
                    });
                }
            }
        }
    }
    rich_text(spans)
        .size(size)
        .font(fonts().ui)
        .on_link_click(|link: String| match link.strip_prefix('#') {
            Some(tag) => Msg::Go(Screen::Tag(tag.to_string())),
            None => Msg::Go(Screen::Person(link)),
        })
        .into()
}

// ------------------------------------------------------------------- notifications

/// Notifications as X lists them: each row its kind's mark, the doers' pictures, what they did in words (their names
/// in bold) and when, and the post it was about; unread rows washed in the accent.
fn notifications(s: &State) -> El<'_> {
    let Some(page) = &s.notifications else { return space().into() };
    let mut col = Column::new();
    let groups = super::grouped(page.items.iter().filter(|n| s.notif_view.shows(&n.kind)));
    if groups.is_empty() && !s.loading {
        col = col.push(kit::empty(
            "Nothing to see here yet",
            match s.notif_view {
                NotifView::All => {
                    "When someone likes, reposts, quotes or replies to your posts, mentions you, follows you, or asks about \
                     one of your organizations, it shows here."
                }
                NotifView::Mentions => "Replies to you, posts that mention you and quotes of your posts show here.",
            },
        ));
    }
    for group in groups {
        col = col.push(notification_row(s, group));
    }
    if page.more {
        col = col.push(more_row(Msg::More, "Show more"));
    }
    col.into()
}

/// The icon and colour of a kind of notification, as the social sites mark theirs: a like's heart filled in its
/// colour, a repost's arrows in theirs, the rest in the accent.
fn kind_mark(kind: &str) -> (Glyph, Color, bool) {
    let p = pal();
    match kind {
        "like" => (Glyph::Heart, p.like, true),
        "repost" => (Glyph::Repost, p.repost, false),
        "follow" | "endorsed" => (Glyph::Person, p.accent, false),
        "reply" => (Glyph::Reply, p.accent, false),
        "quote" => (Glyph::Pen, p.accent, false),
        "mention" => (Glyph::Bell, p.accent, false),
        "job-application" => (Glyph::Jobs, p.accent, false),
        "community-request" | "community-joined" => (Glyph::Communities, p.accent, false),
        "product-launched" | "tester-invited" => (Glyph::Rocket, p.accent, false),
        "maker-invited" | "tester-request" => (Glyph::Person, p.accent, false),
        "product-feedback" | "feedback-status" => (Glyph::Up, p.accent, false),
        "product-review" => (Glyph::Star, p.accent, true),
        _ => (Glyph::Building, p.accent, false),
    }
}

/// Who a row's words begin with: the doers ("Ada Lovelace and 3 others"), or the organization that acted.
fn group_lead(group: &[&Notification]) -> Option<String> {
    let first = group[0];
    let name = |n: &Notification| n.actor.as_ref().map_or_else(|| "Someone".to_string(), |a| a.name().to_string());
    Some(match group.len() {
        1 if matches!(first.kind.as_str(), "title-confirmed" | "experience-confirmed") => {
            first.org.as_ref().map(|o| format!("{} ({})", o.name, o.ticker))?
        }
        1 => name(first),
        2 => format!("{} and {}", name(first), name(group[1])),
        n => format!("{} and {} others", name(first), n - 1),
    })
}

/// What a row of notifications says, its doers in bold as X sets them, then when.
fn group_line<'a>(group: &[&'a Notification], at: &str) -> El<'a> {
    let p = pal();
    let words = group_words(group);
    let (bold, rest) = match group_lead(group).filter(|lead| words.starts_with(lead.as_str())) {
        Some(lead) => {
            let rest = words[lead.len()..].to_string();
            (lead, rest)
        }
        None => (String::new(), words),
    };
    let spans: Vec<iced::widget::text::Span<'a, (), iced::Font>> = vec![
        span(bold).font(fonts().ui_strong).color(p.text),
        span(rest).color(p.text),
        span(" \u{00B7} ").color(p.dim),
        span(short_when(at)).color(p.dim).font(fonts().mono).size(13.0),
    ];
    rich_text(spans).size(BODY).font(fonts().ui).into()
}

/// What a row says: one notification's own sentence, or "Ada Lovelace and 3 others liked your post".
fn group_words(group: &[&Notification]) -> String {
    let first = group[0];
    if group.len() == 1 {
        return notification_words(first);
    }
    let name = |n: &Notification| n.actor.as_ref().map_or_else(|| "Someone".to_string(), |a| a.name().to_string());
    let who = if group.len() == 2 {
        format!("{} and {}", name(first), name(group[1]))
    } else {
        format!("{} and {} others", name(first), group.len() - 1)
    };
    let plain = |n: &&Notification| n.detail.as_deref().and_then(Reaction::from_word).is_none_or(|r| r == Reaction::Like);
    match first.kind.as_str() {
        "like" if group.iter().all(plain) => format!("{who} liked your post"),
        "like" => format!("{who} reacted to your post"),
        "repost" => format!("{who} reposted your post"),
        "follow" => format!("{who} followed you"),
        _ => notification_words(first),
    }
}

/// Where a notification leads: the post; a request to the members; the organization; the person who followed.
fn notification_target(n: &Notification) -> Option<Msg> {
    match (n.kind.as_str(), &n.post, &n.org) {
        ("community-request", _, _) => n.community.as_ref().map(|c| Msg::Go(Screen::CommunityMembers(c.handle.clone()))),
        ("community-joined", _, _) => n.community.as_ref().map(|c| Msg::Go(Screen::Community(c.handle.clone()))),
        ("maker-invited" | "tester-request" | "tester-invited", _, _) => n.product.as_ref().map(|p| Msg::Go(Screen::Product(p.handle.clone()))),
        ("job-application", Some(p), _) => Some(Msg::Go(Screen::Applicants(p.id.clone()))),
        (_, Some(p), _) => Some(Msg::Go(Screen::Thread(p.id.clone()))),
        ("org-request", None, Some(o)) => Some(Msg::Go(Screen::Members(o.ticker.clone()))),
        (_, None, Some(o)) => Some(Msg::Go(Screen::Org(o.ticker.clone()))),
        _ => n.actor.as_ref().and_then(|a| a.username.clone()).map(|u| Msg::Go(Screen::Person(u))),
    }
}

/// A row of notifications: the kind's mark; the doers' pictures; what they did and when; and the post it was about,
/// framed when it answers or names the reader, else a line of it. Unread rows are washed in the accent.
fn notification_row<'a>(s: &'a State, group: Vec<&'a Notification>) -> El<'a> {
    let p = pal();
    let n = group[0];
    let (glyph, color, filled) = kind_mark(&n.kind);
    let mut faces = Row::new().spacing(6);
    let mut seen: Vec<&str> = Vec::new();
    for m in &group {
        if let Some(a) = &m.actor
            && !seen.contains(&a.id.as_str())
        {
            seen.push(a.id.as_str());
            if seen.len() <= 8 {
                faces = faces.push(avatar(s, a.avatar.as_ref(), a.name(), 32.0));
            }
        }
    }
    let mut col = Column::new().spacing(8);
    if !seen.is_empty() {
        col = col.push(faces);
    }
    col = col.push(group_line(&group, &n.at));
    if let Some(post) = &n.post {
        if matches!(n.kind.as_str(), "reply" | "mention" | "quote") {
            col = col.push(quoted(s, post));
        } else if !post.text.is_empty() {
            col = col.push(label(ui::cut(&post.text, 160), BODY, p.dim));
        }
    }
    let unread = group.iter().any(|m| !m.read);
    let mark = container(icons::icon(glyph, 28.0, color, filled)).width(Length::Fixed(40.0)).align_x(Alignment::End);
    let row_ = button(row![mark, col.width(Length::Fill)].spacing(12)).padding([12.0, 16.0]).width(Length::Fill).on_press_maybe(notification_target(n)).style(look::row(p));
    let shown: El<'a> = if unread { container(row_).style(look::washed(p, p.accent)).into() } else { row_.into() };
    column![shown, kit::hairline()].into()
}

/// What a notification says, in a sentence.
fn notification_words(n: &Notification) -> String {
    let who = n.actor.as_ref().map(|a| a.name()).unwrap_or("Someone");
    let org = n.org.as_ref().map(|o| format!("{} ({})", o.name, o.ticker)).unwrap_or_else(|| "an organization".into());
    match n.kind.as_str() {
        "like" => match n.detail.as_deref().and_then(Reaction::from_word) {
            Some(Reaction::Celebrate) => format!("{who} celebrated your post"),
            Some(Reaction::Support) => format!("{who} supported your post"),
            Some(Reaction::Love) => format!("{who} loved your post"),
            Some(Reaction::Insightful) => format!("{who} found your post insightful"),
            Some(Reaction::Funny) => format!("{who} found your post funny"),
            _ => format!("{who} liked your post"),
        },
        "reply" => format!("{who} replied to your post"),
        "repost" => format!("{who} reposted your post"),
        "quote" => format!("{who} quoted your post"),
        "mention" => format!("{who} mentioned you"),
        "follow" => format!("{who} followed you"),
        "org-invite" => format!("{who} invited you to join {org}"),
        "org-request" => format!("{who} asked to join {org}"),
        "org-joined" => format!("{who} accepted you into {org}"),
        "title-confirmed" => format!("{org} confirmed your title"),
        "endorsed" => format!("{who} endorsed your skill {}", n.detail.as_deref().unwrap_or("")),
        "experience-confirmed" => format!("{org} confirmed your position as {}", n.detail.as_deref().unwrap_or("")),
        "job-application" => format!("{who} applied for a job at {org}"),
        "community-request" => format!("{who} asked to join {}", community_words(n.community.as_ref())),
        "community-joined" => format!("{who} accepted you into {}", community_words(n.community.as_ref())),
        "maker-invited" => format!("{who} listed you as a maker of {}", product_words(n)),
        "product-launched" => match n.post.as_ref().and_then(|p| p.launch.as_ref()) {
            Some(launch) => format!("{who} launched {} {}", product_words(n), launch.version),
            None => format!("{who} launched {}", product_words(n)),
        },
        "product-feedback" => format!("{who} posted feedback on {}", product_words(n)),
        "feedback-status" => match n.post.as_ref().and_then(|p| p.feedback.as_ref()) {
            Some(f) => format!("{who} marked feedback on {} as {}", product_words(n), f.status.label().to_lowercase()),
            None => format!("{who} changed the status of feedback on {}", product_words(n)),
        },
        "product-review" => match n.post.as_ref().and_then(|p| p.stars) {
            Some(stars) => format!("{who} reviewed {}: {stars} of 5 stars", product_words(n)),
            None => format!("{who} reviewed {}", product_words(n)),
        },
        "tester-request" => format!("{who} asked to test {}", product_words(n)),
        "tester-invited" => format!("{who} invited you to test {}", product_words(n)),
        _ => format!("{who} did something that concerns you"),
    }
}

/// The product a notification is about, by its name.
fn product_words(n: &Notification) -> String {
    n.product.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "a product".into())
}

/// A community in a sentence: `c/handle`.
fn community_words(c: Option<&Community>) -> String {
    c.map(|c| format!("c/{}", c.handle)).unwrap_or_else(|| "a community".into())
}

// ------------------------------------------------------------------- an organization's flagged posts (owners and admins)

/// What its owners and admins read above the list.
const FLAGGED: &str = "Posts made as this organization whose author's account no longer exists, or whose author's page a \
    moderator suspended. Everyone else sees them as the organization's, without an author. Keep each one, or delete it.";

fn flagged(s: &State) -> El<'_> {
    let Some((_, page)) = &s.flagged else { return space().into() };
    column![kit::note(FLAGGED), posts_list(s, &page.posts, page.more, "No posts by a gone author.")].spacing(10).into()
}

// ------------------------------------------------------------------- a thread

/// A conversation as X shows one: what it answers, the post itself (`focused`), a reply box with the reader's
/// picture, then the replies.
fn thread(s: &State) -> El<'_> {
    let Some(t) = &s.thread else { return space().into() };
    let p = pal();
    let mut col = Column::new();
    if let Some(parent) = &t.parent {
        col = col.push(post_card(s, parent));
    }
    col = col.push(focused(s, &t.post));
    let me = s.me.as_ref();
    let has_page = me.is_some_and(|m| m.me.has_page);
    if has_page && !t.post.you_can_reply {
        col = col.push(kit::block(kit::note(match t.post.replies_from {
            ReplyRule::Followers => "Only followers of whoever started the conversation can reply.",
            _ => "Only the people its first post mentions can reply.",
        })));
    } else if has_page {
        // Follows are private on Pages; a reply under this rule tells others one.
        if t.post.replies_from == ReplyRule::Followers {
            col = col.push(kit::block(kit::note("Followers only: a reply here shows that its writer follows whoever started the conversation.")));
        }
        let face = avatar(s, me.and_then(|m| m.me.avatar.as_ref()), me.map_or("", |m| m.me.name()), 40.0);
        let field = text_input("Post your reply", &s.reply)
            .id(keys::REPLY)
            .on_input(Msg::ReplyText)
            .on_submit(Msg::SendReply)
            .padding([8.0, 0.0])
            .size(18)
            .font(fonts().ui)
            .style(look::bare_field(p))
            .width(Length::Fill);
        let send = kit::primary("Reply", (!s.reply.trim().is_empty() && !s.composer.sending).then_some(Msg::SendReply));
        let send = if s.reply.trim().is_empty() { kit::tip(send, "Write a reply first") } else { send };
        col = col.push(kit::block(row![face, field, send].spacing(12).align_y(Alignment::Center)));
    } else {
        col = col.push(kit::block(kit::note("Open your page to reply: My page, in the navigation.")));
    }
    for r in &t.replies.posts {
        col = col.push(post_card(s, r));
    }
    col.into()
}

/// The post a conversation is about, as X draws it: its author's picture and name over it; its words larger and
/// across the column; its full time; its totals between hairlines; then its actions.
fn focused<'a>(s: &'a State, post: &'a Post) -> El<'a> {
    let p = pal();
    let mine = post.author.as_ref().is_some_and(|a| a.id == me_id(s));
    let admin = post.org.as_ref().is_some_and(|o| admin_of(s, &o.ticker));
    let (url, who) = post_picture(post);
    let face = if post.org.is_some() { logo(s, url, who, 44.0) } else { avatar(s, url, who, 44.0) };
    let name: El<'a> = match (&post.org, &post.author) {
        (Some(o), Some(a)) => column![org_button(o), label(format!("by {}", a.name()), BODY, p.dim)].spacing(1).into(),
        (Some(o), None) => org_button(o),
        (None, Some(a)) => name_line(a),
        (None, None) => kit::note("Posted by someone no longer here."),
    };
    let mut col = Column::new().spacing(12).push(row![face, container(name).width(Length::Fill), more_menu(s, post)].spacing(12).align_y(Alignment::Center));
    if post.org.is_some()
        && let Some(flag) = post.author_flag
    {
        col = col.push(label(flag.words(), SMALL, p.caution));
    }
    if post.deleted {
        return kit::block(col.push(kit::note("Deleted.")));
    }
    col = col.push(post_body(s, post, 17.0, mine, admin));
    let mut when = full_time(&post.created_at);
    if post.edited_at.is_some() {
        when.push_str(" \u{00B7} edited");
    }
    let mut stamp = Row::new().spacing(5).align_y(Alignment::Center).push(kit::figure(when, 13.5, p.dim));
    if let Some(c) = &post.community {
        stamp = stamp.push(label("in", BODY, p.dim)).push(community_link(c));
    }
    col = col.push(stamp);
    let totals = [(post.reposts, "Repost", "Reposts"), (post.likes, "Like", "Likes"), (post.replies, "Reply", "Replies")];
    let mut sums = Row::new().spacing(20);
    let mut any = false;
    for (n, one, many) in totals.into_iter().filter(|(n, _, _)| *n > 0) {
        sums = sums.push(total(n, one, many));
        any = true;
    }
    if any {
        col = col.push(kit::hairline()).push(sums);
    }
    col = col.push(kit::hairline()).push(action_bar(s, post));
    col = open_under(s, post, col);
    kit::block(col)
}

/// A number and what it counts, as the social sites write a post's or a page's totals: the number in bold.
fn total<'a>(n: u64, one: &'a str, many: &'a str) -> El<'a> {
    let p = pal();
    row![kit::figure(compact(n), 14.5, p.text), label(if n == 1 { one } else { many }, 14.0, p.dim)].spacing(5).align_y(Alignment::Center).into()
}

/// A count as X writes one: "1,284" below ten thousand, then "12.5K" and "1.2M".
pub(super) fn compact(n: u64) -> String {
    let short = |value: f64, unit: &str| {
        let tenths = format!("{value:.1}");
        format!("{}{unit}", tenths.strip_suffix(".0").unwrap_or(&tenths))
    };
    match n {
        0..=9_999 => {
            let digits = n.to_string();
            match digits.len() {
                4 => format!("{},{}", &digits[..1], &digits[1..]),
                _ => digits,
            }
        }
        10_000..=999_999 => short((n / 100) as f64 / 10.0, "K"),
        _ => short((n / 100_000) as f64 / 10.0, "M"),
    }
}

// ------------------------------------------------------------------- my page

fn mine(s: &State) -> El<'_> {
    let Some(me) = &s.me else { return space().into() };
    if me.suspended {
        return kit::notice(SUSPENDED, pal().caution);
    }
    if !me.me.has_page {
        return kit::block(
            column![
                strong("Your page", 16.0, pal().text),
                label(
                    "A page is public: once it is open, anyone can read your name, title, biography and posts, signed in or \
                     not. Your email address is never shown. You can close it at any time.",
                    13.5,
                    pal().dim,
                ),
                kit::primary("Open my page", Some(Msg::OpenPage)),
            ]
            .spacing(10),
        );
    }
    let m = &s.mine;
    // The page in four tabs, as a profile's settings are laid out, so each is a screen's height rather than one long
    // column; the way to see the page as others do stays above them whichever is chosen.
    let applied = s.applications.as_ref().map_or(0, |a| a.len());
    let mut page = Column::new();
    if let Some(u) = &me.me.username {
        let see = kit::outline("See my page as others do", Some(Msg::Go(Screen::Person(u.clone()))));
        page = page.push(container(row![space().width(Length::Fill), see]).padding([8.0, 16.0]));
    }
    let tabs = kit::tabs(&MineTab::ALL, m.tab, |t: MineTab| match t {
        MineTab::Profile => "Profile".to_string(),
        MineTab::Background => "Background".to_string(),
        MineTab::Privacy => "Privacy".to_string(),
        MineTab::Applications if applied > 0 => format!("Applications ({applied})"),
        MineTab::Applications => "Applications".to_string(),
    }, Msg::MineTab);
    let body = match m.tab {
        MineTab::Profile => column![mine_profile(s, me), tickers_card(me.active().collect(), &me.me.tickers)].into(),
        MineTab::Background => profile_editor(s, me),
        MineTab::Privacy => mine_privacy(s, me),
        MineTab::Applications => mine_applications(s),
    };
    page.push(tabs).push(body).into()
}

/// A small "?" in Pages' colours that says `words` under the pointer: a rule read once, kept beside what it governs
/// rather than standing under it (UX-PRINCIPLES B2).
fn rule_help<'a>(words: impl text::IntoFragment<'a>) -> El<'a> {
    kit::tip(container(label("?", 11.5, pal().dim)).padding([0.0, 6.0]).style(look::chip(pal(), pal().dim)), words)
}

/// How much of a field is used, in the caution colour once it is full.
fn used<'a>(text: &str, max: usize) -> El<'a> {
    let n = text.chars().count();
    label(format!("{n}/{max}"), SMALL, if n >= max { pal().caution } else { pal().dim }).into()
}

/// A field's name with its rule on a "?" beside it and how much of it is used at the right, then the field.
fn labeled_count<'a>(name: &'a str, rule: impl text::IntoFragment<'a>, count: El<'a>, field: impl Into<El<'a>>) -> El<'a> {
    let head = row![label(name, SMALL, pal().dim), rule_help(rule), space().width(Length::Fill), count].spacing(6).align_y(Alignment::Center).width(320);
    column![head, field.into()].spacing(6).into()
}

/// Ctrl+Enter in the biography saves My page, as Enter does in its one-line fields.
fn save_keys(press: text_editor::KeyPress) -> Option<text_editor::Binding<Msg>> {
    use iced::keyboard::{Key, key::Named};
    if press.modifiers.command() && matches!(press.key.as_ref(), Key::Named(Named::Enter)) {
        return Some(text_editor::Binding::Custom(Msg::SaveMine));
    }
    text_editor::Binding::from_key_press(press)
}

/// The Profile tab: the top of the reader's page as others see it, drawn from the fields as they change, the photos
/// changed on it; then the name, title, the organization the title names and the biography, saved together (Enter in
/// a field, Ctrl+Enter in the biography). What is typed stays until it is saved or discarded.
fn mine_profile<'a>(s: &'a State, me: &'a Me) -> El<'a> {
    let m = &s.mine;
    let p = pal();
    let tidy = social::tidy_display_name(&m.name);
    let shown = if tidy.is_empty() { me.me.name().to_string() } else { tidy.clone() };
    let title = super::tidy_title(&m.title);
    let same_title = title == me.me.title && m.title_org.as_ref().map(|o| o.ticker.as_str()) == me.me.title_org.as_ref().map(|o| o.ticker.as_str());
    // The top as others will see it once saved: the photos changed where they show, taken away only when there is one.
    let banner = m.top.as_ref().and_then(|t| t.banner.as_ref());
    let tools = row![
        photo_controls(s, [(PhotoFor::Page("avatar"), "photo", me.me.avatar.is_some()), (PhotoFor::Page("banner"), "banner", banner.is_some())]),
        rule_help(PHOTO_RULE),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let picture = avatar(s, me.me.avatar.as_ref(), &shown, PICTURE);
    let mut about = Column::new().spacing(12).push(column![page_name(&shown), kit::figure(me.me.handle(), 13.0, p.dim)].spacing(4));
    if !title.is_empty() || !me.me.tickers.is_empty() {
        let mut line = Row::new().spacing(6).align_y(Alignment::Center);
        if !title.is_empty() {
            line = line.push(label(title.clone(), BODY, p.text));
        }
        if !title.is_empty() && same_title && me.me.title_verified {
            line = line.push(kit::tip(label("\u{2713}", BODY, p.accent), "Title confirmed by the organization it names"));
        }
        for t in &me.me.tickers {
            line = line.push(kit::ticker(&t.ticker, None));
        }
        about = about.push(line.wrap());
    }
    if let (false, Some(org)) = (title.is_empty(), m.title_org.as_ref()) {
        about = about.push(kit::note(if same_title && me.me.title_verified {
            format!("\u{2713} {} ({}) confirmed this title.", org.name, org.ticker)
        } else {
            let after = if me.me.title_verified { ": saving takes the \u{2713} away until they confirm it again" } else { "" };
            format!("This title names {} ({}), which has not confirmed it{after}.", org.name, org.ticker)
        }));
    }
    let bio = m.bio.text();
    if !bio.trim().is_empty() {
        about = about.push(label(bio.trim().to_string(), BODY, p.text));
    }
    if let Some(t) = &m.top {
        about = about.push(row![total(t.following, "Following", "Following"), total(t.followers, "Follower", "Followers")].spacing(20));
    }
    let top = column![page_top(s, banner, &shown, picture, tools.into(), PICTURE / 2.0), container(about).padding([12.0, 16.0]).width(Length::Fill), kit::hairline()];
    // The fields, each with how much of it is used.
    let mut orgs: Vec<TitleOrg> = vec![TitleOrg(None)];
    orgs.extend(me.active().map(|ms| TitleOrg(Some(ms.org.clone()))));
    let name_ok = tidy == me.me.display_name || social::valid_display_name(&m.name);
    let title_rule = super::title_problem(&m.title);
    let can_save = m.dirty && !m.saving && name_ok && title_rule.is_none();
    let field = |hint: &'a str, value: &'a str, on: fn(String) -> Msg| {
        text_input(hint, value).on_input(on).on_submit(Msg::SaveMine).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal()))
    };
    let mut card = Column::new().spacing(10).push(
        row![
            labeled_count("Name", NAME_RULE, used(&m.name, 64), field("Your name", &m.name, Msg::MineName)),
            labeled_count(
                "Title",
                format!(
                    "Up to 4 parts separated by |, 30 characters each and {} in all, without brackets. An organization's owners or \
                     admins can confirm a title that names it.",
                    pages::TITLE_MAX_CHARS
                ),
                used(&title, pages::TITLE_MAX_CHARS),
                field("Founder | CEO", &m.title, Msg::MineTitle),
            ),
        ]
        .spacing(16)
        .wrap(),
    );
    if !name_ok {
        card = card.push(kit::notice(NAME_RULE, p.caution));
    }
    if let Some(words) = title_rule {
        card = card.push(kit::notice(words, p.caution));
    }
    let mut actions = row![kit::primary(if m.saving { "Saving\u{2026}" } else { "Save" }, can_save.then_some(Msg::SaveMine))].spacing(12).align_y(Alignment::Center);
    if m.dirty && !m.saving {
        actions = actions.push(small("Discard changes", Some(Msg::DiscardMine))).push(label("Not saved yet: Enter in a field, or Ctrl+Enter in About you, saves.", SMALL, p.dim));
    }
    card = card
        .push(kit::labeled(
            "Title is at",
            pick_list(orgs, Some(TitleOrg(m.title_org.clone())), Msg::MineTitleOrg)
                .padding(6)
                .text_size(14.0)
                .font(fonts().ui)
                .style(look::picker(pal()))
                .menu_style(look::picker_menu(pal())),
        ))
        .push(row![kit::part_title("About you"), space().width(Length::Fill), used(&bio, 1_000)].align_y(Alignment::Center).width(Length::Fill))
        .push(
            text_editor(&m.bio)
                .placeholder("A few words about you")
                .on_action(Msg::MineBio)
                .key_binding(save_keys)
                .height(Length::Fixed(110.0))
                .padding(10)
                .size(13.5)
                .font(fonts().ui)
                .style(look::editor(pal())),
        )
        .push(actions);
    column![top, kit::block(card)].into()
}

/// The Privacy tab: whether public posts may train AI, the people muted, and closing the page.
fn mine_privacy<'a>(s: &'a State, me: &'a Me) -> El<'a> {
    let mut col = column![kit::block(
        column![
            row![kit::part_title("Your posts and AI"), rule_help(AI_TRAINING)].spacing(8).align_y(Alignment::Center),
            checkbox(me.ai_training).label("Let Alelyon use my public posts to improve its services, including training AI models").on_toggle(Msg::AiTraining).style(look::tick(pal())),
        ]
        .spacing(8),
    )];
    let muted = s.muted.as_ref().filter(|m| !m.is_empty());
    let list = muted.into_iter().flatten().fold(Column::new(), |c, p| {
        c.push(
            row![name_line(p), space().width(Length::Fill), small("Unmute", p.username.clone().map(|u| Msg::Mute(u, false)))]
                .align_y(Alignment::Center),
        )
    });
    let head = row![
        kit::note(if muted.is_some() { "Their posts leave your feeds and their acts your notifications." } else { "Nobody is muted." }),
        rule_help("Their posts leave your feeds and their acts your notifications. They are not told."),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    col = col.push(kit::section("Muted", Column::new().push(head).push(list)));
    // The page itself, closed only from here, after a second press: closing erases the title and biography.
    let mut page = column![
        kit::part_title("Your page"),
        kit::note(
            "Open: anyone can read your name, title, the organizations after your name, your biography and your posts, \
             signed in or not. Your email address is never shown."
        ),
        small(if s.mine.confirm_close { "Close it: yes" } else { "Close my page" }, Some(Msg::ClosePage)),
    ]
    .spacing(8);
    if s.mine.confirm_close {
        page = page.push(kit::note("Closing hides your page and posts and erases your title and biography. Press again to close it."));
    }
    col.push(kit::block(page)).into()
}

/// The Applications tab: the jobs applied for, each with a way to withdraw.
fn mine_applications(s: &State) -> El<'_> {
    let applied = s.applications.as_ref().filter(|a| !a.is_empty());
    let list = applied.into_iter().flatten().fold(Column::new().spacing(6), |c, p| {
        let at = p.org.as_ref().map(|o| o.name.as_str()).unwrap_or("");
        let state = if p.job.as_ref().is_some_and(|j| j.closed) { "closed" } else { "open" };
        c.push(
            row![
                link(format!("{} \u{00B7} {at} ({state})", p.title.as_deref().unwrap_or("A job")), 13.0, Msg::Go(Screen::Thread(p.id.clone()))),
                space().width(Length::Fill),
                small("Withdraw", Some(Msg::Withdraw(p.id.clone()))),
            ]
            .align_y(Alignment::Center),
        )
    });
    let words = "Each organization's owners and admins see your page and your note. An application is deleted when you \
                 withdraw it, or 180 days after its job closes.";
    let head = row![
        kit::note(if applied.is_some() { "Who sees them, and how long they are kept:" } else { "You have not applied for a job." }),
        rule_help(words),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    kit::section("Your applications", Column::new().push(head).push(list))
}

/// What the form says about a name: what it holds, and where a title goes instead.
const NAME_RULE: &str = "Your name only: 1-64 characters, without [ ] ( ). Your title goes below, and the organizations \
    after your name further down.";

/// What the form says about the AI-training choice.
const AI_TRAINING: &str = "On unless you turn it off. Turning it off keeps your public posts out of anything Alelyon builds \
    from then on; what was built before stays as it is.";

/// The organizations after the reader's name: each membership with whether it shows, and the order of those that do.
fn tickers_card<'a>(active: Vec<&'a Membership>, shown: &'a [Org]) -> El<'a> {
    let mut col = Column::new().spacing(8).push(
        row![
            kit::part_title("Organizations after your name"),
            rule_help("Show none, one or several. Those on show read in this order after your name."),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if active.is_empty() {
        col = col.push(kit::note("You are not a member of an organization yet: see Organizations, in the navigation."));
    }
    for (i, o) in shown.iter().enumerate() {
        col = col.push(
            row![
                checkbox(true)
                    .label(format!("{} ({})", o.name, o.ticker))
                    .on_toggle({
                        let t = o.ticker.clone();
                        move |on| Msg::ShowTicker(t.clone(), on)
                    })
                    .style(look::tick(pal())),
                space().width(Length::Fill),
                small("\u{2191}", (i > 0).then(|| Msg::MoveTicker(o.ticker.clone(), true))),
                small("\u{2193}", (i + 1 < shown.len()).then(|| Msg::MoveTicker(o.ticker.clone(), false))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    for m in active.into_iter().filter(|m| !shown.iter().any(|o| o.ticker == m.org.ticker)) {
        let t = m.org.ticker.clone();
        col = col.push(checkbox(false).label(format!("{} ({})", m.org.name, m.org.ticker)).on_toggle(move |on| Msg::ShowTicker(t.clone(), on)).style(look::tick(pal())));
    }
    kit::block(col)
}

// ------------------------------------------------------------------- organizations

fn orgs(s: &State) -> El<'_> {
    let Some(me) = &s.me else { return space().into() };
    let mut list = Column::new().spacing(10).push(kit::part_title("Yours"));
    if me.memberships.is_empty() {
        list = list.push(kit::note("None yet. Find one by its ticker in search, or start one below."));
    }
    for m in &me.memberships {
        let t = m.org.ticker.clone();
        let mut r = row![
            logo(s, m.org.logo.as_ref(), &m.org.name, 36.0),
            org_button(&m.org),
            kit::chip(role_words(m), if m.state == "active" { pal().accent } else { pal().dim }),
            space().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        r = match m.state.as_str() {
            "invited" => r.push(small("Accept", Some(Msg::Join(t.clone())))).push(small("Decline", Some(Msg::Leave(t)))),
            "requested" => r.push(small("Withdraw", Some(Msg::Leave(t)))),
            _ if m.role == "owner" || m.role == "admin" => r.push(small("Members", Some(Msg::Go(Screen::Members(t))))),
            _ => r,
        };
        list = list.push(r);
    }
    let n = &s.new_org;
    let create = column![
        kit::part_title("Start an organization"),
        kit::labeled("Ticker", text_input("LACE", &n.ticker).on_input(Msg::NewTicker).padding([10.0, 12.0]).size(15).width(120).style(look::field(pal()))),
        kit::labeled("Name", text_input("Lace Labs", &n.name).on_input(Msg::NewName).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal()))),
        kit::labeled("About", text_input("What it does", &n.about).on_input(Msg::NewAbout).padding([10.0, 12.0]).size(15).style(look::field(pal()))),
        kit::note("A ticker is 2-5 letters and unique. You become its owner, and it shows after your name until you hide it."),
        kit::primary("Start it", (n.ticker.len() >= 2 && !n.name.trim().is_empty()).then_some(Msg::CreateOrg)),
    ]
    .spacing(10);
    column![
        kit::block(list),
        kit::block(create),
    ]
    .into()
}

fn role_words(m: &Membership) -> String {
    match m.state.as_str() {
        "invited" => "Invited".into(),
        "requested" => "Asked to join".into(),
        _ => {
            let mut r = m.role.clone();
            if let Some(first) = r.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            r
        }
    }
}

fn members(s: &State) -> El<'_> {
    // the list is kept only for the screen on show (`Msg::Members`)
    let Some((ticker, list)) = &s.members else { return space().into() };
    let ticker = ticker.as_str();
    let mut col = Column::new().push(kit::padded(
        row![
            text_input("Invite by username", &s.invite)
                .on_input(Msg::InviteText)
                .on_submit(Msg::Invite(ticker.to_string(), s.invite.clone()))
                .padding([10.0, 12.0])
                .size(15)
                .width(260)
                .style(look::field(pal())),
            kit::outline("Invite", (!s.invite.trim().is_empty()).then(|| Msg::Invite(ticker.to_string(), s.invite.clone()))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ));
    let mine = me_id(s);
    let my_role = list.iter().find(|m| m.person.id == mine).map(|m| m.role.as_str()).unwrap_or("member");
    for state in ["requested", "invited", "active"] {
        let rows: Vec<&Member> = list.iter().filter(|m| m.state == state).collect();
        if rows.is_empty() {
            continue;
        }
        col = col.push(kit::column_title(match state {
            "requested" => "Asking to join",
            "invited" => "Invited",
            _ => "Members",
        }));
        for m in rows {
            col = col.push(member_row(ticker, m, mine, my_role));
        }
    }
    col.into()
}

fn member_row<'a>(ticker: &'a str, m: &'a Member, mine: &'a str, my_role: &'a str) -> El<'a> {
    let t = || ticker.to_string();
    let id = m.person.id.clone();
    let mut info = Column::new().spacing(4).push(name_line(&m.person));
    let mut acts = Row::new().spacing(4).align_y(Alignment::Center);
    match m.state.as_str() {
        "requested" => {
            if let Some(u) = &m.person.username {
                acts = acts.push(small("Approve", Some(Msg::Invite(t(), u.clone()))));
            }
            acts = acts.push(small("Refuse", Some(Msg::Remove(t(), id))));
        }
        "invited" => acts = acts.push(small("Withdraw", Some(Msg::Remove(t(), id)))),
        _ => {
            info = info.push(label(m.role.clone(), 12.5, pal().dim));
            let names_us = m.person.title_org.as_ref().is_some_and(|o| o.ticker == ticker) && !m.person.title.is_empty();
            if m.person.title_verified && names_us {
                acts = acts.push(small("Withdraw title confirmation", Some(Msg::Confirm(t(), id.clone(), false))));
            } else if names_us {
                acts = acts.push(small(format!("Confirm \u{201C}{}\u{201D}", m.person.title), Some(Msg::Confirm(t(), id.clone(), true))));
            }
            if my_role == "owner" && m.person.id != mine {
                for role in ["owner", "admin", "member"].into_iter().filter(|r| *r != m.role) {
                    acts = acts.push(small(format!("Make {role}"), Some(Msg::Role(t(), id.clone(), role))));
                }
            }
            let removable = m.person.id != mine && m.role != "owner" && (m.role == "member" || my_role == "owner");
            if removable {
                acts = acts.push(small("Remove", Some(Msg::Remove(t(), id))));
            }
        }
    }
    kit::block(row![info, space().width(Length::Fill), acts.wrap()].spacing(10).align_y(Alignment::Center))
}

// ------------------------------------------------------------------- communities (ADR-0053)

fn community_link<'a>(c: &'a Community) -> El<'a> {
    button(label(format!("c/{}", c.handle), 13.0, pal().accent))
        .padding(0)
        .on_press(Msg::Go(Screen::Community(c.handle.clone())))
        .style(look::bare(pal()))
        .into()
}

fn members_words(n: u64) -> String {
    format!("{} {}", compact(n), if n == 1 { "member" } else { "members" })
}

fn community_role_title(role: &str) -> &'static str {
    match role {
        "owner" => "Owner",
        "moderator" => "Moderator",
        _ => "Member",
    }
}

/// What the reader can do about a community: join or ask to, withdraw a request, leave; or why not.
fn community_action<'a>(s: &'a State, c: &'a CommunitySummary) -> El<'a> {
    let handle = c.community.handle.clone();
    let has_page = s.me.as_ref().is_some_and(|m| m.me.has_page && !m.suspended);
    match c.your_state.as_deref() {
        Some("active") => small("Leave", Some(Msg::LeaveCommunity(handle))),
        Some("requested") => small("Withdraw my request", Some(Msg::LeaveCommunity(handle))),
        Some(_) => label("Its moderators removed you", 13.0, pal().caution).into(),
        None => kit::pill(if c.joining == Joining::Request { "Ask to join" } else { "Join" }, has_page.then_some(Msg::JoinCommunity(handle)), Pill::Solid),
    }
}

/// A community in a list: its name and handle, its members, the reader's part in it, and what it is about.
fn community_row<'a>(s: &'a State, c: &'a CommunitySummary) -> El<'a> {
    let handle = c.community.handle.clone();
    let mut head = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(button(strong(c.community.name.as_str(), 15.0, pal().text)).padding(0).on_press(Msg::Go(Screen::Community(handle))).style(look::bare(pal())))
        .push(label(format!("c/{}", c.community.handle), 13.5, pal().accent))
        .push(label(members_words(c.members), 13.0, pal().dim));
    if let Some(role) = c.your_role.as_deref().filter(|r| c.you_belong() && *r != "member") {
        head = head.push(kit::chip(community_role_title(role), pal().accent));
    }
    if c.your_state.as_deref() == Some("requested") {
        head = head.push(kit::chip("Asked to join", pal().dim));
    }
    let mut col = Column::new().spacing(4).push(row![head.wrap(), space().width(Length::Fill), community_action(s, c)].align_y(Alignment::Center));
    if !c.about.is_empty() {
        col = col.push(label(ui::cut(&c.about, 240), 14.0, pal().dim));
    }
    kit::block(col)
}

/// The Communities tab: the reader's own, the others with the most members, and starting one.
fn communities(s: &State) -> El<'_> {
    let mut col = Column::new();
    if let Some(mine) = s.my_communities.as_ref().filter(|m| !m.is_empty()) {
        col = col.push(kit::column_title("Yours"));
        for c in mine {
            col = col.push(community_row(s, c));
        }
    }
    if let Some(list) = &s.communities {
        let others: Vec<&CommunitySummary> = list.iter().filter(|c| c.your_state.is_none()).collect();
        col = col.push(kit::column_title("Communities"));
        if others.is_empty() && !s.loading {
            col = col.push(kit::padded(kit::note("No other community yet. Start one below.")));
        }
        for c in others {
            col = col.push(community_row(s, c));
        }
    }
    if !s.me.as_ref().is_some_and(|m| m.me.has_page) {
        return col.push(kit::notice("Open your page to join or start a community: My page, in the navigation.", pal().accent)).into();
    }
    let n = &s.new_community;
    let create = column![
        kit::part_title("Start a community"),
        kit::labeled(
            "Handle",
            text_input("engines", &n.handle).on_input(|t| Msg::NewCommunityField(CommunityField::Handle, t)).padding([10.0, 12.0]).size(15).width(220).style(look::field(pal()))
        ),
        kit::labeled(
            "Name",
            text_input("Analytical Engines", &n.name).on_input(|t| Msg::NewCommunityField(CommunityField::Name, t)).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal()))
        ),
        kit::labeled("About", text_input("What it is for", &n.about).on_input(|t| Msg::NewCommunityField(CommunityField::About, t)).padding([10.0, 12.0]).size(15).style(look::field(pal()))),
        kit::labeled(
            "Rules",
            text_input("How members treat each other (optional)", &n.rules)
                .on_input(|t| Msg::NewCommunityField(CommunityField::Rules, t))
                .padding([10.0, 12.0])
                .size(15)
                .style(look::field(pal()))
        ),
        kit::labeled(
            "Joining",
            pick_list(Joining::ALL, Some(n.joining), Msg::NewCommunityJoining)
                .padding(6)
                .text_size(14.0)
                .font(fonts().ui)
                .style(look::picker(pal()))
                .menu_style(look::picker_menu(pal()))
        ),
        kit::note(
            "The handle is its address, c/handle: 3-30 lower-case letters, digits or _, beginning with a letter. A community \
             is public: anyone can read it, its members and its posts. You become its owner."
        ),
        kit::primary("Start it", n.ready().then_some(Msg::CreateCommunity)),
    ]
    .spacing(10);
    col.push(kit::block(create)).into()
}

/// The edit form of the community on show, for its owners: its name, about, rules and how it is joined.
fn community_editor(form: &NewCommunity) -> El<'_> {
    column![
        kit::labeled("Name", text_input("Name", &form.name).on_input(|t| Msg::EditCommunityField(CommunityField::Name, t)).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal()))),
        kit::labeled("About", text_input("What it is for", &form.about).on_input(|t| Msg::EditCommunityField(CommunityField::About, t)).padding([10.0, 12.0]).size(15).style(look::field(pal()))),
        kit::labeled("Rules", text_input("Rules (optional)", &form.rules).on_input(|t| Msg::EditCommunityField(CommunityField::Rules, t)).padding([10.0, 12.0]).size(15).style(look::field(pal()))),
        kit::labeled(
            "Joining",
            pick_list(Joining::ALL, Some(form.joining), Msg::EditCommunityJoining)
                .padding(6)
                .text_size(14.0)
                .font(fonts().ui)
                .style(look::picker(pal()))
                .menu_style(look::picker_menu(pal()))
        ),
        row![kit::primary("Save", (!form.name.trim().is_empty()).then_some(Msg::SaveCommunity)), small("Cancel", Some(Msg::EditCommunity(false)))]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .into()
}

/// A community: its name, handle, members, about and rules; joining; its keepers' tools; and its posts, with the
/// composer for its members.
fn community(s: &State) -> El<'_> {
    let Some(page) = &s.community else { return space().into() };
    let sum = &page.summary;
    let c = &sum.community;
    let handle = c.handle.clone();
    let mut head = Column::new()
        .spacing(8)
        .push(row![strong(c.name.as_str(), 20.0, pal().text), label(format!("c/{}", c.handle), 15.0, pal().accent)].spacing(8).align_y(Alignment::Center))
        .push(label(format!("{} \u{00B7} {}", members_words(sum.members), sum.joining.label()), 13.5, pal().dim));
    if !sum.about.is_empty() {
        head = head.push(label(sum.about.as_str(), 15.0, pal().text));
    }
    if !page.rules.is_empty() {
        head = head.push(kit::part_title("Rules")).push(label(page.rules.as_str(), 14.0, pal().dim));
    }
    let members = match page.requests.filter(|n| *n > 0) {
        Some(n) => format!("Members \u{00B7} {n} asking to join"),
        None => "Members".to_string(),
    };
    let mut acts = Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .push(community_action(s, sum))
        .push(small(members, Some(Msg::Go(Screen::CommunityMembers(handle.clone())))));
    if sum.you_own() {
        acts = acts.push(small(if s.edit_community.is_some() { "Close the editor" } else { "Edit" }, Some(Msg::EditCommunity(s.edit_community.is_none()))));
    }
    if moderator(s) {
        acts = acts.push(small("Suspend community (moderator)", Some(Msg::ModCommunity(handle, true))));
    }
    head = head.push(acts.wrap());
    if let Some(form) = &s.edit_community {
        head = head.push(community_editor(form));
    }
    let mut col = column![kit::block(head)];
    if sum.you_belong() && s.me.as_ref().is_some_and(|m| m.me.has_page && !m.suspended) {
        col = col.push(composer(s));
    }
    col.push(posts_list(s, &page.posts.posts, page.posts.more, "No posts yet. Members post here, and only members reply.")).into()
}

/// A community's members: requests first for its keepers, then members with their roles, then bans.
fn community_members(s: &State) -> El<'_> {
    let Some((handle, list)) = &s.community_members else { return space().into() };
    let handle = handle.as_str();
    let mine = s.my_communities.iter().flatten().find(|c| c.community.handle == handle);
    let keeper = mine.is_some_and(CommunitySummary::you_keep);
    let owner = mine.is_some_and(CommunitySummary::you_own);
    let me = me_id(s);
    let mut col = Column::new();
    for (state, title) in [("requested", "Asking to join"), ("active", "Members"), ("banned", "Kept from joining")] {
        let rows: Vec<&Member> = list.iter().filter(|m| m.state == state).collect();
        if rows.is_empty() {
            continue;
        }
        col = col.push(kit::column_title(title));
        for m in rows {
            col = col.push(community_member_row(handle, m, me, keeper, owner));
        }
    }
    if !keeper {
        col = col.push(kit::padded(kit::note("Members are public. Requests and bans are seen only by the community's owners and moderators.")));
    }
    col.into()
}

fn community_member_row<'a>(handle: &'a str, m: &'a Member, me: &'a str, keeper: bool, owner: bool) -> El<'a> {
    let h = || handle.to_string();
    let id = m.person.id.clone();
    let mut info = Column::new().spacing(4).push(name_line(&m.person));
    let mut acts = Row::new().spacing(4).align_y(Alignment::Center);
    match m.state.as_str() {
        "requested" if keeper => {
            acts = acts.push(small("Approve", Some(Msg::DecideRequest(h(), id.clone(), true)))).push(small("Refuse", Some(Msg::DecideRequest(h(), id, false))));
        }
        "banned" if keeper => acts = acts.push(small("Lift the ban", Some(Msg::RemoveFromCommunity(h(), id, false)))),
        "active" => {
            if m.role != "member" {
                info = info.push(label(community_role_title(&m.role), 12.5, pal().dim));
            }
            if owner && m.person.id != me {
                for role in ["owner", "moderator", "member"].into_iter().filter(|r| *r != m.role) {
                    acts = acts.push(small(format!("Make {role}"), Some(Msg::CommunityRole(h(), id.clone(), role))));
                }
            }
            if keeper && m.person.id != me && m.role != "owner" && (m.role == "member" || owner) {
                acts = acts
                    .push(small("Remove", Some(Msg::RemoveFromCommunity(h(), id.clone(), false))))
                    .push(small("Remove and ban", Some(Msg::RemoveFromCommunity(h(), id, true))));
            }
        }
        _ => {}
    }
    kit::block(row![info, space().width(Length::Fill), acts.wrap()].spacing(10).align_y(Alignment::Center))
}

// ------------------------------------------------------------------- lists (ADR-0054)

fn list_counts(l: &PageList) -> String {
    let people = if l.people == 1 { "1 person".to_string() } else { format!("{} people", compact(l.people)) };
    let orgs = if l.orgs == 1 { "1 organization".to_string() } else { format!("{} organizations", compact(l.orgs)) };
    format!("{people} \u{00B7} {orgs}")
}

/// A list in the Lists tab: its name (which opens it), who is on it, and what it is for.
fn list_row(l: &PageList) -> El<'_> {
    let mut col = Column::new().spacing(4).push(
        row![
            button(strong(l.name.as_str(), 15.0, pal().text)).padding(0).on_press(Msg::Go(Screen::List(l.id.clone()))).style(look::bare(pal())),
            label(list_counts(l), 13.0, pal().dim),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if !l.about.is_empty() {
        col = col.push(label(l.about.as_str(), 14.0, pal().dim));
    }
    kit::block(col)
}

/// The Lists tab: the reader's lists, by name, and making one.
fn lists(s: &State) -> El<'_> {
    let mut col = Column::new();
    if let Some(found) = &s.lists {
        col = col.push(kit::column_title("Your lists"));
        if found.is_empty() && !s.loading {
            col = col.push(kit::padded(kit::note("No list yet. Make one below, then add people and organizations from their pages.")));
        }
        for l in found {
            col = col.push(list_row(l));
        }
    }
    let (name, about) = &s.new_list;
    let make = column![
        kit::part_title("Make a list"),
        kit::labeled("Name", text_input("Engine people", name).on_input(Msg::NewListName).on_submit(Msg::CreateList).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal()))),
        kit::labeled(
            "About",
            text_input("What it is for (optional)", about).on_input(Msg::NewListAbout).on_submit(Msg::CreateList).padding([10.0, 12.0]).size(15).style(look::field(pal()))
        ),
        kit::note(
            "A list is yours alone: nobody else sees it, and nobody on it is told. It shows the posts of the people and \
             organizations on it, newest first, whether or not you follow them."
        ),
        kit::primary("Make it", (!name.trim().is_empty()).then_some(Msg::CreateList)),
    ]
    .spacing(10);
    col.push(kit::block(make)).into()
}

/// A list: its name, counts and line; who is on it (when asked for), each with Take off; deleting it; and its stream.
fn list(s: &State) -> El<'_> {
    let Some(page) = &s.list else { return space().into() };
    let l = &page.list;
    let mut head = Column::new().spacing(8).push(strong(l.name.as_str(), 20.0, pal().text)).push(label(list_counts(l), 13.5, pal().dim));
    if !l.about.is_empty() {
        head = head.push(label(l.about.as_str(), 15.0, pal().text));
    }
    let members = s.list_members.as_ref().filter(|(of, _)| *of == l.id).map(|(_, m)| m);
    let editing = s.edit_list.as_ref().filter(|(of, _, _)| *of == l.id);
    let confirming = s.confirm_delete_list.as_deref() == Some(l.id.as_str());
    head = head.push(
        row![
            small(if members.is_some() { "Hide who is on it" } else { "Who is on it" }, Some(Msg::ShowListMembers(members.is_none()))),
            small(if editing.is_some() { "Close the editor" } else { "Edit" }, Some(Msg::EditList(editing.is_none()))),
            small(if confirming { "Delete it: press again" } else { "Delete list" }, Some(Msg::DeleteList(l.id.clone()))),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );
    if let Some((_, name, about)) = editing {
        head = head.push(list_editor(name, about));
    }
    if let Some(m) = members {
        head = head.push(list_members(&l.id, m));
    }
    head = head.push(kit::note("Only you see this list, and nobody on it is told."));
    column![
        kit::block(head),
        posts_list(s, &page.posts.posts, page.posts.more, "No posts yet. The posts of the people and organizations on this list show here."),
    ]
    .into()
}

/// The edit form of the list on show: its name and line.
fn list_editor<'a>(name: &'a str, about: &'a str) -> El<'a> {
    column![
        kit::labeled("Name", text_input("Name", name).on_input(Msg::EditListName).on_submit(Msg::SaveList).padding([10.0, 12.0]).size(15).width(320).style(look::field(pal()))),
        kit::labeled(
            "About",
            text_input("What it is for (optional)", about).on_input(Msg::EditListAbout).on_submit(Msg::SaveList).padding([10.0, 12.0]).size(15).style(look::field(pal()))
        ),
        row![kit::primary("Save", (!name.trim().is_empty()).then_some(Msg::SaveList)), small("Cancel", Some(Msg::EditList(false)))]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .into()
}

/// Who is on a list, people then organizations, each with Take off.
fn list_members<'a>(list_id: &'a str, m: &'a ListMembers) -> El<'a> {
    if m.people.is_empty() && m.orgs.is_empty() {
        return kit::note("Nobody is on it yet. Add people and organizations from their pages.");
    }
    let take_off = |target: Option<ListTarget>| small("Take off", target.map(|t| Msg::ListTakeOff(list_id.to_string(), t)));
    let mut col = Column::new().spacing(6);
    if !m.people.is_empty() {
        col = col.push(kit::part_title("People"));
        for p in &m.people {
            col = col.push(row![name_line(p), space().width(Length::Fill), take_off(p.username.clone().map(ListTarget::Person))].align_y(Alignment::Center));
        }
    }
    if !m.orgs.is_empty() {
        col = col.push(kit::part_title("Organizations"));
        for o in &m.orgs {
            col = col.push(row![org_button(o), space().width(Length::Fill), take_off(Some(ListTarget::Org(o.ticker.clone())))].align_y(Alignment::Center));
        }
    }
    col.into()
}

/// The reader's lists offered for a person or organization, each with a box that puts it on the list or takes it off.
fn list_picker<'a>(s: &'a State, target: &ListTarget) -> Option<El<'a>> {
    let (shown, found) = s.list_picker.as_ref().filter(|(t, _)| t == target)?;
    let mut col = Column::new().spacing(6).push(kit::part_title("Your lists"));
    if found.is_empty() {
        col = col.push(kit::note("You have no list yet. Make one in Lists, in the navigation."));
    }
    for l in found {
        let id = l.id.clone();
        col = col.push(
            checkbox(l.has == Some(true))
                .label(format!("{}  \u{00B7}  {}", l.name, list_counts(l)))
                .on_toggle(move |add| Msg::ListAdd(id.clone(), shown.clone(), add))
                .style(look::tick(pal())),
        );
    }
    Some(col.push(kit::note("Nobody is told when you put them on a list.")).into())
}

// ------------------------------------------------------------------- builders (ADR-0056)

/// The reader keeps the product (owns it, or makes it), as the products they keep say, or its page on show.
fn keeps_product(s: &State, handle: &str) -> bool {
    s.my_products.iter().flatten().any(|p| p.product.handle == handle)
        || s.product.as_ref().is_some_and(|page| page.summary.product.handle == handle && page.you_keep())
}

/// A product named in a line: its small logo and its name in the accent; pressed, its page.
fn product_chip<'a>(s: &'a State, product: &'a pages::Product) -> El<'a> {
    let p = pal();
    let face = row![logo(s, product.logo.as_ref(), &product.name, 18.0), label(product.name.as_str(), 13.5, p.accent)].spacing(6).align_y(Alignment::Center);
    button(face).padding(0).on_press(Msg::Go(Screen::Product(product.handle.clone()))).style(look::bare(p)).into()
}

/// Over the words of a post about a product: what it is to it in the brand's small capitals (UPDATE, LAUNCH,
/// FEEDBACK, REVIEW, or ARTICLE), the product, and a launch's version, feedback's category and status, or a review's
/// stars.
fn product_line<'a>(s: &'a State, post: &'a Post, product: &'a pages::Product) -> El<'a> {
    let p = pal();
    let mut line = Row::new().spacing(8).align_y(Alignment::Center);
    if let Some(kind) = kind_words(post) {
        line = line.push(kit::kind(kind));
    }
    line = line.push(product_chip(s, product));
    if let Some(launch) = &post.launch {
        line = line.push(kit::figure(launch.version.as_str(), 13.0, p.accent));
    }
    if let Some(f) = &post.feedback {
        line = line.push(kit::chip(f.category.label(), p.dim)).push(status_chip(f.status));
    }
    if let Some(n) = post.stars {
        line = line.push(stars(n, 14.0));
    }
    line.wrap().vertical_spacing(4).into()
}

/// Where a piece of feedback stands, in its colour: quiet while open or under review, the accent once its makers
/// plan it or work on it, the repost's green when done, the caution colour when it will not be done.
fn status_chip<'a>(status: FeedbackStatus) -> El<'a> {
    let p = pal();
    let color = match status {
        FeedbackStatus::Open | FeedbackStatus::UnderReview => p.dim,
        FeedbackStatus::Planned | FeedbackStatus::InProgress => p.accent,
        FeedbackStatus::Done => p.repost,
        FeedbackStatus::WontDo => p.caution,
    };
    kit::chip(status.label(), color)
}

/// Where a product is, in its colour: quiet while building, green while it takes testers, the accent once launched.
fn stage_chip<'a>(stage: Stage) -> El<'a> {
    let p = pal();
    kit::chip(
        stage.label(),
        match stage {
            Stage::Building => p.dim,
            Stage::Testing => p.repost,
            Stage::Launched => p.accent,
        },
    )
}

/// A review's stars: `n` of five, filled in the accent.
fn stars<'a>(n: u8, size: f32) -> El<'a> {
    let p = pal();
    let mut r = Row::new().spacing(1).align_y(Alignment::Center);
    for i in 1..=5u8 {
        let on = i <= n;
        r = r.push(icons::icon(Glyph::Star, size, if on { p.accent } else { p.dim }, on));
    }
    kit::tip(r, format!("{n} of 5 stars"))
}

/// The stars of the reader's own review, each a button that sets them.
fn restar<'a>(post: &'a Post, n: u8) -> El<'a> {
    let p = pal();
    let mut r = Row::new().spacing(2).align_y(Alignment::Center).push(label("Your stars", 13.0, p.dim));
    for i in 1..=5u8 {
        let on = i <= n;
        let star = button(icons::icon(Glyph::Star, 18.0, if on { p.accent } else { p.dim }, on))
            .padding(2)
            .on_press_maybe((i != n).then(|| Msg::ReviewStars(post.id.clone(), i)))
            .style(look::bare(p));
        r = r.push(star);
    }
    r.into()
}

/// "12 supporters", "1 vote": a count and its noun.
fn counted(n: u64, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{} {many}", compact(n)) }
}

/// A launch, framed under its words: the version, how many supported it and, while its day lasts, how long is left
/// and Support (not on one's own product).
fn launch_box<'a>(post: &'a Post, launch: &'a pages::Launch, own: bool) -> El<'a> {
    let p = pal();
    let supporters = counted(launch.supports, "supporter", "supporters");
    let left = crate::utc::parse(&launch.ends_at).map(|end| end - crate::utc::now()).filter(|left| launch.live && *left > 0.0);
    let when = match left {
        Some(left) => format!("{supporters} \u{00B7} {} left to support it", until(left)),
        None => format!("{supporters} \u{00B7} launched {}", short_when(&launch.launched_at)),
    };
    let words = column![strong(format!("Launched {}", launch.version), BODY, p.text), label(when, SMALL, p.dim)].spacing(2).width(Length::Fill);
    let mut r = Row::new().spacing(12).align_y(Alignment::Center).push(icons::icon(Glyph::Rocket, 22.0, p.accent, false)).push(words);
    if left.is_some() && !own {
        r = r.push(if launch.you_support {
            kit::pill("Supported", Some(Msg::SupportLaunch(post.id.clone(), false)), Pill::Outline)
        } else {
            kit::pill("Support", Some(Msg::SupportLaunch(post.id.clone(), true)), Pill::Solid)
        });
    }
    container(r).padding([12.0, 14.0]).width(Length::Fill).style(look::frame(p)).into()
}

/// Under a piece of feedback: its votes and the reader's (not on their own), and for the product's keepers the
/// status to set, which tells its author and its voters.
fn feedback_bar<'a>(post: &'a Post, f: &'a pages::Feedback, mine: bool, own: bool) -> El<'a> {
    let p = pal();
    let color = if f.you_voted { p.accent } else { p.text };
    let face = row![icons::icon(Glyph::Up, 16.0, if f.you_voted { p.accent } else { p.dim }, f.you_voted), label(counted(f.votes, "vote", "votes"), 13.5, color)]
        .spacing(6)
        .align_y(Alignment::Center);
    let vote = (!mine).then(|| Msg::VoteFeedback(post.id.clone(), !f.you_voted));
    let tip = if mine {
        "Your own feedback"
    } else if f.you_voted {
        "Take your vote back"
    } else {
        "Vote for it"
    };
    let mut r = Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(kit::tip(button(face).padding([5.0, 12.0]).on_press_maybe(vote).style(look::choice(p, f.you_voted)), tip));
    if own {
        let id = post.id.clone();
        let status = kit::picked(pick_list(FeedbackStatus::ALL, Some(f.status), move |st| Msg::SetFeedbackStatus(id.clone(), st)));
        r = r.push(space().width(Length::Fill)).push(label("Status", 13.0, p.dim)).push(status);
    }
    r.into()
}

/// The day's first launches, as a card beside the feed: each one's place, the product's logo, name and version and
/// how many supported it; pressed, the product. Show more opens Products.
fn launches_card(s: &State) -> Option<El<'_>> {
    let launches = s.launches_today.as_ref()?;
    let p = pal();
    let mut rows = Vec::new();
    for (i, post) in launches.iter().filter(|post| post.product.is_some() && post.launch.is_some()).take(3).enumerate() {
        let (Some(product), Some(launch)) = (&post.product, &post.launch) else { continue };
        let words = column![
            strong(product.name.as_str(), BODY, p.text),
            label(format!("{} \u{00B7} {}", launch.version, counted(launch.supports, "supporter", "supporters")), SMALL, p.dim),
        ]
        .spacing(2)
        .width(Length::Fill);
        let line = row![container(kit::figure((i + 1).to_string(), 14.0, p.accent)).width(Length::Fixed(14.0)), logo(s, product.logo.as_ref(), &product.name, 36.0), words]
            .spacing(12)
            .align_y(Alignment::Center);
        rows.push(button(line).padding([10.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Product(product.handle.clone()))).style(look::card_row(p)).into());
    }
    if rows.is_empty() {
        return None;
    }
    Some(kit::card("Launching today", rows, (launches.len() > 3).then_some(Msg::Go(Screen::Products))))
}

/// A launch in the day's or the week's ranking: its place in the mono face; the product's logo, name and version,
/// the launch's first line and who launched it; how many supported it, which is Support while its day lasts.
/// Pressed, the launch.
fn launch_row<'a>(s: &'a State, place: usize, post: &'a Post) -> El<'a> {
    let p = pal();
    let (Some(product), Some(launch)) = (&post.product, &post.launch) else { return post_card(s, post) };
    let first = post.title.as_deref().unwrap_or_else(|| post.text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default());
    let mut words = Column::new()
        .spacing(3)
        .width(Length::Fill)
        .push(row![strong(product.name.as_str(), 16.0, p.text), kit::figure(launch.version.as_str(), 13.0, p.accent)].spacing(8).align_y(Alignment::Center));
    if !first.is_empty() {
        words = words.push(label(ui::cut(first, 140), 14.0, p.text));
    }
    let (url, who) = post_picture(post);
    words = words.push(row![avatar(s, url, who, 18.0), label(format!("{who} \u{00B7} {}", short_when(&post.created_at)), SMALL, p.dim)].spacing(6).align_y(Alignment::Center));
    let own = keeps_product(s, &product.handle);
    let lit = launch.you_support;
    let count = column![icons::icon(Glyph::Rocket, 18.0, if lit { p.accent } else { p.dim }, false), kit::figure(compact(launch.supports), 13.0, if lit { p.accent } else { p.text })]
        .spacing(3)
        .align_x(Alignment::Center);
    let can = launch.live && !own;
    let tip = match (own, launch.live, lit) {
        (true, _, _) => "Your own product",
        (false, false, _) => "Its day is over",
        (false, true, true) => "Take your support back",
        (false, true, false) => "Support this launch",
    };
    let support = kit::tip(
        button(container(count).center_x(Length::Fill)).padding([8.0, 0.0]).width(Length::Fixed(60.0)).on_press_maybe(can.then(|| Msg::SupportLaunch(post.id.clone(), !lit))).style(look::choice(p, lit)),
        tip,
    );
    let line = row![
        container(kit::figure(place.to_string(), 18.0, if place <= 3 { p.accent } else { p.dim })).width(Length::Fixed(26.0)),
        logo(s, product.logo.as_ref(), &product.name, 52.0),
        words,
        support,
    ]
    .spacing(14)
    .align_y(Alignment::Center);
    column![button(line).padding([14.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Thread(post.id.clone()))).style(look::row(p)), kit::hairline()].into()
}

/// What a list says of a product in one line: its address, its owner, what it is built with, its followers and its
/// stars.
fn product_facts(summary: &ProductSummary) -> String {
    let mut parts = vec![format!("p/{}", summary.product.handle)];
    match (&summary.org, &summary.owner) {
        (Some(o), _) => parts.push(format!("by {} ({})", o.name, o.ticker)),
        (None, Some(a)) => parts.push(format!("by {}", a.name())),
        (None, None) => {}
    }
    if !summary.tech.is_empty() {
        parts.push(summary.tech.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
    }
    parts.push(counted(summary.followers, "follower", "followers"));
    if let Some(rating) = summary.rating {
        parts.push(format!("{rating:.1} of 5 from {}", counted(summary.reviews, "review", "reviews")));
    }
    parts.join(" \u{00B7} ")
}

/// A product as a row of the column: its logo; its name, stage and tagline; its facts; Follow, or Yours. Pressed,
/// its page.
fn product_row<'a>(s: &'a State, summary: &'a ProductSummary) -> El<'a> {
    let p = pal();
    let product = &summary.product;
    let head = row![strong(product.name.as_str(), 16.0, p.text), stage_chip(summary.stage)].spacing(8).align_y(Alignment::Center);
    let mut words = Column::new().spacing(4).width(Length::Fill).push(head);
    if !summary.tagline.is_empty() {
        words = words.push(label(summary.tagline.as_str(), 14.5, p.text));
    }
    words = words.push(label(product_facts(summary), 13.0, p.dim));
    let act: El<'a> = if summary.you_keep {
        kit::chip("Yours", p.accent)
    } else if summary.you_follow {
        kit::following(Msg::FollowProduct(product.handle.clone(), false))
    } else {
        kit::pill("Follow", Some(Msg::FollowProduct(product.handle.clone(), true)), Pill::Solid)
    };
    let line = row![logo(s, product.logo.as_ref(), &product.name, 48.0), words, act].spacing(14).align_y(Alignment::Center);
    column![button(line).padding([14.0, 16.0]).width(Length::Fill).on_press(Msg::Go(Screen::Product(product.handle.clone()))).style(look::row(p)), kit::hairline()].into()
}

/// The Products tab: the day's or the week's launches, the most supported first; the newest products; the reader's
/// own, and starting one; those they follow.
fn products(s: &State) -> El<'_> {
    let mut col = Column::new();
    match s.products_view {
        view @ (ProductsView::Today | ProductsView::Week) => {
            let week = view == ProductsView::Week;
            let shown = if week { s.launches_week.as_ref() } else { s.launches_today.as_ref() };
            match shown {
                Some(list) if list.is_empty() => {
                    col = col.push(kit::empty(
                        if week { "Nothing launched this week" } else { "Nothing launched today" },
                        "A product's makers launch a version from the composer, About the product. It is ranked here by how many \
                         supported it in its day.",
                    ));
                }
                Some(list) => {
                    col = col.push(kit::padded(kit::note(if week {
                        "This week's launches, the most supported first."
                    } else {
                        "Today's launches, the most supported first. Each can be supported for a day."
                    })));
                    col = col.push(kit::hairline());
                    for (i, post) in list.iter().enumerate() {
                        col = col.push(launch_row(s, i + 1, post));
                    }
                }
                None if s.loading => col = col.push(skeleton()),
                None => {}
            }
        }
        view => {
            match s.product_list.as_ref().filter(|(shown, _)| *shown == view).map(|(_, list)| list) {
                Some(list) if list.products.is_empty() => {
                    col = col.push(match view {
                        ProductsView::Yours => kit::empty(
                            "Nothing of yours yet",
                            "Start a product's page below: what it is, what it runs on and what it is built with. Its makers \
                             post its updates and launches there.",
                        ),
                        ProductsView::Following => kit::empty(
                            "You follow no product yet",
                            "Follow one from its page: its updates and launches then show in your Following feed.",
                        ),
                        _ => kit::empty("No products yet", "Start the first in Yours."),
                    });
                }
                Some(list) => {
                    for summary in &list.products {
                        col = col.push(product_row(s, summary));
                    }
                    if list.more {
                        col = col.push(more_row(Msg::More, "Show more"));
                    }
                }
                None if s.loading => col = col.push(skeleton()),
                None => {}
            }
            if view == ProductsView::Yours {
                col = col.push(product_starter(s));
            }
        }
    }
    col.into()
}

/// What starting a product says under its form.
const PRODUCT_RULE: &str = "The handle is its address, p/handle: 3-40 lower-case letters, digits or -, beginning and ending \
    with a letter or a digit. A product's page is public. You are its first maker, and list the others from its page. \
    Each person or organization owns three products for now.";

/// Starting a product: its handle, then what its page says (`product_fields`), and who owns it: the reader, or an
/// organization they run.
fn product_starter(s: &State) -> El<'_> {
    if !s.me.as_ref().is_some_and(|m| m.me.has_page && !m.suspended) {
        return kit::padded(kit::notice("Open your page to start a product: My page, in the navigation.", pal().accent));
    }
    let form = &s.new_product;
    let mut col = Column::new()
        .spacing(10)
        .push(kit::part_title("Start a product"))
        .push(kit::labeled("Handle", kit::field("query-planner", &form.handle, |t| Msg::NewProductField(ProductField::Handle, t)).width(260)))
        .push(product_fields(form, false));
    let admin = s.me.as_ref().map(|m| m.admin_of()).unwrap_or_default();
    if !admin.is_empty() {
        let mut owners = vec![AsWho(None)];
        owners.extend(admin.into_iter().map(|t| AsWho(Some(t))));
        col = col.push(kit::labeled("Owned by", kit::picked(pick_list(owners, Some(AsWho(form.as_org.clone())), Msg::NewProductOwner)).width(240)));
    }
    col = col.push(kit::note(PRODUCT_RULE));
    let why = form.why_not(true);
    let start = kit::primary("Start it", why.is_none().then_some(Msg::CreateProduct));
    col = col.push(match why {
        Some(why) => kit::tip(start, why),
        None => start,
    });
    kit::block(col)
}

/// What a product's page says, as its form writes it: its name, tagline, about, links, what it runs on, what it is
/// built with and its stage; for starting it, or (`editing`) for its keepers to change it.
fn product_fields(form: &ProductForm, editing: bool) -> El<'_> {
    let p = pal();
    let on = move |field: ProductField| move |t: String| if editing { Msg::EditProductField(field, t) } else { Msg::NewProductField(field, t) };
    let mut platforms = Row::new().spacing(6);
    for platform in pages::PLATFORMS {
        let chosen = form.platforms.contains(&platform);
        let msg = if editing { Msg::EditProductPlatform(platform, !chosen) } else { Msg::NewProductPlatform(platform, !chosen) };
        platforms = platforms
            .push(button(text(pages::platform_label(platform)).size(13.5).font(fonts().ui)).padding([5.0, 12.0]).on_press(msg).style(look::choice(p, chosen)));
    }
    let stage = if editing {
        pick_list(Stage::ALL, Some(form.stage), Msg::EditProductStage)
    } else {
        pick_list(Stage::ALL, Some(form.stage), Msg::NewProductStage)
    };
    column![
        kit::labeled("Name", kit::field("Query Planner", &form.name, on(ProductField::Name)).width(320)),
        kit::labeled("Tagline", kit::field("What it does, in a line", &form.tagline, on(ProductField::Tagline))),
        kit::labeled("About", kit::field("What it is, who it is for, what is next (optional)", &form.about, on(ProductField::About))),
        row![
            kit::labeled("Website", kit::field("https://\u{2026} (optional)", &form.website, on(ProductField::Website))),
            kit::labeled("Source", kit::field("https://\u{2026} (optional)", &form.source, on(ProductField::Source))),
        ]
        .spacing(10),
        kit::labeled("Built with", kit::field("Rust, Postgres, WebAssembly: comma between (optional)", &form.tech, on(ProductField::Tech))),
        kit::labeled("Runs on", platforms.wrap().vertical_spacing(6)),
        kit::labeled("Stage", kit::picked(stage).width(200)),
        kit::note("Testing takes asks to test it; a launch makes it Launched."),
    ]
    .spacing(10)
    .into()
}

/// The edit form of the product on show, for its keepers.
fn product_editor(form: &ProductForm) -> El<'_> {
    let why = form.why_not(false);
    let save = kit::primary("Save", why.is_none().then_some(Msg::SaveProduct));
    column![
        product_fields(form, true),
        row![
            match why {
                Some(why) => kit::tip(save, why),
                None => save,
            },
            small("Cancel", Some(Msg::EditProduct(false)))
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(10)
    .into()
}

/// A screenshot, `w` by `h`: drawn once it is held, a grey panel until then.
fn shot<'a>(s: &'a State, url: &str, w: f32, h: f32) -> El<'a> {
    match s.photos.get(url) {
        Some(Picture::Ready(handle)) => image(handle.clone()).width(w).height(h).content_fit(ContentFit::Cover).border_radius(12.0).into(),
        _ => container(space()).width(w).height(h).style(look::bone(pal(), 12.0)).into(),
    }
}

/// A choice among a few, as a small round button washed when chosen.
fn choice<'a>(words: &'a str, on: bool, msg: Msg) -> El<'a> {
    button(text(words).size(13.5).font(fonts().ui)).padding([5.0, 12.0]).on_press(msg).style(look::choice(pal(), on)).into()
}

/// A product's page: its logo, name, stage and tagline; who owns it and its links; its followers and stars;
/// following it and its keepers' tools; a maker's invitation; asking to test it; its latest launch while its day
/// lasts; what it runs on and is built with; its screenshots and makers; then its updates, launches, feedback board
/// and reviews.
fn product(s: &State) -> El<'_> {
    let Some(page) = &s.product else { return space().into() };
    let p = pal();
    let sum = &page.summary;
    let pr = &sum.product;
    let handle = pr.handle.clone();
    let keeper = page.you_keep();
    let has_page = s.me.as_ref().is_some_and(|m| m.me.has_page && !m.suspended);
    // the top: its logo beside its name, stage, tagline and owner
    let title = row![text(pr.name.as_str()).size(26.0).font(fonts().display).color(p.text), stage_chip(sum.stage)].spacing(10).align_y(Alignment::Center);
    let mut words = Column::new().spacing(6).width(Length::Fill).push(title);
    if !sum.tagline.is_empty() {
        words = words.push(label(sum.tagline.as_str(), 16.0, p.text));
    }
    let mut by = Row::new().spacing(6).align_y(Alignment::Center).push(label("by", 14.0, p.dim));
    by = match (&sum.org, &sum.owner) {
        (Some(o), _) => by.push(org_button(o)),
        (None, Some(a)) => by.push(kit::strong_link(a.name(), 14.0, p.text, a.username.clone().filter(|_| a.has_page).map(|u| Msg::Go(Screen::Person(u))))),
        (None, None) => by.push(kit::note("someone no longer here")),
    };
    words = words.push(by);
    let mut head = Column::new().spacing(14).push(row![logo(s, pr.logo.as_ref(), &pr.name, 80.0), words].spacing(16).align_y(Alignment::Center));
    if !page.about.is_empty() {
        head = head.push(label(page.about.as_str(), BODY, p.text));
    }
    // its links, its followers and its stars
    let mut facts = Row::new().spacing(18).align_y(Alignment::Center);
    for (glyph, name, url) in [(Glyph::Globe, "Website", &page.website), (Glyph::Link, "Source", &page.source)] {
        if !url.is_empty() {
            let face = row![icons::icon(glyph, 16.0, p.accent, false), label(name, 14.0, p.accent)].spacing(6).align_y(Alignment::Center);
            facts = facts.push(kit::tip(button(face).padding(0).on_press(Msg::OpenLink(url.clone())).style(look::bare(p)), url.as_str()));
        }
    }
    facts = facts.push(total(sum.followers, "Follower", "Followers"));
    if let Some(rating) = sum.rating {
        facts = facts.push(
            row![icons::icon(Glyph::Star, 15.0, p.accent, true), kit::figure(format!("{rating:.1}"), 14.0, p.text), label(counted(sum.reviews, "review", "reviews"), 14.0, p.dim)]
                .spacing(5)
                .align_y(Alignment::Center),
        );
    }
    head = head.push(facts.wrap().vertical_spacing(8));
    // following it, and its keepers' tools
    let team_open = s.product_makers.as_ref().is_some_and(|(h, _)| *h == handle);
    let team = match (team_open, keeper, page.testers.filter(|n| *n > 0)) {
        (true, _, _) => "Close makers".to_string(),
        (false, true, Some(n)) => format!("Makers and testers \u{00B7} {n} asking"),
        (false, true, None) => "Makers and testers".to_string(),
        (false, false, _) => "Makers".to_string(),
    };
    let mut acts = Row::new().spacing(8).align_y(Alignment::Center);
    if !keeper {
        acts = acts.push(if sum.you_follow {
            kit::following(Msg::FollowProduct(handle.clone(), false))
        } else {
            kit::primary("Follow", Some(Msg::FollowProduct(handle.clone(), true)))
        });
    }
    acts = acts.push(small(team, Some(Msg::ShowTeam((!team_open).then(|| handle.clone())))));
    if keeper {
        acts = acts.push(small(if s.edit_product.is_some() { "Close the editor" } else { "Edit" }, Some(Msg::EditProduct(s.edit_product.is_none()))));
    }
    if page.you_own() {
        let confirming = s.confirm_delete_product.as_deref() == Some(handle.as_str());
        acts = acts.push(small(if confirming { "Delete it: press again" } else { "Delete product" }, Some(Msg::DeleteProduct(handle.clone()))));
    }
    if moderator(s) && !keeper {
        acts = acts.push(small("Suspend product (moderator)", Some(Msg::ModProduct(handle.clone(), true))));
    }
    head = head.push(acts.wrap().vertical_spacing(8));
    if let Some(form) = &s.edit_product {
        head = head.push(product_editor(form));
    }
    if keeper {
        let free = s.uploading == 0;
        let mut photos = Row::new().spacing(6).align_y(Alignment::Center);
        photos = photos.push(small("Change logo", free.then(|| Msg::PickPhotos(PhotoFor::Product(handle.clone(), "logo", 0)))));
        if pr.logo.is_some() {
            photos = photos.push(small("Remove logo", Some(Msg::ClearPhoto(PhotoFor::Product(handle.clone(), "logo", 0)))));
        }
        if page.shots.len() < pages::MAX_PRODUCT_SHOTS {
            photos = photos.push(small("Add a screenshot", free.then(|| Msg::PickPhotos(PhotoFor::Product(handle.clone(), "shot", page.shots.len())))));
        }
        if !free {
            photos = photos.push(label("Uploading\u{2026}", 13.0, p.dim));
        }
        head = head.push(photos.wrap().vertical_spacing(6));
    }
    // being listed as a maker: confirm it, or decline
    if page.your_role.as_deref() == Some("invited") {
        head = head.push(
            row![
                container(kit::notice("You are listed as one of its makers. Confirm it to show on its page and post its updates and launches.", p.accent))
                    .width(Length::Fill),
                kit::primary("Confirm", Some(Msg::AnswerMaker(handle.clone(), true))),
                small("Decline", Some(Msg::AnswerMaker(handle.clone(), false))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }
    // asking to test it, while it takes testers
    if sum.stage == Stage::Testing && !keeper {
        match page.your_test.as_deref() {
            Some("invited") => {
                let mut invited = Column::new().spacing(6).push(strong("You are invited to test it", BODY, p.text));
                if let Some(how) = page.your_invite.as_ref().filter(|how| !how.is_empty()) {
                    invited = invited.push(label(how.as_str(), BODY, p.text));
                }
                invited = invited.push(kit::note("Only you see this note."));
                head = head.push(container(invited).padding([12.0, 14.0]).width(Length::Fill).style(look::frame(p)));
            }
            Some(_) => {
                head = head.push(
                    row![label("You asked to test it. Its makers decide, and tell you how to join.", 14.0, p.dim).width(Length::Fill), small("Withdraw", Some(Msg::AskToTest(handle.clone(), false)))]
                        .spacing(8)
                        .align_y(Alignment::Center),
                );
            }
            None if has_page => {
                head = head.push(
                    row![
                        kit::field("A note to its makers: what you would test it on (optional)", &s.test_note, Msg::TestNote).on_submit(Msg::AskToTest(handle.clone(), true)),
                        kit::pill("Ask to test", Some(Msg::AskToTest(handle.clone(), true)), Pill::Solid),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
            None => {}
        }
    }
    // its latest launch, while it can be supported
    if let Some(latest) = page.latest_launch.as_deref()
        && let Some(launch) = latest.launch.as_deref().filter(|l| l.live)
    {
        head = head.push(launch_box(latest, launch, keeper));
    }
    // what it runs on, and what it is built with
    if !page.platforms.is_empty() {
        let mut chips = Row::new().spacing(6).align_y(Alignment::Center).push(label("Runs on", 13.0, p.dim));
        for platform in &page.platforms {
            chips = chips.push(kit::chip(pages::platform_label(platform), p.text));
        }
        head = head.push(chips.wrap().vertical_spacing(6));
    }
    if !sum.tech.is_empty() {
        let mut chips = Row::new().spacing(6).align_y(Alignment::Center).push(label("Built with", 13.0, p.dim));
        for tech in &sum.tech {
            chips = chips.push(kit::chip(tech.as_str(), p.accent));
        }
        head = head.push(chips.wrap().vertical_spacing(6));
    }
    // its screenshots, side by side
    if !page.shots.is_empty() {
        let mut strip = Row::new().spacing(10);
        for (i, photo) in page.shots.iter().enumerate() {
            let mut one = Column::new().spacing(4).push(kit::tip(shot(s, &photo.url, 224.0, 140.0), if photo.alt.is_empty() { "A screenshot" } else { photo.alt.as_str() }));
            if keeper {
                one = one.push(small("Remove", Some(Msg::ClearPhoto(PhotoFor::Product(handle.clone(), "shot", i)))));
            }
            strip = strip.push(one);
        }
        let strip = scrollable(strip)
            .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new().width(4).scroller_width(4).margin(2)))
            .style(look::scrollers(p));
        head = head.push(strip);
    }
    // who makes it
    let makers: Vec<&pages::ProductMaker> = page.makers.iter().filter(|m| m.active).collect();
    if !makers.is_empty() {
        let mut r = Row::new().spacing(12).align_y(Alignment::Center).push(label("Made by", 14.0, p.dim));
        for m in makers.iter().take(6) {
            let face = row![avatar(s, m.person.avatar.as_ref(), m.person.name(), 24.0), label(m.person.name(), 14.0, p.text)].spacing(6).align_y(Alignment::Center);
            let open = m.person.username.clone().filter(|_| m.person.has_page).map(|u| Msg::Go(Screen::Person(u)));
            r = r.push(button(face).padding(0).on_press_maybe(open).style(look::bare(p)));
        }
        if makers.len() > 6 {
            r = r.push(label(format!("and {} more", makers.len() - 6), 14.0, p.dim));
        }
        head = head.push(r.wrap().vertical_spacing(6));
    }
    if team_open {
        head = head.push(product_team(s, page));
    }
    let mut col = column![kit::block(head), kit::tabs(&ProductShow::ALL, s.product_show, |t| t.title().to_string(), Msg::ProductShow)];
    match s.product_show {
        show @ (ProductShow::Updates | ProductShow::Launches) => {
            if keeper && has_page {
                col = col.push(composer(s));
            }
            col = col.push(posts_list(
                s,
                &page.posts.posts,
                page.posts.more,
                if show == ProductShow::Updates {
                    "No updates yet. Its makers post what changed and what is next, About it, and its followers see them."
                } else {
                    "Not launched yet. Its makers launch a version from the composer, About it."
                },
            ));
        }
        ProductShow::Feedback => {
            let mut sorts = Row::new().spacing(6).align_y(Alignment::Center);
            for (sort, words) in [(FeedbackSort::Top, "Most voted"), (FeedbackSort::New, "Newest")] {
                sorts = sorts.push(choice(words, s.feedback_sort == sort, Msg::FeedbackSort(sort)));
            }
            if has_page && !matches!(s.giving, Some(Giving::Feedback(..))) {
                sorts = sorts.push(space().width(Length::Fill)).push(kit::pill(
                    "Give feedback",
                    Some(Msg::Give(Some(Giving::Feedback(FeedbackCategory::Idea, String::new())))),
                    Pill::Solid,
                ));
            }
            let mut statuses = Row::new().spacing(6).push(choice("All", s.feedback_status.is_none(), Msg::FeedbackFilter(None)));
            for status in FeedbackStatus::ALL {
                statuses = statuses.push(choice(status.label(), s.feedback_status == Some(status), Msg::FeedbackFilter(Some(status))));
            }
            col = col.push(kit::padded(column![sorts, statuses.wrap().vertical_spacing(6)].spacing(10)));
            if let Some(giving @ Giving::Feedback(..)) = &s.giving {
                col = col.push(giving_form(giving));
            }
            col = col.push(kit::hairline()).push(posts_list(
                s,
                &page.posts.posts,
                page.posts.more,
                "No feedback here yet. An idea, a bug or a question: its makers set where each stands, and everyone who voted is told.",
            ));
        }
        ProductShow::Reviews => {
            col = col.push(ratings(page));
            if has_page && !keeper && page.your_review.is_none() {
                col = col.push(match &s.giving {
                    Some(giving @ Giving::Review(..)) => giving_form(giving),
                    _ => kit::block(kit::pill("Write a review", Some(Msg::Give(Some(Giving::Review(0, String::new())))), Pill::Solid)),
                });
            }
            col = col.push(posts_list(s, &page.posts.posts, page.posts.more, "No reviews yet."));
        }
    }
    col.into()
}

/// Writing feedback (its category, then its words) or a review (its stars, then its words) on the product on show.
fn giving_form(giving: &Giving) -> El<'_> {
    let p = pal();
    let mut col = Column::new().spacing(10);
    let (hint, note) = match giving {
        Giving::Feedback(category, _) => {
            let mut r = Row::new().spacing(6).align_y(Alignment::Center).push(label("It is", 14.0, p.dim));
            for c in FeedbackCategory::ALL {
                r = r.push(choice(c.label(), *category == c, Msg::GiveCategory(c)));
            }
            col = col.push(r);
            let hint = match category {
                FeedbackCategory::Idea => "Your idea",
                FeedbackCategory::Bug => "What happened, and what you expected",
                FeedbackCategory::Question => "Your question",
            };
            (hint, "Feedback is public. Its makers set where it stands, and you and everyone who voted for it are told.")
        }
        Giving::Review(n, _) => {
            let mut r = Row::new().spacing(2).align_y(Alignment::Center).push(label("Your stars", 14.0, p.dim));
            for i in 1..=5u8 {
                let on = i <= *n;
                r = r.push(button(icons::icon(Glyph::Star, 22.0, if on { p.accent } else { p.dim }, on)).padding(3).on_press(Msg::GiveStars(i)).style(look::bare(p)));
            }
            col = col.push(r);
            (
                "What it does well, and what it could do better",
                "A review is public: one for each person and product, from a page at least a week old. No review may be bought or rewarded.",
            )
        }
    };
    let words = giving.text();
    col = col.push(text_input(hint, words).on_input(Msg::GiveText).on_submit(Msg::SendGiving).padding([10.0, 12.0]).size(15).style(look::field(p)));
    let ready = !words.trim().is_empty() && words.chars().count() <= pages::POST_MAX_CHARS && !matches!(giving, Giving::Review(0, _));
    col = col.push(
        row![container(kit::note(note)).width(Length::Fill), small("Cancel", Some(Msg::Give(None))), kit::primary("Post", ready.then_some(Msg::SendGiving))]
            .spacing(8)
            .align_y(Alignment::Center),
    );
    kit::block(col)
}

/// A product's stars: their average, and how many gave each number of stars as bars.
fn ratings(page: &ProductPage) -> El<'_> {
    let p = pal();
    let sum = &page.summary;
    let Some(average) = sum.rating else {
        return kit::block(kit::note("No reviews yet. Anyone with a page a week old can review it once; its makers cannot."));
    };
    let most = page.stars.iter().copied().max().unwrap_or(0).max(1);
    let mut bars = Column::new().spacing(5).width(Length::Fill);
    for n in (1..=5usize).rev() {
        let count = page.stars[n - 1];
        let share = ((count as f64 / most as f64) * 100.0).round() as u16;
        let bar: El<'_> = if share == 0 {
            space().width(Length::Fill).into()
        } else if share >= 100 {
            container(space()).width(Length::Fill).height(8).style(look::share(p, true)).into()
        } else {
            row![container(space()).width(Length::FillPortion(share)).height(8).style(look::share(p, true)), space().width(Length::FillPortion(100 - share))].into()
        };
        bars = bars.push(
            row![kit::figure(n.to_string(), 12.5, p.dim), container(bar).width(Length::Fill).height(8), container(kit::figure(compact(count), 12.5, p.dim)).width(Length::Fixed(40.0))]
                .spacing(8)
                .align_y(Alignment::Center),
        );
    }
    let big = column![
        text(format!("{average:.1}")).size(40.0).font(fonts().display).color(p.text),
        stars(average.round() as u8, 14.0),
        label(counted(sum.reviews, "review", "reviews"), SMALL, p.dim),
    ]
    .spacing(4)
    .align_x(Alignment::Center);
    kit::block(row![big, bars].spacing(24).align_y(Alignment::Center))
}

/// A product's makers, and for its keepers the invited, who asked to test it, and listing another maker.
fn product_team<'a>(s: &'a State, page: &'a ProductPage) -> El<'a> {
    let p = pal();
    let handle = page.summary.product.handle.as_str();
    let keeper = page.you_keep();
    let me = me_id(s);
    let owner = page.summary.owner.as_ref().map(|o| o.id.as_str());
    let mut col = Column::new().spacing(10).push(kit::part_title("Makers"));
    match s.product_makers.as_ref().filter(|(h, _)| h == handle) {
        Some((_, makers)) => {
            for m in makers {
                let mut line = Row::new().spacing(10).align_y(Alignment::Center).push(avatar(s, m.person.avatar.as_ref(), m.person.name(), 32.0)).push(name_line(&m.person));
                if owner == Some(m.person.id.as_str()) {
                    line = line.push(kit::chip("Owner", p.accent));
                } else if !m.active {
                    line = line.push(kit::chip("Invited", p.dim));
                }
                line = line.push(space().width(Length::Fill));
                let id = m.person.id.clone();
                if owner != Some(id.as_str()) {
                    if id == me {
                        line = line.push(small("Leave", Some(Msg::RemoveMaker(handle.to_string(), id))));
                    } else if keeper {
                        line = line.push(small(if m.active { "Take off" } else { "Withdraw" }, Some(Msg::RemoveMaker(handle.to_string(), id))));
                    }
                }
                col = col.push(line);
            }
        }
        None => col = col.push(kit::note("Reading\u{2026}")),
    }
    if keeper {
        col = col.push(
            row![
                kit::field("A username, to list as a maker", &s.maker_name, Msg::MakerName).on_submit(Msg::InviteMaker(handle.to_string())),
                small("List as a maker", (!s.maker_name.is_empty()).then(|| Msg::InviteMaker(handle.to_string()))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        col = col.push(kit::note("A maker shows once they confirm, and can post its updates and launches. Its owner stays its owner."));
        col = col.push(kit::part_title("Asking to test"));
        match s.product_testers.as_ref().filter(|(h, _)| h == handle) {
            Some((_, testers)) if testers.is_empty() => {
                col = col.push(kit::note(if page.summary.stage == Stage::Testing {
                    "Nobody has asked yet."
                } else {
                    "Set its stage to Testing to take asks to test it."
                }));
            }
            Some((_, testers)) => {
                for tester in testers {
                    let mut info = Column::new().spacing(4).push(name_line(&tester.person));
                    if !tester.note.is_empty() {
                        info = info.push(label(format!("\u{201C}{}\u{201D}", tester.note), 14.0, p.text));
                    }
                    let id = tester.person.id.clone();
                    let mut line = Row::new().spacing(8).align_y(Alignment::Center).push(info.width(Length::Fill));
                    if tester.invited {
                        line = line.push(kit::chip("Invited", p.repost));
                    } else {
                        line = line
                            .push(kit::pill("Invite", Some(Msg::AnswerTester(handle.to_string(), id.clone(), true)), Pill::Solid))
                            .push(small("Decline", Some(Msg::AnswerTester(handle.to_string(), id, false))));
                    }
                    col = col.push(line);
                }
                col = col.push(kit::field("How to join, sent with an invitation: a link or a code, which only they see", &s.tester_note, Msg::TesterNote));
            }
            None => col = col.push(kit::note("Reading\u{2026}")),
        }
    }
    container(col).padding([12.0, 14.0]).width(Length::Fill).style(look::frame(p)).into()
}

// ------------------------------------------------------------------- moderation (moderators only)

/// What a suspended person reads where the page's own controls would be.
const SUSPENDED: &str =
    "A moderator suspended your page: it shows nothing, and you cannot post, like or follow until it is restored.";

fn moderator(s: &State) -> bool {
    s.me.as_ref().is_some_and(|m| m.moderator)
}

fn moderation(s: &State) -> El<'_> {
    let mut views = Row::new().spacing(8);
    for view in ModView::ALL {
        let on = view == s.mod_view;
        views = views.push(
            button(label(view.title(), 12.5, if on { pal().accent } else { pal().dim }))
                .padding([5.0, 12.0])
                .on_press(Msg::ModView(view))
                .style(look::choice(pal(), on)),
        );
    }
    let mut col = Column::new().push(kit::padded(views.wrap()));
    if matches!(s.mod_view, ModView::Open | ModView::Closed) {
        col = col
            .push(text_input("A note for the log (optional)", &s.mod_note).on_input(Msg::ModNote).padding([10.0, 12.0]).size(15).style(look::field(pal())))
            .push(kit::note(
                "Hiding or suspending closes every open report on the same post, page or organization. Every act is kept in \
                 the moderation log.",
            ));
    }
    let more = match s.mod_view {
        ModView::Matches => {
            let Some(list) = &s.mod_matches else { return col.into() };
            col = col.push(kit::padded(kit::note(MATCHES)));
            if list.is_empty() {
                col = col.push(kit::padded(kit::note("No photo matches to act on.")));
            }
            for m in list {
                col = col.push(match_card(m));
            }
            false
        }
        ModView::Log => {
            let Some((actions, more)) = &s.mod_log else { return col.into() };
            if actions.is_empty() {
                col = col.push(kit::padded(kit::note("Nothing has been done yet.")));
            }
            for a in actions {
                col = col.push(log_row(a));
            }
            *more
        }
        view => {
            let Some((reports, more)) = &s.mod_reports else { return col.into() };
            if reports.is_empty() {
                col = col.push(kit::padded(kit::note(if view == ModView::Open { "No open reports." } else { "No closed reports." })));
            }
            for r in reports {
                col = col.push(report_card(r));
            }
            *more
        }
    };
    if more {
        col = col.push(kit::outline("Load more", Some(Msg::More)));
    }
    col.into()
}

/// What the photo matches are, said above them.
const MATCHES: &str = "A screen matched each upload below to known illegal imagery. It was refused and never shown; its \
    bytes are kept privately as evidence for a year, and the account cannot upload photos until it is cleared. The \
    operator reports each match to NCMEC's CyberTipline: mark it reported once that is done.";

/// A screen's match: who uploaded it, which screen matched and when, whether it was reported; Mark reported, Clear.
fn match_card(m: &MediaMatch) -> El<'_> {
    let mut head = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(kit::chip(m.provider.as_str(), pal().caution))
        .push(label(format!("matched {}", when(&m.matched_at)), 13.0, pal().dim));
    if let Some(at) = &m.reported_at {
        head = head.push(kit::chip(format!("Reported {}", when(at)), pal().dim));
    }
    let who: El<'_> = match &m.person {
        Some(p) => name_line(p),
        None => kit::note("An account that is no longer available."),
    };
    let mut acts = Row::new().spacing(6).align_y(Alignment::Center);
    if m.reported_at.is_none() {
        acts = acts.push(kit::pill("Mark reported", Some(Msg::MarkMatch(m.id.clone(), true, false)), Pill::Solid));
    }
    acts = acts.push(kit::outline("Clear: the account may upload again", Some(Msg::MarkMatch(m.id.clone(), false, true))));
    kit::block(column![head.wrap(), who, acts].spacing(8))
}

fn reason_words(reason: &str) -> &str {
    Reason::ALL.into_iter().find(|r| r.as_str() == reason).map(|r| r.label()).unwrap_or(reason)
}

fn report_card(r: &ModReport) -> El<'_> {
    let mut head = Row::new().spacing(8).align_y(Alignment::Center).push(kit::chip(reason_words(&r.reason), pal().caution));
    if !r.open() {
        head = head.push(kit::chip(if r.state == "actioned" { "Acted on" } else { "Dismissed" }, pal().dim));
    }
    let by = r.reporter.as_ref().map(|p| p.name().to_string()).unwrap_or_else(|| "an unavailable account".into());
    head = head.push(label(format!("reported {} by {by}", when(&r.at)), 13.0, pal().dim));
    if r.open() && r.open_on_target > 1 {
        head = head.push(label(format!("{} open reports on this", r.open_on_target), 13.0, pal().accent));
    }
    let mut col = Column::new().spacing(8).push(head.wrap());
    if !r.note.is_empty() {
        col = col.push(label(format!("\u{201C}{}\u{201D}", r.note), 14.0, pal().text));
    }
    col = col.push(target_box(&r.target));
    let mut acts = Row::new().spacing(6).align_y(Alignment::Center);
    if let Some(act) = ModAct::for_target(&r.target) {
        // an open report offers the act; a closed one only the undo
        let undo = matches!(act, ModAct::RestorePost | ModAct::RestorePage | ModAct::RestoreOrg);
        if r.open() || undo {
            acts = acts.push(kit::pill(act.label(), Some(Msg::ModAct(act, None, Some(r.id.clone()))), Pill::Solid));
        }
    }
    if r.open() {
        acts = acts.push(kit::outline("Dismiss", Some(Msg::ModDismiss(r.id.clone()))));
    }
    col = col.push(acts);
    kit::block(col)
}

/// What a report names, as a moderator sees it: hidden and suspended things included.
fn target_box(t: &ModTarget) -> El<'_> {
    let mut col = Column::new().spacing(6);
    match t.kind.as_str() {
        "post" => {
            col = col.push(kit::part_title("Post"));
            if let Some(p) = &t.person {
                col = col.push(name_line(p));
            }
            match &t.post {
                Some(post) => {
                    if let Some(title) = &post.title {
                        col = col.push(strong(title.as_str(), 15.0, pal().text));
                    }
                    col = col.push(label(ui::cut(&post.text, 600), 15.0, pal().text));
                    if post.deleted {
                        col = col.push(kit::note("Deleted by its author; kept because others replied or quoted."));
                    }
                }
                None => col = col.push(label(ui::cut(&t.text, 600), 15.0, pal().text)),
            }
        }
        "person" => {
            col = col.push(kit::part_title("Person"));
            col = col.push(match &t.person {
                Some(p) => name_line(p),
                None => kit::note("An account that is no longer available."),
            });
            if !t.text.is_empty() {
                col = col.push(label(ui::cut(&t.text, 400), 14.0, pal().dim));
            }
        }
        "org" => {
            col = col.push(kit::part_title("Organization"));
            if let Some(o) = &t.org {
                col = col.push(strong(format!("{} ({})", o.name, o.ticker), 15.0, pal().text));
            }
            if !t.text.is_empty() {
                col = col.push(label(ui::cut(&t.text, 400), 14.0, pal().dim));
            }
        }
        _ => col = col.push(kit::note("What this report named is not there any more.")),
    }
    let mut chips = Row::new().spacing(6);
    if t.hidden {
        chips = chips.push(kit::chip("Hidden", pal().caution));
    }
    if t.suspended {
        chips = chips.push(kit::chip(if t.kind == "post" { "Its author's page is suspended" } else { "Suspended" }, pal().caution));
    }
    container(col.push(chips)).padding([10.0, 12.0]).width(Length::Fill).style(look::frame(pal())).into()
}

fn log_row(a: &ModAction) -> El<'_> {
    let who = if a.by_host {
        "On the identity host,".to_string()
    } else {
        a.moderator.as_ref().map(|p| p.name().to_string()).unwrap_or_else(|| "A moderator".into())
    };
    let mut col = Column::new().spacing(4).push(
        row![
            label(when(&a.at), 12.5, pal().dim),
            label(format!("{who} {}", a.words()), 14.0, pal().text),
            label(format!("{} {}", a.target_kind, ui::cut(&a.target_id, 20)), 12.5, pal().dim),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    if !a.note.is_empty() {
        col = col.push(label(format!("\u{201C}{}\u{201D}", a.note), 13.5, pal().dim));
    }
    kit::block(col)
}


#[cfg(test)]
mod tests {
    use super::{code_pieces, kind_words, mention_pieces, notification_words, tag_pieces};
    use alelyon_identity_client::pages::{Feedback, FeedbackStatus, Kind, Launch, Notification, Org, Person, Poll, Post, Product};

    #[test]
    fn a_job_an_article_and_a_poll_are_named_over_their_words() {
        let job = Post { kind: Kind::Article, job: Some(Box::default()), ..Post::default() };
        assert_eq!(kind_words(&job), Some("Job opening"), "a job opening is an article, named as the job");
        assert_eq!(kind_words(&Post { kind: Kind::Article, ..Post::default() }), Some("Article"));
        assert_eq!(kind_words(&Post { poll: Some(Box::new(Poll::default())), ..Post::default() }), Some("Poll"));
        assert_eq!(kind_words(&Post::default()), None, "a plain post says nothing");
        let about = || Post { product: Some(Product { handle: "loom".into(), ..Product::default() }), ..Post::default() };
        assert_eq!(kind_words(&about()), Some("Update"), "a post about a product is its update");
        assert_eq!(kind_words(&Post { launch: Some(Box::default()), ..about() }), Some("Launch"));
        assert_eq!(kind_words(&Post { feedback: Some(Box::default()), ..about() }), Some("Feedback"));
        assert_eq!(kind_words(&Post { stars: Some(4), ..about() }), Some("Review"));
        assert_eq!(kind_words(&Post { kind: Kind::Article, ..about() }), Some("Article"), "an article about a product stays one");
    }

    #[test]
    fn what_happens_to_a_product_says_which_product() {
        let ada = Person { display_name: "Ada".into(), ..Person::default() };
        let loom = Product { handle: "loom".into(), name: "Loom".into(), ..Product::default() };
        let said = |kind: &str, post: Option<Post>| {
            notification_words(&Notification { kind: kind.into(), actor: Some(ada.clone()), product: Some(loom.clone()), post, ..Notification::default() })
        };
        let launch = Post { launch: Some(Box::new(Launch { version: "v2.0".into(), ..Launch::default() })), ..Post::default() };
        let planned = Post { feedback: Some(Box::new(Feedback { status: FeedbackStatus::Planned, ..Feedback::default() })), ..Post::default() };
        assert_eq!(said("maker-invited", None), "Ada listed you as a maker of Loom");
        assert_eq!(said("product-launched", Some(launch)), "Ada launched Loom v2.0");
        assert_eq!(said("feedback-status", Some(planned)), "Ada marked feedback on Loom as planned");
        assert_eq!(said("product-review", Some(Post { stars: Some(4), ..Post::default() })), "Ada reviewed Loom: 4 of 5 stars");
        assert_eq!(said("tester-request", None), "Ada asked to test Loom");
        assert_eq!(said("tester-invited", None), "Ada invited you to test Loom");
    }

    #[test]
    fn code_is_what_fences_hold() {
        assert_eq!(code_pieces("No code here."), vec![("No code here.", false)]);
        assert_eq!(
            code_pieces("Look:\n```rust\nfn main() {}\n```\nNice."),
            vec![("Look:", false), ("fn main() {}", true), ("Nice.", false)]
        );
        assert_eq!(code_pieces("```\n  indented\n\nkept\n```"), vec![("  indented\n\nkept", true)], "code keeps its lines as written");
        assert_eq!(code_pieces("Open:\n```\nruns on"), vec![("Open:", false), ("runs on", true)], "an open fence runs to the end");
        assert_eq!(code_pieces("```\n```"), vec![("", true)], "an empty block is still a block");
    }

    #[test]
    fn the_later_kinds_of_notification_say_what_happened() {
        let ada = Person { display_name: "Ada".into(), ..Person::default() };
        let babg = Org { name: "Babbage Engines".into(), ticker: "BABG".into(), ..Org::default() };
        let said = |kind: &str, detail: Option<&str>| {
            notification_words(&Notification {
                kind: kind.into(),
                actor: Some(ada.clone()),
                org: Some(babg.clone()),
                detail: detail.map(str::to_string),
                ..Notification::default()
            })
        };
        assert_eq!(said("endorsed", Some("Engines")), "Ada endorsed your skill Engines");
        assert_eq!(said("experience-confirmed", Some("Analyst")), "Babbage Engines (BABG) confirmed your position as Analyst");
        assert_eq!(said("job-application", None), "Ada applied for a job at Babbage Engines (BABG)");
        assert_eq!(said("like", Some("insightful")), "Ada found your post insightful");
        assert_eq!(said("like", None), "Ada liked your post");
    }

    #[test]
    fn mentions_are_read_as_the_service_reads_them() {
        assert_eq!(
            mention_pieces("Hi @Ada, see @bob."),
            vec![("Hi ", None), ("@Ada", Some("Ada")), (", see ", None), ("@bob", Some("bob")), (".", None)]
        );
        assert_eq!(mention_pieces("mail ada@example.com"), vec![("mail ada@example.com", None)], "not inside an address");
        assert_eq!(mention_pieces("@x and @@ada"), vec![("@x and @@ada", None)], "too short, and not after another @");
        assert_eq!(mention_pieces("Zoë @ada_"), vec![("Zoë ", None), ("@ada", Some("ada")), ("_", None)]);
        assert!(mention_pieces("").is_empty());
    }

    #[test]
    fn hashtags_are_read_as_the_service_reads_them() {
        let tags = |text: &str| tag_pieces(text).into_iter().filter_map(|(_, t)| t).collect::<Vec<_>>();
        assert_eq!(tags("Shipping #Engines today, #engines again and #2026"), ["engines", "engines"]);
        assert_eq!(tags(&format!("a#b ##x #{} #ok_1 (#paren)", "x".repeat(51))), ["ok_1", "paren"]);
        assert_eq!(
            tag_pieces("See #Looms."),
            vec![("See ", None), ("#Looms", Some("looms".to_string())), (".", None)]
        );
        assert_eq!(tags("Zoë#tag and café #café"), ["caf"], "glued to a letter is not a tag; the ASCII run is");
        assert!(tag_pieces("").is_empty());
    }
}
