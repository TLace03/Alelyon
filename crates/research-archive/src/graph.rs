//! A subject's knowledge graph: how its papers connect, built from the archive alone (no request is made).
//!
//! - **Citations**: paper A cites paper B, both in the subject.
//! - **Similarity**: two papers that cite the same works (bibliographic coupling) or are cited by the same papers
//!   (co-citation) work on related questions even when neither cites the other. The weight is Salton's cosine: shared
//!   links over the geometric mean of the two papers' link counts. A work cited by very many of the subject's papers
//!   says little about any pair of them and is skipped as a link (`MAX_FAN`).
//! - **Threads**: communities found by Louvain modularity optimisation over citations and similarity, each named by the
//!   words its titles use more than the subject's other titles do.
//! - **Influence**: PageRank over citations within the subject.
//! - **Roles**: *foundation* (the most influential), *bridge* (connections spread across several threads), *frontier*
//!   (recent, building on the subject, not yet cited by it), or none.
//! - **Layout**: threads placed by how strongly they connect, each paper inside its thread with the most influential
//!   at its centre. Deterministic: the same archive draws the same map.
//!
//! Every quantity is computed from what the indexes said; a reference list an index lacks is a missing edge here.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ident::{self, Id};

/// A referenced work cited by more than this many of the subject's papers is not used as a similarity link.
pub const MAX_FAN: usize = 150;
/// Similar neighbours kept per paper.
pub const TOP_SIMILAR: usize = 8;
/// Least similarity kept.
pub const MIN_SIMILARITY: f64 = 0.1;
/// The label of the bucket holding papers linked to no other paper of the subject.
pub const NOT_CONNECTED: &str = "not connected to the rest";
const UNCONNECTED: usize = usize::MAX;

/// One paper as the graph needs it.
#[derive(Debug, Clone)]
pub struct Input {
    /// The archive's row id.
    pub work: i64,
    pub title: String,
    pub year: Option<i32>,
    pub ids: Vec<Id>,
    pub references: Vec<Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Foundation,
    Bridge,
    Frontier,
    Member,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Foundation => "foundation",
            Role::Bridge => "bridge",
            Role::Frontier => "frontier",
            Role::Member => "member",
        }
    }

    pub fn parse(s: &str) -> Role {
        match s {
            "foundation" => Role::Foundation,
            "bridge" => Role::Bridge,
            "frontier" => Role::Frontier,
            _ => Role::Member,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub work: i64,
    pub community: usize,
    pub pagerank: f64,
    pub role: Role,
    /// Papers of the subject this one cites, and that cite it.
    pub cites: usize,
    pub cited_by: usize,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Community {
    pub id: usize,
    pub label: String,
    pub size: usize,
    pub years: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `a` cites `b`.
    Cites,
    /// `a` and `b` are similar (undirected).
    Similar,
}

impl EdgeKind {
    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::Cites => "cites",
            EdgeKind::Similar => "similar",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Indexes into `Graph::nodes`.
    pub a: usize,
    pub b: usize,
    pub kind: EdgeKind,
    pub weight: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub communities: Vec<Community>,
}

pub fn build(papers: &[Input]) -> Graph {
    let n = papers.len();
    if n == 0 {
        return Graph::default();
    }
    // Which paper each identifier names.
    let mut by_id: HashMap<&Id, usize> = HashMap::new();
    for (i, p) in papers.iter().enumerate() {
        for id in &p.ids {
            by_id.entry(id).or_insert(i);
        }
    }
    // Citations within the subject, and every paper's reference list as a set of keys (in-subject index or the id).
    let mut cites: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let mut refs: Vec<HashSet<&Id>> = vec![HashSet::new(); n];
    for (i, p) in papers.iter().enumerate() {
        for r in &p.references {
            refs[i].insert(r);
            if let Some(&j) = by_id.get(r)
                && j != i {
                    cites[i].insert(j);
                }
        }
    }
    let mut cited_by: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (i, c) in cites.iter().enumerate() {
        for &j in c {
            cited_by[j].insert(i);
        }
    }

    let similar = similarity(&refs, &cites, &cited_by);
    let pagerank = pagerank(&cites);

    // Community detection over one undirected weighted graph: citations weigh 1, similarity its cosine.
    let mut adj: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); n];
    for (i, c) in cites.iter().enumerate() {
        for &j in c {
            *adj[i].entry(j).or_default() += 1.0;
            *adj[j].entry(i).or_default() += 1.0;
        }
    }
    for &(a, b, w) in &similar {
        *adj[a].entry(b).or_default() += w;
        *adj[b].entry(a).or_default() += w;
    }
    // Papers with no connection at all share one bucket at the end rather than each making a thread of one.
    let raw: Vec<usize> = louvain(&adj).into_iter().enumerate().map(|(i, c)| if adj[i].is_empty() { UNCONNECTED } else { c }).collect();
    // Renumber communities by size (largest first, the unconnected last), ties by smallest member, so ids are stable
    // for one archive.
    let mut sizes: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for (i, &c) in raw.iter().enumerate() {
        let e = sizes.entry(c).or_insert((0, i));
        e.0 += 1;
    }
    let mut order: Vec<(usize, usize, usize)> = sizes.into_iter().map(|(c, (size, first))| (c, size, first)).collect();
    order.sort_by(|a, b| (a.0 == UNCONNECTED).cmp(&(b.0 == UNCONNECTED)).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let renumber: HashMap<usize, usize> = order.iter().enumerate().map(|(new, &(old, _, _))| (old, new)).collect();
    let community: Vec<usize> = raw.iter().map(|c| renumber[c]).collect();
    let k = order.len();

    let communities = (0..k)
        .map(|c| {
            let members: Vec<usize> = (0..n).filter(|&i| community[i] == c).collect();
            let years: Vec<i32> = members.iter().filter_map(|&i| papers[i].year).collect();
            let isolated = order[c].0 == UNCONNECTED;
            Community {
                id: c,
                label: if isolated { NOT_CONNECTED.to_string() } else { label(papers, &community, c) },
                size: members.len(),
                years: (!years.is_empty()).then(|| (*years.iter().min().unwrap(), *years.iter().max().unwrap())),
            }
        })
        .collect::<Vec<_>>();

    let roles = roles(papers, &pagerank, &community, &adj, &cites, &cited_by);
    let (xs, ys) = layout(&community, k, &adj, &pagerank);

    let nodes = (0..n)
        .map(|i| Node {
            work: papers[i].work,
            community: community[i],
            pagerank: pagerank[i],
            role: roles[i],
            cites: cites[i].len(),
            cited_by: cited_by[i].len(),
            x: xs[i],
            y: ys[i],
        })
        .collect();
    let mut edges: Vec<Edge> = Vec::new();
    for (i, c) in cites.iter().enumerate() {
        let mut c: Vec<usize> = c.iter().copied().collect();
        c.sort_unstable();
        edges.extend(c.into_iter().map(|j| Edge { a: i, b: j, kind: EdgeKind::Cites, weight: 1.0 }));
    }
    edges.extend(similar.into_iter().map(|(a, b, w)| Edge { a, b, kind: EdgeKind::Similar, weight: w }));
    Graph { nodes, edges, communities }
}

/// Similar pairs (a < b) with their cosine, each paper keeping its `TOP_SIMILAR` strongest.
fn similarity(refs: &[HashSet<&Id>], _cites: &[HashSet<usize>], cited_by: &[HashSet<usize>]) -> Vec<(usize, usize, f64)> {
    let n = refs.len();
    let mut shared: HashMap<(usize, usize), f64> = HashMap::new();
    // Coupling: papers sharing a reference.
    let mut citers: HashMap<&Id, Vec<usize>> = HashMap::new();
    for (i, r) in refs.iter().enumerate() {
        for id in r {
            citers.entry(id).or_default().push(i);
        }
    }
    let mut add_pairs = |group: &[usize]| {
        if group.len() < 2 || group.len() > MAX_FAN {
            return;
        }
        for x in 0..group.len() {
            for y in x + 1..group.len() {
                let (a, b) = (group[x].min(group[y]), group[x].max(group[y]));
                *shared.entry((a, b)).or_default() += 1.0;
            }
        }
    };
    for group in citers.values() {
        add_pairs(group);
    }
    // Co-citation: papers cited together by one paper of the subject.
    let mut cited_together: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (j, by) in cited_by.iter().enumerate() {
        for &i in by {
            cited_together[i].push(j);
        }
    }
    for group in &mut cited_together {
        group.sort_unstable();
        add_pairs(group);
    }
    let degree: Vec<f64> = (0..n).map(|i| (refs[i].len() + cited_by[i].len()) as f64).collect();
    let mut best: Vec<Vec<(f64, usize)>> = vec![Vec::new(); n];
    for (&(a, b), &s) in &shared {
        let w = s / (degree[a] * degree[b]).sqrt().max(1.0);
        if w >= MIN_SIMILARITY {
            best[a].push((w, b));
            best[b].push((w, a));
        }
    }
    let mut keep: HashSet<(usize, usize)> = HashSet::new();
    for (i, list) in best.iter_mut().enumerate() {
        list.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        for &(_, j) in list.iter().take(TOP_SIMILAR) {
            keep.insert((i.min(j), i.max(j)));
        }
    }
    let mut out: Vec<(usize, usize, f64)> = keep
        .into_iter()
        .map(|(a, b)| {
            let s = shared[&(a, b)];
            (a, b, s / (degree[a] * degree[b]).sqrt().max(1.0))
        })
        .collect();
    out.sort_by_key(|x| (x.0, x.1));
    out
}

/// PageRank over citations (rank flows from a citing paper to what it cites), damping 0.85.
fn pagerank(cites: &[HashSet<usize>]) -> Vec<f64> {
    let n = cites.len();
    let mut r = vec![1.0 / n as f64; n];
    for _ in 0..60 {
        let dangling: f64 = (0..n).filter(|&i| cites[i].is_empty()).map(|i| r[i]).sum();
        let mut next = vec![(0.15 + 0.85 * dangling) / n as f64; n];
        for (i, c) in cites.iter().enumerate() {
            if !c.is_empty() {
                let share = 0.85 * r[i] / c.len() as f64;
                for &j in c {
                    next[j] += share;
                }
            }
        }
        let delta: f64 = r.iter().zip(&next).map(|(a, b)| (a - b).abs()).sum();
        r = next;
        if delta < 1e-10 {
            break;
        }
    }
    r
}

/// Louvain: move each node to the neighbouring community that most raises modularity, until none moves; fold each
/// community into one node and repeat while that changes anything. Nodes are visited in index order (deterministic).
fn louvain(adj: &[BTreeMap<usize, f64>]) -> Vec<usize> {
    let n = adj.len();
    let mut membership: Vec<usize> = (0..n).collect();
    let mut graph: Vec<Vec<(usize, f64)>> = adj.iter().map(|m| { let mut v: Vec<(usize, f64)> = m.iter().map(|(&j, &w)| (j, w)).collect(); v.sort_by_key(|e| e.0); v }).collect();
    for _level in 0..10 {
        let local = one_level(&graph);
        let k = local.iter().copied().max().map_or(0, |m| m + 1);
        if k == graph.len() {
            break;
        }
        for c in membership.iter_mut() {
            *c = local[*c];
        }
        let mut folded: Vec<BTreeMap<usize, f64>> = vec![BTreeMap::new(); k];
        for (i, edges) in graph.iter().enumerate() {
            for &(j, w) in edges {
                *folded[local[i]].entry(local[j]).or_default() += w;
            }
        }
        graph = folded.into_iter().map(|m| { let mut v: Vec<(usize, f64)> = m.into_iter().collect(); v.sort_by_key(|e| e.0); v }).collect();
    }
    membership
}

/// One level of Louvain; returns each node's community, numbered 0.. in order of first appearance.
fn one_level(graph: &[Vec<(usize, f64)>]) -> Vec<usize> {
    let n = graph.len();
    let degree: Vec<f64> = graph.iter().map(|e| e.iter().map(|x| x.1).sum()).collect();
    let m2: f64 = degree.iter().sum();
    let mut comm: Vec<usize> = (0..n).collect();
    if m2 == 0.0 {
        return comm;
    }
    let mut total: Vec<f64> = degree.clone();
    for _pass in 0..50 {
        let mut moved = false;
        for i in 0..n {
            let ci = comm[i];
            let mut links: BTreeMap<usize, f64> = BTreeMap::new();
            for &(j, w) in &graph[i] {
                if j != i {
                    *links.entry(comm[j]).or_default() += w;
                }
            }
            total[ci] -= degree[i];
            let gain = |c: usize, k_in: f64| k_in - total[c] * degree[i] / m2;
            let mut best = (ci, gain(ci, links.get(&ci).copied().unwrap_or(0.0)));
            for (&c, &k_in) in &links {
                let g = gain(c, k_in);
                if g > best.1 + 1e-12 {
                    best = (c, g);
                }
            }
            total[best.0] += degree[i];
            if best.0 != ci {
                comm[i] = best.0;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let mut renumber: HashMap<usize, usize> = HashMap::new();
    comm.iter().map(|&c| { let next = renumber.len(); *renumber.entry(c).or_insert(next) }).collect()
}

const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "based", "by", "for", "from", "in", "into", "is", "it", "its", "of", "on", "or",
    "the", "their", "to", "towards", "toward", "using", "via", "with", "without", "we", "new", "study", "approach", "method",
    "methods", "analysis", "use", "under", "through", "over", "two", "one", "large", "efficient", "learning", "network",
    "networks", "neural", "model", "models", "deep",
];

/// The three title words most distinctive of a community (frequency inside over frequency in the subject).
fn label(papers: &[Input], community: &[usize], c: usize) -> String {
    let mut inside: HashMap<String, f64> = HashMap::new();
    let mut everywhere: HashMap<String, f64> = HashMap::new();
    let (mut n_in, mut n_all) = (0.0, 0.0);
    for (i, p) in papers.iter().enumerate() {
        let words: HashSet<String> =
            ident::title_fingerprint(&p.title).split(' ').filter(|w| w.len() > 2 && !STOP.contains(w) && !w.chars().all(|c| c.is_ascii_digit())).map(str::to_string).collect();
        n_all += 1.0;
        let here = community[i] == c;
        if here {
            n_in += 1.0;
        }
        for w in words {
            *everywhere.entry(w.clone()).or_default() += 1.0;
            if here {
                *inside.entry(w).or_default() += 1.0;
            }
        }
    }
    if n_in == 0.0 {
        return String::new();
    }
    let mut scored: Vec<(f64, String)> = inside
        .into_iter()
        .filter(|(_, f)| *f >= 2.0_f64.min(n_in))
        .map(|(w, f)| {
            let lift = (f / n_in) / (everywhere[&w] / n_all);
            ((f / n_in) * lift.ln_1p(), w)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(3).map(|(_, w)| w).collect::<Vec<_>>().join(", ")
}

fn roles(
    papers: &[Input],
    pagerank: &[f64],
    community: &[usize],
    adj: &[BTreeMap<usize, f64>],
    cites: &[HashSet<usize>],
    cited_by: &[HashSet<usize>],
) -> Vec<Role> {
    let n = papers.len();
    // Foundations: the top 5% by influence (at least one, at most 50), and cited inside the subject at all.
    let mut by_rank: Vec<usize> = (0..n).collect();
    by_rank.sort_by(|&a, &b| pagerank[b].total_cmp(&pagerank[a]).then(a.cmp(&b)));
    let top = (n / 20).clamp(1, 50);
    let foundations: HashSet<usize> = by_rank.into_iter().filter(|&i| !cited_by[i].is_empty()).take(top).collect();
    let newest = papers.iter().filter_map(|p| p.year).max();
    // Bridges: papers whose connections are split between threads (participation coefficient, 1 minus the sum of
    // squared shares of their link weight per thread, at least 0.5, with at least two threads holding a fifth each),
    // ranked by that coefficient times the log of their link weight; the top 2% (at most 60).
    let mut bridge_score: Vec<(f64, usize)> = Vec::new();
    for (i, links) in adj.iter().enumerate() {
        let total: f64 = links.values().sum();
        if links.len() < 4 || total <= 0.0 || foundations.contains(&i) {
            continue;
        }
        let mut per: BTreeMap<usize, f64> = BTreeMap::new();
        for (&j, &w) in links {
            *per.entry(community[j]).or_default() += w;
        }
        let p = 1.0 - per.values().map(|w| (w / total).powi(2)).sum::<f64>();
        let strong = per.values().filter(|&&w| w / total >= 0.2).count();
        if p >= 0.5 && strong >= 2 {
            bridge_score.push((p * total.ln_1p(), i));
        }
    }
    bridge_score.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let bridges: HashSet<usize> = bridge_score.into_iter().take((n / 50).clamp(1, 60)).map(|(_, i)| i).collect();
    (0..n)
        .map(|i| {
            if foundations.contains(&i) {
                return Role::Foundation;
            }
            if bridges.contains(&i) {
                return Role::Bridge;
            }
            if let (Some(y), Some(newest)) = (papers[i].year, newest)
                && y >= newest - 1 && cited_by[i].is_empty() && cites[i].len() >= 3 {
                    return Role::Frontier;
                }
            Role::Member
        })
        .collect()
}

/// Positions in a unit square: threads placed by a small force layout over their connections, papers inside a
/// thread on a sunflower spiral, most influential at the centre.
fn layout(community: &[usize], k: usize, adj: &[BTreeMap<usize, f64>], pagerank: &[f64]) -> (Vec<f32>, Vec<f32>) {
    let n = community.len();
    let mut size = vec![0usize; k];
    for &c in community {
        size[c] += 1;
    }
    let mut link = vec![vec![0.0f64; k]; k];
    for (i, m) in adj.iter().enumerate() {
        for (&j, &w) in m {
            if community[i] != community[j] {
                link[community[i]][community[j]] += w;
            }
        }
    }
    let radius: Vec<f64> = size.iter().map(|&s| (s as f64).sqrt()).collect();
    // Start on a golden-angle spiral, largest thread at the centre; then relax.
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    let mut pos: Vec<(f64, f64)> = (0..k).map(|c| { let r = (c as f64).sqrt() * 4.0; (r * (c as f64 * golden).cos(), r * (c as f64 * golden).sin()) }).collect();
    for step in 0..300 {
        let cool = 1.0 - step as f64 / 300.0;
        let mut force = vec![(0.0, 0.0); k];
        for a in 0..k {
            for b in a + 1..k {
                let (dx, dy) = (pos[b].0 - pos[a].0, pos[b].1 - pos[a].1);
                let d = (dx * dx + dy * dy).sqrt().max(0.01);
                let gap = radius[a] + radius[b] + 1.0;
                // Repel when closer than their radii allow; attract in proportion to their links.
                let push = if d < gap { (gap - d) * 0.5 } else { 0.0 };
                let pull = (link[a][b] + link[b][a]).ln_1p() * 0.02 * (d - gap).max(0.0);
                let f = pull - push;
                let (ux, uy) = (dx / d, dy / d);
                force[a].0 += f * ux;
                force[a].1 += f * uy;
                force[b].0 -= f * ux;
                force[b].1 -= f * uy;
            }
            // A gentle pull to the centre keeps unconnected threads near.
            force[a].0 -= pos[a].0 * 0.01;
            force[a].1 -= pos[a].1 * 0.01;
        }
        for c in 0..k {
            pos[c].0 += force[c].0 * cool;
            pos[c].1 += force[c].1 * cool;
        }
    }
    // Papers inside their thread.
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); k];
    for i in 0..n {
        members[community[i]].push(i);
    }
    let mut xy = vec![(0.0f64, 0.0f64); n];
    for (c, list) in members.iter_mut().enumerate() {
        list.sort_by(|&a, &b| pagerank[b].total_cmp(&pagerank[a]).then(a.cmp(&b)));
        for (rank, &i) in list.iter().enumerate() {
            let r = (rank as f64 + 0.5).sqrt();
            let t = rank as f64 * golden;
            xy[i] = (pos[c].0 + r * t.cos() * 0.9, pos[c].1 + r * t.sin() * 0.9);
        }
    }
    // Into the unit square, keeping the aspect.
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for &(x, y) in &xy {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    }
    let span = (hi.0 - lo.0).max(hi.1 - lo.1).max(1e-9);
    let off = ((span - (hi.0 - lo.0)) / 2.0, (span - (hi.1 - lo.1)) / 2.0);
    let xs = xy.iter().map(|&(x, _)| ((x - lo.0 + off.0) / span) as f32).collect();
    let ys = xy.iter().map(|&(_, y)| ((y - lo.1 + off.1) / span) as f32).collect();
    (xs, ys)
}

/// The shortest chain of citations (either direction) from node `a` to node `b`, as node indexes, or `None`.
pub fn path(graph: &Graph, a: usize, b: usize) -> Option<Vec<usize>> {
    let n = graph.nodes.len();
    if a >= n || b >= n {
        return None;
    }
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &graph.edges {
        if e.kind == EdgeKind::Cites {
            next[e.a].push(e.b);
            next[e.b].push(e.a);
        }
    }
    let mut prev = vec![usize::MAX; n];
    let mut queue = std::collections::VecDeque::from([a]);
    prev[a] = a;
    while let Some(u) = queue.pop_front() {
        if u == b {
            let mut out = vec![b];
            let mut at = b;
            while at != a {
                at = prev[at];
                out.push(at);
            }
            out.reverse();
            return Some(out);
        }
        for &v in &next[u] {
            if prev[v] == usize::MAX {
                prev[v] = u;
                queue.push_back(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(i: i64, title: &str, year: i32, refs: &[&str]) -> Input {
        Input {
            work: i,
            title: title.into(),
            year: Some(year),
            ids: vec![Id::OpenAlex(format!("W{i}"))],
            references: refs.iter().map(|r| Id::OpenAlex(r.to_string())).collect(),
        }
    }

    /// Two threads: binary weights (1-4) and vector quantization (5-8), joined only by paper 9, which cites both.
    fn two_threads() -> Vec<Input> {
        vec![
            p(1, "Binary weights for perceptrons", 1990, &["W100", "W101"]),
            p(2, "Training binary weights networks", 1992, &["W1", "W100", "W101"]),
            p(3, "Binary weights in hardware", 1995, &["W1", "W2", "W100"]),
            p(4, "Binarized weights revisited", 2016, &["W1", "W2", "W3", "W101"]),
            p(5, "Vector quantization codebooks", 1980, &["W200", "W201"]),
            p(6, "Codebook design for vector quantization", 1984, &["W5", "W200", "W201"]),
            p(7, "Lattice vector quantization codebooks", 1990, &["W5", "W6", "W200"]),
            p(8, "Product quantization codebooks", 2011, &["W5", "W6", "W7", "W201"]),
            p(9, "Codebook binary weights", 2024, &["W4", "W8", "W3", "W7"]),
        ]
    }

    #[test]
    fn citations_inside_the_subject_become_edges() {
        let g = build(&two_threads());
        let cites: HashSet<(i64, i64)> =
            g.edges.iter().filter(|e| e.kind == EdgeKind::Cites).map(|e| (g.nodes[e.a].work, g.nodes[e.b].work)).collect();
        assert!(cites.contains(&(2, 1)) && cites.contains(&(9, 8)));
        assert!(!cites.iter().any(|&(_, b)| b >= 100), "references outside the subject are not nodes");
        assert_eq!(g.nodes[0].cited_by, 3);
    }

    #[test]
    fn two_threads_are_found_and_named() {
        let g = build(&two_threads());
        let c = |w: i64| g.nodes.iter().find(|n| n.work == w).unwrap().community;
        assert_eq!(c(1), c(2));
        assert_eq!(c(2), c(4));
        assert_eq!(c(5), c(8));
        assert_ne!(c(1), c(5));
        let label = |w: i64| &g.communities[c(w)].label;
        assert!(label(1).contains("binary") || label(1).contains("weights"), "{}", label(1));
        assert!(label(5).contains("codebooks") || label(5).contains("quantization"), "{}", label(5));
        assert_eq!(g.communities.iter().map(|c| c.size).sum::<usize>(), 9);
    }

    #[test]
    fn foundations_bridges_and_frontiers() {
        let g = build(&two_threads());
        let role = |w: i64| g.nodes.iter().find(|n| n.work == w).unwrap().role;
        assert!(role(1) == Role::Foundation || role(5) == Role::Foundation);
        // Paper 9 cites both threads equally: the bridge between them.
        assert_eq!(role(9), Role::Bridge);
        assert!(g.nodes.iter().all(|n| (0.0..=1.0).contains(&n.x) && (0.0..=1.0).contains(&n.y)));
        // A newest paper building on one thread only, cited by nothing yet, is its frontier.
        let mut more = two_threads();
        more.push(p(10, "Binary weights at the edge", 2024, &["W1", "W2", "W3"]));
        let g = build(&more);
        assert_eq!(g.nodes.iter().find(|n| n.work == 10).unwrap().role, Role::Frontier);
    }

    #[test]
    fn unconnected_papers_share_one_bucket_at_the_end() {
        let mut papers = two_threads();
        papers.push(p(10, "Alone", 2000, &[]));
        papers.push(p(11, "Also alone", 2001, &["W999"]));
        let g = build(&papers);
        let last = g.communities.last().unwrap();
        assert_eq!((last.label.as_str(), last.size), (NOT_CONNECTED, 2));
        assert_eq!(g.communities.len(), 3);
    }

    #[test]
    fn shared_references_make_papers_similar() {
        let g = build(&two_threads());
        let sim: HashSet<(i64, i64)> =
            g.edges.iter().filter(|e| e.kind == EdgeKind::Similar).map(|e| (g.nodes[e.a].work, g.nodes[e.b].work)).collect();
        assert!(sim.contains(&(1, 2)), "{sim:?}");
        assert!(!sim.contains(&(1, 6)));
    }

    #[test]
    fn the_path_between_threads_runs_through_the_bridge() {
        let g = build(&two_threads());
        let at = |w: i64| g.nodes.iter().position(|n| n.work == w).unwrap();
        let route: Vec<i64> = path(&g, at(1), at(5)).unwrap().into_iter().map(|i| g.nodes[i].work).collect();
        assert_eq!(route.first(), Some(&1));
        assert_eq!(route.last(), Some(&5));
        assert!(route.contains(&9), "{route:?}");
        let mut lone = two_threads();
        lone.push(p(10, "Unrelated", 2000, &[]));
        let g = build(&lone);
        assert_eq!(path(&g, 0, 9), None);
    }

    #[test]
    fn the_same_archive_draws_the_same_graph() {
        assert_eq!(build(&two_threads()), build(&two_threads()));
        assert_eq!(build(&[]), Graph::default());
    }
}
