//! Rebuild a saved subject's knowledge graph and describe it, offline.
//!
//!   cargo run --release --example graph -- <db.sqlite> <subject> [--json out.json] [--graphml out.graphml]

use std::path::PathBuf;

use alelyon_research::graph::Role;
use alelyon_research::store::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(topic)) = (args.next(), args.next()) else {
        eprintln!("usage: graph <db.sqlite> <subject> [--json out.json] [--graphml out.graphml]");
        std::process::exit(2);
    };
    let mut store = Store::open(&PathBuf::from(db)).expect("open archive");
    let t = std::time::Instant::now();
    let (nodes, edges, threads) = store.rebuild_graph(&topic).expect("rebuild");
    println!("{nodes} papers, {edges} edges, {threads} threads, built in {:.2} s", t.elapsed().as_secs_f64());
    for c in store.communities(&topic).expect("threads").iter().take(12) {
        let top = store.graph_nodes(&topic, Some(c.id), None, 2).expect("nodes");
        println!(
            "\nthread {} ({} papers, {}): {}",
            c.id,
            c.size,
            c.years.map(|y| format!("{}-{}", y.0, y.1)).unwrap_or_default(),
            c.label
        );
        for n in top {
            println!("    {} {}", n.paper.year.map_or("????".into(), |y| y.to_string()), n.paper.title);
        }
    }
    for role in [Role::Foundation, Role::Bridge, Role::Frontier] {
        let list = store.graph_nodes(&topic, None, Some(role), 6).expect("nodes");
        let total = store.graph_nodes(&topic, None, Some(role), usize::MAX).expect("nodes").len();
        println!("\n{} ({total}):", role.name());
        for n in list {
            println!("    {} [thread {}] {}", n.paper.year.map_or("????".into(), |y| y.to_string()), n.community, n.paper.title);
        }
    }
    let gaps = store.gaps(Some(&topic)).expect("gaps");
    println!("\n{} gaps:", gaps.len());
    let mut kind = None;
    for g in &gaps {
        if kind != Some(g.kind) {
            println!("  {}", g.kind.title());
            kind = Some(g.kind);
        }
        if g.rank < 4 {
            println!("    - {}\n      {}", g.headline, g.detail);
        }
    }
    while let Some(flag) = args.next() {
        let out = args.next().expect("a path");
        match flag.as_str() {
            "--json" => std::fs::write(&out, serde_json::to_string(&store.graph_json(&topic).expect("json")).unwrap()).expect("write"),
            "--graphml" => std::fs::write(&out, store.graph_ml(&topic).expect("graphml")).expect("write"),
            other => panic!("unknown flag {other}"),
        }
        println!("wrote {out}");
    }
}
