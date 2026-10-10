//! The Models panel's rules: listing, adding, switching, readiness, the priority of Sinai's mind, and Sinai's page
//! talking to a stubbed OpenAI-compatible server. Every test works in a scratch folder with an environment of its own:
//! no test reads or writes the person's models folder, registry, preferences or Credential Manager.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;

use futures::StreamExt;
use lattice_core::MapEnv;
use serde_json::json;

use super::*;

fn gguf(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}

/// A keeper that remembers in memory, for the tests: what it was asked to keep and to forget.
#[derive(Default)]
struct Memory {
    kept: Mutex<Vec<(String, String)>>,
    forgot: Mutex<Vec<String>>,
}

impl KeyKeeper for Memory {
    fn keep(&self, name: &str, value: &SecretString) -> Result<(), String> {
        self.kept.lock().unwrap().push((name.into(), value.expose().into()));
        Ok(())
    }

    fn forget(&self, name: &str) -> Result<bool, String> {
        self.forgot.lock().unwrap().push(name.into());
        Ok(true)
    }
}

struct Scratch {
    dir: PathBuf,
    sources: Sources,
    keeper: Arc<Memory>,
}

/// A scratch place: its own home, models folder, registry, state root and preferences; llama.cpp's server where
/// `server` says (None: not installed).
fn scratch(name: &str, server: Option<&str>) -> Scratch {
    let dir = crate::sqlite_ro::tests::scratch(name);
    let mut env = MapEnv::new().with("USERPROFILE", dir.join("home").as_os_str());
    env.set("ALELYON_MODELS_DIR", dir.join("models").as_os_str());
    match server {
        Some(name) => {
            let exe = dir.join("bin").join(name);
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"MZ").unwrap();
            env.set("ALELYON_LLAMA_SERVER", exe.as_os_str());
        }
        None => env.set("ALELYON_LLAMA_SERVER", dir.join("absent").join("llama-server.exe").as_os_str()),
    }
    let keeper = Arc::new(Memory::default());
    let sources = Sources {
        env: Arc::new(env),
        state: StateRoot::at(dir.join("checkout")),
        models_dir: dir.join("models"),
        registry: dir.join("checkout").join("globals").join("model_endpoints.json"),
        prefs: Some(dir.join("preferences.json")),
        keeper: keeper.clone(),
    };
    Scratch { dir, sources, keeper }
}

fn write_registry(s: &Scratch, rows: serde_json::Value) {
    std::fs::create_dir_all(s.sources.registry.parent().unwrap()).unwrap();
    std::fs::write(&s.sources.registry, json!({"version": 1, "endpoints": rows}).to_string()).unwrap();
}

#[test]
fn a_pick_is_kept_as_text_and_read_back() {
    for pick in [Pick::Gguf("qwen3-4b".into()), Pick::Endpoint("my-box".into())] {
        assert_eq!(Pick::parse(&pick.key()), Some(pick.clone()));
    }
    for junk in ["", "gguf:", "endpoint:  ", "ollama:llama3", "qwen3"] {
        assert_eq!(Pick::parse(junk), None, "{junk:?}");
    }
}

/// Nothing changes for a person who never chose: Sinai's model is Local's, and Lattice's chat keeps Auto.
#[test]
fn each_surface_defaults_to_what_lattice_core_already_selects() {
    assert_eq!(sinai_pick(None, "qwen3-4b"), Some(Pick::Gguf("qwen3-4b".into())));
    assert_eq!(sinai_pick(None, "  "), None, "no Local model, no default: a model is never guessed");
    let kept = Pick::Endpoint("box".into());
    assert_eq!(sinai_pick(Some(&kept), "qwen3-4b"), Some(kept), "a kept choice wins");
    assert_eq!(lattice_pick("auto", "qwen3-4b"), None, "Auto is Lattice's own choice");
    assert_eq!(lattice_pick("local", "qwen3-4b"), Some(Pick::Gguf("qwen3-4b".into())));
    assert_eq!(lattice_pick("endpoint:box", ""), Some(Pick::Endpoint("box".into())));
    assert_eq!(lattice_pick("endpoint:llamacpp-local", "x"), None, "the managed row is the GGUF files");
}

/// The priority rule: where the build carries Sinai's mind, it answers unless the person explicitly picks their own
/// model instead; a model of their own never silently replaces it.
#[test]
fn sinais_mind_answers_unless_the_person_picks_their_own_model_instead() {
    let own = Pick::Gguf("qwen3-4b".into());
    assert_eq!(answering(true, false, Some(&own)), Answering::Mind, "a chosen model does not replace the mind");
    assert_eq!(answering(true, false, None), Answering::Mind);
    assert_eq!(answering(true, true, Some(&own)), Answering::Own(own.clone()), "only the explicit alternative");
    assert_eq!(answering(true, true, None), Answering::Mind, "nothing to switch to");
    assert_eq!(answering(false, false, Some(&own)), Answering::Own(own.clone()), "no mind: the person's own answers");
    assert_eq!(answering(false, true, Some(&own)), Answering::Own(own));
    assert_eq!(answering(false, false, None), Answering::Nobody);
}

#[test]
fn the_catalog_lists_gguf_files_and_switched_on_openai_compatible_endpoints() {
    let s = scratch("models-list", Some("llama-server.exe"));
    gguf(&s.sources.models_dir.join("qwen3-4b.gguf"));
    gguf(&s.sources.models_dir.join("sub").join("tiny.gguf"));
    gguf(&s.sources.models_dir.join("mmproj-vision.gguf"));
    std::fs::write(s.sources.models_dir.join("fake.gguf"), b"not a model").unwrap();
    write_registry(
        &s,
        json!([
            {"id": "box", "label": "My box", "base_url": "http://127.0.0.1:18080/v1", "model": "m", "enabled": true},
            {"id": "far", "label": "Far", "base_url": "https://api.example.test/v1", "model": "m", "api_key_name": "FAR_KEY", "enabled": true},
            {"id": "off", "label": "Off", "base_url": "http://127.0.0.1:1/v1", "model": "m", "enabled": false},
            {"id": "old", "label": "Old", "kind": "ollama", "base_url": "http://localhost:11434", "model": "m", "enabled": true}
        ]),
    );
    crate::prefs::save_sinai_model(s.sources.prefs.as_ref().unwrap(), "endpoint:box").unwrap();
    let c = read(&s.sources);
    assert!(c.server.is_ok());
    assert_eq!(c.ggufs.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["qwen3-4b", "tiny"], "no projector, no fake");
    let ids: Vec<&str> = c.endpoints.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["box", "far"], "switched on and OpenAI-compatible only; never Ollama");
    assert!(c.not_listed >= 2, "the turned-off, the Ollama row and the built-ins are counted, not listed");
    let box_ = c.endpoint("box").unwrap();
    assert!(box_.local && box_.configured && box_.knock == Some(("127.0.0.1".into(), 18080)));
    let far = c.endpoint("far").unwrap();
    assert!(!far.local && !far.configured && far.needs_key && far.knock.is_none(), "off this PC is never knocked on");
    assert_eq!(far.status, "It needs its key, and none is kept.");
    assert_eq!(c.sinai_kept, Some(Pick::Endpoint("box".into())));
    let shown = format!("{c:?}");
    assert!(!shown.contains("FAR_KEY") || far.needs_key, "only the key's name is known here, never a value");
}

#[test]
fn readiness_is_said_for_each_kind_of_model() {
    let s = scratch("models-ready", None);
    gguf(&s.sources.models_dir.join("qwen3-4b.gguf"));
    write_registry(
        &s,
        json!([
            {"id": "box", "label": "My box", "base_url": "http://127.0.0.1:18081/v1", "model": "m", "enabled": true},
            {"id": "far", "label": "Far", "base_url": "https://api.example.test/v1", "model": "m", "enabled": true}
        ]),
    );
    let c = read(&s.sources);
    let mut knocked = HashMap::new();
    assert!(matches!(c.ready(&Pick::Gguf("qwen3-4b".into()), &knocked), Ready::No(w) if w.contains("cannot run yet")),
        "the file is here but llama.cpp's server is not installed");
    assert!(matches!(c.ready(&Pick::Gguf("gone".into()), &knocked), Ready::No(w) if w.contains("not in the models folder")));
    let box_ = Pick::Endpoint("box".into());
    assert!(matches!(c.ready(&box_, &knocked), Ready::Asking(_)), "not knocked on yet");
    knocked.insert("box".into(), false);
    assert!(matches!(c.ready(&box_, &knocked), Ready::No(w) if w.contains("Nothing answers at 127.0.0.1:18081")));
    knocked.insert("box".into(), true);
    assert!(matches!(c.ready(&box_, &knocked), Ready::Yes(_)));
    assert!(matches!(c.ready(&Pick::Endpoint("far".into()), &knocked), Ready::Yes(w) if w.contains("not contacted until you send")));
    let s = scratch("models-ready-server", Some("llama-server.exe"));
    gguf(&s.sources.models_dir.join("qwen3-4b.gguf"));
    assert!(matches!(read(&s.sources).ready(&Pick::Gguf("qwen3-4b".into()), &knocked), Ready::Yes(_)));
}

/// A knock goes to an address on this PC and says whether something answers there.
#[test]
fn a_knock_finds_a_listener_on_this_pc_and_no_one_where_nothing_listens() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(knock("127.0.0.1", port));
    drop(listener);
    assert!(!knock("127.0.0.1", port));
    assert!(!knock("198.51.100.7", 80), "an address off this PC is never knocked on");
}

#[test]
fn host_and_port_are_read_from_an_address() {
    assert_eq!(host_port("http://127.0.0.1:8080/v1"), Some(("127.0.0.1".into(), 8080)));
    assert_eq!(host_port("https://api.example.test/v1"), Some(("api.example.test".into(), 443)));
    assert_eq!(host_port("http://localhost/v1"), Some(("localhost".into(), 80)));
    assert_eq!(host_port("http://[::1]:1234/v1"), Some(("::1".into(), 1234)));
    assert_eq!(host_port("http://user:pw@host/v1"), None);
    assert_eq!(host_port("ftp://host"), None);
}

#[test]
fn a_gguf_file_is_linked_into_the_models_folder_and_refused_when_it_is_not_one() {
    let s = scratch("models-add", None);
    let elsewhere = s.dir.join("downloads");
    gguf(&elsewhere.join("Tiny-Q4.gguf"));
    assert_eq!(add_gguf(&elsewhere.join("Tiny-Q4.gguf"), &s.sources.models_dir), Ok("Tiny-Q4".into()));
    let linked = s.sources.models_dir.join("Tiny-Q4.gguf");
    assert!(linked.is_file());
    assert_eq!(std::fs::read(&linked).unwrap(), std::fs::read(elsewhere.join("Tiny-Q4.gguf")).unwrap());
    assert_eq!(read(&s.sources).ggufs.len(), 1, "the folder lists it");
    // the same file again: it is already there
    assert_eq!(add_gguf(&linked, &s.sources.models_dir), Ok("Tiny-Q4".into()));
    // another file of the same name is refused: a name means one file
    gguf(&s.dir.join("other").join("Tiny-Q4.gguf"));
    assert!(add_gguf(&s.dir.join("other").join("Tiny-Q4.gguf"), &s.sources.models_dir).unwrap_err().contains("already has a model named"));
    std::fs::write(elsewhere.join("notes.txt"), b"GGUF").unwrap();
    assert!(add_gguf(&elsewhere.join("notes.txt"), &s.sources.models_dir).unwrap_err().contains("not a GGUF file"));
    std::fs::write(elsewhere.join("fake.gguf"), b"nope").unwrap();
    assert!(add_gguf(&elsewhere.join("fake.gguf"), &s.sources.models_dir).unwrap_err().contains("does not start"));
    gguf(&elsewhere.join("mmproj-x.gguf"));
    assert!(add_gguf(&elsewhere.join("mmproj-x.gguf"), &s.sources.models_dir).unwrap_err().contains("projector"));
    assert_eq!(read(&s.sources).ggufs.len(), 1, "nothing refused was linked");
}

#[test]
fn an_endpoint_is_checked_by_the_registrys_own_rules() {
    let form = |address: &str, key: &str| EndpointForm {
        label: "My Server".into(),
        address: address.into(),
        model: "m".into(),
        key: key.into(),
        problem: None,
    };
    let (e, key) = endpoint_of(&form("http://127.0.0.1:8080/v1", ""), &[]).unwrap();
    assert_eq!((e.id.as_str(), e.kind, e.enabled, e.builtin), ("my-server", EndpointKind::OpenaiCompatible, true, false));
    assert!(key.is_none() && e.api_key_name.is_empty());
    let (e, key) = endpoint_of(&form("https://api.example.test/v1", "sk-test"), &["my-server".into()]).unwrap();
    assert_eq!(e.id, "my-server-2", "an id is never taken twice");
    assert_eq!(key.as_deref(), Some("ALELYON_MY_SERVER_2_KEY"));
    assert!(keys::valid_key_name("ALELYON_MY_SERVER_2_KEY"));
    assert!(endpoint_of(&form("http://api.example.test/v1", "sk-test"), &[]).unwrap_err().contains("https"), "a key off this PC goes over https");
    assert!(endpoint_of(&form("ftp://x", ""), &[]).is_err());
    assert!(endpoint_of(&form("http://user:pw@127.0.0.1/v1", ""), &[]).is_err());
    assert!(endpoint_of(&form("", ""), &[]).is_err());
    assert!(!format!("{:?}", form("http://x", "sk-secret-value")).contains("sk-secret-value"), "a form never prints its key");
}

/// The key goes to the keeper (Credential Manager in Alelyon) and only its name to the registry; nothing goes to the
/// preferences. A registry that cannot be written takes the key back out.
#[test]
fn a_new_endpoints_key_is_kept_apart_and_never_written_with_it() {
    let s = scratch("models-endpoint-save", None);
    let form = EndpointForm {
        label: "Lab".into(),
        address: "https://lab.example.test/v1".into(),
        model: "m".into(),
        key: "  sk-fixture-value  ".into(),
        problem: None,
    };
    let id = save_endpoint(&s.sources, &form).unwrap();
    assert_eq!(id, "lab");
    assert_eq!(*s.keeper.kept.lock().unwrap(), [("ALELYON_LAB_KEY".to_string(), "sk-fixture-value".to_string())]);
    let text = std::fs::read_to_string(&s.sources.registry).unwrap();
    assert!(text.contains("ALELYON_LAB_KEY") && !text.contains("sk-fixture-value"), "the registry holds the name only");
    assert!(!s.sources.prefs.as_ref().unwrap().exists(), "nothing went to the preferences");
    let c = read(&s.sources);
    assert!(c.endpoint("lab").is_some(), "it is listed");

    // a registry path that is a folder cannot be written: the key is taken back out
    let mut broken = scratch("models-endpoint-broken", None);
    std::fs::create_dir_all(&broken.sources.registry).unwrap();
    broken.sources.registry = broken.sources.registry.clone();
    assert!(save_endpoint(&broken.sources, &form).is_err());
    assert_eq!(*broken.keeper.forgot.lock().unwrap(), ["ALELYON_LAB_KEY".to_string()]);
}

#[test]
fn switching_keeps_sinais_choice_in_the_preferences_and_lattices_in_lattice_core() {
    let s = scratch("models-switch", Some("llama-server.exe"));
    gguf(&s.sources.models_dir.join("qwen3-4b.gguf"));
    write_registry(&s, json!([{"id": "box", "label": "My box", "base_url": "http://127.0.0.1:18082/v1", "model": "m", "enabled": true}]));
    use_for_sinai(&s.sources, &Pick::Endpoint("box".into())).unwrap();
    assert_eq!(crate::prefs::load_sinai_model(s.sources.prefs.as_ref().unwrap()).as_deref(), Some("endpoint:box"));
    assert_eq!(use_for_lattice(&s.sources, &Pick::Gguf("qwen3-4b".into())), Ok("local".into()));
    assert_eq!(files::selected_model(&s.sources.state), "qwen3-4b", "Local's model, as every Lattice reads it");
    assert!(use_for_lattice(&s.sources, &Pick::Gguf("absent".into())).is_err(), "a name that is no file is refused");
    assert_eq!(use_for_lattice(&s.sources, &Pick::Endpoint("box".into())), Ok("endpoint:box".into()));

    // through the panel's own messages
    let mut state = State::default();
    state.with_sources(s.sources.clone());
    let _ = state.update(Msg::Catalogued(Box::new(read(&s.sources))), &mut String::new());
    let mut choice = "auto".to_string();
    let _ = state.update(Msg::Used(Surface::Lattice, Pick::Endpoint("box".into()), Ok("endpoint:box".into())), &mut choice);
    assert_eq!(choice, "endpoint:box", "one click sets Lattice chat's choice");
    let _ = state.update(Msg::Use(Surface::Sinai, Pick::Gguf("gone".into())), &mut choice);
    assert!(matches!(&state.said, Some(Err(w)) if w.contains("cannot be chosen")), "a model that is not there is not chosen");
    assert!(!state.working);
    let _ = state.update(Msg::Used(Surface::Sinai, Pick::Gguf("qwen3-4b".into()), Ok(String::new())), &mut choice);
    assert_eq!(state.sinai(), Some(Pick::Gguf("qwen3-4b".into())));
    assert_eq!(choice, "endpoint:box", "Sinai's switch leaves Lattice's alone");
}

#[test]
fn sinais_route_names_a_host_off_this_pc_and_refuses_what_cannot_run() {
    let s = scratch("models-route", None);
    gguf(&s.sources.models_dir.join("qwen3-4b.gguf"));
    write_registry(
        &s,
        json!([
            {"id": "box", "label": "My box", "base_url": "http://127.0.0.1:18083/v1", "model": "m", "enabled": true},
            {"id": "far", "label": "Far", "base_url": "https://api.example.test/v1", "model": "m", "enabled": true}
        ]),
    );
    let c = read(&s.sources);
    assert!(c.route(&Pick::Gguf("qwen3-4b".into()), &s.sources).unwrap_err().contains("llama.cpp"), "no server: said, not tried");
    assert_eq!(c.route(&Pick::Endpoint("box".into()), &s.sources).unwrap().off_pc, None);
    assert_eq!(c.route(&Pick::Endpoint("far".into()), &s.sources).unwrap().off_pc.as_deref(), Some("api.example.test"));
}

/// A message to an endpoint off this PC waits for the person's yes, once a session; one on this PC goes at once.
#[test]
fn the_first_message_off_this_pc_waits_for_a_yes() {
    let mut strip = byo::Strip::default();
    let route = byo::Route {
        target: byo::Target::Endpoint { id: "far".into(), registry: PathBuf::from("x") },
        label: "Far".into(),
        off_pc: Some("api.example.test".into()),
    };
    let _ = strip.update(byo::Msg::Typed("hello".into()), Ok(route.clone()), || Err("no engine".into()));
    let _ = strip.update(byo::Msg::Send, Ok(route.clone()), || panic!("nothing is sent before the yes"));
    assert_eq!(strip.confirm.as_deref(), Some("api.example.test"));
    assert!(strip.lines.is_empty());
    let _ = strip.update(byo::Msg::Confirm(false), Ok(route.clone()), || panic!("a no sends nothing"));
    assert!(strip.confirm.is_none() && strip.lines.is_empty() && strip.typed == "hello");
    let _ = strip.update(byo::Msg::Send, Err("Choose a model first".into()), || panic!("no route, nothing sent"));
    assert_eq!(strip.problem.as_deref(), Some("Choose a model first"));
}

/// A stand-in OpenAI-compatible server: answers one chat request with `pieces` streamed, and keeps what it was sent.
fn stub_server(pieces: &'static [&'static str]) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got).to_string();
            if let Some(at) = text.find("\r\n\r\n") {
                let length: usize = text[..at]
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                    .unwrap_or(0);
                if got.len() >= at + 4 + length {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
        for piece in pieces {
            let chunk = json!({"id": "stub", "object": "chat.completion.chunk",
                "choices": [{"index": 0, "delta": {"content": piece}, "finish_reason": null}]});
            stream.write_all(format!("data: {chunk}\n\n").as_bytes()).unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        let end = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
        stream.write_all(format!("data: {end}\n\ndata: [DONE]\n\n").as_bytes()).unwrap();
        String::from_utf8_lossy(&got).to_string()
    });
    (url, handle)
}

/// Sinai's chat strip against a stubbed OpenAI-compatible server, through lattice-core's own client: the answer
/// streams in piece by piece, ends complete, and the request carried the conversation and no key.
#[test]
fn sinais_strip_streams_an_answer_from_a_stubbed_openai_compatible_server() {
    let (url, server) = stub_server(&["Two ", "meetings ", "today."]);
    let s = scratch("models-stub-chat", None);
    write_registry(&s, json!([{"id": "stub", "label": "Stub", "base_url": url, "model": "stub-model", "enabled": true}]));
    let engine = Arc::new(byo::Engine::endpoints_only(s.sources.env.clone(), s.sources.state.clone()).unwrap());
    let lines = vec![(byo::Who::You, "hello".to_string()), (byo::Who::Model, "hi".to_string())];
    let events: Vec<byo::Event> = futures::executor::block_on(
        byo::ask(engine, byo::Target::Endpoint { id: "stub".into(), registry: s.sources.registry.clone() }, byo::request(&lines, "what is on today?"))
            .collect(),
    );
    assert_eq!(events.first(), Some(&byo::Event::Thinking));
    let pieces: Vec<&str> = events.iter().filter_map(|e| if let byo::Event::Delta(p) = e { Some(p.as_str()) } else { None }).collect();
    assert_eq!(pieces.concat(), "Two meetings today.");
    assert!(pieces.len() >= 2, "it streamed: {pieces:?}");
    assert!(matches!(events.last(), Some(byo::Event::Done(_))), "{events:?}");
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /v1/chat/completions"), "{request}");
    assert!(request.contains("what is on today?") && request.contains("hello") && request.contains("stub-model"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"), "no key was named, so none was sent");

    // the strip takes the events as the page would
    let mut strip = byo::Strip::default();
    for e in events {
        strip.event(e);
    }
    assert_eq!(strip.lines.last().map(|l| l.1.as_str()), Some("Two meetings today."));
    assert_eq!(strip.phase, byo::Phase::Idle);
}

/// A server that is not there is a sentence, not a hang or a panic.
#[test]
fn an_endpoint_that_does_not_answer_is_said_to() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    drop(listener);
    let s = scratch("models-stub-gone", None);
    write_registry(&s, json!([{"id": "gone", "label": "Gone", "base_url": url, "model": "m", "enabled": true}]));
    let engine = Arc::new(byo::Engine::endpoints_only(s.sources.env.clone(), s.sources.state.clone()).unwrap());
    let events: Vec<byo::Event> = futures::executor::block_on(
        byo::ask(engine.clone(), byo::Target::Endpoint { id: "gone".into(), registry: s.sources.registry.clone() }, byo::request(&[], "hi")).collect(),
    );
    assert!(matches!(events.last(), Some(byo::Event::Failed(w)) if w.contains("Could not reach the model")), "{events:?}");
    let events: Vec<byo::Event> = futures::executor::block_on(
        byo::ask(engine, byo::Target::Gguf { name: "x".into(), models_dir: s.sources.models_dir.clone() }, byo::request(&[], "hi")).collect(),
    );
    assert!(matches!(events.last(), Some(byo::Event::Failed(_))), "an engine without a server says so");
}

/// The live smoke, by hand: a real llama-server on the processor (`-ngl 0` from a scratch settings.json, and the
/// Vulkan loader shown no card with `VK_LOADER_DEVICE_ID_FILTER=0x1234`), a small GGUF already on this PC, through
/// the same engine Sinai's page uses. `CENTCOM_SMOKE_LLAMA` names the binary and `CENTCOM_SMOKE_MODELS` the folder,
/// `CENTCOM_SMOKE_MODEL` the model.
#[test]
#[ignore = "starts a real llama-server on the processor; run by hand"]
fn smoke_a_real_llama_server_on_the_processor_answers_through_sinais_engine() {
    let binary = std::env::var("CENTCOM_SMOKE_LLAMA").expect("CENTCOM_SMOKE_LLAMA");
    let models = std::env::var("CENTCOM_SMOKE_MODELS").expect("CENTCOM_SMOKE_MODELS");
    let model = std::env::var("CENTCOM_SMOKE_MODEL").expect("CENTCOM_SMOKE_MODEL");
    let dir = crate::sqlite_ro::tests::scratch("models-smoke");
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".alelyon").join("llama")).unwrap();
    std::fs::write(home.join(".alelyon").join("llama").join("settings.json"), r#"{"gpu_layers": 0, "ctx_size": 2048, "idle_seconds": 30}"#).unwrap();
    let mut env = MapEnv::new().with("USERPROFILE", home.as_os_str());
    for (k, v) in [("ALELYON_LLAMA_SERVER", binary.as_str()), ("ALELYON_MODELS_DIR", models.as_str()), ("VK_LOADER_DEVICE_ID_FILTER", "0x1234")] {
        env.set(k, v);
    }
    for k in ["TEMP", "TMP", "SystemRoot", "PATH", "LOCALAPPDATA"] {
        if let Some(v) = std::env::var_os(k) {
            env.set(k, v);
        }
    }
    let engine = Arc::new(byo::Engine::new(Arc::new(env), StateRoot::at(dir.join("checkout"))).unwrap());
    let started = std::time::Instant::now();
    let events: Vec<byo::Event> = futures::executor::block_on(
        byo::ask(
            engine,
            byo::Target::Gguf { name: model.clone(), models_dir: PathBuf::from(&models) },
            byo::request(&[], "Say hello in five words."),
        )
        .collect(),
    );
    let pieces: String = events.iter().filter_map(|e| if let byo::Event::Delta(p) = e { Some(p.as_str()) } else { None }).collect();
    eprintln!("smoke: {} events in {:.1} s; loading said: {}; answer: {pieces:?}; last: {:?}",
        events.len(), started.elapsed().as_secs_f32(), events.iter().any(|e| matches!(e, byo::Event::Loading(_))), events.last());
    assert!(matches!(events.last(), Some(byo::Event::Done(_))), "{events:?}");
    assert!(!pieces.trim().is_empty());
}

/// In the public build (no `sinai-mind`), Sinai's page answers with the person's own model once one is chosen, and
/// says nobody answers until then; the Models panel's messages reach the page through the app.
#[test]
fn the_public_builds_sinai_page_answers_with_the_persons_own_model() {
    let s = scratch("models-app", Some("llama-server.exe"));
    gguf(&s.sources.models_dir.join("qwen3-4b.gguf"));
    let (mut app, _) = crate::app::App::boot(crate::app::Options::default());
    assert!(!app.can(crate::capability::Capability::SinaiMind) && app.can(crate::capability::Capability::OwnModel));
    app.models.with_sources(s.sources.clone());
    let _ = app.update(crate::app::Message::Models(Msg::Catalogued(Box::new(read(&s.sources)))));
    assert_eq!(app.answering(), Answering::Nobody, "no model chosen and no Local model: nobody answers");
    let _ = app.update(crate::app::Message::Models(Msg::Used(Surface::Sinai, Pick::Gguf("qwen3-4b".into()), Ok(String::new()))));
    assert_eq!(app.answering(), Answering::Own(Pick::Gguf("qwen3-4b".into())));
    let _ = app.update(crate::app::Message::Models(Msg::Open));
    assert!(app.models.open);
    let _ = app.update(crate::app::Message::Models(Msg::Close));
    assert!(!app.models.open);
}

// ------------------------------------------------------------------- hosted open-weight models

#[test]
fn every_hosted_provider_has_a_card_and_a_key_in_the_environment_connects_it() {
    let mut s = scratch("models-hosted-catalog", None);
    let mut env = MapEnv::new().with("USERPROFILE", s.dir.join("home").as_os_str());
    env.set("ALELYON_MODELS_DIR", s.dir.join("models").as_os_str());
    env.set("GROQ_API_KEY", "gsk-from-the-environment");
    s.sources.env = Arc::new(env);
    let c = read(&s.sources);
    let ids: Vec<&str> = c.hosted.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["openrouter", "huggingface", "together", "groq", "fireworks", "deepinfra", "cerebras"]);
    assert_eq!(c.hosted.iter().find(|(id, _)| *id == "groq").unwrap().1, Some("the environment"));
}

#[test]
fn connecting_keeps_the_key_with_the_keeper_and_switches_the_row_on() {
    let s = scratch("models-hosted-connect", None);
    let deepinfra = lattice_core::hosted::provider("deepinfra").unwrap();
    // an empty key is refused before anything is kept or written
    assert_eq!(hosted::connect(&s.sources, deepinfra, &SecretString::from("  ".to_string())), Err("Paste the key first.".into()));
    assert!(s.keeper.kept.lock().unwrap().is_empty());
    assert!(!s.sources.registry.exists());

    let c = hosted::connect(&s.sources, deepinfra, &SecretString::from("di-key".to_string())).unwrap();
    assert_eq!(*s.keeper.kept.lock().unwrap(), [("DEEPINFRA_API_KEY".to_string(), "di-key".to_string())]);
    let row = c.endpoint("deepinfra").expect("listed once switched on");
    assert_eq!(row.address, "https://api.deepinfra.com/v1/openai");
    assert!(row.model.is_empty());
    // the key is never in the registry file
    assert!(!std::fs::read_to_string(&s.sources.registry).unwrap().contains("di-key"));

    let c = hosted::choose(&s.sources, deepinfra, "deepseek-ai/DeepSeek-V4").unwrap();
    assert_eq!(c.endpoint("deepinfra").unwrap().model, "deepseek-ai/DeepSeek-V4");
}

#[test]
fn a_registry_that_cannot_be_written_takes_the_key_back_out() {
    let s = scratch("models-hosted-unwritable", None);
    write_registry(&s, json!([]));
    std::fs::write(&s.sources.registry, b"{not json").unwrap();
    let groq = lattice_core::hosted::provider("groq").unwrap();
    assert!(hosted::connect(&s.sources, groq, &SecretString::from("gsk".to_string())).is_err());
    assert_eq!(*s.keeper.forgot.lock().unwrap(), ["GROQ_API_KEY".to_string()]);
}

#[test]
fn the_hosted_cards_say_what_to_do_and_a_sign_in_draws_nothing_while_it_waits() {
    let s = scratch("models-hosted-cards", None);
    let mut state = State::default();
    state.with_sources(s.sources.clone());
    let mut choice = "auto".to_string();
    // Connect with nothing typed says so, and starts nothing
    let _ = state.update(Msg::Hosted(hosted::Msg::Connect("groq")), &mut choice);
    assert_eq!(state.said, Some(Err("Paste your Groq key first.".into())));
    assert!(!state.hosted.working());
    // Get a key opens the provider's page in the person's own browser (a test build records it)
    let _ = state.update(Msg::Hosted(hosted::Msg::KeyPage("together")), &mut choice);
    assert!(crate::signin::web::opened("https://api.together.ai/settings/api-keys"));
    // a sign-in waits on the person: no redraws for it, and Cancel or closing the panel ends it
    let _ = state.update(Msg::Hosted(hosted::Msg::SignIn), &mut choice);
    assert!(state.hosted.signing_in());
    assert!(!state.busy());
    let _ = state.update(Msg::Hosted(hosted::Msg::SignIn), &mut choice);
    let _ = state.update(Msg::Close, &mut choice);
    assert!(!state.hosted.signing_in());
    let _ = state.update(
        Msg::Hosted(hosted::Msg::Connected("openrouter", Err("The sign-in was cancelled.".into()))),
        &mut choice,
    );
    assert!(!state.hosted.working());
    assert_eq!(state.said, Some(Err("The sign-in was cancelled.".into())));
}

#[test]
fn a_list_is_narrowed_by_what_is_typed() {
    let m = |id: &str, name: &str| lattice_core::hosted::HostedModel { id: id.into(), name: name.into(), context: None, price: None };
    let models = [m("meta-llama/llama-4-maverick", "Llama 4 Maverick"), m("qwen/qwen3-235b", "Qwen3 235B"), m("deepseek/v4", "DeepSeek V4")];
    assert_eq!(hosted::shown(&models, "").len(), 3);
    assert_eq!(hosted::shown(&models, " QWEN ").iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["qwen/qwen3-235b"]);
    assert_eq!(hosted::shown(&models, "maverick")[0].name, "Llama 4 Maverick");
    assert!(hosted::shown(&models, "gpt").is_empty());
}

#[test]
fn a_chosen_hosted_model_switches_as_an_endpoint_does() {
    let s = scratch("models-hosted-choose", None);
    let mut state = State::default();
    state.with_sources(s.sources.clone());
    let mut choice = "auto".to_string();
    let groq = lattice_core::hosted::provider("groq").unwrap();
    let c = hosted::connect(&s.sources, groq, &SecretString::from("gsk".to_string())).unwrap();
    let c = Box::new(hosted::choose(&s.sources, groq, "llama-3.3-70b-versatile").map(|_| c).unwrap());
    let _ = state.update(Msg::Hosted(hosted::Msg::Chosen(Surface::Lattice, "groq", Ok(c))), &mut choice);
    // the switch is the panel's own: Lattice's choice comes back in Used, as for any endpoint
    let _ = state.update(Msg::Used(Surface::Lattice, Pick::Endpoint("groq".into()), Ok("endpoint:groq".into())), &mut choice);
    assert_eq!(choice, "endpoint:groq");
    assert_eq!(state.said, Some(Ok("Lattice's chat uses Groq now.".into())));
}
