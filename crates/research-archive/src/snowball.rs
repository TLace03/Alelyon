//! Gather a subject's papers: search every index, then follow citations backward (what the relevant papers cite)
//! and forward (what cites them) until a round accepts nothing new.
//!
//! Keyword search alone misses papers that describe the same idea in other words, which is most of what is older
//! than the subject's current vocabulary. Citations do not depend on vocabulary: a 1990s paper that today's papers
//! keep citing is reached through them. A paper is therefore accepted on either of two kinds of evidence, its text
//! or its links, and the reason is kept with it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::archive::{Archive, Key};
use crate::coverage::{self, Coverage};
use crate::ident::{self, Id};
use crate::index::{Error, Index};
use crate::work::{Source, Work};

/// What the user is researching.
#[derive(Debug, Clone, PartialEq)]
pub struct Topic {
    pub name: String,
    /// Sent to every index's search.
    pub queries: Vec<String>,
    /// The subject's concepts, one entry each; a concept's spellings are separated by `|`
    /// ("quantization|quantized|quantisation"): there is no stemming. A paper's text evidence is the number of
    /// distinct concepts it mentions, so two spellings of one idea count once.
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Distinct concepts a paper's text must mention to be accepted on text alone.
    pub min_phrases: usize,
    /// Core papers (accepted on their text) that must cite a paper for it to be accepted on links alone; also the
    /// number of core papers a paper must cite (with at least one phrase) to be accepted on outgoing links.
    pub min_links: usize,
    /// For acceptance on incoming links: the least share of a paper's citations (as its index counts them) that
    /// must come from the core. A subject's own foundations are cited mostly by the subject; a classic every field
    /// cites (Bayes, Newton, a standard optimiser) is cited by the core in a vanishing share.
    pub min_specificity: f64,
    /// For acceptance on outgoing links: the least share of a paper's reference list that must be core papers.
    pub min_ref_share: f64,
    /// For acceptance on incoming links in a large subject: one more citing core paper needed per this many core
    /// papers (`min_links` stays the floor). Two citers mean much in a core of 200 and little in one of 5,000.
    pub core_per_link: usize,
    pub search_limit: usize,
    /// Citing papers read per accepted paper. A paper cited more often than this is expanded only partly, and the
    /// report names it.
    pub citing_limit: usize,
    /// Index calls (not HTTP requests: one call may page) the run may make.
    pub max_calls: usize,
    pub max_rounds: usize,
}

impl Policy {
    pub fn for_topic(topic: &Topic) -> Policy {
        Policy {
            min_phrases: topic.phrases.len().clamp(1, 2),
            min_links: 2,
            min_specificity: 0.05,
            min_ref_share: 0.1,
            core_per_link: 250,
            search_limit: 200,
            citing_limit: 200,
            max_calls: 2_000,
            max_rounds: 8,
        }
    }
}

/// Why a paper is in the subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Its title and abstract contain this many of the topic's phrases.
    Text { phrases: usize },
    /// Cites this many core papers (a sufficient share of its references) and contains at least one phrase.
    Cites { links: usize, phrases: usize },
    /// This many core papers cite it, a sufficient share of everything that cites it; its own text may use other
    /// words. Kept, and its references followed, but its citers are not.
    CitedBy { links: usize },
    /// Accepted as `CitedBy` would be, but on fewer citers than a subject this large asks for and in words that are
    /// not near the subject: kept (nothing is dropped), shown apart, and left out of the subject's map, threads, gaps
    /// and reading lists. Measured 2026-10-09 on a 116,063-paper archive: about two thirds of these were tangential
    /// (hashing, chip reliability, battery materials) and a third were older foundations in other words
    /// (stochastic bit streams, committee-machine theory); dropping them would lose the latter.
    Peripheral { links: usize },
}

impl Reason {
    fn follow_citers(self) -> bool {
        !matches!(self, Reason::CitedBy { .. } | Reason::Peripheral { .. })
    }

    /// Whether the paper is on the edge of the subject (see `Reason::Peripheral`).
    pub fn peripheral(self) -> bool {
        matches!(self, Reason::Peripheral { .. })
    }

    /// A reason as the store writes it ("text:2", "cites:3:1", "cited_by:4").
    pub fn parse(text: &str) -> Option<Reason> {
        let parts: Vec<&str> = text.split(':').collect();
        let n = |i: usize| parts.get(i).and_then(|x| x.parse::<usize>().ok());
        match parts.first().copied() {
            Some("text") => Some(Reason::Text { phrases: n(1)? }),
            Some("cites") => Some(Reason::Cites { links: n(1)?, phrases: n(2)? }),
            Some("cited_by") => Some(Reason::CitedBy { links: n(1)? }),
            Some("peripheral") => Some(Reason::Peripheral { links: n(1)? }),
            _ => None,
        }
    }

    /// An ordering for expansion: more phrases first, then more links.
    fn strength(self) -> (usize, usize) {
        match self {
            Reason::Text { phrases } => (phrases, 0),
            Reason::Cites { links, phrases } => (phrases, links),
            Reason::CitedBy { links } => (0, links),
            Reason::Peripheral { links } => (0, links.min(1)),
        }
    }
}

/// How a paper was reached. A paper can be reached several ways; each is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Found {
    Search(Source),
    Reference,
    Citation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// A round accepted nothing new: every accepted paper's references and citers have been read.
    Closed,
    /// The call budget ran out with accepted papers still unexpanded.
    Budget,
    /// The round limit was reached with accepted papers still unexpanded.
    Rounds,
    /// An index refused further requests for longer than the run will wait (its quota is spent).
    Throttled,
    /// An index refused the API key.
    KeyRefused,
    /// The user stopped the run.
    Cancelled,
    /// Not a stop: a checkpoint written while the run goes on. Left in the archive, it means the run ended without
    /// saving its outcome (its process closed or crashed); what it had gathered up to then is kept.
    InProgress,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    pub accepted_total: usize,
    pub newly_accepted: usize,
    pub archive_size: usize,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub stop: Stop,
    pub rounds: Vec<Round>,
    pub calls: usize,
    pub accepted: usize,
    pub candidates: usize,
    /// Accepted papers whose citers were read only up to the limit, with the index's total count.
    pub truncated: Vec<(Key, u64)>,
    /// Calls that failed. The papers involved are expanded incompletely; the run went on.
    pub failures: Vec<String>,
    /// Why the run halted early (a spent quota, a refused key, its user), when it did.
    pub halt_reason: Option<String>,
    /// Keyword search versus citation-following, over accepted papers.
    pub coverage: Coverage,
    pub by_search: u64,
    pub by_citation: u64,
    pub by_both: u64,
    /// An update run: it read only what is new, starting from a saved subject.
    pub update: bool,
}

/// A progress callback: the round just judged, and the index calls spent so far.
pub type RoundHook = Box<dyn FnMut(&Round, usize) + Send>;

pub struct Snowball {
    pub topic: Topic,
    pub policy: Policy,
    pub archive: Archive,
    accepted: BTreeMap<Key, Reason>,
    found: HashMap<Key, BTreeSet<Found>>,
    expanded: HashSet<Key>,
    calls: usize,
    truncated: Vec<(Key, u64)>,
    failures: Vec<String>,
    rounds: Vec<Round>,
    /// Each concept's spellings, fingerprinted.
    concepts: Vec<Vec<String>>,
    /// Stems of the concepts' one-word spellings (their first six letters): a paper whose text has a word starting
    /// with one is near the subject even when it misses every spelling ("quantizer" near "quantization").
    stems: Vec<String>,
    /// Why the run halted early, when something other than its budget or round limit stopped it.
    halt: Option<(Stop, String)>,
    /// Set from another thread to stop the run before its next index call.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Called after each round is judged, with the round and the calls spent so far.
    pub on_round: Option<RoundHook>,
    /// Papers whose citers are read right after the searches (an update reads the newest citers of the subject's
    /// most influential papers).
    pub seed_citers: Vec<Key>,
    update: bool,
    /// Search-only indexes that refused further requests (a spent quota, a refused key): not asked again this run.
    dropped: BTreeSet<Source>,
}

impl Snowball {
    pub fn new(topic: Topic, policy: Policy) -> Snowball {
        let concepts = topic
            .phrases
            .iter()
            .map(|c| c.split('|').map(ident::title_fingerprint).filter(|v| !v.is_empty()).collect::<Vec<_>>())
            .filter(|c| !c.is_empty())
            .collect();
        let stems: Vec<String> = {
            let mut v: Vec<String> = topic
                .phrases
                .iter()
                .flat_map(|c| c.split('|'))
                .map(ident::title_fingerprint)
                .filter(|sp| !sp.is_empty() && !sp.contains(' '))
                .map(|sp| sp.chars().take(6).collect())
                .collect();
            v.sort();
            v.dedup();
            v
        };
        Snowball {
            stems,
            topic,
            policy,
            archive: Archive::new(),
            accepted: BTreeMap::new(),
            found: HashMap::new(),
            expanded: HashSet::new(),
            calls: 0,
            truncated: Vec::new(),
            failures: Vec::new(),
            rounds: Vec::new(),
            concepts,
            halt: None,
            cancel: None,
            on_round: None,
            seed_citers: Vec::new(),
            update: false,
            dropped: BTreeSet::new(),
        }
    }

    /// Start from a saved subject: its papers accepted already, with their reasons, and counted as expanded, so the
    /// run reads only what is new (an update) and expands only what it newly accepts.
    pub fn resume(topic: Topic, policy: Policy, members: Vec<(Work, Reason)>) -> Snowball {
        let mut s = Snowball::new(topic, policy);
        s.update = true;
        for (w, r) in members {
            let k = s.archive.insert(w).key();
            s.accepted.insert(k, r);
            s.expanded.insert(k);
        }
        s
    }

    pub fn reason(&self, key: Key) -> Option<Reason> {
        self.accepted.get(&key).copied()
    }

    pub fn accepted(&self) -> impl Iterator<Item = (Key, &Work, Reason)> {
        self.accepted.iter().map(|(&k, &r)| (k, self.archive.get(k), r))
    }

    pub fn found(&self, key: Key) -> Option<&BTreeSet<Found>> {
        self.found.get(&key)
    }

    fn take(&mut self, works: Vec<Work>, how: Found) {
        for w in works {
            let k = self.archive.insert(w).key();
            self.found.entry(k).or_default().insert(how);
        }
    }

    fn halted(&self) -> bool {
        self.halt.is_some()
    }

    fn spend(&mut self) -> bool {
        if self.halt.is_none() && self.cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst)) {
            self.halt = Some((Stop::Cancelled, "stopped by its user".into()));
        }
        if self.halted() || self.calls >= self.policy.max_calls {
            return false;
        }
        self.calls += 1;
        true
    }

    /// Record a failed call to `source`. Waiting out a spent quota could take hours and a refused key will not be
    /// accepted on the next call, so: the citation index refusing ends the run (nothing more can be followed), and a
    /// search-only index refusing is dropped for the rest of it (observed 2026-10-09: arXiv answering 429 ended a run
    /// after 4 requests while OpenAlex had its whole day's budget).
    fn note(&mut self, source: Source, carries_citations: bool, what: String, e: Error) {
        let refused = match &e {
            Error::RateLimited(m) => Some((Stop::Throttled, m.clone())),
            Error::KeyRefused(m) => Some((Stop::KeyRefused, m.clone())),
            _ => None,
        };
        match refused {
            Some(halt) if carries_citations => self.halt = Some(halt),
            Some(_) => {
                self.dropped.insert(source);
                self.failures.push(format!("{what}: {e} ({} is not asked again this run)", source.name()));
                return;
            }
            None => {}
        }
        self.failures.push(format!("{what}: {e}"));
    }

    /// Whether a paper's title or abstract has a word starting with a concept stem.
    fn near(&self, w: &Work) -> bool {
        let text = format!("{} {}", ident::title_fingerprint(&w.title), ident::title_fingerprint(w.abstract_text.as_deref().unwrap_or("")));
        text.split(' ').any(|word| self.stems.iter().any(|s| word.starts_with(s.as_str())))
    }

    /// Distinct topic concepts in a paper's title and abstract.
    pub fn phrases_in(&self, w: &Work) -> usize {
        let text = format!(" {} {} ", ident::title_fingerprint(&w.title), ident::title_fingerprint(w.abstract_text.as_deref().unwrap_or("")));
        self.concepts.iter().filter(|c| c.iter().any(|v| text.contains(&format!(" {v} ")))).count()
    }

    /// Accept every paper the evidence supports, repeating until nothing changes (a paper accepted on its text joins
    /// the core and can make another paper's links sufficient). Returns how many were newly accepted.
    ///
    /// Only the core (papers accepted on their own text) counts as link evidence. Letting link-accepted papers vouch
    /// for further papers cascades: observed 2026-10-08, a live run with that rule accepted 79,493 papers in one
    /// round, Newton's Principia among them.
    fn evaluate(&mut self) -> usize {
        let before = self.accepted.len();
        let p = self.policy.clone();
        loop {
            let core: HashSet<Key> = self.accepted.iter().filter(|(_, r)| matches!(r, Reason::Text { .. })).map(|(&k, _)| k).collect();
            // Citers needed for acceptance on incoming links: the floor, rising with the core's size.
            let need = p.min_links.max(core.len().div_ceil(p.core_per_link.max(1)));
            // For each archived paper, how many core papers cite it.
            let mut cited_by: HashMap<Key, usize> = HashMap::new();
            for &a in &core {
                let targets: HashSet<Key> = self.archive.get(a).references.iter().filter_map(|id| self.archive.find(id)).collect();
                for t in targets {
                    *cited_by.entry(t).or_default() += 1;
                }
            }
            let mut newly = Vec::new();
            for (k, w) in self.archive.works() {
                if self.accepted.contains_key(&k) {
                    continue;
                }
                let phrases = self.phrases_in(w);
                let cites: HashSet<Key> = w.references.iter().filter_map(|id| self.archive.find(id)).filter(|t| core.contains(t)).collect();
                let ref_share = cites.len() as f64 / w.references.len().max(1) as f64;
                let links = cited_by.get(&k).copied().unwrap_or(0);
                let specific = match w.cited_by_count {
                    Some(total) => links as f64 / (total.max(links as u64)).max(1) as f64 >= p.min_specificity,
                    // Without the index's count the share is unknown; ask for twice the links instead.
                    None => links >= 2 * need,
                };
                let reason = if phrases >= p.min_phrases {
                    Some(Reason::Text { phrases })
                } else if phrases >= 1 && cites.len() >= p.min_links && ref_share >= p.min_ref_share {
                    Some(Reason::Cites { links: cites.len(), phrases })
                } else {
                    // A large core needs more citers; a paper near the subject in its own words keeps the floor (the
                    // old foundation that calls it a "quantizer" is not the chemistry paper about binary gases).
                    if !(specific && links >= p.min_links) {
                        None
                    } else if links >= need || self.near(w) {
                        Some(Reason::CitedBy { links })
                    } else {
                        Some(Reason::Peripheral { links })
                    }
                };
                if let Some(r) = reason {
                    newly.push((k, r));
                }
            }
            // Only a new core paper can change the evidence for another.
            let grew_core = newly.iter().any(|(_, r)| matches!(r, Reason::Text { .. }));
            self.accepted.extend(newly);
            if !grew_core {
                break;
            }
        }
        self.accepted.len() - before
    }

    /// Judge the archive's papers under the current policy without calling any index. For re-judging a saved
    /// archive after a policy change; it expands nothing, so it never closes a run.
    pub fn judge(&mut self) -> usize {
        self.evaluate()
    }

    fn record_round(&mut self, newly: usize) {
        self.rounds.push(Round { accepted_total: self.accepted.len(), newly_accepted: newly, archive_size: self.archive.len() });
    }

    /// Tell `on_round` about the latest round. Called after its checkpoint, so a round reported is a round saved.
    fn announce(&mut self) {
        if let (Some(f), Some(r)) = (self.on_round.as_mut(), self.rounds.last()) {
            f(r, self.calls);
        }
    }

    /// Run to closure or budget. Index failures are recorded and the run continues.
    pub fn run(&mut self, indexes: &mut [&mut dyn Index]) -> Report {
        self.run_with(indexes, |_, _| {})
    }

    /// Run as `run` does, calling `checkpoint` after each round that read anything (with a report marked
    /// `Stop::InProgress`), so a caller can save as it goes and a crash loses at most one round.
    pub fn run_with(&mut self, indexes: &mut [&mut dyn Index], mut checkpoint: impl FnMut(&Snowball, &Report)) -> Report {
        let mut searched_all = true;
        for i in indexes.iter_mut() {
            for q in self.topic.queries.clone() {
                if self.dropped.contains(&i.source()) {
                    break;
                }
                if !self.spend() {
                    searched_all = false;
                    break;
                }
                match i.search(&q, self.policy.search_limit) {
                    Ok(ws) => self.take(ws, Found::Search(i.source())),
                    Err(e) => self.note(i.source(), i.has_citations(), format!("{} search {q:?}", i.source().name()), e),
                }
            }
        }
        for k in std::mem::take(&mut self.seed_citers) {
            let work = self.archive.get(k).clone();
            for i in indexes.iter_mut().filter(|i| i.has_citations()) {
                if !self.spend() {
                    searched_all = false;
                    break;
                }
                match i.citing(&work, self.policy.citing_limit) {
                    Ok(Some(ws)) => self.take(ws, Found::Citation),
                    Ok(None) => {}
                    Err(e) => self.note(i.source(), true, format!("{} new citers of {:?}", i.source().name(), work.title), e),
                }
            }
        }

        let stop = loop {
            let newly = self.evaluate();
            self.record_round(newly);
            if self.calls > 0 {
                let r = self.report(Stop::InProgress);
                checkpoint(self, &r);
            }
            self.announce();
            let mut pending: Vec<(Key, Reason)> = self.accepted.iter().filter(|(k, _)| !self.expanded.contains(k)).map(|(&k, &r)| (k, r)).collect();
            if pending.is_empty() {
                // Closure means every search ran and every accepted paper was expanded.
                break match &self.halt {
                    Some((stop, _)) => *stop,
                    None if !searched_all => Stop::Budget,
                    None => Stop::Closed,
                };
            }
            if self.rounds.len() > self.policy.max_rounds {
                break Stop::Rounds;
            }
            // Strongest evidence first, so a budget that runs out mid-round has spent itself on the core papers.
            pending.sort_by_key(|&(_, r)| std::cmp::Reverse(r.strength()));
            if !self.expand(&pending, indexes) {
                // Judge what the partial round reached; an unjudged paper would read as rejected.
                let newly = self.evaluate();
                self.record_round(newly);
                self.announce();
                break self.halt.as_ref().map_or(Stop::Budget, |(stop, _)| *stop);
            }
        };
        self.report(stop)
    }

    /// The run's outcome so far, as if it stopped now with `stop`.
    pub fn report(&self, stop: Stop) -> Report {
        let (mut s, mut c, mut both) = (0u64, 0u64, 0u64);
        for k in self.accepted.keys() {
            // A paper put into the archive directly (a saved archive reloaded, a user's own pick) was found by neither.
            let Some(f) = self.found.get(k) else { continue };
            let by_s = f.iter().any(|x| matches!(x, Found::Search(_)));
            let by_c = f.iter().any(|x| matches!(x, Found::Reference | Found::Citation));
            s += by_s as u64;
            c += by_c as u64;
            both += (by_s && by_c) as u64;
        }
        Report {
            stop,
            rounds: self.rounds.clone(),
            calls: self.calls,
            accepted: self.accepted.len(),
            candidates: self.archive.len() - self.accepted.len(),
            truncated: self.truncated.clone(),
            failures: self.failures.clone(),
            halt_reason: self.halt.as_ref().map(|(_, m)| m.clone()),
            // Citation-following is only a full second method once it has run to closure; before that it has read
            // the neighbourhoods of some papers and not others, and its overlap with search measures the budget.
            coverage: if stop == Stop::InProgress {
                Coverage::Unmeasured("the run is still going")
            } else if self.update {
                Coverage::Unmeasured("an update reads only what is new; the estimate belongs to a full run")
            } else if stop == Stop::Closed {
                coverage::chapman(s, c, both)
            } else {
                Coverage::Unmeasured("the run did not close, so citation-following is incomplete")
            },
            by_search: s,
            by_citation: c,
            by_both: both,
            update: self.update,
        }
    }

    /// Read the references and citers of `pending`. False when the budget ran out first.
    fn expand(&mut self, pending: &[(Key, Reason)], indexes: &mut [&mut dyn Index]) -> bool {
        // Backward: every reference not yet in the archive, looked up in one batch per index.
        let missing: BTreeSet<Id> = pending
            .iter()
            .flat_map(|(k, _)| self.archive.get(*k).references.iter())
            .filter(|id| self.archive.find(id).is_none())
            .cloned()
            .collect();
        let missing: Vec<Id> = missing.into_iter().collect();
        if !missing.is_empty() {
            for i in indexes.iter_mut().filter(|i| missing.iter().any(|id| i.resolves(id))) {
                if self.dropped.contains(&i.source()) {
                    continue;
                }
                if !self.spend() {
                    return false;
                }
                match i.resolve(&missing) {
                    Ok(ws) => self.take(ws, Found::Reference),
                    Err(e) => self.note(i.source(), i.has_citations(), format!("{} references of {} papers", i.source().name(), pending.len()), e),
                }
                if self.halted() {
                    return false;
                }
            }
        }
        // Papers already in the archive that an accepted paper cites were reached by reference as well.
        for (k, _) in pending {
            let targets: Vec<Key> = self.archive.get(*k).references.iter().filter_map(|id| self.archive.find(id)).collect();
            for t in targets {
                self.found.entry(t).or_default().insert(Found::Reference);
            }
        }
        // Forward.
        for &(k, reason) in pending {
            if reason.follow_citers() {
                for i in indexes.iter_mut().filter(|i| i.has_citations()) {
                    if !self.spend() {
                        return false;
                    }
                    let work = self.archive.get(k).clone();
                    match i.citing(&work, self.policy.citing_limit) {
                        Ok(Some(ws)) => {
                            if let Some(total) = work.cited_by_count
                                && ws.len() as u64 >= self.policy.citing_limit as u64 && total > ws.len() as u64 {
                                    self.truncated.push((k, total));
                                }
                            self.take(ws, Found::Citation);
                        }
                        Ok(None) => {}
                        Err(e) => self.note(i.source(), true, format!("{} citers of {:?}", i.source().name(), work.title), e),
                    }
                    if self.halted() {
                        // This paper's citers were not read: it stays unexpanded.
                        return false;
                    }
                }
            }
            self.expanded.insert(k);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny citation graph held in memory. Paper i is OpenAlex id W{i}.
    struct Graph {
        works: Vec<Work>,
        calls: usize,
    }

    fn counted(mut w: Work, n: u64) -> Work {
        w.cited_by_count = Some(n);
        w
    }

    fn w(i: usize, title: &str, abs: &str, refs: &[usize]) -> Work {
        Work {
            ids: [Id::OpenAlex(format!("W{i}"))].into(),
            title: title.into(),
            abstract_text: Some(abs.into()),
            references: refs.iter().map(|r| Id::OpenAlex(format!("W{r}"))).collect(),
            seen_in: [Source::OpenAlex].into(),
            ..Work::default()
        }
    }

    impl Index for Graph {
        fn source(&self) -> Source {
            Source::OpenAlex
        }
        fn search(&mut self, query: &str, limit: usize) -> Result<Vec<Work>, Error> {
            self.calls += 1;
            let q = query.to_lowercase();
            Ok(self.works.iter().filter(|w| w.title.to_lowercase().contains(&q)).take(limit).cloned().collect())
        }
        fn citing(&mut self, work: &Work, limit: usize) -> Result<Option<Vec<Work>>, Error> {
            self.calls += 1;
            Ok(Some(self.works.iter().filter(|c| c.references.iter().any(|r| work.ids.contains(r))).take(limit).cloned().collect()))
        }
        fn resolve(&mut self, ids: &[Id]) -> Result<Vec<Work>, Error> {
            self.calls += 1;
            Ok(self.works.iter().filter(|w| w.ids.iter().any(|i| ids.contains(i))).cloned().collect())
        }
    }

    fn topic() -> Topic {
        Topic { name: "t".into(), queries: vec!["low-rank adaptation".into()], phrases: vec!["low rank".into(), "adapter".into(), "fine tuning".into()] }
    }

    /// 0: found by search. 1: an old paper in other words, cited by 0 and 2. 2: reached only as a citer of 0.
    /// 3: a generic method paper cited by 0 and 2, and by 150,000 papers in all: not specific, rejected.
    /// 4: cites 3 only, off-subject: must not be reached. 5: cites 0, off-subject: reached, not accepted.
    fn graph() -> Graph {
        Graph {
            works: vec![
                w(0, "Low-rank adaptation of language models", "low rank adapter fine tuning", &[1, 3]),
                counted(w(1, "Factorised weight updates", "approximating a matrix by products of thin matrices", &[]), 12),
                w(2, "Quantized adapters", "an adapter for fine tuning in four bits", &[0, 1, 3]),
                counted(w(3, "A stochastic optimiser", "gradient methods", &[]), 150_000),
                w(4, "Protein folding", "uses the optimiser", &[3]),
                w(5, "A survey of weather", "mentions it once", &[0]),
            ],
            calls: 0,
        }
    }

    #[test]
    fn closure_reaches_old_vocabulary_without_drifting() {
        let mut g = graph();
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let r = s.run(&mut [&mut g]);
        assert_eq!(r.stop, Stop::Closed);
        let ids: BTreeMap<String, Reason> = s.accepted().map(|(_, w, r)| (w.ids.iter().next().unwrap().value().to_string(), r)).collect();
        assert_eq!(ids.get("W0"), Some(&Reason::Text { phrases: 3 }));
        assert_eq!(ids.get("W2"), Some(&Reason::Text { phrases: 2 }));
        assert_eq!(ids.get("W1"), Some(&Reason::CitedBy { links: 2 }));
        assert!(!ids.contains_key("W3"), "a classic every field cites was accepted");
        assert!(!ids.contains_key("W4"), "the method paper's citers were followed");
        assert!(!ids.contains_key("W5"));
        assert_eq!(r.accepted, 3);
        // Search found W0; citation-following found W0 again (cited by W2) plus W1 and W2.
        assert_eq!((r.by_search, r.by_citation, r.by_both), (1, 3, 1));
        assert!(r.failures.is_empty());
    }

    #[test]
    fn link_evidence_comes_from_the_core_only_and_must_be_a_real_share() {
        let mut works = graph().works;
        // 6: cites two core papers among twenty references and mentions one phrase: a passing citation, rejected.
        let mut refs: Vec<usize> = (100..118).collect();
        refs.extend([0, 2]);
        works.push(w(6, "A broad survey", "one adapter among many methods", &refs));
        // 7: cited by W1 and W6 only (neither is core), with a small total: link-accepted papers do not vouch.
        works.push(counted(w(7, "Products of thin matrices", "approximation theory", &[]), 3));
        works[1].references.insert(Id::OpenAlex("W7".into()));
        works[6].references.insert(Id::OpenAlex("W7".into()));
        let mut g = Graph { works, calls: 0 };
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let r = s.run(&mut [&mut g]);
        assert_eq!(r.stop, Stop::Closed);
        let ids: BTreeSet<String> = s.accepted().map(|(_, w, _)| w.ids.iter().next().unwrap().value().to_string()).collect();
        assert_eq!(ids, ["W0", "W1", "W2"].iter().map(|s| s.to_string()).collect());
    }

    #[test]
    fn spellings_of_one_concept_count_once() {
        let t = Topic { name: "q".into(), queries: vec![], phrases: vec!["quantization|quantized".into(), "neural network".into()] };
        let s = Snowball::new(t.clone(), Policy::for_topic(&t));
        let audio = Work { title: "Quantized audio".into(), abstract_text: Some("a quantization scheme".into()), ..Work::default() };
        let nets = Work { title: "Quantized neural network weights".into(), ..Work::default() };
        assert_eq!(s.phrases_in(&audio), 1);
        assert_eq!(s.phrases_in(&nets), 2);
    }

    #[test]
    fn budget_exhaustion_is_reported_not_called_closure() {
        let mut g = graph();
        let mut p = Policy::for_topic(&topic());
        p.max_calls = 2;
        let mut s = Snowball::new(topic(), p);
        let r = s.run(&mut [&mut g]);
        assert_eq!(r.stop, Stop::Budget);
        assert!(g.calls <= 2);
        assert!(matches!(r.coverage, Coverage::Unmeasured(_)), "an unclosed run must not estimate the gap");
    }

    #[test]
    fn papers_reached_before_the_budget_ran_out_are_judged() {
        // W0 is found by search and cites W6, an on-subject paper search did not find. The budget allows the
        // search and the reference lookup, then runs out on W0's citers.
        let mut g = Graph {
            works: vec![
                w(0, "Low-rank adaptation of language models", "low rank adapter fine tuning", &[6]),
                w(6, "Earlier work", "a low rank adapter", &[]),
            ],
            calls: 0,
        };
        let mut p = Policy::for_topic(&topic());
        p.max_calls = 2;
        let mut s = Snowball::new(topic(), p);
        let r = s.run(&mut [&mut g]);
        assert_eq!(r.stop, Stop::Budget);
        assert_eq!(r.accepted, 2);
        assert_eq!(r.rounds.last().unwrap().newly_accepted, 1);
    }

    struct Broken;
    impl Index for Broken {
        fn source(&self) -> Source {
            Source::Arxiv
        }
        fn search(&mut self, _: &str, _: usize) -> Result<Vec<Work>, Error> {
            Err(Error::Http("503".into()))
        }
        fn citing(&mut self, _: &Work, _: usize) -> Result<Option<Vec<Work>>, Error> {
            Ok(None)
        }
        fn resolve(&mut self, _: &[Id]) -> Result<Vec<Work>, Error> {
            Err(Error::Http("503".into()))
        }
    }

    /// An index with no citation graph and no lookup for OpenAlex ids: the run must never spend a call on it
    /// after its search.
    struct SearchOnly;
    impl Index for SearchOnly {
        fn source(&self) -> Source {
            Source::Arxiv
        }
        fn search(&mut self, _: &str, _: usize) -> Result<Vec<Work>, Error> {
            Ok(vec![])
        }
        fn citing(&mut self, _: &Work, _: usize) -> Result<Option<Vec<Work>>, Error> {
            panic!("asked a search-only index for citers")
        }
        fn has_citations(&self) -> bool {
            false
        }
        fn resolve(&mut self, _: &[Id]) -> Result<Vec<Work>, Error> {
            panic!("asked an index to resolve ids it cannot")
        }
        fn resolves(&self, id: &Id) -> bool {
            matches!(id, Id::Arxiv(_))
        }
    }

    #[test]
    fn calls_are_spent_only_where_an_index_can_answer() {
        let mut g = graph();
        let mut a = SearchOnly;
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let r = s.run(&mut [&mut g, &mut a]);
        assert_eq!(r.stop, Stop::Closed);
        // One search each, then only the graph's calls.
        assert_eq!(r.calls, 1 + g.calls);
    }

    /// The graph, but its quota runs out after `left` calls.
    struct Quota {
        g: Graph,
        left: usize,
    }
    impl Quota {
        fn take(&mut self) -> Result<(), Error> {
            if self.left == 0 {
                return Err(Error::RateLimited("quota spent".into()));
            }
            self.left -= 1;
            Ok(())
        }
    }
    impl Index for Quota {
        fn source(&self) -> Source {
            Source::OpenAlex
        }
        fn search(&mut self, q: &str, l: usize) -> Result<Vec<Work>, Error> {
            self.take()?;
            self.g.search(q, l)
        }
        fn citing(&mut self, w: &Work, l: usize) -> Result<Option<Vec<Work>>, Error> {
            self.take()?;
            self.g.citing(w, l)
        }
        fn resolve(&mut self, ids: &[Id]) -> Result<Vec<Work>, Error> {
            self.take()?;
            self.g.resolve(ids)
        }
    }

    #[test]
    fn a_spent_quota_stops_the_run_without_calling_it_closed() {
        // Search and the reference lookup succeed; W0's citers are refused.
        let mut q = Quota { g: graph(), left: 2 };
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let r = s.run(&mut [&mut q]);
        assert_eq!(r.stop, Stop::Throttled);
        assert!(matches!(r.coverage, Coverage::Unmeasured(_)));
        // The refused call was the last one attempted.
        assert_eq!(q.g.calls, 2);
        assert_eq!(r.calls, 3);
        assert!(r.failures.iter().any(|f| f.contains("rate limited")));
        // W0's citers were never read, so W0 is still owed an expansion.
        let w0 = s.archive.find(&Id::OpenAlex("W0".into())).unwrap();
        assert!(!s.expanded.contains(&w0));
    }

    #[test]
    fn a_run_stopped_during_search_is_not_closed() {
        let mut g = graph();
        let cancel = Arc::new(AtomicBool::new(true));
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        s.cancel = Some(cancel);
        let r = s.run(&mut [&mut g]);
        assert_eq!(r.stop, Stop::Cancelled);
        assert_eq!(g.calls, 0);
        assert!(matches!(r.coverage, Coverage::Unmeasured(_)));

        let mut g = graph();
        let mut p = Policy::for_topic(&topic());
        p.max_calls = 0;
        let mut s = Snowball::new(topic(), p);
        assert_eq!(s.run(&mut [&mut g]).stop, Stop::Budget);
    }

    #[test]
    fn a_refused_key_halts_the_run() {
        struct Refuses;
        impl Index for Refuses {
            fn source(&self) -> Source {
                Source::OpenAlex
            }
            fn search(&mut self, _: &str, _: usize) -> Result<Vec<Work>, Error> {
                Err(Error::KeyRefused("401".into()))
            }
            fn citing(&mut self, _: &Work, _: usize) -> Result<Option<Vec<Work>>, Error> {
                unreachable!()
            }
            fn resolve(&mut self, _: &[Id]) -> Result<Vec<Work>, Error> {
                unreachable!()
            }
        }
        let mut t = topic();
        t.queries.push("second query".into());
        let mut s = Snowball::new(t.clone(), Policy::for_topic(&t));
        let r = s.run(&mut [&mut Refuses]);
        assert_eq!(r.stop, Stop::KeyRefused);
        assert_eq!(r.calls, 1, "kept calling with a refused key");
        assert_eq!(r.halt_reason.as_deref(), Some("401"));
    }

    #[test]
    fn an_update_reads_only_what_is_new_and_expands_only_what_it_accepts() {
        // The saved subject: W0 (core) and W1 (cited by it). Since then W2 appeared, citing W0, on the subject.
        let mut g = graph();
        let saved = vec![(g.works[0].clone(), Reason::Text { phrases: 3 }), (g.works[1].clone(), Reason::CitedBy { links: 2 })];
        let mut t = topic();
        t.queries.clear();
        let mut s = Snowball::resume(t.clone(), Policy::for_topic(&t), saved);
        let w0 = s.archive.find(&Id::OpenAlex("W0".into())).unwrap();
        s.seed_citers = vec![w0];
        let r = s.run(&mut [&mut g]);
        assert!(r.update);
        assert!(matches!(r.coverage, Coverage::Unmeasured(_)));
        let ids: BTreeSet<String> = s.accepted().map(|(_, w, _)| w.ids.iter().next().unwrap().value().to_string()).collect();
        assert!(ids.contains("W2"), "{ids:?}");
        // One call for W0's citers, then W2's references (all known) and citers: the saved papers were not re-read.
        assert!(g.calls <= 3, "{} calls", g.calls);
        for text in ["text:2", "cites:3:1", "cited_by:4", "peripheral:2"] {
            assert_eq!(crate::store::reason_text(Reason::parse(text).unwrap()), text);
        }
        assert_eq!(Reason::parse("nonsense"), None);
    }

    #[test]
    fn each_round_that_read_anything_is_offered_as_a_checkpoint() {
        let mut g = graph();
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let mut seen = Vec::new();
        let r = s.run_with(&mut [&mut g], |run, report| seen.push((report.stop, report.accepted, run.archive.len())));
        assert_eq!(r.stop, Stop::Closed);
        assert!(seen.len() >= 2, "{seen:?}");
        assert!(seen.iter().all(|(stop, _, _)| *stop == Stop::InProgress));
        // Checkpoints grow with the run, and the last one already holds what the run ends with.
        assert!(seen.windows(2).all(|w| w[0].1 <= w[1].1));
        assert_eq!(seen.last().unwrap().1, r.accepted);
    }

    #[test]
    fn a_round_is_announced_only_after_its_checkpoint() {
        use std::sync::Mutex;
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let mut g = graph();
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let o = order.clone();
        s.on_round = Some(Box::new(move |_, _| o.lock().unwrap().push("announced")));
        let o = order.clone();
        s.run_with(&mut [&mut g], move |_, _| o.lock().unwrap().push("saved"));
        let order = order.lock().unwrap();
        // Every announcement that follows a read comes right after a save.
        for (i, e) in order.iter().enumerate() {
            if *e == "announced" && i > 0 {
                assert_eq!(order[i - 1], "saved", "{order:?}");
            }
        }
        assert!(order.contains(&"saved"));
    }

    #[test]
    fn a_throttled_search_only_index_is_dropped_and_the_run_goes_on() {
        struct Throttled(usize);
        impl Index for Throttled {
            fn source(&self) -> Source {
                Source::Arxiv
            }
            fn search(&mut self, _: &str, _: usize) -> Result<Vec<Work>, Error> {
                self.0 += 1;
                Err(Error::RateLimited("export.arxiv.org asks to wait".into()))
            }
            fn citing(&mut self, _: &Work, _: usize) -> Result<Option<Vec<Work>>, Error> {
                Ok(None)
            }
            fn has_citations(&self) -> bool {
                false
            }
            fn resolve(&mut self, _: &[Id]) -> Result<Vec<Work>, Error> {
                self.0 += 1;
                Ok(vec![])
            }
            fn resolves(&self, id: &Id) -> bool {
                matches!(id, Id::Arxiv(_))
            }
        }
        let mut t = topic();
        t.queries.push("a second query".into());
        let mut g = graph();
        let mut a = Throttled(0);
        let mut s = Snowball::new(t.clone(), Policy::for_topic(&t));
        let r = s.run(&mut [&mut g, &mut a]);
        assert_eq!(r.stop, Stop::Closed, "{:?}", r.failures);
        assert_eq!(r.accepted, 3);
        assert_eq!(a.0, 1, "asked once, then dropped");
        assert!(r.failures.iter().any(|f| f.contains("not asked again")));
        assert_eq!(r.halt_reason, None);
    }

    #[test]
    fn a_large_core_needs_more_citers_for_acceptance_on_links() {
        // 600 core papers (on text); paper X is cited by 2 of them, paper Y by 3, both specific (few citations in all).
        let mut works = Vec::new();
        for i in 0..600 {
            let refs: Vec<usize> = match i {
                0 | 1 => vec![9001],
                2..=4 => vec![9002],
                _ => vec![],
            };
            works.push(w(i, &format!("Low-rank adapter study {i}"), "low rank adapter fine tuning", &refs));
        }
        works.push(counted(w(9001, "X", "other words", &[]), 4));
        works.push(counted(w(9002, "Y", "other words", &[]), 4));
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        for wk in works {
            s.archive.insert(wk);
        }
        s.judge();
        let accepted = |id: usize| s.archive.find(&Id::OpenAlex(format!("W{id}"))).and_then(|k| s.reason(k));
        // 600 / 250 rounds up to 3 citers needed.
        assert_eq!(accepted(9001), Some(Reason::Peripheral { links: 2 }), "kept, but on the edge");
        assert_eq!(accepted(9002), Some(Reason::CitedBy { links: 3 }));
        // In a small core, 2 still suffice (the existing test above keeps W1 with 2).
    }

    #[test]
    fn near_the_subject_in_its_own_words_keeps_the_floor_in_a_large_core() {
        let t = Topic { name: "q".into(), queries: vec![], phrases: vec!["quantization|quantized".into(), "neural network".into()] };
        let mut s = Snowball::new(t.clone(), Policy::for_topic(&t));
        for i in 0..600usize {
            let refs: Vec<usize> = if i < 2 { vec![9001, 9002] } else { vec![] };
            s.archive.insert(w(i, &format!("Quantized neural network {i}"), "quantization of a neural network", &refs));
        }
        s.archive.insert(counted(w(9001, "Optimum quantizer design", "minimum distortion", &[]), 30));
        s.archive.insert(counted(w(9002, "Diffusion coefficients of binary gas mixtures", "chromatography", &[]), 30));
        s.judge();
        let reason = |id: usize| s.archive.find(&Id::OpenAlex(format!("W{id}"))).and_then(|k| s.reason(k));
        assert_eq!(reason(9001), Some(Reason::CitedBy { links: 2 }), "a quantizer is near quantization");
        assert_eq!(reason(9002), Some(Reason::Peripheral { links: 2 }), "kept on the edge: few citers, other words");
    }

    #[test]
    fn a_failing_index_is_named_and_the_run_continues() {
        let mut g = graph();
        let mut b = Broken;
        let mut s = Snowball::new(topic(), Policy::for_topic(&topic()));
        let r = s.run(&mut [&mut g, &mut b]);
        assert_eq!(r.stop, Stop::Closed);
        assert_eq!(r.accepted, 3);
        assert!(r.failures.iter().any(|f| f.starts_with("arxiv search")), "{:?}", r.failures);
    }
}
