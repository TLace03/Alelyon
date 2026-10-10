//! Models: one panel for the models this PC can use, and which one Sinai's page and Lattice's chat each talk to
//! (an easy way for people to bring their own models, and to switch the active ones with a couple of clicks).
//!
//! It works in the public build, with no server and no closed part, from what lattice-core already keeps:
//! - **GGUF files** in the models folder lattice-core lists (`ALELYON_MODELS_DIR`, else `~/.alelyon/models`, and one
//!   level below it; `llama::files::list_models`). Add a GGUF file… hard-links the chosen file there;
//!   it brings models in: no copy, no download, and a file on another drive is refused with what to do instead.
//!   A GGUF model runs on lattice-core's managed llama.cpp server (`byo`), when that server is installed;
//! - **OpenAI-compatible endpoints** of lattice-core's registry (`model_endpoints.json`, `registry::upsert`), enabled
//!   ones listed. Add an endpoint… takes an address, a model name and, if it needs one, a key: the key goes to Windows
//!   Credential Manager as `Alelyon/<NAME>` (`keys::vault_store`, where lattice-core keeps keys typed into Alelyon),
//!   the registry keeps only its name, and the key is never written to the preferences, logged or shown again.
//!
//! **Switching** is one click per surface, two surfaces kept apart:
//! - **Lattice chat**: a GGUF model is chosen as Local's model (`local_model::set_selected_model`, the
//!   `analyst_model.json` every Lattice reads) and the chat set to Local; an endpoint sets the chat to that endpoint.
//!   With nothing chosen here the chat keeps its own choice (Auto), as before;
//! - **Sinai**: kept in Alelyon's preferences (`"sinai_model"`); with nothing kept, it is Local's model, so nothing
//!   changes for a person who never chose. Where the build carries Sinai's mind (`sinai-mind`), the mind answers;
//!   the person's own model answers only when they pick it on the page for this session ([`answering`]).
//!
//! **Readiness** is said on every row, live and change-only: the models folder, the registry, Local's choice and the
//! preferences are watched (`live`), and an endpoint on this PC is knocked on (a connection opened and closed, as
//! `probe` does for the services) every few seconds while the panel shows it, the window told only when an answer
//! changes. An endpoint off this PC is not contacted until a message is sent to it. There is no Check again.

pub mod byo;
pub mod hosted;
#[cfg(windows)]
#[allow(unsafe_code)]
mod picker;
pub mod view;

use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iced::{Subscription, Task};

use lattice_core::keys::{self, KeySource, SecretString};
use lattice_core::llama::{self, files};
use lattice_core::registry::{self, EndpointKind, ModelEndpoint};
use lattice_core::{Env, KeyStore, ProcessEnv, StateRoot};

use crate::live::{Freshness, Seen, Source, Watch};

/// How often an endpoint on this PC is knocked on while the panel (or Sinai's page talking to it) shows it.
pub const KNOCK_EVERY: Duration = Duration::from_secs(3);

// ------------------------------------------------------------------- what a choice is

/// A model of the person's own.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// A GGUF file of the models folder, by its name (the file's stem).
    Gguf(String),
    /// An endpoint of the registry, by its id.
    Endpoint(String),
}

impl Pick {
    /// As the preferences keep it: `gguf:<name>` or `endpoint:<id>`.
    pub fn key(&self) -> String {
        match self {
            Pick::Gguf(name) => format!("gguf:{name}"),
            Pick::Endpoint(id) => format!("endpoint:{id}"),
        }
    }

    pub fn parse(key: &str) -> Option<Pick> {
        let (kind, rest) = key.split_once(':')?;
        let rest = rest.trim();
        if rest.is_empty() {
            return None;
        }
        match kind {
            "gguf" => Some(Pick::Gguf(rest.to_string())),
            "endpoint" => Some(Pick::Endpoint(rest.to_string())),
            _ => None,
        }
    }
}

/// The two surfaces a model is chosen for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Sinai,
    Lattice,
}

/// The model Sinai's page talks to when its mind is not the one answering: the one kept, else Local's model.
pub fn sinai_pick(kept: Option<&Pick>, local: &str) -> Option<Pick> {
    kept.cloned().or_else(|| (!local.trim().is_empty()).then(|| Pick::Gguf(local.trim().to_string())))
}

/// The model Lattice's chat uses, from its choice (`auto`, `local`, `endpoint:<id>`, ...) and Local's model: the
/// model, when the choice names one of the person's own.
pub fn lattice_pick(choice: &str, local: &str) -> Option<Pick> {
    match choice {
        "local" => (!local.trim().is_empty()).then(|| Pick::Gguf(local.trim().to_string())),
        _ => choice.strip_prefix("endpoint:").filter(|id| *id != "llamacpp-local").map(|id| Pick::Endpoint(id.to_string())),
    }
}

/// Who answers on Sinai's page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answering {
    /// Sinai's mind (the build's `sinai-mind`).
    Mind,
    /// The person's own model.
    Own(Pick),
    /// Nobody: no mind in this build and no model chosen.
    Nobody,
}

/// The priority rule: where the build carries Sinai's mind, the mind answers, unless the person explicitly picked their
/// own model instead (`instead`, for this session); a model of their own never silently replaces the mind. Without
/// the mind, the person's own model answers, if they have one.
pub fn answering(mind: bool, instead: bool, own: Option<&Pick>) -> Answering {
    match (mind, own) {
        (true, Some(pick)) if instead => Answering::Own(pick.clone()),
        (true, _) => Answering::Mind,
        (false, Some(pick)) => Answering::Own(pick.clone()),
        (false, None) => Answering::Nobody,
    }
}

// ------------------------------------------------------------------- where things are

/// Keeps a key typed for an endpoint: Windows Credential Manager in Alelyon; a test's own in its tests.
pub trait KeyKeeper: Send + Sync {
    fn keep(&self, name: &str, value: &SecretString) -> Result<(), String>;
    fn forget(&self, name: &str) -> Result<bool, String>;
}

/// Windows Credential Manager, as lattice-core keeps keys typed into Alelyon (`Alelyon/<NAME>`).
pub struct Vault;

impl KeyKeeper for Vault {
    fn keep(&self, name: &str, value: &SecretString) -> Result<(), String> {
        keys::vault_store(name, value)
    }

    fn forget(&self, name: &str) -> Result<bool, String> {
        keys::vault_remove(name)
    }
}

/// What the panel reads and writes: the environment and state root lattice-core reads by, the models folder, the
/// registry, the preferences file (None: nothing is kept) and where keys go.
#[derive(Clone)]
pub struct Sources {
    pub env: Arc<dyn Env>,
    pub state: StateRoot,
    pub models_dir: PathBuf,
    pub registry: PathBuf,
    pub prefs: Option<PathBuf>,
    pub keeper: Arc<dyn KeyKeeper>,
}

impl std::fmt::Debug for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources").field("models_dir", &self.models_dir).field("registry", &self.registry).finish_non_exhaustive()
    }
}

impl Sources {
    /// This process's: lattice-core's own places. A test build keeps no preferences (it names none).
    pub fn real() -> Sources {
        let env: Arc<dyn Env> = Arc::new(ProcessEnv);
        let state = lattice_core::state::resolve();
        let models_dir = files::LlamaPaths::from_env(env.as_ref()).models_dir;
        let registry = registry::config_path(env.as_ref(), &state);
        let prefs = if cfg!(test) { None } else { crate::prefs::path() };
        Sources { env, state, models_dir, registry, prefs, keeper: Arc::new(Vault) }
    }

    pub fn keys(&self) -> KeyStore {
        KeyStore::new(self.env.clone(), &self.state)
    }

    fn local_choice(&self) -> PathBuf {
        self.state.globals.join("analyst_model.json")
    }

    /// The files and folders whose change can change what the panel shows.
    pub fn watched(&self) -> Vec<(String, Source)> {
        let mut out = vec![
            ("the models folder".to_string(), Source::Folder { path: self.models_dir.clone(), subtree: true }),
            ("the model registry".to_string(), Source::File(self.registry.clone())),
            ("Local's model".to_string(), Source::File(self.local_choice())),
        ];
        if let Some(prefs) = &self.prefs {
            out.push(("Alelyon's preferences".to_string(), Source::File(prefs.clone())));
        }
        out
    }
}

// ------------------------------------------------------------------- what is there

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gguf {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub id: String,
    pub label: String,
    /// Its address, as the registry holds it (no credentials: the registry refuses them).
    pub address: String,
    pub model: String,
    /// On this PC (a loopback address).
    pub local: bool,
    /// Enabled, with an address, a model name and its key: what lattice-core calls ready.
    pub configured: bool,
    /// Why not, when not configured, in a sentence.
    pub status: String,
    /// Where the key it names is kept, in words; never its value. None when it needs none or none is kept.
    pub key_kept: Option<&'static str>,
    pub needs_key: bool,
    /// The host and port knocked on, for an endpoint on this PC.
    pub knock: Option<(String, u16)>,
}

/// What the panel shows, as one read found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub models_dir: PathBuf,
    /// llama.cpp's server, as lattice-core finds it, or why it cannot run a GGUF model.
    pub server: Result<PathBuf, String>,
    pub ggufs: Vec<Gguf>,
    pub endpoints: Vec<Endpoint>,
    /// Registry rows not listed: turned off, or not an OpenAI-compatible endpoint.
    pub not_listed: usize,
    /// The registry could not be read whole (its rows that could be are listed).
    pub registry_issue: Option<String>,
    /// Local's model (`analyst_model.json`), or empty.
    pub local: String,
    /// Sinai's model as the preferences keep it.
    pub sinai_kept: Option<Pick>,
    /// Each hosted provider (`hosted`), by id, and where its key is found (None: not connected).
    pub hosted: Vec<(&'static str, Option<&'static str>)>,
}

/// One sentence for a registry status (`ModelEndpoint::status`), never naming a key.
pub fn status_sentence(status: &str) -> String {
    match status {
        "ready" => "Set up.".into(),
        "disabled" => "It is turned off in the model registry.".into(),
        "no server URL set" => "No address is set for it.".into(),
        "no model name set" => "No model name is set for it.".into(),
        s if s.starts_with("needs ") => "It needs its key, and none is kept.".into(),
        _ => "It is not ready.".into(),
    }
}

/// `http(s)://host[:port]/...`: the host and port a connection would go to.
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let url = url.trim();
    let (rest, default) = if let Some(r) = url.strip_prefix("http://") {
        (r, 80)
    } else if let Some(r) = url.strip_prefix("https://") {
        (r, 443)
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if after.is_empty() => default,
            None => return None,
        };
        return Some((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
        None => Some((authority.to_string(), default)),
    }
}

/// Read what the panel shows. Reads only: nothing is written, started or contacted.
pub fn read(sources: &Sources) -> Catalog {
    let paths = files::LlamaPaths::from_env(sources.env.as_ref());
    let server = files::find_binary(&paths).map_err(|p| llama::binary_sentence(p).to_string());
    let ggufs = files::list_models(&sources.models_dir).into_iter().map(|m| Gguf { name: m.name, path: m.path, size: m.size }).collect();
    let report = registry::load_with_report(&sources.registry);
    let registry_issue = (!report.complete()).then(|| {
        let names: Vec<&str> = report.issues.iter().map(|i| i.as_str()).collect();
        format!("The model registry could not be read whole ({}); the endpoints that could be are listed.", names.join(", "))
    });
    let keys = sources.keys();
    let kept_in = |name: &str| {
        keys.source(name).map(|s| match s {
            KeySource::Environment => "the environment",
            KeySource::CredentialManager => "Windows Credential Manager",
            KeySource::File(_) => "an env file",
        })
    };
    let hosted = hosted::providers().iter().map(|p| (p.id, kept_in(p.key_name))).collect();
    let mut endpoints = Vec::new();
    let mut not_listed = 0;
    for e in report.endpoints {
        if e.kind == EndpointKind::Llamacpp {
            // the managed server's own row: its models are the GGUF files above
            continue;
        }
        if e.kind != EndpointKind::OpenaiCompatible || !e.enabled {
            not_listed += 1;
            continue;
        }
        let local = e.local(sources.env.as_ref());
        let status = e.status(&keys);
        endpoints.push(Endpoint {
            knock: if local { host_port(&e.base_url) } else { None },
            configured: e.ready(&keys),
            status: status_sentence(&status),
            key_kept: (!e.api_key_name.is_empty()).then(|| kept_in(&e.api_key_name)).flatten(),
            needs_key: e.needs_key(),
            id: e.id,
            label: e.label,
            address: e.base_url,
            model: e.model,
            local,
        });
    }
    Catalog {
        models_dir: sources.models_dir.clone(),
        server,
        ggufs,
        endpoints,
        not_listed,
        registry_issue,
        local: files::selected_model(&sources.state),
        sinai_kept: sources.prefs.as_deref().and_then(crate::prefs::load_sinai_model).as_deref().and_then(Pick::parse),
        hosted,
    }
}

/// Whether a model can be used now, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ready {
    Yes(String),
    /// Not yet known (an endpoint on this PC not knocked on yet).
    Asking(String),
    No(String),
}

impl Ready {
    pub fn words(&self) -> &str {
        match self {
            Ready::Yes(w) | Ready::Asking(w) | Ready::No(w) => w,
        }
    }
}

impl Catalog {
    pub fn gguf(&self, name: &str) -> Option<&Gguf> {
        self.ggufs.iter().find(|g| g.name == name)
    }

    pub fn endpoint(&self, id: &str) -> Option<&Endpoint> {
        self.endpoints.iter().find(|e| e.id == id)
    }

    /// A model's name as the page shows it.
    pub fn label(&self, pick: &Pick) -> String {
        match pick {
            Pick::Gguf(name) => name.clone(),
            Pick::Endpoint(id) => self.endpoint(id).map(|e| e.label.clone()).unwrap_or_else(|| id.clone()),
        }
    }

    /// Whether `pick` can be used now, with `knocked` the endpoints on this PC as last knocked on (true: it answered).
    pub fn ready(&self, pick: &Pick, knocked: &HashMap<String, bool>) -> Ready {
        match pick {
            Pick::Gguf(name) => match (self.gguf(name), &self.server) {
                (None, _) => Ready::No(format!("Its file is not in the models folder ({}).", self.models_dir.display())),
                (Some(_), Err(why)) => Ready::No(format!("The file is here, but it cannot run yet. {why}")),
                (Some(_), Ok(_)) => Ready::Yes("The file is here; llama.cpp starts it on this PC when you send.".into()),
            },
            Pick::Endpoint(id) => match self.endpoint(id) {
                None => Ready::No("It is not in the model registry, or it is turned off there.".into()),
                Some(e) if !e.configured => Ready::No(e.status.clone()),
                Some(e) if !e.local => Ready::Yes(format!(
                    "Set up. It is off this PC ({}): it is not contacted until you send, and what you send goes there.",
                    host_port(&e.address).map(|(h, _)| h).unwrap_or_else(|| e.address.clone())
                )),
                Some(e) => {
                    let at = e.knock.as_ref().map(|(h, p)| format!("{h}:{p}")).unwrap_or_else(|| e.address.clone());
                    match knocked.get(&e.id) {
                        Some(true) => Ready::Yes(format!("It answers at {at}.")),
                        Some(false) => Ready::No(format!("Nothing answers at {at}: start its server.")),
                        None => Ready::Asking(format!("Asking whether anything answers at {at}…")),
                    }
                }
            },
        }
    }

    /// Why `pick` cannot be chosen, if it cannot: its file is not in the folder, or the endpoint is not listed or not
    /// set up. A model that is there but cannot run yet (no llama.cpp server, nothing answering) can be chosen; its row
    /// says why it is not ready.
    pub fn cannot_choose(&self, pick: &Pick) -> Option<String> {
        match pick {
            Pick::Gguf(name) => self.gguf(name).is_none().then(|| format!("its file is not in the models folder ({}).", self.models_dir.display())),
            Pick::Endpoint(id) => match self.endpoint(id) {
                None => Some("it is not in the model registry, or it is turned off there.".into()),
                Some(e) if !e.configured => Some(e.status.to_lowercase()),
                Some(_) => None,
            },
        }
    }

    /// Where a message to `pick` goes (for Sinai's strip), or why it cannot go.
    pub fn route(&self, pick: &Pick, sources: &Sources) -> Result<byo::Route, String> {
        match pick {
            Pick::Gguf(name) => {
                self.gguf(name).ok_or_else(|| format!("{name} is not in the models folder any more."))?;
                if let Err(why) = &self.server {
                    return Err(why.clone());
                }
                Ok(byo::Route {
                    target: byo::Target::Gguf { name: name.clone(), models_dir: self.models_dir.clone() },
                    label: name.clone(),
                    off_pc: None,
                })
            }
            Pick::Endpoint(id) => {
                let e = self.endpoint(id).ok_or("That endpoint is not in the model registry, or it is turned off there.")?;
                if !e.configured {
                    return Err(e.status.clone());
                }
                Ok(byo::Route {
                    target: byo::Target::Endpoint { id: id.clone(), registry: sources.registry.clone() },
                    label: e.label.clone(),
                    off_pc: (!e.local).then(|| host_port(&e.address).map(|(h, _)| h).unwrap_or_else(|| e.address.clone())),
                })
            }
        }
    }
}

// ------------------------------------------------------------------- adding

/// Bring a GGUF file into the models folder: checked to be a GGUF model (its `GGUF` magic, read through local links
/// only, as lattice-core reads models; not a vision projector), then hard-linked there under its own name, so the
/// folder lists it and nothing is copied. Returns its name. A file already in the folder is simply there; a name the
/// folder already has is refused (lattice-core never guesses between two files of one name); a file on another drive
/// is refused with what to do instead.
pub fn add_gguf(source: &Path, models_dir: &Path) -> Result<String, String> {
    let file_name = source.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("That is not a file.")?;
    if !file_name.to_lowercase().ends_with(".gguf") {
        return Err(format!("{file_name} is not a GGUF file (its name does not end in .gguf)."));
    }
    if file_name.to_lowercase().starts_with("mmproj") {
        return Err(format!("{file_name} is a vision projector, a companion of a model, not a model."));
    }
    if !files::is_gguf(source) {
        return Err(format!("{file_name} does not start the way a GGUF file does, or it cannot be read on this PC."));
    }
    let name = file_name[..file_name.len() - ".gguf".len()].to_string();
    let listed = files::list_models(models_dir);
    if let Some(there) = listed.iter().find(|m| m.name == name) {
        if same_file(&there.path, source) {
            return Ok(name);
        }
        return Err(format!("The models folder already has a model named {name}. Rename the file and add it again."));
    }
    std::fs::create_dir_all(models_dir).map_err(|e| format!("The models folder could not be made ({}): {e}", models_dir.display()))?;
    let link = models_dir.join(&file_name);
    if let Err(e) = std::fs::hard_link(source, &link) {
        // ERROR_NOT_SAME_DEVICE: a hard link cannot cross drives.
        if e.raw_os_error() == Some(17) {
            return Err(format!(
                "{file_name} is on another drive than the models folder ({}), and a link cannot cross drives. Move or \
                 copy it into that folder yourself, or set ALELYON_MODELS_DIR to a folder on its drive.",
                models_dir.display()
            ));
        }
        return Err(format!("{file_name} could not be linked into the models folder: {e}"));
    }
    if files::list_models(models_dir).iter().any(|m| m.name == name) {
        Ok(name)
    } else {
        Err(format!("{file_name} was linked into the models folder, but the folder does not list it."))
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The endpoint form, as typed. The key lives here only until Save, then goes to Credential Manager and is forgotten.
#[derive(Clone, Default)]
pub struct EndpointForm {
    pub label: String,
    pub address: String,
    pub model: String,
    pub key: String,
    pub problem: Option<String>,
}

impl std::fmt::Debug for EndpointForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndpointForm")
            .field("label", &self.label)
            .field("address", &self.address)
            .field("model", &self.model)
            .field("key", &if self.key.is_empty() { "" } else { "***" })
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Label,
    Address,
    Model,
    Key,
}

/// A registry id from a name: lowercase letters, digits and dashes, not one of `taken`.
pub fn new_id(name: &str, taken: &[String]) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let mut slug: String = slug.trim_matches('-').chars().take(40).collect();
    slug = slug.trim_matches('-').to_string();
    if slug.is_empty() || !slug.starts_with(|c: char| c.is_ascii_lowercase()) {
        slug = format!("endpoint-{slug}").trim_matches('-').to_string();
    }
    let mut id = slug.clone();
    let mut n = 2;
    while taken.iter().any(|t| *t == id) {
        id = format!("{slug}-{n}");
        n += 1;
    }
    id
}

/// The name a key for endpoint `id` is kept under: `ALELYON_<ID>_KEY`, as lattice-core's names go.
pub fn key_name(id: &str) -> String {
    let mut name = format!("ALELYON_{}_KEY", id.to_uppercase().replace('-', "_"));
    name.truncate(64);
    name
}

/// The endpoint the form describes, checked by the registry's own rules (the same constructor its writer applies:
/// http or https with a host, no user, query or fragment, https when it has a key and is off this PC), with the key's
/// name when a key was typed.
pub fn endpoint_of(form: &EndpointForm, taken: &[String]) -> Result<(ModelEndpoint, Option<String>), String> {
    let address = form.address.trim().to_string();
    if address.is_empty() {
        return Err("Type the endpoint's address, such as http://127.0.0.1:8080/v1.".into());
    }
    let model = form.model.trim().to_string();
    if model.is_empty() {
        return Err("Type the model's name as the server knows it (llama.cpp's own server takes any name).".into());
    }
    let label = if form.label.trim().is_empty() {
        host_port(&address).map(|(h, _)| h).unwrap_or_else(|| address.clone())
    } else {
        form.label.trim().to_string()
    };
    let id = new_id(&label, taken);
    let key = (!form.key.trim().is_empty()).then(|| key_name(&id));
    let endpoint = ModelEndpoint {
        id: id.clone(),
        label,
        kind: EndpointKind::OpenaiCompatible,
        base_url: address,
        model,
        api_key_name: key.clone().unwrap_or_default(),
        enabled: true,
        builtin: false,
        note: "Added in Alelyon's Models panel.".into(),
    };
    if key.as_deref().is_some_and(|k| !keys::valid_key_name(k)) {
        return Err("A key name could not be made for that name; give the endpoint a shorter name.".into());
    }
    // The registry's own rules, as its writer applies them, on this one row.
    let checked = registry::load_bytes_with_report(registry::registry_text(std::slice::from_ref(&endpoint)).as_bytes());
    if !checked.complete() || !checked.endpoints.iter().any(|e| e.id == id) {
        return Err(if key.is_some() && !registry::is_local_url(&endpoint.base_url) {
            "That address is not accepted: an endpoint off this PC that takes a key must use https, with a host and no \
             user name, query or fragment."
                .into()
        } else {
            "That address is not accepted: it must be http or https, with a host and no user name, query or fragment."
                .into()
        });
    }
    Ok((endpoint, key))
}

/// Save a new endpoint: its key (when one was typed) to the keeper first, then the row to the registry; a registry
/// that cannot be written takes the key back out. Returns the endpoint's id. The key's value is never returned,
/// written anywhere else or logged.
pub fn save_endpoint(sources: &Sources, form: &EndpointForm) -> Result<String, String> {
    let report = registry::load_with_report(&sources.registry);
    let taken: Vec<String> = report.endpoints.iter().map(|e| e.id.clone()).collect();
    let (endpoint, key) = endpoint_of(form, &taken)?;
    let id = endpoint.id.clone();
    if let Some(name) = &key {
        sources.keeper.keep(name, &SecretString::from(form.key.trim().to_string()))?;
    }
    if let Err(e) = registry::upsert(&sources.registry, endpoint) {
        if let Some(name) = &key {
            let _ = sources.keeper.forget(name);
        }
        return Err(e.to_string());
    }
    Ok(id)
}

/// Make `pick` Sinai's model: kept in the preferences.
pub fn use_for_sinai(sources: &Sources, pick: &Pick) -> Result<(), String> {
    match &sources.prefs {
        Some(at) => crate::prefs::save_sinai_model(at, &pick.key()),
        None => Err("This window keeps no preferences, so the choice was not kept.".into()),
    }
}

/// Make `pick` Lattice chat's model: a GGUF model becomes Local's model (refused when the folder has no such file);
/// returns the chat's choice id.
pub fn use_for_lattice(sources: &Sources, pick: &Pick) -> Result<String, String> {
    match pick {
        Pick::Gguf(name) => {
            lattice_core::local_model::set_selected_model(&sources.state, &sources.models_dir, name).map_err(|e| e.to_string())?;
            Ok("local".to_string())
        }
        Pick::Endpoint(id) => Ok(format!("endpoint:{id}")),
    }
}

// ------------------------------------------------------------------- knocking on endpoints on this PC

/// Endpoints on this PC to knock on: (id, host, port).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Knocks(pub Vec<(String, String, u16)>);

fn knock(host: &str, port: u16) -> bool {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().map(|a| a.collect()).unwrap_or_default();
    // only an address on this PC is ever knocked on
    addrs.iter().filter(|a| a.ip().is_loopback()).any(|a| TcpStream::connect_timeout(a, Duration::from_millis(250)).is_ok())
}

fn knocking(knocks: &Knocks) -> impl futures::Stream<Item = Vec<(String, bool)>> + use<> {
    let list = knocks.0.clone();
    iced::stream::channel(4, async move |mut output: futures::channel::mpsc::Sender<Vec<(String, bool)>>| {
        let _ = std::thread::Builder::new().name("centcom-models-knock".into()).spawn(move || {
            let mut said: Option<Vec<(String, bool)>> = None;
            while !output.is_closed() {
                let now: Vec<(String, bool)> = list.iter().map(|(id, h, p)| (id.clone(), knock(h, *p))).collect();
                if said.as_ref() != Some(&now) {
                    said = Some(now.clone());
                    if futures::executor::block_on(futures::SinkExt::send(&mut output, now)).is_err() {
                        return;
                    }
                }
                std::thread::sleep(KNOCK_EVERY);
            }
        });
    })
}

// ------------------------------------------------------------------- the panel's state

#[derive(Clone, Debug)]
pub enum Msg {
    Open,
    Close,
    Catalogued(Box<Catalog>),
    Live(Seen),
    Knocked(Vec<(String, bool)>),
    AddFile,
    FilePicked(Result<Option<PathBuf>, String>),
    FileAdded(Result<String, String>),
    NewEndpoint,
    Form(Field, String),
    SaveEndpoint,
    CancelForm,
    EndpointSaved(Result<String, String>),
    Use(Surface, Pick),
    Used(Surface, Pick, Result<String, String>),
    /// Sinai's page: talk to the person's own model instead of Sinai's mind (true), or go back (false).
    Instead(bool),
    Own(byo::Msg),
    /// The hosted providers' cards (`hosted`).
    Hosted(hosted::Msg),
    Dismiss,
}

/// The Models panel, and Sinai's page's own-model conversation.
pub struct State {
    pub open: bool,
    sources: Option<Sources>,
    pub catalog: Option<Catalog>,
    pub reading: bool,
    pub fresh: Freshness,
    /// Endpoints on this PC as last knocked on: true when something answered.
    pub knocked: HashMap<String, bool>,
    pub form: Option<EndpointForm>,
    /// A file being chosen or linked, or an endpoint being saved, or a switch being written.
    pub working: bool,
    /// What the last add or switch did.
    pub said: Option<Result<String, String>>,
    /// The person chose their own model over Sinai's mind, for this session.
    pub instead: bool,
    pub own: byo::Strip,
    /// The hosted providers' part of the panel.
    pub hosted: hosted::Hosted,
}

impl Default for State {
    fn default() -> State {
        State {
            open: false,
            sources: None,
            catalog: None,
            reading: false,
            fresh: Freshness::waiting(),
            knocked: HashMap::new(),
            form: None,
            working: false,
            said: None,
            instead: false,
            own: byo::Strip::default(),
            hosted: hosted::Hosted::default(),
        }
    }
}

impl State {
    /// The places this state reads and writes, made on first use (lattice-core's own places).
    pub fn sources(&mut self) -> &Sources {
        self.sources.get_or_insert_with(Sources::real)
    }

    /// Use these places (a test's).
    #[cfg(test)]
    pub fn with_sources(&mut self, sources: Sources) {
        self.sources = Some(sources);
    }

    pub fn busy(&self) -> bool {
        (self.reading && self.catalog.is_none())
            || self.working
            || self.own.waiting()
            || (self.hosted.working() && !self.hosted.signing_in())
    }

    /// The model Sinai's page talks to when its mind is not answering.
    pub fn sinai(&self) -> Option<Pick> {
        self.catalog.as_ref().and_then(|c| sinai_pick(c.sinai_kept.as_ref(), &c.local))
    }

    /// Read what the panel shows, off the window's thread.
    pub fn read(&mut self) -> Task<Msg> {
        if self.reading {
            return Task::none();
        }
        self.reading = true;
        let sources = self.sources().clone();
        Task::perform(off_thread(move || read(&sources)), |c| match c {
            Some(c) => Msg::Catalogued(Box::new(c)),
            None => Msg::Dismiss,
        })
    }

    /// What one message does. `lattice_choice` is Lattice chat's choice, which a switch for Lattice sets.
    pub fn update(&mut self, msg: Msg, lattice_choice: &mut String) -> Task<Msg> {
        match msg {
            Msg::Open => {
                self.open = true;
                self.said = None;
                return self.read();
            }
            Msg::Close => {
                self.open = false;
                self.form = None;
                // a sign-in still waiting for the browser ends with the panel
                let sources = self.sources().clone();
                let _ = self.hosted.update(hosted::Msg::CancelSignIn, &sources);
            }
            Msg::Catalogued(catalog) => {
                self.reading = false;
                // endpoints no longer listed are no longer knocked on
                self.knocked.retain(|id, _| catalog.endpoint(id).is_some());
                self.catalog = Some(*catalog);
            }
            Msg::Live(seen) => {
                self.fresh.saw(&seen);
                if !seen.first {
                    return self.read();
                }
            }
            Msg::Knocked(answers) => {
                for (id, up) in answers {
                    self.knocked.insert(id, up);
                }
            }
            Msg::AddFile => {
                if self.working {
                    return Task::none();
                }
                self.working = true;
                self.said = None;
                return Task::perform(off_thread(pick_file), |r| Msg::FilePicked(r.unwrap_or(Ok(None))));
            }
            Msg::FilePicked(Ok(Some(path))) => {
                let models_dir = self.sources().models_dir.clone();
                return Task::perform(off_thread(move || add_gguf(&path, &models_dir)), |r| {
                    Msg::FileAdded(r.unwrap_or_else(|| Err("The link was not made: its thread stopped.".into())))
                });
            }
            Msg::FilePicked(Ok(None)) => self.working = false,
            Msg::FilePicked(Err(why)) => {
                self.working = false;
                self.said = Some(Err(why));
            }
            Msg::FileAdded(result) => {
                self.working = false;
                self.said = Some(result.map(|name| format!("{name} is in the models folder now.")));
                return self.read();
            }
            Msg::NewEndpoint => {
                self.form = Some(EndpointForm::default());
                self.said = None;
            }
            Msg::Form(field, value) => {
                if let Some(form) = &mut self.form {
                    match field {
                        Field::Label => form.label = value,
                        Field::Address => form.address = value,
                        Field::Model => form.model = value,
                        Field::Key => form.key = value,
                    }
                    form.problem = None;
                }
            }
            Msg::CancelForm => self.form = None,
            Msg::SaveEndpoint => {
                let Some(form) = self.form.clone() else { return Task::none() };
                if self.working {
                    return Task::none();
                }
                // checked here first, so a wrong address says so without a write
                if let Err(why) = endpoint_of(&form, &[]) {
                    if let Some(f) = &mut self.form {
                        f.problem = Some(why);
                    }
                    return Task::none();
                }
                self.working = true;
                let sources = self.sources().clone();
                return Task::perform(off_thread(move || save_endpoint(&sources, &form)), |r| {
                    Msg::EndpointSaved(r.unwrap_or_else(|| Err("The endpoint was not saved: its thread stopped.".into())))
                });
            }
            Msg::EndpointSaved(Ok(id)) => {
                self.working = false;
                // the key typed is forgotten here with the form
                self.form = None;
                self.said = Some(Ok(format!("The endpoint is saved in the model registry as {id}.")));
                return self.read();
            }
            Msg::EndpointSaved(Err(why)) => {
                self.working = false;
                match &mut self.form {
                    Some(form) => form.problem = Some(why),
                    None => self.said = Some(Err(why)),
                }
            }
            Msg::Use(surface, pick) => {
                if self.working {
                    return Task::none();
                }
                // a model that is not there, or an endpoint not set up, is not chosen: the row says why
                if let Some(c) = &self.catalog
                    && let Some(why) = c.cannot_choose(&pick)
                {
                    self.said = Some(Err(format!("{} cannot be chosen: {why}", c.label(&pick))));
                    return Task::none();
                }
                if surface == Surface::Lattice && matches!(pick, Pick::Endpoint(_)) {
                    let choice = use_for_lattice(self.sources(), &pick);
                    return Task::done(Msg::Used(surface, pick, choice));
                }
                self.working = true;
                let sources = self.sources().clone();
                return Task::perform(
                    off_thread(move || {
                        let done = match surface {
                            Surface::Sinai => use_for_sinai(&sources, &pick).map(|()| String::new()),
                            Surface::Lattice => use_for_lattice(&sources, &pick),
                        };
                        (pick, done)
                    }),
                    move |r| match r {
                        Some((pick, done)) => Msg::Used(surface, pick, done),
                        None => Msg::Dismiss,
                    },
                );
            }
            Msg::Used(surface, pick, result) => {
                self.working = false;
                let label = self.catalog.as_ref().map(|c| c.label(&pick)).unwrap_or_else(|| pick.key());
                match result {
                    Ok(choice) => {
                        match surface {
                            Surface::Sinai => {
                                if let Some(c) = &mut self.catalog {
                                    c.sinai_kept = Some(pick);
                                }
                                self.said = Some(Ok(format!("Sinai's page talks to {label} now.")));
                            }
                            Surface::Lattice => {
                                *lattice_choice = choice;
                                if let (Some(c), Pick::Gguf(name)) = (&mut self.catalog, &pick) {
                                    c.local = name.clone();
                                }
                                self.said = Some(Ok(format!("Lattice's chat uses {label} now.")));
                            }
                        }
                        return self.read();
                    }
                    Err(why) => self.said = Some(Err(why)),
                }
            }
            Msg::Instead(on) => {
                self.instead = on;
                if !on {
                    // Sinai's mind answers again: the face is the loop's
                    self.own.rest();
                }
                if self.catalog.is_none() {
                    return self.read();
                }
            }
            Msg::Own(msg) => {
                let sources = self.sources().clone();
                let route = match (self.sinai(), &self.catalog) {
                    (Some(pick), Some(c)) => c.route(&pick, &sources),
                    _ => Err("Choose a model of your own first: Models… on this page.".into()),
                };
                return self.own.update(msg, route, move || byo::engine(sources.env.clone(), &sources.state)).map(Msg::Own);
            }
            Msg::Hosted(msg) => {
                if matches!(msg, hosted::Msg::Typed(..) | hosted::Msg::Filter(_)) {
                    // typing says nothing new
                } else {
                    self.said = None;
                }
                let sources = self.sources().clone();
                match self.hosted.update(msg, &sources) {
                    hosted::Then::Nothing => {}
                    hosted::Then::Task(task) => return task.map(Msg::Hosted),
                    hosted::Then::Say(words, error) => self.said = Some(if error { Err(words) } else { Ok(words) }),
                    hosted::Then::Catalogued(catalog, words) => {
                        let task = self.update(Msg::Catalogued(catalog), lattice_choice);
                        self.said = Some(Ok(words));
                        return task;
                    }
                    hosted::Then::Switch(catalog, surface, pick) => {
                        let _ = self.update(Msg::Catalogued(catalog), lattice_choice);
                        return self.update(Msg::Use(surface, pick), lattice_choice);
                    }
                }
            }
            Msg::Dismiss => {
                self.reading = false;
                self.working = false;
                self.said = None;
            }
        }
        Task::none()
    }

    /// While the panel shows (or Sinai's page talks to a model of the person's own, `own_on_show`): the watch over
    /// what it shows, and the knock on endpoints on this PC it shows. Nothing while neither is on show.
    pub fn subscription(&self, own_on_show: bool) -> Subscription<Msg> {
        if !(self.open || own_on_show) {
            return Subscription::none();
        }
        let mut subs = Vec::new();
        if let Some(sources) = &self.sources {
            subs.push(Watch::new("models", sources.watched()).subscription().map(Msg::Live));
        }
        if let Some(c) = &self.catalog {
            let wanted: Vec<&Endpoint> = if self.open {
                c.endpoints.iter().collect()
            } else {
                match self.sinai() {
                    Some(Pick::Endpoint(id)) => c.endpoint(&id).into_iter().collect(),
                    _ => Vec::new(),
                }
            };
            let knocks: Vec<(String, String, u16)> =
                wanted.iter().filter_map(|e| e.knock.clone().map(|(h, p)| (e.id.clone(), h, p))).collect();
            if !knocks.is_empty() {
                subs.push(Subscription::run_with(Knocks(knocks), knocking).map(Msg::Knocked));
            }
        }
        Subscription::batch(subs)
    }
}

#[cfg(windows)]
fn pick_file() -> Result<Option<PathBuf>, String> {
    picker::pick_file()
}

#[cfg(not(windows))]
fn pick_file() -> Result<Option<PathBuf>, String> {
    Err("Adding a file here needs Windows' file picker.".into())
}

async fn off_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let _ = std::thread::Builder::new().name("centcom-models".into()).spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.ok()
}

#[cfg(test)]
mod tests;
