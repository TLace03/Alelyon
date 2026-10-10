//! Re-judge a saved archive under the current policy, offline: no index is called.
//!
//!   cargo run --release --example regate -- <db.sqlite> <name> --phrase "<p>"... [--core-per-link N] [--list out.tsv]

use std::collections::BTreeMap;
use std::path::PathBuf;

use alelyon_research::snowball::{Policy, Reason, Snowball, Topic};
use alelyon_research::store::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(name)) = (args.next(), args.next()) else {
        eprintln!("usage: regate <db.sqlite> <name> --phrase P...");
        std::process::exit(2);
    };
    let mut topic = Topic { name, queries: vec![], phrases: vec![] };
    let mut core_per_link = None;
    let mut list: Option<String> = None;
    while let Some(flag) = args.next() {
        let v = args.next().expect("flag without a value");
        match flag.as_str() {
            "--phrase" => topic.phrases.push(v),
            "--list" => list = Some(v),
            "--core-per-link" => core_per_link = Some(v.parse::<usize>().expect("--core-per-link takes a number")),
            other => panic!("unknown flag {other}"),
        }
    }
    let store = Store::open(&PathBuf::from(db)).expect("open archive");
    let works = store.all_works().expect("read archive");
    let mut policy = Policy::for_topic(&topic);
    if let Some(n) = core_per_link {
        policy.core_per_link = n;
    }
    let mut run = Snowball::new(topic.clone(), policy);
    for w in works {
        run.archive.insert(w);
    }
    let t = std::time::Instant::now();
    run.judge();
    let mut by: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, r) in run.accepted() {
        *by.entry(match r {
            Reason::Text { .. } => "text",
            Reason::Cites { .. } => "cites",
            Reason::CitedBy { .. } => "cited_by",
            Reason::Peripheral { .. } => "peripheral",
        })
        .or_default() += 1;
    }
    println!("archived {}, accepted {} ({:?}) in {:.1} s", run.archive.len(), run.accepted().count(), by, t.elapsed().as_secs_f64());
    let mut linked: Vec<_> = run.accepted().filter(|(_, _, r)| !matches!(r, Reason::Text { .. })).collect();
    linked.sort_by_key(|(_, w, _)| w.year.unwrap_or(9999));
    if let Some(path) = &list {
        let mut out = String::new();
        for (_, w, r) in run.accepted() {
            out.push_str(&format!("{:?}	{}	{}
", r, w.year.map_or("????".into(), |y| y.to_string()), w.title));
        }
        std::fs::write(path, out).expect("write the list");
    }
    println!("accepted on links, oldest first:");
    for (_, w, r) in linked.iter().take(30) {
        println!("  {} {:?} cited_by_count={:?} {}", w.year.map_or("????".into(), |y| y.to_string()), r, w.cited_by_count, w.title);
    }
}
