//! The scholarly indexes the archive reads, behind one trait, and the transport they read through.

use std::fmt;

use crate::ident::Id;
use crate::work::{Source, Work};

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The request did not produce a usable response (network, status, or the transport refused it).
    Http(String),
    /// A response arrived but was not what the index's format promises.
    Parse(String),
    /// The archive's database refused an operation.
    Store(String),
    /// The index refused further requests for longer than is worth waiting (a spent daily quota, say).
    RateLimited(String),
    /// The index refused the API key (401 or 403).
    KeyRefused(String),
    /// The run was stopped by its user.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(m) => write!(f, "request failed: {m}"),
            Error::Parse(m) => write!(f, "unreadable response: {m}"),
            Error::Store(m) => write!(f, "archive store: {m}"),
            Error::RateLimited(m) => write!(f, "rate limited: {m}"),
            Error::KeyRefused(m) => write!(f, "the API key was refused: {m}"),
            Error::Cancelled => write!(f, "stopped"),
        }
    }
}

impl std::error::Error for Error {}

/// One GET, returning the body. The live implementation is net::HttpFetch; tests hand in canned bodies.
pub trait Fetch {
    fn get(&mut self, url: &str) -> Result<String, Error>;
}

/// What one index can do. Every call returns records exactly as the index described them (normalised, not checked).
pub trait Index {
    fn source(&self) -> Source;

    /// Papers whose title or abstract match `query`, at most `limit`.
    fn search(&mut self, query: &str, limit: usize) -> Result<Vec<Work>, Error>;

    /// Papers that cite `work`, at most `limit`. An index without a citation graph returns `Ok(None)`, which is
    /// not the same as a paper nobody cites (`Ok(Some(vec![]))`).
    fn citing(&mut self, work: &Work, limit: usize) -> Result<Option<Vec<Work>>, Error>;

    /// False for an index with no citation graph, so a run spends no budget asking it for citers.
    fn has_citations(&self) -> bool {
        true
    }

    /// Whether `resolve` can look this identifier up, so a run spends no budget on an index that cannot.
    fn resolves(&self, _id: &Id) -> bool {
        true
    }

    /// The records for these identifiers, for those this index holds. Identifiers in schemes the index cannot
    /// look up are ignored.
    fn resolve(&mut self, ids: &[Id]) -> Result<Vec<Work>, Error>;
}

/// Percent-encode a query value (RFC 3986 unreserved characters pass through).
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
