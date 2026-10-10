//! OpenAlex (https://openalex.org): an open index of about 250 million scholarly works with their reference lists,
//! released under CC0. It is the archive's citation graph.

use serde_json::Value;

use crate::ident::Id;
use crate::index::{Error, Fetch, Index, encode};
use crate::work::{Source, Work};

const BASE: &str = "https://api.openalex.org/works";
const SELECT: &str = "id,doi,display_name,publication_year,authorships,primary_location,locations,abstract_inverted_index,referenced_works,cited_by_count,open_access";
/// OpenAlex's largest page.
const PAGE: usize = 200;
/// How many ids one `openalex_id:` filter may OR together.
const OR_LIMIT: usize = 50;

pub struct OpenAlex<F: Fetch> {
    fetch: F,
    /// Sent as `mailto=` (OpenAlex's "polite pool") only when the user configured one; never filled in for them.
    mailto: Option<String>,
    /// For an update: only works published on or after this date (YYYY-MM-DD) are searched for and read as citers.
    since: Option<String>,
}

impl<F: Fetch> OpenAlex<F> {
    pub fn new(fetch: F) -> Self {
        OpenAlex { fetch, mailto: None, since: None }
    }

    /// Read only works published on or after `date` (YYYY-MM-DD) in searches and citer lists. OpenAlex's
    /// `from_created_date` would be the exact "new to the index" filter, but it needs a paid key; a publication-date
    /// floor with a margin is what a free key can ask for.
    pub fn with_since(mut self, date: Option<String>) -> Self {
        self.since = date.filter(|d| d.len() == 10 && d.chars().all(|c| c.is_ascii_digit() || c == '-'));
        self
    }

    fn since_filter(&self) -> String {
        self.since.as_ref().map(|d| format!(",from_publication_date:{d}")).unwrap_or_default()
    }

    pub fn with_mailto(mut self, mailto: Option<String>) -> Self {
        self.mailto = mailto;
        self
    }

    fn url(&self, filter: &str, per_page: usize, cursor: &str) -> String {
        let mut u = format!("{BASE}?filter={filter}&select={SELECT}&per-page={per_page}&cursor={}", encode(cursor));
        if let Some(m) = &self.mailto {
            u.push_str(&format!("&mailto={}", encode(m)));
        }
        u
    }

    /// Follow OpenAlex's cursor through a filter until `limit` records or the end.
    fn paged(&mut self, filter: &str, limit: usize) -> Result<Vec<Work>, Error> {
        let mut out = Vec::new();
        let mut cursor = "*".to_string();
        while out.len() < limit {
            let url = self.url(filter, PAGE.min(limit - out.len()).max(1), &cursor);
            let body = self.fetch.get(&url)?;
            let (works, next) = parse_page(&body)?;
            let empty = works.is_empty();
            out.extend(works);
            match next {
                Some(n) if !empty => cursor = n,
                _ => break,
            }
        }
        out.truncate(limit);
        Ok(out)
    }
}

impl<F: Fetch> Index for OpenAlex<F> {
    fn source(&self) -> Source {
        Source::OpenAlex
    }

    fn resolves(&self, id: &Id) -> bool {
        matches!(id, Id::OpenAlex(_) | Id::Doi(_))
    }

    fn search(&mut self, query: &str, limit: usize) -> Result<Vec<Work>, Error> {
        // Commas separate filters in OpenAlex's syntax, so they cannot appear inside the search value.
        let q = query.replace(',', " ");
        let since = self.since_filter();
        self.paged(&format!("title_and_abstract.search:{}{since}", encode(&q)), limit)
    }

    fn citing(&mut self, work: &Work, limit: usize) -> Result<Option<Vec<Work>>, Error> {
        let Some(Id::OpenAlex(w)) = work.ids.iter().find(|i| matches!(i, Id::OpenAlex(_))) else {
            // Not in OpenAlex as far as the archive knows: the graph exists, this paper's place in it is unknown.
            return Ok(None);
        };
        let since = self.since_filter();
        self.paged(&format!("cites:{w}{since}"), limit).map(Some)
    }

    fn resolve(&mut self, ids: &[Id]) -> Result<Vec<Work>, Error> {
        let mut out = Vec::new();
        let wanted: Vec<&str> = ids.iter().filter_map(|i| match i {
            Id::OpenAlex(w) => Some(w.as_str()),
            _ => None,
        }).collect();
        for chunk in wanted.chunks(OR_LIMIT) {
            out.extend(self.paged(&format!("openalex_id:{}", chunk.join("|")), chunk.len())?);
        }
        let dois: Vec<String> = ids.iter().filter_map(|i| match i {
            Id::Doi(d) => Some(encode(d)),
            _ => None,
        }).collect();
        for chunk in dois.chunks(OR_LIMIT) {
            out.extend(self.paged(&format!("doi:{}", chunk.join("|")), chunk.len())?);
        }
        Ok(out)
    }
}

/// Where OpenAlex reports a key's daily budget. The key goes in the transport's bearer header (net.rs).
pub const RATE_LIMIT_URL: &str = "https://api.openalex.org/rate-limit";
/// Where a person signs up (about thirty seconds) and copies a free key.
pub const KEY_PAGE: &str = "https://openalex.org/settings/api";
/// OpenAlex's explanation of keys and budgets.
pub const KEY_HELP: &str = "https://help.openalex.org/api/authentication/";

/// A key's daily budget as OpenAlex counts it, in credits (a page of a list costs one, a search ten, observed in
/// its `/rate-limit` answer on 2026-10-08).
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub credits_limit: u64,
    pub credits_used: u64,
    pub credits_remaining: u64,
    /// When the budget refills, as OpenAlex wrote it (ISO 8601, UTC midnight).
    pub resets_at: Option<String>,
    pub resets_in_seconds: Option<u64>,
}

pub fn parse_budget(body: &str) -> Result<Budget, Error> {
    let v: Value = serde_json::from_str(body).map_err(|e| Error::Parse(format!("openalex rate-limit: {e}")))?;
    let r = v.get("rate_limit").ok_or_else(|| Error::Parse("openalex rate-limit: no rate_limit object".into()))?;
    let n = |k: &str| r.get(k).and_then(Value::as_u64).ok_or_else(|| Error::Parse(format!("openalex rate-limit: no {k}")));
    Ok(Budget {
        credits_limit: n("credits_limit")?,
        credits_used: n("credits_used")?,
        credits_remaining: n("credits_remaining")?,
        resets_at: r.get("resets_at").and_then(Value::as_str).map(str::to_string),
        resets_in_seconds: r.get("resets_in_seconds").and_then(Value::as_u64),
    })
}

/// One page of `/works`: its records and the next cursor (absent at the end).
pub fn parse_page(body: &str) -> Result<(Vec<Work>, Option<String>), Error> {
    let v: Value = serde_json::from_str(body).map_err(|e| Error::Parse(format!("openalex: {e}")))?;
    let results = v.get("results").and_then(Value::as_array).ok_or_else(|| Error::Parse("openalex: no results array".into()))?;
    let next = v.pointer("/meta/next_cursor").and_then(Value::as_str).map(str::to_string);
    Ok((results.iter().map(parse_work).collect(), next))
}

pub fn parse_work(r: &Value) -> Work {
    let s = |p: &str| r.pointer(p).and_then(Value::as_str);
    let mut w = Work { seen_in: [Source::OpenAlex].into(), ..Work::default() };
    w.ids.extend(s("/id").and_then(Id::openalex));
    w.ids.extend(s("/doi").and_then(Id::doi));
    // OpenAlex has no arXiv field; an arXiv copy shows up as a location whose landing page is on arxiv.org.
    for loc in r.get("locations").and_then(Value::as_array).into_iter().flatten() {
        if let Some(url) = loc.get("landing_page_url").and_then(Value::as_str)
            && url.contains("arxiv.org/abs/") {
                w.ids.extend(Id::arxiv(url));
            }
    }
    w.title = s("/display_name").unwrap_or_default().to_string();
    w.year = r.get("publication_year").and_then(Value::as_i64).map(|y| y as i32);
    w.authors = r
        .get("authorships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| a.pointer("/author/display_name").and_then(Value::as_str).map(str::to_string))
        .collect();
    w.venue = s("/primary_location/source/display_name").map(str::to_string);
    w.open_url = s("/open_access/oa_url").map(str::to_string);
    w.references = r
        .get("referenced_works")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().and_then(Id::openalex))
        .collect();
    w.cited_by_count = r.get("cited_by_count").and_then(Value::as_u64);
    w.abstract_text = r.get("abstract_inverted_index").and_then(Value::as_object).map(|idx| {
        // OpenAlex ships abstracts as word -> positions; put the words back in order.
        let mut slots: Vec<(u64, &str)> = idx
            .iter()
            .flat_map(|(word, pos)| pos.as_array().into_iter().flatten().filter_map(|p| p.as_u64()).map(move |p| (p, word.as_str())))
            .collect();
        slots.sort_unstable();
        slots.into_iter().map(|(_, w)| w).collect::<Vec<_>>().join(" ")
    });
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Canned(VecDeque<String>, Vec<String>);
    impl Fetch for Canned {
        fn get(&mut self, url: &str) -> Result<String, Error> {
            self.1.push(url.to_string());
            self.0.pop_front().ok_or_else(|| Error::Http("no more canned pages".into()))
        }
    }

    const PAGE1: &str = r#"{"meta":{"count":3,"next_cursor":"abc"},"results":[
      {"id":"https://openalex.org/W1","doi":"https://doi.org/10.1/A","display_name":"Low-rank adaptation","publication_year":2021,
       "authorships":[{"author":{"display_name":"E. Hu"}}],"primary_location":{"source":{"display_name":"ICLR"}},
       "locations":[{"landing_page_url":"http://arxiv.org/abs/2106.09685v2"}],
       "abstract_inverted_index":{"adapt":[1],"We":[0],"models":[2]},
       "referenced_works":["https://openalex.org/W7","https://openalex.org/W8"],"cited_by_count":9000,
       "open_access":{"oa_url":"https://arxiv.org/pdf/2106.09685"}},
      {"id":"https://openalex.org/W2","display_name":"Second","publication_year":null,"abstract_inverted_index":null}
    ]}"#;
    const PAGE2: &str = r#"{"meta":{"next_cursor":null},"results":[{"id":"https://openalex.org/W3","display_name":"Third"}]}"#;

    #[test]
    fn parses_a_record_completely() {
        let (works, next) = parse_page(PAGE1).unwrap();
        assert_eq!(next.as_deref(), Some("abc"));
        let w = &works[0];
        assert!(w.ids.contains(&Id::OpenAlex("W1".into())));
        assert!(w.ids.contains(&Id::Doi("10.1/a".into())));
        assert!(w.ids.contains(&Id::Arxiv("2106.09685".into())));
        assert_eq!(w.abstract_text.as_deref(), Some("We adapt models"));
        assert_eq!(w.references.len(), 2);
        assert_eq!(w.venue.as_deref(), Some("ICLR"));
        assert_eq!(w.cited_by_count, Some(9000));
        assert_eq!(works[1].year, None);
        assert_eq!(works[1].abstract_text, None);
    }

    #[test]
    fn search_follows_the_cursor_to_the_end() {
        let mut oa = OpenAlex::new(Canned([PAGE1.to_string(), PAGE2.to_string()].into(), vec![]));
        let works = oa.search("low-rank, adaptation", 10).unwrap();
        assert_eq!(works.len(), 3);
        assert!(oa.fetch.1[0].contains("title_and_abstract.search:low-rank%20%20adaptation"));
        assert!(oa.fetch.1[1].contains("cursor=abc"));
        // Neither an api key nor an address is sent unless configured.
        assert!(!oa.fetch.1[0].contains("mailto") && !oa.fetch.1[0].contains("api_key"));
    }

    #[test]
    fn a_paper_outside_openalex_has_no_known_citers() {
        let mut oa = OpenAlex::new(Canned(VecDeque::new(), vec![]));
        let w = Work { ids: [Id::Arxiv("2106.09685".into())].into(), ..Work::default() };
        assert_eq!(oa.citing(&w, 5).unwrap(), None);
        assert!(oa.fetch.1.is_empty());
    }

    #[test]
    fn reads_a_keys_budget_and_never_its_echoed_key() {
        // The shape of OpenAlex's answer on 2026-10-08, values changed.
        let body = r#"{"api_key":"abc...xyz","is_grandfathered":false,"rate_limit":{"daily_budget_usd":1,"credits_limit":10000,
            "credits_used":3811,"credits_remaining":6189,"resets_at":"2026-10-09T00:00:00.000Z","resets_in_seconds":31268}}"#;
        let b = parse_budget(body).unwrap();
        assert_eq!((b.credits_limit, b.credits_used, b.credits_remaining), (10000, 3811, 6189));
        assert_eq!(b.resets_in_seconds, Some(31268));
        assert!(!format!("{b:?}").contains("abc"));
        assert!(matches!(parse_budget(r#"{"error":"x"}"#), Err(Error::Parse(_))));
    }

    #[test]
    fn an_update_asks_only_for_recent_works() {
        let mut oa = OpenAlex::new(Canned([PAGE2.to_string(), PAGE2.to_string()].into(), vec![])).with_since(Some("2026-09-01".into()));
        oa.search("binary", 5).unwrap();
        let w = Work { ids: [Id::OpenAlex("W1".into())].into(), ..Work::default() };
        oa.citing(&w, 5).unwrap();
        assert!(oa.fetch.1[0].contains("title_and_abstract.search:binary,from_publication_date:2026-09-01"), "{}", oa.fetch.1[0]);
        assert!(oa.fetch.1[1].contains("cites:W1,from_publication_date:2026-09-01"));
        // Anything but a date is not sent.
        let oa = OpenAlex::new(Canned(VecDeque::new(), vec![])).with_since(Some("2026,cites:W9".into()));
        assert_eq!(oa.since_filter(), "");
    }

    #[test]
    fn malformed_body_is_a_parse_error_not_an_empty_page() {
        assert!(matches!(parse_page("<html>rate limited</html>"), Err(Error::Parse(_))));
        assert!(matches!(parse_page("{}"), Err(Error::Parse(_))));
    }
}
