//! How Auto configure chooses, part by part. Pure: everything it reads is given ([`Inputs`]), so the same inputs give
//! the same proposal.
//!
//! For each part, in the order mind, ears, embeddings, eyes, voice:
//! 1. **The eyes stay off** unless the person turned them on for this proposal; **a choice the catalogue cannot rank**
//!    (Sinai's own mind, a model of the person's own) is kept, never replaced by a guess.
//! 2. **Candidates** are the catalogue's for the part that suit the effort level ([`Effort`]); when none of them can
//!    run, the part's other candidates, and the row says so.
//! 3. **What may run**: on this PC, a candidate whose files (their sizes from the Hub's listing), with a fifth more for
//!    its context and runtime, fit what is left of the Foundry's memory budget after the parts chosen before it (a
//!    budget that is not measured lets nothing run here); in the cloud (not at "This PC only"), a provider the part
//!    offers, with the model on its live list: free (an OpenRouter `:free` variant, or another provider's free plan),
//!    or, at a paid tier, a model whose price the list carries and whose expected month fits what is left of the cap.
//!    A price that is not known is never offered for a paid tier.
//! 4. **Best for the effort level**: the highest rank. This PC is preferred when close: the cloud is chosen only when
//!    its best is more than one rank above this PC's best. Ties: this PC beats the cloud, open weights beat closed,
//!    free beats paid, then the cheaper month.
//!
//! A model chosen for several parts is one row covering them ([`merge`]): the one-model switch is a later step.

use std::collections::{HashMap, HashSet};

use lattice_core::hosted::spend::Price;

use super::catalogue::{Candidate, Catalogue, Effort, LocalFile};
use crate::sinai_parts::{Choice, Part, cloud};

/// The price tier dial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tier {
    /// $0: nothing leaves this PC.
    ThisPc,
    /// $0: this PC and free cloud models on the person's own accounts.
    FreeCloud,
    /// Paid models allowed up to this many US dollars a month (and no more).
    Cap(f64),
}

impl Tier {
    pub fn cloud(self) -> bool {
        !matches!(self, Tier::ThisPc)
    }

    /// The monthly cap, at a paid tier.
    pub fn cap(self) -> Option<f64> {
        match self {
            Tier::Cap(usd) if usd.is_finite() && usd > 0.0 => Some(usd),
            _ => None,
        }
    }
}

/// What the Foundry measured of this PC.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    /// The memory budget models may use, in bytes; None when it is not measured.
    pub budget: Option<u64>,
    /// Where the budget comes from, in words.
    pub basis: String,
    /// Free disk where models are kept, in bytes; None when not known.
    pub disk_free: Option<u64>,
}

/// A provider's live list, as read for the proposal.
#[derive(Clone, Debug, PartialEq)]
pub enum Listed {
    /// The models it lists for the part, each with its price per million tokens.
    Read(Vec<(String, Price)>),
    /// Not read (not connected, or the read failed): why.
    NotRead(String),
}

/// The part's choice in effect now, as the chooser needs it.
#[derive(Clone, Debug, PartialEq)]
pub enum Now {
    /// Nothing chosen, or a choice the catalogue ranks: the chooser proposes.
    Open,
    /// A choice the catalogue cannot rank (by its words): kept.
    Unranked(String),
}

pub struct Inputs<'a> {
    pub catalogue: &'a Catalogue,
    pub effort: Effort,
    pub tier: Tier,
    pub machine: Machine,
    /// Each local candidate's download in bytes, from the Hub's listing (Err: why it could not be read).
    pub sizes: HashMap<String, Result<u64, String>>,
    /// Live lists by provider id, for each part's task (keyed by `<provider>/<part>`).
    pub lists: HashMap<String, Listed>,
    /// The providers the person has connected.
    pub connected: HashSet<String>,
    pub eyes_on: bool,
    pub now: HashMap<Part, Now>,
}

/// What typical use is taken to be, for the expected month.
pub const TYPICAL_USE: &str = "one hour of conversation a day for 30 days: 60 exchanges an hour, each about 1,500 tokens \
     sent (the conversation so far) and 300 written; for the eyes, 30 pictures a day, each about 1,000 tokens sent and 200 \
     written";

/// Tokens a month at typical use for `part`: (sent, written).
pub fn typical_tokens(part: Part) -> (u64, u64) {
    match part {
        Part::Mind => (60 * 30 * 1_500, 60 * 30 * 300),
        Part::Eyes => (30 * 30 * 1_000, 30 * 30 * 200),
        _ => (0, 0),
    }
}

/// A fifth more than a model's files, for its context and runtime, when it is held against the memory budget.
pub fn footprint(bytes: u64) -> u64 {
    bytes.saturating_add(bytes / 5)
}

/// What a row proposes.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    /// Download and run on this PC.
    Local { id: String, name: String, repo: String, revision: String, files: Vec<LocalFile>, bytes: u64 },
    /// Run on a provider, on the person's own account.
    Cloud { id: String, name: String, provider: String, model: String, price: Price, monthly: f64 },
    /// Keep the part's choice as it is.
    Keep(String),
    /// The part stays off.
    Off,
    /// Nothing fits.
    Nothing,
}

impl Pick {
    pub fn paid(&self) -> bool {
        matches!(self, Pick::Cloud { price, .. } if !price.is_free())
    }

    /// The choice it sets for `part` (None: none, the part keeps what it has or stays off).
    pub fn choice(&self, part: Part) -> Option<Choice> {
        match self {
            Pick::Local { files, .. } => {
                let model = files.iter().find(|f| f.role == "model")?;
                let name = model.name().to_string();
                Some(match part {
                    Part::Ears => Choice::SpeechFile(name),
                    Part::Voice => Choice::VoiceFile(name),
                    _ => Choice::Gguf(name.strip_suffix(".gguf").unwrap_or(&name).to_string()),
                })
            }
            Pick::Cloud { provider, model, .. } => Some(Choice::Cloud { provider: provider.clone(), model: model.clone(), agreed: None }),
            _ => None,
        }
    }
}

/// One row of a proposal: the parts it covers, what it proposes, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub parts: Vec<Part>,
    pub pick: Pick,
    pub why: String,
    pub thinking: bool,
    /// The provider needs connecting first.
    pub connect: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub rows: Vec<Row>,
    pub effort: Effort,
    pub tier: Tier,
    /// Bytes to download, added up.
    pub download: u64,
    /// Expected US dollars a month at typical use.
    pub monthly: f64,
}

impl Proposal {
    /// Each part's choice as the proposal would set it (parts it leaves as they are are absent).
    pub fn choices(&self) -> Vec<(Part, Choice)> {
        self.rows.iter().flat_map(|r| r.parts.iter().filter_map(|p| r.pick.choice(*p).map(|c| (*p, c)))).collect()
    }
}

#[derive(Clone, Debug)]
struct Option_<'a> {
    c: &'a Candidate,
    local: Option<u64>,
    cloud: Option<(String, String, Price, f64)>,
}

impl Option_<'_> {
    /// Larger is better: rank, then this PC, open weights, free, the cheaper month.
    fn key(&self) -> (u32, bool, bool, bool, i64) {
        let (free, monthly) = self.cloud.as_ref().map_or((true, 0.0), |(_, _, p, m)| (p.is_free(), *m));
        (self.c.rank, self.local.is_some(), self.c.licence.open_weights, free, -(monthly * 1e6) as i64)
    }
}

fn best<'a>(options: &[Option_<'a>]) -> Option<Option_<'a>> {
    options.iter().max_by_key(|o| o.key()).cloned()
}

fn money(usd: f64) -> String {
    if usd <= 0.0 { "$0".into() } else if usd < 0.01 { "under 1 cent".into() } else { format!("${usd:.2}") }
}

/// The id a provider is called with for a candidate's model, and its price, from the live list: an OpenRouter free
/// variant first, then (at a paid tier) the paid model with a known price; another provider's free plan model.
fn on_list(provider: &str, model: &str, listed: Option<&Listed>, paid_ok: bool) -> Result<(String, Price), String> {
    match listed {
        Some(Listed::Read(rows)) => {
            if provider == "openrouter" {
                let free = format!("{model}:free");
                if let Some((id, p)) = rows.iter().find(|(id, p)| *id == free && p.is_free()) {
                    return Ok((id.clone(), *p));
                }
                if paid_ok && let Some((id, p)) = rows.iter().find(|(id, p)| id == model && !p.is_free()) {
                    return Ok((id.clone(), *p));
                }
                return Err(if paid_ok { "not on OpenRouter's list with a price".into() } else { "no free variant on OpenRouter's list".into() });
            }
            rows.iter().find(|(id, _)| id == model).map(|(id, p)| (id.clone(), *p)).ok_or_else(|| "not on the provider's live list".into())
        }
        // a free plan's model, checked against its live list once the person connects
        Some(Listed::NotRead(_)) | None if provider != "openrouter" => Ok((model.to_string(), Price { input_per_m: 0.0, output_per_m: 0.0 })),
        Some(Listed::NotRead(why)) => Err(why.clone()),
        None => Err("OpenRouter's list was not read".into()),
    }
}

/// The proposal for `inputs`.
pub fn propose<'a>(inputs: &Inputs<'a>) -> Proposal {
    let mut rows = Vec::new();
    let mut used_memory = 0u64;
    let mut spent = 0.0f64;
    let cap = inputs.tier.cap();
    for part in [Part::Mind, Part::Ears, Part::Embeddings, Part::Eyes, Part::Voice] {
        let row = |pick: Pick, why: String| Row { parts: vec![part], pick, why, thinking: false, connect: false };
        if part == Part::Eyes && !inputs.eyes_on {
            rows.push(row(Pick::Off, "Off at every tier until you turn them on: pictures of your screen are the most sensitive thing Sinai can be given.".into()));
            continue;
        }
        if let Some(Now::Unranked(words)) = inputs.now.get(&part) {
            rows.push(row(Pick::Keep(words.clone()), format!("Kept: the catalogue cannot rank {words} against its candidates, so it is not replaced.")));
            continue;
        }
        let candidates: Vec<&'a Candidate> = inputs.catalogue.of(part).filter(|c| c.efforts.contains(&inputs.effort)).collect();
        if inputs.catalogue.of(part).next().is_none() {
            let p = part.title().to_lowercase();
            rows.push(row(Pick::Nothing, format!("The catalogue has no ranked {p} model yet, so the {p} keeps what it has.")));
            continue;
        }
        let options_for = |list: &[&'a Candidate]| -> (Vec<Option_<'a>>, Vec<String>) {
            let mut options = Vec::new();
            let mut held_back: Vec<String> = Vec::new();
            for c in list {
                if c.local.is_some() {
                    match (inputs.sizes.get(&c.id), inputs.machine.budget) {
                        (Some(Ok(bytes)), Some(budget)) if used_memory + footprint(*bytes) <= budget => options.push(Option_ { c, local: Some(*bytes), cloud: None }),
                        (Some(Ok(_)), Some(_)) => held_back.push(format!("{} does not fit this PC's memory beside the parts above", c.name)),
                        (Some(Ok(_)), None) => held_back.push("this PC's memory budget is not measured".into()),
                        (Some(Err(why)), _) => held_back.push(format!("{}: {why}", c.name)),
                        (None, _) => held_back.push(format!("{}'s download size was not read", c.name)),
                    }
                }
                if !inputs.tier.cloud() {
                    continue;
                }
                for m in &c.cloud {
                    if cloud::offer(part, &m.provider).is_none() {
                        continue;
                    }
                    let listed = inputs.lists.get(&format!("{}/{}", m.provider, part.key()));
                    match on_list(&m.provider, &m.model, listed, cap.is_some()) {
                        Ok((id, price)) => {
                            let (sent, written) = typical_tokens(part);
                            let monthly = price.cost(sent, written);
                            if !price.is_free() && cap.is_none_or(|cap| spent + monthly > cap) {
                                held_back.push(format!("{} on {} would pass the monthly cap at typical use", c.name, cloud::provider_label(&m.provider)));
                                continue;
                            }
                            options.push(Option_ { c, local: None, cloud: Some((m.provider.clone(), id, price, monthly)) });
                        }
                        Err(why) => held_back.push(format!("{} on {}: {why}", c.name, cloud::provider_label(&m.provider))),
                    }
                }
            }
            held_back.dedup();
            (options, held_back)
        };
        let (mut options, mut held_back) = options_for(&candidates);
        // nothing suited to this effort level fits: the best of the part's other candidates that does, said so
        let mut stepped = false;
        if options.is_empty() {
            let others: Vec<&'a Candidate> = inputs.catalogue.of(part).filter(|c| !c.efforts.contains(&inputs.effort)).collect();
            let (more, why) = options_for(&others);
            if more.is_empty() {
                held_back.extend(why);
            } else {
                options = more;
                stepped = true;
            }
        }
        let here: Vec<_> = options.iter().filter(|o| o.local.is_some()).cloned().collect();
        let away: Vec<_> = options.iter().filter(|o| o.cloud.is_some()).cloned().collect();
        let (best_here, best_away) = (best(&here), best(&away));
        let chosen = match (&best_here, &best_away) {
            (Some(h), Some(a)) if a.c.rank > h.c.rank + 1 => Some((a.clone(), format!("rank {} in the cloud is more than one step above rank {} on this PC", a.c.rank, h.c.rank))),
            (Some(h), Some(a)) => Some((h.clone(), format!("this PC is preferred: its best (rank {}) is within a step of the cloud's (rank {})", h.c.rank, a.c.rank))),
            (Some(h), None) => Some((h.clone(), String::new())),
            (None, Some(a)) => Some((a.clone(), String::new())),
            (None, None) => None,
        };
        let Some((o, versus)) = chosen else {
            let mut why = format!("Nothing for the {} fits", part.title().to_lowercase());
            why.push_str(if inputs.tier.cloud() { " this PC or the budget" } else { " this PC, and This PC only keeps it here" });
            if !held_back.is_empty() {
                why.push_str(&format!(" ({})", held_back.join("; ")));
            }
            why.push('.');
            rows.push(row(Pick::Nothing, why));
            continue;
        };
        let scale = inputs.catalogue.scales.get(&o.c.scale);
        let source = scale.map(|s| s.title.clone()).unwrap_or_default();
        let cited = match scale.map(|s| &s.source) {
            Some(super::catalogue::Source::Benchmark(url)) => url.clone(),
            Some(super::catalogue::Source::Measurement(file)) => file.clone(),
            None => String::new(),
        };
        let mut why = format!(
            "Best ranked for {} that fits (rank {} of {} by {}. {})",
            inputs.effort.key(),
            o.c.rank,
            inputs.catalogue.of(part).count(),
            source,
            o.c.basis
        );
        if stepped {
            why.push_str(&format!(" Nothing suited to {} fits, so this is the best that does.", inputs.effort.key()));
        }
        if !cited.is_empty() {
            why.push_str(&format!(" Source: {cited}."));
        }
        if !versus.is_empty() {
            why.push_str(&format!(" {}.", capital(&versus)));
        }
        let mut r = match (o.local, &o.cloud) {
            (Some(bytes), _) => {
                used_memory += footprint(bytes);
                let l = o.c.local.clone().expect("a local option has local files");
                why.push_str(" Runs on this PC: nothing leaves it.");
                row(Pick::Local { id: o.c.id.clone(), name: o.c.name.clone(), repo: l.repo, revision: l.revision, files: l.files, bytes }, why)
            }
            (None, Some((provider, model, price, monthly))) => {
                spent += monthly;
                let label = cloud::provider_label(provider);
                if price.is_free() {
                    why.push_str(&format!(" Free on your own {label} account."));
                } else {
                    why.push_str(&format!(
                        " Paid on your own {label} account at ${:.2} sent and ${:.2} written per million tokens; about {} a month at typical use, within your cap.",
                        price.input_per_m,
                        price.output_per_m,
                        money(*monthly)
                    ));
                }
                let mut r = row(Pick::Cloud { id: o.c.id.clone(), name: o.c.name.clone(), provider: provider.clone(), model: model.clone(), price: *price, monthly: *monthly }, why);
                r.connect = !inputs.connected.contains(provider);
                r
            }
            _ => unreachable!("an option runs here or in the cloud"),
        };
        r.thinking = part == Part::Mind && inputs.effort == Effort::Deep && o.c.thinking;
        rows.push(r);
    }
    let rows = merge(rows);
    let download = rows.iter().map(|r| if let Pick::Local { bytes, .. } = r.pick { bytes } else { 0 }).sum();
    let monthly = rows.iter().map(|r| if let Pick::Cloud { monthly, .. } = r.pick { monthly } else { 0.0 }).sum();
    Proposal { rows, effort: inputs.effort, tier: inputs.tier, download, monthly }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Rows that propose the same model in the same place become one row covering their parts.
pub fn merge(rows: Vec<Row>) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for r in rows {
        let same = |a: &Pick, b: &Pick| match (a, b) {
            (Pick::Local { id: x, .. }, Pick::Local { id: y, .. }) => x == y,
            (Pick::Cloud { provider: p, model: m, .. }, Pick::Cloud { provider: q, model: n, .. }) => p == q && m == n,
            _ => false,
        };
        match out.iter_mut().find(|o| same(&o.pick, &r.pick)) {
            Some(o) => {
                o.parts.extend(r.parts);
                o.thinking |= r.thinking;
            }
            None => out.push(r),
        }
    }
    out
}

/// Whether a proposal would change what is in effect: some part's proposed choice differs from `current`.
pub fn differs(proposal: &Proposal, current: &dyn Fn(Part) -> Option<Choice>) -> bool {
    proposal.choices().into_iter().any(|(part, want)| {
        let have = current(part);
        match (&want, &have) {
            // a cloud choice is the same model on the same provider, whenever it was agreed
            (Choice::Cloud { provider: p, model: m, .. }, Some(Choice::Cloud { provider: q, model: n, .. })) => p != q || m != n,
            _ => have.as_ref() != Some(&want),
        }
    })
}
