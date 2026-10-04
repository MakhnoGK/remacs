use std::borrow::Cow;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    File,
    Directory,
    Error,
    Warning,
    Info,
}

/// One row of a minibuffer completion list.
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    /// What the minibuffer returns when the candidate is chosen.
    pub value: String,
    pub label: String,
    /// Byte ranges of `label` that matched the input.
    pub matches: Vec<Range<usize>>,
    pub annotation: String,
    /// Key sequence bound to the candidate, one entry per key.
    pub keys: Vec<String>,
    pub tag: Option<&'static str>,
    pub detail: Option<Cow<'static, str>>,
    pub group: Option<Cow<'static, str>>,
    pub icon: Option<Icon>,
    pub modified: bool,
    pub dim: bool,
}

impl Candidate {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
            ..Self::default()
        }
    }
}

#[derive(Default)]
pub struct Completions {
    /// The best matches; `total` may count more than were materialised.
    pub candidates: Vec<Candidate>,
    pub total: usize,
}

impl From<Vec<Candidate>> for Completions {
    fn from(candidates: Vec<Candidate>) -> Self {
        Self {
            total: candidates.len(),
            candidates,
        }
    }
}

pub struct Match<T> {
    pub item: T,
    pub ranges: Vec<Range<usize>>,
    /// Prefix matches first, then shorter labels.
    rank: (bool, usize),
}

/// Items whose label contains every space-separated term of `query`, in source order.
/// A term ignores ASCII case unless it has an uppercase letter.
pub fn filter<T>(
    query: &str,
    items: impl IntoIterator<Item = T>,
    label: impl Fn(&T) -> &str,
) -> Vec<Match<T>> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    items
        .into_iter()
        .filter_map(|item| {
            let text = label(&item);
            let mut ranges = Vec::with_capacity(terms.len());
            for term in &terms {
                let start = find(text, term)?;
                ranges.push(start..start + term.len());
            }
            let rank = match ranges.first() {
                Some(first) => (first.start != 0, text.len()),
                None => (false, 0),
            };
            Some(Match {
                ranges: merge(ranges),
                item,
                rank,
            })
        })
        .collect()
}

/// Orders matches best first; ties keep source order.
pub fn rank<T>(matches: &mut [Match<T>]) {
    matches.sort_by_key(|matched| matched.rank);
}

/// Keeps the candidates whose label matches `query` and highlights the matches;
/// `ranked` puts the best first instead of keeping the given order.
pub fn filter_candidates(query: &str, candidates: Vec<Candidate>, ranked: bool) -> Completions {
    let mut matches = filter(query, candidates, |candidate| candidate.label.as_str());
    if ranked {
        rank(&mut matches);
    }
    matches
        .into_iter()
        .map(|matched| Candidate {
            matches: matched.ranges,
            ..matched.item
        })
        .collect::<Vec<_>>()
        .into()
}

fn find(text: &str, term: &str) -> Option<usize> {
    if term.chars().any(char::is_uppercase) {
        return text.find(term);
    }
    let (text, term) = (text.as_bytes(), term.as_bytes());
    (0..=text.len().checked_sub(term.len())?)
        .find(|&start| text[start..start + term.len()].eq_ignore_ascii_case(term))
}

fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_all_terms_with_smart_case_and_ranks_prefixes_first() {
        let names = [
            "desktop-save",
            "save-some-buffers",
            "Save-Place",
            "save-buffer",
        ];
        let mut matches = filter("save", names, |name| name);
        rank(&mut matches);
        let ranked: Vec<_> = matches.iter().map(|matched| matched.item).collect();
        assert_eq!(
            ranked,
            [
                "Save-Place",
                "save-buffer",
                "save-some-buffers",
                "desktop-save"
            ]
        );
        assert_eq!(matches[3].ranges.first(), Some(&(8..12)));

        let matches = filter("buf save", names, |name| name);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].ranges, [0..4, 10..13]);
        assert!(filter("Save", names, |name| name).len() == 1);
    }
}
