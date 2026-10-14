//! Auto configure Sinai: two dials, a proposal, one Agree, then the setup.
//!
//! **The dials** (on Sinai's page, kept in the preferences as `"sinai_auto"`):
//! - **Effort**: Light (quick: a small mind of about 3-4B on this PC), Balanced (a mid-size mind of about 8-14B on this
//!   PC, or a free cloud model), Deep (the strongest the budget allows, thinking on);
//! - **Price tier**: This PC only ($0, nothing leaves this PC), Free cloud ($0: this PC and free cloud models on the
//!   person's own accounts), Up to $5 a month, Up to $20 a month, Custom (the person's own number). A paid tier is the
//!   person's consent to pay up to its cap and no more.
//!
//! **The proposal.** "Auto configure Sinai" reads what it needs ([`gather`]: this PC's memory budget from the Foundry,
//! the free disk, each candidate's file list on Hugging Face, which is public and gives each file's size and checksum,
//! and, at a cloud tier, the providers' live model lists and prices), then shows one row per part: the model, where it
//! runs, why, the download or the price, and the expected month at typical use ([`choose::TYPICAL_USE`]). How it
//! chooses is [`choose`]; what it may choose from is the quality catalogue ([`catalogue`]). The eyes stay off at every
//! tier until the person turns them on in the proposal. Nothing is downloaded, connected or paid before the single
//! **Agree**.
//!
//! **After Agree.** The setup before it is kept (`"previous"`), and a paid row's price and the tier's cap are kept as
//! that part's paid allowance (`"paid"`). Then each model for this PC is downloaded from Hugging Face's public files,
//! one at a time (free disk checked first, each file's checksum checked after, Pause, Resume and Cancel), and once it is
//! in place its part's choice is kept. Each cloud row opens that part's own consent card (connect the provider's
//! account where it is not connected, then the card's Agree): nothing is sent for a part before its own card's Agree.
//!
//! **Spending.** A paid part's calls are counted in lattice-core's spending ledger (`hosted::spend`) from the
//! provider's reported usage; Sinai's page shows the month so far against the cap, and at the cap the paid part pauses
//! ("Monthly cap reached"). It never moves to a pricier model or another provider. A paid model is called only while
//! the tier is a paid one, its price is known, and its allowance names that very model.
//!
//! **Later.** When prices, provider lists or this PC change, the next look (once each time Sinai's page is first shown)
//! may find a different proposal; the page then suggests reviewing it. It never switches by itself. "Back to my previous
//! setup" puts back the choices and dials from before the last Agree in one write; downloaded files stay.
//!
//! The kept object:
//!
//! ```json
//! "sinai_auto": {
//!   "version": 1,
//!   "effort": "light" | "balanced" | "deep",
//!   "tier": "this-pc" | "free-cloud" | "up-to-5" | "up-to-20" | "custom",
//!   "custom_usd": 12.5,
//!   "paid": {"mind": {"provider": "openrouter", "model": "<id>", "input_per_m": 0.13, "output_per_m": 0.6, "agreed": "<UTC>"}},
//!   "agreed": "<UTC time of the last Agree>",
//!   "previous": {"parts": {...}, "sinai_model": "...", "settings": {...}, "at": "<UTC>"}
//! }
//! ```

pub mod catalogue;
pub mod choose;
pub mod view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::{Subscription, Task};
use serde_json::{Map, Value, json};

use lattice_core::fetch;
use lattice_core::hosted::parts::{self as routes, Reach};
use lattice_core::hosted::spend::{self, Price, Spend};

use self::catalogue::{Catalogue, Effort};
use self::choose::{Listed, Machine, Now, Pick, Proposal, Row, Tier};
use super::{Choice, Part, cloud, off_thread};

// ------------------------------------------------------------------- the dials

/// The price tier dial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Budget {
    ThisPc,
    FreeCloud,
    Five,
    Twenty,
    Custom,
}

impl Budget {
    pub const ALL: [Budget; 5] = [Budget::ThisPc, Budget::FreeCloud, Budget::Five, Budget::Twenty, Budget::Custom];

    pub fn key(self) -> &'static str {
        match self {
            Budget::ThisPc => "this-pc",
            Budget::FreeCloud => "free-cloud",
            Budget::Five => "up-to-5",
            Budget::Twenty => "up-to-20",
            Budget::Custom => "custom",
        }
    }

    pub fn of(key: &str) -> Option<Budget> {
        Budget::ALL.into_iter().find(|b| b.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Budget::ThisPc => "This PC only",
            Budget::FreeCloud => "Free cloud",
            Budget::Five => "Up to $5 a month",
            Budget::Twenty => "Up to $20 a month",
            Budget::Custom => "Custom",
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            Budget::ThisPc => "$0. Nothing leaves this PC.",
            Budget::FreeCloud => "$0. This PC, and free cloud models on your own accounts.",
            Budget::Five | Budget::Twenty => "You agree to pay up to this cap a month for paid models, and no more.",
            Budget::Custom => "Your own monthly cap: you agree to pay up to it for paid models, and no more.",
        }
    }
}

impl Effort {
    pub fn title(self) -> &'static str {
        match self {
            Effort::Light => "Light",
            Effort::Balanced => "Balanced",
            Effort::Deep => "Deep",
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            Effort::Light => "Quick: a small mind (about 3-4B) on this PC.",
            Effort::Balanced => "A mid-size mind (about 8-14B) on this PC, or a free cloud model.",
            Effort::Deep => "The strongest the budget allows, thinking on.",
        }
    }
}

/// The largest custom cap the dial takes, in US dollars a month.
pub const CUSTOM_MAX: f64 = 1_000.0;

/// A custom cap as typed: a positive number of dollars, at most [`CUSTOM_MAX`].
pub fn custom_cap(text: &str) -> Option<f64> {
    let n: f64 = text.trim().trim_start_matches('$').trim().parse().ok()?;
    (n.is_finite() && n > 0.0 && n <= CUSTOM_MAX).then_some((n * 100.0).round() / 100.0)
}

/// A paid model agreed for a part: its provider, the model's id, its price when agreed, and when.
#[derive(Clone, Debug, PartialEq)]
pub struct PaidTerms {
    pub provider: String,
    pub model: String,
    pub price: Price,
    pub agreed: String,
}

/// The setup before the last Agree, as it was kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Previous {
    pub parts: Option<Value>,
    pub sinai_model: Option<String>,
    /// Auto configure's own settings then (without their own `"previous"`).
    pub settings: Map<String, Value>,
    pub at: String,
}

/// Auto configure's settings, as the preferences keep them.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub effort: Effort,
    pub budget: Budget,
    pub custom_usd: Option<f64>,
    pub paid: HashMap<Part, PaidTerms>,
    pub agreed: Option<String>,
    pub previous: Option<Previous>,
}

impl Default for Settings {
    fn default() -> Settings {
        // the most private tier until the person turns the dial
        Settings { effort: Effort::Balanced, budget: Budget::ThisPc, custom_usd: None, paid: HashMap::new(), agreed: None, previous: None }
    }
}

fn part_of(key: &str) -> Option<Part> {
    Part::ALL.into_iter().find(|p| p.key() == key)
}

impl Settings {
    pub fn from_map(m: &Map<String, Value>) -> Settings {
        let d = Settings::default();
        let text = |k: &str| m.get(k).and_then(Value::as_str);
        let paid = m
            .get("paid")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| {
                let price = Price::known(v.get("input_per_m")?.as_f64()?, v.get("output_per_m")?.as_f64()?)?;
                Some((
                    part_of(k)?,
                    PaidTerms {
                        provider: v.get("provider")?.as_str()?.to_string(),
                        model: v.get("model")?.as_str()?.to_string(),
                        price,
                        agreed: v.get("agreed")?.as_str()?.to_string(),
                    },
                ))
            })
            .collect();
        let previous = m.get("previous").and_then(Value::as_object).map(|p| Previous {
            parts: p.get("parts").filter(|v| v.is_object()).cloned(),
            sinai_model: p.get("sinai_model").and_then(Value::as_str).map(str::to_owned),
            settings: p.get("settings").and_then(Value::as_object).cloned().unwrap_or_default(),
            at: p.get("at").and_then(Value::as_str).unwrap_or_default().to_string(),
        });
        Settings {
            effort: text("effort").and_then(Effort::of).unwrap_or(d.effort),
            budget: text("tier").and_then(Budget::of).unwrap_or(d.budget),
            custom_usd: m.get("custom_usd").and_then(Value::as_f64).filter(|n| n.is_finite() && *n > 0.0 && *n <= CUSTOM_MAX),
            paid,
            agreed: text("agreed").map(str::to_owned),
            previous,
        }
    }

    /// As kept, without `"previous"` (the caller adds it where it belongs).
    pub fn to_map_own(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("version".into(), json!(1));
        m.insert("effort".into(), json!(self.effort.key()));
        m.insert("tier".into(), json!(self.budget.key()));
        if let Some(n) = self.custom_usd {
            m.insert("custom_usd".into(), json!(n));
        }
        if !self.paid.is_empty() {
            let paid: Map<String, Value> = self
                .paid
                .iter()
                .map(|(part, t)| {
                    (
                        part.key().to_string(),
                        json!({"provider": t.provider, "model": t.model, "input_per_m": t.price.input_per_m, "output_per_m": t.price.output_per_m, "agreed": t.agreed}),
                    )
                })
                .collect();
            m.insert("paid".into(), Value::Object(paid));
        }
        if let Some(at) = &self.agreed {
            m.insert("agreed".into(), json!(at));
        }
        m
    }

    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = self.to_map_own();
        if let Some(p) = &self.previous {
            let mut prev = Map::new();
            if let Some(parts) = &p.parts {
                prev.insert("parts".into(), parts.clone());
            }
            if let Some(s) = &p.sinai_model {
                prev.insert("sinai_model".into(), json!(s));
            }
            prev.insert("settings".into(), Value::Object(p.settings.clone()));
            prev.insert("at".into(), json!(p.at));
            m.insert("previous".into(), Value::Object(prev));
        }
        m
    }

    /// The tier as the chooser takes it. A custom tier without a number allows nothing paid.
    pub fn tier(&self) -> Tier {
        match self.budget {
            Budget::ThisPc => Tier::ThisPc,
            Budget::FreeCloud => Tier::FreeCloud,
            Budget::Five => Tier::Cap(5.0),
            Budget::Twenty => Tier::Cap(20.0),
            Budget::Custom => Tier::Cap(self.custom_usd.unwrap_or(0.0)),
        }
    }

    pub fn cap(&self) -> Option<f64> {
        self.tier().cap()
    }

    /// The paid allowance for `model` of `provider` on `part`, with the cap it is held to: only at a paid tier, and
    /// only for the very model agreed.
    pub fn paid_for(&self, part: Part, provider: &str, model: &str) -> Option<(&PaidTerms, f64)> {
        let cap = self.cap()?;
        self.paid.get(&part).filter(|t| t.provider == provider && t.model == model).map(|t| (t, cap))
    }

    /// As kept at `at` (the defaults without a file).
    pub fn read(at: Option<&Path>) -> Settings {
        at.map(|p| Settings::from_map(&crate::prefs::load_auto(p))).unwrap_or_default()
    }
}

// ------------------------------------------------------------------- spending

#[cfg(test)]
thread_local! {
    static TEST_LEDGER: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    static TEST_PREFS: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The spending ledger and the preferences this test thread uses (None: none).
#[cfg(test)]
pub fn test_places(ledger: Option<PathBuf>, prefs: Option<PathBuf>) {
    TEST_LEDGER.with(|l| *l.borrow_mut() = ledger);
    TEST_PREFS.with(|p| *p.borrow_mut() = prefs);
}

/// The paid spending ledger (lattice-core's own file, which every paid call counts in).
pub fn ledger() -> Option<Spend> {
    #[cfg(test)]
    {
        TEST_LEDGER.with(|l| l.borrow().clone()).map(Spend::at)
    }
    #[cfg(not(test))]
    {
        spend::default_path().map(Spend::at)
    }
}

fn prefs_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_PREFS.with(|p| p.borrow().clone())
    }
    #[cfg(not(test))]
    {
        crate::prefs::path()
    }
}

/// The settings kept now.
pub fn settings_now() -> Settings {
    Settings::read(prefs_path().as_deref())
}

/// `reach` allowed to call the paid `model` for `part` when the settings kept now allow it (a paid tier, an allowance
/// for that very model); else `reach` as it is, and the route refuses a model that is not free.
pub fn paid_reach(reach: Reach, part: Part, model: &str) -> Reach {
    if routes::free_model(reach.cloud, model) {
        return reach;
    }
    let s = settings_now();
    match (s.paid_for(part, reach.cloud.id, model), ledger()) {
        (Some((t, cap)), Some(l)) => reach.with_paid(routes::Paid {
            model: t.model.clone(),
            part: part.key().to_string(),
            price: t.price,
            cap_usd: cap,
            ledger: Arc::new(l),
        }),
        _ => reach,
    }
}

/// While a paid model is `part`'s choice and the month has reached the cap: the words its card shows.
pub fn capped_words(part: Part, provider: &str, model: &str, now: i64) -> Option<String> {
    let s = settings_now();
    let cap = match s.paid_for(part, provider, model) {
        Some((_, cap)) => cap,
        None if !routes::cloud(provider).is_some_and(|c| routes::free_model(c, model)) => {
            return Some("A paid model needs a paid price tier: choose one with Auto configure, or a free model.".into());
        }
        None => return None,
    };
    ledger().filter(|l| l.capped(cap, now)).map(|_| {
        format!("Monthly cap reached (${cap:.2}): this part pauses until {} or until you raise the cap.", crate::utc::day_stamp(spend::next_month_start(now) as f64))
    })
}

/// The month's paid spending so far.
pub fn month_so_far(now: i64) -> f64 {
    ledger().map(|l| l.used(now)).unwrap_or(0.0)
}

// ------------------------------------------------------------------- what the proposal reads

/// Everything a proposal is made from that is not the dials.
#[derive(Clone, Debug, Default)]
pub struct Gathered {
    pub machine: Machine,
    pub sizes: HashMap<String, Result<u64, String>>,
    /// Each local candidate's Hub listing, for its download plan.
    pub listings: HashMap<String, Vec<fetch::Listed>>,
    pub lists: HashMap<String, Listed>,
    pub connected: HashSet<String>,
    /// The cloud lists were read for a cloud tier.
    pub cloud_read: bool,
}

/// Where each part's local model is kept.
pub fn folder_for(part: Part) -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    match part {
        Part::Ears => super::speech_dir(),
        Part::Voice => super::voice_dir(),
        _ => Some(lattice_core::llama::files::LlamaPaths::from_env(&lattice_core::ProcessEnv).models_dir),
    }
}

/// Free space on the disk of `dir` (or its nearest folder that is there), in bytes; None when not known.
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut at = dir.to_path_buf();
    while !at.exists() {
        at = at.parent()?.to_path_buf();
    }
    free_space_at(&at)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn free_space_at(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is a NUL-terminated path that outlives the call; `free` is a valid u64 to write.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

#[cfg(not(windows))]
fn free_space_at(_: &Path) -> Option<u64> {
    None
}

/// This PC's memory budget from a Foundry measurement: the budget its fit verdicts use, else the accelerator's memory
/// less the serving reserve; None (UNMEASURED) otherwise.
pub fn machine_of(m: &crate::lattice::foundry::Measured, disk_free: Option<u64>) -> Machine {
    if let Ok(f) = &m.foundry {
        if let Some(fit) = f.runnable.first().map(|r| &r.fit)
            && let Some(b) = fit.budget_bytes.filter(|b| *b > 0)
        {
            return Machine { budget: Some(b as u64), basis: format!("the Foundry's budget: {} [{}]", fit.budget_basis, fit.budget_provenance), disk_free };
        }
        if let Some(total) = m.reading.gpu.as_ref().and_then(|g| g.total_bytes).filter(|t| *t > 0) {
            let keep = (1.0 - f.reserve.fraction).clamp(0.0, 1.0);
            return Machine {
                budget: Some((total as f64 * keep) as u64),
                basis: format!("the accelerator's memory less the serving reserve of {:.0}% [DERIVED]", f.reserve.fraction * 100.0),
                disk_free,
            };
        }
    }
    Machine { budget: None, basis: "UNMEASURED: the Foundry did not measure an accelerator's memory".into(), disk_free }
}

/// Read what a proposal needs (blocking: run it off the window's thread). Reads only public lists and this PC; sends
/// nothing of the person's. The cloud lists are read only for a cloud tier.
pub fn gather(cat: &Catalogue, cloud_tier: bool) -> Gathered {
    let mut g = Gathered { cloud_read: cloud_tier, ..Gathered::default() };
    let disk = folder_for(Part::Mind).and_then(|d| free_space(&d));
    g.machine = match crate::lattice::foundry::measure(4096) {
        Ok(m) => machine_of(&m, disk),
        Err(why) => Machine { budget: None, basis: format!("UNMEASURED: {why}"), disk_free: disk },
    };
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return g };
    let source = fetch::Source::hub();
    if let Ok(client) = fetch::client(&source) {
        for c in cat.candidates.iter() {
            let Some(l) = &c.local else { continue };
            let read = rt.block_on(fetch::list(&client, &source, &l.repo, &l.revision));
            let size = read.as_ref().map_err(Clone::clone).and_then(|listing| {
                l.files
                    .iter()
                    .map(|f| {
                        let row = listing.iter().find(|x| x.path == f.path).ok_or_else(|| format!("{} is not in its Hugging Face listing", f.path))?;
                        row.sha256.as_ref().ok_or_else(|| format!("{} has no checksum in its listing", f.path))?;
                        Ok(row.size)
                    })
                    .sum::<Result<u64, String>>()
            });
            g.sizes.insert(c.id.clone(), size);
            if let Ok(listing) = read {
                g.listings.insert(c.id.clone(), listing);
            }
        }
    }
    for c in routes::CLOUDS {
        if cloud::connected(c) {
            g.connected.insert(c.id.to_string());
        }
    }
    if !cloud_tier {
        return g;
    }
    // OpenRouter's list is public: read without a key, for its free variants and its prices
    let openrouter = lattice_core::hosted::Web::new().ok().map(|web| {
        use lattice_core::hosted::Http;
        rt.block_on(web.get(routes::OPENROUTER.list.to_string(), None))
    });
    let value = match openrouter {
        Some(Ok((200, body))) => serde_json::from_slice::<Value>(&body).map_err(|_| "OpenRouter's list could not be read".to_string()),
        Some(Ok((status, _))) => Err(format!("OpenRouter answered {status}")),
        Some(Err(why)) => Err(why),
        None => Err("OpenRouter could not be reached".into()),
    };
    for part in Part::ALL {
        let key = format!("openrouter/{}", part.key());
        let listed = match &value {
            Ok(v) => Listed::Read(routes::priced(&routes::OPENROUTER, cloud::task(part), v).into_iter().map(|(m, p)| (m.id, p)).collect()),
            Err(why) => Listed::NotRead(why.clone()),
        };
        g.lists.insert(key, listed);
    }
    // the other providers list with the person's key: read only where connected
    for c in [&routes::GROQ, &routes::CLOUDFLARE] {
        for part in Part::ALL {
            if cloud::offer(part, c.id).is_none() {
                continue;
            }
            let key = format!("{}/{}", c.id, part.key());
            let listed = if !g.connected.contains(c.id) {
                Listed::NotRead(format!("{} is not connected yet", c.label))
            } else {
                match cloud::reach(c).and_then(|r| routes::Client::new(&r).map(|cl| (r, cl))) {
                    Ok((r, client)) => match rt.block_on(routes::models(&client, &r, cloud::task(part), cloud::now())) {
                        Ok(models) => Listed::Read(models.into_iter().map(|m| (m.id, Price { input_per_m: 0.0, output_per_m: 0.0 })).collect()),
                        Err(f) => Listed::NotRead(f.sentence()),
                    },
                    Err(why) => Listed::NotRead(why),
                }
            };
            g.lists.insert(key, listed);
        }
    }
    g
}

/// How the chooser sees `part`'s choice in effect: open when there is none or the catalogue ranks it; the words for
/// it when the catalogue cannot rank it (Sinai's own mind, a model of the person's own).
pub fn now_of(cat: &Catalogue, part: Part, current: Option<&Choice>, label: &str) -> Now {
    let ranked = |c: &Choice| {
        cat.of(part).any(|cand| match c {
            Choice::Gguf(stem) => cand.local.as_ref().and_then(|l| l.model()).is_some_and(|f| f.name().strip_suffix(".gguf") == Some(stem.as_str())),
            Choice::SpeechFile(file) | Choice::VoiceFile(file) => cand.local.as_ref().and_then(|l| l.model()).is_some_and(|f| f.name() == file),
            Choice::Cloud { provider, model, .. } => cand.cloud.iter().any(|m| &m.provider == provider && (&m.model == model || model.strip_suffix(":free") == Some(m.model.as_str()))),
            _ => false,
        })
    };
    match current {
        None => Now::Open,
        Some(c) if ranked(c) => Now::Open,
        Some(_) => Now::Unranked(label.to_string()),
    }
}

// ------------------------------------------------------------------- the setup

/// Where one row of an agreed proposal is.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Nothing to do: kept, off, or nothing fit.
    Nothing,
    /// Waiting for its download's turn.
    Waiting,
    Fetching(fetch::Progress),
    Paused,
    /// Its files are in place and its choice is kept.
    Done,
    Cancelled,
    Failed(String),
    /// A cloud row: its part's own consent card is next.
    Card,
    /// Its part's card was agreed.
    Agreed,
}

#[derive(Clone, Debug)]
pub struct Setup {
    pub rows: Vec<(Row, Step, Option<fetch::Plan>)>,
    control: Arc<fetch::Control>,
    /// The row downloading now.
    current: Option<usize>,
}

impl Setup {
    pub fn downloading(&self) -> bool {
        self.current.is_some()
    }

    pub fn paused(&self) -> bool {
        self.rows.iter().any(|(_, s, _)| *s == Step::Paused)
    }
}

/// A download's news.
#[derive(Clone, Debug)]
pub enum Fetched {
    Progress(fetch::Progress),
    Ended(Result<fetch::Outcome, String>),
}

#[derive(Clone, Debug)]
pub enum Msg {
    Effort(Effort),
    Budget(Budget),
    CustomTyped(String),
    Saved(Result<(), String>),
    /// "Auto configure Sinai": read, then propose.
    Propose,
    Gathered(Arc<Gathered>),
    EyesOn(bool),
    Agree,
    Close,
    /// The previous setup and the paid allowances were kept: the setup starts.
    Begun(Result<(), String>),
    Fetch(usize, Fetched),
    Pause,
    Resume,
    Cancel,
    /// A row's choice was kept (or why not).
    Kept(usize, Result<(), String>),
    /// Open a cloud row's own consent card.
    Card(usize),
    /// Put back the setup from before the last Agree.
    Back,
    BackDone(Result<(), String>),
    /// Sinai's page is on show: read the month's spending, and look once for a better setup.
    Look,
    Month(f64),
    Suggested(Arc<Gathered>),
    /// Open the suggested proposal.
    Review,
}

/// What the window does after a message.
pub enum Then {
    Nothing,
    Task(Task<Msg>),
    /// Choices were kept: the window reads them again and applies the mind's.
    Chosen(Vec<(Part, Option<Choice>)>),
    /// Open `part`'s consent card for this model on this provider.
    Card(Part, String, String),
}

/// What the window knows that Auto configure needs: each part's choice in effect, and its words.
pub struct Ctx {
    pub current: HashMap<Part, (Option<Choice>, String)>,
}

pub struct State {
    pub open: bool,
    pub settings: Settings,
    pub custom_text: String,
    pub gathering: bool,
    pub gathered: Option<Arc<Gathered>>,
    pub proposal: Option<Proposal>,
    pub eyes_on: bool,
    pub setup: Option<Setup>,
    pub said: Option<Result<String, String>>,
    pub month: Option<f64>,
    /// A proposal that differs from what is in effect, found by a later look.
    pub suggestion: Option<Proposal>,
    looked: bool,
    catalogue: Result<Arc<Catalogue>, Vec<String>>,
    prefs: Option<PathBuf>,
}

impl Default for State {
    fn default() -> State {
        let prefs = prefs_path();
        let settings = Settings::read(prefs.as_deref());
        State::with(prefs, settings)
    }
}

impl State {
    pub fn with(prefs: Option<PathBuf>, settings: Settings) -> State {
        State {
            open: false,
            custom_text: settings.custom_usd.map(|n| format!("{n}")).unwrap_or_default(),
            settings,
            gathering: false,
            gathered: None,
            proposal: None,
            eyes_on: false,
            setup: None,
            said: None,
            month: None,
            suggestion: None,
            looked: false,
            catalogue: catalogue::shipped().map(Arc::new),
            prefs,
        }
    }

    pub fn catalogue(&self) -> Result<&Catalogue, &[String]> {
        self.catalogue.as_deref().map_err(|e| e.as_slice())
    }

    fn save_settings(&self) -> Then {
        let Some(at) = self.prefs.clone() else { return Then::Nothing };
        let map = self.settings.to_map();
        Then::Task(Task::perform(off_thread(move || crate::prefs::save_auto(&at, map)), |r| {
            Msg::Saved(r.unwrap_or_else(|| Err("the settings were not kept: their thread stopped".into())))
        }))
    }

    /// The proposal for what was gathered, with the dials as they are.
    pub fn propose_now(&self, g: &Gathered, ctx: &Ctx) -> Option<Proposal> {
        let cat = self.catalogue().ok()?;
        let now = Part::ALL
            .into_iter()
            .map(|p| {
                let (c, words) = ctx.current.get(&p).cloned().unwrap_or((None, String::new()));
                (p, now_of(cat, p, c.as_ref(), &words))
            })
            .collect();
        let inputs = choose::Inputs {
            catalogue: cat,
            effort: self.settings.effort,
            tier: self.settings.tier(),
            machine: g.machine.clone(),
            sizes: g.sizes.clone(),
            lists: g.lists.clone(),
            connected: g.connected.clone(),
            eyes_on: self.eyes_on,
            now,
        };
        Some(choose::propose(&inputs))
    }

    fn gather_task(&self, then: fn(Arc<Gathered>) -> Msg) -> Task<Msg> {
        let Ok(cat) = self.catalogue.clone() else { return Task::none() };
        let cloud_tier = self.settings.tier().cloud();
        Task::perform(off_thread(move || gather(&cat, cloud_tier)), move |g| then(Arc::new(g.unwrap_or_default())))
    }

    pub fn update(&mut self, msg: Msg, ctx: &Ctx) -> Then {
        match msg {
            Msg::Effort(e) => {
                self.settings.effort = e;
                self.repropose(ctx);
                return self.save_settings();
            }
            Msg::Budget(b) => {
                self.settings.budget = b;
                // a cloud tier needs the cloud lists: read again when they were not read
                if self.settings.tier().cloud() && self.gathered.as_ref().is_some_and(|g| !g.cloud_read) && self.open {
                    self.gathered = None;
                    self.proposal = None;
                    self.gathering = true;
                    let save = self.save_settings();
                    let read = self.gather_task(Msg::Gathered);
                    return match save {
                        Then::Task(t) => Then::Task(Task::batch([t, read])),
                        _ => Then::Task(read),
                    };
                }
                self.repropose(ctx);
                return self.save_settings();
            }
            Msg::CustomTyped(text) => {
                self.custom_text = text;
                self.settings.custom_usd = custom_cap(&self.custom_text);
                self.repropose(ctx);
                return self.save_settings();
            }
            Msg::Saved(r) => {
                if let Err(why) = r {
                    self.said = Some(Err(format!("The dials were not kept: {why}")));
                }
            }
            Msg::Propose => {
                self.open = true;
                self.said = None;
                self.suggestion = None;
                if self.setup.is_some() {
                    return Then::Nothing;
                }
                if let Err(wrong) = &self.catalogue {
                    self.said = Some(Err(format!("The quality catalogue did not load: {}", wrong.join("; "))));
                    return Then::Nothing;
                }
                self.gathering = true;
                self.proposal = None;
                return Then::Task(self.gather_task(Msg::Gathered));
            }
            Msg::Gathered(g) => {
                self.gathering = false;
                self.proposal = self.propose_now(&g, ctx);
                self.gathered = Some(g);
            }
            Msg::EyesOn(on) => {
                self.eyes_on = on;
                self.repropose(ctx);
            }
            Msg::Close => {
                self.open = false;
                if self.setup.as_ref().is_none_or(|s| !s.downloading()) {
                    self.proposal = None;
                }
            }
            Msg::Agree => return self.agree(ctx),
            Msg::Begun(r) => match r {
                Ok(()) => return self.next(),
                Err(why) => {
                    self.setup = None;
                    self.said = Some(Err(format!("Nothing was set up: {why}")));
                }
            },
            Msg::Fetch(i, Fetched::Progress(p)) => {
                if let Some(s) = &mut self.setup
                    && let Some(row) = s.rows.get_mut(i)
                {
                    row.1 = Step::Fetching(p);
                }
            }
            Msg::Fetch(i, Fetched::Ended(r)) => return self.fetched(i, r),
            Msg::Pause => {
                if let Some(s) = &self.setup {
                    s.control.pause();
                }
            }
            Msg::Resume => {
                if let Some(s) = &mut self.setup {
                    s.control.resume();
                    for row in &mut s.rows {
                        if row.1 == Step::Paused {
                            row.1 = Step::Waiting;
                        }
                    }
                }
                return self.next();
            }
            Msg::Cancel => {
                if let Some(s) = &mut self.setup {
                    s.control.cancel();
                    if s.current.is_none() {
                        // nothing running to see it: the waiting and paused rows are cancelled here, their partial files removed
                        for (_, step, plan) in &mut s.rows {
                            if matches!(step, Step::Waiting | Step::Paused) {
                                if let Some(plan) = plan {
                                    fetch::discard_partials(plan);
                                }
                                *step = Step::Cancelled;
                            }
                        }
                    }
                }
            }
            Msg::Kept(i, r) => {
                let Some(s) = &mut self.setup else { return Then::Nothing };
                let Some(row) = s.rows.get_mut(i) else { return Then::Nothing };
                return match r {
                    Ok(()) => {
                        row.1 = Step::Done;
                        let chosen = row.0.parts.iter().map(|p| (*p, row.0.pick.choice(*p))).collect();
                        Then::Chosen(chosen)
                    }
                    Err(why) => {
                        row.1 = Step::Failed(format!("Downloaded, but its choice was not kept: {why}"));
                        Then::Nothing
                    }
                };
            }
            Msg::Card(i) => {
                let Some(s) = &self.setup else { return Then::Nothing };
                if let Some((row, Step::Card, _)) = s.rows.get(i)
                    && let Pick::Cloud { provider, model, .. } = &row.pick
                {
                    self.open = false;
                    return Then::Card(row.parts[0], provider.clone(), model.clone());
                }
            }
            Msg::Back => return self.back(),
            Msg::BackDone(r) => match r {
                Ok(()) => {
                    self.settings = Settings::read(self.prefs.as_deref());
                    self.custom_text = self.settings.custom_usd.map(|n| format!("{n}")).unwrap_or_default();
                    self.suggestion = None;
                    self.said = Some(Ok("Your previous setup is back. Downloaded files stay where they are.".into()));
                    let restored = Part::ALL.into_iter().map(|p| (p, None)).collect();
                    return Then::Chosen(restored);
                }
                Err(why) => self.said = Some(Err(format!("The previous setup was not put back: {why}"))),
            },
            Msg::Look => {
                let month = Task::perform(off_thread(|| month_so_far(cloud::now())), |m| Msg::Month(m.unwrap_or(0.0)));
                // one look a session for a better setup, after an agreed one, never while one is under way
                if self.looked || self.settings.agreed.is_none() || self.setup.is_some() || self.catalogue.is_err() {
                    return Then::Task(month);
                }
                self.looked = true;
                return Then::Task(Task::batch([month, self.gather_task(Msg::Suggested)]));
            }
            Msg::Month(m) => self.month = Some(m),
            Msg::Suggested(g) => {
                if let Some(p) = self.propose_now(&g, ctx) {
                    let current = |part: Part| ctx.current.get(&part).and_then(|(c, _)| c.clone());
                    if choose::differs(&p, &current) {
                        self.suggestion = Some(p);
                        self.gathered = Some(g);
                    }
                }
            }
            Msg::Review => {
                if let Some(p) = self.suggestion.take() {
                    self.proposal = Some(p);
                    self.open = true;
                    self.said = None;
                }
            }
        }
        Then::Nothing
    }

    /// A choice was kept in a part's picker (its consent card's Agree): the matching cloud row is agreed.
    pub fn chosen(&mut self, part: Part, choice: &Option<Choice>) {
        let Some(s) = &mut self.setup else { return };
        for (row, step, _) in &mut s.rows {
            if *step == Step::Card
                && row.parts.contains(&part)
                && let (Pick::Cloud { provider, model, .. }, Some(Choice::Cloud { provider: p, model: m, agreed: Some(_) })) = (&row.pick, choice)
                && provider == p
                && model == m
            {
                *step = Step::Agreed;
            }
        }
    }

    fn repropose(&mut self, ctx: &Ctx) {
        if let (Some(g), true) = (self.gathered.clone(), self.setup.is_none() && self.open) {
            self.proposal = self.propose_now(&g, ctx);
        }
    }

    /// The single Agree: keep the setup before it and any paid allowance, then begin.
    fn agree(&mut self, ctx: &Ctx) -> Then {
        let (Some(p), Some(g)) = (self.proposal.clone(), self.gathered.clone()) else { return Then::Nothing };
        if self.setup.is_some() {
            return Then::Nothing;
        }
        let Some(at) = self.prefs.clone() else {
            self.said = Some(Err("This window keeps no preferences, so nothing was set up.".into()));
            return Then::Nothing;
        };
        let stamp = cloud::stamp(cloud::now());
        // plans for the downloads, each checked against the free disk of its folder
        let mut rows = Vec::new();
        for row in &p.rows {
            let step = match &row.pick {
                Pick::Local { id, repo, revision, files, .. } => {
                    let Some(dir) = row.parts.first().and_then(|part| folder_for(*part)) else {
                        self.said = Some(Err("The folder for this model is not known, so nothing was set up.".into()));
                        return Then::Nothing;
                    };
                    let wanted: Vec<(String, String)> = files.iter().map(|f| (f.path.clone(), f.name().to_string())).collect();
                    let listing = g.listings.get(id).cloned().unwrap_or_default();
                    match fetch::plan(&fetch::Source::hub(), repo, revision, &wanted, &listing, &dir) {
                        Ok(plan) => {
                            if let Err(why) = plan.check_disk(free_space(&dir)) {
                                self.said = Some(Err(why));
                                return Then::Nothing;
                            }
                            rows.push((row.clone(), Step::Waiting, Some(plan)));
                            continue;
                        }
                        Err(why) => Step::Failed(why),
                    }
                }
                Pick::Cloud { .. } => Step::Card,
                _ => Step::Nothing,
            };
            rows.push((row.clone(), step, None));
        }
        // the setup before, and the paid allowances, kept in one write before anything starts
        let before = Settings::read(Some(&at));
        let (parts, sinai_model) = crate::prefs::load_parts_whole(&at);
        self.settings = agreed_settings(&before, &self.settings, &p, Previous { parts, sinai_model, settings: before.to_map_own(), at: stamp.clone() }, &stamp);
        self.setup = Some(Setup { rows, control: Arc::new(fetch::Control::new()), current: None });
        self.said = None;
        let _ = ctx;
        let map = self.settings.to_map();
        Then::Task(Task::perform(off_thread(move || crate::prefs::save_auto(&at, map)), |r| {
            Msg::Begun(r.unwrap_or_else(|| Err("its thread stopped".into())))
        }))
    }

    /// Start the next waiting download, one at a time.
    fn next(&mut self) -> Then {
        let Some(s) = &mut self.setup else { return Then::Nothing };
        if s.current.is_some() {
            return Then::Nothing;
        }
        let Some(i) = s.rows.iter().position(|(_, step, _)| *step == Step::Waiting) else { return Then::Nothing };
        let Some(plan) = s.rows[i].2.clone() else { return Then::Nothing };
        s.current = Some(i);
        let control = s.control.clone();
        let (tx, rx) = futures::channel::mpsc::unbounded::<Fetched>();
        let _ = std::thread::Builder::new().name("centcom-auto-fetch".into()).spawn(move || {
            let ended = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt.block_on(async {
                    let client = fetch::client(&fetch::Source::hub())?;
                    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
                    let tx2 = tx.clone();
                    let mut progress = move |p: fetch::Progress| {
                        // a few times a second is enough for the window
                        if p.checking || last.elapsed() >= std::time::Duration::from_millis(250) {
                            last = std::time::Instant::now();
                            let _ = tx2.unbounded_send(Fetched::Progress(p));
                        }
                    };
                    fetch::download(&client, &plan, &control, &mut progress).await
                }),
                Err(_) => Err("The download could not start.".to_string()),
            };
            let _ = tx.unbounded_send(Fetched::Ended(ended));
        });
        Then::Task(Task::run(rx, move |f| Msg::Fetch(i, f)))
    }

    fn fetched(&mut self, i: usize, r: Result<fetch::Outcome, String>) -> Then {
        let Some(s) = &mut self.setup else { return Then::Nothing };
        s.current = None;
        let Some(row) = s.rows.get_mut(i) else { return Then::Nothing };
        match r {
            Ok(fetch::Outcome::Done) => {
                row.1 = Step::Done;
                let Some(at) = self.prefs.clone() else { return Then::Nothing };
                let choices: Vec<(Part, Option<Choice>)> = row.0.parts.iter().map(|p| (*p, row.0.pick.choice(*p))).collect();
                let save = Task::perform(
                    off_thread(move || choices.iter().try_for_each(|(p, c)| super::save(&at, *p, c.as_ref()))),
                    move |r| Msg::Kept(i, r.unwrap_or_else(|| Err("its thread stopped".into()))),
                );
                return match self.next() {
                    Then::Task(t) => Then::Task(Task::batch([save, t])),
                    _ => Then::Task(save),
                };
            }
            Ok(fetch::Outcome::Paused) => row.1 = Step::Paused,
            Ok(fetch::Outcome::Cancelled) => {
                for (_, step, _) in &mut s.rows {
                    if matches!(step, Step::Waiting | Step::Fetching(_) | Step::Paused) {
                        *step = Step::Cancelled;
                    }
                }
            }
            Err(why) => {
                row.1 = Step::Failed(why);
                return self.next();
            }
        }
        Then::Nothing
    }

    /// Put back the setup from before the last Agree, in one write.
    fn back(&mut self) -> Then {
        let (Some(at), Some(prev)) = (self.prefs.clone(), self.settings.previous.clone()) else { return Then::Nothing };
        if self.setup.as_ref().is_some_and(Setup::downloading) {
            self.said = Some(Err("Cancel or finish the downloads first.".into()));
            return Then::Nothing;
        }
        self.setup = None;
        self.proposal = None;
        Then::Task(Task::perform(off_thread(move || crate::prefs::restore_parts(&at, prev.parts, prev.sinai_model, prev.settings)), |r| {
            Msg::BackDone(r.unwrap_or_else(|| Err("its thread stopped".into())))
        }))
    }

    /// While Sinai's page is on show: the month's spending read again each minute.
    pub fn subscription(&self, on_page: bool) -> Subscription<Msg> {
        if on_page && self.settings.cap().is_some() {
            iced::time::every(std::time::Duration::from_secs(60)).map(|_| Msg::Look)
        } else {
            Subscription::none()
        }
    }
}

/// The settings an Agree keeps: the dials as set, a paid allowance for each paid row (its price and when; the cap is
/// the tier's), no allowance for anything else, when it was agreed, and the setup before it.
pub fn agreed_settings(before: &Settings, dials: &Settings, p: &Proposal, previous: Previous, stamp: &str) -> Settings {
    let mut next = Settings { effort: dials.effort, budget: dials.budget, custom_usd: dials.custom_usd, ..before.clone() };
    next.paid.clear();
    for row in &p.rows {
        if let Pick::Cloud { provider, model, price, .. } = &row.pick
            && !price.is_free()
        {
            for part in &row.parts {
                next.paid.insert(*part, PaidTerms { provider: provider.clone(), model: model.clone(), price: *price, agreed: stamp.to_string() });
            }
        }
    }
    next.agreed = Some(stamp.to_string());
    next.previous = Some(previous);
    next
}

/// A finished setup's last words for its rows.
pub fn setup_done(s: &Setup) -> bool {
    s.rows.iter().all(|(_, step, _)| matches!(step, Step::Nothing | Step::Done | Step::Agreed | Step::Cancelled | Step::Failed(_)))
}

#[cfg(test)]
mod tests;
