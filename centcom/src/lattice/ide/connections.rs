//! The Connections page: the online services the agent can use for you, by computer use (it connects local models to
//! online services such as Google, Amazon, Instagram and X; Sinai relies on computer use to operate these
//! applications as a normal user would).
//!
//! A connection is the agent's own browser signed in to a service's website by you: no API key, no password given
//! to Lattice. Each service's Sign in opens the agent's browser at its sign-in page (the core's `browser_show`), where
//! you sign in yourself; the sign-in stays in the browser's own profile on this PC. The agent then uses the site in
//! Agent mode, in a chat with or without a folder, and asks before posting, messaging, buying, deleting or changing
//! an account. This page lists and opens; it decides nothing and reads no sign-in.

use iced::widget::{Column, Row, column, container, row, scrollable, space};
use iced::{Alignment, Element, Length};

use crate::theme;
use lattice_core::browser::BrowserStatus;

use super::IdeMsg;
use super::tools::{ToolsMsg, browser_words};
use crate::lattice::{Msg, State};
use crate::ui::{self, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

/// One service, as the page lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Service {
    pub name: &'static str,
    /// Where Sign in opens the agent's browser.
    pub sign_in: &'static str,
    /// What the agent can do there, in a few words.
    pub does: &'static str,
}

/// The services listed, in a fixed order, the first four being the most asked for.
pub const SERVICES: [Service; 10] = [
    Service {
        name: "Google",
        sign_in: "https://accounts.google.com/",
        does: "Search, Gmail, Drive, Calendar and YouTube, through their websites.",
    },
    Service {
        name: "Amazon",
        sign_in: "https://www.amazon.com/",
        does: "Shopping: search, compare and fill a cart. Placing an order always asks you.",
    },
    Service {
        name: "Amazon Web Services",
        sign_in: "https://console.aws.amazon.com/",
        does: "The AWS console. Anything that changes an account or costs money asks you.",
    },
    Service {
        name: "Instagram",
        sign_in: "https://www.instagram.com/",
        does: "Your feed and messages. Posting, commenting and messaging ask you.",
    },
    Service {
        name: "X",
        sign_in: "https://x.com/",
        does: "Your timeline and messages. Posting, replying, liking and following ask you.",
    },
    Service {
        name: "Facebook",
        sign_in: "https://www.facebook.com/",
        does: "Your feed, pages and messages. Anything others will see asks you.",
    },
    Service {
        name: "LinkedIn",
        sign_in: "https://www.linkedin.com/login",
        does: "Your feed, profile and messages. Posting and messaging ask you.",
    },
    Service {
        name: "GitHub",
        sign_in: "https://github.com/login",
        does: "Repositories, issues and pull requests on the website. Comments and changes ask you.",
    },
    Service {
        name: "Outlook",
        sign_in: "https://outlook.live.com/",
        does: "Mail and calendar. Sending and deleting ask you.",
    },
    Service {
        name: "Reddit",
        sign_in: "https://www.reddit.com/login/",
        does: "Reading and searching. Posting, commenting and voting ask you.",
    },
];

fn go(msg: ToolsMsg) -> Msg {
    Msg::Ide(IdeMsg::Tools(msg))
}

/// The Connections page, in the editor area.
pub fn page<'a>(state: &'a State, _phase: f32) -> El<'a> {
    let t = &state.ide.tools;
    let (on, status) = match &t.browser {
        Some((on, status, _)) => (*on, status.clone()),
        None => (false, BrowserStatus::Stopped),
    };
    let mut col = Column::new().spacing(14).padding([18.0, 24.0]).max_width(980);
    col = col.push(
        column![
            strong("Connections", 20.0, theme::TEXT),
            label(
                "The agent uses these services through their websites, in its own browser, as a person does: no API keys, and no password given to Lattice. Sign in to each one yourself in the agent's browser; the sign-in stays in its profile on this PC. Then ask for it in a chat in Agent mode, with or without a folder. Posting, messaging, buying, deleting and account changes ask you first.",
                12.5,
                theme::TEXT_DIM,
            ),
        ]
        .spacing(4),
    );
    if let Some((words, warn)) = &t.said {
        col = col.push(ui::notice(words.clone(), if *warn { theme::CAUTION } else { theme::POSITIVE }));
    }
    // The browser: connections need it on.
    let (color, words) = browser_words(on, &status);
    let mut browser = Row::new().spacing(10).align_y(Alignment::Center);
    browser = browser.push(ui::dot(color));
    browser = browser.push(column![label("The agent's browser", 13.0, theme::TEXT), label(words, 11.5, theme::TEXT_FAINT)].spacing(1));
    browser = browser.push(space().width(Length::Fill));
    if t.browser_busy {
        browser = browser.push(label("Working\u{2026}", 12.0, theme::TEXT_FAINT));
    } else if !on {
        browser = browser.push(ui::primary("Switch it on", Some(go(ToolsMsg::BrowserSet(true)))));
    } else {
        browser = browser.push(ui::secondary("Show it", Some(go(ToolsMsg::BrowserShow))));
    }
    col = col.push(container(browser).padding([10.0, 14.0]).width(Length::Fill).style(theme::card));
    if !on {
        col = col.push(note("Switch the browser on to use a connection: until then no chat offers it."));
    }
    // The services.
    let mut list = Column::new().spacing(8);
    for service in SERVICES {
        list = list.push(service_row(service, t.browser_busy));
    }
    col = col.push(list);
    col = col.push(label(
        "A service not listed works the same way: ask the agent to open its website. The agent opens only the public web, never this PC or its network, and never types a password or card details.",
        11.5,
        theme::TEXT_FAINT,
    ));
    scrollable(container(col).center_x(Length::Fill)).height(Length::Fill).style(theme::scrollbars).into()
}

fn service_row<'a>(service: Service, busy: bool) -> El<'a> {
    let site = lattice_core::browser::policy::site_of(service.sign_in).unwrap_or_default();
    let line = row![
        column![
            row![strong(service.name, 14.0, theme::TEXT), mono(site, 11.0, theme::TEXT_FAINT)].spacing(8).align_y(Alignment::Center),
            label(service.does, 12.0, theme::TEXT_DIM),
        ]
        .spacing(2)
        .width(Length::Fill),
        ui::secondary("Sign in\u{2026}", (!busy).then(|| go(ToolsMsg::BrowserShowAt(service.sign_in.to_string())))),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    container(line).padding([10.0, 14.0]).width(Length::Fill).style(theme::card).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every service opens an address the agent's browser may open (the public web, https), under a distinct name.
    #[test]
    fn every_service_signs_in_on_the_public_web() {
        let mut names = std::collections::HashSet::new();
        for service in SERVICES {
            assert!(names.insert(service.name), "{} twice", service.name);
            let url = lattice_core::browser::policy::check_url(service.sign_in).unwrap();
            assert!(url.starts_with("https://"), "{url}");
            assert!(lattice_core::browser::policy::site_of(&url).is_some());
            assert!(!service.does.is_empty());
        }
        let first: Vec<&str> = SERVICES.iter().take(5).map(|s| s.name).collect();
        assert_eq!(first, ["Google", "Amazon", "Amazon Web Services", "Instagram", "X"], "the four most asked for come first");
    }
}
