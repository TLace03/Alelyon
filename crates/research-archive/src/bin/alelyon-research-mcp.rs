//! The research archive as an MCP server on stdio, and what agents keep from it.
//!
//!   alelyon-research-mcp [--db <archive.sqlite>] [--allow-findings] [--read-only]
//!   alelyon-research-mcp [--db <archive.sqlite>] --ask "<question>" [--subject <name>]
//!
//! Agents ask and add to the knowledge kept beside the archive (`knowledge.sqlite`); `--read-only` offers only the
//! tools that read. `--allow-findings` offers `save_finding`, which appends an agent's conclusion about a gap beside
//! it. `--ask` answers one question as JSON on stdout and exits, for agents that run commands.
//!
//! Without --db: CENTCOM_RESEARCH_DB, else ~/.alelyon/research/archive.sqlite. CENTCOM's own executable serves the
//! same thing with `centcom --research-mcp`.

fn main() {
    let mut args = std::env::args().skip(1);
    let mut db = alelyon_research::mcp::default_db();
    let mut findings = false;
    let mut knowledge = true;
    let mut ask: Option<String> = None;
    let mut subject: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--db" => db = args.next().map(std::path::PathBuf::from).unwrap_or_else(|| usage()),
            "--allow-findings" => findings = true,
            "--read-only" => knowledge = false,
            "--ask" => ask = Some(args.next().unwrap_or_else(|| usage())),
            "--subject" => subject = Some(args.next().unwrap_or_else(|| usage())),
            _ => usage(),
        }
    }
    if let Some(question) = ask {
        match alelyon_research::mcp::ask_once(&db, &question, subject.as_deref()) {
            Ok(answer) => println!("{}", serde_json::to_string_pretty(&answer).unwrap_or_default()),
            Err(why) => {
                eprintln!("alelyon-research-mcp: {why}");
                std::process::exit(1);
            }
        }
        return;
    }
    let stdin = std::io::stdin();
    if let Err(e) = alelyon_research::mcp::serve_options(&db, findings, knowledge, stdin.lock(), std::io::stdout().lock()) {
        eprintln!("alelyon-research-mcp: {e}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: alelyon-research-mcp [--db <archive.sqlite>] [--allow-findings] [--read-only] | --ask <question> [--subject <name>]");
    std::process::exit(2);
}
