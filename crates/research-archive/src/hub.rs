//! The research hub: what to work on next, personalised from what the person has done on this PC, never sent
//! anywhere. Interests are the words of their subjects' concepts and searches, their searches in the archive, and
//! the titles of papers they opened or saved, weighted toward the recent. They rank:
//!
//! - **Ideas**: the subjects' gaps (gaps.rs) not dismissed, each kind's strongest first, lifted by how much of the
//!   person's interests they touch, and mixed across kinds so one kind does not crowd out the rest;
//! - **Read next**: foundations and forgotten foundations of their subjects they have not opened;
//! - **New at the frontier**: the newest papers that build on a subject and are not yet cited by it;
//! - **Subjects to start**: new subjects proposed from the strongest gaps, ready to gather;
//! - **Saved**: the papers they saved.
//! - **New from updates**: what watched subjects' updates added in the last fortnight, recent work apart from older
//!   papers an update found for the first time.
//!
//! Every suggestion carries why it was made. Nothing here fetches; it reads the archive.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::gaps::Kind;
use crate::graph::Role;
use crate::ident;
use crate::index::Error;
use crate::store::{NewPaper, PaperRef, Store, StoredGap};

#[derive(Debug, Clone, PartialEq)]
pub struct Idea {
    pub gap: StoredGap,
    /// The person's interests it touches, if any.
    pub because: Vec<String>,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub paper: PaperRef,
    pub subject: String,
    pub why: String,
}

/// A subject the hub proposes, ready to fill the new-subject form.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub name: String,
    pub queries: Vec<String>,
    pub concepts: Vec<String>,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hub {
    pub ideas: Vec<Idea>,
    pub read_next: Vec<Reading>,
    pub frontier: Vec<Reading>,
    pub proposals: Vec<Proposal>,
    pub saved: Vec<PaperRef>,
    /// The person's strongest interests, as the hub read them.
    pub interests: Vec<String>,
    /// Papers updates added in the last fortnight that were published recently (this year or last).
    pub new_work: Vec<NewPaper>,
    /// Papers updates added in the last fortnight that are older: found, not new.
    pub newly_found: Vec<NewPaper>,
}

/// How far back the hub reports what updates added.
pub const NEW_WINDOW_SECS: i64 = 14 * 86_400;

/// Words too general to say what a person is interested in.
const GENERAL: &[&str] = &[
    "neural", "network", "networks", "learning", "deep", "model", "models", "based", "using", "with", "from", "data", "method",
    "methods", "approach", "analysis", "system", "systems", "paper", "papers", "study", "towards", "toward", "efficient",
];

fn words(text: &str) -> Vec<String> {
    ident::title_fingerprint(text).split(' ').filter(|w| w.len() > 3 && !GENERAL.contains(w)).map(str::to_string).collect()
}

/// The person's interests: word -> weight.
pub fn interests(store: &Store) -> Result<HashMap<String, f64>, Error> {
    let mut w: HashMap<String, f64> = HashMap::new();
    for t in store.topics()? {
        for c in t.phrases.iter().chain(t.queries.iter()) {
            for x in words(&c.replace('|', " ")) {
                *w.entry(x).or_default() += 1.0;
            }
        }
    }
    // Activity, newest first, fading by 2% a step.
    for (i, a) in store.activity(500)?.iter().enumerate() {
        let fade = 0.98f64.powi(i as i32);
        let (text, weight) = match a.kind.as_str() {
            "search" => (a.text.clone().unwrap_or_default(), 2.0),
            "save" => (a.work.and_then(|x| store.work(x).ok().flatten()).map(|x| x.title).unwrap_or_default(), 2.0),
            "open" => (a.work.and_then(|x| store.work(x).ok().flatten()).map(|x| x.title).unwrap_or_default(), 1.0),
            _ => continue,
        };
        for x in words(&text) {
            *w.entry(x).or_default() += weight * fade;
        }
    }
    Ok(w)
}

/// The hub as of `now` (Unix seconds).
pub fn build(store: &Store, now: i64) -> Result<Hub, Error> {
    let interest = interests(store)?;
    let seen = store.seen_works()?;
    let mut hub = Hub::default();
    let mut top: Vec<(&String, &f64)> = interest.iter().collect();
    top.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
    hub.interests = top.into_iter().take(12).map(|(w, _)| w.clone()).collect();

    // Ideas: within a kind, strength by rank (1, 1/2, 1/3, …), lifted by interests touched; then interleaved by kind.
    let mut by_kind: BTreeMap<&'static str, Vec<Idea>> = BTreeMap::new();
    for gap in store.gaps(None)?.into_iter().filter(|g| !g.dismissed) {
        let text = format!("{} {} {}", gap.headline, gap.terms.join(" "), gap.works.iter().map(|p| p.title.as_str()).collect::<Vec<_>>().join(" "));
        let mut because: Vec<String> = words(&text).into_iter().filter(|x| interest.get(x).is_some_and(|v| *v >= 1.0)).collect::<HashSet<_>>().into_iter().collect();
        because.sort_by(|a, b| interest[b].total_cmp(&interest[a]).then(a.cmp(b)));
        because.truncate(4);
        let lift: f64 = because.iter().map(|x| interest[x].ln_1p()).sum();
        let score = (1.0 / (1.0 + gap.rank as f64)) * (1.0 + lift);
        by_kind.entry(gap.kind.name()).or_default().push(Idea { gap, because, score });
    }
    for list in by_kind.values_mut() {
        list.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.gap.headline.cmp(&b.gap.headline)));
    }
    let order = [Kind::Unbridged, Kind::Combination, Kind::Forgotten, Kind::Young, Kind::Stalled, Kind::BlindSpot];
    let mut iters: Vec<std::vec::IntoIter<Idea>> = order.iter().filter_map(|k| by_kind.remove(k.name())).map(|v| v.into_iter()).collect();
    while hub.ideas.len() < 18 {
        let before = hub.ideas.len();
        for it in iters.iter_mut() {
            if let Some(i) = it.next() {
                hub.ideas.push(i);
            }
        }
        if hub.ideas.len() == before {
            break;
        }
    }

    // Read next: each subject's foundations and forgotten foundations not yet opened, by influence and interest.
    let match_score = |title: &str| words(title).iter().filter_map(|x| interest.get(x)).map(|v| v.ln_1p()).sum::<f64>();
    let mut reading: Vec<(f64, Reading)> = Vec::new();
    let mut frontier: Vec<(i32, f64, Reading)> = Vec::new();
    for t in store.topics()? {
        for n in store.graph_nodes(&t.name, None, Some(Role::Foundation), 30)? {
            if !seen.contains(&n.paper.work) {
                let s = n.pagerank * 100.0 * (1.0 + match_score(&n.paper.title));
                reading.push((s, Reading { why: format!("a foundation of \"{}\": among its most influential papers", t.name), subject: t.name.clone(), paper: n.paper }));
            }
        }
        for n in store.graph_nodes(&t.name, None, Some(Role::Frontier), 60)? {
            if !seen.contains(&n.paper.work) {
                let s = match_score(&n.paper.title);
                frontier.push((n.paper.year.unwrap_or(0), s, Reading { why: format!("new in \"{}\": builds on it, not yet cited by it", t.name), subject: t.name.clone(), paper: n.paper }));
            }
        }
    }
    for idea in hub.ideas.iter().filter(|i| i.gap.kind == Kind::Forgotten) {
        for p in &idea.gap.works {
            if !seen.contains(&p.work) {
                reading.push((f64::MAX / 4.0, Reading { why: format!("forgotten in \"{}\": recent work stopped citing it", idea.gap.subject), subject: idea.gap.subject.clone(), paper: p.clone() }));
            }
        }
    }
    reading.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.paper.work.cmp(&b.1.paper.work)));
    let mut have = HashSet::new();
    hub.read_next = reading.into_iter().filter(|(_, r)| have.insert(r.paper.work)).take(10).map(|(_, r)| r).collect();
    frontier.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)).then(a.2.paper.work.cmp(&b.2.paper.work)));
    hub.frontier = frontier.into_iter().take(8).map(|(_, _, r)| r).collect();

    // Subjects to start, from the strongest gaps that name words to search for.
    let existing: HashSet<String> = store.topics()?.into_iter().map(|t| t.name.to_lowercase()).collect();
    for idea in hub.ideas.iter().filter(|i| matches!(i.gap.kind, Kind::Unbridged | Kind::Combination)) {
        let terms: Vec<String> = idea.gap.terms.iter().filter(|t| !t.is_empty()).cloned().collect();
        if terms.len() < 2 {
            continue;
        }
        let (a, b) = match idea.gap.kind {
            Kind::Combination => (terms[0].clone(), terms[1].clone()),
            _ => {
                let half = terms.len() / 2;
                (terms[0].clone(), terms[half].clone())
            }
        };
        if a == b {
            continue;
        }
        let name = format!("{a} × {b}");
        if existing.contains(&name.to_lowercase()) || hub.proposals.iter().any(|p| p.name == name) {
            continue;
        }
        hub.proposals.push(Proposal {
            queries: vec![format!("{a} {b}")],
            concepts: vec![a.clone(), b.clone()],
            why: format!("from \"{}\": {}", idea.gap.subject, idea.gap.headline),
            name,
        });
        if hub.proposals.len() == 5 {
            break;
        }
    }
    hub.saved = store.saved()?;
    let this_year = crate::harvest_date::year_of(now);
    for n in store.new_papers(None, now - NEW_WINDOW_SECS, 200)? {
        if n.paper.year.is_some_and(|y| y >= this_year - 1) {
            hub.new_work.push(n);
        } else {
            hub.newly_found.push(n);
        }
    }
    hub.new_work.truncate(20);
    Ok(hub)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Activity;

    fn gap(store: &Store, kind: &str, rank: i64, headline: &str, terms: &str) {
        store.raw(&format!(
            "INSERT INTO research_gaps VALUES ('s', '{kind}', {rank}, 1.0, '{headline}', 'd', '{terms}', '[]', '[]')"
        ));
    }

    #[test]
    fn ideas_mix_kinds_follow_interests_and_respect_dismissal() {
        let mut store = Store::in_memory().unwrap();
        store.raw("INSERT INTO topics (name, queries, phrases) VALUES ('s', '[\"binary weights\"]', '[\"binary\"]')");
        gap(&store, "unbridged", 0, "\"memory, sram\" and \"spiking\" rarely cite each other", "[\"memory\",\"sram\",\"spiking\",\"neuromorphic\"]");
        gap(&store, "unbridged", 1, "\"fpga\" and \"speech\" rarely cite each other", "[\"fpga\",\"speech\"]");
        gap(&store, "combination", 0, "\"ternary\" with \"medical\": rarely studied together", "[\"ternary\",\"medical\"]");
        gap(&store, "young", 0, "\"segmentation\" is young", "[]");
        let act = |kind: &str, text: &str| Activity { at: "t".into(), kind: kind.into(), subject: Some("s".into()), work: None, text: Some(text.into()) };
        // The person keeps searching for speech.
        store.log(&act("search", "speech keyword spotting")).unwrap();
        store.log(&act("search", "speech fpga")).unwrap();
        let hub = build(&store, 0).unwrap();
        let kinds: Vec<Kind> = hub.ideas.iter().map(|i| i.gap.kind).collect();
        assert_eq!(&kinds[..3], &[Kind::Unbridged, Kind::Combination, Kind::Young], "one of each kind before seconds");
        assert!(hub.ideas[0].gap.headline.contains("fpga"), "interests lifted the speech gap first: {:?}", hub.ideas[0]);
        assert!(hub.ideas[0].because.contains(&"speech".to_string()));
        assert!(hub.interests.contains(&"speech".to_string()));
        assert!(hub.proposals.iter().any(|p| p.name == "ternary × medical"), "{:?}", hub.proposals);
        store.log(&act("dismiss_gap", "\"fpga\" and \"speech\" rarely cite each other")).unwrap();
        let hub = build(&store, 0).unwrap();
        assert!(hub.ideas.iter().all(|i| !i.gap.headline.contains("fpga")));
    }
}
