//! What the Pages page draws: its tabs and finder, the feeds with their composer, a post as a card, a person's and an
//! organization's page, a thread, the reader's own page and organizations, and an organization's members.

use alelyon_identity_client::pages::{self, Kind, Member, Membership, Org, Person, Post, Reason, Target};
use iced::widget::{Column, Row, button, checkbox, column, container, pick_list, row, space, text_editor, text_input};
use iced::{Alignment, Element, Length};

use super::{AsWho, Msg, Screen, State, TitleOrg};
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
    for tab in Screen::TABS {
        let on = tab == screen;
        tabs = tabs.push(
            button(label(tab.title(), 13.0, if on { theme::GOLD } else { theme::TEXT_DIM }))
                .padding([7.0, 14.0])
                .on_press(Msg::Go(tab))
                .style(theme::segment_button(on)),
        );
    }
    let finder = text_input("Find @username or (TICKER)", &s.find)
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
    if !Screen::TABS.contains(&screen) {
        page = page.push(row![small("\u{2039} Back", Some(Msg::Back)), strong(screen.title(), 15.0, theme::TEXT)].spacing(10).align_y(Alignment::Center));
    }
    if s.loading {
        page = page.push(ui::working(phase, "Reading\u{2026}"));
    }
    let body: El<'_> = match &screen {
        Screen::Home => home(s),
        Screen::Explore => feed(s, s.explore.as_ref(), "Nothing is posted yet."),
        Screen::Mine => mine(s),
        Screen::Orgs => orgs(s),
        Screen::Person(_) => person(s),
        Screen::Org(_) => org(s),
        Screen::Thread(_) => thread(s),
        Screen::Members(_) => members(s),
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

fn post_card<'a>(s: &'a State, p: &'a Post) -> El<'a> {
    if p.is_plain_repost() {
        let by = column![label(format!("\u{21BB} {} reposted", p.author.name()), 12.0, theme::TEXT_FAINT)];
        return match p.repost_of.as_deref() {
            Some(inner) => by.push(post_card(s, inner)).spacing(4).into(),
            None => by.push(note("The post it reposted is not there any more.")).spacing(4).into(),
        };
    }
    let mine = p.author.id == me_id(s);
    let mut col = Column::new().spacing(8);
    if let Some(o) = &p.org {
        col = col.push(
            row![
                button(strong(format!("{} ({})", o.name, o.ticker), 14.0, theme::GOLD)).padding(0).on_press(Msg::Go(Screen::Org(o.ticker.clone()))).style(theme::ghost_button),
                label(format!("posted by {}", p.author.name()), 12.0, theme::TEXT_FAINT),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    } else {
        col = col.push(name_line(&p.author));
    }
    let mut stamp = when(&p.created_at);
    if p.edited_at.is_some() {
        stamp.push_str(" \u{00B7} edited");
    }
    col = col.push(label(stamp, 11.5, theme::TEXT_FAINT));
    if p.deleted {
        return container(col.push(note("Deleted."))).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into();
    }
    if let Some(title) = &p.title {
        col = col.push(strong(title.as_str(), 17.0, theme::TEXT));
    }
    if !p.text.is_empty() {
        col = col.push(label(p.text.as_str(), 14.0, theme::TEXT));
    }
    if p.repost_of_id.is_some() {
        col = col.push(match p.repost_of.as_deref() {
            Some(inner) => quoted(inner),
            None => note("The quoted post is not there any more."),
        });
    }
    let heart = if p.liked { "\u{2665}" } else { "\u{2661}" };
    let mut actions = row![
        small(format!("Reply {}", p.replies), Some(Msg::Go(Screen::Thread(p.id.clone())))),
        small(format!("Repost {}", p.reposts), (!mine).then(|| Msg::Repost(p.id.clone()))),
        small("Quote", Some(Msg::Quote(p.clone()))),
        small(format!("{heart} {}", p.likes), Some(Msg::Like(p.id.clone(), !p.liked))),
    ]
    .spacing(4);
    actions = actions.push(space().width(Length::Fill));
    if mine {
        actions = actions.push(small("Delete", Some(Msg::Delete(p.id.clone()))));
    } else {
        actions = actions.push(small("Report", Some(Msg::Reporting(Some(Target::Post(p.id.clone()))))));
    }
    col = col.push(actions.align_y(Alignment::Center));
    if s.reporting == Some(Target::Post(p.id.clone())) {
        col = col.push(report_reasons(Target::Post(p.id.clone())));
    }
    container(col).padding([12.0, 16.0]).width(Length::Fill).style(theme::panel).into()
}

/// A quoted post, small and inside the card that quotes it.
fn quoted<'a>(p: &'a Post) -> El<'a> {
    let mut col = column![name_line(&p.author)].spacing(6);
    if let Some(title) = &p.title {
        col = col.push(strong(title.as_str(), 14.0, theme::TEXT));
    }
    col = col.push(label(ui::cut(&p.text, 400), 13.0, theme::TEXT_DIM));
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
    if !me.me.has_page {
        col = col.push(ui::notice("Open your page to post: My page, above.", theme::GOLD));
    } else {
        col = col.push(composer(s));
    }
    col.push(feed(s, s.home.as_ref(), "Your posts, and those of the people and organizations you follow, show here. Explore shows everyone's."))
        .into()
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
    let count = c.chars();
    let over = count > c.limit();
    col = col.push(
        row![
            label(format!("{count} / {}", c.limit()), 11.5, if over { theme::CAUTION } else { theme::TEXT_FAINT }),
            space().width(Length::Fill),
            ui::primary(if c.sending { "Posting\u{2026}" } else { "Post" }, c.ready().then_some(Msg::Submit)),
        ]
        .align_y(Alignment::Center),
    );
    container(col).padding(16).width(Length::Fill).style(theme::panel).into()
}

// ------------------------------------------------------------------- a person's page

fn person(s: &State) -> El<'_> {
    let Some(page) = &s.person else { return space().into() };
    let p = &page.person;
    let mine = p.id == me_id(s);
    let mut head = Column::new().spacing(8).push(name_line(p));
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
        acts = acts.push(small(if page.you_block { "Unblock" } else { "Block" }, Some(Msg::Block(u.clone(), !page.you_block))));
        acts = acts.push(small("Report", Some(Msg::Reporting(Some(Target::Person(u.clone()))))));
        head = head.push(acts.align_y(Alignment::Center));
        if s.reporting == Some(Target::Person(u.clone())) {
            head = head.push(report_reasons(Target::Person(u.clone())));
        }
    }
    column![
        container(head).padding(16).width(Length::Fill).style(theme::panel),
        posts_list(s, &page.posts.posts, page.posts.more, "No posts yet."),
    ]
    .spacing(14)
    .into()
}

// ------------------------------------------------------------------- an organization's page

fn org(s: &State) -> El<'_> {
    let Some(page) = &s.org else { return space().into() };
    let o = &page.org;
    let ticker = o.ticker.clone();
    let mut head = Column::new().spacing(8).push(row![strong(o.name.as_str(), 20.0, theme::TEXT), label(format!("({})", o.ticker), 16.0, theme::GOLD)].spacing(8).align_y(Alignment::Center));
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
    }
    acts = acts.push(small("Report", Some(Msg::Reporting(Some(Target::Org(ticker.clone()))))));
    head = head.push(acts);
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

// ------------------------------------------------------------------- a thread

fn thread(s: &State) -> El<'_> {
    let Some(t) = &s.thread else { return space().into() };
    let mut col = Column::new().spacing(10);
    if let Some(parent) = &t.parent {
        col = col.push(label("In reply to", 12.0, theme::TEXT_DIM)).push(post_card(s, parent));
    }
    col = col.push(post_card(s, &t.post));
    let can_reply = s.me.as_ref().is_some_and(|m| m.me.has_page);
    if can_reply {
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
    let mut card = Column::new().spacing(10).push(subheading("How your name reads")).push(name_line(&me.me));
    if let Some(n) = verified_note(&me.me) {
        card = card.push(n);
    }
    card = card
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
                ui::primary(if m.saving { "Saving\u{2026}" } else { "Save" }, (m.dirty && !m.saving).then_some(Msg::SaveMine)),
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
    if let Some(u) = &me.me.username {
        col = col.push(ui::secondary("See my page as others do", Some(Msg::Go(Screen::Person(u.clone())))));
    }
    col.into()
}

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
