//! Pictures of the Pages screens, drawn on the processor (tiny-skia) from made-up pages, for a person to look at
//! while the screens change. `cargo test --lib pages::snapshots -- --ignored` writes them to
//! `target/pages-snapshots`, or to the folder `PAGES_SNAPSHOTS` names. Nothing is compared: a picture that changed
//! fails nothing.

use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use alelyon_identity_client::pages;
use alelyon_identity_client::pages::{
    Community, CommunityPage, CommunitySummary, Draft, Employment, Experience, Feedback, FeedbackCategory, FeedbackStatus, Job, Joining, Kind,
    Launch, ListPage, Me, Membership, NewPost, Notification, Notifications, Org, OrgPage, PageList, Person, PersonPage, Post, Posts, Product,
    ProductMaker, ProductPage, ProductShow, ProductSummary, Products, Reaction, Stage, Suggestion, Suggestions, Thread, Trending,
    TrendingTag, Why, Workplace,
};
use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::Tree;
use iced::widget::container;
use iced::{Element, Length, Pixels, Rectangle, Size, mouse};

use super::look::{self, Which};
use super::{HomeFeed, Msg, OrgTab, ProductsView, Screen, State};

/// Where the pictures go.
fn folder() -> PathBuf {
    let dir = std::env::var_os("PAGES_SNAPSHOTS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/pages-snapshots"));
    std::fs::create_dir_all(&dir).expect("the pictures' folder");
    dir
}

/// A future that is ready when first asked: tiny-skia's renderer awaits nothing.
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the renderer did not start at once"),
    }
}

/// Draws `page` as the window lays Pages out (to its edges, beside the app's rail of 84 and its hairline) in a window
/// `window` wide and `height` high, and writes it to `name`.png.
fn draw(page: Element<'_, Msg>, window: f32, height: f32, name: &str) -> PathBuf {
    let width = window - 85.0;
    lattice_app::theme::set_fonts(lattice_app::theme::Fonts::detect());
    crate::theme::set_display(crate::theme::detect_display());
    let mut renderer =
        ready(<iced::Renderer as Headless>::new(crate::theme::fonts().ui, Pixels(14.0), Some("tiny-skia"))).expect("the tiny-skia renderer");
    let theme = crate::theme::theme();
    let mut root: Element<'_, Msg> = container(page).width(Length::Fill).height(Length::Fill).into();
    let mut tree = Tree::new(root.as_widget());
    let node = root.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, Size::new(width, height)));
    let size = Size::new(width.ceil(), height.ceil());
    let viewport = Rectangle::with_size(size);
    // The event every frame begins with: a button learns from it whether it can be pressed, and draws so.
    let mut messages = Vec::new();
    let mut shell = iced::advanced::Shell::new(&mut messages);
    root.as_widget_mut().update(
        &mut tree,
        &iced::Event::Window(iced::window::Event::RedrawRequested(std::time::Instant::now())),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut iced::advanced::clipboard::Null,
        &mut shell,
        &viewport,
    );
    renderer.reset(viewport);
    let palette = look::palette();
    let style = renderer::Style { text_color: palette.text };
    root.as_widget().draw(&tree, &mut renderer, &theme, &style, Layout::new(&node), mouse::Cursor::Unavailable, &viewport);
    // what floats over the page: an open menu (`popover`)
    if let Some(mut over) = root.as_widget_mut().overlay(&mut tree, Layout::new(&node), &renderer, &viewport, iced::Vector::ZERO) {
        let over_node = over.as_overlay_mut().layout(&renderer, size);
        renderer.with_layer(viewport, |renderer| {
            over.as_overlay().draw(renderer, &theme, &style, Layout::new(&over_node), mouse::Cursor::Unavailable)
        });
    }
    let (w, h) = (size.width as u32, size.height as u32);
    let rgba = renderer.screenshot(Size::new(w, h), 1.0, palette.canvas);
    let to = folder().join(format!("{name}.png"));
    write_png(&rgba, w, h, &to);
    to
}

fn write_png(rgba: &[u8], width: u32, height: u32, to: &Path) {
    let file = std::io::BufWriter::new(std::fs::File::create(to).expect("the picture's file"));
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut w| w.write_image_data(rgba)).expect("the picture written");
}

// ------------------------------------------------------------------- made-up pages

/// RFC 3339, `seconds` before now.
fn ago(seconds: f64) -> String {
    let full = crate::utc::full(crate::utc::now() - seconds);
    format!("{}T{}", &full[..10], &full[11..])
}

fn org(ticker: &str, name: &str) -> Org {
    Org { id: format!("org_{ticker}"), ticker: ticker.into(), name: name.into(), ..Org::default() }
}

fn person(username: &str, name: &str, title: &str, at: Option<Org>) -> Person {
    Person {
        id: format!("acc_{username}"),
        username: Some(username.into()),
        display_name: name.into(),
        title: title.into(),
        title_verified: at.is_some(),
        tickers: at.iter().cloned().collect(),
        title_org: at,
        has_page: true,
        ..Person::default()
    }
}

/// A home feed with what a feed usually holds: a launch with reactions, an organization's post, a poll, a quote
/// and a plain repost.
fn home() -> State {
    let lace = org("LACE", "Lace Labs");
    let ada = person("ada", "Ada Lovelace", "Founder | CEO", Some(lace.clone()));
    let bob = person("bob", "Bob Babbage", "Engineer", None);
    let carol = person("carol", "Carol Shaw", "Game developer", None);
    let launch = Post {
        id: "pst_launch".into(),
        author: Some(ada.clone()),
        text: "Shipped v0.4 of our query planner today: joins are 3x faster on the TPC-H suite, and EXPLAIN finally \
               reads like English. Feedback welcome, especially on the new plan view. #rust #databases"
            .into(),
        created_at: ago(2.0 * 3600.0),
        likes: 42,
        replies: 7,
        reposts: 5,
        reactions: vec![(Reaction::Like, 30), (Reaction::Celebrate, 9), (Reaction::Insightful, 3)],
        ..Post::default()
    };
    let from_org = Post {
        id: "pst_org".into(),
        author: Some(ada.clone()),
        org: Some(lace.clone()),
        text: "Lace Labs is in public beta. If you run Postgres in production, we would like you to try the planner \
               and tell us where it is wrong."
            .into(),
        created_at: ago(5.0 * 3600.0),
        likes: 18,
        replies: 2,
        reposts: 3,
        ..Post::default()
    };
    let poll = Post {
        id: "pst_poll".into(),
        author: Some(bob.clone()),
        text: "When a build is slow, what do you reach for first?".into(),
        created_at: ago(9.0 * 3600.0),
        likes: 6,
        replies: 11,
        poll: Some(Box::new(alelyon_identity_client::pages::Poll {
            options: vec!["Profile it".into(), "Read the logs".into(), "Ask the team".into()],
            votes: Some(vec![31, 12, 5]),
            total: 48,
            closes_at: ago(-20.0 * 3600.0),
            closed: false,
            you_voted: Some(0),
        })),
        ..Post::default()
    };
    let quote = Post {
        id: "pst_quote".into(),
        author: Some(carol.clone()),
        text: "The clearest EXPLAIN output I have seen in years.".into(),
        created_at: ago(26.0 * 3600.0),
        likes: 3,
        repost_of_id: Some(launch.id.clone()),
        repost_of: Some(Box::new(launch.clone())),
        ..Post::default()
    };
    let plain = Post {
        id: "pst_repost".into(),
        author: Some(carol),
        created_at: ago(30.0 * 3600.0),
        repost_of_id: Some(poll.id.clone()),
        repost_of: Some(Box::new(poll.clone())),
        ..Post::default()
    };
    let me = Me { me: person("thomas", "Thomas Lacy", "Founder", None), ..Me::default() };
    let tag = |tag: &str, posts: u64, people: u64| TrendingTag { tag: tag.into(), posts, people };
    let dan = person("dan", "Dan Ingalls", "Researcher", None);
    State {
        me: Some(me),
        screen: Some(Screen::Home),
        home_feed: HomeFeed::Following,
        home: Some(Posts { posts: vec![launch, from_org, poll, quote, plain], more: true }),
        trending: Some(Trending { hours: 24, tags: vec![tag("rust", 41, 23), tag("databases", 17, 9), tag("launch", 12, 11), tag("wasm", 8, 6)] }),
        suggestions: Some(Suggestions {
            people: vec![
                Suggestion { person: dan, why: Why::Popular, org: None },
                Suggestion { person: person("grace", "Grace Hopper", "Compiler engineer", None), why: Why::Replied, org: None },
            ],
            orgs: vec![org("WEFT", "Weft Systems")],
        }),
        ..State::default()
    }
}

/// Notifications as they arrive: three reactions to one post (one celebrating), a reply, two follows, a mention and
/// a repost; the newest unread.
fn notifications() -> State {
    let base = home();
    let posts = base.home.clone().map(|h| h.posts).unwrap_or_default();
    let launch = posts.first().cloned().unwrap_or_default();
    let bob = person("bob", "Bob Babbage", "Engineer", None);
    let carol = person("carol", "Carol Shaw", "Game developer", None);
    let grace = person("grace", "Grace Hopper", "Compiler engineer", None);
    let dan = person("dan", "Dan Ingalls", "Researcher", None);
    let erin = person("erin", "Erin Catto", "Physics engines", None);
    let told = |id: &str, kind: &str, who: &Person, post: Option<&Post>, hours: f64, read: bool| Notification {
        id: id.into(),
        kind: kind.into(),
        actor: Some(who.clone()),
        post: post.cloned(),
        at: ago(hours * 3600.0),
        read,
        ..Notification::default()
    };
    let reply = Post {
        id: "pst_reply".into(),
        author: Some(grace.clone()),
        text: "Does the plan view show which index each join used? That is the first thing I would look for.".into(),
        created_at: ago(1.5 * 3600.0),
        likes: 2,
        ..Post::default()
    };
    let mut celebrate = told("n2", "like", &carol, Some(&launch), 1.2, false);
    celebrate.detail = Some("celebrate".into());
    let items = vec![
        told("n1", "like", &bob, Some(&launch), 1.0, false),
        celebrate,
        told("n3", "reply", &grace, Some(&reply), 1.5, false),
        told("n4", "like", &dan, Some(&launch), 2.0, true),
        told("n5", "follow", &erin, None, 3.0, true),
        told("n6", "follow", &grace, None, 4.0, true),
        told("n7", "mention", &bob, Some(&reply), 6.0, true),
        told("n8", "repost", &carol, Some(&launch), 8.0, true),
    ];
    State { screen: Some(Screen::Notifications), notifications: Some(Notifications { items, more: false, unread: 3 }), ..base }
}

/// A conversation: the launch, opened, with two replies.
fn conversation() -> State {
    let base = home();
    let posts = base.home.clone().map(|h| h.posts).unwrap_or_default();
    let mut launch = posts.first().cloned().unwrap_or_default();
    launch.you_can_reply = true;
    let grace = person("grace", "Grace Hopper", "Compiler engineer", None);
    let bob = person("bob", "Bob Babbage", "Engineer", None);
    let replies = vec![
        Post {
            id: "pst_r1".into(),
            author: Some(grace),
            reply_to: Some(launch.id.clone()),
            text: "Does the plan view show which index each join used? That is the first thing I would look for.".into(),
            created_at: ago(1.5 * 3600.0),
            likes: 2,
            ..Post::default()
        },
        Post {
            id: "pst_r2".into(),
            author: Some(bob),
            reply_to: Some(launch.id.clone()),
            text: "Ran it against our nightly reports: 41 minutes down to 13. Thank you.".into(),
            created_at: ago(1.0 * 3600.0),
            likes: 5,
            replies: 1,
            ..Post::default()
        },
    ];
    State {
        screen: Some(Screen::Thread(launch.id.clone())),
        thread: Some(Thread { post: launch, parent: None, replies: Posts { posts: replies, more: false } }),
        ..base
    }
}

/// Ada's page, as Thomas sees it: her title confirmed at Lace Labs, her words, her posts.
fn ada_page() -> State {
    let base = home();
    let posts = base.home.clone().map(|h| h.posts).unwrap_or_default();
    let ada = posts.first().and_then(|p| p.author.clone()).unwrap_or_default();
    State {
        screen: Some(Screen::Person("ada".into())),
        person: Some(PersonPage {
            person: ada,
            bio: "Building a query planner that explains itself. Previously: tables for the Analytical Engine.".into(),
            followers: 1_284,
            following: 312,
            posts: Posts { posts: posts.iter().take(2).cloned().collect(), more: false },
            ..PersonPage::default()
        }),
        ..base
    }
}

/// Lace Labs' page on its People tab.
fn lace_page() -> State {
    let base = home();
    let posts = base.home.clone().map(|h| h.posts).unwrap_or_default();
    let ada = posts.first().and_then(|p| p.author.clone()).unwrap_or_default();
    let lace = org("LACE", "Lace Labs");
    State {
        screen: Some(Screen::Org("LACE".into())),
        org: Some(OrgPage {
            org: lace,
            about: "We build databases that tell you what they are doing. Query planning, observability and the tools around them.".into(),
            created_at: ago(30.0 * 86_400.0),
            followers: 2_410,
            members: vec![ada, person("bob", "Bob Babbage", "Engineer", None)],
            posts: Posts { posts: posts.iter().skip(1).take(1).cloned().collect(), more: false },
            ..OrgPage::default()
        }),
        org_tab: OrgTab::People,
        ..base
    }
}

/// Communities: two of the reader's (one they own), two others.
fn communities_screen() -> State {
    let summary = |handle: &str, name: &str, about: &str, members: u64, role: Option<&str>| CommunitySummary {
        community: Community { id: format!("com_{handle}"), handle: handle.into(), name: name.into() },
        about: about.into(),
        members,
        your_role: role.map(Into::into),
        your_state: role.map(|_| "active".to_string()),
        ..CommunitySummary::default()
    };
    let mine = vec![
        summary("databases", "Databases", "Query planners, storage engines and the people who tune them.", 1_284, Some("owner")),
        summary("rustaceans", "Rustaceans", "Rust, from the first borrow to unsafe.", 18_400, Some("member")),
    ];
    let others = vec![
        summary("indiedevs", "Indie developers", "Shipping alone or in twos: launches, feedback and the numbers.", 3_920, None),
        CommunitySummary { joining: Joining::Request, ..summary("ml-infra", "ML infrastructure", "Training runs, inference serving, the bills.", 2_110, None) },
    ];
    State { screen: Some(Screen::Communities), my_communities: Some(mine.clone()), communities: Some([mine, others].concat()), ..home() }
}

/// A community's page, as its owner sees it, with a post in it.
fn community_screen() -> State {
    let base = communities_screen();
    let summary = base.my_communities.clone().unwrap_or_default().remove(0);
    let posts = base.home.clone().map(|h| h.posts).unwrap_or_default();
    State {
        screen: Some(Screen::Community("databases".into())),
        community: Some(CommunityPage {
            summary,
            rules: "Be specific: name the database, the version and what you measured. No vendor pitches.".into(),
            created_at: ago(90.0 * 86_400.0),
            posts: Posts { posts: posts.into_iter().take(1).collect(), more: false },
            requests: Some(2),
        }),
        ..base
    }
}

/// The reader's lists, and one of them open.
fn lists_screen() -> State {
    let list = |id: &str, name: &str, about: &str, people: u64, orgs: u64| PageList {
        id: id.into(),
        name: name.into(),
        about: about.into(),
        people,
        orgs,
        ..PageList::default()
    };
    State {
        screen: Some(Screen::Lists),
        lists: Some(vec![list("lst_1", "Database people", "Planners, storage and the people who tune them", 12, 3), list("lst_2", "Launch watch", "", 40, 9)]),
        ..home()
    }
}

fn list_screen() -> State {
    let base = lists_screen();
    let first = base.lists.clone().unwrap_or_default().remove(0);
    let posts = base.home.clone().map(|h| h.posts).unwrap_or_default();
    State {
        screen: Some(Screen::List(first.id.clone())),
        list: Some(ListPage { list: first, posts: Posts { posts: posts.into_iter().take(2).collect(), more: false } }),
        ..base
    }
}

/// Drafts: one scheduled, one kept, one that could not be posted at its time.
fn drafts_screen() -> State {
    let draft = |id: &str, text: &str, at: Option<String>, failed: Option<&str>| Draft {
        id: id.into(),
        post: NewPost { text: text.into(), ..NewPost::default() },
        publish_at: at,
        failed: failed.map(|w| ("rate".to_string(), w.to_string())),
        updated_at: ago(3_600.0),
        ..Draft::default()
    };
    State {
        screen: Some(Screen::Drafts),
        drafts: Some(vec![
            draft("drf_1", "v0.5 is out: the planner now says why it chose each join, in words. #rust", Some(ago(-20.0 * 3_600.0)), None),
            draft("drf_2", "Thread: how we cut our CI from 41 minutes to 13, and what we stopped testing.", None, None),
            draft("drf_3", "Launch day!", None, Some("You posted too often just then.")),
        ]),
        ..home()
    }
}

/// Open jobs: two at Lace Labs.
fn jobs_screen() -> State {
    let lace = org("LACE", "Lace Labs");
    let ada = person("ada", "Ada Lovelace", "Founder | CEO", Some(lace.clone()));
    let job = |id: &str, title: &str, text: &str, location: &str, workplace: Workplace, pay: &str| Post {
        id: id.into(),
        kind: Kind::Article,
        author: Some(ada.clone()),
        org: Some(lace.clone()),
        title: Some(title.into()),
        text: text.into(),
        created_at: ago(2.0 * 86_400.0),
        job: Some(Box::new(Job {
            location: location.into(),
            workplace,
            employment: Employment::FullTime,
            pay: pay.into(),
            closes_at: ago(-14.0 * 86_400.0),
            ..Job::default()
        })),
        ..Post::default()
    };
    State {
        screen: Some(Screen::Jobs),
        jobs: Some(Posts {
            posts: vec![
                job("pst_j1", "Query engine engineer", "Work on the planner's cost model with the people who wrote it.", "London", Workplace::Hybrid, "£95k-£120k"),
                job("pst_j2", "Developer advocate", "Write the guides, run the office hours, read every issue.", "", Workplace::Remote, ""),
            ],
            more: false,
        }),
        ..home()
    }
}

/// The reader's own page and organizations.
fn mine_screen() -> State {
    let base = home();
    let lace = org("LACE", "Lace Labs");
    let mut me = base.me.clone().unwrap_or_default();
    me.memberships = vec![Membership { org: lace.clone(), role: "owner".into(), state: "active".into(), shown: true, ..Membership::default() }];
    me.profile.experience = vec![Experience {
        id: "exp_1".into(),
        title: "Founder".into(),
        org: Some(lace),
        org_name: "Lace Labs".into(),
        started: "2024-01".into(),
        confirmed: true,
        ..Experience::default()
    }];
    me.profile.skills = vec![
        pages::Skill { skill: "Rust".into(), endorsements: 12, ..pages::Skill::default() },
        pages::Skill { skill: "Query planning".into(), ..pages::Skill::default() },
    ];
    // The page as others see it: what the top's totals read.
    let top = PersonPage { person: me.me.clone(), followers: 1_284, following: 96, ..PersonPage::default() };
    let mut s = State {
        screen: Some(Screen::Mine),
        mine: super::Mine {
            name: "Thomas Lacy".into(),
            title: "Founder | CEO".into(),
            bio: iced::widget::text_editor::Content::with_text("Building Alelyon."),
            dirty: true,
            top: Some(top),
            ..super::Mine::default()
        },
        me: Some(me),
        ..base
    };
    s.profile_form.skills = vec!["Rust".into(), "Query planning".into()];
    s
}

/// My page's Background with the position form open, a month it would refuse, and the skills as chips.
fn mine_background() -> State {
    let mut s = mine_screen();
    s.mine.tab = super::MineTab::Background;
    s.profile_form.exp_open = true;
    s.profile_form.experience.title = "Engineer".into();
    s.profile_form.experience.org_name = "Babbage & Co".into();
    s.profile_form.experience.started = "2021-13".into();
    s.profile_form.skill_new = "Postgres".into();
    s
}

fn orgs_screen() -> State {
    State { screen: Some(Screen::Orgs), ..mine_screen() }
}

/// A made-up photo: a soft gradient `w` by `h`, in `hue`'s colours.
fn made_photo(w: u32, h: u32, hue: [u8; 3]) -> iced::widget::image::Handle {
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            for c in hue {
                pixels.push((f32::from(c) * (0.35 + 0.65 * t)) as u8);
            }
            pixels.push(255);
        }
    }
    iced::widget::image::Handle::from_rgba(w, h, pixels)
}

/// A launch with two photos at the top of Home, both drawn.
fn photos_home() -> State {
    let mut base = home();
    let ada = base.home.as_ref().and_then(|h| h.posts.first()).and_then(|p| p.author.clone());
    let shots = vec![
        alelyon_identity_client::pages::Photo { url: "https://media.alelyon.com/plan.webp".into(), width: 1600, height: 1000, alt: "The plan view: each join with the index it used.".into() },
        alelyon_identity_client::pages::Photo { url: "https://media.alelyon.com/bench.webp".into(), width: 1600, height: 1000, alt: "TPC-H times before and after.".into() },
    ];
    for (shot, hue) in shots.iter().zip([[90, 140, 220], [230, 190, 100]]) {
        base.photos.insert(shot.url.clone(), super::photos::Picture::Ready(made_photo(320, 200, hue)));
    }
    let launch = Post {
        id: "pst_photos".into(),
        author: ada,
        text: "The plan view, and what it did to our nightly reports.".into(),
        created_at: ago(40.0 * 60.0),
        likes: 64,
        replies: 9,
        reposts: 12,
        media: shots,
        ..Post::default()
    };
    if let Some(home) = base.home.as_mut() {
        home.posts.insert(0, launch);
    }
    base
}

/// Explore: what is trending over everyone's latest posts.
fn explore_screen() -> State {
    let base = home();
    State { screen: Some(Screen::Explore), explore: base.home.clone(), ..base }
}

fn product(handle: &str, name: &str) -> Product {
    Product { id: format!("prd_{handle}"), handle: handle.into(), name: name.into(), logo: None }
}

/// Products: the day's launches, the most supported first; the reader supported the first.
fn products_screen() -> State {
    let lace = org("LACE", "Lace Labs");
    let launch = |id: &str, who: Person, made: Product, version: &str, text: &str, supports: u64, you: bool| Post {
        id: id.into(),
        author: Some(who),
        text: text.into(),
        created_at: ago(3.0 * 3600.0),
        product: Some(made),
        launch: Some(Box::new(Launch {
            version: version.into(),
            major: 1,
            launched_at: ago(3.0 * 3600.0),
            ends_at: ago(-21.0 * 3600.0),
            live: true,
            supports,
            you_support: you,
        })),
        likes: supports / 2,
        replies: 4,
        ..Post::default()
    };
    let launches = vec![
        launch(
            "pst_l1",
            person("ada", "Ada Lovelace", "Founder | CEO", Some(lace)),
            product("query-planner", "Query Planner"),
            "v2.0",
            "Plans that explain themselves: every join with the index it used, in words.",
            214,
            true,
        ),
        launch(
            "pst_l2",
            person("grace", "Grace Hopper", "Compiler engineer", None),
            product("cobalt", "Cobalt"),
            "1.0",
            "A compiler explorer for the languages your team ships.",
            128,
            false,
        ),
        launch("pst_l3", person("erin", "Erin Catto", "Physics engines", None), product("box-two", "Box Two"), "3.1", "Rigid bodies at 240 Hz on a laptop.", 77, false),
        launch("pst_l4", person("dan", "Dan Ingalls", "Researcher", None), product("morphic", "Morphic Live"), "0.9", "Live objects in the browser.", 12, false),
    ];
    State { screen: Some(Screen::Products), launches_today: Some(launches), ..home() }
}

/// Query Planner's page, as a follower sees it, on its feedback board.
fn product_screen() -> State {
    let base = products_screen();
    let lace = org("LACE", "Lace Labs");
    let ada = person("ada", "Ada Lovelace", "Founder | CEO", Some(lace.clone()));
    let bob = person("bob", "Bob Babbage", "Engineer", None);
    let planner = product("query-planner", "Query Planner");
    let feedback = |id: &str, who: Person, text: &str, category: FeedbackCategory, status: FeedbackStatus, votes: u64, you: bool| Post {
        id: id.into(),
        author: Some(who),
        text: text.into(),
        created_at: ago(26.0 * 3600.0),
        product: Some(planner.clone()),
        feedback: Some(Box::new(Feedback { category, status, votes, you_voted: you, status_at: None })),
        replies: 3,
        ..Post::default()
    };
    let posts = vec![
        feedback(
            "pst_f1",
            person("grace", "Grace Hopper", "Compiler engineer", None),
            "Show the estimated rows beside the actual rows for each join, so a bad estimate stands out.",
            FeedbackCategory::Idea,
            FeedbackStatus::Planned,
            41,
            true,
        ),
        feedback(
            "pst_f2",
            bob.clone(),
            "EXPLAIN on a view with a lateral join stops the plan view:\n```\nERROR: plan node 7 has no parent\n```",
            FeedbackCategory::Bug,
            FeedbackStatus::InProgress,
            17,
            false,
        ),
        feedback("pst_f3", person("erin", "Erin Catto", "Physics engines", None), "Does it read MySQL's plans too?", FeedbackCategory::Question, FeedbackStatus::Open, 6, false),
    ];
    let latest = base.launches_today.clone().unwrap_or_default().remove(0);
    State {
        screen: Some(Screen::Product("query-planner".into())),
        product: Some(ProductPage {
            summary: ProductSummary {
                product: planner,
                tagline: "A query planner that explains itself".into(),
                stage: Stage::Launched,
                org: Some(lace),
                followers: 1_930,
                rating: Some(4.6),
                reviews: 38,
                tech: vec!["Rust".into(), "PostgreSQL".into(), "WebAssembly".into()],
                launched_at: Some(ago(3.0 * 3600.0)),
                you_follow: true,
                ..ProductSummary::default()
            },
            about: "Point it at a Postgres database and it draws each plan as a tree you can read: which index each join \
                    used, where the rows went, and why the planner chose what it did."
                .into(),
            website: "https://planner.example".into(),
            source: "https://code.example/lace/planner".into(),
            platforms: vec!["web".into(), "linux".into(), "macos".into(), "cli".into()],
            makers: vec![
                ProductMaker { person: ada, active: true, since: ago(90.0 * 86_400.0) },
                ProductMaker { person: bob, active: true, since: ago(30.0 * 86_400.0) },
            ],
            created_at: ago(120.0 * 86_400.0),
            stars: [1, 1, 2, 9, 25],
            latest_launch: Some(Box::new(latest)),
            show: ProductShow::Feedback,
            posts: Posts { posts, more: false },
            ..ProductPage::default()
        }),
        product_show: ProductShow::Feedback,
        ..base
    }
}

/// The same page on its reviews.
fn product_reviews() -> State {
    let base = product_screen();
    let planner = product("query-planner", "Query Planner");
    let review = |id: &str, who: Person, stars: u8, text: &str| Post {
        id: id.into(),
        author: Some(who),
        text: text.into(),
        created_at: ago(4.0 * 86_400.0),
        product: Some(planner.clone()),
        stars: Some(stars),
        likes: 9,
        ..Post::default()
    };
    let posts = vec![
        review("pst_v1", person("grace", "Grace Hopper", "Compiler engineer", None), 5, "Found a missing index in our billing job in ten minutes."),
        review("pst_v2", person("dan", "Dan Ingalls", "Researcher", None), 4, "Clear and fast. I miss MySQL."),
    ];
    let page = base.product.clone().map(|page| ProductPage { show: ProductShow::Reviews, posts: Posts { posts, more: false }, ..page });
    State { product: page, product_show: ProductShow::Reviews, ..base }
}

/// Products, on Yours, with nothing yet: starting one.
fn products_start() -> State {
    State {
        products_view: ProductsView::Yours,
        product_list: Some((ProductsView::Yours, Products::default())),
        ..products_screen()
    }
}

#[test]
#[ignore = "draws pictures for a person to look at; run with --ignored"]
fn draw_the_pages_screens() {
    // Home in each palette, side by side, to choose the look between them
    for which in Which::ALL {
        look::show(which);
        let s = State { width: Some(1280.0), ..home() };
        let to = draw(super::view(&s, 0.0), 1280.0, 900.0, &format!("{}-home-1280", which.palette().name.to_lowercase()));
        println!("{}", to.display());
    }
    look::show(Which::Onyx);
    let mut shots: Vec<(State, f32, &str)> = Vec::new();
    for (window, name) in [(1180.0, "home-1180"), (900.0, "home-900")] {
        shots.push((State { width: Some(window), ..home() }, window, name));
    }
    shots.push((State { width: Some(1280.0), post_menu: Some("pst_launch".into()), ..home() }, 1280.0, "home-menu-open"));
    shots.push((State { width: Some(1280.0), reacting: Some("pst_org".into()), ..home() }, 1280.0, "home-reactions-open"));
    shots.push((State { width: Some(1280.0), selected: Some("pst_org".into()), ..home() }, 1280.0, "home-key-chosen"));
    shots.push((State { width: Some(1280.0), ..photos_home() }, 1280.0, "home-photos"));
    shots.push((State { width: Some(1280.0), viewing: Some(("pst_photos".into(), 1)), ..photos_home() }, 1280.0, "photo-viewer"));
    shots.push((State { width: Some(1280.0), loading: true, home: None, ..home() }, 1280.0, "home-first-read"));
    let mut typing = home();
    typing.composer.body = iced::widget::text_editor::Content::with_text("Thanks @gr");
    typing.composer.body.perform(iced::widget::text_editor::Action::Move(iced::widget::text_editor::Motion::DocumentEnd));
    typing.mentioning = Some(super::Mentioning {
        line: 0,
        start: 7,
        end: 10,
        word: "gr".into(),
        found: vec![person("grace", "Grace Hopper", "Compiler engineer", None), person("greg", "Greg Chen", "Designer", None)],
        at: 0,
    });
    shots.push((State { width: Some(1280.0), ..typing }, 1280.0, "composer-mention"));
    shots.push((State { width: Some(1280.0), ..notifications() }, 1280.0, "notifications"));
    shots.push((State { width: Some(1280.0), ..conversation() }, 1280.0, "conversation"));
    shots.push((State { width: Some(1280.0), ..ada_page() }, 1280.0, "person"));
    shots.push((State { width: Some(1280.0), page_menu: true, ..ada_page() }, 1280.0, "person-menu-open"));
    let followed = ada_page();
    let followed = State { person: followed.person.clone().map(|p| PersonPage { you_follow: true, ..p }), ..followed };
    shots.push((State { width: Some(1280.0), ..followed }, 1280.0, "person-followed"));
    shots.push((State { width: Some(1280.0), ..lace_page() }, 1280.0, "organization-people"));
    shots.push((State { width: Some(1280.0), ..explore_screen() }, 1280.0, "explore"));
    shots.push((State { width: Some(1280.0), ..communities_screen() }, 1280.0, "communities"));
    shots.push((State { width: Some(1280.0), ..community_screen() }, 1280.0, "community"));
    shots.push((State { width: Some(1280.0), ..lists_screen() }, 1280.0, "lists"));
    shots.push((State { width: Some(1280.0), ..list_screen() }, 1280.0, "list"));
    shots.push((State { width: Some(1280.0), ..drafts_screen() }, 1280.0, "drafts"));
    shots.push((State { width: Some(1280.0), ..jobs_screen() }, 1280.0, "jobs"));
    shots.push((State { width: Some(1280.0), ..mine_screen() }, 1280.0, "my-page"));
    shots.push((State { width: Some(1280.0), ..mine_background() }, 1280.0, "my-page-background"));
    shots.push((State { width: Some(1280.0), ..orgs_screen() }, 1280.0, "organizations"));
    shots.push((State { width: Some(1280.0), ..products_screen() }, 1280.0, "products-today"));
    shots.push((State { width: Some(1280.0), ..products_start() }, 1280.0, "products-start"));
    for (s, window, name) in &shots {
        let to = draw(super::view(s, 0.0), *window, 900.0, name);
        println!("{}", to.display());
    }
    // a product's page is long: drawn taller, to its board
    for (s, name) in [(product_screen(), "product-feedback"), (product_reviews(), "product-reviews")] {
        let s = State { width: Some(1280.0), ..s };
        let to = draw(super::view(&s, 0.0), 1280.0, 1700.0, name);
        println!("{}", to.display());
    }
}
