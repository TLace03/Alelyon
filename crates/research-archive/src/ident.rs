//! Identifiers, normalised so that two indexes' spellings of one paper compare equal.

use std::fmt;

/// A paper's identifier in one scheme. The string is already normalised by the constructor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Id {
    /// Lower-case, without any `https://doi.org/` or `doi:` prefix.
    Doi(String),
    /// Without its version suffix: `2106.09685`, or the old style `hep-th/9901001`.
    Arxiv(String),
    /// OpenAlex's work id, upper-case: `W2741809807`.
    OpenAlex(String),
}

impl Id {
    pub fn doi(raw: &str) -> Option<Id> {
        let s = raw.trim();
        let lower = s.to_ascii_lowercase();
        let rest = ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/", "http://dx.doi.org/", "doi:"]
            .iter()
            .find_map(|p| lower.strip_prefix(p))
            .unwrap_or(&lower);
        let rest = rest.trim();
        // Every DOI starts with the directory indicator "10." and has a suffix after a slash.
        if rest.starts_with("10.") && rest.contains('/') {
            Some(Id::Doi(rest.to_string()))
        } else {
            None
        }
    }

    pub fn arxiv(raw: &str) -> Option<Id> {
        let mut s = raw.trim();
        for p in ["http://arxiv.org/abs/", "https://arxiv.org/abs/", "arXiv:", "arxiv:"] {
            if let Some(r) = s.strip_prefix(p) {
                s = r;
            }
        }
        // Drop a version suffix: 2106.09685v3 -> 2106.09685.
        let base = match s.rfind('v') {
            Some(i) if i > 0 && s[i + 1..].chars().all(|c| c.is_ascii_digit()) && i + 1 < s.len() => &s[..i],
            _ => s,
        };
        if !base.is_ascii() {
            return None;
        }
        let new_style = base.len() >= 9
            && base.as_bytes()[4] == b'.'
            && base[..4].chars().all(|c| c.is_ascii_digit())
            && base[5..].chars().all(|c| c.is_ascii_digit());
        let old_style = base.contains('/') && base.rsplit('/').next().is_some_and(|n| n.len() == 7 && n.chars().all(|c| c.is_ascii_digit()));
        if new_style || old_style { Some(Id::Arxiv(base.to_string())) } else { None }
    }

    pub fn openalex(raw: &str) -> Option<Id> {
        let s = raw.trim();
        let tail = s.rsplit('/').next().unwrap_or(s).to_ascii_uppercase();
        if tail.len() > 1 && tail.starts_with('W') && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            Some(Id::OpenAlex(tail))
        } else {
            None
        }
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            Id::Doi(_) => "doi",
            Id::Arxiv(_) => "arxiv",
            Id::OpenAlex(_) => "openalex",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Id::Doi(v) | Id::Arxiv(v) | Id::OpenAlex(v) => v,
        }
    }

    pub fn from_parts(scheme: &str, value: &str) -> Option<Id> {
        match scheme {
            "doi" => Id::doi(value),
            "arxiv" => Id::arxiv(value),
            "openalex" => Id::openalex(value),
            _ => None,
        }
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.scheme(), self.value())
    }
}

/// A title reduced to its lower-case letters and digits, words joined by single spaces. Two indexes that
/// differ only in case, punctuation, markup or spacing produce the same fingerprint. Non-ASCII letters are
/// kept as they are (no transliteration), so "Schrödinger" and "Schrodinger" stay different: a known gap.
pub fn title_fingerprint(title: &str) -> String {
    // Strip simple markup the indexes leave in titles (<i>, <sub>, $...$ delimiters).
    let mut plain = String::with_capacity(title.len());
    let mut in_tag = false;
    for c in title.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => plain.push(c),
        }
    }
    plain
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Bracketed tags an index appends to a title to mark an edition of the same paper, not a different one. Only these
/// are dropped for matching: "Part I" and "Part II" stay apart.
const EDITION_TAGS: &[&str] = &[
    "abstract only",
    "abstract",
    "extended abstract",
    "invited paper",
    "invited talk",
    "invited",
    "short paper",
    "poster",
    "preprint",
    "conference version",
    "journal version",
];

/// The title as two records of one paper should share it: trailing bracketed edition tags dropped (observed
/// 2026-10-09: "… Binary Convolutional Neural Networks (Abstract Only)" archived beside the same title without the
/// tag), then fingerprinted.
pub fn merge_key(title: &str) -> String {
    let mut t = title.trim();
    loop {
        let close = match t.chars().last() {
            Some(')') => '(',
            Some(']') => '[',
            _ => break,
        };
        let Some(open) = t.rfind(close) else { break };
        let inner = t[open + 1..t.len() - 1].trim().trim_end_matches('.').to_lowercase();
        if !EDITION_TAGS.contains(&inner.as_str()) {
            break;
        }
        t = t[..open].trim_end();
    }
    title_fingerprint(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edition_tags_do_not_split_a_paper_but_parts_stay_apart() {
        let a = "A 7.663-TOPS 8.2-W Energy-efficient FPGA Accelerator for Binary Convolutional Neural Networks";
        assert_eq!(merge_key(&format!("{a} (Abstract Only)")), merge_key(a));
        assert_eq!(merge_key(&format!("{a} [Extended Abstract] (Invited Paper)")), merge_key(a));
        assert_ne!(merge_key("Model compression, Part I"), merge_key("Model compression, Part II"));
        assert_ne!(merge_key("Quantizing for minimum distortion (Corresp.)"), merge_key("Quantizing for minimum distortion (Part 2)"));
        // A bracket that is not an edition tag stays part of the title.
        assert_eq!(merge_key("Learning (with) bits"), title_fingerprint("Learning (with) bits"));
    }

    #[test]
    fn doi_spellings_agree() {
        let a = Id::doi("https://doi.org/10.48550/ARXIV.2106.09685").unwrap();
        let b = Id::doi("doi:10.48550/arxiv.2106.09685").unwrap();
        assert_eq!(a, b);
        assert_eq!(Id::doi("not a doi"), None);
    }

    #[test]
    fn arxiv_drops_version_and_prefix() {
        assert_eq!(Id::arxiv("http://arxiv.org/abs/2106.09685v2"), Some(Id::Arxiv("2106.09685".into())));
        assert_eq!(Id::arxiv("arXiv:2106.09685"), Some(Id::Arxiv("2106.09685".into())));
        assert_eq!(Id::arxiv("hep-th/9901001v1"), Some(Id::Arxiv("hep-th/9901001".into())));
        assert_eq!(Id::arxiv("victory"), None);
    }

    #[test]
    fn openalex_from_url() {
        assert_eq!(Id::openalex("https://openalex.org/W2741809807"), Some(Id::OpenAlex("W2741809807".into())));
        assert_eq!(Id::openalex("https://openalex.org/A123"), None);
    }

    #[test]
    fn fingerprint_ignores_case_markup_and_punctuation() {
        assert_eq!(
            title_fingerprint("LoRA: Low-Rank <i>Adaptation</i> of Large  Language Models."),
            title_fingerprint("Lora low rank adaptation of large language models")
        );
    }
}
