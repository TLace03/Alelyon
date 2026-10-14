//! What the Pages page draws: its tabs and finder, the feeds with their composer, a post as a card, a person's and an
//! organization's page, a thread, the reader's own page and organizations, and an organization's members.

use alelyon_identity_client::pages::{
    self, Community, CommunitySummary, Education, Employment, Experience, Job, Joining, Kind, ListMembers, ListTarget, MediaMatch, Me,
    Member, Membership, ModAct, ModAction, ModReport, ModTarget,
    Notification, Org, PageList, Person, PersonPage, Photo, Poll, Post, Profile, Reaction, Reason, ReplyRule, SearchKind, Target, Workplace,
};
use alelyon_identity_client::social;
use iced::widget::{
    Column, Row, button, checkbox, column, container, image, pick_list, rich_text, row, space, span, text_editor, text_input,
};
use iced::{Alignment, ContentFit, Element, Length};

use super::photos::Picture;
use super::{
    AsWho, CommunityField, EduField, EmploymentPick, ExpField, HomeFeed, InWhere, JobDays, ModView, Msg, NewCommunity, PhotoFor,
    PollLength, Screen, State, TitleOrg, WorkplacePick,
};
use crate::catalogue::Section;
use crate::theme::{self, fonts};
use crate::ui::{self, chip, heading, label, note, strong, subheading};

type El<'a> = Element<'a, Msg>;

fn small<'a>(words: impl Into<String>, msg: Option<Msg>) -> El<'a> {
    button(label(words.into(), 12.0, theme::TEXT)).padding([4.0, 10.0]).on_press_maybe(msg).style(theme::ghost_button).into()
}

fn link<'a>(words: impl Into<String>, size: f32, msg: Msg) -> El<'a> {
    button(strong(words.into(), size, theme::TEXT)).padding(0).on_press(msg).style(theme::ghost_button).into()
}

fn when(at: &str) -> String {
    crate::utc::parse(at).map(|t| crate::utc::ago(t, crate::utc::now())).unwrap_or_default()
}

pub fn view(s: &State, phase: f32) -> El<'_> {
    let screen = s.screen.clone().unwrap_or(Screen::Home);
    let mut page = Column::new().spacing(16).push(heading("Pages", Section::Pages.purpose()));

    let mut tabs = Row::new().spacing(8);
    let mut shown_tabs = Screen::TABS.to_vec();
    if moderator(s) {
        shown_tabs.push(Screen::Moderation);
    }
    let unread = s.me.as_ref().map(|m| m.unread_notifications).unwrap_or(0);
    for tab in shown_tabs {
        let on = tab == screen;
        let words = match tab {
            Screen::Notifications if unread > 0 => format!("Notifications ({unread})"),
            _ => tab.title(),
        };
        tabs = tabs.push(
            button(label(words, 13.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([7.0, 14.0])
                .on_press(Msg::Go(tab))
                .style(theme::segment_button(on)),
        );
    }
    let finder = text_input("Search, @name, (TICKER) or #tag", &s.find)
        .on_input(Msg::FindText)
        .on_submit(Msg::Find)
        .padding(8)
        .size(13)
        .width(260)
        .style(ui::input_style);
    page = page.push(
        row![tabs.wrap(), space().width(Length::Fill), finder, ui::secondary("Refresh", (!s.busy()).then_some(Msg::Refresh))]
            .spacing(10)
            .align_y(Alignment::Center),
    );

    if let Some(p) = &s.problem {
        if pages::not_offered(p) {
            return page.push(ui::notice(pages::words(p), theme::GOLD)).into();
        }
        page = page.push(row![ui::notice(pages::words(p), theme::CAUTION), small("Dismiss", Some(Msg::Dismiss))].spacing(8).align_y(Alignment::Center));
    } else if let Some(n) = &s.note {
        page = page.push(row![label(n.as_str(), 12.5, theme::TEXT_DIM), small("Dismiss", Some(Msg::Dismiss))].spacing(8).align_y(Alignment::Center));
    }
    if !screen.is_tab() {
        page = page.push(row![small("\u{2039} Back", Some(Msg::Back)), strong(screen.title(), 15.0, theme::TEXT)].spacing(10).align_y(Alignment::Center));
    }
    if s.loading {
        page = page.push(ui::working(phase, "Reading\u{2026}"));
    }
    let body: El<'_> = match &screen {
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
    };
    page.push(body).into()
}

// ------------------------------------------------------------------- a person's name, as every card draws it

/// `[ Founder | CEO ]` with its gold mark when confirmed, the name (a link to the page), each shown ticker (a link to
/// its organization), and the handle.
fn name_line<'a>(p: &'a Person) -> El<'a> {
    let mut line = Row::new().spacing(6).align_y(Alignment::Center);
    if !p.title.is_empty() {
        line = line.push(label(format!("[ {} ]", p.title), 13.0, theme::TEXT_DIM));
        if p.title_verified {
            line = line.push(label("\u{2713}", 13.0, theme::GOLD));
        }
    }
    line = line.push(match &p.username {
        Some(u) if p.has_page => link(p.name(), 14.0, Msg::Go(Screen::Person(u.clone()))),
        _ => strong(p.name(), 14.0, theme::TEXT).into(),
    });
    for o in &p.tickers {
        line = line.push(button(label(format!("({})", o.ticker), 12.5, theme::GOLD)).padding(0).on_press(Msg::Go(Screen::Org(o.ticker.clone()))).style(theme::ghost_button));
    }
    line = line.push(label(p.handle(), 12.0, theme::TEXT_FAINT));
    line.wrap().into()
}

// ------------------------------------------------------------------- photos

/// A face or a logo, `px` across: the photo once it is drawn, else a frame with the name's first letter.
fn face<'a>(s: &'a State, url: &str, name: &str, px: f32) -> El<'a> {
    if let Some(Picture::Ready(h)) = s.photos.get(url) {
        return image(h.clone()).width(px).height(px).content_fit(ContentFit::Cover).border_radius(px / 2.0).into();
    }
    let first: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    container(label(first, px * 0.42, theme::TEXT_DIM)).center(px).style(theme::well).into()
}

/// A card's byline with the face before it, when there is one.
fn with_face<'a>(s: &'a State, url: Option<&'a String>, name: &'a str, line: El<'a>) -> El<'a> {
    match url {
        Some(u) => row![face(s, u, name, 36.0), line].spacing(10).align_y(Alignment::Center).into(),
        None => line,
    }
}

/// A post's photos: one at its own shape, two side by side, three or four two to a row.
fn photo_grid<'a>(s: &'a State, photos: &'a [Photo]) -> El<'a> {
    if let [one] = photos {
        return photo_cell(s, one, None);
    }
    let height = if photos.len() == 2 { 260.0 } else { 180.0 };
    let mut grid = Column::new().spacing(4);
    for pair in photos.chunks(2) {
        grid = grid.push(Row::with_children(pair.iter().map(|p| photo_cell(s, p, Some(height)))).spacing(4));
    }
    grid.into()
}

/// One photo: at its own shape (at most 480 high) when `height` is None, else cropped to fill `height`. A photo not
/// drawn shows its description in a frame.
fn photo_cell<'a>(s: &'a State, p: &'a Photo, height: Option<f32>) -> El<'a> {
    let state = s.photos.get(&p.url);
    if let Some(Picture::Ready(h)) = state {
        let shown = image(h.clone()).width(Length::Fill).border_radius(6.0);
        return match height {
            Some(px) => shown.height(px).content_fit(ContentFit::Cover).into(),
            None => container(shown).max_height(480.0).into(),
        };
    }
    let why = match state {
        Some(Picture::Failed) => "The photo could not be loaded.",
        _ => "Loading the photo\u{2026}",
    };
    let alt = if p.alt.is_empty() { "A photo without a description." } else { p.alt.as_str() };
    container(column![label(alt, 13.0, theme::TEXT), label(why, 11.5, theme::TEXT_FAINT)].spacing(4))
        .padding(12)
        .width(Length::Fill)
        .height(height.unwrap_or(110.0))
        .style(theme::well)
        .into()
}

/// A small square of a photo being attached, or a frame until it is drawn.
fn thumb<'a>(s: &'a State, url: &str, px: f32) -> El<'a> {
    match s.photos.get(url) {
        Some(Picture::Ready(h)) => image(h.clone()).width(px).height(px).content_fit(ContentFit::Cover).border_radius(4.0).into(),
        _ => container(label("Photo", 11.0, theme::TEXT_FAINT)).center(px).style(theme::well).into(),
    }
}

/// A page's or an organization's banner, cropped to fill; nothing until it is drawn.
fn banner<'a>(s: &'a State, url: Option<&String>) -> Option<El<'a>> {
    let Some(Picture::Ready(h)) = s.photos.get(url?) else { return None };
    Some(image(h.clone()).width(Length::Fill).height(150).content_fit(ContentFit::Cover).border_radius(6.0).into())
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
        r = r.push(label("Uploading\u{2026}", 12.0, theme::TEXT_DIM));
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
        note(format!("\u{2713} {} ({}) confirmed this title.", org.name, org.ticker))
    } else {
        note(format!("This title names {} ({}), which has not confirmed it.", org.name, org.ticker))
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

fn org_button<'a>(o: &'a Org) -> El<'a> {
    button(strong(format!("{} ({})", o.name, o.ticker), 14.0, theme::GOLD)).padding(0).on_press(Msg::Go(Screen::Org(o.ticker.clone()))).style(theme::ghost_button).into()
}

fn post_card<'a>(s: &'a State, p: &'a Post) -> El<'a> {
    if p.is_plain_repost() {
        let by = column![label(format!("\u{21BB} {} reposted", by_name(p)), 12.0, theme::TEXT_FAINT)];
        return match p.repost_of.as_deref() {
            Some(inner) => by.push(post_card(s, inner)).spacing(4).into(),
            None => by.push(note("The post it reposted is not there any more.")).spacing(4).into(),
        };
    }
    let mine = p.author.as_ref().is_some_and(|a| a.id == me_id(s));
    let admin = p.org.as_ref().is_some_and(|o| admin_of(s, &o.ticker));
    let mut col = Column::new().spacing(8);
    if let Some(o) = &p.org {
        let mut head = Row::new().spacing(8).align_y(Alignment::Center);
        if let Some(logo) = &o.logo {
            head = head.push(face(s, logo, &o.name, 36.0));
        }
        head = head.push(org_button(o));
        if let Some(a) = &p.author {
            head = head.push(label(format!("posted by {}", a.name()), 12.0, theme::TEXT_FAINT));
        }
        col = col.push(head);
        // Only the organization's owners and admins are told why a post has no author.
        if let Some(flag) = p.author_flag {
            col = col.push(label(flag.words(), 12.0, theme::CAUTION));
        }
    } else if let Some(a) = &p.author {
        col = col.push(with_face(s, a.avatar.as_ref(), a.name(), name_line(a)));
    }
    let mut stamp = when(&p.created_at);
    if p.edited_at.is_some() {
        stamp.push_str(" \u{00B7} edited");
    }
    col = col.push(match &p.community {
        Some(c) => row![label(stamp, 11.5, theme::TEXT_FAINT), label("in", 11.5, theme::TEXT_FAINT), community_link(c)]
            .spacing(5)
            .align_y(Alignment::Center)
            .into(),
        None => El::from(label(stamp, 11.5, theme::TEXT_FAINT)),
    });
    if p.deleted {
        return container(col.push(note("Deleted."))).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into();
    }
    if let Some(title) = &p.title {
        col = col.push(strong(title.as_str(), 17.0, theme::TEXT));
    }
    if !p.text.is_empty() {
        col = col.push(post_text(p.text.as_str(), 14.0));
    }
    if !p.media.is_empty() {
        col = col.push(photo_grid(s, &p.media));
    }
    if p.repost_of_id.is_some() {
        col = col.push(match p.repost_of.as_deref() {
            Some(inner) => quoted(inner),
            None => note("The quoted post is not there any more."),
        });
    }
    if let Some(poll) = &p.poll {
        col = col.push(poll_view(p, poll, mine));
    }
    match p.replies_from {
        ReplyRule::Everyone => {}
        ReplyRule::Followers => col = col.push(label("Replies: followers only", 11.5, theme::TEXT_FAINT)),
        ReplyRule::Mentioned => col = col.push(label("Replies: the people mentioned only", 11.5, theme::TEXT_FAINT)),
    }
    if let Some(job) = &p.job {
        col = col.push(job_view(s, p, job, admin));
    }
    // How people reacted, when any did otherwise than with a plain like.
    if p.reactions.iter().any(|(r, _)| *r != Reaction::Like) {
        let words: Vec<String> = p.reactions.iter().map(|(r, n)| format!("{n} {r}")).collect();
        col = col.push(label(words.join(" \u{00B7} "), 11.5, theme::TEXT_FAINT));
    }
    let heart = if p.liked { "\u{2665}" } else { "\u{2661}" };
    let mut actions = row![
        small(format!("Reply {}", p.replies), Some(Msg::Go(Screen::Thread(p.id.clone())))),
        small(format!("Repost {}", p.reposts), (!mine).then(|| Msg::Repost(p.id.clone()))),
        small("Quote", Some(Msg::Quote(p.clone()))),
        small(format!("{heart} {}", p.likes), Some(Msg::Like(p.id.clone(), !p.liked))),
        pick_list(Reaction::ALL, p.your_reaction, {
            let id = p.id.clone();
            move |r| Msg::React(id.clone(), r)
        })
        .placeholder("React")
        .padding([3.0, 8.0])
        .text_size(12.0)
        .font(fonts().ui)
        .style(theme::picker)
        .menu_style(theme::picker_menu),
        small(if p.bookmarked { "Saved" } else { "Save" }, Some(Msg::Bookmark(p.id.clone(), !p.bookmarked))),
    ]
    .spacing(4);
    actions = actions.push(space().width(Length::Fill));
    if mine {
        if p.reply_to.is_none() && p.org.is_none() {
            actions = actions.push(small("Pin", Some(Msg::Pin(p.id.clone()))));
        }
        actions = actions.push(small("Delete", Some(Msg::Delete(p.id.clone()))));
    } else {
        if admin {
            actions = actions.push(small("Delete (admin)", Some(Msg::Delete(p.id.clone()))));
        }
        if moderator(s) {
            actions = actions.push(small("Hide (moderator)", Some(Msg::ModAct(ModAct::HidePost, Some(Target::Post(p.id.clone())), None))));
        }
        actions = actions.push(small("Report", Some(Msg::Reporting(Some(Target::Post(p.id.clone()))))));
    }
    // The community's owners and moderators take a conversation out of it (its first post).
    if p.community.as_ref().is_some_and(|c| s.keeps(&c.handle)) {
        actions = actions.push(small("Take out of the community", Some(Msg::TakeOut(p.id.clone()))));
    }
    col = col.push(actions.align_y(Alignment::Center));
    if s.reporting == Some(Target::Post(p.id.clone())) {
        col = col.push(report_reasons(Target::Post(p.id.clone())));
    }
    container(col).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

/// A job opening: where, how and the pay, and until when it is open; Apply (with a note) or Withdraw; and for the
/// organization's owners and admins, how many applied and Close.
fn job_view<'a>(s: &'a State, p: &'a Post, job: &'a Job, admin: bool) -> El<'a> {
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
    let mut col = column![label(parts.join(" \u{00B7} "), 13.5, theme::TEXT), label(until, 11.5, theme::TEXT_FAINT)].spacing(6);
    let mut acts = Row::new().spacing(6).align_y(Alignment::Center);
    if admin {
        acts = acts.push(small(format!("Applicants ({})", job.applicants.unwrap_or(0)), Some(Msg::Go(Screen::Applicants(p.id.clone())))));
        if !job.closed {
            acts = acts.push(small("Close the job", Some(Msg::CloseJob(p.id.clone()))));
        }
    } else if job.applied {
        acts = acts.push(label("\u{2713} Applied", 12.0, theme::GOLD)).push(small("Withdraw", Some(Msg::Withdraw(p.id.clone()))));
    } else if !job.closed && s.me.as_ref().is_some_and(|m| m.me.has_page) {
        acts = acts.push(ui::secondary("Apply", Some(Msg::Applying(Some(p.id.clone())))));
    }
    col = col.push(acts);
    if let Some((_, words)) = s.applying.as_ref().filter(|(id, _)| id == &p.id) {
        col = col.push(
            text_input("A note for the organization (optional)", words)
                .on_input(Msg::ApplyNote)
                .on_submit(Msg::Apply)
                .padding(8)
                .size(13)
                .style(ui::input_style),
        );
        col = col.push(
            row![
                note("Its owners and admins will see your page and this note."),
                space().width(Length::Fill),
                small("Cancel", Some(Msg::Applying(None))),
                ui::primary("Apply", Some(Msg::Apply)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    container(col).padding([8.0, 12.0]).width(Length::Fill).style(theme::well).into()
}

/// A poll: each option's share once the votes are shown, else the options to vote for; its total and whether it is
/// still open.
fn poll_view<'a>(p: &'a Post, poll: &'a Poll, mine: bool) -> El<'a> {
    let mut col = Column::new().spacing(6);
    match poll.shares() {
        Some(shares) => {
            for (i, (option, share)) in poll.options.iter().zip(shares).enumerate() {
                let chosen = poll.you_voted == Some(i);
                let mut line = row![
                    label(format!("{share}%"), 12.5, if chosen { theme::GOLD } else { theme::TEXT_DIM }).width(44),
                    label(option.as_str(), 13.5, theme::TEXT)
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                if chosen {
                    line = line.push(label("\u{2713} your vote", 11.5, theme::GOLD));
                }
                col = col.push(line);
            }
        }
        None => {
            for (i, option) in poll.options.iter().enumerate() {
                col = col.push(ui::secondary(option.as_str(), (!poll.closed && !mine).then(|| Msg::Vote(p.id.clone(), i))));
            }
        }
    }
    let votes = if poll.total == 1 { "1 vote".to_string() } else { format!("{} votes", poll.total) };
    col = col.push(label(format!("{votes} \u{00B7} {}", if poll.closed { "closed" } else { "open" }), 11.5, theme::TEXT_FAINT));
    container(col).padding([8.0, 12.0]).width(Length::Fill).style(theme::well).into()
}

/// A quoted post, small and inside the card that quotes it.
fn quoted<'a>(p: &'a Post) -> El<'a> {
    let by: El<'a> = match (&p.author, &p.org) {
        (Some(a), _) => name_line(a),
        (None, Some(o)) => org_button(o),
        (None, None) => note("Posted by someone no longer here."),
    };
    let mut col = column![by].spacing(6);
    if let Some(title) = &p.title {
        col = col.push(strong(title.as_str(), 14.0, theme::TEXT));
    }
    col = col.push(label(ui::cut(&p.text, 400), 13.0, theme::TEXT_DIM));
    match p.media.len() {
        0 => {}
        1 => col = col.push(label("1 photo", 11.5, theme::TEXT_FAINT)),
        n => col = col.push(label(format!("{n} photos"), 11.5, theme::TEXT_FAINT)),
    }
    button(container(col).padding([8.0, 12.0]).width(Length::Fill).style(theme::well))
        .padding(0)
        .on_press(Msg::Go(Screen::Thread(p.id.clone())))
        .style(theme::ghost_button)
        .into()
}

fn report_reasons<'a>(target: Target) -> El<'a> {
    let mut r = Row::new().spacing(4).align_y(Alignment::Center).push(label("Why?", 12.0, theme::TEXT_DIM));
    for reason in Reason::ALL {
        r = r.push(small(reason.label(), Some(Msg::Report(target.clone(), reason))));
    }
    r.push(small("Cancel", Some(Msg::Reporting(None)))).wrap().into()
}

fn posts_list<'a>(s: &'a State, posts: &'a [Post], more: bool, empty: &'a str) -> El<'a> {
    let mut col = Column::new().spacing(10);
    if posts.is_empty() && !s.loading {
        col = col.push(note(empty));
    }
    for p in posts {
        col = col.push(post_card(s, p));
    }
    if more {
        col = col.push(ui::secondary("Load more", Some(Msg::More)));
    }
    col.into()
}

fn feed<'a>(s: &'a State, page: Option<&'a pages::Posts>, empty: &'a str) -> El<'a> {
    match page {
        Some(p) => posts_list(s, &p.posts, p.more, empty),
        None => space().into(),
    }
}

// ------------------------------------------------------------------- home: the composer and the feed

fn home(s: &State) -> El<'_> {
    let Some(me) = &s.me else { return space().into() };
    let mut col = Column::new().spacing(14);
    if me.suspended {
        col = col.push(ui::notice(SUSPENDED, theme::CAUTION));
    } else if !me.me.has_page {
        col = col.push(ui::notice("Open your page to post: My page, above.", theme::GOLD));
    } else {
        col = col.push(composer(s));
    }
    col = col.push(ui::tabs(&HomeFeed::ALL, s.home_feed, |f| f.title().to_string(), Msg::HomeFeed));
    col.push(match s.home_feed {
        HomeFeed::ForYou => match &s.for_you {
            Some((_, p)) => posts_list(
                s,
                &p.posts,
                p.more,
                "Nothing here yet. For you ranks others' posts of the last three days; Following shows those you follow.",
            ),
            None => space().into(),
        },
        HomeFeed::Following => {
            feed(s, s.home.as_ref(), "Your posts, and those of the people and organizations you follow, show here. Explore shows everyone's.")
        }
    })
    .into()
}

// ------------------------------------------------------------------- discovery: explore, a hashtag, a search

/// Trending hashtags and who to follow above everyone's latest posts.
fn explore(s: &State) -> El<'_> {
    let mut col = Column::new().spacing(14);
    if let Some(t) = &s.trending {
        let mut tags = Column::new();
        if t.tags.is_empty() {
            tags = tags.push(note("Nothing is trending yet. A hashtag trends once two or more people use it."));
        }
        for tag in &t.tags {
            let people = if tag.people == 1 { "person" } else { "people" };
            let posts = if tag.posts == 1 { "post" } else { "posts" };
            tags = tags.push(
                row![
                    button(label(format!("#{}", tag.tag), 14.0, theme::GOLD))
                        .padding(0)
                        .on_press(Msg::Go(Screen::Tag(tag.tag.clone())))
                        .style(theme::ghost_button),
                    label(format!("{} {posts} by {} {people}", tag.posts, tag.people), 12.0, theme::TEXT_FAINT),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(ui::card(if t.hours > 24 { "Trending this week" } else { "Trending today" }, tags));
    }
    if let Some(found) = s.suggestions.as_ref().filter(|f| !f.people.is_empty() || !f.orgs.is_empty()) {
        let mut who = Column::new();
        for sug in &found.people {
            let follow = sug.person.username.clone().map(|u| ui::primary("Follow", Some(Msg::FollowPerson(u, true))));
            let mut line = row![column![name_line(&sug.person), label(sug.why.words(sug.org.as_ref()), 12.0, theme::TEXT_FAINT)].spacing(2)]
                .spacing(10)
                .align_y(Alignment::Center);
            line = line.push(space().width(Length::Fill));
            if let Some(f) = follow {
                line = line.push(f);
            }
            who = who.push(line);
        }
        for o in &found.orgs {
            who = who.push(
                row![org_button(o), space().width(Length::Fill), ui::secondary("Follow", Some(Msg::FollowOrg(o.ticker.clone(), true)))]
                    .spacing(10)
                    .align_y(Alignment::Center),
            );
        }
        col = col.push(ui::card("Who to follow", who));
    }
    col.push(subheading("Latest")).push(feed(s, s.explore.as_ref(), "Nothing is posted yet.")).into()
}

fn tagged(s: &State) -> El<'_> {
    match &s.tagged {
        Some(t) => posts_list(s, &t.posts.posts, t.posts.more, "No post carries this hashtag."),
        None => space().into(),
    }
}

/// A search's parts as tabs, then what it found: people, organizations, posts.
fn search(s: &State) -> El<'_> {
    let mut col = Column::new().spacing(14).push(ui::tabs(&SearchKind::ALL, s.search_kind, |k| k.label().to_string(), Msg::SearchKind));
    let shown = s.found.as_ref().filter(|(q, k, _)| s.screen.as_ref() == Some(&Screen::Search(q.clone())) && *k == s.search_kind);
    let Some((_, kind, found)) = shown else { return col.into() };
    let all = *kind == SearchKind::All;
    if matches!(kind, SearchKind::All | SearchKind::People) {
        let mut people = Column::new();
        if found.people.is_empty() {
            people = people.push(note("Nobody with a page matches."));
        }
        for p in &found.people {
            people = people.push(name_line(p));
        }
        col = col.push(ui::card("People", people));
    }
    if matches!(kind, SearchKind::All | SearchKind::Orgs) {
        let mut orgs = Column::new();
        if found.orgs.is_empty() {
            orgs = orgs.push(note("No organization matches."));
        }
        for o in &found.orgs {
            orgs = orgs.push(org_button(o));
        }
        col = col.push(ui::card("Organizations", orgs));
    }
    if matches!(kind, SearchKind::All | SearchKind::Posts) {
        col = col.push(subheading("Posts")).push(posts_list(s, &found.posts.posts, found.posts.more && !all, "No post matches."));
        if all && found.posts.more {
            col = col.push(ui::secondary("More posts", Some(Msg::SearchKind(SearchKind::Posts))));
        }
    }
    col.into()
}

fn composer(s: &State) -> El<'_> {
    let c = &s.composer;
    let kinds = row![
        button(label("Post", 12.5, if c.kind == Kind::Post { theme::GOLD } else { theme::TEXT_DIM }))
            .padding([5.0, 12.0])
            .on_press(Msg::Kind(Kind::Post))
            .style(theme::segment_button(c.kind == Kind::Post)),
        button(label("Article", 12.5, if c.kind == Kind::Article { theme::GOLD } else { theme::TEXT_DIM }))
            .padding([5.0, 12.0])
            .on_press_maybe(c.quoting.is_none().then_some(Msg::Kind(Kind::Article)))
            .style(theme::segment_button(c.kind == Kind::Article)),
    ]
    .spacing(6);
    let mut top = row![kinds, space().width(Length::Fill)].spacing(10).align_y(Alignment::Center);
    let admin = s.me.as_ref().map(|m| m.admin_of()).unwrap_or_default();
    if !admin.is_empty() {
        let mut choices = vec![AsWho(None)];
        choices.extend(admin.into_iter().map(|t| AsWho(Some(t))));
        top = top.push(label("Post as", 12.0, theme::TEXT_DIM)).push(
            pick_list(choices, Some(AsWho(c.as_who.clone())), Msg::AsWho)
                .padding(6)
                .text_size(12.5)
                .font(fonts().ui)
                .style(theme::picker)
                .menu_style(theme::picker_menu),
        );
    }
    // Post in one of the reader's communities: one's own post, not an organization's, and not a quote.
    let joined: Vec<InWhere> =
        s.my_communities.iter().flatten().filter(|m| m.you_belong()).map(|m| InWhere(Some(m.community.handle.clone()))).collect();
    if !joined.is_empty() && c.as_who.is_none() && c.quoting.is_none() {
        let mut choices = vec![InWhere(None)];
        choices.extend(joined);
        top = top.push(label("Post in", 12.0, theme::TEXT_DIM)).push(
            pick_list(choices, Some(InWhere(c.in_community.clone())), Msg::InCommunity)
                .padding(6)
                .text_size(12.5)
                .font(fonts().ui)
                .style(theme::picker)
                .menu_style(theme::picker_menu),
        );
    }
    let mut col = column![top].spacing(10);
    if c.kind == Kind::Article {
        col = col.push(text_input("Title", &c.title).on_input(Msg::Title).padding(8).size(15).style(ui::input_style));
    }
    if let Some(q) = &c.quoting {
        col = col.push(row![label("Quoting", 12.0, theme::TEXT_DIM), space().width(Length::Fill), small("Cancel", Some(Msg::Unquote))].align_y(Alignment::Center));
        col = col.push(quoted(q));
    }
    col = col.push(
        text_editor(&c.body)
            .placeholder(if c.kind == Kind::Article { "Write your article" } else { "What are you working on?" })
            .on_action(Msg::Body)
            .height(Length::Fixed(if c.kind == Kind::Article { 260.0 } else { 90.0 }))
            .padding(10)
            .size(14)
            .font(fonts().ui)
            .style(theme::editor),
    );
    let mut extras = row![
        pick_list(ReplyRule::ALL, Some(c.replies), Msg::Replies)
            .padding(6)
            .text_size(12.5)
            .font(fonts().ui)
            .style(theme::picker)
            .menu_style(theme::picker_menu)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if c.kind == Kind::Post && c.quoting.is_none() {
        // a post carries a poll or photos, not both
        extras = extras.push(small(if c.poll_on { "Remove the poll" } else { "Add a poll" }, (c.poll_on || c.photos.is_empty()).then_some(Msg::PollToggle)));
    }
    let room = c.photos.len() + s.uploading < pages::POST_MAX_PHOTOS;
    extras = extras.push(small("Add photos", (room && !c.poll_on).then_some(Msg::PickPhotos(PhotoFor::Post))));
    if s.uploading > 0 {
        extras = extras.push(label("Uploading\u{2026}", 12.0, theme::TEXT_DIM));
    }
    col = col.push(extras);
    if !c.photos.is_empty() {
        let mut list = Column::new().spacing(8);
        for (i, photo) in c.photos.iter().enumerate() {
            list = list.push(
                row![
                    thumb(s, &photo.url, 64.0),
                    column![
                        label(photo.name.as_str(), 12.0, theme::TEXT_DIM),
                        text_input("Describe the photo for people who cannot see it (optional)", &photo.alt)
                            .on_input(move |t| Msg::PhotoAlt(i, t))
                            .padding(8)
                            .size(13)
                            .style(ui::input_style),
                    ]
                    .spacing(4),
                    small("Remove", Some(Msg::RemovePhoto(i))),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(list.push(note(PHOTO_RULE)));
    }
    if c.poll_on && c.kind == Kind::Post && c.quoting.is_none() {
        let mut poll = Column::new().spacing(6);
        for (i, option) in c.poll_options.iter().enumerate() {
            let hint = if i < 2 { "An option" } else { "Another option (optional)" };
            poll = poll.push(text_input(hint, option).on_input(move |t| Msg::PollOption(i, t)).padding(8).size(13).width(320).style(ui::input_style));
        }
        poll = poll.push(
            row![
                label("Open for", 12.0, theme::TEXT_DIM),
                pick_list(PollLength::ALL, Some(c.poll_length), Msg::PollHours)
                    .padding(6)
                    .text_size(12.5)
                    .font(fonts().ui)
                    .style(theme::picker)
                    .menu_style(theme::picker_menu),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        col = col.push(poll);
    }
    if c.kind == Kind::Article && c.as_who.is_some() {
        col = col.push(small(if c.job_on { "Not a job opening" } else { "Make it a job opening" }, Some(Msg::JobToggle)));
        if c.job_on {
            let job = &c.job;
            col = col.push(
                row![
                    pick_list(Workplace::ALL, Some(job.workplace), Msg::JobWorkplace)
                        .padding(6)
                        .text_size(12.5)
                        .font(fonts().ui)
                        .style(theme::picker)
                        .menu_style(theme::picker_menu),
                    pick_list(Employment::ALL, Some(job.employment), Msg::JobEmployment)
                        .padding(6)
                        .text_size(12.5)
                        .font(fonts().ui)
                        .style(theme::picker)
                        .menu_style(theme::picker_menu),
                    label("Open for", 12.0, theme::TEXT_DIM),
                    pick_list(JobDays::ALL, Some(c.job_days), Msg::JobDays)
                        .padding(6)
                        .text_size(12.5)
                        .font(fonts().ui)
                        .style(theme::picker)
                        .menu_style(theme::picker_menu),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
            let place = if job.workplace == Workplace::Remote { "Location (optional)" } else { "Location" };
            col = col.push(
                row![
                    text_input(place, &job.location).on_input(Msg::JobLocation).padding(8).size(13).width(240).style(ui::input_style),
                    text_input("Pay (optional), as you would write it", &job.pay).on_input(Msg::JobPay).padding(8).size(13).style(ui::input_style),
                ]
                .spacing(8),
            );
        }
    }
    let count = c.chars();
    let over = count > c.limit();
    col = col.push(
        row![
            label(format!("{count} / {}", c.limit()), 11.5, if over { theme::CAUTION } else { theme::TEXT_FAINT }),
            space().width(Length::Fill),
            ui::primary(if c.sending { "Posting\u{2026}" } else { "Post" }, (c.ready() && s.uploading == 0).then_some(Msg::Submit)),
        ]
        .align_y(Alignment::Center),
    );
    container(col).padding(16).width(Length::Fill).style(theme::panel).into()
}

// ------------------------------------------------------------------- jobs

fn jobs(s: &State) -> El<'_> {
    let q = &s.jobs_query;
    let filters = row![
        text_input("Search jobs", &q.words).on_input(Msg::JobsWords).on_submit(Msg::JobsSearch).padding(8).size(13).width(260).style(ui::input_style),
        pick_list(WorkplacePick::ALL, Some(WorkplacePick(q.workplace)), Msg::JobsWorkplace)
            .padding(6)
            .text_size(12.5)
            .font(fonts().ui)
            .style(theme::picker)
            .menu_style(theme::picker_menu),
        pick_list(EmploymentPick::ALL, Some(EmploymentPick(q.employment)), Msg::JobsEmployment)
            .padding(6)
            .text_size(12.5)
            .font(fonts().ui)
            .style(theme::picker)
            .menu_style(theme::picker_menu),
        ui::secondary("Search", Some(Msg::JobsSearch)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let empty = "No open jobs match. An organization's owners and admins open one as an article: Post as the organization, \
                 then Make it a job opening.";
    column![filters, feed(s, s.jobs.as_ref(), empty)].spacing(12).into()
}

fn applicants(s: &State) -> El<'_> {
    let Some((_, list)) = &s.applicants else { return space().into() };
    let mut col = Column::new().spacing(10);
    if list.is_empty() {
        col = col.push(note("Nobody has applied yet."));
    }
    for a in list {
        let mut card = column![
            row![name_line(&a.person), space().width(Length::Fill), label(when(&a.applied_at), 11.5, theme::TEXT_FAINT)].align_y(Alignment::Center)
        ]
        .spacing(6);
        if !a.note.is_empty() {
            card = card.push(post_text(a.note.as_str(), 13.5));
        }
        col = col.push(container(card).padding([10.0, 14.0]).width(Length::Fill).style(theme::panel));
    }
    col.push(note("Only the organization's owners and admins see who applied. An application is deleted 180 days after its job closes."))
        .into()
}

// ------------------------------------------------------------------- a person's page

fn person(s: &State) -> El<'_> {
    let Some(page) = &s.person else { return space().into() };
    let p = &page.person;
    let mine = p.id == me_id(s);
    let mut head = Column::new().spacing(8);
    if let Some(b) = banner(s, page.banner.as_ref()) {
        head = head.push(b);
    }
    head = head.push(match &p.avatar {
        Some(u) => row![face(s, u, p.name(), 72.0), name_line(p)].spacing(14).align_y(Alignment::Center).into(),
        None => name_line(p),
    });
    if let Some(n) = verified_note(p) {
        head = head.push(n);
    }
    if !page.bio.is_empty() {
        head = head.push(label(page.bio.as_str(), 14.0, theme::TEXT));
    }
    head = head.push(label(format!("{} followers \u{00B7} {} following", page.followers, page.following), 12.5, theme::TEXT_DIM));
    if !mine && let Some(u) = &p.username {
        let mut acts = Row::new().spacing(6);
        acts = acts.push(if page.you_follow {
            ui::secondary("Following", Some(Msg::FollowPerson(u.clone(), false)))
        } else {
            ui::primary("Follow", (!page.you_block).then(|| Msg::FollowPerson(u.clone(), true)))
        });
        acts = acts.push(small(if page.you_mute { "Unmute" } else { "Mute" }, Some(Msg::Mute(u.clone(), !page.you_mute))));
        acts = acts.push(small(if page.you_block { "Unblock" } else { "Block" }, Some(Msg::Block(u.clone(), !page.you_block))));
        let target = ListTarget::Person(u.clone());
        acts = acts.push(lists_button(s, &target, !page.you_block));
        if moderator(s) {
            acts = acts.push(small("Suspend page (moderator)", Some(Msg::ModAct(ModAct::SuspendPage, Some(Target::Person(u.clone())), None))));
        }
        acts = acts.push(small("Report", Some(Msg::Reporting(Some(Target::Person(u.clone()))))));
        head = head.push(acts.align_y(Alignment::Center));
        if let Some(picker) = list_picker(s, &target) {
            head = head.push(picker);
        }
        if s.reporting == Some(Target::Person(u.clone())) {
            head = head.push(report_reasons(Target::Person(u.clone())));
        }
    }
    let mut col = column![container(head).padding(16).width(Length::Fill).style(theme::panel)].spacing(14);
    if let Some(pinned) = &page.pinned {
        let mut over = Row::new().spacing(8).align_y(Alignment::Center).push(label("Pinned", 12.0, theme::GOLD));
        if page.person.id == me_id(s) {
            over = over.push(small("Unpin", Some(Msg::Unpin)));
        }
        col = col.push(column![over, post_card(s, pinned)].spacing(6));
    }
    if let Some(profile) = page.profile.as_ref().and_then(|p| profile_view(s, page, p)) {
        col = col.push(profile);
    }
    col.push(posts_list(s, &page.posts.posts, page.posts.more, "No posts yet.")).into()
}

// ------------------------------------------------------------------- the professional profile

/// Experience, education and skills as a page shows them; another person's skills carry Endorse. None when empty.
fn profile_view<'a>(s: &'a State, page: &'a PersonPage, profile: &'a Profile) -> Option<El<'a>> {
    if profile.experience.is_empty() && profile.education.is_empty() && profile.skills.is_empty() {
        return None;
    }
    let mut col = Column::new().spacing(14);
    if !profile.experience.is_empty() {
        col = col.push(ui::card("Experience", profile.experience.iter().fold(Column::new(), |c, e| c.push(experience_row(e)))));
    }
    if !profile.education.is_empty() {
        col = col.push(ui::card("Education", profile.education.iter().fold(Column::new(), |c, e| c.push(education_row(e)))));
    }
    if !profile.skills.is_empty() {
        let endorser = page.person.id != me_id(s) && s.me.as_ref().is_some_and(|m| m.me.has_page) && !page.you_block;
        let mut list = Column::new();
        for skill in &profile.skills {
            let mut line = row![label(skill.skill.as_str(), 13.5, theme::TEXT), label(endorsement_words(skill.endorsements), 12.0, theme::TEXT_FAINT)]
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
        col = col.push(ui::card("Skills", list));
    }
    Some(col.into())
}

/// A position: its title, where (the organization as a link, or the employer as written), when, a confirmation's mark.
fn experience_row(e: &Experience) -> El<'_> {
    let mut at = Row::new().spacing(8).align_y(Alignment::Center);
    match &e.org {
        Some(o) => at = at.push(org_button(o)),
        None if !e.org_name.is_empty() => at = at.push(label(e.org_name.as_str(), 13.0, theme::TEXT_DIM)),
        None => {}
    }
    at = at.push(label(e.span(), 12.0, theme::TEXT_FAINT));
    if e.confirmed {
        at = at.push(label("\u{2713} confirmed by the organization", 12.0, theme::GOLD));
    }
    let mut col = column![strong(e.title.as_str(), 14.0, theme::TEXT), at.wrap()].spacing(2);
    if !e.about.is_empty() {
        col = col.push(label(e.about.as_str(), 13.0, theme::TEXT_DIM));
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
    column![strong(e.school.as_str(), 14.0, theme::TEXT), label(line.join(" \u{00B7} "), 12.5, theme::TEXT_DIM)].spacing(2).into()
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
    text_input(hint, value).on_input(on).padding(8).size(13).width(320).style(ui::input_style).into()
}

/// My page's experience, education and skills: each entry with Edit and Delete, a form that adds or changes one, and
/// the skills as one comma-separated line.
fn profile_editor<'a>(s: &'a State, me: &'a Me) -> El<'a> {
    let f = &s.profile_form;
    let mut exp = Column::new();
    for e in &me.profile.experience {
        exp = exp.push(
            row![experience_row(e), space().width(Length::Fill), small("Edit", Some(Msg::ExpEdit(e.clone()))), small("Delete", Some(Msg::DeleteExperience(e.id.clone())))]
                .spacing(6)
                .align_y(Alignment::Center),
        );
    }
    let x = &f.experience;
    exp = exp
        .push(subheading(if x.id.is_empty() { "Add a position" } else { "Change this position" }))
        .push(ui::fact("Title", field("Founder", &x.title, |t| Msg::ExpField(ExpField::Title, t))))
        .push(ui::fact("At (TICKER)", field("An Alelyon organization's ticker", &x.ticker, |t| Msg::ExpField(ExpField::Ticker, t))))
        .push(ui::fact("Or employer", field("An employer's name, as written", &x.org_name, |t| Msg::ExpField(ExpField::OrgName, t))))
        .push(ui::fact("From", field("YYYY-MM", &x.started, |t| Msg::ExpField(ExpField::Started, t))))
        .push(ui::fact("To", field("YYYY-MM, empty while you hold it", &x.ended, |t| Msg::ExpField(ExpField::Ended, t))))
        .push(ui::fact("About", field("What you did (optional)", &x.about, |t| Msg::ExpField(ExpField::About, t))))
        .push(
            row![
                ui::primary("Save position", (!x.title.trim().is_empty() && !x.started.trim().is_empty()).then_some(Msg::SaveExperience)),
                small("Clear", Some(Msg::ProfileClear)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .push(note("An organization's owners or admins can confirm a position that names it by ticker; a change clears that."));
    let mut edu = Column::new();
    for e in &me.profile.education {
        edu = edu.push(
            row![education_row(e), space().width(Length::Fill), small("Edit", Some(Msg::EduEdit(e.clone()))), small("Delete", Some(Msg::DeleteEducation(e.id.clone())))]
                .spacing(6)
                .align_y(Alignment::Center),
        );
    }
    let y = &f.education;
    edu = edu
        .push(subheading(if y.id.is_empty() { "Add a school" } else { "Change this school" }))
        .push(ui::fact("School", field("University of London", &y.school, |t| Msg::EduField(EduField::School, t))))
        .push(ui::fact("Degree", field("BSc (optional)", &y.degree, |t| Msg::EduField(EduField::Degree, t))))
        .push(ui::fact("Field", field("Mathematics (optional)", &y.field, |t| Msg::EduField(EduField::Field, t))))
        .push(ui::fact("From", field("YYYY (optional)", &y.started, |t| Msg::EduField(EduField::Started, t))))
        .push(ui::fact("To", field("YYYY (optional)", &y.ended, |t| Msg::EduField(EduField::Ended, t))))
        .push(
            row![ui::primary("Save school", (!y.school.trim().is_empty()).then_some(Msg::SaveEducation)), small("Clear", Some(Msg::ProfileClear))]
                .spacing(8)
                .align_y(Alignment::Center),
        );
    let skills = Column::new()
        .push(field("Mathematics, Analytical engines", &f.skills, Msg::SkillsText))
        .push(ui::primary("Save skills", f.skills_dirty.then_some(Msg::SaveSkills)))
        .push(note("Up to 50, separated by commas. Others can endorse them; only the totals show."));
    column![ui::card("Experience", exp), ui::card("Education", edu), ui::card("Skills", skills)].spacing(14).into()
}

/// For an organization's owners and admins: the positions that name it, each with Confirm or Take back.
fn org_experience(s: &State) -> El<'_> {
    let Some((ticker, list)) = &s.org_experience else { return space().into() };
    let mut col = Column::new().spacing(10);
    if list.is_empty() && !s.loading {
        col = col.push(note("Nobody lists a position here yet."));
    }
    for (person, e) in list {
        let act = if e.confirmed {
            small("Take back", Some(Msg::ConfirmExperience(ticker.clone(), e.id.clone(), false)))
        } else {
            ui::primary("Confirm", Some(Msg::ConfirmExperience(ticker.clone(), e.id.clone(), true)))
        };
        col = col.push(
            container(column![name_line(person), experience_row(e), act].spacing(6)).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel),
        );
    }
    col.into()
}

// ------------------------------------------------------------------- an organization's page

fn org(s: &State) -> El<'_> {
    let Some(page) = &s.org else { return space().into() };
    let o = &page.org;
    let ticker = o.ticker.clone();
    let mut head = Column::new().spacing(8);
    if let Some(b) = banner(s, page.banner.as_ref()) {
        head = head.push(b);
    }
    let mut title = Row::new().spacing(8).align_y(Alignment::Center);
    if let Some(u) = &o.logo {
        title = title.push(face(s, u, &o.name, 72.0));
    }
    head = head.push(title.push(strong(o.name.as_str(), 20.0, theme::TEXT)).push(label(format!("({})", o.ticker), 16.0, theme::GOLD)));
    if !page.about.is_empty() {
        head = head.push(label(page.about.as_str(), 14.0, theme::TEXT));
    }
    head = head.push(label(format!("{} followers \u{00B7} {} members on show", page.followers, page.members.len()), 12.5, theme::TEXT_DIM));
    let mut acts = Row::new().spacing(6).align_y(Alignment::Center);
    acts = acts.push(if page.you_follow {
        ui::secondary("Following", Some(Msg::FollowOrg(ticker.clone(), false)))
    } else {
        ui::primary("Follow", Some(Msg::FollowOrg(ticker.clone(), true)))
    });
    acts = acts.push(match page.your_state.as_deref() {
        None => small("Ask to join", Some(Msg::Join(ticker.clone()))),
        Some("requested") => small("Withdraw my request", Some(Msg::Leave(ticker.clone()))),
        Some("invited") => small("Accept the invitation", Some(Msg::Join(ticker.clone()))),
        Some(_) => small("Leave", Some(Msg::Leave(ticker.clone()))),
    });
    if page.you_admin() {
        acts = acts.push(small("Members", Some(Msg::Go(Screen::Members(ticker.clone())))));
        acts = acts.push(small("Flagged posts", Some(Msg::Go(Screen::Flagged(ticker.clone())))));
        acts = acts.push(small("Experience to confirm", Some(Msg::Go(Screen::OrgExperience(ticker.clone())))));
    }
    let target = ListTarget::Org(ticker.clone());
    acts = acts.push(lists_button(s, &target, true));
    if moderator(s) {
        acts = acts.push(small("Suspend organization (moderator)", Some(Msg::ModAct(ModAct::SuspendOrg, Some(Target::Org(ticker.clone())), None))));
    }
    acts = acts.push(small("Report", Some(Msg::Reporting(Some(Target::Org(ticker.clone()))))));
    head = head.push(acts);
    if let Some(picker) = list_picker(s, &target) {
        head = head.push(picker);
    }
    if page.you_admin() {
        head = head.push(photo_controls(
            s,
            [(PhotoFor::Org(ticker.clone(), "logo"), "logo", o.logo.is_some()), (PhotoFor::Org(ticker.clone(), "banner"), "banner", page.banner.is_some())],
        ));
    }
    if s.reporting == Some(Target::Org(ticker.clone())) {
        head = head.push(report_reasons(Target::Org(ticker)));
    }
    if !page.members.is_empty() {
        let mut people = Column::new().spacing(4).push(subheading("Members"));
        for m in &page.members {
            people = people.push(name_line(m));
        }
        head = head.push(people);
    }
    column![
        container(head).padding(16).width(Length::Fill).style(theme::panel),
        posts_list(s, &page.posts.posts, page.posts.more, "No posts yet."),
    ]
    .spacing(14)
    .into()
}

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

/// A post's words, each @username a link to that person's page and each #hashtag a link to its posts.
fn post_text<'a>(text: &'a str, size: f32) -> El<'a> {
    let mut spans: Vec<iced::widget::text::Span<'a, String, iced::Font>> = Vec::new();
    for (piece, name) in mention_pieces(text) {
        match name {
            Some(user) => spans.push(span(piece).color(theme::GOLD).link(user.to_lowercase())),
            None => {
                for (words, tag) in tag_pieces(piece) {
                    spans.push(match tag {
                        Some(t) => span(words).color(theme::GOLD).link(format!("#{t}")),
                        None => span(words).color(theme::TEXT),
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

fn notifications(s: &State) -> El<'_> {
    let Some(page) = &s.notifications else { return space().into() };
    let mut col = Column::new().spacing(10);
    if page.items.is_empty() && !s.loading {
        col = col.push(note(
            "Nothing yet. When someone likes, reposts, quotes or replies to your posts, mentions you, follows you, or \
             asks about one of your organizations, it shows here.",
        ));
    }
    for n in &page.items {
        col = col.push(notification_card(n));
    }
    if page.more {
        col = col.push(ui::secondary("Load more", Some(Msg::More)));
    }
    col.into()
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
        _ => format!("{who} did something that concerns you"),
    }
}

/// A community in a sentence: `c/handle`.
fn community_words(c: Option<&Community>) -> String {
    c.map(|c| format!("c/{}", c.handle)).unwrap_or_else(|| "a community".into())
}

fn notification_card<'a>(n: &'a Notification) -> El<'a> {
    let mut head = Row::new().spacing(8).align_y(Alignment::Center);
    if !n.read {
        head = head.push(label("\u{25CF}", 10.0, theme::GOLD));
    }
    head = head.push(strong(notification_words(n), 13.5, theme::TEXT)).push(label(when(&n.at), 11.5, theme::TEXT_FAINT));
    // Where it leads: the post; a request to the members; the organization; the person who followed.
    let go = match (n.kind.as_str(), &n.post, &n.org) {
        ("community-request", _, _) => n.community.as_ref().map(|c| Msg::Go(Screen::CommunityMembers(c.handle.clone()))),
        ("community-joined", _, _) => n.community.as_ref().map(|c| Msg::Go(Screen::Community(c.handle.clone()))),
        ("job-application", Some(p), _) => Some(Msg::Go(Screen::Applicants(p.id.clone()))),
        (_, Some(p), _) => Some(Msg::Go(Screen::Thread(p.id.clone()))),
        ("org-request", None, Some(o)) => Some(Msg::Go(Screen::Members(o.ticker.clone()))),
        (_, None, Some(o)) => Some(Msg::Go(Screen::Org(o.ticker.clone()))),
        _ => n.actor.as_ref().and_then(|a| a.username.clone()).map(|u| Msg::Go(Screen::Person(u))),
    };
    if let Some(msg) = go {
        head = head.push(space().width(Length::Fill)).push(small("Open", Some(msg)));
    }
    let mut col = Column::new().spacing(6).push(head);
    if let Some(p) = &n.post {
        col = col.push(quoted(p));
    }
    container(col).padding([10.0, 14.0]).width(Length::Fill).style(theme::panel).into()
}

// ------------------------------------------------------------------- an organization's flagged posts (owners and admins)

/// What its owners and admins read above the list.
const FLAGGED: &str = "Posts made as this organization whose author's account no longer exists, or whose author's page a \
    moderator suspended. Everyone else sees them as the organization's, without an author. Keep each one, or delete it.";

fn flagged(s: &State) -> El<'_> {
    let Some((_, page)) = &s.flagged else { return space().into() };
    column![note(FLAGGED), posts_list(s, &page.posts, page.more, "No posts by a gone author.")].spacing(10).into()
}

// ------------------------------------------------------------------- a thread

fn thread(s: &State) -> El<'_> {
    let Some(t) = &s.thread else { return space().into() };
    let mut col = Column::new().spacing(10);
    if let Some(parent) = &t.parent {
        col = col.push(label("In reply to", 12.0, theme::TEXT_DIM)).push(post_card(s, parent));
    }
    col = col.push(post_card(s, &t.post));
    let has_page = s.me.as_ref().is_some_and(|m| m.me.has_page);
    if has_page && !t.post.you_can_reply {
        col = col.push(note(match t.post.replies_from {
            ReplyRule::Followers => "Only followers of whoever started the conversation can reply.",
            _ => "Only the people its first post mentions can reply.",
        }));
    } else if has_page {
        // Follows are private on Pages; a reply under this rule tells others one.
        if t.post.replies_from == ReplyRule::Followers {
            col = col.push(note("Followers only: a reply here shows that its writer follows whoever started the conversation."));
        }
        col = col.push(
            row![
                text_input("Reply", &s.reply).on_input(Msg::ReplyText).on_submit(Msg::SendReply).padding(8).size(13).style(ui::input_style),
                ui::primary("Reply", (!s.reply.trim().is_empty() && !s.composer.sending).then_some(Msg::SendReply)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    } else {
        col = col.push(note("Open your page to reply: My page, above."));
    }
    col = col.push(subheading(format!("Replies ({})", t.post.replies)));
    for r in &t.replies.posts {
        col = col.push(post_card(s, r));
    }
    col.into()
}

// ------------------------------------------------------------------- my page

fn mine(s: &State) -> El<'_> {
    let Some(me) = &s.me else { return space().into() };
    if me.suspended {
        return ui::notice(SUSPENDED, theme::CAUTION);
    }
    if !me.me.has_page {
        return container(
            column![
                strong("Your page", 16.0, theme::TEXT),
                label(
                    "A page is public: once it is open, anyone can read your name, title, biography and posts, signed in or \
                     not. Your email address is never shown. You can close it at any time.",
                    13.5,
                    theme::TEXT_DIM,
                ),
                ui::primary("Open my page", Some(Msg::OpenPage)),
            ]
            .spacing(10),
        )
        .padding(16)
        .width(Length::Fill)
        .style(theme::panel)
        .into();
    }
    let m = &s.mine;
    let mut orgs: Vec<TitleOrg> = vec![TitleOrg(None)];
    orgs.extend(me.active().map(|ms| TitleOrg(Some(ms.org.clone()))));
    let mut card = Column::new().spacing(10).push(subheading("How your name reads")).push(match &me.me.avatar {
        Some(u) => row![face(s, u, me.me.name(), 56.0), name_line(&me.me)].spacing(12).align_y(Alignment::Center).into(),
        None => name_line(&me.me),
    });
    if let Some(n) = verified_note(&me.me) {
        card = card.push(n);
    }
    // The service does not say whether the page has a banner here: removing one that is not there changes nothing.
    card = card
        .push(photo_controls(s, [(PhotoFor::Page("avatar"), "photo", me.me.avatar.is_some()), (PhotoFor::Page("banner"), "banner", true)]))
        .push(note(PHOTO_RULE));
    let current = me.me.display_name.as_str();
    let name_ok = social::tidy_display_name(&m.name) == current || social::valid_display_name(&m.name);
    card = card
        .push(ui::fact(
            "Name",
            text_input("Your name", &m.name).on_input(Msg::MineName).padding(8).size(13).width(320).style(ui::input_style),
        ))
        .push(if name_ok { note(NAME_RULE) } else { ui::notice(NAME_RULE, theme::CAUTION) })
        .push(ui::fact(
            "Title",
            text_input("Founder | CEO", &m.title).on_input(Msg::MineTitle).padding(8).size(13).width(320).style(ui::input_style),
        ))
        .push(note(format!(
            "Up to 4 parts separated by |, {} characters in all, without brackets. An organization's owners or admins can \
             confirm a title that names it.",
            pages::TITLE_MAX_CHARS
        )))
        .push(ui::fact(
            "Title is at",
            pick_list(orgs, Some(TitleOrg(m.title_org.clone())), Msg::MineTitleOrg)
                .padding(6)
                .text_size(12.5)
                .font(fonts().ui)
                .style(theme::picker)
                .menu_style(theme::picker_menu),
        ))
        .push(subheading("About you"))
        .push(text_editor(&m.bio).placeholder("A few words about you").on_action(Msg::MineBio).height(Length::Fixed(110.0)).padding(10).size(13.5).font(fonts().ui).style(theme::editor))
        .push(
            row![
                ui::primary(if m.saving { "Saving\u{2026}" } else { "Save" }, (m.dirty && !m.saving && name_ok).then_some(Msg::SaveMine)),
                space().width(Length::Fill),
                small(if m.confirm_close { "Close it: yes" } else { "Close my page" }, Some(Msg::ClosePage)),
            ]
            .align_y(Alignment::Center),
        );
    if m.confirm_close {
        card = card.push(note("Closing hides your page and posts and erases your title and biography. Press again to close it."));
    }
    let mut col = column![container(card).padding(16).width(Length::Fill).style(theme::panel)].spacing(14);
    col = col.push(tickers_card(me.active().collect(), &me.me.tickers));
    col = col.push(
        container(
            column![
                subheading("Your posts and AI"),
                checkbox(me.ai_training).label("Let Alelyon use my public posts to improve its services, including training AI models").on_toggle(Msg::AiTraining),
                note(AI_TRAINING),
            ]
            .spacing(8),
        )
        .padding(16)
        .width(Length::Fill)
        .style(theme::panel),
    );
    col = col.push(profile_editor(s, me));
    if let Some(muted) = s.muted.as_ref().filter(|m| !m.is_empty()) {
        let list = muted.iter().fold(Column::new(), |c, p| {
            c.push(
                row![name_line(p), space().width(Length::Fill), small("Unmute", p.username.clone().map(|u| Msg::Mute(u, false)))]
                    .align_y(Alignment::Center),
            )
        });
        col = col.push(ui::card("Muted", list.push(note("Their posts leave your feeds and their acts your notifications. They are not told."))));
    }
    if let Some(applied) = s.applications.as_ref().filter(|a| !a.is_empty()) {
        let list = applied.iter().fold(Column::new().spacing(6), |c, p| {
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
        col = col.push(ui::card("Your applications", list.push(note(words))));
    }
    if let Some(u) = &me.me.username {
        col = col.push(ui::secondary("See my page as others do", Some(Msg::Go(Screen::Person(u.clone())))));
    }
    col.into()
}

/// What the form says about a name: what it holds, and where a title goes instead.
const NAME_RULE: &str = "Your name only: 1-64 characters, without [ ] ( ). Your title goes below, and the organizations \
    after your name further down.";

/// What the form says about the AI-training choice.
const AI_TRAINING: &str = "On unless you turn it off. Turning it off keeps your public posts out of anything Alelyon builds \
    from then on; what was built before stays as it is.";

/// The organizations after the reader's name: each membership with whether it shows, and the order of those that do.
fn tickers_card<'a>(active: Vec<&'a Membership>, shown: &'a [Org]) -> El<'a> {
    let mut col = Column::new().spacing(8).push(subheading("Organizations after your name")).push(note(
        "Show none, one or several. Those on show read in this order after your name.",
    ));
    if active.is_empty() {
        col = col.push(note("You are not a member of an organization yet: see Organizations, above."));
    }
    for (i, o) in shown.iter().enumerate() {
        col = col.push(
            row![
                checkbox(true).label(format!("{} ({})", o.name, o.ticker)).on_toggle({
                    let t = o.ticker.clone();
                    move |on| Msg::ShowTicker(t.clone(), on)
                }),
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
        col = col.push(checkbox(false).label(format!("{} ({})", m.org.name, m.org.ticker)).on_toggle(move |on| Msg::ShowTicker(t.clone(), on)));
    }
    container(col).padding(16).width(Length::Fill).style(theme::panel).into()
}

// ------------------------------------------------------------------- organizations

fn orgs(s: &State) -> El<'_> {
    let Some(me) = &s.me else { return space().into() };
    let mut list = Column::new().spacing(6).push(subheading("Yours"));
    if me.memberships.is_empty() {
        list = list.push(note("None yet. Find one by its ticker above, or start one below."));
    }
    for m in &me.memberships {
        let t = m.org.ticker.clone();
        let mut r = row![
            button(strong(format!("{} ({})", m.org.name, m.org.ticker), 13.5, theme::TEXT)).padding(0).on_press(Msg::Go(Screen::Org(t.clone()))).style(theme::ghost_button),
            chip(role_words(m), if m.state == "active" { theme::GOLD } else { theme::TEXT_DIM }),
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
        subheading("Start an organization"),
        ui::fact("Ticker", text_input("LACE", &n.ticker).on_input(Msg::NewTicker).padding(8).size(13).width(120).style(ui::input_style)),
        ui::fact("Name", text_input("Lace Labs", &n.name).on_input(Msg::NewName).padding(8).size(13).width(320).style(ui::input_style)),
        ui::fact("About", text_input("What it does", &n.about).on_input(Msg::NewAbout).padding(8).size(13).style(ui::input_style)),
        note("A ticker is 2-5 letters and unique. You become its owner, and it shows after your name until you hide it."),
        ui::primary("Start it", (n.ticker.len() >= 2 && !n.name.trim().is_empty()).then_some(Msg::CreateOrg)),
    ]
    .spacing(10);
    column![
        container(list).padding(16).width(Length::Fill).style(theme::panel),
        container(create).padding(16).width(Length::Fill).style(theme::panel),
    ]
    .spacing(14)
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
    let mut col = Column::new().spacing(10).push(
        row![
            text_input("Invite by username", &s.invite)
                .on_input(Msg::InviteText)
                .on_submit(Msg::Invite(ticker.to_string(), s.invite.clone()))
                .padding(8)
                .size(13)
                .width(260)
                .style(ui::input_style),
            ui::secondary("Invite", (!s.invite.trim().is_empty()).then(|| Msg::Invite(ticker.to_string(), s.invite.clone()))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    let mine = me_id(s);
    let my_role = list.iter().find(|m| m.person.id == mine).map(|m| m.role.as_str()).unwrap_or("member");
    for state in ["requested", "invited", "active"] {
        let rows: Vec<&Member> = list.iter().filter(|m| m.state == state).collect();
        if rows.is_empty() {
            continue;
        }
        col = col.push(subheading(match state {
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
            info = info.push(label(m.role.clone(), 11.5, theme::TEXT_FAINT));
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
    container(row![info, space().width(Length::Fill), acts.wrap()].spacing(10).align_y(Alignment::Center))
        .padding([10.0, 14.0])
        .width(Length::Fill)
        .style(theme::panel)
        .into()
}

// ------------------------------------------------------------------- communities (ADR-0053)

fn community_link<'a>(c: &'a Community) -> El<'a> {
    button(label(format!("c/{}", c.handle), 12.0, theme::GOLD))
        .padding(0)
        .on_press(Msg::Go(Screen::Community(c.handle.clone())))
        .style(theme::ghost_button)
        .into()
}

fn members_words(n: u64) -> String {
    if n == 1 { "1 member".into() } else { format!("{n} members") }
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
        Some(_) => label("Its moderators removed you", 12.0, theme::CAUTION).into(),
        None => ui::primary(
            if c.joining == Joining::Request { "Ask to join" } else { "Join" },
            has_page.then_some(Msg::JoinCommunity(handle)),
        ),
    }
}

/// A community in a list: its name and handle, its members, the reader's part in it, and what it is about.
fn community_row<'a>(s: &'a State, c: &'a CommunitySummary) -> El<'a> {
    let handle = c.community.handle.clone();
    let mut head = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(button(strong(c.community.name.as_str(), 14.0, theme::TEXT)).padding(0).on_press(Msg::Go(Screen::Community(handle))).style(theme::ghost_button))
        .push(label(format!("c/{}", c.community.handle), 12.5, theme::GOLD))
        .push(label(members_words(c.members), 12.0, theme::TEXT_FAINT));
    if let Some(role) = c.your_role.as_deref().filter(|r| c.you_belong() && *r != "member") {
        head = head.push(chip(community_role_title(role), theme::GOLD));
    }
    if c.your_state.as_deref() == Some("requested") {
        head = head.push(chip("Asked to join", theme::TEXT_DIM));
    }
    let mut col = Column::new().spacing(4).push(row![head.wrap(), space().width(Length::Fill), community_action(s, c)].align_y(Alignment::Center));
    if !c.about.is_empty() {
        col = col.push(label(ui::cut(&c.about, 240), 13.0, theme::TEXT_DIM));
    }
    container(col).padding([10.0, 14.0]).width(Length::Fill).style(theme::panel).into()
}

/// The Communities tab: the reader's own, the others with the most members, and starting one.
fn communities(s: &State) -> El<'_> {
    let mut col = Column::new().spacing(12);
    if let Some(mine) = s.my_communities.as_ref().filter(|m| !m.is_empty()) {
        col = col.push(subheading("Yours"));
        for c in mine {
            col = col.push(community_row(s, c));
        }
    }
    if let Some(list) = &s.communities {
        let others: Vec<&CommunitySummary> = list.iter().filter(|c| c.your_state.is_none()).collect();
        col = col.push(subheading("Communities"));
        if others.is_empty() && !s.loading {
            col = col.push(note("No other community yet. Start one below."));
        }
        for c in others {
            col = col.push(community_row(s, c));
        }
    }
    if !s.me.as_ref().is_some_and(|m| m.me.has_page) {
        return col.push(ui::notice("Open your page to join or start a community: My page, above.", theme::GOLD)).into();
    }
    let n = &s.new_community;
    let create = column![
        subheading("Start a community"),
        ui::fact(
            "Handle",
            text_input("engines", &n.handle).on_input(|t| Msg::NewCommunityField(CommunityField::Handle, t)).padding(8).size(13).width(220).style(ui::input_style)
        ),
        ui::fact(
            "Name",
            text_input("Analytical Engines", &n.name).on_input(|t| Msg::NewCommunityField(CommunityField::Name, t)).padding(8).size(13).width(320).style(ui::input_style)
        ),
        ui::fact("About", text_input("What it is for", &n.about).on_input(|t| Msg::NewCommunityField(CommunityField::About, t)).padding(8).size(13).style(ui::input_style)),
        ui::fact(
            "Rules",
            text_input("How members treat each other (optional)", &n.rules)
                .on_input(|t| Msg::NewCommunityField(CommunityField::Rules, t))
                .padding(8)
                .size(13)
                .style(ui::input_style)
        ),
        ui::fact(
            "Joining",
            pick_list(Joining::ALL, Some(n.joining), Msg::NewCommunityJoining)
                .padding(6)
                .text_size(12.5)
                .font(fonts().ui)
                .style(theme::picker)
                .menu_style(theme::picker_menu)
        ),
        note(
            "The handle is its address, c/handle: 3-30 lower-case letters, digits or _, beginning with a letter. A community \
             is public: anyone can read it, its members and its posts. You become its owner."
        ),
        ui::primary("Start it", n.ready().then_some(Msg::CreateCommunity)),
    ]
    .spacing(10);
    col.push(container(create).padding(16).width(Length::Fill).style(theme::panel)).into()
}

/// The edit form of the community on show, for its owners: its name, about, rules and how it is joined.
fn community_editor(form: &NewCommunity) -> El<'_> {
    column![
        ui::fact("Name", text_input("Name", &form.name).on_input(|t| Msg::EditCommunityField(CommunityField::Name, t)).padding(8).size(13).width(320).style(ui::input_style)),
        ui::fact("About", text_input("What it is for", &form.about).on_input(|t| Msg::EditCommunityField(CommunityField::About, t)).padding(8).size(13).style(ui::input_style)),
        ui::fact("Rules", text_input("Rules (optional)", &form.rules).on_input(|t| Msg::EditCommunityField(CommunityField::Rules, t)).padding(8).size(13).style(ui::input_style)),
        ui::fact(
            "Joining",
            pick_list(Joining::ALL, Some(form.joining), Msg::EditCommunityJoining)
                .padding(6)
                .text_size(12.5)
                .font(fonts().ui)
                .style(theme::picker)
                .menu_style(theme::picker_menu)
        ),
        row![ui::primary("Save", (!form.name.trim().is_empty()).then_some(Msg::SaveCommunity)), small("Cancel", Some(Msg::EditCommunity(false)))]
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
        .push(row![strong(c.name.as_str(), 20.0, theme::TEXT), label(format!("c/{}", c.handle), 15.0, theme::GOLD)].spacing(8).align_y(Alignment::Center))
        .push(label(format!("{} \u{00B7} {}", members_words(sum.members), sum.joining.label()), 12.5, theme::TEXT_DIM));
    if !sum.about.is_empty() {
        head = head.push(label(sum.about.as_str(), 14.0, theme::TEXT));
    }
    if !page.rules.is_empty() {
        head = head.push(subheading("Rules")).push(label(page.rules.as_str(), 13.0, theme::TEXT_DIM));
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
    let mut col = column![container(head).padding(16).width(Length::Fill).style(theme::panel)].spacing(14);
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
    let mut col = Column::new().spacing(10);
    for (state, title) in [("requested", "Asking to join"), ("active", "Members"), ("banned", "Kept from joining")] {
        let rows: Vec<&Member> = list.iter().filter(|m| m.state == state).collect();
        if rows.is_empty() {
            continue;
        }
        col = col.push(subheading(title));
        for m in rows {
            col = col.push(community_member_row(handle, m, me, keeper, owner));
        }
    }
    if !keeper {
        col = col.push(note("Members are public. Requests and bans are seen only by the community's owners and moderators."));
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
                info = info.push(label(community_role_title(&m.role), 11.5, theme::TEXT_FAINT));
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
    container(row![info, space().width(Length::Fill), acts.wrap()].spacing(10).align_y(Alignment::Center))
        .padding([10.0, 14.0])
        .width(Length::Fill)
        .style(theme::panel)
        .into()
}

// ------------------------------------------------------------------- lists (ADR-0054)

fn list_counts(l: &PageList) -> String {
    let people = if l.people == 1 { "1 person".to_string() } else { format!("{} people", l.people) };
    let orgs = if l.orgs == 1 { "1 organization".to_string() } else { format!("{} organizations", l.orgs) };
    format!("{people} \u{00B7} {orgs}")
}

/// A list in the Lists tab: its name (which opens it), who is on it, and what it is for.
fn list_row(l: &PageList) -> El<'_> {
    let mut col = Column::new().spacing(4).push(
        row![
            button(strong(l.name.as_str(), 14.0, theme::TEXT)).padding(0).on_press(Msg::Go(Screen::List(l.id.clone()))).style(theme::ghost_button),
            label(list_counts(l), 12.0, theme::TEXT_FAINT),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if !l.about.is_empty() {
        col = col.push(label(l.about.as_str(), 13.0, theme::TEXT_DIM));
    }
    container(col).padding([10.0, 14.0]).width(Length::Fill).style(theme::panel).into()
}

/// The Lists tab: the reader's lists, by name, and making one.
fn lists(s: &State) -> El<'_> {
    let mut col = Column::new().spacing(12);
    if let Some(found) = &s.lists {
        col = col.push(subheading("Your lists"));
        if found.is_empty() && !s.loading {
            col = col.push(note("No list yet. Make one below, then add people and organizations from their pages."));
        }
        for l in found {
            col = col.push(list_row(l));
        }
    }
    let (name, about) = &s.new_list;
    let make = column![
        subheading("Make a list"),
        ui::fact("Name", text_input("Engine people", name).on_input(Msg::NewListName).on_submit(Msg::CreateList).padding(8).size(13).width(320).style(ui::input_style)),
        ui::fact(
            "About",
            text_input("What it is for (optional)", about).on_input(Msg::NewListAbout).on_submit(Msg::CreateList).padding(8).size(13).style(ui::input_style)
        ),
        note(
            "A list is yours alone: nobody else sees it, and nobody on it is told. It shows the posts of the people and \
             organizations on it, newest first, whether or not you follow them."
        ),
        ui::primary("Make it", (!name.trim().is_empty()).then_some(Msg::CreateList)),
    ]
    .spacing(10);
    col.push(container(make).padding(16).width(Length::Fill).style(theme::panel)).into()
}

/// A list: its name, counts and line; who is on it (when asked for), each with Take off; deleting it; and its stream.
fn list(s: &State) -> El<'_> {
    let Some(page) = &s.list else { return space().into() };
    let l = &page.list;
    let mut head = Column::new().spacing(8).push(strong(l.name.as_str(), 20.0, theme::TEXT)).push(label(list_counts(l), 12.5, theme::TEXT_DIM));
    if !l.about.is_empty() {
        head = head.push(label(l.about.as_str(), 14.0, theme::TEXT));
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
    head = head.push(note("Only you see this list, and nobody on it is told."));
    column![
        container(head).padding(16).width(Length::Fill).style(theme::panel),
        posts_list(s, &page.posts.posts, page.posts.more, "No posts yet. The posts of the people and organizations on this list show here."),
    ]
    .spacing(14)
    .into()
}

/// The edit form of the list on show: its name and line.
fn list_editor<'a>(name: &'a str, about: &'a str) -> El<'a> {
    column![
        ui::fact("Name", text_input("Name", name).on_input(Msg::EditListName).on_submit(Msg::SaveList).padding(8).size(13).width(320).style(ui::input_style)),
        ui::fact(
            "About",
            text_input("What it is for (optional)", about).on_input(Msg::EditListAbout).on_submit(Msg::SaveList).padding(8).size(13).style(ui::input_style)
        ),
        row![ui::primary("Save", (!name.trim().is_empty()).then_some(Msg::SaveList)), small("Cancel", Some(Msg::EditList(false)))]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .into()
}

/// Who is on a list, people then organizations, each with Take off.
fn list_members<'a>(list_id: &'a str, m: &'a ListMembers) -> El<'a> {
    if m.people.is_empty() && m.orgs.is_empty() {
        return note("Nobody is on it yet. Add people and organizations from their pages.");
    }
    let take_off = |target: Option<ListTarget>| small("Take off", target.map(|t| Msg::ListTakeOff(list_id.to_string(), t)));
    let mut col = Column::new().spacing(6);
    if !m.people.is_empty() {
        col = col.push(subheading("People"));
        for p in &m.people {
            col = col.push(row![name_line(p), space().width(Length::Fill), take_off(p.username.clone().map(ListTarget::Person))].align_y(Alignment::Center));
        }
    }
    if !m.orgs.is_empty() {
        col = col.push(subheading("Organizations"));
        for o in &m.orgs {
            col = col.push(row![org_button(o), space().width(Length::Fill), take_off(Some(ListTarget::Org(o.ticker.clone())))].align_y(Alignment::Center));
        }
    }
    col.into()
}

/// "Add to list" on a person's or an organization's page; it closes the offer when the offer is open.
fn lists_button<'a>(s: &State, target: &ListTarget, allowed: bool) -> El<'a> {
    if s.list_picker.as_ref().is_some_and(|(t, _)| t == target) {
        small("Close lists", Some(Msg::PickLists(None)))
    } else {
        small("Add to list", allowed.then(|| Msg::PickLists(Some(target.clone()))))
    }
}

/// The reader's lists offered for a person or organization, each with a box that puts it on the list or takes it off.
fn list_picker<'a>(s: &'a State, target: &ListTarget) -> Option<El<'a>> {
    let (shown, found) = s.list_picker.as_ref().filter(|(t, _)| t == target)?;
    let mut col = Column::new().spacing(6).push(subheading("Your lists"));
    if found.is_empty() {
        col = col.push(note("You have no list yet. Make one in Lists, above."));
    }
    for l in found {
        let id = l.id.clone();
        col = col.push(
            checkbox(l.has == Some(true))
                .label(format!("{}  \u{00B7}  {}", l.name, list_counts(l)))
                .on_toggle(move |add| Msg::ListAdd(id.clone(), shown.clone(), add)),
        );
    }
    Some(col.push(note("Nobody is told when you put them on a list.")).into())
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
            button(label(view.title(), 12.5, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([5.0, 12.0])
                .on_press(Msg::ModView(view))
                .style(theme::segment_button(on)),
        );
    }
    let mut col = Column::new().spacing(12).push(views.wrap());
    if matches!(s.mod_view, ModView::Open | ModView::Closed) {
        col = col
            .push(text_input("A note for the log (optional)", &s.mod_note).on_input(Msg::ModNote).padding(8).size(13).style(ui::input_style))
            .push(note(
                "Hiding or suspending closes every open report on the same post, page or organization. Every act is kept in \
                 the moderation log.",
            ));
    }
    let more = match s.mod_view {
        ModView::Matches => {
            let Some(list) = &s.mod_matches else { return col.into() };
            col = col.push(note(MATCHES));
            if list.is_empty() {
                col = col.push(note("No photo matches to act on."));
            }
            for m in list {
                col = col.push(match_card(m));
            }
            false
        }
        ModView::Log => {
            let Some((actions, more)) = &s.mod_log else { return col.into() };
            if actions.is_empty() {
                col = col.push(note("Nothing has been done yet."));
            }
            for a in actions {
                col = col.push(log_row(a));
            }
            *more
        }
        view => {
            let Some((reports, more)) = &s.mod_reports else { return col.into() };
            if reports.is_empty() {
                col = col.push(note(if view == ModView::Open { "No open reports." } else { "No closed reports." }));
            }
            for r in reports {
                col = col.push(report_card(r));
            }
            *more
        }
    };
    if more {
        col = col.push(ui::secondary("Load more", Some(Msg::More)));
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
        .push(chip(m.provider.as_str(), theme::CAUTION))
        .push(label(format!("matched {}", when(&m.matched_at)), 12.0, theme::TEXT_FAINT));
    if let Some(at) = &m.reported_at {
        head = head.push(chip(format!("Reported {}", when(at)), theme::TEXT_DIM));
    }
    let who: El<'_> = match &m.person {
        Some(p) => name_line(p),
        None => note("An account that is no longer available."),
    };
    let mut acts = Row::new().spacing(6).align_y(Alignment::Center);
    if m.reported_at.is_none() {
        acts = acts.push(ui::primary("Mark reported", Some(Msg::MarkMatch(m.id.clone(), true, false))));
    }
    acts = acts.push(ui::secondary("Clear: the account may upload again", Some(Msg::MarkMatch(m.id.clone(), false, true))));
    container(column![head.wrap(), who, acts].spacing(8)).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

fn reason_words(reason: &str) -> &str {
    Reason::ALL.into_iter().find(|r| r.as_str() == reason).map(|r| r.label()).unwrap_or(reason)
}

fn report_card(r: &ModReport) -> El<'_> {
    let mut head = Row::new().spacing(8).align_y(Alignment::Center).push(chip(reason_words(&r.reason), theme::CAUTION));
    if !r.open() {
        head = head.push(chip(if r.state == "actioned" { "Acted on" } else { "Dismissed" }, theme::TEXT_DIM));
    }
    let by = r.reporter.as_ref().map(|p| p.name().to_string()).unwrap_or_else(|| "an unavailable account".into());
    head = head.push(label(format!("reported {} by {by}", when(&r.at)), 12.0, theme::TEXT_FAINT));
    if r.open() && r.open_on_target > 1 {
        head = head.push(label(format!("{} open reports on this", r.open_on_target), 12.0, theme::GOLD));
    }
    let mut col = Column::new().spacing(8).push(head.wrap());
    if !r.note.is_empty() {
        col = col.push(label(format!("\u{201C}{}\u{201D}", r.note), 13.0, theme::TEXT));
    }
    col = col.push(target_box(&r.target));
    let mut acts = Row::new().spacing(6).align_y(Alignment::Center);
    if let Some(act) = ModAct::for_target(&r.target) {
        // an open report offers the act; a closed one only the undo
        let undo = matches!(act, ModAct::RestorePost | ModAct::RestorePage | ModAct::RestoreOrg);
        if r.open() || undo {
            acts = acts.push(ui::primary(act.label(), Some(Msg::ModAct(act, None, Some(r.id.clone())))));
        }
    }
    if r.open() {
        acts = acts.push(ui::secondary("Dismiss", Some(Msg::ModDismiss(r.id.clone()))));
    }
    col = col.push(acts);
    container(col).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

/// What a report names, as a moderator sees it: hidden and suspended things included.
fn target_box(t: &ModTarget) -> El<'_> {
    let mut col = Column::new().spacing(6);
    match t.kind.as_str() {
        "post" => {
            col = col.push(ui::subheading("Post"));
            if let Some(p) = &t.person {
                col = col.push(name_line(p));
            }
            match &t.post {
                Some(post) => {
                    if let Some(title) = &post.title {
                        col = col.push(strong(title.as_str(), 14.0, theme::TEXT));
                    }
                    col = col.push(label(ui::cut(&post.text, 600), 13.5, theme::TEXT));
                    if post.deleted {
                        col = col.push(note("Deleted by its author; kept because others replied or quoted."));
                    }
                }
                None => col = col.push(label(ui::cut(&t.text, 600), 13.5, theme::TEXT)),
            }
        }
        "person" => {
            col = col.push(ui::subheading("Person"));
            col = col.push(match &t.person {
                Some(p) => name_line(p),
                None => note("An account that is no longer available."),
            });
            if !t.text.is_empty() {
                col = col.push(label(ui::cut(&t.text, 400), 13.0, theme::TEXT_DIM));
            }
        }
        "org" => {
            col = col.push(ui::subheading("Organization"));
            if let Some(o) = &t.org {
                col = col.push(strong(format!("{} ({})", o.name, o.ticker), 14.0, theme::TEXT));
            }
            if !t.text.is_empty() {
                col = col.push(label(ui::cut(&t.text, 400), 13.0, theme::TEXT_DIM));
            }
        }
        _ => col = col.push(note("What this report named is not there any more.")),
    }
    let mut chips = Row::new().spacing(6);
    if t.hidden {
        chips = chips.push(chip("Hidden", theme::CAUTION));
    }
    if t.suspended {
        chips = chips.push(chip(if t.kind == "post" { "Its author's page is suspended" } else { "Suspended" }, theme::CAUTION));
    }
    container(col.push(chips)).padding([10.0, 12.0]).width(Length::Fill).style(theme::well).into()
}

fn log_row(a: &ModAction) -> El<'_> {
    let who = if a.by_host {
        "On the identity host,".to_string()
    } else {
        a.moderator.as_ref().map(|p| p.name().to_string()).unwrap_or_else(|| "A moderator".into())
    };
    let mut col = Column::new().spacing(4).push(
        row![
            label(when(&a.at), 11.5, theme::TEXT_FAINT),
            label(format!("{who} {}", a.words()), 13.0, theme::TEXT),
            label(format!("{} {}", a.target_kind, ui::cut(&a.target_id, 20)), 11.5, theme::TEXT_FAINT),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    if !a.note.is_empty() {
        col = col.push(label(format!("\u{201C}{}\u{201D}", a.note), 12.5, theme::TEXT_DIM));
    }
    container(col).padding([8.0, 12.0]).width(Length::Fill).style(theme::panel).into()
}


#[cfg(test)]
mod tests {
    use super::{mention_pieces, notification_words, tag_pieces};
    use alelyon_identity_client::pages::{Notification, Org, Person};

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
