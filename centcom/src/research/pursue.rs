//! Handing a gap to Sinai (a person has Sinai start working on a gap the research feature makes apparent, in one or
//! two clicks).
//!
//! One click on a gap writes a brief and sends it to whoever answers on Sinai's page (`App::hand_to_sinai`): Sinai's
//! loop, which takes typed text as a request of its own and works it as a task, or the person's own model. The brief
//! carries everything the gap rests on (its headline, the evidence, the papers with their addresses) because neither
//! answerer can read the archive itself; it is kept short, for a small model's context.

use alelyon_research::store::{PaperRef, StoredGap};

/// At most this many papers go into a brief.
pub const BRIEF_PAPERS: usize = 6;

/// A paper's web address from its identifiers as the store gives them ("doi:10.1/x", "arxiv:2106.09685",
/// "openalex:W1"): its DOI first, then arXiv, then OpenAlex.
pub fn address(p: &PaperRef) -> Option<String> {
    let of = |scheme: &str| p.ids.iter().find_map(|i| i.split_once(':').filter(|(s, v)| *s == scheme && !v.is_empty()).map(|(_, v)| v.to_string()));
    of("doi")
        .map(|d| format!("https://doi.org/{d}"))
        .or_else(|| of("arxiv").map(|a| format!("https://arxiv.org/abs/{a}")))
        .or_else(|| of("openalex").map(|w| format!("https://openalex.org/{w}")))
}

/// A finding's source as an address to open: the source itself when it is one, else the address in its closing
/// brackets ("A page (https://...)", as Sinai's web notes name their sources); None for anything else.
pub fn source_link(source: &str) -> Option<String> {
    let s = source.trim();
    let web = |u: &str| (u.starts_with("https://") || u.starts_with("http://")).then(|| u.to_string());
    web(s).or_else(|| s.strip_suffix(')').and_then(|t| t.rfind('(').map(|i| &t[i + 1..])).and_then(web))
}

/// What Sinai is asked to do with a gap: check it is real, and if it is, propose a next step.
pub fn brief(g: &StoredGap) -> String {
    let mut out = format!(
        "Please work on a research gap from my archive, in my subject \"{}\".\n\nThe gap: {}\nWhy it looks like one: {}\n",
        g.subject, g.headline, g.detail
    );
    let terms: Vec<&str> = g.terms.iter().map(String::as_str).filter(|t| !t.is_empty()).collect();
    if !terms.is_empty() {
        out.push_str(&format!("Its key words: {}.\n", terms.join(", ")));
    }
    if !g.works.is_empty() {
        out.push_str("\nPapers it rests on:\n");
        for p in g.works.iter().take(BRIEF_PAPERS) {
            let year = p.year.map_or(String::new(), |y| format!("{y} "));
            match address(p) {
                Some(a) => out.push_str(&format!("- {year}\"{}\" {a}\n", p.title)),
                None => out.push_str(&format!("- {year}\"{}\"\n", p.title)),
            }
        }
    }
    out.push_str(
        "\nWhat I need:\n\
         1. Read the papers above (search for a title if its address does not open).\n\
         2. Search for recent work that already closes this gap.\n\
         3. Tell me whether the gap is real. If it is, propose one concrete next step: an experiment, two methods to \
         combine, or a question to answer, and what it would need.\n\
         The archive raised this from counts of papers and citations, so it is a lead, not a finding. Keep notes as you \
         go and report back here.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alelyon_research::gaps::Kind;

    fn paper(title: &str, ids: &[&str]) -> PaperRef {
        PaperRef { work: 1, title: title.into(), year: Some(2014), ids: ids.iter().map(|s| s.to_string()).collect() }
    }

    fn gap(works: Vec<PaperRef>) -> StoredGap {
        StoredGap {
            subject: "Low-bit weights".into(),
            kind: Kind::Forgotten,
            rank: 0,
            score: 1.0,
            headline: "2014: \"Predicting parameters in deep learning\" is no longer cited".into(),
            detail: "Cited by 202 papers of the subject.".into(),
            terms: vec!["low".into(), "".into(), "rank".into()],
            works,
            threads: vec![],
            dismissed: false,
            pursued: None,
            findings: vec![],
        }
    }

    #[test]
    fn a_sources_link_is_the_source_or_its_bracketed_address() {
        assert_eq!(source_link("https://doi.org/10.1/x").as_deref(), Some("https://doi.org/10.1/x"));
        assert_eq!(source_link("A page (https://arxiv.org/abs/1)").as_deref(), Some("https://arxiv.org/abs/1"));
        assert_eq!(source_link("A page (file:///C:/x.exe)"), None);
        assert_eq!(source_link("no address"), None);
    }

    #[test]
    fn a_paper_opens_at_its_doi_then_arxiv_then_openalex() {
        assert_eq!(address(&paper("a", &["openalex:W1", "arxiv:2106.09685", "doi:10.1/x"])).as_deref(), Some("https://doi.org/10.1/x"));
        assert_eq!(address(&paper("a", &["openalex:W1", "arxiv:2106.09685"])).as_deref(), Some("https://arxiv.org/abs/2106.09685"));
        assert_eq!(address(&paper("a", &["openalex:W1"])).as_deref(), Some("https://openalex.org/W1"));
        assert_eq!(address(&paper("a", &["isbn:1", "doi:"])), None);
    }

    #[test]
    fn the_brief_carries_the_gap_its_papers_and_the_ask() {
        let mut works: Vec<PaperRef> = (0..9).map(|i| paper(&format!("Paper {i}"), &["doi:10.1/x"])).collect();
        works.push(paper("No address", &[]));
        let b = brief(&gap(works));
        assert!(b.contains("in my subject \"Low-bit weights\""));
        assert!(b.contains("is no longer cited") && b.contains("Cited by 202 papers"));
        assert!(b.contains("Its key words: low, rank."), "empty terms are left out");
        assert!(b.contains("- 2014 \"Paper 0\" https://doi.org/10.1/x"));
        assert_eq!(b.matches("\n- ").count(), BRIEF_PAPERS, "only the first papers go in");
        assert!(b.contains("Tell me whether the gap is real"));
        let none = brief(&gap(vec![]));
        assert!(!none.contains("Papers it rests on"));
    }
}
