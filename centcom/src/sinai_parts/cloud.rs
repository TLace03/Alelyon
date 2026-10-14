//! Free cloud models for Sinai's parts, on the person's own provider accounts: what each part offers, the steps
//! that come before anything is sent (Offer, Connect, Agree), and the pause at a free limit.
//!
//! **Offer.** Each part's picker offers "Use a free cloud model", naming the provider and the model:
//! - the mind: OpenRouter's free models, open-weight only, read live from its list;
//! - the eyes: Cloudflare Workers AI's Llama 3.2 11B Vision (off until the person turns them on here);
//! - the ears: Groq's `whisper-large-v3-turbo` (the model the local ears use);
//! - the embeddings: Cloudflare Workers AI's `bge-m3` (its live list also has Qwen3-Embedding-0.6B);
//! - the voice: Cloudflare Workers AI's MeloTTS.
//!
//! Gemini is offered for the mind, the eyes and the embeddings only as a labelled choice, never a default: its free
//! tier uses what is sent to improve Google's products. Free offers change, so each provider's list is read live
//! (`hosted::parts::models`) and the defaults above are suggestions within it.
//!
//! **Connect.** OpenRouter signs in in the person's own browser (or takes a pasted key); Groq and Gemini take a
//! pasted key; Cloudflare a pasted API token and the account id it belongs to. Keys go to Windows Credential Manager
//! (`keys::vault_store`, `Alelyon/<NAME>`), never to a file, a log or the screen; `ALELYON_NO_CREDENTIALS` refuses the
//! write. Alelyon holds no key of its own.
//!
//! **Agree, per part.** A card says what will be sent, to whom, the provider's data policy in one line and its free
//! limit. Only "Agree and use" keeps the choice, stamped with when it was agreed (`"agreed"` in the part's entry); a
//! cloud entry without that stamp is never used, so nothing is sent before Agree. Agreeing for one part agrees for
//! that part only: the eyes stay off until their own Agree.
//!
//! **Run out.** At a free limit the provider says when to come back; the part pauses until then ([`pause`]) and its
//! card says "Free limit reached, back at <time>". Nothing switches provider or pays. A pause is kept beside the
//! preferences (`cloud-pauses.json`: part, provider, until) so a restart does not send a request into a limit it
//! already knows, and is dropped once its time passes.
//!
//! **Cloudflare stays free by metering.** Every Cloudflare call is counted against the account's 10,000 free Neurons
//! a day (lattice-core's `hosted::meter`, from Cloudflare's published per-model rates) and refused before the count
//! could pass 9,000, until 00:00 UTC, whatever plan the account is on; the eyes, embeddings and voice share the one
//! count. A Cloudflare model the meter has no rate for is not offered.
//!
//! **Paid, only through Auto configure.** At a paid price tier, Auto configure may propose a paid model (an OpenRouter
//! model whose price its live list carries). After the proposal's Agree, that part's card opens with the model chosen
//! ([`Step::StartWith`]) and says its price and the monthly cap; only the card's own Agree keeps it. Its calls are
//! counted against the cap, and at the cap the part pauses ("Monthly cap reached").
//!
//! **The eyes' licence.** Cloudflare's Llama 3.2 Vision asks each account to accept Meta's licence. The eyes' Agree
//! card links it and has its own unticked "I accept the Llama 3.2 licence"; Agree stays off until it is ticked, and
//! only then is the acceptance sent, once per account and model (remembered in `cloud-licences.json`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use futures::future::{AbortHandle, Abortable};
use iced::widget::{Column, checkbox, column, container, row, space, text_input};
use iced::{Alignment, Element, Length, Task};

use lattice_core::hosted::HostedModel;
use lattice_core::hosted::meter::{self, Meter};
use lattice_core::hosted::parts::{self as routes, CLOUDFLARE, Cloud, Connect, GEMINI, GROQ, OPENROUTER};
use lattice_core::keys::SecretString;

use super::{Choice, Part, off_thread};
use crate::theme;
use crate::ui::{chip, label, mono, note, primary, secondary, strong, working};

// ------------------------------------------------------------------- what each part offers

/// A free cloud model a part offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub part: Part,
    pub cloud: &'static Cloud,
    /// Its suggested model; None: the person picks one from the provider's live list.
    pub model: Option<&'static str>,
    /// The provider's free limit, as its pages say it (read 2026-10-10).
    pub limit: &'static str,
    /// A licence the provider asks the person to accept before this model answers: what it is, and where it is read.
    pub licence: Option<(&'static str, &'static str)>,
    /// Offered only as a labelled choice, never a default: why.
    pub caveat: Option<&'static str>,
}

/// What the part asks a model to do.
pub fn task(part: Part) -> routes::Task {
    match part {
        Part::Mind => routes::Task::Chat,
        Part::Eyes => routes::Task::Vision,
        Part::Ears => routes::Task::Transcribe,
        Part::Embeddings => routes::Task::Embed,
        Part::Voice => routes::Task::Speak,
    }
}

/// What goes off this PC when the part runs in the cloud.
pub fn sends(part: Part) -> &'static str {
    match part {
        Part::Mind => "What you type to Sinai on its page, with the conversation so far.",
        Part::Eyes => "Pictures of the windows you choose, when Sinai looks.",
        Part::Ears => {
            "Your voice while Sinai is listening (and while captions or dictation are on), and the audio of files you \
             transcribe, while the speech engine runs in the cloud."
        }
        Part::Embeddings => "The text Sinai remembers or searches for.",
        Part::Voice => "The words Sinai says aloud.",
    }
}

const OPENROUTER_LIMIT: &str =
    "20 requests a minute and 50 a day (1,000 a day once the account has bought $10 of credit), for its free models.";
const GROQ_LIMIT: &str = "Groq lists 20 requests a minute, 2,000 a day and 8 hours of audio a day for this model; your account's limits page has its own.";
const CLOUDFLARE_LIMIT: &str = "10,000 Neurons a day across all Workers AI models, starting again at 00:00 UTC. Alelyon counts each call's Neurons from Cloudflare's published rates and stops at 9,000, before the free allocation, whatever your plan, until 00:00 UTC. Your eyes, embeddings and voice share that count.";
/// Meta's licence for Llama 3.2, as Meta publishes it.
pub const LLAMA_LICENCE: &str = "https://www.llama.com/llama3_2/license/";
const GEMINI_LIMIT: &str = "Per model and per day, as Google's rate-limits page lists them for the free tier.";
const GEMINI_CAVEAT: &str = "Gemini's free tier uses what you send to improve Google's products, with human review. It is never a default.";

/// The free cloud models `part` offers: its default first, then labelled choices.
pub fn offers(part: Part) -> Vec<Offer> {
    let default = |cloud, model, limit, licence| Offer { part, cloud, model, limit, licence, caveat: None };
    let gemini = |model| Offer { part, cloud: &GEMINI, model, limit: GEMINI_LIMIT, licence: None, caveat: Some(GEMINI_CAVEAT) };
    match part {
        Part::Mind => vec![default(&OPENROUTER, None, OPENROUTER_LIMIT, None), gemini(None)],
        Part::Eyes => vec![
            default(
                &CLOUDFLARE,
                Some("@cf/meta/llama-3.2-11b-vision-instruct"),
                CLOUDFLARE_LIMIT,
                Some((
                    "Cloudflare asks each account to accept Meta's Llama 3.2 Community License and Acceptable Use Policy for this model.",
                    LLAMA_LICENCE,
                )),
            ),
            gemini(None),
        ],
        Part::Ears => vec![default(&GROQ, Some("whisper-large-v3-turbo"), GROQ_LIMIT, None)],
        Part::Embeddings => vec![default(&CLOUDFLARE, Some("@cf/baai/bge-m3"), CLOUDFLARE_LIMIT, None), gemini(None)],
        Part::Voice => vec![default(&CLOUDFLARE, Some("@cf/myshell-ai/melotts"), CLOUDFLARE_LIMIT, None)],
    }
}

/// `part`'s offer on `cloud`.
pub fn offer(part: Part, cloud: &str) -> Option<Offer> {
    offers(part).into_iter().find(|o| o.cloud.id == cloud)
}

/// Whether `part` may use `model` of `cloud`: a provider it offers, and a free model.
pub fn allowed(part: Part, cloud: &str, model: &str) -> bool {
    // a Cloudflare model the meter cannot count is not offered: it could not be kept within the free allocation
    offer(part, cloud).is_some_and(|o| routes::free_model(o.cloud, model) && (o.cloud.id != CLOUDFLARE.id || meter::rate(model).is_some()))
}

// ------------------------------------------------------------------- the person's keys

/// A key or account id where lattice-core finds keys (Credential Manager, the environment, its env files). A test
/// build finds only the names its own thread made up ([`test_keys`]).
pub fn found(name: &str) -> Option<SecretString> {
    #[cfg(test)]
    {
        TEST_KEYS.with(|k| k.borrow().contains(&name.to_string()).then(|| SecretString::new("stub-key-for-tests")))
    }
    #[cfg(not(test))]
    {
        lattice_core::keys::get_key(name)
    }
}

#[cfg(test)]
thread_local! {
    static TEST_KEYS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The key names this test thread's [`found`] finds (made-up keys, never a real one).
#[cfg(test)]
pub fn test_keys(names: &[&str]) {
    TEST_KEYS.with(|k| *k.borrow_mut() = names.iter().map(|n| n.to_string()).collect());
}

pub fn connected(cloud: &Cloud) -> bool {
    routes::connected(cloud, &found)
}

/// The provider with the person's key, ready for a route.
pub fn reach(cloud: &'static Cloud) -> Result<routes::Reach, String> {
    routes::Reach::found(cloud, &found)
}

/// Keep a key (and an account id) the person pasted.
fn keep(cloud: &Cloud, key: &str, account: &str) -> Result<(), String> {
    let key = SecretString::new(key.trim());
    if key.expose().is_empty() {
        return Err(format!("Paste your {} key first.", cloud.label));
    }
    if let Some(name) = cloud.account_name {
        let account = account.trim();
        if account.is_empty() || !account.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("Paste your Cloudflare account id (letters and digits, from the dashboard's Workers AI page).".into());
        }
        lattice_core::keys::vault_store(name, &SecretString::new(account))?;
    }
    lattice_core::keys::vault_store(cloud.key_name, &key)
}

// ------------------------------------------------------------------- pauses

/// A part paused at a provider's free limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paused {
    pub provider: String,
    pub until: i64,
}

/// Beside the preferences: a file of this name. None in a test build, which keeps nothing.
pub(crate) fn beside_prefs(name: &str) -> Option<PathBuf> {
    if cfg!(test) { None } else { crate::prefs::path().and_then(|p| p.parent().map(|d| d.join(name))) }
}

/// The pauses kept at `path` that still last at `now` (a part's key, its provider, until when).
pub fn load_pauses(path: &Path, now: i64) -> HashMap<Part, Paused> {
    let Some(value) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
        return HashMap::new();
    };
    Part::ALL
        .into_iter()
        .filter_map(|part| {
            let v = value.get(part.key())?;
            let provider = v.get("provider")?.as_str()?.to_string();
            let until = v.get("until")?.as_i64()?;
            (until > now).then_some((part, Paused { provider, until }))
        })
        .collect()
}

/// Keep `pauses` at `path` (whole, then renamed into place).
pub fn save_pauses(path: &Path, pauses: &HashMap<Part, Paused>) -> Result<(), String> {
    let mut v = serde_json::Map::new();
    for (part, p) in pauses {
        v.insert(part.key().into(), serde_json::json!({"provider": p.provider, "until": p.until}));
    }
    write_whole(path, &serde_json::Value::Object(v).to_string())
}

fn write_whole(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let partial = path.with_extension("json.saving");
    std::fs::write(&partial, text).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        e.to_string()
    })
}

static PAUSED: LazyLock<Mutex<HashMap<Part, Paused>>> =
    LazyLock::new(|| Mutex::new(beside_prefs("cloud-pauses.json").map(|p| load_pauses(&p, now())).unwrap_or_default()));

/// Seconds since 1970, now.
pub fn now() -> i64 {
    crate::utc::now() as i64
}

/// `part` paused at `provider`'s free limit until `until` (seconds since 1970); a later time wins. Kept on disk,
/// merged into the pauses there now (the parts service keeps pauses there too), and the cache is read back from it.
pub fn pause(part: Part, provider: &str, until: i64) {
    let mut paused = PAUSED.lock().unwrap_or_else(|e| e.into_inner());
    let entry = paused.entry(part).or_insert(Paused { provider: provider.to_string(), until });
    if entry.provider != provider || until > entry.until {
        *entry = Paused { provider: provider.to_string(), until };
    }
    if let Some(path) = beside_prefs("cloud-pauses.json") {
        if pause_at(&path, part, provider, until, now()).is_ok() {
            *paused = load_pauses(&path, now());
        }
    }
}

/// When `part`'s pause ends, while it lasts at `now` (on any provider). A pause past its time is dropped; the file
/// is rewritten from what is on disk now, so a pause another process kept there is not lost.
pub fn paused(part: Part, now: i64) -> Option<i64> {
    let mut paused = PAUSED.lock().unwrap_or_else(|e| e.into_inner());
    match paused.get(&part).map(|p| p.until) {
        Some(t) if t > now => Some(t),
        Some(_) => {
            paused.remove(&part);
            if let Some(path) = beside_prefs("cloud-pauses.json") {
                let kept = load_pauses(&path, now);
                if save_pauses(&path, &kept).is_ok() {
                    *paused = kept;
                }
            }
            paused.get(&part).map(|p| p.until)
        }
        None => None,
    }
}

/// When `part`'s pause on `provider` ends, while it lasts at `now`; Cloudflare's parts also pause while the account's
/// metered count has reached its stop.
pub fn paused_on(part: Part, provider: &str, now: i64) -> Option<i64> {
    let own = paused(part, now).filter(|_| {
        PAUSED.lock().unwrap_or_else(|e| e.into_inner()).get(&part).is_some_and(|p| p.provider == provider)
    });
    own.or_else(|| (provider == CLOUDFLARE.id).then(|| cloudflare_spent(now)).flatten())
}

/// `part` paused on `provider` until `until`, written into the pauses kept at `path` as they are on disk now (another
/// process keeps pauses there too): a later time wins, and pauses past their time are dropped.
pub fn pause_at(path: &Path, part: Part, provider: &str, until: i64, now: i64) -> Result<(), String> {
    let mut kept = load_pauses(path, now);
    let entry = kept.entry(part).or_insert(Paused { provider: provider.to_string(), until });
    if entry.provider != provider || until > entry.until {
        *entry = Paused { provider: provider.to_string(), until };
    }
    save_pauses(path, &kept)
}

/// When `part`'s pause on `provider`, as kept at `path` on disk now, ends; None when it does not last at `now`.
pub fn paused_at(path: &Path, part: Part, provider: &str, now: i64) -> Option<i64> {
    load_pauses(path, now).get(&part).filter(|p| p.provider == provider).map(|p| p.until)
}

// ------------------------------------------------------------------- Cloudflare's meter, as the cards read it

#[cfg(test)]
thread_local! {
    static TEST_METER: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    static TEST_LICENCES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The meter's file this test thread reads (None: no meter).
#[cfg(test)]
pub fn test_meter(path: Option<PathBuf>) {
    TEST_METER.with(|m| *m.borrow_mut() = path);
}

/// Cloudflare's meter for this user (lattice-core's own file, which every Cloudflare call counts in).
pub fn cloudflare_meter() -> Option<Meter> {
    #[cfg(test)]
    {
        TEST_METER.with(|m| m.borrow().clone()).map(Meter::at)
    }
    #[cfg(not(test))]
    {
        meter::default_path().map(Meter::at)
    }
}

/// Neurons counted today on the connected Cloudflare account, where there is one.
pub fn cloudflare_used(now: i64) -> Option<f64> {
    let account = found(CLOUDFLARE.account_name?)?;
    Some(cloudflare_meter()?.used(account.expose(), now))
}

/// The next 00:00 UTC, while the connected Cloudflare account's count has reached the meter's stop.
pub fn cloudflare_spent(now: i64) -> Option<i64> {
    // within one Neuron of the stop, no call of any part fits any more
    cloudflare_used(now).filter(|used| *used >= meter::STOP_AT - 1.0).map(|_| meter::next_midnight(now))
}

// ------------------------------------------------------------------- licences accepted

fn licence_key(account: &str, model: &str) -> String {
    format!("cloudflare/{account}/{model}")
}

/// Whether the licence for `model` was accepted on `account` from this PC.
pub fn licence_accepted(account: &str, model: &str) -> bool {
    let key = licence_key(account, model);
    #[cfg(test)]
    {
        TEST_LICENCES.with(|l| l.borrow().contains(&key))
    }
    #[cfg(not(test))]
    {
        beside_prefs("cloud-licences.json")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .is_some_and(|v| v.get(&key).is_some())
    }
}

/// Remember that the licence for `model` was accepted on `account`, and when.
pub fn remember_licence(account: &str, model: &str, at: i64) -> Result<(), String> {
    let key = licence_key(account, model);
    #[cfg(test)]
    {
        let _ = at;
        TEST_LICENCES.with(|l| l.borrow_mut().push(key));
        Ok(())
    }
    #[cfg(not(test))]
    {
        let path = beside_prefs("cloud-licences.json").ok_or("This window keeps no preferences, so the licence was not remembered.")?;
        let mut v = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&t).ok())
            .unwrap_or_default();
        v.insert(key, serde_json::json!(stamp(at)));
        write_whole(&path, &serde_json::Value::Object(v).to_string())
    }
}

/// Send the licence's acceptance on the person's account (once they ticked it), and remember it.
fn accept(model: String) -> Result<(), String> {
    let reach = reach(&CLOUDFLARE)?;
    let account = reach.account().to_string();
    if licence_accepted(&account, &model) {
        return Ok(());
    }
    let client = routes::Client::new(&reach)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "The licence could not be sent.".to_string())?
        .block_on(routes::accept_licence(&client, &reach, &model, now()))
        .map_err(|f| match f {
            routes::Failure::Limited { back_at } => back_words(back_at, now()),
            other => other.sentence(),
        })?;
    remember_licence(&account, &model, now())
}

/// "Free limit reached, back at 14:05." in this PC's time (with the day when it is not today).
pub fn back_words(at: i64, now: i64) -> String {
    let offset = i64::from(crate::face::world::local_offset_seconds());
    let (local, today) = (at + offset, now + offset);
    let hm = format!("{:02}:{:02}", local.rem_euclid(86_400) / 3600, local.rem_euclid(86_400) % 3600 / 60);
    if local.div_euclid(86_400) == today.div_euclid(86_400) {
        format!("Free limit reached, back at {hm}.")
    } else {
        let day = crate::utc::day_stamp(local as f64);
        format!("Free limit reached, back at {hm} on {}.", &day[..5])
    }
}

// ------------------------------------------------------------------- what is chosen

/// The mind's cloud model, chosen and agreed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudPick {
    pub provider: String,
    pub model: String,
}

impl CloudPick {
    pub fn of(choice: Option<&Choice>) -> Option<CloudPick> {
        match choice {
            Some(Choice::Cloud { provider, model, agreed: Some(_) }) => Some(CloudPick { provider: provider.clone(), model: model.clone() }),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        let provider = routes::cloud(&self.provider).map_or(self.provider.as_str(), |c| c.label);
        format!("{} ({provider})", self.model)
    }
}

/// The provider's name, by id.
pub fn provider_label(id: &str) -> &str {
    routes::cloud(id).map_or(id, |c| c.label)
}

/// Where the mind's message goes when its cloud model answers, or why it cannot go now: paused, or not connected.
pub fn mind_route(pick: &CloudPick) -> Result<crate::models::byo::Route, String> {
    let cloud = routes::cloud(&pick.provider).ok_or("That provider is not one this version knows.")?;
    if let Some(until) = paused_on(Part::Mind, &pick.provider, now()) {
        return Err(back_words(until, now()));
    }
    if let Some(words) = super::auto::capped_words(Part::Mind, &pick.provider, &pick.model, now()) {
        return Err(words);
    }
    if !connected(cloud) {
        return Err(format!("Connect {} again in the mind's picker: no key is kept for it.", cloud.label));
    }
    Ok(crate::models::byo::Route {
        target: crate::models::byo::Target::Cloud { provider: pick.provider.clone(), model: pick.model.clone() },
        label: pick.label(),
        // the person agreed to send this part's words to this provider in its picker
        off_pc: None,
    })
}

/// What the speech engine is started with for a cloud choice of the ears: `--cloud <provider> --cloud-model <model>`.
pub fn ears_args(choice: Option<&Choice>) -> Vec<String> {
    let paused = match choice {
        Some(Choice::Cloud { provider, .. }) => paused_on(Part::Ears, provider, now()),
        _ => None,
    };
    ears_args_with(choice, paused)
}

/// [`ears_args`] with the ears' known pause given: `--cloud-paused-until <time>` while it lasts, so the engine starts
/// paused and sends nothing into a limit already known.
pub fn ears_args_with(choice: Option<&Choice>, paused: Option<i64>) -> Vec<String> {
    match choice {
        Some(Choice::Cloud { provider, model, agreed: Some(_) }) if allowed(Part::Ears, provider, model) => {
            let mut args = vec!["--cloud".into(), provider.clone(), "--cloud-model".into(), model.clone()];
            if let Some(until) = paused {
                args.extend(["--cloud-paused-until".to_string(), until.to_string()]);
            }
            args
        }
        _ => Vec::new(),
    }
}

/// The environment the speech engine is started with for a cloud choice of the ears: the provider's key under its
/// own name, read now; None when the choice is not the cloud. An error when the key is not found.
pub fn ears_env(choice: Option<&Choice>) -> Result<Option<(String, SecretString)>, String> {
    if ears_args(choice).is_empty() {
        return Ok(None);
    }
    let Some(Choice::Cloud { provider, .. }) = choice else { return Ok(None) };
    let cloud = routes::cloud(provider).ok_or("That provider is not one this version knows.")?;
    let key = found(cloud.key_name).ok_or_else(|| format!("Connect {} in the ears' picker first: no key is kept for it.", cloud.label))?;
    Ok(Some((cloud.key_name.to_string(), key)))
}

/// A card for a part on a cloud model: its model, where it runs, its state.
pub fn card_lines(part: Part, provider: &str, model: &str, now: i64) -> (String, String, super::Level, String) {
    let label = provider_label(provider);
    let capped = super::auto::capped_words(part, provider, model, now);
    let (level, state) = match paused_on(part, provider, now) {
        Some(until) => (super::Level::Problem, back_words(until, now)),
        None if capped.is_some() => (super::Level::Problem, capped.unwrap_or_default()),
        None if routes::cloud(provider).is_some_and(|c| !connected(c)) => {
            (super::Level::Problem, format!("Not connected: connect {label} again in this picker."))
        }
        None => {
            let what = sends(part).trim_end_matches('.');
            let mut first = what.chars();
            let what = first.next().map(|c| c.to_lowercase().chain(first).collect::<String>()).unwrap_or_default();
            let counted = match (provider == CLOUDFLARE.id, cloudflare_used(now)) {
                (true, Some(used)) => format!(" Today {used:.0} of the {:.0} Neurons Alelyon stops at.", meter::STOP_AT),
                _ => String::new(),
            };
            (super::Level::Up, format!("Sends {what} to {label}.{counted}"))
        }
    };
    (model.to_string(), format!("in the cloud ({label})"), level, state)
}

// ------------------------------------------------------------------- the steps

/// Where the steps are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Connect,
    Choose,
    Agree,
}

/// The steps for one part on one provider, open in its picker.
#[derive(Debug)]
pub struct Flow {
    pub offer: Offer,
    pub stage: Stage,
    pub model: Option<String>,
    key: String,
    account: String,
    pub models: Option<Result<Vec<HostedModel>, String>>,
    pub busy: bool,
    pub said: Option<String>,
    signing_in: Option<AbortHandle>,
    /// The person ticked "I accept the Llama 3.2 licence" (an offer with a licence only).
    pub licence_ticked: bool,
    /// Opened from Auto configure with its model already chosen: after Connect it goes straight to Agree.
    pub preset: bool,
    /// The paid model this card agrees to, with the tier's monthly cap, where Auto configure's Agree allowed one.
    pub paid: Option<(super::auto::PaidTerms, f64)>,
}

impl Flow {
    /// Whether `model` may be agreed to here: a free model the part offers, or the paid model this card carries.
    pub fn may_use(&self, model: &str) -> bool {
        allowed(self.offer.part, self.offer.cloud.id, model) || self.paid.as_ref().is_some_and(|(t, _)| t.model == model)
    }
}

impl Flow {
    pub fn signing_in(&self) -> bool {
        self.signing_in.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Key,
    Account,
}

#[derive(Clone, Debug)]
pub enum Step {
    /// Begin the steps for this part on this provider.
    Start(Part, &'static str),
    /// Begin them with this model already chosen (Auto configure's proposal): connect if needed, then Agree.
    StartWith(Part, String, String),
    Typed(Field, String),
    KeyPage,
    Connect,
    SignIn,
    CancelSignIn,
    Connected(Result<(), String>),
    Listed(Result<Vec<HostedModel>, String>),
    Pick(String),
    /// Back to choosing a model.
    Back,
    /// The licence's own tick.
    Licence(bool),
    /// Read the licence, in the person's own browser.
    ReadLicence,
    /// The licence's acceptance was sent (or why not).
    LicenceSent(Result<(), String>),
    Agree,
    Close,
}

/// What the steps ask of the picker.
pub enum Out {
    Nothing,
    Task(Task<Step>),
    /// Keep this choice for the part (the person agreed).
    Choose(Part, Choice),
}

/// UTC now, as the agreement's stamp.
pub fn stamp(at: i64) -> String {
    crate::utc::full(at as f64).replacen(' ', "T", 1)
}

fn list(offer: Offer) -> Task<Step> {
    let cloud = offer.cloud;
    let part = offer.part;
    Task::perform(
        off_thread(move || -> Result<Vec<HostedModel>, String> {
            let reach = reach(cloud)?;
            let client = routes::Client::new(&reach)?;
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "The list could not be read.".to_string())?
                .block_on(routes::models(&client, &reach, task(part), now()))
                .map_err(|f| match f {
                    routes::Failure::Limited { back_at } => back_words(back_at, now()),
                    other => other.sentence(),
                })
        }),
        |r| Step::Listed(r.unwrap_or_else(|| Err("The list was not read: its thread stopped.".into()))),
    )
}

/// One step. `flow` is the steps open in the picker (None: closed).
pub fn update(flow: &mut Option<Flow>, step: Step) -> Out {
    if let Step::StartWith(part, cloud, model) = step {
        let Some(offer) = offer(part, &cloud) else { return Out::Nothing };
        let paid = super::auto::settings_now().paid_for(part, offer.cloud.id, &model).map(|(t, cap)| (t.clone(), cap));
        let ready = connected(offer.cloud);
        let f = Flow {
            offer,
            stage: if ready { Stage::Agree } else { Stage::Connect },
            model: Some(model),
            key: String::new(),
            account: String::new(),
            models: None,
            busy: false,
            said: None,
            signing_in: None,
            licence_ticked: false,
            preset: true,
            paid,
        };
        if !f.model.as_deref().is_some_and(|m| f.may_use(m)) {
            *flow = None;
            return Out::Nothing;
        }
        *flow = Some(f);
        return Out::Nothing;
    }
    if let Step::Start(part, cloud) = step {
        let Some(offer) = offer(part, cloud) else { return Out::Nothing };
        let ready = connected(offer.cloud);
        *flow = Some(Flow {
            offer,
            stage: if ready { Stage::Choose } else { Stage::Connect },
            model: offer.model.map(str::to_string),
            key: String::new(),
            account: String::new(),
            models: None,
            busy: ready,
            said: None,
            signing_in: None,
            licence_ticked: false,
            preset: false,
            paid: None,
        });
        return if ready { Out::Task(list(offer)) } else { Out::Nothing };
    }
    let Some(f) = flow.as_mut() else { return Out::Nothing };
    match step {
        Step::Start(..) | Step::StartWith(..) => {}
        Step::Typed(Field::Key, v) => f.key = v,
        Step::Typed(Field::Account, v) => f.account = v,
        Step::KeyPage => {
            if let Err(why) = crate::signin::web::open_yours(f.offer.cloud.key_page) {
                f.said = Some(why);
            }
        }
        Step::Connect => {
            if f.busy {
                return Out::Nothing;
            }
            f.busy = true;
            f.said = None;
            let (cloud, key, account) = (f.offer.cloud, std::mem::take(&mut f.key), f.account.clone());
            return Out::Task(Task::perform(off_thread(move || keep(cloud, &key, &account)), |r| {
                Step::Connected(r.unwrap_or_else(|| Err("It was not connected: its thread stopped.".into())))
            }));
        }
        Step::SignIn => {
            if f.busy || f.offer.cloud.connect != Connect::OpenRouterSignIn {
                return Out::Nothing;
            }
            let (handle, registration) = AbortHandle::new_pair();
            f.busy = true;
            f.said = None;
            f.signing_in = Some(handle);
            let cloud = f.offer.cloud;
            let job = move || -> Result<(), String> {
                let key = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| "The sign-in could not start.".to_string())?
                    .block_on(async move {
                        let pending = lattice_core::hosted::openrouter::begin().await?;
                        crate::signin::web::open_yours(pending.url())?;
                        let web = lattice_core::hosted::Web::new()?;
                        Abortable::new(pending.finish(&web), registration)
                            .await
                            .unwrap_or_else(|_| Err("The sign-in was cancelled.".to_string()))
                    })?;
                lattice_core::keys::vault_store(cloud.key_name, &key)
            };
            return Out::Task(Task::perform(off_thread(job), |r| {
                Step::Connected(r.unwrap_or_else(|| Err("The sign-in stopped.".into())))
            }));
        }
        Step::CancelSignIn => {
            if let Some(h) = f.signing_in.take() {
                h.abort();
            }
        }
        Step::Connected(result) => {
            f.busy = false;
            f.signing_in = None;
            match result {
                Ok(()) => {
                    f.account.clear();
                    if f.preset && f.model.is_some() {
                        // Auto configure chose the model: its Agree is next
                        f.stage = Stage::Agree;
                        return Out::Nothing;
                    }
                    f.stage = Stage::Choose;
                    f.busy = true;
                    return Out::Task(list(f.offer));
                }
                Err(why) => f.said = Some(why),
            }
        }
        Step::Listed(result) => {
            f.busy = false;
            if let Ok(models) = &result {
                // the suggested model, where the live list still has it; else the person picks one
                // (a model already picked, at Agree, stays as picked)
                if f.stage == Stage::Choose && f.model.as_ref().is_some_and(|m| !models.iter().any(|x| &x.id == m)) {
                    f.model = None;
                }
            }
            f.models = Some(result);
        }
        Step::Pick(model) => {
            if f.may_use(&model) {
                f.model = Some(model);
                f.stage = Stage::Agree;
            } else {
                f.said = Some("That is not a free model: this version uses free models only.".into());
            }
        }
        Step::Back => f.stage = Stage::Choose,
        Step::Licence(on) => f.licence_ticked = on,
        Step::ReadLicence => {
            if let Some((_, url)) = f.offer.licence
                && let Err(why) = crate::signin::web::open_yours(url)
            {
                f.said = Some(why);
            }
        }
        Step::LicenceSent(result) => {
            f.busy = false;
            match result {
                Ok(()) => return agreed(flow),
                Err(why) => f.said = Some(format!("The licence was not accepted, so nothing was kept: {why}")),
            }
        }
        Step::Agree => {
            let Some(model) = f.model.clone().filter(|m| f.may_use(m)) else {
                f.said = Some("Choose a free model first.".into());
                return Out::Nothing;
            };
            if f.stage != Stage::Agree || f.busy {
                return Out::Nothing;
            }
            if f.offer.licence.is_some() {
                if !f.licence_ticked {
                    f.said = Some("Tick \"I accept the Llama 3.2 licence\" first.".into());
                    return Out::Nothing;
                }
                let accepted = found(CLOUDFLARE.account_name.unwrap_or_default()).is_some_and(|a| licence_accepted(a.expose(), &model));
                if !accepted {
                    // the acceptance goes on the person's account once, after their tick; the choice is kept after it
                    f.busy = true;
                    f.said = None;
                    return Out::Task(Task::perform(off_thread(move || accept(model)), |r| {
                        Step::LicenceSent(r.unwrap_or_else(|| Err("its thread stopped.".into())))
                    }));
                }
            }
            return agreed(flow);
        }
        Step::Close => {
            if let Some(h) = f.signing_in.take() {
                h.abort();
            }
            *flow = None;
        }
    }
    Out::Nothing
}

/// The choice the person agreed to, kept: the steps close.
fn agreed(flow: &mut Option<Flow>) -> Out {
    let Some(f) = flow.as_ref() else { return Out::Nothing };
    let Some(model) = f.model.clone() else { return Out::Nothing };
    let (part, provider) = (f.offer.part, f.offer.cloud.id.to_string());
    let choice = Choice::Cloud { provider, model, agreed: Some(stamp(now())) };
    *flow = None;
    Out::Choose(part, choice)
}

// ------------------------------------------------------------------- how it looks

type El<'a> = Element<'a, Step>;

fn line<'a>(name: &'a str, value: impl Into<El<'a>>) -> El<'a> {
    row![label(name, 12.5, theme::TEXT_FAINT).width(150), container(value).width(Length::Fill)].spacing(10).into()
}

/// The offer rows in a part's picker: one per provider, with "Use a free cloud model".
pub fn offer_rows<'a>(part: Part, busy: bool) -> El<'a> {
    let mut col = Column::new().spacing(8);
    for o in offers(part) {
        let model = o.model.map_or_else(|| "a free model from its live list".to_string(), str::to_string);
        let mut top = row![strong(format!("{} \u{b7} {}", o.cloud.label, model), 14.0, theme::TEXT)].spacing(10).align_y(Alignment::Center);
        top = top.push(chip("in the cloud", theme::GOLD_DIM));
        if o.caveat.is_some() {
            top = top.push(chip("trains on what you send", theme::CAUTION));
        }
        top = top.push(space().width(Length::Fill)).push(secondary("Use a free cloud model", (!busy).then_some(Step::Start(part, o.cloud.id))));
        let mut body = column![top, label(format!("Sends: {}", sends(part)), 12.5, theme::TEXT_DIM)].spacing(4);
        if let Some(c) = o.caveat {
            body = body.push(label(c, 12.5, theme::CAUTION));
        }
        body = body.push(label(if connected(o.cloud) { "Connected." } else { "Not connected yet: the next step connects your own account." }, 12.0, theme::TEXT_FAINT));
        col = col.push(container(body).padding([10.0, 12.0]).width(Length::Fill).style(theme::well));
    }
    col.into()
}

/// The steps, in place of the picker's list while they are open.
pub fn view<'a>(f: &'a Flow, phase: f32) -> El<'a> {
    let o = f.offer;
    let cloud = o.cloud;
    let mut col = Column::new().spacing(12);
    let title = match f.stage {
        Stage::Connect => format!("Connect {}", cloud.label),
        Stage::Choose => format!("Choose a free model on {}", cloud.label),
        Stage::Agree => format!("Send to {}?", cloud.label),
    };
    col = col.push(row![strong(title, 17.0, theme::TEXT).width(Length::Fill), secondary("Not now", Some(Step::Close))].spacing(10).align_y(Alignment::Center));
    if let Some(said) = &f.said {
        col = col.push(crate::ui::notice(said.as_str(), theme::CAUTION));
    }
    if let Some(c) = o.caveat {
        col = col.push(crate::ui::notice(c, theme::CAUTION));
    }
    match f.stage {
        Stage::Connect => {
            col = col.push(note(format!(
                "Your own {} account: Alelyon holds no key of its own. The key goes to Windows Credential Manager, never to a file or the screen.",
                cloud.label
            )));
            if cloud.connect == Connect::OpenRouterSignIn {
                let sign = if f.signing_in() {
                    row![working(phase, "Waiting for your approval in the browser"), secondary("Cancel", Some(Step::CancelSignIn))].spacing(10).align_y(Alignment::Center)
                } else {
                    row![primary("Sign in with OpenRouter", (!f.busy).then_some(Step::SignIn))].spacing(10)
                };
                col = col.push(sign).push(label("Or paste a key made on OpenRouter's keys page:", 12.5, theme::TEXT_DIM));
            }
            if cloud.account_name.is_some() {
                col = col.push(line(
                    "Account id",
                    text_input("Your Cloudflare account id", &f.account).on_input(|v| Step::Typed(Field::Account, v)).padding(8).size(13.5).style(crate::ui::input_style),
                ));
            }
            let key_name = if cloud.account_name.is_some() { "API token" } else { "Key" };
            col = col.push(line(
                key_name,
                text_input("Paste it here", &f.key).on_input(|v| Step::Typed(Field::Key, v)).secure(true).padding(8).size(13.5).style(crate::ui::input_style),
            ));
            col = col.push(
                row![
                    secondary("Open the key page", Some(Step::KeyPage)),
                    space().width(Length::Fill),
                    primary("Connect", (!f.busy && !f.key.trim().is_empty()).then_some(Step::Connect)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
            if f.busy && !f.signing_in() {
                col = col.push(working(phase, "Keeping the key"));
            }
        }
        Stage::Choose => match &f.models {
            None => col = col.push(working(phase, format!("Reading {}'s free models", cloud.label))),
            Some(Err(why)) => {
                col = col.push(crate::ui::notice(format!("The list was not read: {why}"), theme::CAUTION));
                if let Some(m) = o.model {
                    col = col.push(row![label(format!("The suggested model is {m}."), 13.0, theme::TEXT).width(Length::Fill), primary("Use it", Some(Step::Pick(m.to_string())))].spacing(10).align_y(Alignment::Center));
                }
            }
            Some(Ok(models)) if models.is_empty() => {
                col = col.push(note(format!("{} lists no free model for this part right now.", cloud.label)));
            }
            Some(Ok(models)) => {
                col = col.push(note(format!("{} free models, as {} lists them now:", models.len(), cloud.label)));
                let mut list = Column::new().spacing(6);
                for m in models.iter().take(60) {
                    let suggested = o.model == Some(m.id.as_str());
                    let mut r = row![column![strong(m.name.clone(), 13.5, theme::TEXT), mono(m.id.clone(), 11.5, theme::TEXT_FAINT)].spacing(2).width(Length::Fill)].spacing(10).align_y(Alignment::Center);
                    if suggested {
                        r = r.push(chip("suggested", theme::GOLD));
                    }
                    r = r.push(secondary("Choose", Some(Step::Pick(m.id.clone()))));
                    list = list.push(container(r).padding([8.0, 10.0]).width(Length::Fill).style(theme::well));
                }
                col = col.push(list);
            }
        },
        Stage::Agree => {
            let model = f.model.clone().unwrap_or_default();
            col = col.push(line("What is sent", label(sends(o.part), 13.0, theme::TEXT)));
            col = col.push(line("To whom", label(format!("{} ({model}), on your own account", cloud.label), 13.0, theme::TEXT)));
            col = col.push(line("Their data policy", label(cloud.policy, 13.0, theme::TEXT)));
            match &f.paid {
                Some((t, cap)) => {
                    col = col.push(line(
                        "Price",
                        label(
                            format!(
                                "Paid: ${:.2} for each million tokens sent and ${:.2} for each million written, charged by {} to your own account. It counts toward your monthly cap of ${cap:.2}; at the cap this part pauses until next month.",
                                t.price.input_per_m, t.price.output_per_m, cloud.label
                            ),
                            13.0,
                            theme::CAUTION,
                        ),
                    ));
                }
                None => col = col.push(line("The free limit", label(o.limit, 13.0, theme::TEXT))),
            }
            if let Some((licence, url)) = o.licence {
                col = col.push(line(
                    "Licence",
                    column![
                        label(licence, 13.0, theme::TEXT),
                        row![secondary("Read Meta's licence", Some(Step::ReadLicence)), mono(url, 11.5, theme::TEXT_FAINT)].spacing(10).align_y(Alignment::Center),
                        checkbox(f.licence_ticked).label("I accept the Llama 3.2 licence").on_toggle(Step::Licence).size(16).text_size(13.0),
                    ]
                    .spacing(8),
                ));
            }
            col = col.push(note(
                "Nothing is sent before you agree. At the free limit this part pauses until the provider's time; Alelyon never switches provider or pays on its own.",
            ));
            if cloud.id == CLOUDFLARE.id {
                col = col.push(note("On Cloudflare, Alelyon stops before the free allocation: it counts each call's Neurons and pauses at 9,000 of the 10,000 a day, until 00:00 UTC, whatever plan your account is on."));
                if o.part == Part::Eyes {
                    col = col.push(note("Cloudflare publishes no rate for a picture sent to this model, so each is counted as about 6,400 tokens, Alelyon's estimate."));
                }
            }
            if f.busy {
                col = col.push(working(phase, "Accepting the licence on your account"));
            }
            if o.part == Part::Eyes {
                col = col.push(note("Agreeing turns Sinai's eyes on with this model. Agreeing for other parts never turns them on."));
            }
            col = col.push(
                row![
                    secondary("Back", Some(Step::Back)),
                    space().width(Length::Fill),
                    primary("Agree and use", (!f.busy && (o.licence.is_none() || f.licence_ticked)).then_some(Step::Agree)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            );
        }
    }
    container(col).padding(16).width(Length::Fill).style(theme::panel).into()
}
