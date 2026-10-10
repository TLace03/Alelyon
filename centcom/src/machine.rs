//! The Machine: what Sinai keeps and how it listens, read from its loop, read-only.
//!
//! - Its memory, as the loop's `memory` snapshot says (`loop/inner.py` `snapshot`; sent to every client as it
//!   connects and again after each change): whether it remembers, the newest saved conversations of all it keeps and
//!   each one's status, the trash (each erase, restorable), what it has observed of windows (working and saved), the
//!   memory map's size, and how recall by meaning is doing. A snapshot older than the one shown, or from before an
//!   erase, is dropped, as the Angel window drops it (`link.rs`): a queued snapshot must not bring back what was erased.
//! - How it listens, from the loop's own `/about` (`being_loop.py` `handle_about`): the follow-up window, whether a
//!   spoken command can grant authority, and the voice gate (no measured voiceprint, open, or refused), with its
//!   threshold and the probe run that measured it. Only that part of `/about` is read; the rest (its system prompt,
//!   its anatomy) is not kept.
//!
//! It acts as the Angel window does (`inner_ui.rs`): each act is
//! the loop's own `memory_control` message, sent as the Angel window sends it, and the loop does the rest. Remembering
//! on or off, marking a conversation, restoring an erase and saving observations on or off are one press (there is no
//! Refresh: the loop sends a snapshot on connecting and after every change, so the one on show is the newest); erasing (to the trash, restorable) and clearing the working views ask once; deleting for good and
//! emptying the trash ask twice, because nothing brings them back. Erasing is the person's act, never Sinai's
//! (`CHARTER.md`): nothing here acts unless a person presses.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

/// The loop's HTTP port (its websocket's).
pub const LOOP_PORT: u16 = 8180;

/// The port /about is asked on: the websocket's, so a stand-in the build names answers both.
pub fn loop_port() -> u16 {
    crate::sinai::url().and_then(|url| port_of(&url)).unwrap_or(LOOP_PORT)
}

fn port_of(url: &str) -> Option<u16> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    host.rsplit_once(':')?.1.parse().ok()
}
/// How often the listening rules are read again while the Sinai page is open: they change only when the loop restarts.
pub const ABOUT_EVERY_S: f64 = 60.0;
/// The newest saved conversations listed, and observations.
pub const EPISODES_SHOWN: usize = 40;
pub const VIEWS_SHOWN: usize = 12;

fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// One saved conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub id: String,
    pub created: String,
    pub status: String,
    pub user: String,
    pub assistant: String,
}

/// One erase in the trash.
#[derive(Clone, Debug, PartialEq)]
pub struct Erased {
    /// The erase's batch: what Restore and Delete permanently name.
    pub batch: String,
    pub when: String,
    /// Counts as the loop sent them; None when it did not ("?", never zero).
    pub episodes: Option<u64>,
    pub views: Option<u64>,
    pub record: Option<u64>,
    pub glimpse: String,
}

/// One observation of a window.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub id: String,
    pub title: String,
    pub created: String,
    pub description: String,
    /// Where each half of the account came from, as the loop labels it: "model-reported" or "unavailable" for the
    /// vision model's words, "OS-reported" or "unavailable" for the accessibility reading (`visual_memory.py`).
    pub vision: String,
    pub accessibility: String,
    /// The controls the accessibility reading named: name and role.
    pub controls: Vec<(String, String)>,
    /// What the vision model located on it, when it was asked to find something.
    pub located: String,
}

/// The memory, as the last snapshot kept said.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Memory {
    pub version: Option<(u64, u64)>,
    pub on: bool,
    pub error: String,
    pub path: String,
    pub total: Option<u64>,
    pub episodes: Vec<Episode>,
    pub trash: Vec<Erased>,
    pub visual_status: String,
    pub visual_observing: bool,
    pub visual_saving: bool,
    pub working: Vec<View>,
    pub working_count: usize,
    pub saved: Vec<View>,
    pub saved_count: usize,
    /// The memory map: its nodes and links, as the loop's graph counts them, and the map itself to draw.
    pub nodes: Option<usize>,
    pub edges: Option<usize>,
    pub map: Option<Map>,
    pub meaning: String,
    /// Whether the forward pass is sampled on the next turn.
    pub forward_on: bool,
}

fn views(v: &Value) -> (Vec<View>, usize) {
    let rows = v.as_array().map(Vec::as_slice).unwrap_or_default();
    let shown = rows.iter().take(VIEWS_SHOWN).map(|r| View {
        id: text(r, "id"),
        title: text(r, "title"),
        created: text(r, "created"),
        description: text(r, "description"),
        vision: text(r, "vision"),
        accessibility: text(r, "accessibility"),
        controls: r["controls"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .take(16)
            .map(|c| (text(c, "name"), text(c, "role")))
            .filter(|(name, _)| !name.is_empty())
            .collect(),
        located: text(r, "located_label"),
    });
    (shown.collect(), rows.len())
}

impl Memory {
    /// Read a `memory` snapshot.
    pub fn from_snapshot(v: &Value) -> Memory {
        let version = v.get("epoch").and_then(Value::as_u64).zip(v.get("revision").and_then(Value::as_u64));
        let episodes = v
            .get("episodes")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .take(EPISODES_SHOWN)
                    .map(|e| Episode {
                        id: text(e, "id"),
                        created: text(e, "created"),
                        status: text(e, "status"),
                        user: text(e, "user"),
                        assistant: text(e, "assistant"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        // As the Angel window's trash_rows: a row without its batch could not be restored, so it is left out.
        let trash = v
            .get("trash")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter(|t| t.get("batch").and_then(Value::as_str).is_some_and(|b| !b.is_empty()))
                    .map(|t| Erased {
                        batch: text(t, "batch"),
                        when: text(t, "erased"),
                        episodes: t.get("episodes").and_then(Value::as_u64),
                        views: t.get("views").and_then(Value::as_u64),
                        record: t.get("record").and_then(Value::as_u64),
                        glimpse: text(t, "glimpse"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (working, working_count) = views(&v["working_views"]);
        let (saved, saved_count) = views(&v["saved_views"]);
        let count = |key: &str| match &v["brain"][key] {
            Value::Array(a) => Some(a.len()),
            Value::Object(m) => Some(m.len()),
            _ => None,
        };
        let meaning = match v.get("meaning") {
            Some(m) if m.is_object() => {
                let mut s = text(m, "status");
                if let Some(coded) = m.get("coded").and_then(Value::as_u64) {
                    s = format!("{s} · {coded} coded");
                }
                s
            }
            _ => String::new(),
        };
        Memory {
            version,
            on: v.get("on").and_then(Value::as_bool).unwrap_or(false),
            error: text(v, "error"),
            path: text(v, "path"),
            total: v.get("total").and_then(Value::as_u64),
            episodes,
            trash,
            visual_status: text(v, "visual_status"),
            visual_observing: v.get("visual_observing").and_then(Value::as_bool).unwrap_or(false),
            visual_saving: v.get("visual_saving").and_then(Value::as_bool).unwrap_or(false),
            working,
            working_count,
            saved,
            saved_count,
            nodes: count("nodes"),
            edges: count("links"),
            map: Map::from_brain(&v["brain"]),
            meaning,
            forward_on: v.get("forward_on").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// One record on the memory map, at its display cell (`memory_graph.py`: 8 by 8 by 3, a grid for showing, not
/// neurons or model KV).
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: String,
    pub title: String,
    pub created: String,
    pub at: [u8; 3],
    /// Included in the last prepared request (gold), or only retained (blue).
    pub included: bool,
    /// What the record says, when it is one of the observations the snapshot carries.
    pub description: String,
}

/// The memory map, as the loop's evidence graph gives it (schema 1).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Map {
    pub cells: Vec<Cell>,
    /// Links by place in `cells`: from, to.
    pub links: Vec<(usize, usize)>,
    pub retrieval: String,
    pub kv_status: String,
    pub layout: String,
    pub rule: String,
}

/// A record's display cell, only when it is a real one (as the Angel window's `brain_ui.rs` checks).
fn cell_of(node: &Value) -> Option<[u8; 3]> {
    let c = node.get("cell")?.as_array()?;
    if c.len() != 3 {
        return None;
    }
    let xyz = [c[0].as_u64()?, c[1].as_u64()?, c[2].as_u64()?];
    (xyz[0] < 8 && xyz[1] < 8 && xyz[2] < 3).then_some([xyz[0] as u8, xyz[1] as u8, xyz[2] as u8])
}

impl Map {
    /// Read the snapshot's `brain`; None when the loop does not provide the scene graph (no schema 1).
    pub fn from_brain(brain: &Value) -> Option<Map> {
        if brain.get("schema").and_then(Value::as_u64) != Some(1) {
            return None;
        }
        let included: Vec<&str> =
            brain["retrieval"]["included"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        // as the Angel window draws them: the first 192 records, and only those with a real cell
        let cells: Vec<Cell> = brain["nodes"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .take(192)
            .filter_map(|n| {
                let id = text(n, "id");
                Some(Cell {
                    included: included.contains(&id.as_str()),
                    title: text(n, "title"),
                    created: text(n, "created"),
                    at: cell_of(n)?,
                    description: String::new(),
                    id,
                })
            })
            .collect();
        let at = |id: &str| cells.iter().position(|c| c.id == id);
        let links = brain["links"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .take(576)
            .filter_map(|l| Some((at(&text(l, "from"))?, at(&text(l, "to"))?)))
            .collect();
        Some(Map {
            cells,
            links,
            retrieval: text(&brain["retrieval"], "status"),
            kv_status: text(brain, "kv_status"),
            layout: text(brain, "layout"),
            rule: text(brain, "rule"),
        })
    }

    /// Give each cell its observation's words, from the snapshot's working and saved views.
    fn describe(&mut self, snapshot: &Value) {
        for key in ["working_views", "saved_views"] {
            for row in snapshot[key].as_array().map(Vec::as_slice).unwrap_or_default() {
                let id = text(row, "id");
                if let Some(c) = self.cells.iter_mut().find(|c| c.id == id && c.description.is_empty()) {
                    c.description = text(row, "description");
                }
            }
        }
    }
}

/// The last turn's ledger in lines, as the Angel window writes them (`inner_ui.rs` `last_turn_lines`): a reading the
/// loop did not send is "unmeasured", never a zero.
pub fn last_turn_lines(v: &Value) -> Vec<String> {
    if !v.is_object() {
        return vec!["No turn measured yet this session.".to_owned()];
    }
    let reading =
        |key: &str, f: &dyn Fn(f64) -> String| v.get(key).and_then(Value::as_f64).map(f).unwrap_or_else(|| "unmeasured".to_owned());
    let ms = |key: &str| reading(key, &|x| format!("{x:.0} ms"));
    let pct = |key: &str| reading(key, &|x| format!("{:.0}%", x * 100.0));
    let count = |key: &str| v.get(key).and_then(Value::as_u64);
    let tokens_in = match (count("prompt_tokens"), count("cached_tokens")) {
        (Some(all), Some(cached)) => format!("{all} ({cached} reused from cache)"),
        (Some(all), None) => format!("{all}"),
        (None, _) => "unmeasured".to_owned(),
    };
    let tokens_out = count("completion_tokens").map(|n| n.to_string()).unwrap_or_else(|| "unmeasured".to_owned());
    // the loop sends 0.0 tokens/s when it had nothing to divide: a rate is a reading only with tokens behind it
    let rate = match (count("total_tokens"), v.get("tokens_per_sec").and_then(Value::as_f64)) {
        (Some(n), Some(r)) if n > 0 => format!("{r:.1} tokens/s"),
        _ => "unmeasured".to_owned(),
    };
    let flag = |key: &str| match v.get(key).and_then(Value::as_bool) {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unmeasured",
    };
    vec![
        "Where the time went, after you stopped speaking:".to_owned(),
        format!("  heard, transcript ready   {}", ms("stt_done_ms")),
        format!("  first token               {}", ms("first_token_ms")),
        format!("  first sentence            {}", ms("first_sentence_ms")),
        format!("  first audio, Sinai speaks {}", ms("first_audio_ms")),
        format!("Tokens in: {tokens_in}"),
        format!("Tokens out: {tokens_out} · {rate}"),
        format!(
            "Confidence (top-k): {} · longest pause between tokens: {}",
            pct("confidence_topk"),
            reading("max_token_gap_s", &|x| format!("{x:.2} s"))
        ),
        format!("Context used: {} · prompt reused from cache: {}", pct("context_fraction"), pct("cache_fraction")),
        format!("Interrupted: {} · opener used: {}", flag("interrupted"), flag("opener")),
    ]
}

/// The engine's retained sequence positions, when the forward pass measured them and they hold together (the Angel
/// window's `kv_span`): span, capacity, lowest and highest position.
pub fn kv_span(forward: &Value) -> Option<(u64, u64, i64, i64)> {
    if forward["status"] != "measured" || forward["kv"]["status"] != "measured" {
        return None;
    }
    let kv = &forward["kv"];
    let capacity = kv["context_capacity"].as_u64()?;
    let span = kv["position_span"].as_u64()?;
    let low = kv["position_min"].as_i64()?;
    let high = kv["position_max"].as_i64()?;
    let expected = if low == -1 && high == -1 {
        0
    } else if low >= 0 && high >= low && high <= i32::MAX as i64 {
        (high - low + 1) as u64
    } else {
        return None;
    };
    (kv["source"] == "llama_memory_seq_positions"
        && kv["sequence"].as_u64() == Some(0)
        && (1..=1_048_576).contains(&capacity)
        && span == expected
        && span <= capacity)
        .then_some((span, capacity, low, high))
}

/// One layer's sampled reading: layer, signal, RMS, largest magnitude.
pub fn layer_readings(forward: &Value) -> Vec<(u64, String, f64, f64)> {
    forward["readings"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .take(384)
        .filter_map(|r| Some((r["layer"].as_u64()?, text(r, "signal"), r["rms"].as_f64()?, r["max_abs"].as_f64()?)))
        .collect()
}

/// Take a snapshot or an erase into `held`, by the Angel window's rule: a snapshot is kept only when it is newer than
/// the one held (by epoch, then revision; an unversioned one only when nothing is held), and an erase of an epoch older
/// than the one held is ignored, while a current one empties the memory shown.
pub fn absorb(held: &mut Option<Memory>, version: &mut Option<(u64, u64)>, v: &Value) {
    match v.get("type").and_then(Value::as_str) {
        Some("memory") => {
            let next = v.get("epoch").and_then(Value::as_u64).zip(v.get("revision").and_then(Value::as_u64));
            if version.is_none() || next.is_some_and(|n| Some(n) > *version) {
                *version = next;
                let mut m = Memory::from_snapshot(v);
                if let Some(map) = m.map.as_mut() {
                    map.describe(v);
                }
                *held = Some(m);
            }
        }
        Some("memory_erased") => {
            if let Some(epoch) = v.get("epoch").and_then(Value::as_u64) {
                if version.is_some_and(|old| old.0 > epoch) {
                    return;
                }
                *version = Some(version.unwrap_or((0, 0)).max((epoch, 0)));
            }
            *held = None;
        }
        _ => {}
    }
}

/// The statuses a person may mark a conversation with, as the Angel window offers them.
pub const STATUSES: [&str; 3] = ["open", "answered", "dead_end"];

/// One act on Sinai's memory, as the loop's `memory_control` takes it (`being_loop.py`; the Angel window's
/// `inner_ui.rs`).
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Remember completed turns, or not.
    Remember(bool),
    /// Mark a conversation's status.
    Mark { id: String, status: String },
    /// Move one conversation or saved observation to the trash and clear the current chat; with no id, everything.
    Erase { id: Option<String>, what: String },
    /// Put an erase back.
    Restore { batch: String },
    /// Delete one erase for good, or with no batch empty the whole trash.
    Purge { batch: Option<String>, what: String },
    /// Observe a window while idle (at most once every 5 s), or stop.
    Observe(Option<(i64, String)>),
    /// Clear the working views and the current chat.
    ForgetVisual,
    /// Keep observations on this PC, or not.
    SaveVisual(bool),
    /// Sample the model's layers and KV positions on the next turn, or stop (the observatory's tier 2).
    Forward(bool),
}

impl Act {
    /// The message the loop takes for this act.
    pub fn message(&self) -> Value {
        match self {
            Act::Remember(on) => json!({"type": "memory_control", "action": "enable", "on": on}),
            Act::Mark { id, status } => json!({"type": "memory_control", "action": "mark", "id": id, "status": status}),
            Act::Erase { id: Some(id), .. } => json!({"type": "memory_control", "action": "erase", "id": id}),
            Act::Erase { id: None, .. } => json!({"type": "memory_control", "action": "erase"}),
            Act::Restore { batch } => json!({"type": "memory_control", "action": "restore", "batch": batch}),
            Act::Purge { batch: Some(b), .. } => json!({"type": "memory_control", "action": "purge", "batch": b}),
            Act::Purge { batch: None, .. } => json!({"type": "memory_control", "action": "purge"}),
            Act::Observe(Some((handle, title))) => {
                json!({"type": "memory_control", "action": "observe", "on": true, "handle": handle, "title": title})
            }
            Act::Observe(None) => json!({"type": "memory_control", "action": "observe", "on": false}),
            Act::ForgetVisual => json!({"type": "memory_control", "action": "forget_visual"}),
            Act::SaveVisual(on) => json!({"type": "memory_control", "action": "save_visual", "on": on}),
            Act::Forward(on) => json!({"type": "forward_control", "on": on}),
        }
    }

    /// How many times a person confirms it before it is sent: none, once, or twice for what cannot be undone.
    pub fn confirmations(&self) -> u8 {
        match self {
            Act::Purge { .. } => 2,
            Act::Erase { .. } | Act::ForgetVisual => 1,
            _ => 0,
        }
    }

    /// What a confirmation says, the `step`th time (1 or 2).
    pub fn question(&self, step: u8) -> String {
        match (self, step) {
            (Act::Erase { id: None, .. }, _) => {
                "Move every saved conversation and observation to the trash, stop observing, and clear the \
                                                current chat? You can restore them from the trash."
                    .into()
            }
            (Act::Erase { what, .. }, _) => {
                format!("Move {what} to the trash and clear the current chat? You can restore it from the trash.")
            }
            (Act::ForgetVisual, _) => "Clear the working views and the current chat?".into(),
            (Act::Purge { batch: None, .. }, 1) => {
                "Empty the trash? Everything in it is deleted permanently and cannot be restored.".into()
            }
            (Act::Purge { what, .. }, 1) => format!("Delete {what} permanently? It cannot be restored."),
            (Act::Purge { .. }, _) => "Are you sure? This is the last step: once deleted, nothing brings it back.".into(),
            _ => String::new(),
        }
    }
}

/// An act waiting for its confirmations: `given` of `act.confirmations()` so far.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub act: Act,
    pub given: u8,
}

/// The next step after a press: send it now, or ask (again).
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Send(Value),
    Ask(Pending),
}

/// A person pressed for `act`: send it, or ask first.
pub fn press(act: Act) -> Step {
    if act.confirmations() == 0 { Step::Send(act.message()) } else { Step::Ask(Pending { act, given: 0 }) }
}

/// A person confirmed `pending` once more: send it after its last confirmation, or ask again.
pub fn confirm(pending: Pending) -> Step {
    let given = pending.given + 1;
    if given >= pending.act.confirmations() { Step::Send(pending.act.message()) } else { Step::Ask(Pending { given, ..pending }) }
}

/// Sinai's sight as the loop last said (`sight`): its vision model open, with how long it took to load, or closed
/// (after ten minutes unused, the next look opens it again).
#[derive(Clone, Debug, PartialEq)]
pub enum Sight {
    Open { took_s: Option<f64> },
    Closed { why: String },
}

impl Sight {
    pub fn from_message(v: &Value) -> Option<Sight> {
        if v.get("type").and_then(Value::as_str) != Some("sight") {
            return None;
        }
        Some(match v.get("ready").and_then(Value::as_bool) {
            Some(true) => Sight::Open { took_s: v.get("took_s").and_then(Value::as_f64) },
            _ => Sight::Closed { why: text(v, "closed") },
        })
    }

    /// In words, for the Machine.
    pub fn words(&self) -> String {
        match self {
            Sight::Open { took_s: Some(s) } => format!("Its eyes are open (the vision model took {s:.1} s to load)."),
            Sight::Open { took_s: None } => "Its eyes are open.".into(),
            Sight::Closed { why } if why == "idle" => {
                "Its eyes are closed: unused for ten minutes, they closed to free the graphics card; the next look opens them.".into()
            }
            Sight::Closed { .. } => "Its eyes are closed; the next look opens them.".into(),
        }
    }
}

/// How Sinai listens, from `/about`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Listening {
    pub microphone_open: Option<bool>,
    /// None: the follow-up window never lapses (the loop's "always"), or it was not said.
    pub follow_up_s: Option<f64>,
    pub follow_up_said: bool,
    pub voice_authority: Option<bool>,
    pub gate_status: String,
    pub gate_threshold: Option<f64>,
    pub gate_min_seconds: Option<f64>,
    pub gate_measured_by: String,
    pub listen_at_start: Option<bool>,
    pub overheard: Option<u64>,
}

impl Listening {
    /// Read the `listening` part of an `/about` reply; nothing else of it is kept.
    pub fn from_about(about: &Value) -> Result<Listening, String> {
        let l = about.get("listening").filter(|l| l.is_object()).ok_or("the loop's /about has no listening section")?;
        let gate = &l["voice_gate"];
        Ok(Listening {
            microphone_open: l.get("microphone_open").and_then(Value::as_bool),
            follow_up_said: l.get("follow_up_s").is_some(),
            follow_up_s: l.get("follow_up_s").and_then(Value::as_f64),
            voice_authority: l.get("voice_authority").and_then(Value::as_bool),
            gate_status: text(gate, "status"),
            gate_threshold: gate.get("threshold").and_then(Value::as_f64),
            gate_min_seconds: gate.get("min_seconds").and_then(Value::as_f64),
            gate_measured_by: text(gate, "measured_by"),
            listen_at_start: l.get("listen_at_start").and_then(Value::as_bool),
            overheard: l.get("overheard_and_discarded").and_then(Value::as_u64),
        })
    }

    /// The follow-up window in words.
    pub fn follow_up(&self) -> String {
        match (self.follow_up_said, self.follow_up_s) {
            (false, _) => "not said".into(),
            (true, None) => "never lapses: a conversation stays open until it is ended".into(),
            (true, Some(s)) => format!("{s:.0} s after Sinai speaks, speech reaches it without its name"),
        }
    }
}

/// GET `/about` from the loop on this PC, bounded: 5 s to answer, 4 MiB at most.
pub fn read_about(port: u16) -> Result<Listening, String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| format!("Sinai's loop did not answer: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let head = format!("GET /about HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).map_err(|e| format!("Sinai's loop stopped answering: {e}"))?;
    let mut raw = Vec::new();
    stream.take((4 << 20) + 1).read_to_end(&mut raw).map_err(|e| format!("Sinai's loop stopped answering: {e}"))?;
    if raw.len() > 4 << 20 {
        return Err("Sinai's /about is larger than 4 MiB.".into());
    }
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("Sinai's /about reply has no end of headers.")?;
    let status = std::str::from_utf8(&raw[..split]).ok().and_then(|h| h.split_whitespace().nth(1)).unwrap_or("");
    if status != "200" {
        return Err(format!("Sinai's /about answered {status}"));
    }
    let about: Value = serde_json::from_slice(&raw[split + 4..]).map_err(|e| format!("Sinai's /about is not JSON: {e}"))?;
    Listening::from_about(&about)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(epoch: u64, revision: u64, n: usize) -> Value {
        let episodes: Vec<Value> = (0..n)
            .map(|i| json!({"id": format!("e{i}"), "created": "2026-10-07", "status": "open", "user": "hi", "assistant": "hello"}))
            .collect();
        json!({"type": "memory", "on": true, "epoch": epoch, "revision": revision, "path": "C:/m.sqlite", "total": 90,
               "episodes": episodes, "trash": [{"batch": "b1", "erased": "today", "episodes": 2, "glimpse": "a talk"}, {"erased": "x"}],
               "working_views": [{"title": "Notepad", "created": "now", "description": "a text"}], "saved_views": [],
               "brain": {"nodes": [1, 2, 3], "links": []}, "meaning": {"status": "ready", "coded": 12},
               "visual_status": "not observing"})
    }

    #[test]
    fn a_snapshot_is_read_and_a_trash_row_without_its_batch_is_left_out() {
        let m = Memory::from_snapshot(&snapshot(1, 1, 3));
        assert!(m.on);
        assert_eq!((m.total, m.episodes.len(), m.episodes[2].id.as_str()), (Some(90), 3, "e2"));
        assert_eq!(m.trash.len(), 1, "no batch, nothing could restore it");
        assert_eq!((m.trash[0].episodes, m.trash[0].views), (Some(2), None), "a count not sent stays unknown");
        assert_eq!((m.working_count, m.saved_count, m.nodes, m.edges), (1, 0, Some(3), Some(0)));
        assert_eq!(m.meaning, "ready · 12 coded");
        assert_eq!(Memory::from_snapshot(&snapshot(1, 1, 100)).episodes.len(), EPISODES_SHOWN);
    }

    #[test]
    fn an_older_snapshot_or_one_from_before_an_erase_never_brings_memory_back() {
        let (mut held, mut version) = (None, None);
        absorb(&mut held, &mut version, &snapshot(1, 5, 2));
        assert_eq!(held.as_ref().map(|m| m.episodes.len()), Some(2));
        absorb(&mut held, &mut version, &snapshot(1, 4, 9));
        assert_eq!(held.as_ref().map(|m| m.episodes.len()), Some(2), "older revision dropped");
        absorb(&mut held, &mut version, &json!({"type": "memory_erased", "epoch": 2}));
        assert!(held.is_none(), "an erase empties what is shown");
        absorb(&mut held, &mut version, &snapshot(1, 9, 9));
        assert!(held.is_none(), "a snapshot queued before the erase is dropped");
        absorb(&mut held, &mut version, &json!({"type": "memory_erased", "epoch": 1}));
        absorb(&mut held, &mut version, &snapshot(2, 1, 1));
        assert_eq!(held.as_ref().map(|m| m.episodes.len()), Some(1), "the next epoch's snapshot is shown");
    }

    #[test]
    fn only_the_listening_part_of_about_is_read() {
        let about = json!({"name": "Sinai", "system_prompt": "private", "listening": {"microphone_open": false,
            "follow_up_s": null, "voice_authority": true, "voice_gate": {"status": "no measured voiceprint",
            "threshold": null, "min_seconds": 1.9, "measured_by": null}, "listen_at_start": true, "overheard_and_discarded": 4}});
        let l = Listening::from_about(&about).unwrap();
        assert_eq!(l.follow_up(), "never lapses: a conversation stays open until it is ended");
        assert_eq!(
            (l.voice_authority, l.gate_status.as_str(), l.gate_min_seconds, l.overheard),
            (Some(true), "no measured voiceprint", Some(1.9), Some(4))
        );
        let l = Listening::from_about(&json!({"listening": {"follow_up_s": 8.0}})).unwrap();
        assert!(l.follow_up().starts_with("8 s"));
        assert!(Listening::from_about(&json!({"name": "x"})).is_err());
        assert_eq!(Listening::from_about(&json!({"listening": {}})).unwrap().follow_up(), "not said");
    }

    #[test]
    fn sight_open_or_closed_is_said_as_the_loop_said_it() {
        assert_eq!(Sight::from_message(&json!({"type": "sight", "ready": true, "took_s": 6.4})), Some(Sight::Open { took_s: Some(6.4) }));
        let idle = Sight::from_message(&json!({"type": "sight", "ready": false, "closed": "idle"})).unwrap();
        assert!(idle.words().contains("unused for ten minutes"));
        assert!(Sight::from_message(&json!({"type": "mic"})).is_none());
        assert!(Sight::Open { took_s: Some(6.4) }.words().contains("6.4 s"));
    }

    #[test]
    fn an_observation_keeps_what_each_half_of_it_rests_on() {
        let (v, n) = views(&json!([{"id": "v1", "title": "Notepad", "description": "a list", "vision": "model-reported",
            "accessibility": "OS-reported", "controls": [{"name": "Save", "role": "button"}, {"name": "", "role": "pane"}],
            "located_label": "Save"}]));
        assert_eq!(n, 1);
        assert_eq!((v[0].vision.as_str(), v[0].accessibility.as_str(), v[0].located.as_str()), ("model-reported", "OS-reported", "Save"));
        assert_eq!(v[0].controls, [("Save".to_string(), "button".to_string())], "a control without a name is left out");
    }

    #[test]
    fn the_map_keeps_real_cells_their_links_and_what_the_last_request_used() {
        let brain = json!({"schema": 1, "kv_status": "unavailable", "layout": "grid", "rule": "lexical",
            "retrieval": {"status": "ok", "included": ["v2"]},
            "nodes": [{"id": "v1", "title": "Notepad", "cell": [0, 0, 0]}, {"id": "v2", "title": "Mail", "cell": [1, 0, 0]},
                      {"id": "bad", "cell": [8, 0, 0]}, {"id": "short", "cell": [0, 0]}, {"id": "neg", "cell": [0, -1, 0]}],
            "links": [{"from": "v1", "to": "v2"}, {"from": "v1", "to": "bad"}]});
        let m = Map::from_brain(&brain).unwrap();
        assert_eq!(m.cells.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["v1", "v2"], "invalid cells are not drawn");
        assert_eq!((m.cells[0].included, m.cells[1].included), (false, true));
        assert_eq!(m.links, [(0, 1)], "a link to a cell not drawn is dropped");
        assert!(Map::from_brain(&json!({"nodes": []})).is_none(), "no scene graph without schema 1");
        let mut snap = snapshot(1, 1, 1);
        snap["brain"] = brain;
        snap["working_views"] = json!([{"id": "v1", "title": "Notepad", "description": "a shopping list"}]);
        let (mut held, mut version) = (None, None);
        absorb(&mut held, &mut version, &snap);
        let m = held.unwrap();
        assert_eq!(m.edges, Some(2), "the loop's links, counted");
        assert_eq!(m.map.unwrap().cells[0].description, "a shopping list");
    }

    #[test]
    fn the_last_turn_says_unmeasured_never_zero() {
        assert_eq!(last_turn_lines(&Value::Null), ["No turn measured yet this session."]);
        let full = json!({"stt_done_ms": 412.0, "first_token_ms": 603.4, "prompt_tokens": 1204, "cached_tokens": 1180,
            "completion_tokens": 87, "total_tokens": 87, "tokens_per_sec": 38.24, "context_fraction": 0.12, "interrupted": false});
        let text = last_turn_lines(&full).join("\n");
        for want in ["412 ms", "603 ms", "1204 (1180 reused from cache)", "Tokens out: 87", "38.2 tokens/s", "12%", "Interrupted: no"] {
            assert!(text.contains(want), "{want} in {text}");
        }
        assert!(text.contains("first audio, Sinai speaks unmeasured"));
        let none = last_turn_lines(&json!({"total_tokens": 0, "tokens_per_sec": 0.0})).join("\n");
        assert!(none.contains("Tokens out: unmeasured · unmeasured"), "no tokens, no rate: {none}");
    }

    #[test]
    fn kv_positions_are_shown_only_when_they_hold_together() {
        let fwd = |min: i64, max: i64, span: u64| {
            json!({"status": "measured", "kv": {"status": "measured",
            "source": "llama_memory_seq_positions", "sequence": 0, "context_capacity": 8192,
            "position_span": span, "position_min": min, "position_max": max}})
        };
        assert_eq!(kv_span(&fwd(0, 99, 100)), Some((100, 8192, 0, 99)));
        assert_eq!(kv_span(&fwd(-1, -1, 0)), Some((0, 8192, -1, -1)), "empty");
        assert_eq!(kv_span(&fwd(0, 99, 50)), None, "a span that does not match its range");
        assert_eq!(kv_span(&json!({"status": "unavailable"})), None);
        let r = layer_readings(&json!({"readings": [{"layer": 3, "signal": "hidden", "rms": 0.5, "max_abs": 2.0}, {"layer": "x"}]}));
        assert_eq!(r, [(3, "hidden".to_string(), 0.5, 2.0)]);
        assert_eq!(Act::Forward(true).message(), json!({"type": "forward_control", "on": true}));
    }

    #[test]
    fn each_act_is_the_loops_own_message_as_the_angel_window_sends_it() {
        assert_eq!(Act::Remember(false).message(), json!({"type": "memory_control", "action": "enable", "on": false}));
        assert_eq!(
            Act::Mark { id: "e1".into(), status: "dead_end".into() }.message(),
            json!({"type": "memory_control", "action": "mark", "id": "e1", "status": "dead_end"})
        );
        assert_eq!(Act::Erase { id: None, what: String::new() }.message(), json!({"type": "memory_control", "action": "erase"}));
        assert_eq!(Act::Erase { id: Some("v3".into()), what: String::new() }.message()["id"], "v3");
        assert_eq!(Act::Restore { batch: "b7".into() }.message(), json!({"type": "memory_control", "action": "restore", "batch": "b7"}));
        assert_eq!(Act::Purge { batch: None, what: String::new() }.message(), json!({"type": "memory_control", "action": "purge"}));
        assert_eq!(Act::Purge { batch: Some("b7".into()), what: String::new() }.message()["batch"], "b7");
        assert_eq!(
            Act::Observe(Some((42, "Notepad".into()))).message(),
            json!({"type": "memory_control", "action": "observe", "on": true, "handle": 42, "title": "Notepad"})
        );
        assert_eq!(Act::Observe(None).message(), json!({"type": "memory_control", "action": "observe", "on": false}));
        assert_eq!(Act::ForgetVisual.message(), json!({"type": "memory_control", "action": "forget_visual"}));
        assert_eq!(Act::SaveVisual(true).message(), json!({"type": "memory_control", "action": "save_visual", "on": true}));
    }

    #[test]
    fn what_cannot_be_undone_is_asked_twice_and_an_erase_once() {
        assert!(matches!(press(Act::Restore { batch: "b".into() }), Step::Send(_)), "a restore is one press");
        let Step::Ask(p) = press(Act::Erase { id: None, what: String::new() }) else { panic!("an erase asks") };
        assert!(matches!(confirm(p), Step::Send(_)), "once");
        let Step::Ask(p) = press(Act::Purge { batch: None, what: String::new() }) else { panic!("a purge asks") };
        assert!(p.act.question(1).contains("cannot be restored"));
        let Step::Ask(p) = confirm(p) else { panic!("a purge asks twice") };
        assert_eq!(p.given, 1);
        assert!(p.act.question(2).contains("last step"));
        assert_eq!(confirm(p), Step::Send(json!({"type": "memory_control", "action": "purge"})));
    }

    #[test]
    fn about_is_asked_on_the_websockets_port() {
        assert_eq!(port_of("ws://127.0.0.1:18180/ws"), Some(18180));
        assert_eq!(port_of("ws://127.0.0.1/ws"), None);
    }

    #[test]
    fn a_loop_that_is_not_running_is_said_so() {
        // a port nothing listens on, on loopback
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(read_about(free).unwrap_err().contains("did not answer"));
    }
}
