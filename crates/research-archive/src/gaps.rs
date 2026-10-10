//! Where a subject's literature may have gaps worth pursuing, read from its knowledge graph and its papers' text alone
//! (no request is made). Every gap is a lead with the counts that raised it, not a finding: a pair of threads that
//! rarely cite each other may be unrelated after all, and an untried combination may be untried because it fails.
//!
//! - **Threads that rarely talk**: two threads whose papers share much of their vocabulary but almost never cite each
//!   other: methods or results in one may not have reached the other.
//! - **Forgotten foundations**: influential older papers of the subject that its recent papers have stopped citing.
//! - **Untried combinations**: two concepts that are each common in the subject, appear together far less often than
//!   their frequencies predict, and share many neighbouring concepts (Swanson's literature-based discovery: if A
//!   relates to B and B to C, A–C may be an open question).
//! - **Young threads**: lines of work that are mostly recent and small: little prior art, open questions likely.
//! - **Stalled threads**: lines of work with no paper for years: settled, abandoned, or waiting for a new tool.
//!
//! A sixth kind, works many of the subject's papers cite that the archive does not hold (a blind spot of the
//! gathering, not of the research), is found by the store, which holds the reference lists (store.rs).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::graph::{EdgeKind, Graph, NOT_CONNECTED};
use crate::ident;

/// One paper as gap-finding reads it; `papers[i]` is `graph.nodes[i]`.
#[derive(Debug, Clone)]
pub struct Text {
    pub title: String,
    pub abstract_text: Option<String>,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Unbridged,
    Forgotten,
    Combination,
    Young,
    Stalled,
    BlindSpot,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Unbridged => "unbridged",
            Kind::Forgotten => "forgotten",
            Kind::Combination => "combination",
            Kind::Young => "young",
            Kind::Stalled => "stalled",
            Kind::BlindSpot => "blind_spot",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        [Kind::Unbridged, Kind::Forgotten, Kind::Combination, Kind::Young, Kind::Stalled, Kind::BlindSpot].into_iter().find(|k| k.name() == s)
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Unbridged => "Threads that rarely talk",
            Kind::Forgotten => "Forgotten foundations",
            Kind::Combination => "Untried combinations",
            Kind::Young => "Young threads",
            Kind::Stalled => "Stalled threads",
            Kind::BlindSpot => "Blind spots in the gathering",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gap {
    pub kind: Kind,
    /// Comparable within a kind; higher is stronger.
    pub score: f64,
    /// One line a person reads first.
    pub headline: String,
    /// The evidence, in words, with its counts.
    pub detail: String,
    /// Words a new subject pursuing this gap could search for.
    pub terms: Vec<String>,
    /// Papers that show the gap (archive rows), most telling first.
    pub works: Vec<i64>,
    pub threads: Vec<usize>,
}

const STOP: &[&str] = &[
    "a", "about", "above", "after", "all", "also", "an", "and", "any", "are", "as", "at", "be", "been", "being", "between",
    "both", "but", "by", "can", "could", "do", "does", "each", "for", "from", "further", "has", "have", "here", "how", "however",
    "if", "in", "into", "is", "it", "its", "may", "more", "most", "much", "new", "no", "not", "of", "on", "one", "only", "or",
    "other", "our", "over", "paper", "propose", "proposed", "results", "same", "show", "shows", "so", "such", "than", "that",
    "the", "their", "them", "then", "there", "these", "they", "this", "those", "through", "to", "two", "under", "up", "use",
    "used", "using", "via", "was", "we", "were", "what", "when", "which", "while", "with", "without", "within", "would",
    "based", "approach", "method", "methods", "work", "study", "present", "presents", "existing", "different", "several",
    "well", "first", "three", "high", "low", "large", "small", "significantly", "achieve", "achieves", "performance",
    "compared", "result", "provide", "provides", "art", "state", "experimental", "demonstrate", "et", "al", "i", "e", "g",
];

fn words(t: &Text) -> HashSet<String> {
    let text = format!("{} {}", t.title, t.abstract_text.as_deref().unwrap_or(""));
    ident::title_fingerprint(&text)
        .split(' ')
        .filter(|w| w.len() > 2 && !STOP.contains(w) && !w.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

pub fn find(papers: &[Text], g: &Graph) -> Vec<Gap> {
    let n = papers.len().min(g.nodes.len());
    if n < 10 {
        return Vec::new();
    }
    let docs: Vec<HashSet<String>> = papers[..n].iter().map(words).collect();
    let mut out = Vec::new();
    out.extend(unbridged(papers, g, &docs));
    out.extend(forgotten(papers, g));
    out.extend(combinations(&docs, g));
    out.extend(young_and_stalled(papers, g));
    out
}

fn real_threads(g: &Graph, min: usize) -> Vec<usize> {
    g.communities.iter().filter(|c| c.label != NOT_CONNECTED && c.size >= min).map(|c| c.id).collect()
}

fn thread_label(g: &Graph, c: usize) -> &str {
    g.communities.iter().find(|x| x.id == c).map_or("", |x| x.label.as_str())
}

fn top_in(g: &Graph, c: usize, k: usize) -> Vec<i64> {
    let mut v: Vec<&_> = g.nodes.iter().filter(|n| n.community == c).collect();
    v.sort_by(|a, b| b.pagerank.total_cmp(&a.pagerank).then(a.work.cmp(&b.work)));
    v.into_iter().take(k).map(|n| n.work).collect()
}

fn unbridged(_papers: &[Text], g: &Graph, docs: &[HashSet<String>]) -> Vec<Gap> {
    let threads = real_threads(g, 6);
    if threads.len() < 2 {
        return Vec::new();
    }
    let n = docs.len();
    // Each thread's vocabulary: document frequency of each word inside the thread, weighted by its rarity in the
    // subject (tf-idf over threads), as a unit vector.
    let mut df: HashMap<&str, f64> = HashMap::new();
    for d in docs {
        for w in d {
            *df.entry(w.as_str()).or_default() += 1.0;
        }
    }
    let mut vectors: HashMap<usize, HashMap<&str, f64>> = HashMap::new();
    let mut sizes: HashMap<usize, f64> = HashMap::new();
    for (i, d) in docs.iter().enumerate() {
        let c = g.nodes[i].community;
        *sizes.entry(c).or_default() += 1.0;
        let v = vectors.entry(c).or_default();
        for w in d {
            *v.entry(w.as_str()).or_default() += 1.0;
        }
    }
    for (c, v) in vectors.iter_mut() {
        let size = sizes[c];
        for (w, f) in v.iter_mut() {
            // Words in fewer than three papers of the thread say little about it.
            *f = if *f < 3.0 { 0.0 } else { (*f / size) * (n as f64 / df[w]).ln() };
        }
        let norm = v.values().map(|x| x * x).sum::<f64>().sqrt().max(1e-12);
        v.values_mut().for_each(|x| *x /= norm);
    }
    // Citations between each pair of threads, either direction.
    let mut cross: HashMap<(usize, usize), usize> = HashMap::new();
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Cites) {
        let (a, b) = (g.nodes[e.a].community, g.nodes[e.b].community);
        if a != b {
            *cross.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for (x, &a) in threads.iter().enumerate() {
        for &b in &threads[x + 1..] {
            let (va, vb) = (&vectors[&a], &vectors[&b]);
            let sim: f64 = va.iter().filter_map(|(w, x)| vb.get(w).map(|y| x * y)).sum();
            let links = cross.get(&(a, b)).copied().unwrap_or(0);
            let pairs = sizes[&a] * sizes[&b];
            let per_thousand = links as f64 * 1000.0 / pairs;
            if sim < 0.15 || per_thousand > 2.0 {
                continue;
            }
            let mut shared: Vec<(f64, &str)> = va.iter().filter_map(|(w, x)| vb.get(w).map(|y| (x * y, *w))).filter(|(s, _)| *s > 0.0).collect();
            shared.sort_by(|p, q| q.0.total_cmp(&p.0).then(p.1.cmp(q.1)));
            let shared: Vec<String> = shared.into_iter().take(6).map(|(_, w)| w.to_string()).collect();
            let (la, lb) = (thread_label(g, a), thread_label(g, b));
            out.push(Gap {
                kind: Kind::Unbridged,
                score: sim / (1.0 + per_thousand),
                headline: format!("\"{la}\" and \"{lb}\" share vocabulary but rarely cite each other"),
                detail: format!(
                    "{} and {} papers; {links} citations between them ({per_thousand:.2} per thousand possible pairs); vocabulary \
                     similarity {sim:.2}. Words both use: {}. Methods or results in one may not have reached the other.",
                    sizes[&a] as usize,
                    sizes[&b] as usize,
                    shared.join(", ")
                ),
                terms: la.split(", ").chain(lb.split(", ")).map(str::to_string).collect(),
                works: top_in(g, a, 3).into_iter().chain(top_in(g, b, 3)).collect(),
                threads: vec![a, b],
            });
        }
    }
    out.sort_by(|p, q| q.score.total_cmp(&p.score));
    out.truncate(8);
    out
}

fn forgotten(papers: &[Text], g: &Graph) -> Vec<Gap> {
    let newest = papers.iter().filter_map(|p| p.year).max().unwrap_or(0);
    let recent_from = newest - 4;
    let n = g.nodes.len().min(papers.len());
    // Who cites whom, with the citer's year.
    let mut citers: Vec<Vec<Option<i32>>> = vec![Vec::new(); n];
    for e in g.edges.iter().filter(|e| e.kind == EdgeKind::Cites) {
        if e.a < n && e.b < n {
            citers[e.b].push(papers[e.a].year);
        }
    }
    let recent_papers = papers.iter().filter(|p| p.year.is_some_and(|y| y >= recent_from)).count() as f64;
    let recent_share = recent_papers / n as f64;
    let mut out = Vec::new();
    for i in 0..n {
        let Some(y) = papers[i].year else { continue };
        if y > newest - 12 || citers[i].len() < 4 {
            continue;
        }
        let recent = citers[i].iter().filter(|c| c.is_some_and(|c| c >= recent_from)).count();
        let share = recent as f64 / citers[i].len() as f64;
        // Recent papers are a large part of most subjects; a foundation they no longer cite stands out.
        if share > recent_share * 0.35 {
            continue;
        }
        let last = citers[i].iter().flatten().max().copied();
        out.push(Gap {
            kind: Kind::Forgotten,
            score: g.nodes[i].pagerank * citers[i].len() as f64 * (1.0 - share),
            headline: format!("{y}: \"{}\" is no longer cited", ui_cut(&papers[i].title, 90)),
            detail: format!(
                "Cited by {} papers of the subject, {recent} of them from {recent_from} on (the subject's papers from then are {:.0}% of \
                 it); last cited {}. Ideas in it may be worth re-reading against today's methods.",
                citers[i].len(),
                recent_share * 100.0,
                last.map_or("never by a dated paper".into(), |l| format!("in {l}"))
            ),
            terms: Vec::new(),
            works: vec![g.nodes[i].work],
            threads: vec![g.nodes[i].community],
        });
    }
    out.sort_by(|p, q| q.score.total_cmp(&p.score));
    out.truncate(10);
    out
}

fn combinations(docs: &[HashSet<String>], g: &Graph) -> Vec<Gap> {
    let n = docs.len() as f64;
    let mut df: HashMap<&str, usize> = HashMap::new();
    for d in docs {
        for w in d {
            *df.entry(w.as_str()).or_default() += 1;
        }
    }
    // Concepts: words in 2% to 15% of the subject's papers (common enough to matter, rare enough to be specific), the
    // 250 most frequent of them.
    let mut terms: Vec<(&str, usize)> = df.iter().map(|(w, &f)| (*w, f)).filter(|(_, f)| *f as f64 >= 0.02 * n && *f as f64 <= 0.15 * n && *f >= 5).collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    terms.truncate(250);
    let k = terms.len();
    if k < 3 {
        return Vec::new();
    }
    let index: HashMap<&str, usize> = terms.iter().enumerate().map(|(i, (w, _))| (*w, i)).collect();
    let mut co = vec![vec![0u32; k]; k];
    let mut holders: Vec<Vec<usize>> = vec![Vec::new(); k];
    for (p, d) in docs.iter().enumerate() {
        let ids: Vec<usize> = d.iter().filter_map(|w| index.get(w.as_str()).copied()).collect();
        for &a in &ids {
            holders[a].push(p);
            for &b in &ids {
                if a != b {
                    co[a][b] += 1;
                }
            }
        }
    }
    // Neighbours: concepts that co-occur at least twice as often as chance.
    let lift = |a: usize, b: usize| co[a][b] as f64 * n / (terms[a].1 as f64 * terms[b].1 as f64);
    let neighbours: Vec<HashSet<usize>> = (0..k).map(|a| (0..k).filter(|&b| b != a && co[a][b] >= 3 && lift(a, b) >= 2.0).collect()).collect();
    let mut out = Vec::new();
    for a in 0..k {
        for c in a + 1..k {
            let expected = terms[a].1 as f64 * terms[c].1 as f64 / n;
            let observed = co[a][c] as f64;
            if expected < 4.0 || observed > expected * 0.25 {
                continue;
            }
            let bridges: Vec<usize> = neighbours[a].intersection(&neighbours[c]).copied().collect();
            if bridges.len() < 3 {
                continue;
            }
            let mut via: Vec<&str> = bridges.iter().map(|&b| terms[b].0).collect();
            via.sort_unstable();
            via.truncate(6);
            // The papers that come closest: those with both, else the most influential with either.
            let both: Vec<usize> = holders[a].iter().filter(|p| holders[c].contains(p)).copied().collect();
            let mut example: Vec<usize> = if both.is_empty() { holders[a].iter().take(2).chain(holders[c].iter().take(2)).copied().collect() } else { both };
            example.sort_by(|&x, &y| g.nodes[y].pagerank.total_cmp(&g.nodes[x].pagerank));
            out.push(Gap {
                kind: Kind::Combination,
                score: (expected - observed) * bridges.len() as f64 / expected.sqrt(),
                headline: format!("\"{}\" with \"{}\": rarely studied together", terms[a].0, terms[c].0),
                detail: format!(
                    "{} papers mention \"{}\" and {} mention \"{}\"; by chance about {expected:.0} would mention both, and {observed:.0} do. \
                     Both go with: {}.",
                    terms[a].1,
                    terms[a].0,
                    terms[c].1,
                    terms[c].0,
                    via.join(", ")
                ),
                terms: vec![terms[a].0.to_string(), terms[c].0.to_string()],
                works: example.into_iter().take(4).map(|p| g.nodes[p].work).collect(),
                threads: Vec::new(),
            });
        }
    }
    out.sort_by(|p, q| q.score.total_cmp(&p.score).then(p.headline.cmp(&q.headline)));
    out.truncate(10);
    out
}

fn young_and_stalled(papers: &[Text], g: &Graph) -> Vec<Gap> {
    let newest = papers.iter().filter_map(|p| p.year).max().unwrap_or(0);
    let mut years: BTreeMap<usize, Vec<i32>> = BTreeMap::new();
    for (i, node) in g.nodes.iter().enumerate().take(papers.len()) {
        if let Some(y) = papers[i].year {
            years.entry(node.community).or_default().push(y);
        }
    }
    let mut out = Vec::new();
    for c in real_threads(g, 5) {
        let Some(ys) = years.get(&c) else { continue };
        let recent = ys.iter().filter(|&&y| y >= newest - 2).count();
        let last = ys.iter().max().copied().unwrap_or(0);
        let label = thread_label(g, c);
        let size = ys.len();
        if size <= 60 && recent * 2 >= size {
            out.push(Gap {
                kind: Kind::Young,
                score: recent as f64 / size as f64 * (size as f64).ln_1p(),
                headline: format!("\"{label}\" is young: {recent} of its {size} papers are from {} on", newest - 2),
                detail: "A small line of work that is mostly recent: little prior art, so open questions are likely.".into(),
                terms: label.split(", ").map(str::to_string).collect(),
                works: top_in(g, c, 4),
                threads: vec![c],
            });
        } else if last <= newest - 5 {
            out.push(Gap {
                kind: Kind::Stalled,
                score: (newest - last) as f64 * (size as f64).ln_1p(),
                headline: format!("\"{label}\" has had no paper since {last}"),
                detail: format!(
                    "{size} papers, the last from {last}. Settled, abandoned, or waiting for a tool that now exists: worth a look before \
                     assuming it is closed."
                ),
                terms: label.split(", ").map(str::to_string).collect(),
                works: top_in(g, c, 4),
                threads: vec![c],
            });
        }
    }
    out.sort_by(|p, q| q.score.total_cmp(&p.score));
    out
}

fn ui_cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Input, build};
    use crate::ident::Id;

    fn input(i: i64, title: &str, year: i32, refs: &[i64]) -> Input {
        Input { work: i, title: title.into(), year: Some(year), ids: vec![Id::OpenAlex(format!("W{i}"))], references: refs.iter().map(|r| Id::OpenAlex(format!("W{r}"))).collect() }
    }

    /// Two threads using the same words (binary, weights, hardware) that never cite each other, plus an old paper
    /// the early papers cite and recent ones do not.
    fn subject() -> (Vec<Input>, Vec<Text>) {
        let mut inputs = vec![input(1, "Binary weights foundations", 1990, &[])];
        let mut texts = vec![Text { title: "Binary weights foundations".into(), abstract_text: Some("binary weights hardware theory".into()), year: Some(1990) }];
        for i in 0..12 {
            let id = 10 + i;
            let refs: Vec<i64> = if i < 6 { vec![1, 10 + (i + 11) % 12] } else { vec![10 + (i + 11) % 12, 10 + (i + 10) % 12] };
            let year = if i < 6 { 1995 + i as i32 } else { 2022 + (i as i32 % 4) };
            inputs.push(input(id, &format!("Binary weights hardware accelerator {i}"), year, &refs));
            texts.push(Text { title: format!("Binary weights hardware accelerator {i}"), abstract_text: Some("binary weights hardware energy accelerator memory".into()), year: Some(year) });
        }
        for i in 0..12 {
            let id = 30 + i;
            let refs = vec![30 + (i + 11) % 12, 30 + (i + 10) % 12];
            inputs.push(input(id, &format!("Binary weights hardware spiking {i}"), 2023 + (i as i32 % 3), &refs));
            texts.push(Text { title: format!("Binary weights hardware spiking {i}"), abstract_text: Some("binary weights hardware energy spiking neuromorphic".into()), year: Some(2023 + (i as i32 % 3)) });
        }
        // An unrelated thread, so the two threads' shared words are distinctive of them rather than of everything.
        for i in 0..12 {
            let id = 50 + i;
            let refs = vec![50 + (i + 11) % 12, 50 + (i + 10) % 12];
            inputs.push(input(id, &format!("Speech recognition audio {i}"), 2010 + i as i32, &refs));
            texts.push(Text { title: format!("Speech recognition audio {i}"), abstract_text: Some("speech recognition audio acoustic".into()), year: Some(2010 + i as i32) });
        }
        (inputs, texts)
    }

    #[test]
    fn two_threads_with_one_vocabulary_and_no_citations_are_a_gap() {
        let (inputs, texts) = subject();
        let g = build(&inputs);
        let gaps = find(&texts, &g);
        let u: Vec<&Gap> = gaps.iter().filter(|x| x.kind == Kind::Unbridged).collect();
        assert!(!u.is_empty(), "{gaps:#?}");
        assert!(u[0].detail.contains("0 citations between them"), "{}", u[0].detail);
        assert_eq!(u[0].threads.len(), 2);
    }

    #[test]
    fn an_old_paper_recent_work_stopped_citing_is_forgotten() {
        let (inputs, texts) = subject();
        let g = build(&inputs);
        let gaps = find(&texts, &g);
        let f: Vec<&Gap> = gaps.iter().filter(|x| x.kind == Kind::Forgotten).collect();
        assert_eq!(f.len(), 1, "{gaps:#?}");
        assert_eq!(f[0].works, vec![1]);
        assert!(f[0].headline.starts_with("1990"));
    }

    #[test]
    fn too_small_a_subject_reports_no_gaps_rather_than_noise() {
        let g = build(&[input(1, "a", 2000, &[]), input(2, "b", 2001, &[1])]);
        assert!(find(&[Text { title: "a".into(), abstract_text: None, year: Some(2000) }, Text { title: "b".into(), abstract_text: None, year: Some(2001) }], &g).is_empty());
    }

    #[test]
    fn kinds_round_trip() {
        for k in [Kind::Unbridged, Kind::Forgotten, Kind::Combination, Kind::Young, Kind::Stalled, Kind::BlindSpot] {
            assert_eq!(Kind::parse(k.name()), Some(k));
        }
    }
}
