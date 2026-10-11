//! What agents kept from the archive (`alelyon_research::knowledge`), on the hub: how much is kept, the gatherings
//! agents asked for (each waits for the person: Approve opens it as a new subject, whose Gather is what spends credits;
//! Dismiss puts it away), and the newest answers, facts and notes, each of which the person can forget.

use alelyon_research::knowledge::{self, Counts, Kind, Knowledge, Recent, Request};
use iced::widget::{Column, column, row};
use iced::{Alignment, Element, Length, Task};

use crate::theme;
use crate::ui::{self, card, label, note, strong};

use super::{Msg, State, off_thread};

type El<'a> = Element<'a, Msg>;

/// How many of the newest records the hub shows.
const SHOWN: usize = 8;

/// The kept knowledge as the hub shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kept {
    pub counts: Counts,
    pub waiting: Vec<Request>,
    pub recent: Recent,
}

impl Kept {
    pub fn is_empty(&self) -> bool {
        self.counts == Counts::default()
    }
}

impl State {
    pub(super) fn knowledge_path(&self) -> std::path::PathBuf {
        knowledge::beside(&self.db)
    }

    /// Read what is kept, off the window's thread. Nothing kept yet reads as empty (no file is made).
    pub(super) fn read_kept(&self) -> Task<Msg> {
        let path = self.knowledge_path();
        Task::perform(
            off_thread(move || -> Result<Kept, String> {
                let Some(k) = Knowledge::open_read_only(&path)? else { return Ok(Kept::default()) };
                Ok(Kept { counts: k.counts()?, waiting: k.requests(Some("waiting"))?, recent: k.recent(SHOWN)? })
            }),
            |r| Msg::KeptRead(r.unwrap_or_else(|| Err("the reading thread ended without an answer".into()))),
        )
    }

    /// Write to the kept knowledge off the window's thread, then read it again.
    pub(super) fn act_on_kept(&self, work: impl FnOnce(&mut Knowledge, &str) -> Result<(), String> + Send + 'static) -> Task<Msg> {
        let path = self.knowledge_path();
        Task::perform(
            off_thread(move || {
                let at = alelyon_research::harvest_date::time_of(crate::utc::now() as i64);
                let mut k = Knowledge::open(&path)?;
                work(&mut k, &at)
            }),
            |r| Msg::KeptActed(r.unwrap_or_else(|| Err("the writing thread ended without an answer".into()))),
        )
    }

    /// Approve a request: the new-subject form opens with its words, and the request is marked approved. Nothing is
    /// searched until the person presses Gather there.
    pub(super) fn approve(&mut self, r: Request) -> Task<Msg> {
        let mut name: String = r.query.chars().take(60).collect();
        if let Some(s) = &r.subject {
            name = format!("{s}: {name}");
        }
        let concepts: Vec<String> = knowledge::keywords(&r.query).into_iter().take(3).collect();
        self.prefill(name, vec![r.query.clone()], concepts);
        let id = r.id;
        self.act_on_kept(move |k, at| k.decide(id, "approved", at))
    }
}

/// The hub's card for what agents kept; none when nothing is kept or could be read.
pub fn card_of(state: &State) -> Option<El<'_>> {
    let kept = match &state.kept {
        Some(Ok(k)) if !k.is_empty() => k,
        Some(Err(why)) => return Some(card("What agents kept", Column::new().push(ui::notice(why.as_str(), theme::CAUTION)))),
        _ => return None,
    };
    let c = &kept.counts;
    let mut col = Column::new().spacing(10).push(label(
        format!(
            "{} answer{}, {} fact{}, {} concept{} with {} relation{}, {} note{}: agents ask these before reading papers again.",
            c.answers,
            plural(c.answers),
            c.facts,
            plural(c.facts),
            c.concepts,
            plural(c.concepts),
            c.relations,
            plural(c.relations),
            c.notes,
            plural(c.notes)
        ),
        12.5,
        theme::TEXT_DIM,
    ));
    if !kept.waiting.is_empty() {
        col = col.push(ui::subheading("Gatherings agents asked for"));
        for r in &kept.waiting {
            col = col.push(
                row![
                    column![
                        strong(r.query.as_str(), 13.0, theme::TEXT),
                        label(ui::cut(&r.why, 160), 11.5, theme::TEXT_DIM),
                        label(format!("asked by {}, {}{}", r.by, r.at, r.subject.as_ref().map(|s| format!(", for \"{s}\"")).unwrap_or_default()), 11.0, theme::TEXT_FAINT),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    ui::primary("Approve", state.running.is_none().then(|| Msg::ApproveRequest(r.clone()))),
                    ui::secondary("Dismiss", Some(Msg::DismissRequest(r.id))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        col = col.push(note("Approve opens it as a new subject; nothing is searched, and no credits are spent, until you press Gather there."));
    }
    let recent = &kept.recent;
    if !recent.answers.is_empty() || !recent.facts.is_empty() || !recent.notes.is_empty() {
        col = col.push(ui::subheading("Newest"));
    }
    for a in &recent.answers {
        col = col.push(line(format!("Q: {}", a.question), ui::cut(&a.answer, 200), format!("answer by {}, {}", a.by, a.at), Kind::Answer, a.id));
    }
    for f in &recent.facts {
        col = col.push(line(f.text.clone(), format!("from \u{201c}{}\u{201d}", ui::cut(&f.paper.title, 120)), format!("{} by {}, {}", f.kind, f.by, f.at), Kind::Fact, f.id));
    }
    for n in &recent.notes {
        col = col.push(line(n.about.clone(), ui::cut(&n.text, 200), format!("note by {}, {}", n.by, n.at), Kind::Note, n.id));
    }
    col = col.push(note("Written by agents from your archive's papers; check a record's grounds before relying on it. Forget removes it for every agent."));
    Some(card("What agents kept", col))
}

fn line<'a>(title: String, body: String, by: String, kind: Kind, id: i64) -> El<'a> {
    row![
        column![strong(title, 12.5, theme::TEXT), label(body, 12.0, theme::TEXT_DIM), label(by, 11.0, theme::TEXT_FAINT)].spacing(2).width(Length::Fill),
        ui::secondary("Forget", Some(Msg::ForgetKept(kind, id))),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn plural(n: i64) -> &'static str {
    if n == 1 { "" } else { "s" }
}
