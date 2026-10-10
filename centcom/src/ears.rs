//! The ears from the window: a live link to the speech engine's `serve`, what it says, and starting it.
//!
//! Every build can start the engine: its program is `angel-ears.exe` beside this window's program, or the one
//! `CENTCOM_EARS` names (a build may register its own rule instead, `installed.rs`). Before it is started, the
//! engine's own rule (`alelyon_ears::setup`) says whether its recogniser program and speech model are in place;
//! what is missing is shown as a step to take, never downloaded.
//!
//! The service writes its address and a fresh token to `~/.alelyon/angel/ears.json` each time it starts. The
//! link reads them, connects (loopback, no TLS, no `Origin` header, which the ears refuse), hands the window a
//! way to send commands, and passes on every message. When the service is not running, or goes away, the link
//! says so and looks again every two seconds, so starting the engine later needs nothing from the window.
//!
//! The token never leaves this module: it is not shown, logged or put in an error message.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use futures::SinkExt;
use futures::channel::mpsc as stream_mpsc;
use iced::Subscription;
use serde_json::{Value, json};

/// How often the link looks again for a service that is not there.
const RETRY: Duration = Duration::from_secs(2);
/// How long one read waits before the link checks the window's commands: the latency of a click.
const POLL: Duration = Duration::from_millis(100);

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

/// Where the service writes its address and token.
pub fn connection_file() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".alelyon").join("angel").join("ears.json"))
}

/// The window's way to talk to the service: one JSON command at a time.
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

/// What the link reports.
#[derive(Clone, Debug)]
pub enum Feed {
    /// Looking for the service.
    Connecting,
    /// No service answers; why, in words a person can act on.
    Unavailable(String),
    Connected(Commands),
    /// One message from the service.
    Event(Value),
    /// The connection broke.
    Lost(String),
}

pub fn subscription() -> Subscription<Feed> {
    Subscription::run(link)
}

fn link() -> impl futures::Stream<Item = Feed> {
    iced::stream::channel(64, async |output: stream_mpsc::Sender<Feed>| {
        // The socket is a blocking one, as the ears' own is: it lives on a thread of its own and ends when the
        // window stops listening (its sends fail).
        let _ = std::thread::Builder::new().name("centcom-ears".into()).spawn(move || run(output));
    })
}

fn run(mut output: stream_mpsc::Sender<Feed>) {
    let mut emit = |feed: Feed| futures::executor::block_on(output.send(feed)).is_ok();
    // The window hears "looking" once, at the start, and why the service is not there only when the reason changes:
    // each message redraws the whole window, and the same words every two seconds change nothing on it.
    let mut told = Told::default();
    loop {
        if told.looking() && !emit(Feed::Connecting) {
            return;
        }
        match details().and_then(|(url, token)| connect(&url, &token)) {
            Err(why) => {
                if told.unavailable(&why) && !emit(Feed::Unavailable(why)) {
                    return;
                }
            }
            Ok(mut ws) => {
                told.reached();
                let (tx, rx) = mpsc::channel::<String>();
                if !emit(Feed::Connected(Commands(tx))) {
                    return;
                }
                let why = loop {
                    let mut broken = None;
                    while let Ok(command) = rx.try_recv() {
                        if let Err(e) = ws.send(tungstenite::Message::text(command)) {
                            broken = Some(e.to_string());
                            break;
                        }
                    }
                    if let Some(why) = broken {
                        break why;
                    }
                    match ws.read() {
                        Ok(tungstenite::Message::Text(text)) => {
                            if let Ok(v) = serde_json::from_str::<Value>(&text)
                                && !emit(Feed::Event(v))
                            {
                                let _ = ws.close(None);
                                return;
                            }
                        }
                        Ok(tungstenite::Message::Close(_)) => break "the speech engine closed the connection".into(),
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                        Err(e) => break format!("the connection to the speech engine broke ({e})"),
                    }
                };
                if !emit(Feed::Lost(why)) {
                    return;
                }
            }
        }
        std::thread::sleep(RETRY);
    }
}

/// What the link has told the window, so it is not told the same again.
#[derive(Debug, Default)]
struct Told {
    looked: bool,
    unavailable: Option<String>,
}

impl Told {
    /// The first look is said; later ones are not (the window shows the last outcome meanwhile).
    fn looking(&mut self) -> bool {
        !std::mem::replace(&mut self.looked, true)
    }

    /// Whether "not available, because `why`" is news.
    fn unavailable(&mut self, why: &str) -> bool {
        if self.unavailable.as_deref() == Some(why) {
            return false;
        }
        self.unavailable = Some(why.to_string());
        true
    }

    /// Connected: a later failure to connect is news again, whatever its reason.
    fn reached(&mut self) {
        self.unavailable = None;
    }
}

/// The service's address and token, from its connection file.
fn details() -> Result<(String, String), String> {
    let path = connection_file().ok_or("USERPROFILE is not set, so the speech engine cannot be found")?;
    let text = std::fs::read_to_string(&path).map_err(|_| NOT_RUNNING.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|_| format!("{} is not readable JSON", path.display()))?;
    match (v.get("url").and_then(Value::as_str), v.get("token").and_then(Value::as_str)) {
        (Some(url), Some(token)) => Ok((url.to_string(), token.to_string())),
        _ => Err(format!("{} has no address or token", path.display())),
    }
}

const NOT_RUNNING: &str = "The speech engine is not running.";

fn connect(url: &str, token: &str) -> Result<Socket, String> {
    let request = format!("{url}?token={token}");
    let (mut ws, _) = tungstenite::connect(request.as_str()).map_err(|e| refusal(&e))?;
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
        let _ = stream.set_read_timeout(Some(POLL));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    }
    Ok(ws)
}

/// Why a connection was not made, in words a person can act on. Nothing of the request is repeated: its
/// address carries the token. A file left by an engine that has since stopped reads as "not running"
/// (2026-10-03: tungstenite reports a refused loopback port as a URL error, not an I/O one).
fn refusal(e: &tungstenite::Error) -> String {
    use tungstenite::error::UrlError;
    match e {
        tungstenite::Error::Url(UrlError::UnableToConnect(_)) => NOT_RUNNING.to_string(),
        tungstenite::Error::Io(io) if io.kind() == ErrorKind::ConnectionRefused => NOT_RUNNING.to_string(),
        // 401: the file is from an earlier run while a new engine is starting; the next look reads the new one
        tungstenite::Error::Http(response) => format!("The speech engine turned Alelyon away ({}).", response.status()),
        _ => "The speech engine could not be reached.".to_string(),
    }
}

/// `text` with every copy of `secret` blanked.
#[cfg(test)]
fn without(text: &str, secret: &str) -> String {
    if secret.is_empty() { text.to_string() } else { text.replace(secret, "[token]") }
}

// ------------------------------------------------------------------- what the service says

/// The service's on-air truth (`ears.state`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub mic_on: bool,
    pub mic_device: String,
    pub listening: bool,
    pub dictating: bool,
    pub pc_on: bool,
    pub pc_device: String,
    /// "ready", or what the recogniser is doing.
    pub recognizer: String,
    pub model: String,
}

pub fn parse_state(v: &Value) -> State {
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let b = |v: &Value| v.as_bool().unwrap_or(false);
    State {
        mic_on: b(&v["mic"]["on"]),
        mic_device: s(&v["mic"]["device"]),
        listening: b(&v["mic"]["listen"]),
        dictating: b(&v["mic"]["dictation"]),
        pc_on: b(&v["pc"]["on"]),
        pc_device: s(&v["pc"]["device"]),
        recognizer: s(&v["recognizer"]["state"]),
        model: s(&v["recognizer"]["model"]),
    }
}

/// A file job, as its `ears.file` events and the state's job list describe it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    pub id: String,
    pub path: String,
    /// decoding, transcribing, done, failed or cancelled
    pub state: String,
    pub progress: f32,
    pub outputs: Vec<String>,
    pub error: Option<String>,
    pub lines: Option<u64>,
    pub seconds: Option<f64>,
}

impl Job {
    pub fn running(&self) -> bool {
        matches!(self.state.as_str(), "decoding" | "transcribing")
    }

    pub fn name(&self) -> String {
        std::path::Path::new(&self.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| self.path.clone())
    }

    /// Take in one `ears.file` event, or one entry of the state's job list.
    pub fn absorb(&mut self, v: &Value) {
        if let Some(path) = v.get("path").and_then(Value::as_str) {
            self.path = path.to_string();
        }
        if let Some(state) = v.get("state").and_then(Value::as_str) {
            self.state = state.to_string();
        }
        if let Some(p) = v.get("progress").and_then(Value::as_f64) {
            self.progress = p as f32;
        }
        if let Some(list) = v.get("outputs").and_then(Value::as_array) {
            self.outputs = list.iter().filter_map(Value::as_str).map(str::to_string).collect();
        }
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            self.error = Some(e.to_string());
        }
        if let Some(n) = v.get("lines").and_then(Value::as_u64) {
            self.lines = Some(n);
        }
        if let Some(s) = v.get("seconds").and_then(Value::as_f64) {
            self.seconds = Some(s);
        }
    }
}

// ------------------------------------------------------------------- commands

pub fn listen(source: &str, on: bool) -> Value {
    json!({"cmd": "listen", "source": source, "on": on})
}

pub fn dictate(on: bool) -> Value {
    json!({"cmd": "dictate", "on": on})
}

pub fn transcribe_file(path: &str, formats: &[&str]) -> Value {
    json!({"cmd": "transcribe_file", "path": path, "formats": formats})
}

pub fn cancel(job: &str) -> Value {
    json!({"cmd": "cancel", "job": job})
}

// ------------------------------------------------------------------- starting the engine

/// The engine's program name beside the window's.
#[cfg(windows)]
pub const PROGRAM: &str = "angel-ears.exe";
#[cfg(not(windows))]
pub const PROGRAM: &str = "angel-ears";

/// The engine: `named` (`CENTCOM_EARS`) when given, which must be a file; else [`PROGRAM`] beside `exe`.
pub fn find(named: Option<PathBuf>, exe: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = named {
        return if path.is_file() { Ok(path) } else { Err(format!("CENTCOM_EARS names {}, which is not a file.", path.display())) };
    }
    let beside = exe.map(|e| e.with_file_name(PROGRAM));
    match beside {
        Some(path) if path.is_file() => Ok(path),
        _ => Err(format!("To start it from here, put {PROGRAM} beside Alelyon's program, or set CENTCOM_EARS to its path.")),
    }
}

/// The built-in rule, for this process: `CENTCOM_EARS`, else beside the running program.
pub fn locate() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().ok();
    find(std::env::var_os("CENTCOM_EARS").map(PathBuf::from), exe.as_deref())
}

/// The speech engine's program: as the build's own rule finds it when it registers one (`installed.rs`, the `ears`
/// plug-in), else by the built-in rule ([`locate`]). The program, or why it was not found.
pub fn engine_exe() -> Result<PathBuf, String> {
    crate::installed::ears().unwrap_or_else(locate)
}

/// The options the window adds to `serve`: `CENTCOM_EARS_ARGS`, split at spaces.
fn extra_args() -> Vec<String> {
    std::env::var("CENTCOM_EARS_ARGS").map(|a| a.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
}

/// Whether the window can start the engine now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// The engine and what it runs are in place.
    Ready,
    /// The engine's program was not found: why, and what to do.
    NoEngine(String),
    /// The engine is here, but something it runs is not: each, in the engine's own words. An empty state with the
    /// place to put it, not an error.
    Missing(Vec<String>),
}

/// [`Start`] from what was found: the engine's program, and what the engine's own rule says is missing beside it.
pub fn start_state(engine: Result<PathBuf, String>, missing: impl FnOnce(&Path) -> Vec<alelyon_ears::setup::Missing>) -> Start {
    match engine {
        Err(why) => Start::NoEngine(why),
        Ok(exe) => {
            let missing = missing(&exe);
            if missing.is_empty() { Start::Ready } else { Start::Missing(missing.iter().map(alelyon_ears::setup::Missing::explain).collect()) }
        }
    }
}

/// Whether the window can start the engine now, on this PC: the engine's program, and its recogniser and model by
/// the engine's own rule, read with `CENTCOM_EARS_ARGS` as `serve` will read it.
pub fn can_start() -> Start {
    start_state(engine_exe(), |exe| {
        alelyon_ears::setup::Given::from_env(Some(exe)).with_args(&extra_args()).missing(|p| p.is_file())
    })
}

/// What the Words page says above the steps when the engine cannot start yet: a model is a file a person chooses and
/// puts in place, as Lattice's GGUF models are, and nothing is fetched for it.
pub const MISSING_TITLE: &str = "Before the speech engine can start";
pub const MISSING_NOTE: &str =
    "Alelyon does not download speech models or programs: put the files in place, then start the engine here.";

/// The engine CENTCOM started: it ends with the window (a job object closes it, a crash included).
pub struct Engine {
    /// Shared with `watch`'s thread, which asks it once a second whether it has ended.
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
    #[cfg(windows)]
    job: crate::job::Job,
}

impl Engine {
    /// `<engine> serve`, with `CENTCOM_EARS_ARGS` added (for example `--attach 127.0.0.1:18178`, to use a
    /// recogniser that is already running), its output in `~/.alelyon/angel/centcom-ears.log`.
    pub fn start() -> Result<Engine, String> {
        use std::process::{Command, Stdio};
        let exe = match can_start() {
            Start::Ready => engine_exe()?,
            Start::NoEngine(why) => return Err(why),
            Start::Missing(steps) => return Err(steps.join(" ")),
        };
        let extra = extra_args();
        let log_path = connection_file().map(|p| p.with_file_name("centcom-ears.log"));
        let (out, err) = match log_path.as_ref().and_then(|p| std::fs::File::create(p).ok()) {
            Some(log) => match log.try_clone() {
                Ok(log2) => (Stdio::from(log), Stdio::from(log2)),
                Err(_) => (Stdio::null(), Stdio::null()),
            },
            None => (Stdio::null(), Stdio::null()),
        };
        let mut command = Command::new(&exe);
        command.arg("serve").args(&extra).stdin(Stdio::null()).stdout(out).stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        #[cfg(windows)]
        let job = crate::job::Job::new()?;
        let mut child = command.spawn().map_err(|e| format!("could not start {}: {e}", exe.display()))?;
        #[cfg(windows)]
        if let Err(e) = job.adopt(&child, "the speech engine") {
            let _ = child.kill();
            return Err(e);
        }
        Ok(Engine {
            child: std::sync::Arc::new(std::sync::Mutex::new(child)),
            #[cfg(windows)]
            job,
        })
    }

    /// How the engine ended, said once when it ends. It is asked once a second on this subscription's own thread, so
    /// an engine that runs on sends the window nothing (a one-second timer redrew the whole window every second).
    pub fn watch(&self) -> Subscription<String> {
        let pid = self.child.lock().unwrap_or_else(|p| p.into_inner()).id();
        Subscription::run_with(Watched { pid, child: self.child.clone() }, watching)
    }
}

/// The engine `watch` follows, known to iced by its process id.
struct Watched {
    pid: u32,
    child: std::sync::Arc<std::sync::Mutex<std::process::Child>>,
}

impl std::hash::Hash for Watched {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pid.hash(state);
    }
}

fn watching(watched: &Watched) -> impl futures::Stream<Item = String> + use<> {
    let child = watched.child.clone();
    iced::stream::channel(1, async move |mut output: stream_mpsc::Sender<String>| {
        let _ = std::thread::Builder::new().name("centcom-ears-engine".into()).spawn(move || {
            while !output.is_closed() {
                std::thread::sleep(Duration::from_secs(1));
                if let Some(how) = ended(&mut child.lock().unwrap_or_else(|p| p.into_inner())) {
                    let _ = futures::executor::block_on(output.send(how));
                    return;
                }
            }
        });
    })
}

/// Still running? (`Some(status)` once it has ended.)
fn ended(child: &mut std::process::Child) -> Option<String> {
    match child.try_wait() {
        Ok(Some(status)) => Some(status.to_string()),
        Ok(None) => None,
        Err(e) => Some(e.to_string()),
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
        let _ = child.kill();
        let _ = child.wait();
        #[cfg(windows)]
        let _ = &self.job;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_still_away_for_the_same_reason_is_not_told_again() {
        let mut told = Told::default();
        assert!(told.looking() && !told.looking(), "the first look only");
        assert!(told.unavailable(NOT_RUNNING));
        assert!(!told.unavailable(NOT_RUNNING), "every two seconds, the same words: not news");
        assert!(told.unavailable("ears.json is not readable JSON"), "a new reason is");
        told.reached();
        assert!(told.unavailable("ears.json is not readable JSON"), "after a connection, any failure is news");
    }

    #[test]
    fn the_on_air_truth_is_read_whole() {
        let v = json!({"type": "ears.state",
            "mic": {"on": true, "device": "Headset Microphone", "session": "mic-1", "purpose": "dictation", "listen": false, "dictation": true},
            "pc": {"on": false},
            "recognizer": {"state": "ready", "model": "ggml-large-v3-turbo-q5_0.bin"},
            "jobs": []});
        let s = parse_state(&v);
        assert!(s.mic_on && s.dictating && !s.listening && !s.pc_on);
        assert_eq!((s.mic_device.as_str(), s.recognizer.as_str()), ("Headset Microphone", "ready"));
        assert_eq!(parse_state(&json!({})), State::default(), "a bare message reads as everything off");
    }

    #[test]
    fn a_job_takes_in_its_events() {
        let mut job = Job { id: "job-3".into(), ..Job::default() };
        job.absorb(&json!({"type": "ears.file", "job": "job-3", "path": "C:/talks/a talk.m4a", "state": "transcribing", "progress": 0.25}));
        assert!(job.running());
        assert_eq!((job.name().as_str(), job.progress), ("a talk.m4a", 0.25));
        job.absorb(&json!({"state": "done", "progress": 1.0, "outputs": ["C:/talks/a talk.txt"], "lines": 12, "seconds": 61.5}));
        assert!(!job.running());
        assert_eq!(job.outputs, ["C:/talks/a talk.txt"]);
        assert_eq!((job.lines, job.seconds), (Some(12), Some(61.5)));
    }

    #[test]
    fn the_named_engine_or_the_one_beside_the_window_and_otherwise_what_to_do() {
        let dir = std::env::temp_dir().join(format!("centcom-ears-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let window = dir.join("centcom.exe");
        let err = find(None, Some(&window)).unwrap_err();
        assert!(err.contains(PROGRAM) && err.contains("CENTCOM_EARS"), "{err}");
        std::fs::write(dir.join(PROGRAM), b"").unwrap();
        assert_eq!(find(None, Some(&window)), Ok(dir.join(PROGRAM)));
        let named = dir.join("elsewhere.exe");
        assert!(find(Some(named.clone()), Some(&window)).unwrap_err().contains("not a file"), "a named engine must exist");
        std::fs::write(&named, b"").unwrap();
        assert_eq!(find(Some(named.clone()), Some(&window)), Ok(named));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_public_build_starts_the_engine_by_the_built_in_rule() {
        // no centcom test installs a registry, so this is the public build: it has a rule, not "not in this build"
        assert!(crate::installed::ears().is_none());
        let built_in = locate();
        assert_eq!(engine_exe(), built_in);
    }

    #[test]
    fn a_missing_model_is_a_step_with_its_place_and_an_engine_with_everything_starts() {
        use alelyon_ears::setup::{Given, MODEL_ENV, MODEL_FILE, Missing};
        let home = PathBuf::from("H");
        let given = |exe: &Path| Given { home: Some(home.clone()), engine_dir: exe.parent().map(Path::to_path_buf), ..Given::default() };
        let exe = PathBuf::from("E").join(PROGRAM);
        // nothing in place: two steps, the model's naming its file, its folder and the variable
        match start_state(Ok(exe.clone()), |e| given(e).missing(|_| false)) {
            Start::Missing(steps) => {
                assert_eq!(steps.len(), 2, "{steps:?}");
                let folder = alelyon_ears::setup::models_dir(&home).display().to_string();
                assert!(steps[0].contains(MODEL_FILE) && steps[0].contains(&folder) && steps[0].contains(MODEL_ENV), "{}", steps[0]);
                for step in &steps {
                    assert!(!step.to_lowercase().contains("error") && !step.to_lowercase().contains("failed"), "{step}");
                }
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(start_state(Ok(exe.clone()), |e| given(e).missing(|_| true)), Start::Ready);
        // `--attach` (through CENTCOM_EARS_ARGS) needs neither
        assert_eq!(start_state(Ok(exe.clone()), |e| given(e).with_args(&["--attach", "127.0.0.1:18178"]).missing(|_| false)), Start::Ready);
        assert_eq!(start_state(Err("not here".into()), |_| vec![Missing::NoHome]), Start::NoEngine("not here".into()));
        for words in [MISSING_TITLE, MISSING_NOTE] {
            assert!(!words.to_lowercase().contains("error"), "{words}");
        }
    }

    #[test]
    fn the_token_never_reaches_an_error_message() {
        assert_eq!(without("GET /?token=abc123 refused", "abc123"), "GET /?token=[token] refused");
        assert_eq!(without("nothing secret", ""), "nothing secret");
        // a refused port, as tungstenite reports it, names the request; the words shown do not
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        drop(held);
        let e = match tungstenite::connect(format!("ws://127.0.0.1:{port}/?token=abc123").as_str()) {
            Err(e) => e,
            Ok(_) => panic!("nothing listens there"),
        };
        assert_eq!(refusal(&e), NOT_RUNNING);
        assert!(!refusal(&e).contains("abc123"));
    }
}
