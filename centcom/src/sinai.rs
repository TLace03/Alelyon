//! Sinai from the window: a live link to its loop's socket, the conversation it carries, and the three
//! things a person does there: type to Sinai, open or close its microphone, and allow or refuse an act it holds.
//!
//! The loop serves every surface over one WebSocket: the Angel window and the face page are
//! clients of it, and so is CENTCOM. Where that socket is comes from the build (`installed.rs`, the `sinai-mind`
//! plug-in); the public build carries none, connects to nothing and says Sinai's mind is not installed. Much of what it sends is for the face (the mouth's opening many times a second, the
//! observatory's gauges, pictures of the window Sinai looks at). This link passes on to the window only what the
//! conversation needs, so the window redraws for words, not for a moving mouth: the mouth and the expression go
//! straight to the face (`face::LIVE`), whose own frame clock reads them while it is on show.
//!
//! The loop's own words for its state are shown as sent: `waiting for "Sinai"` is its on-air honesty for a
//! listener that is not always listening for you.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::SinkExt;
use futures::channel::mpsc as stream_mpsc;
use iced::Subscription;
use serde_json::{Map, Value, json};

const RETRY: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(100);

/// The message types read off the socket: the face's two (handed to it, never to the window) and the conversation's.
/// Everything else (the observatory's gauges, pictures) is left on the socket.
const KEPT: &[&str] = &[
    "mouth",
    "expression",
    "state",
    "partial",
    "transcript",
    "mic",
    "held",
    "judged",
    "journal",
    "journal_all",
    "memory",
    "memory_erased",
    // the observatory (the Machine): tier 1, every quarter second while Sinai answers; tier 2, sampled; tier 3,
    // derived; and the last turn's measured ledger
    "pulse",
    "forward",
    "landscape",
    "ledger",
    // Sinai's eyes: its vision model opened or closed
    "sight",
    "standing",
    "working",
    // what the loop can be asked beyond typed words, and a research finding it kept (the Research page reads it)
    "abilities",
    "research_finding",
];

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

/// Where Sinai's mind is reached, as the build finds it; None when this build does not carry Sinai's mind or it was not found.
pub fn url() -> Option<String> {
    crate::installed::sinai_mind().and_then(Result::ok)
}

#[derive(Clone, Debug)]
pub struct Commands(mpsc::Sender<String>);

impl Commands {
    pub fn send(&self, command: Value) -> bool {
        self.0.send(command.to_string()).is_ok()
    }

    /// Commands that land in `tx`, for tests of what the window sends.
    #[cfg(test)]
    pub fn into(tx: mpsc::Sender<String>) -> Commands {
        Commands(tx)
    }
}

#[derive(Clone, Debug)]
pub enum Feed {
    Unavailable,
    Connected(Commands),
    Event(Value),
    Lost,
}

pub fn subscription() -> Subscription<Feed> {
    Subscription::run(link)
}

fn link() -> impl futures::Stream<Item = Feed> {
    iced::stream::channel(64, async |output: stream_mpsc::Sender<Feed>| {
        let _ = std::thread::Builder::new().name("centcom-sinai".into()).spawn(move || run(output));
    })
}

fn run(mut output: stream_mpsc::Sender<Feed>) {
    let mut emit = |feed: Feed| futures::executor::block_on(output.send(feed)).is_ok();
    // The window has been told the loop is not there: it is told again only after the loop has come and gone,
    // not every two seconds.
    let mut told = false;
    loop {
        // The window subscribes only in a build that carries Sinai's mind; asked anyway, it says it is not there.
        let Some(url) = url() else {
            let _ = emit(Feed::Unavailable);
            return;
        };
        match connect(&url) {
            None => {
                if !told {
                    if !emit(Feed::Unavailable) {
                        return;
                    }
                    told = true;
                }
            }
            Some(mut ws) => {
                told = false;
                crate::face::LIVE.connected(true);
                let (tx, rx) = mpsc::channel::<String>();
                if !emit(Feed::Connected(Commands(tx))) {
                    return;
                }
                let mut hands_seen = None;
                loop {
                    let mut broken = false;
                    while let Ok(command) = rx.try_recv() {
                        if ws.send(tungstenite::Message::text(command)).is_err() {
                            broken = true;
                            break;
                        }
                    }
                    if broken {
                        break;
                    }
                    match ws.read() {
                        Ok(tungstenite::Message::Text(text)) => {
                            let Some(v) = keep(&text) else { continue };
                            // The face's own go to the face and no further: a moving mouth redraws no page.
                            if crate::face::LIVE.absorb(&v) {
                                continue;
                            }
                            if hands_changed(&v, &mut hands_seen) && !emit(Feed::Event(v)) {
                                crate::face::LIVE.lost();
                                let _ = ws.close(None);
                                return;
                            }
                        }
                        Ok(tungstenite::Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                        Err(_) => break,
                    }
                }
                crate::face::LIVE.lost();
                if !emit(Feed::Lost) {
                    return;
                }
            }
        }
        std::thread::sleep(RETRY);
    }
}

/// `working` comes twice a second whether anything changed or not: the window hears it only when Sinai's hands are
/// armed or disarmed, or move to another window, so an idle window is not redrawn for it. Every other message passes.
fn hands_changed(v: &Value, seen: &mut Option<(i64, bool)>) -> bool {
    if v.get("type").and_then(Value::as_str) != Some("working") {
        return true;
    }
    let now = (v.get("handle").and_then(Value::as_i64).unwrap_or(0), v.get("armed").and_then(Value::as_bool).unwrap_or(false));
    if *seen == Some(now) {
        return false;
    }
    *seen = Some(now);
    true
}

/// A message this link reads, parsed; `None` for the rest (checked by type before parsing the whole).
fn keep(text: &str) -> Option<Value> {
    // What is left on the socket is many messages and some are large (pictures): skip them on a cheap look at the
    // type first.
    // The loop writes "type" first in every message. The cut falls on a character boundary.
    let end = (0..=text.len().min(48)).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    let head = &text[..end];
    if !KEPT.iter().any(|k| head.contains(&format!("\"{k}\""))) {
        return None;
    }
    let v: Value = serde_json::from_str(text).ok()?;
    let kind = v.get("type").and_then(Value::as_str)?;
    KEPT.contains(&kind).then_some(v)
}

fn connect(url: &str) -> Option<Socket> {
    let (mut ws, _) = tungstenite::connect(url).ok()?;
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
        let _ = stream.set_read_timeout(Some(POLL));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    }
    Some(ws)
}

// ------------------------------------------------------------------- what the loop says

/// One act Sinai holds until a person answers it (`held` items, `loop/consequence.py`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Held {
    pub id: String,
    pub question: String,
    pub what: String,
    pub reason: String,
    /// The act's kind (click, press, control, ...) and the fields it acts on (a click's `name`, `x` and `y`).
    pub kind: String,
    pub fields: Map<String, Value>,
    /// How long the loop still waited when it sent the list. The loop does not resend the list as time passes, so
    /// the window counts down from when it arrived (`Conversation::held_at`).
    pub seconds_left: f32,
}

impl Held {
    /// The seconds left at `now`, counted from when the list arrived at `since`; never below zero.
    pub fn left(&self, since: Option<Instant>, now: Instant) -> f32 {
        let gone = since.map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        (self.seconds_left - gone).max(0.0)
    }

    /// Where a click would land, when this act is a click the loop lets a person move first: the only kind whose
    /// change the loop carries out (it re-reads `x` and `y`; a pressed key or a control ignores or refuses a change).
    pub fn click_at(&self) -> Option<(i64, i64)> {
        (self.kind == "click").then_some(())?;
        Some((self.fields.get("x")?.as_i64()?, self.fields.get("y")?.as_i64()?))
    }
}

/// What a person asked to change in a held click, checked before it is sent: the loop drops an act whose change it
/// refuses, so a change that is not two whole numbers for a click's own `x` and `y` is never sent.
pub fn click_change(held: &Held, x: &str, y: &str) -> Option<Map<String, Value>> {
    held.click_at()?;
    let (x, y) = (x.trim().parse::<i64>().ok()?, y.trim().parse::<i64>().ok()?);
    let mut changes = Map::new();
    changes.insert("x".into(), json!(x));
    changes.insert("y".into(), json!(y));
    Some(changes)
}

/// How a held act was decided, as the loop reports it (`judged`): the act, the gate's verdict and reason, and what
/// became of it ("granted by the coach", "refused by the coach", "abandoned: expired", ...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Judged {
    pub what: String,
    pub verdict: String,
    pub reason: String,
    pub outcome: String,
    pub resolved: bool,
}

/// Which of Sinai's journal rows to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    All,
    Doing,
    Done,
    /// The loop's `allowed: false`, which covers both a refusal and a failure; the outcome says which.
    RefusedOrFailed,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::All, Status::Doing, Status::Done, Status::RefusedOrFailed];

    pub fn title(self) -> &'static str {
        match self {
            Status::All => "All",
            Status::Doing => "Doing",
            Status::Done => "Done",
            Status::RefusedOrFailed => "Refused or failed",
        }
    }

    pub fn shows(self, note: &Note) -> bool {
        match self {
            Status::All => true,
            Status::Doing => note.tense == "doing",
            Status::Done => note.tense == "done" && note.allowed,
            Status::RefusedOrFailed => !note.allowed,
        }
    }
}

/// One line of Sinai's journal: what it did, is doing, or plans (`loop/journal.py`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    pub seq: u64,
    pub wall: String,
    pub kind: String,
    pub what: String,
    pub detail: String,
    pub outcome: String,
    pub allowed: bool,
    pub tense: String,
}

/// Sinai's microphone, as the loop reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mic {
    pub on: bool,
    /// In a conversation: speech reaches Sinai without its name.
    pub attending: bool,
    pub follow_up_s: Option<f32>,
    pub error: Option<String>,
}

/// The conversation as the window keeps it.
#[derive(Debug, Default)]
pub struct Conversation {
    /// "you" or "being", and the words; newest last.
    pub lines: Vec<(String, String)>,
    /// What the loop is hearing right now.
    pub hearing: String,
    /// idle, listening, thinking, speaking, or `waiting for "Sinai"`, as sent.
    pub state: String,
    pub mic: Mic,
    pub held: Vec<Held>,
    /// When the held list arrived: each act's `seconds_left` counts from here.
    pub held_at: Option<Instant>,
    /// How held acts were decided, newest last.
    pub judged: Vec<Judged>,
    /// Newest last.
    pub journal: Vec<Note>,
    /// What Sinai may do, one sentence a grant, as the loop reads the grants file (`loop/authority.py`).
    pub grants: Vec<String>,
    /// Grants that failed to load: worse than none, because somebody believes they are in force.
    pub grant_complaints: Vec<String>,
    /// The grants file, which a person writes and Sinai only reads.
    pub grants_path: String,
    /// Whether the loop has said what is in force since the window connected. It says so only when Sinai's hands
    /// are armed or disarmed, so until then "no grants" would be a guess.
    pub standing_seen: bool,
    /// Sinai's hands as the loop last said (`working`): armed or not, and the window they work in. None until it says.
    pub hands: Option<Hands>,
    /// Sinai's memory as the loop's newest snapshot says (`machine`), and that snapshot's version: None until one
    /// arrives, and again after an erase.
    pub memory: Option<crate::machine::Memory>,
    pub memory_version: Option<(u64, u64)>,
    /// The observatory, as the loop last sent each part (`machine` reads them): what Sinai is doing and the rule that
    /// said so (tier 1), the sampled forward pass (tier 2), the derived landscape (tier 3), and the last turn's ledger.
    pub pulse: Value,
    pub forward: Value,
    pub landscape: Value,
    pub last_turn: Value,
    /// Sinai's sight as the loop last said; None until it says (it says when the eyes open or close).
    pub sight: Option<crate::machine::Sight>,
}

/// Where Sinai's hands are: armed on a window (its handle and title), or not armed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hands {
    pub armed: bool,
    pub handle: i64,
    pub title: String,
}

impl Conversation {
    const LINES_KEPT: usize = 200;
    const NOTES_KEPT: usize = 300;
    const JUDGED_KEPT: usize = 30;

    pub fn absorb(&mut self, v: &Value) {
        let s = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        match v.get("type").and_then(Value::as_str).unwrap_or("") {
            "state" => self.state = s("s"),
            "partial" => self.hearing = s("text"),
            "transcript" => {
                let text = s("text");
                if !text.trim().is_empty() {
                    self.lines.push((s("who"), text));
                    let n = self.lines.len();
                    if n > Self::LINES_KEPT {
                        self.lines.drain(..n - Self::LINES_KEPT);
                    }
                }
            }
            "mic" => {
                self.mic = Mic {
                    on: v.get("on").and_then(Value::as_bool).unwrap_or(false),
                    attending: v.get("attending").and_then(Value::as_bool).unwrap_or(false),
                    follow_up_s: v.get("follow_up_s").and_then(Value::as_f64).map(|f| f as f32),
                    error: v.get("error").and_then(Value::as_str).map(str::to_string),
                }
            }
            "held" => {
                let text = |h: &Value, key: &str| h.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                self.held = v
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .map(|h| Held {
                                id: h.get("id").map(|i| i.as_str().map(str::to_string).unwrap_or_else(|| i.to_string())).unwrap_or_default(),
                                question: text(h, "question"),
                                what: text(h, "what"),
                                reason: text(h, "reason"),
                                kind: text(h, "kind"),
                                fields: h.get("fields").and_then(Value::as_object).cloned().unwrap_or_default(),
                                seconds_left: h.get("seconds_left").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.held_at = Some(Instant::now());
            }
            "judged" => {
                let judged = Judged {
                    what: s("what"),
                    verdict: s("verdict"),
                    reason: s("reason"),
                    outcome: s("outcome"),
                    resolved: v.get("resolved").and_then(Value::as_bool).unwrap_or(false),
                };
                // The loop names an act by what it does, not by its id: a later word on an unresolved act updates it.
                match self.judged.iter_mut().rev().find(|j| j.what == judged.what && !j.resolved) {
                    Some(slot) => *slot = judged,
                    None => self.judged.push(judged),
                }
                let n = self.judged.len();
                if n > Self::JUDGED_KEPT {
                    self.judged.drain(..n - Self::JUDGED_KEPT);
                }
            }
            "journal" => {
                if let Some(entry) = v.get("entry") {
                    self.note(entry);
                }
            }
            "journal_all" => {
                self.journal.clear();
                for row in v.get("rows").and_then(Value::as_array).into_iter().flatten() {
                    self.note(row);
                }
            }
            "standing" => {
                let list = |key: &str| -> Vec<String> {
                    v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
                };
                self.grants = list("grants");
                self.grant_complaints = list("complaints");
                self.grants_path = s("path");
                self.standing_seen = true;
            }
            "working" => {
                self.hands = Some(Hands {
                    armed: v.get("armed").and_then(Value::as_bool).unwrap_or(false),
                    handle: v.get("handle").and_then(Value::as_i64).unwrap_or(0),
                    title: s("title"),
                })
            }
            // The person erased Sinai's memory: what was said goes from the screen too.
            "memory_erased" => {
                // an erase of an epoch older than the memory shown is a stale one: the conversation stays too
                let stale = v.get("epoch").and_then(Value::as_u64).is_some_and(|e| self.memory_version.is_some_and(|old| old.0 > e));
                if !stale {
                    self.lines.clear();
                    self.hearing.clear();
                    // as the Angel window: what was derived from the erased conversation goes with it
                    self.landscape = Value::Null;
                    self.last_turn = Value::Null;
                }
                crate::machine::absorb(&mut self.memory, &mut self.memory_version, v);
            }
            "memory" => crate::machine::absorb(&mut self.memory, &mut self.memory_version, v),
            "pulse" => self.pulse = v.clone(),
            "forward" => self.forward = v.clone(),
            "landscape" => self.landscape = v.clone(),
            "ledger" => self.last_turn = v.clone(),
            "sight" => self.sight = crate::machine::Sight::from_message(v),
            _ => {}
        }
    }

    fn note(&mut self, row: &Value) {
        let s = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        let note = Note {
            seq: row.get("seq").and_then(Value::as_u64).unwrap_or(0),
            wall: s("wall"),
            kind: s("kind"),
            what: s("what"),
            detail: s("detail"),
            outcome: s("outcome"),
            allowed: row.get("allowed").and_then(Value::as_bool).unwrap_or(true),
            tense: s("tense"),
        };
        // A row seen again is an update to it (an act that has now finished).
        match self.journal.iter_mut().find(|n| n.seq == note.seq && note.seq != 0) {
            Some(slot) => *slot = note,
            None => self.journal.push(note),
        }
        let n = self.journal.len();
        if n > Self::NOTES_KEPT {
            self.journal.drain(..n - Self::NOTES_KEPT);
        }
    }

    /// Sinai is working on an answer.
    pub fn thinking(&self) -> bool {
        self.state == "thinking"
    }

    /// The kinds of act in the journal, for its filter: sorted, once each.
    pub fn kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self.journal.iter().map(|n| n.kind.clone()).filter(|k| !k.is_empty()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    }
}

// ------------------------------------------------------------------- commands

pub fn say(text: &str) -> Value {
    json!({"type": "text", "text": text})
}

/// Work on one gap of the person's research archive (the loop's gapwork.py: it reads the gap and its papers over the
/// archive's MCP server, looks for newer work, and keeps its finding beside the gap). Sent only to a loop that said
/// it can (`abilities`).
pub fn research_gap(subject: &str, headline: &str) -> Value {
    json!({"type": "research_gap", "subject": subject, "headline": headline})
}

pub fn listen(on: bool) -> Value {
    json!({"type": "listen", "on": on})
}

pub fn decide(id: &str, yes: bool) -> Value {
    json!({"type": "decide", "id": id, "answer": if yes { "yes" } else { "no" }})
}

/// Name the window showing what Sinai sees (`mine`), so the loop never has Sinai observe it, with `on`, its word for
/// a view being shown (the Angel window sends the same once).
pub fn watch(mine: u64) -> Value {
    json!({"type": "watch", "on": true, "mine": mine})
}

/// Allow a held act with `changes` made first (see `click_change`): the loop applies them, checks the act again,
/// and carries it out.
pub fn decide_changed(id: &str, changes: &Map<String, Value>) -> Value {
    json!({"type": "decide", "id": id, "answer": "yes", "changes": changes})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_neither_the_face_nor_the_conversation_reads_is_left_on_the_socket() {
        // the Angel window's own views that nothing here draws stay on the socket; the observatory's parts are kept
        assert!(keep(r#"{"type":"seen","jpeg":"..."}"#).is_none());
        assert!(keep(r#"{"type":"surfaces","windows":[]}"#).is_none());
        for kept in ["pulse", "forward", "landscape", "ledger", "sight"] {
            assert!(keep(&format!(r#"{{"type":"{kept}"}}"#)).is_some(), "{kept}");
        }
        assert!(keep(r#"{"type":"partial","text":"what time"}"#).is_some());
        assert!(keep("not json at all").is_none());
        // a kept word inside another message's text does not let it through
        assert!(keep(r#"{"type":"surfaces","note":"\"state\" was idle"}"#).is_none());
        // a cut in the middle of a character does not break the look
        assert!(keep(r#"{"type":"transcript","who":"you","text":"ééééééééééééééé"}"#).is_some());
    }

    #[test]
    fn the_mouth_and_the_expression_go_to_the_face_and_not_to_the_window() {
        // as the loop writes them, "type" first
        let mouth = keep(r#"{"type": "mouth", "v": 0.4}"#).expect("the mouth is read");
        let tone = keep(r#"{"type": "expression", "tone": "smile", "lid": 0.1, "tilt": -0.03, "warmth": 0.8}"#).expect("and the expression");
        let live = crate::face::Live::new();
        assert!(live.absorb(&mouth) && live.absorb(&tone), "both are the face's, so the window never hears them");
        assert_eq!(live.read().0, 0.4);
        assert_eq!(live.read().2.tone, "smile");
        let words = keep(r#"{"type":"transcript","who":"being","text":"hello"}"#).unwrap();
        assert!(!live.absorb(&words), "the conversation's words still reach the window");
    }

    #[test]
    fn a_conversation_is_kept_as_the_loop_tells_it() {
        let mut c = Conversation::default();
        c.absorb(&json!({"type": "mic", "on": true, "attending": false, "follow_up_s": null}));
        c.absorb(&json!({"type": "state", "s": "waiting for \"Sinai\""}));
        assert!(c.mic.on && !c.mic.attending);
        c.absorb(&json!({"type": "partial", "text": "Sinai what is"}));
        assert_eq!(c.hearing, "Sinai what is");
        c.absorb(&json!({"type": "transcript", "who": "you", "text": "Sinai, what is on my calendar?"}));
        c.absorb(&json!({"type": "state", "s": "thinking"}));
        assert!(c.thinking());
        c.absorb(&json!({"type": "transcript", "who": "being", "text": "Two meetings this afternoon."}));
        assert_eq!(c.lines.len(), 2);
        assert_eq!(c.lines[1].0, "being");
        c.absorb(&json!({"type": "memory_erased", "epoch": 3}));
        assert!(c.lines.is_empty(), "an erased memory leaves the screen too");
    }

    /// A held click as the loop sends it (`loop/being_loop.py` `_publish_held`, `loop/consequence.py`).
    fn held_click() -> Value {
        json!({"type": "held", "items": [{"id": "h1", "question": "click Send at (412, 300) -- I need a yes first",
            "what": "click Send at (412, 300)", "verdict": "coordinate with a human", "reason": "I am not certain that is the thing you mean",
            "kind": "click", "fields": {"name": "Send", "x": 412, "y": 300}, "seconds_left": 117.4}]})
    }

    #[test]
    fn held_acts_and_the_journal_are_read_whole() {
        let mut c = Conversation::default();
        c.absorb(&held_click());
        let mut fields = Map::new();
        fields.insert("name".into(), json!("Send"));
        fields.insert("x".into(), json!(412));
        fields.insert("y".into(), json!(300));
        assert_eq!(
            c.held,
            [Held {
                id: "h1".into(),
                question: "click Send at (412, 300) -- I need a yes first".into(),
                what: "click Send at (412, 300)".into(),
                reason: "I am not certain that is the thing you mean".into(),
                kind: "click".into(),
                fields,
                seconds_left: 117.4
            }]
        );
        assert!(c.held_at.is_some());
        c.absorb(&json!({"type": "held", "items": []}));
        assert!(c.held.is_empty());
        c.absorb(&json!({"type": "journal", "entry": {"seq": 4, "wall": "15:02", "kind": "act", "what": "opened Notepad", "detail": "",
            "outcome": "", "allowed": true, "tense": "doing"}}));
        c.absorb(&json!({"type": "journal", "entry": {"seq": 4, "wall": "15:02", "kind": "act", "what": "opened Notepad", "detail": "",
            "outcome": "done", "allowed": true, "tense": "done"}}));
        assert_eq!(c.journal.len(), 1, "the same row again is an update");
        assert_eq!(c.journal[0].tense, "done");
        c.absorb(&json!({"type": "journal_all", "rows": [{"seq": 1, "what": "a"}, {"seq": 2, "what": "b"}]}));
        assert_eq!(c.journal.iter().map(|n| n.what.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn grants_are_read_with_the_ones_that_failed_to_load() {
        let mut c = Conversation::default();
        assert!(!c.standing_seen, "until the loop says, what is in force is unknown");
        // the path is the loop's own word for the file it reads (`authority.json` beside the app's folder)
        c.absorb(&json!({"type": "standing", "grants": ["Tom granted: groceries -- 1 recipient(s), any subject, 40.00 of 50.00 USD left, 3600s remaining, live"],
            "complaints": ["grant 2 (press) does not state until; not loaded"], "path": r"D:\Sinai\authority.json"}));
        assert!(c.standing_seen);
        assert_eq!(c.grants.len(), 1);
        assert_eq!(c.grant_complaints, ["grant 2 (press) does not state until; not loaded"]);
        assert!(c.grants_path.ends_with("authority.json"));
    }

    #[test]
    fn a_held_act_counts_down_from_when_the_list_arrived() {
        let mut c = Conversation::default();
        c.absorb(&held_click());
        let since = c.held_at.unwrap();
        let held = &c.held[0];
        assert_eq!(held.left(Some(since), since), 117.4);
        assert!((held.left(Some(since), since + Duration::from_secs(100)) - 17.4).abs() < 0.01);
        assert_eq!(held.left(Some(since), since + Duration::from_secs(200)), 0.0, "never below zero");
    }

    #[test]
    fn only_a_click_is_changed_and_only_to_two_whole_numbers() {
        let mut c = Conversation::default();
        c.absorb(&held_click());
        let click = c.held[0].clone();
        assert_eq!(click.click_at(), Some((412, 300)));
        let changes = click_change(&click, " 420 ", "-5").expect("a click moves to whole numbers");
        assert_eq!(Value::Object(changes.clone()), json!({"x": 420, "y": -5}));
        assert_eq!(decide_changed("h1", &changes), json!({"type": "decide", "id": "h1", "answer": "yes", "changes": {"x": 420, "y": -5}}));
        // the loop drops an act whose change it refuses: none of these is sent
        for (x, y) in [("420.5", "300"), ("", "300"), ("abc", "1"), ("1", "1e3")] {
            assert!(click_change(&click, x, y).is_none(), "{x:?} {y:?}");
        }
        let press = Held { kind: "press".into(), fields: json!({"key": "ctrl+w"}).as_object().unwrap().clone(), ..click.clone() };
        assert!(press.click_at().is_none() && click_change(&press, "1", "2").is_none(), "a pressed key ignores a change");
        let no_point = Held { fields: json!({"name": "Send"}).as_object().unwrap().clone(), ..click };
        assert!(click_change(&no_point, "1", "2").is_none(), "a click without x and y has nothing to move");
    }

    #[test]
    fn decisions_are_followed_by_what_they_do() {
        let mut c = Conversation::default();
        c.absorb(&json!({"type": "judged", "what": "click Send at (412, 300)", "verdict": "coordinate with a human",
            "reason": "unsure", "outcome": "", "resolved": false}));
        c.absorb(&json!({"type": "judged", "what": "click Send at (412, 300)", "verdict": "coordinate with a human",
            "reason": "unsure", "outcome": "granted by the coach, after changing it", "resolved": true}));
        assert_eq!(c.judged.len(), 1, "a later word on the same act updates it");
        assert_eq!(c.judged[0].outcome, "granted by the coach, after changing it");
        c.absorb(&json!({"type": "judged", "what": "click Send at (412, 300)", "verdict": "coordinate with a human",
            "reason": "unsure", "outcome": "abandoned: expired", "resolved": true}));
        assert_eq!(c.judged.len(), 2, "a resolved act is not reopened: the same act asked again is a new line");
    }

    #[test]
    fn the_journal_filters_by_status_and_kind() {
        let mut c = Conversation::default();
        let row = |seq: u64, kind: &str, outcome: &str, allowed: bool, tense: &str| {
            json!({"type": "journal", "entry": {"seq": seq, "wall": "15:02:41", "kind": kind, "what": format!("{kind} {seq}"),
                "detail": "", "outcome": outcome, "allowed": allowed, "tense": tense}})
        };
        c.absorb(&row(1, "click", "carried out", true, "done"));
        c.absorb(&row(2, "key", "refused: the window is not in front", false, "done"));
        c.absorb(&row(3, "invoke", "failed: TimeoutError", false, "done"));
        c.absorb(&row(4, "look", "", true, "doing"));
        let shown = |status: Status| -> Vec<u64> { c.journal.iter().filter(|n| status.shows(n)).map(|n| n.seq).collect() };
        assert_eq!(shown(Status::All), [1, 2, 3, 4]);
        assert_eq!(shown(Status::Doing), [4]);
        assert_eq!(shown(Status::Done), [1]);
        assert_eq!(shown(Status::RefusedOrFailed), [2, 3], "allowed:false covers refused and failed");
        assert_eq!(c.kinds(), ["click", "invoke", "key", "look"]);
    }

    #[test]
    fn hands_are_heard_only_when_they_change() {
        let mut seen = None;
        let armed = json!({"type": "working", "handle": 4242, "armed": true, "title": "Notepad", "rect": [0, 0, 10, 10]});
        assert!(hands_changed(&armed, &mut seen));
        assert!(!hands_changed(&armed, &mut seen), "the same again, twice a second, is not passed on");
        let moved = json!({"type": "working", "handle": 77, "armed": true, "title": "Mail", "rect": [0, 0, 10, 10]});
        assert!(hands_changed(&moved, &mut seen));
        assert!(hands_changed(&json!({"type": "working", "handle": 77, "armed": false}), &mut seen), "disarmed is heard");
        assert!(hands_changed(&json!({"type": "state", "s": "idle"}), &mut seen), "other messages pass");
        // as the loop writes it, "type" first
        assert!(keep(r#"{"type": "working", "handle": 4242, "armed": true, "title": "Notepad", "rect": [0, 0, 10, 10]}"#).is_some());
        let mut c = Conversation::default();
        assert_eq!(c.hands, None, "unknown until the loop says");
        c.absorb(&armed);
        assert_eq!(c.hands, Some(Hands { armed: true, handle: 4242, title: "Notepad".into() }));
    }

    #[test]
    fn commands_are_the_loops_own() {
        assert_eq!(say("hello"), json!({"type": "text", "text": "hello"}));
        assert_eq!(listen(true), json!({"type": "listen", "on": true}));
        assert_eq!(decide("h-7", false), json!({"type": "decide", "id": "h-7", "answer": "no"}));
        assert_eq!(watch(132_456), json!({"type": "watch", "on": true, "mine": 132_456}));
    }
}
