//! Is it running? One look at each local service's port, taken off the window's thread.
//!
//! A look is a connection opened and closed at once on loopback: a service that is not there refuses at once,
//! so a whole round takes milliseconds. The window looks only while a page that shows the answers is open.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::catalogue::Section;

pub struct Service {
    pub name: &'static str,
    pub what: &'static str,
    pub port: u16,
    pub section: Section,
}

/// Sinai's companion program holds 8178-8182; the speech engine holds 8186 and 8187.
pub const SERVICES: &[Service] = &[
    Service { name: "Sinai's loop", what: "Its face and its conversation socket", port: 8180, section: Section::Sinai },
    Service { name: "Sinai's mind", what: "The language model (llama.cpp)", port: 8179, section: Section::Sinai },
    Service { name: "Sinai's hearing", what: "The loop's own speech model", port: 8178, section: Section::Sinai },
    Service { name: "Sinai's eyes", what: "The vision model, while it looks", port: 8181, section: Section::Sinai },
    Service { name: "Sinai's embeddings", what: "The embedding model", port: 8182, section: Section::Sinai },
    Service { name: "The ears", what: "Captions, dictation and files", port: 8186, section: Section::Transcription },
    Service { name: "The ears' recogniser", what: "whisper large-v3-turbo", port: 8187, section: Section::Transcription },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Up,
    Down,
}

/// The ears' port as their connection file says (a test service may use another), else the default.
fn port_of(service: &Service) -> u16 {
    if service.port != 8186 {
        return service.port;
    }
    crate::ears::connection_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(str::to_string))
        .and_then(|url| url.trim_end_matches('/').rsplit(':').next().and_then(|p| p.parse().ok()))
        .unwrap_or(service.port)
}

pub fn look() -> Vec<Reading> {
    SERVICES
        .iter()
        .map(|s| {
            let addr = SocketAddr::from(([127, 0, 0, 1], port_of(s)));
            match TcpStream::connect_timeout(&addr, Duration::from_millis(250)) {
                Ok(_) => Reading::Up,
                Err(_) => Reading::Down,
            }
        })
        .collect()
}

/// How often the pages that show the services look: a round is a few loopback connections refused or accepted at once,
/// and it makes no message unless something started or stopped, so the pages need no Check again.
pub const EVERY: Duration = Duration::from_secs(3);

/// The services' states while a page that shows them is open: a look at once, then one every `EVERY`, each on this
/// subscription's own thread, and said to the window only when it differs from the last look said. A look that
/// changes nothing makes no message, so the window is not drawn for it (iced draws after every message), and no
/// spinner turns for it: the tiles' spinner is for the first look only.
pub fn watch() -> iced::Subscription<Vec<Reading>> {
    iced::Subscription::run(watching)
}

fn watching() -> impl futures::Stream<Item = Vec<Reading>> {
    iced::stream::channel(4, async |mut output: futures::channel::mpsc::Sender<Vec<Reading>>| {
        let _ = std::thread::Builder::new().name("centcom-probe-watch".into()).spawn(move || {
            let mut said: Option<Vec<Reading>> = None;
            // The window stops listening when no open page shows the services: the channel closes and the thread ends.
            while !output.is_closed() {
                let now = look();
                if let Some(news) = news(&mut said, now)
                    && futures::executor::block_on(futures::SinkExt::send(&mut output, news)).is_err()
                {
                    return;
                }
                std::thread::sleep(EVERY);
            }
        });
    })
}

/// What a look has to tell the window: the readings, when they differ from what it last said (or it has said nothing).
pub fn news(said: &mut Option<Vec<Reading>>, now: Vec<Reading>) -> Option<Vec<Reading>> {
    if said.as_ref() == Some(&now) {
        return None;
    }
    *said = Some(now.clone());
    Some(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_is_told_only_when_it_differs_from_the_last_one_told() {
        let mut said = None;
        assert_eq!(news(&mut said, vec![Reading::Down, Reading::Up]), Some(vec![Reading::Down, Reading::Up]), "the first is told");
        assert_eq!(news(&mut said, vec![Reading::Down, Reading::Up]), None, "the same again is not");
        assert_eq!(news(&mut said, vec![Reading::Up, Reading::Up]), Some(vec![Reading::Up, Reading::Up]), "a change is");
        assert_eq!(news(&mut said, vec![Reading::Up, Reading::Up]), None);
    }

    #[test]
    fn a_port_nobody_holds_reads_down_and_one_held_reads_up() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        let up = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(250));
        assert!(up.is_ok());
        drop(held);
        let down = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(250));
        assert!(down.is_err());
    }

    #[test]
    fn every_service_has_its_own_port() {
        let mut ports: Vec<u16> = SERVICES.iter().map(|s| s.port).collect();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(ports.len(), SERVICES.len());
    }
}
