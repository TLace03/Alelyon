//! arXiv (https://arxiv.org) through its Atom query interface. arXiv holds preprints, often months before a journal
//! version, and has no citation graph; its value to the archive is a second, independent search.

use crate::ident::Id;
use crate::index::{Error, Fetch, Index, encode};
use crate::work::{Source, Work};

const BASE: &str = "https://export.arxiv.org/api/query";
/// arXiv asks for pages of at most 2000 and slices beyond 30000 results are refused; pages of 100 stay polite.
const PAGE: usize = 100;
const ATOM: &str = "http://www.w3.org/2005/Atom";
const ARXIV: &str = "http://arxiv.org/schemas/atom";

pub struct Arxiv<F: Fetch> {
    fetch: F,
    /// For an update: only papers submitted on or after this date (YYYYMMDD).
    since: Option<String>,
}

impl<F: Fetch> Arxiv<F> {
    pub fn new(fetch: F) -> Self {
        Arxiv { fetch, since: None }
    }

    /// Search only papers submitted on or after `date` (YYYY-MM-DD).
    pub fn with_since(mut self, date: Option<String>) -> Self {
        self.since = date.map(|d| d.replace('-', "")).filter(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()));
        self
    }
}

/// arXiv's query language: each word must appear in the title or abstract.
fn query_expr(query: &str) -> String {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .map(|w| format!("(ti:{w} OR abs:{w})"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

impl<F: Fetch> Index for Arxiv<F> {
    fn source(&self) -> Source {
        Source::Arxiv
    }

    fn search(&mut self, query: &str, limit: usize) -> Result<Vec<Work>, Error> {
        let mut expr = query_expr(query);
        if expr.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(d) = &self.since {
            expr = format!("({expr}) AND submittedDate:[{d}0000 TO 299912312359]");
        }
        let mut out = Vec::new();
        while out.len() < limit {
            let n = PAGE.min(limit - out.len());
            let url = format!("{BASE}?search_query={}&start={}&max_results={n}&sortBy=relevance", encode(&expr), out.len());
            let page = parse_feed(&self.fetch.get(&url)?)?;
            let done = page.len() < n;
            out.extend(page);
            if done {
                break;
            }
        }
        Ok(out)
    }

    fn citing(&mut self, _work: &Work, _limit: usize) -> Result<Option<Vec<Work>>, Error> {
        Ok(None)
    }

    fn has_citations(&self) -> bool {
        false
    }

    fn resolves(&self, id: &Id) -> bool {
        matches!(id, Id::Arxiv(_))
    }

    fn resolve(&mut self, ids: &[Id]) -> Result<Vec<Work>, Error> {
        let wanted: Vec<&str> = ids.iter().filter_map(|i| match i {
            Id::Arxiv(a) => Some(a.as_str()),
            _ => None,
        }).collect();
        let mut out = Vec::new();
        for chunk in wanted.chunks(PAGE) {
            let url = format!("{BASE}?id_list={}&max_results={}", encode(&chunk.join(",")), chunk.len());
            out.extend(parse_feed(&self.fetch.get(&url)?)?);
        }
        Ok(out)
    }
}

pub fn parse_feed(body: &str) -> Result<Vec<Work>, Error> {
    let doc = roxmltree::Document::parse(body).map_err(|e| Error::Parse(format!("arxiv: {e}")))?;
    let root = doc.root_element();
    if !root.has_tag_name((ATOM, "feed")) {
        return Err(Error::Parse("arxiv: not an Atom feed".into()));
    }
    let text = |n: roxmltree::Node, ns: &str, tag: &str| {
        n.children().find(|c| c.has_tag_name((ns, tag))).and_then(|c| c.text()).map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    let mut out = Vec::new();
    for e in root.children().filter(|c| c.has_tag_name((ATOM, "entry"))) {
        let Some(id) = text(e, ATOM, "id").as_deref().and_then(Id::arxiv) else {
            // arXiv reports query errors as an entry whose id is not a paper's.
            let msg = text(e, ATOM, "summary").unwrap_or_default();
            return Err(Error::Parse(format!("arxiv: {msg}")));
        };
        let mut w = Work { seen_in: [Source::Arxiv].into(), ..Work::default() };
        w.ids.insert(id.clone());
        w.ids.extend(text(e, ARXIV, "doi").as_deref().and_then(Id::doi));
        // arXiv mints a DOI for every paper; recording it lets OpenAlex's copy (which carries it) merge.
        w.ids.extend(Id::doi(&format!("10.48550/arxiv.{}", id.value())));
        w.title = text(e, ATOM, "title").unwrap_or_default();
        w.abstract_text = text(e, ATOM, "summary");
        w.year = text(e, ATOM, "published").and_then(|p| p.get(..4).and_then(|y| y.parse().ok()));
        w.authors = e
            .children()
            .filter(|c| c.has_tag_name((ATOM, "author")))
            .filter_map(|a| text(a, ATOM, "name"))
            .collect();
        w.venue = text(e, ARXIV, "journal_ref");
        w.open_url = e
            .children()
            .find(|c| c.has_tag_name((ATOM, "link")) && c.attribute("title") == Some("pdf"))
            .and_then(|l| l.attribute("href"))
            .map(str::to_string);
        out.push(w);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <id>http://arxiv.org/abs/2106.09685v2</id>
    <published>2021-06-17T17:37:18Z</published>
    <title>LoRA: Low-Rank Adaptation of
      Large Language Models</title>
    <summary>  We propose Low-Rank Adaptation. </summary>
    <author><name>Edward J. Hu</name></author>
    <author><name>Yelong Shen</name></author>
    <arxiv:journal_ref>ICLR 2022</arxiv:journal_ref>
    <link title="pdf" href="http://arxiv.org/pdf/2106.09685v2" rel="related" type="application/pdf"/>
  </entry>
</feed>"#;

    #[test]
    fn parses_an_entry() {
        let w = &parse_feed(FEED).unwrap()[0];
        assert_eq!(w.title, "LoRA: Low-Rank Adaptation of Large Language Models");
        assert!(w.ids.contains(&Id::Arxiv("2106.09685".into())));
        assert!(w.ids.contains(&Id::Doi("10.48550/arxiv.2106.09685".into())));
        assert_eq!(w.year, Some(2021));
        assert_eq!(w.authors, vec!["Edward J. Hu", "Yelong Shen"]);
        assert_eq!(w.abstract_text.as_deref(), Some("We propose Low-Rank Adaptation."));
        assert_eq!(w.open_url.as_deref(), Some("http://arxiv.org/pdf/2106.09685v2"));
    }

    #[test]
    fn an_error_entry_is_an_error() {
        let err = r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><id>http://arxiv.org/api/errors#bad</id><summary>malformed query</summary></entry></feed>"#;
        assert_eq!(parse_feed(err), Err(Error::Parse("arxiv: malformed query".into())));
    }

    #[test]
    fn query_requires_every_word() {
        assert_eq!(query_expr("low-rank adapters"), "(ti:low-rank OR abs:low-rank) AND (ti:adapters OR abs:adapters)");
    }
}
