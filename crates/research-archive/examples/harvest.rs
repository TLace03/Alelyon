//! Harvest one subject live and save it.
//!
//!   cargo run --features net --example harvest -- <db.sqlite> <name> --query "<q>"... --phrase "<a|b|c>"... [--max-calls N]
//!
//! Each --phrase is one concept; separate its spellings with |.
//!
//! Sends the queries (and nothing else about the user) to api.openalex.org and export.arxiv.org.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use alelyon_research::arxiv::Arxiv;
use alelyon_research::coverage::Coverage;
use alelyon_research::index::Index;
use alelyon_research::net::HttpFetch;
use alelyon_research::openalex::OpenAlex;
use alelyon_research::snowball::{Found, Policy, Snowball, Stop, Topic};
use alelyon_research::store::{Store, reason_text};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(name)) = (args.next(), args.next()) else {
        eprintln!("usage: harvest <db.sqlite> <name> --query Q... --phrase P... [--max-calls N]");
        std::process::exit(2);
    };
    let mut topic = Topic { name, queries: vec![], phrases: vec![] };
    let mut max_calls = None;
    while let Some(flag) = args.next() {
        let v = args.next().expect("flag without a value");
        match flag.as_str() {
            "--query" => topic.queries.push(v),
            "--phrase" => topic.phrases.push(v),
            "--max-calls" => max_calls = Some(v.parse().expect("--max-calls takes a number")),
            other => panic!("unknown flag {other}"),
        }
    }
    let mut policy = Policy::for_topic(&topic);
    if let Some(n) = max_calls {
        policy.max_calls = n;
    }
    let started = std::time::Instant::now();
    let key = std::env::var("OPENALEX_API_KEY").ok();
    let mut oa = OpenAlex::new(HttpFetch::new().expect("http client").with_bearer("api.openalex.org", key));
    let mut ax = Arxiv::new(HttpFetch::new().expect("http client"));
    let mut run = Snowball::new(topic, policy);
    let t0 = std::time::Instant::now();
    run.on_round = Some(Box::new(move |r, calls| {
        eprintln!("[{:>5.0} s] round: +{} accepted, {} accepted, {} archived, {calls} calls", t0.elapsed().as_secs_f64(), r.newly_accepted, r.accepted_total, r.archive_size)
    }));
    let report = run.run(&mut [&mut oa as &mut dyn Index, &mut ax]);

    println!("stop: {:?} after {} index calls, {:.0} s", report.stop, report.calls, started.elapsed().as_secs_f64());
    for (i, r) in report.rounds.iter().enumerate() {
        println!("  round {i}: +{} accepted, {} accepted, {} archived", r.newly_accepted, r.accepted_total, r.archive_size);
    }
    println!("accepted {} of {} archived", report.accepted, report.accepted + report.candidates);
    println!("found by search {}, by citation {}, by both {}", report.by_search, report.by_citation, report.by_both);
    match report.coverage {
        Coverage::Estimated { estimated_missing, missing_95, .. } => {
            println!("estimated relevant papers not found: {estimated_missing:.0} (95% {:.0}..{:.0}; assumes independent methods, so a floor)", missing_95.0, missing_95.1)
        }
        Coverage::Unmeasured(why) => println!("estimated relevant papers not found: UNMEASURED ({why})"),
    }
    if report.stop != Stop::Closed {
        println!("NOT CLOSED: accepted papers remain unexpanded; the estimate covers an incomplete run");
    }
    println!("citer lists truncated: {}", report.truncated.len());
    for f in &report.failures {
        println!("failure: {f}");
    }
    let only_citation: Vec<_> = run
        .accepted()
        .filter(|(k, _, _)| run.found(*k).is_some_and(|f| !f.iter().any(|x| matches!(x, Found::Search(_)))))
        .collect();
    println!("\naccepted papers keyword search did NOT find ({}), oldest first:", only_citation.len());
    let mut sorted = only_citation;
    sorted.sort_by_key(|(_, w, _)| w.year.unwrap_or(9999));
    for (_, w, r) in sorted.iter().take(25) {
        println!("  {} [{}] {}", w.year.map(|y| y.to_string()).unwrap_or("????".into()), reason_text(*r), w.title);
    }

    let now = alelyon_research::store::Stamp {
        text: utc_now(),
        secs: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
    };
    let mut store = Store::open(&PathBuf::from(db)).expect("open archive");
    store.save(&run, &report, &now).expect("save");
    store.rebuild_graph(&run.topic.name).expect("graph");
}

/// Now as "2026-10-08 10:35:12Z" (civil date from days since 1970, Howard Hinnant's algorithm).
fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}
