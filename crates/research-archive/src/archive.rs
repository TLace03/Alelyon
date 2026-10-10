//! The set of papers gathered for one subject, with each paper held once however many indexes returned it.

use std::collections::HashMap;

use crate::ident::{self, Id};
use crate::work::Work;

/// A paper's position in the archive. Stable for the archive's lifetime (papers are merged, never removed).
pub type Key = usize;

#[derive(Debug, Default)]
pub struct Archive {
    works: Vec<Work>,
    by_id: HashMap<Id, Key>,
    /// Title fingerprint and year: the fallback match when two records share no identifier.
    by_title: HashMap<String, Vec<Key>>,
}

/// What `insert` did with a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    New(Key),
    Merged(Key),
}

impl Inserted {
    pub fn key(self) -> Key {
        match self {
            Inserted::New(k) | Inserted::Merged(k) => k,
        }
    }
}

impl Archive {
    pub fn new() -> Archive {
        Archive::default()
    }

    pub fn len(&self) -> usize {
        self.works.len()
    }

    pub fn is_empty(&self) -> bool {
        self.works.is_empty()
    }

    pub fn get(&self, key: Key) -> &Work {
        &self.works[key]
    }

    pub fn works(&self) -> impl Iterator<Item = (Key, &Work)> {
        self.works.iter().enumerate()
    }

    pub fn find(&self, id: &Id) -> Option<Key> {
        self.by_id.get(id).copied()
    }

    /// Add a record, merging it into the paper it shares an identifier with, or failing that into a paper with
    /// the same title fingerprint whose year is within one (preprint and journal versions straddle a year end).
    ///
    /// A record whose identifiers point at two different archived papers joins the first one found; the two are
    /// not merged with each other. That case means the indexes disagree, and it is left visible rather than
    /// resolved by guessing.
    pub fn insert(&mut self, work: Work) -> Inserted {
        let by_id = work.ids.iter().find_map(|id| self.by_id.get(id).copied());
        let key = by_id.or_else(|| self.title_match(&work));
        match key {
            Some(k) => {
                self.works[k].absorb(work);
                self.index(k);
                Inserted::Merged(k)
            }
            None => {
                let k = self.works.len();
                self.works.push(work);
                self.index(k);
                Inserted::New(k)
            }
        }
    }

    fn title_match(&self, work: &Work) -> Option<Key> {
        let fp = ident::merge_key(&work.title);
        // A title of a word or two ("Introduction", "Editorial") identifies nothing.
        if fp.split(' ').count() < 4 {
            return None;
        }
        self.by_title.get(&compact(&fp))?.iter().copied().find(|&k| match (self.works[k].year, work.year) {
            (Some(a), Some(b)) => (a - b).abs() <= 1,
            _ => true,
        })
    }

    fn index(&mut self, k: Key) {
        for id in &self.works[k].ids {
            self.by_id.entry(id.clone()).or_insert(k);
        }
        let fp = compact(&ident::merge_key(&self.works[k].title));
        let keys = self.by_title.entry(fp).or_default();
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
}

/// The fingerprint without its word breaks, so "opto-electronic" and "optoelectronic" agree.
fn compact(fp: &str) -> String {
    fp.replace(' ', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::Source;

    fn work(ids: &[Id], title: &str, year: i32, src: Source) -> Work {
        Work { ids: ids.iter().cloned().collect(), title: title.into(), year: Some(year), seen_in: [src].into(), ..Work::default() }
    }

    #[test]
    fn shared_id_merges_and_records_both_sources() {
        let mut a = Archive::new();
        let doi = Id::doi("10.1/x").unwrap();
        a.insert(work(&[doi.clone(), Id::openalex("W1").unwrap()], "A paper about things here", 2020, Source::OpenAlex));
        let r = a.insert(work(&[doi, Id::arxiv("2001.00001").unwrap()], "A paper about things here", 2020, Source::Arxiv));
        assert_eq!(r, Inserted::Merged(0));
        assert_eq!(a.len(), 1);
        assert_eq!(a.get(0).seen_in.len(), 2);
        assert_eq!(a.find(&Id::arxiv("2001.00001").unwrap()), Some(0));
    }

    #[test]
    fn title_merge_needs_close_years_and_a_real_title() {
        let mut a = Archive::new();
        a.insert(work(&[Id::openalex("W1").unwrap()], "Attention is all you need", 2017, Source::OpenAlex));
        assert_eq!(a.insert(work(&[Id::arxiv("1706.03762").unwrap()], "Attention Is All You Need.", 2018, Source::Arxiv)), Inserted::Merged(0));
        assert!(matches!(a.insert(work(&[Id::openalex("W9").unwrap()], "Attention is all you need", 2024, Source::OpenAlex)), Inserted::New(_)));
        assert!(matches!(a.insert(work(&[Id::openalex("W2").unwrap()], "Introduction", 2017, Source::OpenAlex)), Inserted::New(_)));
        assert!(matches!(a.insert(work(&[Id::openalex("W3").unwrap()], "Introduction", 2017, Source::OpenAlex)), Inserted::New(_)));
    }

    #[test]
    fn hyphenation_does_not_split_a_paper() {
        // Seen in a live OpenAlex harvest (2026-10-08): one 1990 paper indexed under both spellings.
        let mut a = Archive::new();
        a.insert(work(&[Id::openalex("W1").unwrap()], "Character recognition using a dynamic optoelectronic neural network", 1990, Source::OpenAlex));
        let r = a.insert(work(&[Id::openalex("W2").unwrap()], "Character recognition using a dynamic opto-electronic neural network", 1990, Source::OpenAlex));
        assert_eq!(r, Inserted::Merged(0));
    }
}
