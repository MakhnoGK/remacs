use std::borrow::Cow;
use std::ops::Range;

use crate::outline::CommitChain;
use crate::status::Tone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    File,
    Directory,
    Error,
    Warning,
    Info,
    /// A version control branch.
    Branch,
    /// A branch of a remote repository.
    Remote,
    /// Something made by choosing it.
    Create,
    /// Bringing commits in, an arrow down.
    Pull,
    /// Sending commits out, an arrow up.
    Push,
}

/// What a completion candidate or a documented symbol is; the UI gives each its own badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Field,
    Variable,
    Constant,
    EnumMember,
    Type,
    Interface,
    Module,
    Keyword,
    Snippet,
    Text,
}

impl SymbolKind {
    pub fn name(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Field => "field",
            SymbolKind::Variable => "variable",
            SymbolKind::Constant => "constant",
            SymbolKind::EnumMember => "variant",
            SymbolKind::Type => "type",
            SymbolKind::Interface => "interface",
            SymbolKind::Module => "module",
            SymbolKind::Keyword => "keyword",
            SymbolKind::Snippet => "snippet",
            SymbolKind::Text => "text",
        }
    }
}

/// One row of a completion list.
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    /// What the minibuffer returns when the candidate is chosen.
    pub value: String,
    /// What choosing it does, before the label, e.g. `Create`.
    pub verb: Option<&'static str>,
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
    pub kind: Option<SymbolKind>,
    pub modified: bool,
    pub dim: bool,
    /// Makes the candidate a step of a plan, like a commit of a rebase: the action
    /// taken on it, drawn as a button before it, then the label as an id and the
    /// annotation as what it is.
    pub action: Option<(&'static str, Tone)>,
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
    /// Said in place of the count, e.g. that the input names a folder to be created.
    pub note: Option<String>,
    /// Keys and what they do for this input, in place of the prompt's.
    pub hints: Option<&'static [(&'static str, &'static str)]>,
    /// In place of the prompt's label, e.g. as an option changes what the input is.
    pub label: Option<String>,
    /// In place of the prompt's subject.
    pub subject: Option<String>,
    /// Groups' titles say how many candidates are in them.
    pub count_groups: bool,
    /// What choosing would make, under the candidates, with a summary of it.
    pub outcome: Option<(String, CommitChain)>,
    /// A note beside the chips, e.g. what a setting will do.
    pub footnote: Option<String>,
}

impl From<Vec<Candidate>> for Completions {
    fn from(candidates: Vec<Candidate>) -> Self {
        Self {
            total: candidates.len(),
            candidates,
            ..Self::default()
        }
    }
}

/// The selected row of a list and the rows on screen, which always include it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Selection {
    selected: usize,
    first_visible: usize,
}

impl Selection {
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Rows on screen of a list of `len`, at most `rows` of them.
    pub fn visible(&self, len: usize, rows: usize) -> Range<usize> {
        self.first_visible..(self.first_visible + rows).min(len)
    }

    pub fn select(&mut self, index: usize, len: usize, rows: usize) {
        self.selected = index.min(len.saturating_sub(1));
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible + rows {
            self.first_visible = self.selected + 1 - rows;
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

/// Byte ranges of `text` matching the characters of `query` in order, like the Emacs
/// `flex` completion style; a contiguous match is preferred. Smart case, as in [`filter`].
pub fn flex_match(query: &str, text: &str) -> Option<Vec<Range<usize>>> {
    if query.is_empty() {
        return Some(Vec::new());
    }
    if let Some(start) = find(text, query) {
        let contiguous = start..start + query.len();
        return Some(vec![contiguous]);
    }
    let case_sensitive = query.chars().any(char::is_uppercase);
    let same = |a: char, b: char| {
        if case_sensitive {
            a == b
        } else {
            a.eq_ignore_ascii_case(&b)
        }
    };
    let mut ranges = Vec::with_capacity(query.len());
    let mut rest = text.char_indices();
    for wanted in query.chars() {
        let (start, found) = rest.find(|&(_, character)| same(character, wanted))?;
        ranges.push(start..start + found.len_utf8());
    }
    Some(merge(ranges))
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

    #[test]
    fn flex_matches_prefer_contiguous_runs() {
        let contiguous = 2..4;
        assert_eq!(flex_match("st", "last()"), Some(vec![contiguous]));
        assert_eq!(flex_match("st", "size_hint()"), Some(vec![0..1, 8..9]));
        assert_eq!(flex_match("sT", "is_sorted"), None);
        assert_eq!(flex_match("жx", "ажbx"), Some(vec![2..4, 5..6]));
        assert_eq!(flex_match("", "any"), Some(vec![]));
    }
}
