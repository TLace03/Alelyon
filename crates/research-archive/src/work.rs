//! One paper as the archive keeps it, whichever index it came from.

use std::collections::BTreeSet;

use crate::ident::{self, Id};

/// The index a record was read from. Kept per paper, because the missing-paper estimate
/// (coverage.rs) needs to know which indexes saw which papers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    OpenAlex,
    Arxiv,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::OpenAlex => "openalex",
            Source::Arxiv => "arxiv",
        }
    }

    pub fn parse(name: &str) -> Option<Source> {
        match name {
            "openalex" => Some(Source::OpenAlex),
            "arxiv" => Some(Source::Arxiv),
            _ => None,
        }
    }
}

/// A paper. Every field is what an index said; nothing here has been checked against the paper itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Work {
    /// Every identifier any index gave this paper (normalised, see ident.rs).
    pub ids: BTreeSet<Id>,
    pub title: String,
    pub abstract_text: Option<String>,
    pub year: Option<i32>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    /// An openly readable copy, when an index named one.
    pub open_url: Option<String>,
    /// The papers this one cites, as identifiers (OpenAlex gives its own ids).
    pub references: BTreeSet<Id>,
    /// How many papers cite this one, as the index that said so counted (not a count of the archive's own edges).
    pub cited_by_count: Option<u64>,
    /// The indexes this paper was seen in.
    pub seen_in: BTreeSet<Source>,
}

impl Work {
    /// The fallback key when two records share no identifier: the title's fingerprint.
    pub fn title_key(&self) -> String {
        ident::title_fingerprint(&self.title)
    }

    /// Fold another index's record of the same paper into this one. Fields already present are kept;
    /// sets are unioned. The longer abstract wins (indexes truncate them differently).
    pub fn absorb(&mut self, other: Work) {
        self.ids.extend(other.ids);
        self.references.extend(other.references);
        self.seen_in.extend(other.seen_in);
        if self.title.is_empty() {
            self.title = other.title;
        }
        match (&self.abstract_text, other.abstract_text) {
            (None, Some(a)) => self.abstract_text = Some(a),
            (Some(mine), Some(a)) if a.len() > mine.len() => self.abstract_text = Some(a),
            _ => {}
        }
        self.year = self.year.or(other.year);
        if self.authors.is_empty() {
            self.authors = other.authors;
        }
        self.venue = self.venue.take().or(other.venue);
        self.open_url = self.open_url.take().or(other.open_url);
        self.cited_by_count = match (self.cited_by_count, other.cited_by_count) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
}
