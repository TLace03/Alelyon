//! Update a saved subject with what is new, live, and report what it added.
//!
//!   cargo run --release --features net --example update -- <db.sqlite> <subject> [--max-calls N]
//!
//! Reads OPENALEX_API_KEY from the environment. Sends the subject's searches and paper identifiers to
//! api.openalex.org and export.arxiv.org.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use alelyon_research::harvest::{date_of, update};
use alelyon_research::store::{Stamp, Store};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(name)) = (args.next(), args.next()) else {
        eprintln!("usage: update <db.sqlite> <subject> [--max-calls N]");
        std::process::exit(2);
    };
    let mut max_calls = 300;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--max-calls" => max_calls = args.next().and_then(|v| v.parse().ok()).expect("--max-calls takes a number"),
            other => panic!("unknown flag {other}"),
        }
    }
    let db = PathBuf::from(db);
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let now = Stamp { text: format!("{} (update)", date_of(secs)), secs };
    let before = Store::open(&db).expect("open").members(&name).expect("members").len();
    let t = std::time::Instant::now();
    let hook: alelyon_research::snowball::RoundHook =
        Box::new(|r, calls| eprintln!("round: +{} accepted, {} accepted, {} read, {calls} calls", r.newly_accepted, r.accepted_total, r.archive_size));
    let report = update(&db, &name, max_calls, std::env::var("OPENALEX_API_KEY").ok(), Arc::new(AtomicBool::new(false)), Some(hook), &now)
        .expect("update");
    println!("stop: {:?} after {} calls, {:.0} s; members {before} -> {}", report.stop, report.calls, t.elapsed().as_secs_f64(), report.accepted);
    for f in &report.failures {
        println!("failure: {f}");
    }
    let store = Store::open(&db).expect("open");
    let fresh = store.new_papers(Some(&name), secs, 30).expect("new");
    println!("\nnew in this update ({}):", fresh.len());
    for n in fresh.iter().take(20) {
        println!("  {} [{}] {}", n.paper.year.map_or("????".into(), |y| y.to_string()), n.reason, n.paper.title);
    }
}
