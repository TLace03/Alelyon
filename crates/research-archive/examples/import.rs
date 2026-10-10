//! Copy one subject from an archive into another, offline.
//!
//!   cargo run --release --example import -- <from.sqlite> <into.sqlite> <subject>

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use alelyon_research::store::{Stamp, Store};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [from, into, name] = args.as_slice() else {
        eprintln!("usage: import <from.sqlite> <into.sqlite> <subject>");
        std::process::exit(2);
    };
    let from = Store::open_read_only(&PathBuf::from(from)).expect("open the source archive");
    let mut into = Store::open(&PathBuf::from(into)).expect("open the destination archive");
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let now = Stamp { text: alelyon_research::harvest_date::date_of(secs) + " (imported)", secs };
    match into.import_subject(&from, name, &now) {
        Ok(n) => {
            let t = into.topics().expect("topics").into_iter().find(|t| &t.name == name).expect("the imported subject");
            println!("imported {n} papers into {name:?}; threads {}, gaps {}", into.communities(name).expect("threads").len(), into.gaps(Some(name)).expect("gaps").len());
            println!("last run: {:?}", t.last.map(|r| (r.stop, r.accepted, r.missing)));
        }
        Err(e) => {
            eprintln!("import refused: {e}");
            std::process::exit(1);
        }
    }
}
