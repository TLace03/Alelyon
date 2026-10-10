//! One live harvest, start to saved: the call an application makes. Blocking and long (minutes); run it on a plain
//! thread (reqwest's blocking client must not be used inside an async runtime).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::arxiv::Arxiv;
use crate::index::{Error, Index};
use crate::net::HttpFetch;
use crate::openalex::OpenAlex;
use crate::graph::Role;
use crate::snowball::{Policy, Reason, Report, RoundHook, Snowball, Topic};
use crate::store::{Stamp, Store};

/// Gather `topic` from OpenAlex and arXiv and save it to the archive at `db`. `openalex_key` goes to
/// api.openalex.org only, as a bearer header. `now` stamps the saved run. A run that stops early (budget, quota,
/// refused key, `cancel`) is saved too, marked with why.
pub fn harvest(
    db: &Path,
    topic: Topic,
    policy: Policy,
    openalex_key: Option<String>,
    cancel: Arc<AtomicBool>,
    on_round: Option<RoundHook>,
    now: &Stamp,
) -> Result<Report, Error> {
    // Open the archive first: a store that cannot be written should fail before any request is spent.
    let mut store = Store::open(db)?;
    let mut oa = OpenAlex::new(HttpFetch::new()?.with_bearer("api.openalex.org", openalex_key));
    let mut ax = Arxiv::new(HttpFetch::new()?);
    let mut run = Snowball::new(topic, policy);
    run.cancel = Some(cancel);
    run.on_round = on_round;
    let report = run.run_with(&mut [&mut oa as &mut dyn Index, &mut ax], |r, progress| checkpoint(&mut store, r, progress, now));
    store.save(&run, &report, now)?;
    // The subject's knowledge graph follows every run, so it is never older than the archive.
    store.rebuild_graph(&run.topic.name)?;
    Ok(report)
}

/// Days before the last run that an update still reads, to catch papers published earlier but indexed late (the free
/// API filters by publication date, not by when a work reached the index).
pub const UPDATE_MARGIN_DAYS: i64 = 45;
/// The subject's most influential papers whose new citers an update reads.
pub const UPDATE_SEEDS: usize = 40;

/// Update a saved subject with what is new: its searches and the newest citers of its most influential papers,
/// limited to works published since its last run (less a margin); papers newly accepted are expanded as in a full
/// run. Saved as an `update` run, then the graph and gaps are rebuilt.
pub fn update(
    db: &Path,
    name: &str,
    max_calls: usize,
    openalex_key: Option<String>,
    cancel: Arc<AtomicBool>,
    on_round: Option<RoundHook>,
    now: &Stamp,
) -> Result<Report, Error> {
    let mut store = Store::open(db)?;
    let t = store.topics()?.into_iter().find(|t| t.name == name).ok_or_else(|| Error::Store(format!("no subject named {name:?}")))?;
    let topic = Topic { name: t.name.clone(), queries: t.queries.clone(), phrases: t.phrases.clone() };
    let mut policy = Policy::for_topic(&topic);
    policy.max_calls = max_calls;
    let members: Vec<_> = store
        .members(name)?
        .into_iter()
        .filter_map(|m| Reason::parse(&m.reason).map(|r| (m.work, r)))
        .collect();
    let from = t.watched_at.unwrap_or(now.secs - 365 * 86_400) - UPDATE_MARGIN_DAYS * 86_400;
    let since = date_of(from);
    let mut run = Snowball::resume(topic, policy, members);
    // Seeds: the most influential papers (by the graph), else the first members.
    let mut seeds = Vec::new();
    for n in store.graph_nodes(name, None, None, UPDATE_SEEDS)? {
        if n.role == Role::Member && seeds.len() >= UPDATE_SEEDS / 2 {
            continue;
        }
        if let Some(w) = store.work(n.paper.work)?
            && let Some(k) = w.ids.iter().find_map(|id| run.archive.find(id))
        {
            seeds.push(k);
        }
    }
    run.seed_citers = seeds;
    run.cancel = Some(cancel);
    run.on_round = on_round;
    let mut oa = OpenAlex::new(HttpFetch::new()?.with_bearer("api.openalex.org", openalex_key)).with_since(Some(since.clone()));
    let mut ax = Arxiv::new(HttpFetch::new()?).with_since(Some(since));
    let report = run.run_with(&mut [&mut oa as &mut dyn Index, &mut ax], |r, progress| checkpoint(&mut store, r, progress, now));
    store.save(&run, &report, now)?;
    store.rebuild_graph(name)?;
    Ok(report)
}

/// Save a run in progress with its graph, so a crash of the process loses at most the round being read. A failed
/// checkpoint is not fatal: the run goes on and its outcome is saved at the end.
fn checkpoint(store: &mut Store, run: &Snowball, progress: &Report, now: &Stamp) {
    if store.save(run, progress, now).is_ok() {
        let _ = store.rebuild_graph(&run.topic.name);
    }
}

pub use crate::harvest_date::date_of;
