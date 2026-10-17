//! The agent bridge: how a person's own AI agent drives this app, when they switch it on in Settings.
//!
//! While it is on, the app listens on this computer only (`127.0.0.1`, a port the system picks) for requests from a
//! program the person runs, such as `alelyon-cli`'s MCP server. Each launch makes a new random token. The address
//! and the token go in a file only this Windows user can read (`%USERPROFILE%\.alelyon\app\agent-bridge.json`),
//! which the program reads; the file is removed when the bridge stops.
//!
//! A request is `POST /v1/<command>` with a JSON object of its fields and `Authorization: Bearer <token>`. It is
//! refused when it carries an `Origin` header (a web page, not a program) or names another host than this one. Each
//! request goes into the window's own update loop as a [`Message`](crate::app::Message), which answers it; nothing
//! here touches the window's state. What the commands are and which ones change something is the app's
//! (`app.rs`, `Message::Bridge`).
//!
//! HTTP is plain HTTP/1.1 over the standard library's socket: one request per connection, read whole and bounded
//! (at most [`MAX_HEAD`] of head and [`MAX_BODY`] of body), no chunked bodies, no keep-alive, at most [`AT_ONCE`]
//! requests at once, each read and write waiting at most [`WAIT`].

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc;
use futures::lock::Mutex as AsyncMutex;
use serde_json::{Value, json};

pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 64;
pub const MAX_BODY: usize = 256 * 1024;
pub const WAIT: Duration = Duration::from_secs(30);
pub const AT_ONCE: usize = 4;
/// How long a request waits for the window to answer it.
pub const ANSWER_WAIT: Duration = Duration::from_secs(30);

/// One request from an agent, carried into the window's update loop, and the way back to the agent.
pub struct Ask {
    pub command: String,
    pub fields: Value,
    reply: Mutex<Option<std::sync::mpsc::Sender<(u16, Value)>>>,
}

impl Ask {
    /// Answer the agent (once; a second answer is dropped).
    pub fn answer(&self, status: u16, body: Value) {
        if let Some(tx) = self.reply.lock().ok().and_then(|mut r| r.take()) {
            let _ = tx.send((status, body));
        }
    }
}

/// A request made here, and where its answer arrives: for the window's own tests.
#[cfg(test)]
pub fn asked(command: &str, fields: Value) -> (AskCell, std::sync::mpsc::Receiver<(u16, Value)>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (AskCell(Arc::new(Ask { command: command.into(), fields, reply: Mutex::new(Some(tx)) })), rx)
}

/// An [`Ask`] that can ride in a `Message`.
#[derive(Clone)]
pub struct AskCell(pub Arc<Ask>);

impl std::fmt::Debug for AskCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AskCell({})", self.0.command)
    }
}

/// A command name: lower-case words joined by `_`.
pub fn command_ok(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// Whether `a` and `b` are equal, taking the same time wherever they differ (their lengths are not secret).
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Where the address and token are handed to programs.
pub fn handoff_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".alelyon").join("app").join("agent-bridge.json"))
}

fn write_handoff(path: &Path, url: &str, token: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let mut file = lattice_sys::fs::create_private_file(path)?;
    let body = json!({"version": 1, "url": url, "token": token, "pid": std::process::id()});
    file.write_all(body.to_string().as_bytes())?;
    file.sync_all()
}

/// A running bridge. Dropping it stops the listener and removes the handoff file.
pub struct Bridge {
    port: u16,
    stopped: Arc<AtomicBool>,
    handoff: Option<PathBuf>,
    asks: Arc<AsyncMutex<mpsc::UnboundedReceiver<AskCell>>>,
}

impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bridge").field("port", &self.port).finish_non_exhaustive()
    }
}

struct Server {
    token: String,
    port: u16,
    stopped: Arc<AtomicBool>,
    asks: mpsc::UnboundedSender<AskCell>,
}

impl Bridge {
    /// Listen on this computer and write the handoff file at `handoff` (none in tests).
    pub fn start(handoff: Option<PathBuf>) -> std::io::Result<(Bridge, String)> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let url = format!("http://127.0.0.1:{port}");
        if let Some(path) = &handoff {
            write_handoff(path, &url, &token)?;
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::unbounded();
        let server = Arc::new(Server { token: token.clone(), port, stopped: stopped.clone(), asks: tx });
        std::thread::Builder::new()
            .name("alelyon-agent-bridge".into())
            .spawn(move || serve(listener, server))?;
        Ok((Bridge { port, stopped, handoff, asks: Arc::new(AsyncMutex::new(rx)) }, token))
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The requests, as the window's subscription reads them.
    pub fn asks(&self) -> Arc<AsyncMutex<mpsc::UnboundedReceiver<AskCell>>> {
        self.asks.clone()
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        // Wake the listener so it sees it has stopped.
        let _ = TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, self.port).into(), Duration::from_millis(200));
        if let Some(path) = &self.handoff {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The subscription key for a bridge's requests: one per running bridge.
#[derive(Clone)]
pub struct Asks(pub Arc<AsyncMutex<mpsc::UnboundedReceiver<AskCell>>>, pub u16);

impl std::hash::Hash for Asks {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "agent-bridge".hash(state);
        self.1.hash(state);
    }
}

/// The requests of `asks`, one by one, for `Subscription::run_with`.
pub fn ask_stream(asks: &Asks) -> futures::stream::BoxStream<'static, AskCell> {
    use futures::StreamExt;
    let rx = asks.0.clone();
    futures::stream::unfold(rx, |rx| async move {
        let next = rx.lock().await.next().await;
        next.map(|ask| (ask, rx))
    })
    .boxed()
}

// ---- HTTP

#[derive(Debug, Default)]
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

fn refused(status: u16, reason: &str, words: &str) -> (u16, Value) {
    (status, json!({"error": reason, "message": words}))
}

fn read_request(stream: &mut impl Read) -> Result<Request, Option<(u16, Value)>> {
    let bad = |status, words| Some(refused(status, "bad-request", words));
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(bad(431, "The request's headers are too large."));
        }
        let n = stream.read(&mut chunk).map_err(|_| None)?;
        if n == 0 {
            return Err(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| bad(400, "The request's headers are not text."))?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path, version) = (first.next().unwrap_or(""), first.next().unwrap_or(""), first.next().unwrap_or(""));
    if method.is_empty() || !path.starts_with('/') || !version.starts_with("HTTP/1.") {
        return Err(bad(400, "That is not an HTTP/1.1 request."));
    }
    let mut request = Request { method: method.into(), path: path.into(), ..Request::default() };
    let mut length = 0usize;
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(|| bad(400, "A header is not a name and a value."))?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if name == "content-length" {
            length = value.parse().map_err(|_| bad(400, "The body's length is not a number."))?;
        } else if name == "transfer-encoding" {
            return Err(bad(411, "A body must say its length."));
        }
        request.headers.push((name, value));
        if request.headers.len() > MAX_HEADERS {
            return Err(bad(431, "The request has too many headers."));
        }
    }
    if length > MAX_BODY {
        return Err(bad(413, "The request's body is too large."));
    }
    let mut body = buf[head_end..].to_vec();
    if body.len() > length {
        return Err(bad(400, "The body is longer than its length."));
    }
    while body.len() < length {
        let want = (length - body.len()).min(chunk.len());
        let n = stream.read(&mut chunk[..want]).map_err(|_| None)?;
        if n == 0 {
            return Err(None);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    request.body = body;
    Ok(request)
}

fn write_response(stream: &mut impl Write, status: u16, body: &Value) -> std::io::Result<()> {
    let body = body.to_string();
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

impl Server {
    fn handle(&self, request: &Request) -> (u16, Value) {
        if request.header("origin").is_some() {
            return refused(403, "from-a-web-page", "The agent bridge answers programs on this computer, not web pages.");
        }
        let host_ok = request.header("host").is_some_and(|h| {
            let h = h.to_ascii_lowercase();
            h == format!("127.0.0.1:{}", self.port) || h == format!("localhost:{}", self.port)
        });
        if !host_ok {
            return refused(403, "wrong-host", "The request names another host than this computer's bridge.");
        }
        let given = request.header("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
        if !same(given.as_bytes(), self.token.as_bytes()) {
            return refused(401, "invalid-token", "The bridge's token is missing or wrong; read it again from its file.");
        }
        if request.method != "POST" {
            return refused(405, "post-only", "Every bridge command is a POST.");
        }
        let Some(command) = request.path.strip_prefix("/v1/").filter(|c| command_ok(c)) else {
            return refused(404, "unknown-command", "That is not a bridge command.");
        };
        let fields: Value = if request.body.is_empty() {
            json!({})
        } else {
            match serde_json::from_slice(&request.body) {
                Ok(v @ Value::Object(_)) => v,
                _ => return refused(400, "bad-request", "A command's fields are one JSON object."),
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ask = AskCell(Arc::new(Ask { command: command.to_string(), fields, reply: Mutex::new(Some(tx)) }));
        if self.asks.unbounded_send(ask).is_err() {
            return refused(503, "closed", "The app is closing.");
        }
        rx.recv_timeout(ANSWER_WAIT)
            .unwrap_or_else(|_| refused(504, "no-answer", "The app did not answer in time; it may be busy. Try again."))
    }
}

fn serve(listener: TcpListener, server: Arc<Server>) {
    let busy = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if server.stopped.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(WAIT));
        let _ = stream.set_write_timeout(Some(WAIT));
        if busy.fetch_add(1, Ordering::SeqCst) >= AT_ONCE {
            busy.fetch_sub(1, Ordering::SeqCst);
            let (s, b) = refused(503, "busy", "The bridge is answering other requests; try again.");
            let _ = write_response(&mut stream, s, &b);
            continue;
        }
        let (server, held) = (server.clone(), busy.clone());
        let spawned = std::thread::Builder::new().name("alelyon-agent-bridge-request".into()).spawn(move || {
            let answer = match read_request(&mut stream) {
                Ok(request) => Some(server.handle(&request)),
                Err(bad) => bad,
            };
            if let Some((status, body)) = answer {
                let _ = write_response(&mut stream, status, &body);
            }
            let _ = stream.shutdown(std::net::Shutdown::Both);
            held.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            busy.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn call(port: u16, headers: &str, path: &str, body: &str) -> (u16, Value) {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let req = format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{headers}Content-Length: {}\r\n\r\n{body}", body.len());
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let status = out[9..12].parse().unwrap();
        let body = out.split("\r\n\r\n").nth(1).unwrap_or("null");
        (status, serde_json::from_str(body).unwrap())
    }

    #[test]
    fn a_request_reaches_the_window_and_its_answer_comes_back() {
        let (bridge, token) = Bridge::start(None).unwrap();
        let asks = bridge.asks();
        let port = bridge.port();
        std::thread::spawn(move || {
            let ask = futures::executor::block_on(async { asks.lock().await.next().await }).unwrap();
            assert_eq!(ask.0.command, "status");
            assert_eq!(ask.0.fields, json!({"x": 1}));
            ask.0.answer(200, json!({"ok": true}));
        });
        let auth = format!("Authorization: Bearer {token}\r\n");
        assert_eq!(call(port, &auth, "/v1/status", "{\"x\":1}"), (200, json!({"ok": true})));
    }

    #[test]
    fn a_wrong_token_a_web_page_and_a_bad_command_are_refused() {
        let (bridge, token) = Bridge::start(None).unwrap();
        let port = bridge.port();
        assert_eq!(call(port, "Authorization: Bearer nope\r\n", "/v1/status", "").0, 401);
        let auth = format!("Authorization: Bearer {token}\r\n");
        let origin = format!("{auth}Origin: https://evil.example\r\n");
        assert_eq!(call(port, &origin, "/v1/status", "").1["error"], "from-a-web-page");
        assert_eq!(call(port, &auth, "/v1/../x", "").0, 404);
        assert_eq!(call(port, &auth, "/v1/status", "[1]").0, 400);
    }

    #[test]
    fn the_handoff_file_is_written_private_and_removed_with_the_bridge() {
        let dir = std::env::temp_dir().join(format!("alelyon-bridge-test-{}", std::process::id()));
        let path = dir.join("agent-bridge.json");
        let (bridge, token) = Bridge::start(Some(path.clone())).unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["token"], token.as_str());
        assert_eq!(written["url"], format!("http://127.0.0.1:{}", bridge.port()));
        drop(bridge);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commands_are_plain_names() {
        assert!(command_ok("chat_send") && command_ok("status"));
        assert!(!command_ok("") && !command_ok("../x") && !command_ok("Status") && !command_ok(&"a".repeat(33)));
    }
}
