//! Sinai's page with a model of the person's own: the chat strip that talks to it, and the engine that reaches it.
//!
//! The model is one of two kinds (`models::Pick`):
//! - **a GGUF file** in the models folder, run by lattice-core's managed llama.cpp server (`LlamaRuntime`):
//!   started by this process on the first message, on 127.0.0.1 with a token made for that launch (only this process
//!   holds it, so only this process uses the server), inside a Job Object that ends it with Alelyon, and stopped after
//!   its settings' idle time without a message. The model is fixed at launch; another model restarts it;
//! - **an OpenAI-compatible endpoint** from lattice-core's registry, reached through lattice-core's own client
//!   (`models::build`: the key read at that moment from where lattice-core finds keys, no proxy for an address on this
//!   PC, errors worded without the response, the key or the address's credentials).
//!
//! The reply streams in as it is written. Sinai's face follows it: its expression thinks while the model loads and
//! reads the message, its mouth moves with each piece of the answer, and it is at rest again when the answer ends; it
//! looks up, listening, while the person types. Nothing here is Sinai's mind: the page says which model answers.
//!
//! Nothing is kept: the conversation lives in this window's memory and goes with it. What is typed goes only to the
//! chosen model; an endpoint off this PC is named and asked about before the first message of the session goes there.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use futures::StreamExt;
use futures::stream::BoxStream;
use iced::Task;
use serde_json::json;

use lattice_agents::{InputItem, ModelEvent, ModelRequest, ModelSettings, OutputItem};
use lattice_core::llama::server::{LlamaRuntime, RuntimeConfig};
use lattice_core::llama::{self, files};
use lattice_core::registry::{self, EndpointKind};
use lattice_core::{Env, KeyStore, StateRoot};

/// What the model is told about where it is. It answers in Sinai's page, not as Sinai's mind.
pub const SYSTEM: &str = "You are answering on the Sinai page of Alelyon, a desktop app, in place of Sinai's own mind, \
which is not the one answering now. Answer the person plainly and briefly.";
/// The most lines of the conversation sent with a message.
pub const HISTORY: usize = 24;
/// The longest answer asked for, in tokens.
pub const MAX_TOKENS: u32 = 1024;

/// Which model a message goes to, as the strip asks for it; resolved again, from the disk, when the message is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A GGUF file of the models folder, by its name, run by the managed llama.cpp server.
    Gguf { name: String, models_dir: PathBuf },
    /// An OpenAI-compatible endpoint of the registry, by its id.
    Endpoint { id: String, registry: PathBuf },
}

/// What the engine says while a message is answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// llama.cpp's server is starting with this model (a large model takes a while to load).
    Loading(String),
    /// The model has the message and has not written anything yet.
    Thinking,
    /// A piece of the answer.
    Delta(String),
    /// The answer is complete; the text is the whole answer when no piece of it came on its own.
    Done(String),
    /// Why there is no answer, in one sentence.
    Failed(String),
}

/// The tokio runtime the model clients run on, and the managed llama.cpp server for GGUF models.
pub struct Engine {
    runtime: tokio::runtime::Runtime,
    llama: Option<LlamaRuntime>,
    env: Arc<dyn Env>,
    state: StateRoot,
}

impl Engine {
    /// An engine whose GGUF models run on lattice-core's managed server, configured from `env` (the binary, the models
    /// folder and `settings.json`, as lattice-core reads them).
    pub fn new(env: Arc<dyn Env>, state: StateRoot) -> Result<Engine, String> {
        let runtime = runtime()?;
        let config = RuntimeConfig::new(env.clone(), &state)
            .map_err(|e| format!("the local model server's client could not be made: {e:?}"))?;
        let llama = LlamaRuntime::new(config, runtime.handle().clone());
        Ok(Engine { runtime, llama: Some(llama), env, state })
    }

    /// An engine that reaches endpoints only (a test's).
    #[cfg(test)]
    pub fn endpoints_only(env: Arc<dyn Env>, state: StateRoot) -> Result<Engine, String> {
        Ok(Engine { runtime: runtime()?, llama: None, env, state })
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("centcom-own-model")
        .enable_all()
        .build()
        .map_err(|e| format!("the model's worker thread could not start: {e}"))
}

static ENGINE: OnceLock<Result<Arc<Engine>, String>> = OnceLock::new();

/// This process's engine, made on the first message: one managed server per process for Sinai's page.
pub fn engine(env: Arc<dyn Env>, state: &StateRoot) -> Result<Arc<Engine>, String> {
    ENGINE.get_or_init(|| Engine::new(env, state.clone()).map(Arc::new)).clone()
}

/// The request for `message` after the conversation `lines` (oldest first; at most [`HISTORY`] of them are sent).
pub fn request(lines: &[(Who, String)], message: &str) -> ModelRequest {
    let start = lines.len().saturating_sub(HISTORY);
    let mut input: Vec<InputItem> = lines[start..]
        .iter()
        .map(|(who, text)| match who {
            Who::You => InputItem::User(text.clone()),
            Who::Model => InputItem::Assistant { text: Some(text.clone()), tool_calls: Vec::new() },
        })
        .collect();
    input.push(InputItem::User(message.to_string()));
    ModelRequest {
        system: SYSTEM.to_string(),
        input,
        tools: Vec::new(),
        settings: ModelSettings { max_tokens: Some(MAX_TOKENS), ..ModelSettings::default() },
    }
}

/// Ask `target` for an answer to `request`: what happens comes as [`Event`]s, ending with `Done` or `Failed`. Dropping
/// the stream abandons the answer (the request is closed at the next piece).
pub fn ask(engine: Arc<Engine>, target: Target, request: ModelRequest) -> BoxStream<'static, Event> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    // The task holds what it needs, never the engine itself: the runtime must not be dropped from inside itself.
    let parts = Parts { llama: engine.llama.clone(), env: engine.env.clone(), state: engine.state.clone() };
    engine.runtime.spawn(async move { answer(parts, target, request, tx).await });
    // The stream keeps the engine (and so its runtime) alive while the answer is read. Alelyon's own engine is also
    // held by `ENGINE` for the life of the process, so the stream never drops the last of it inside the window's
    // runtime; a test's engine is dropped outside any runtime.
    rx.map(move |event| {
        let _alive = &engine;
        event
    })
    .boxed()
}

/// What one answer needs of the engine.
struct Parts {
    llama: Option<LlamaRuntime>,
    env: Arc<dyn Env>,
    state: StateRoot,
}

async fn answer(engine: Parts, target: Target, request: ModelRequest, tx: futures::channel::mpsc::UnboundedSender<Event>) {
    let send = |event: Event| tx.unbounded_send(event).is_ok();
    let (built, _lease) = match target {
        Target::Gguf { name, models_dir } => {
            let Some(llama) = engine.llama.as_ref() else {
                send(Event::Failed("This window cannot start llama.cpp's server.".into()));
                return;
            };
            let model = match files::resolve_model(&name, &models_dir) {
                Ok(model) => model,
                Err(problem) => {
                    send(Event::Failed(llama::model_sentence(problem).into()));
                    return;
                }
            };
            if llama.running().as_deref() != Some(model.name.as_str()) && !send(Event::Loading(model.name.clone())) {
                return;
            }
            let opened = match llama.open(model).await {
                Ok(opened) => opened,
                Err(problem) => {
                    send(Event::Failed(problem.sentence().into()));
                    return;
                }
            };
            match lattice_core::models::build_managed(&opened.endpoint, None) {
                Ok(built) => (built, Some(opened.lease)),
                Err(refusal) => {
                    send(Event::Failed(refusal.message));
                    return;
                }
            }
        }
        Target::Endpoint { id, registry: path } => {
            let report = registry::load_with_report(&path);
            let Some(endpoint) = report.endpoints.into_iter().find(|e| e.id == id) else {
                send(Event::Failed("That endpoint is no longer in the model registry.".into()));
                return;
            };
            if endpoint.kind != EndpointKind::OpenaiCompatible {
                send(Event::Failed("That endpoint does not speak the OpenAI-compatible API.".into()));
                return;
            }
            let keys = KeyStore::new(engine.env.clone(), &engine.state);
            if !endpoint.ready(&keys) {
                send(Event::Failed(super::status_sentence(&endpoint.status(&keys))));
                return;
            }
            match lattice_core::models::build(&endpoint, engine.env.as_ref(), &keys, None) {
                Ok(built) => (built, None),
                Err(refusal) => {
                    send(Event::Failed(refusal.message));
                    return;
                }
            }
        }
    };
    if !send(Event::Thinking) {
        return;
    }
    let mut stream = built.model.stream(request);
    let mut wrote = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(ModelEvent::TextDelta(piece)) => {
                wrote = true;
                if !send(Event::Delta(piece)) {
                    return;
                }
            }
            Ok(ModelEvent::ReasoningDelta(_)) => {}
            Ok(ModelEvent::Done(response)) => {
                let mut whole = String::new();
                for item in &response.output {
                    match item {
                        OutputItem::Message { text } if !wrote => whole.push_str(text),
                        OutputItem::Refusal { text } => whole = lattice_core::models::refusal_sentence(text),
                        _ => {}
                    }
                }
                send(Event::Done(whole));
                return;
            }
            Err(error) => {
                send(Event::Failed(lattice_core::models::error_sentence(&error, Some(&built.base_url))));
                return;
            }
        }
    }
    send(Event::Failed("The model's answer ended before it was complete.".into()));
}

// ------------------------------------------------------------------- the strip

/// Who said a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    You,
    Model,
}

/// What the strip is doing, as the page and the face show it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// The person is typing.
    Listening,
    /// llama.cpp's server is loading this model.
    Loading(String),
    Thinking,
    Speaking,
}

impl Phase {
    /// A message is being answered.
    pub fn busy(&self) -> bool {
        matches!(self, Phase::Loading(_) | Phase::Thinking | Phase::Speaking)
    }
}

/// Where the message goes, as the strip needs to know it before sending: the model and, for an endpoint off this PC,
/// the host it reaches (asked about once a session).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub target: Target,
    pub label: String,
    /// The host of an endpoint off this PC; None for a model on it.
    pub off_pc: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Typed(String),
    Send,
    /// The person answered "send this off the PC?".
    Confirm(bool),
    Stop,
    /// What the engine said, for the answer this generation asked for.
    Event(u64, Event),
}

/// The conversation with the person's own model on Sinai's page.
#[derive(Default)]
pub struct Strip {
    /// Oldest first.
    pub lines: Vec<(Who, String)>,
    pub typed: String,
    pub phase: Phase,
    /// The answer as it is being written.
    pub partial: String,
    /// Why the last message has no answer.
    pub problem: Option<String>,
    /// A message waiting for the person's yes before it goes to a host off this PC.
    pub confirm: Option<String>,
    /// Hosts off this PC the person said yes to, this session.
    allowed: HashSet<String>,
    /// Which answer the events are for: a stopped answer's late events are dropped.
    generation: u64,
    abort: Option<iced::task::Handle>,
    /// When the answer began to arrive: the mouth's rhythm counts from here.
    speaking_since: Option<Instant>,
    pieces: u32,
}

impl Strip {
    const LINES_KEPT: usize = 200;

    /// The window's own: it waits for an answer and has a spinner to turn.
    pub fn waiting(&self) -> bool {
        matches!(self.phase, Phase::Loading(_) | Phase::Thinking)
    }

    /// What one message does. `route` is where it would go now (or why nowhere); `engine` makes or finds the engine.
    pub fn update(
        &mut self,
        msg: Msg,
        route: Result<Route, String>,
        engine: impl FnOnce() -> Result<Arc<Engine>, String>,
    ) -> Task<Msg> {
        match msg {
            Msg::Typed(text) => {
                self.typed = text;
                if !self.phase.busy() {
                    let phase = if self.typed.trim().is_empty() { Phase::Idle } else { Phase::Listening };
                    if phase != self.phase {
                        self.phase = phase;
                        face(&self.phase, 0);
                    }
                }
            }
            Msg::Send => {
                let text = self.typed.trim().to_string();
                if text.is_empty() || self.phase.busy() {
                    return Task::none();
                }
                let route = match route {
                    Ok(route) => route,
                    Err(why) => {
                        self.problem = Some(why);
                        return Task::none();
                    }
                };
                if let Some(host) = route.off_pc.clone().filter(|h| !self.allowed.contains(h)) {
                    self.confirm = Some(host);
                    return Task::none();
                }
                return self.send(text, route, engine);
            }
            Msg::Confirm(yes) => {
                let Some(host) = self.confirm.take() else { return Task::none() };
                if !yes {
                    return Task::none();
                }
                self.allowed.insert(host);
                let text = self.typed.trim().to_string();
                if let Ok(route) = route
                    && !text.is_empty()
                {
                    return self.send(text, route, engine);
                }
            }
            Msg::Stop => {
                if let Some(handle) = self.abort.take() {
                    handle.abort();
                }
                self.generation += 1;
                self.finish();
            }
            Msg::Event(generation, event) => {
                if generation != self.generation {
                    return Task::none();
                }
                self.event(event);
            }
        }
        Task::none()
    }

    fn send(&mut self, text: String, route: Route, engine: impl FnOnce() -> Result<Arc<Engine>, String>) -> Task<Msg> {
        let engine = match engine() {
            Ok(engine) => engine,
            Err(why) => {
                self.problem = Some(why);
                return Task::none();
            }
        };
        let request = request(&self.lines, &text);
        self.push(Who::You, text);
        self.typed.clear();
        self.problem = None;
        self.partial.clear();
        self.pieces = 0;
        self.speaking_since = None;
        self.phase = Phase::Thinking;
        face(&self.phase, 0);
        self.generation += 1;
        let generation = self.generation;
        let (task, handle) = Task::run(ask(engine, route.target, request), move |e| Msg::Event(generation, e)).abortable();
        self.abort = Some(handle);
        task
    }

    /// What the engine said for the answer being waited for.
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Loading(name) => {
                self.phase = Phase::Loading(name);
                face(&self.phase, 0);
            }
            Event::Thinking => {
                if self.phase != Phase::Speaking {
                    self.phase = Phase::Thinking;
                    face(&self.phase, 0);
                }
            }
            Event::Delta(piece) => {
                if self.speaking_since.is_none() {
                    self.speaking_since = Some(Instant::now());
                }
                self.phase = Phase::Speaking;
                self.partial.push_str(&piece);
                self.pieces += 1;
                face(&self.phase, self.pieces);
            }
            Event::Done(whole) => {
                let mut answer = std::mem::take(&mut self.partial);
                if answer.trim().is_empty() {
                    answer = whole;
                }
                if answer.trim().is_empty() {
                    self.problem = Some("The model answered with nothing.".into());
                } else {
                    self.push(Who::Model, answer.trim().to_string());
                }
                self.finish();
            }
            Event::Failed(why) => {
                // What was written before the failure is kept, marked as cut short.
                let partial = std::mem::take(&mut self.partial);
                if !partial.trim().is_empty() {
                    self.push(Who::Model, format!("{} …", partial.trim()));
                }
                self.problem = Some(why);
                self.finish();
            }
        }
    }

    fn finish(&mut self) {
        self.abort = None;
        if !self.partial.trim().is_empty() {
            let partial = std::mem::take(&mut self.partial);
            self.push(Who::Model, format!("{} …", partial.trim()));
        }
        self.partial.clear();
        self.speaking_since = None;
        self.phase = if self.typed.trim().is_empty() { Phase::Idle } else { Phase::Listening };
        face(&self.phase, 0);
    }

    fn push(&mut self, who: Who, text: String) {
        self.lines.push((who, text));
        let n = self.lines.len();
        if n > Self::LINES_KEPT {
            self.lines.drain(..n - Self::LINES_KEPT);
        }
    }

    /// The face back at rest (the person's own model stopped being the one answering).
    pub fn rest(&mut self) {
        if let Some(handle) = self.abort.take() {
            handle.abort();
        }
        self.generation += 1;
        self.partial.clear();
        self.phase = Phase::Idle;
        crate::face::LIVE.lost();
    }
}

/// The face as the strip's phase moves it, through the same channel the loop's messages take (`face::LIVE`): a tone
/// and a mouth, bounded there. `piece` counts the answer's pieces, so the mouth opens and closes as they come.
fn face(phase: &Phase, piece: u32) {
    let live = &crate::face::LIVE;
    let (tone, lid, mouth) = match phase {
        Phase::Idle => ("", 0.0, 0.0),
        // looking up, attentive, while the person types
        Phase::Listening => ("", -0.08, 0.0),
        Phase::Loading(_) | Phase::Thinking => ("thinking", 0.10, 0.0),
        Phase::Speaking => ("", 0.0, mouth_for(piece)),
    };
    live.absorb(&json!({"type": "expression", "tone": tone, "lid": lid, "tilt": 0.0, "warmth": 0.0}));
    live.absorb(&json!({"type": "mouth", "v": mouth}));
}

/// The mouth's opening for the `piece`th piece of an answer: open and closing as words arrive, never shut mid-answer.
pub fn mouth_for(piece: u32) -> f32 {
    if piece == 0 {
        return 0.0;
    }
    0.12 + 0.33 * ((piece as f32) * 1.7).sin().abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_carries_the_last_lines_and_the_new_message() {
        let lines: Vec<(Who, String)> =
            (0..30).map(|i| (if i % 2 == 0 { Who::You } else { Who::Model }, format!("line {i}"))).collect();
        let r = request(&lines, "and now?");
        assert_eq!(r.input.len(), HISTORY + 1, "at most {HISTORY} lines, then the message");
        assert_eq!(r.input.last(), Some(&InputItem::User("and now?".into())));
        assert_eq!(r.input[0], InputItem::User("line 6".into()), "the oldest lines are left out");
        assert!(matches!(&r.input[1], InputItem::Assistant { text: Some(t), .. } if t == "line 7"));
        assert!(r.tools.is_empty(), "no tools: the model can only talk");
        assert_eq!(r.settings.max_tokens, Some(MAX_TOKENS));
        assert!(r.system.contains("not the one answering"), "the model is not told it is Sinai's mind");
    }

    #[test]
    fn an_answer_streams_into_the_conversation_and_moves_the_mouth() {
        let mut s = Strip::default();
        s.typed = "hello".into();
        s.phase = Phase::Thinking;
        s.event(Event::Loading("qwen".into()));
        assert_eq!(s.phase, Phase::Loading("qwen".into()));
        assert!(s.waiting());
        s.event(Event::Thinking);
        s.event(Event::Delta("Hel".into()));
        assert_eq!(s.phase, Phase::Speaking);
        assert!(!s.waiting(), "a streaming answer redraws for its pieces, not a spinner");
        s.event(Event::Delta("lo there".into()));
        assert_eq!(s.partial, "Hello there");
        s.event(Event::Done(String::new()));
        assert_eq!(s.lines, [(Who::Model, "Hello there".to_string())]);
        assert!(s.partial.is_empty() && s.problem.is_none());
        assert_eq!(s.phase, Phase::Listening, "the box still holds text");
        assert!(mouth_for(1) > 0.1 && mouth_for(2) > 0.1 && mouth_for(0) == 0.0);
    }

    #[test]
    fn a_failure_keeps_what_was_written_and_says_why() {
        let mut s = Strip::default();
        s.event(Event::Delta("Partly".into()));
        s.event(Event::Failed("The model did not answer in time.".into()));
        assert_eq!(s.lines, [(Who::Model, "Partly …".to_string())]);
        assert_eq!(s.problem.as_deref(), Some("The model did not answer in time."));
        assert_eq!(s.phase, Phase::Idle);
        let mut s = Strip::default();
        s.event(Event::Done("whole answer".into()));
        assert_eq!(s.lines, [(Who::Model, "whole answer".to_string())], "an answer that came whole");
    }
}
