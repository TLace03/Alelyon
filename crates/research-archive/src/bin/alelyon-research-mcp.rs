//! The research archive as a read-only MCP server on stdio.
//!
//!   alelyon-research-mcp [--db <archive.sqlite>] [--allow-findings]
//!
//! `--allow-findings` offers `save_finding`, which appends an agent's conclusion about a gap beside it (and writes
//! nothing else).
//!
//! Without --db: CENTCOM_RESEARCH_DB, else ~/.alelyon/research/archive.sqlite. CENTCOM's own executable serves the
//! same thing with `centcom --research-mcp`.

fn main() {
    let mut args = std::env::args().skip(1);
    let mut db = alelyon_research::mcp::default_db();
    let mut findings = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => db = args.next().map(std::path::PathBuf::from).unwrap_or_else(|| usage()),
            "--allow-findings" => findings = true,
            _ => usage(),
        }
    }
    let stdin = std::io::stdin();
    if let Err(e) = alelyon_research::mcp::serve_with(&db, findings, stdin.lock(), std::io::stdout().lock()) {
        eprintln!("alelyon-research-mcp: {e}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: alelyon-research-mcp [--db <archive.sqlite>] [--allow-findings]");
    std::process::exit(2);
}
