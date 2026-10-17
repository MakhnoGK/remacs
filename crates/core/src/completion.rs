use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::num::NonZero;
use std::ops::Range;
use std::rc::Rc;
use std::thread;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::minibuffer::ListItem;
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
    /// Starting a program, a triangle.
    Run,
    /// Ending a program, a square.
    Stop,
    /// Starting a program again, a circular arrow.
    Rerun,
    /// A Rust package, run by cargo.
    Package,
    /// An npm script.
    Script,
    /// A program run as it is.
    Terminal,
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
    /// The color the candidate names, `0xRRGGBBAA`, drawn in place of its kind.
    pub swatch: Option<u32>,
    pub modified: bool,
    pub dim: bool,
    /// Makes the candidate a step of a plan, like a commit of a rebase: the action
    /// taken on it, drawn as a button before it, then the label as an id and the
    /// annotation as what it is.
    pub action: Option<(&'static str, Tone)>,
    /// Makes the candidate an entry, its label drawn as an id, that opens up while
    /// selected to show this.
    pub expansion: Option<Rc<Expansion>>,
}

/// What an entry shows while selected, e.g. a stash's files and what can be done with it.
#[derive(Debug, Default)]
pub struct Expansion {
    pub items: Vec<ListItem>,
    pub actions: Vec<EntryAction>,
    /// In place of the candidate's detail, e.g. without the count of the items shown.
    pub detail: String,
}

/// A button under an open entry: what it does, the key that does it too and the command
/// both run.
#[derive(Clone, Copy, Debug)]
pub struct EntryAction {
    pub label: &'static str,
    pub key: &'static str,
    pub command: &'static str,
    /// It throws something away; drawn apart, as a warning.
    pub destructive: bool,
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

#[derive(Clone, Default)]
pub struct Completions {
    /// The best matches; `total` may count more than were materialised.
    pub candidates: Vec<Candidate>,
    pub total: usize,
    /// Selected first for this input, e.g. the match nearest point; else the first.
    pub selected: Option<usize>,
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

/// The completions a source last computed and what they were computed from, handed out
/// again while that holds: a background refresh re-asks every source, though usually
/// neither the input nor the source's data changed.
pub struct CompletionMemo<K> {
    last: RefCell<Option<(K, Completions)>>,
}

impl<K> Default for CompletionMemo<K> {
    fn default() -> Self {
        Self {
            last: RefCell::new(None),
        }
    }
}

impl<K> CompletionMemo<K> {
    /// The completions for `key`, computed again only when `same` tells it from the last.
    pub fn get(
        &self,
        key: K,
        same: impl FnOnce(&K, &K) -> bool,
        compute: impl FnOnce() -> Completions,
    ) -> Completions {
        let mut last = self.last.borrow_mut();
        if let Some((last_key, completions)) = last.as_ref()
            && same(last_key, &key)
        {
            return completions.clone();
        }
        let completions = compute();
        *last = Some((key, completions.clone()));
        completions
    }
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

/// A query matched the way fzf does: every space-separated term matches its characters in
/// order, `'term` as written, `^term` at the start, `term$` at the end, and `!term` must
/// not match. A term ignores case unless it has an uppercase letter.
pub struct FuzzyQuery {
    pattern: Pattern,
    config: Config,
}

#[derive(Default)]
struct Scratch {
    matcher: Matcher,
    chars: Vec<char>,
    indices: Vec<u32>,
}

thread_local! {
    /// A matcher allocates ~135 KB up front, so one is kept and reused.
    static SCRATCH: RefCell<Scratch> = RefCell::default();
}

impl FuzzyQuery {
    pub fn new(query: &str) -> Self {
        Self::with_config(query, Config::DEFAULT)
    }

    /// Rewards matches right after a `/`, for queries over file paths.
    pub fn for_paths(query: &str) -> Self {
        Self::with_config(query, Config::DEFAULT.match_paths())
    }

    fn with_config(query: &str, config: Config) -> Self {
        Self {
            pattern: Pattern::parse(query, CaseMatching::Smart, Normalization::Smart),
            config,
        }
    }

    /// Everything matches an empty query, with the same score.
    pub fn is_empty(&self) -> bool {
        self.pattern.atoms.is_empty()
    }

    /// How well `text` matches, higher being better; none when it does not.
    pub fn score(&self, text: &str) -> Option<u32> {
        SCRATCH.with_borrow_mut(|Scratch { matcher, chars, .. }| {
            matcher.config.clone_from(&self.config);
            self.pattern.score(Utf32Str::new(text, chars), matcher)
        })
    }

    /// The score, and the byte ranges of `text` that matched.
    pub fn score_and_ranges(&self, text: &str) -> Option<(u32, Vec<Range<usize>>)> {
        SCRATCH.with_borrow_mut(|scratch| {
            let Scratch {
                matcher,
                chars,
                indices,
            } = scratch;
            matcher.config.clone_from(&self.config);
            indices.clear();
            let score = self
                .pattern
                .indices(Utf32Str::new(text, chars), matcher, indices)?;
            indices.sort_unstable();
            indices.dedup();
            Some((score, byte_ranges(text, indices)))
        })
    }
}

/// Byte ranges of the chars of `text` at the sorted `char_indices`.
fn byte_ranges(text: &str, char_indices: &[u32]) -> Vec<Range<usize>> {
    let mut wanted = char_indices.iter().peekable();
    let ranges = text
        .char_indices()
        .zip(0u32..)
        .filter_map(|((start, character), index)| {
            wanted
                .next_if_eq(&&index)
                .map(|_| start..start + character.len_utf8())
        })
        .collect();
    merge(ranges)
}

pub struct Match<T> {
    pub item: T,
    pub ranges: Vec<Range<usize>>,
    /// Higher scores first, then shorter labels.
    rank: (Reverse<u32>, usize),
}

/// Items whose label matches `query` as [`FuzzyQuery`] does, in source order.
pub fn filter<T>(
    query: &str,
    items: impl IntoIterator<Item = T>,
    label: impl Fn(&T) -> &str,
) -> Vec<Match<T>> {
    let query = FuzzyQuery::new(query);
    items
        .into_iter()
        .filter_map(|item| {
            let text = label(&item);
            let (score, ranges) = query.score_and_ranges(text)?;
            let length = if query.is_empty() { 0 } else { text.len() };
            Some(Match {
                ranges,
                item,
                rank: (Reverse(score), length),
            })
        })
        .collect()
}

/// Orders matches best first; ties keep source order.
pub fn rank<T>(matches: &mut [Match<T>]) {
    matches.sort_by_key(|matched| matched.rank);
}

/// Lists shorter than this are matched on the calling thread.
const PARALLEL_MIN_ITEMS: usize = 8192;

/// `matched` of each item and its index, keeping those that match in order; a long list
/// is split across threads, so that a prompt over 100 000 paths keeps up with typing.
pub fn filter_map_parallel<T: Sync, R: Send>(
    items: &[T],
    matched: impl Fn(usize, &T) -> Option<R> + Sync,
) -> Vec<R> {
    let threads = thread::available_parallelism().map_or(1, NonZero::get);
    if items.len() < PARALLEL_MIN_ITEMS || threads == 1 {
        return (0..)
            .zip(items)
            .filter_map(|(index, item)| matched(index, item))
            .collect();
    }
    let chunk_size = items.len().div_ceil(threads);
    let matched = &matched;
    thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks(chunk_size)
            .enumerate()
            .map(|(chunk, items)| {
                scope.spawn(move || {
                    let first = chunk * chunk_size;
                    (first..)
                        .zip(items)
                        .filter_map(|(index, item)| matched(index, item))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("matching does not panic"))
            .collect()
    })
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

/// Whether `text` matches `query` as [`flex_match`] matches it, and if so whether it
/// starts with it; nothing is allocated, so whole lists can be ranked per key.
pub fn flex_rank(query: &str, text: &str) -> Option<bool> {
    let case_sensitive = query.chars().any(char::is_uppercase);
    let starts_with = if case_sensitive {
        text.starts_with(query)
    } else {
        text.as_bytes()
            .get(..query.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(query.as_bytes()))
    };
    if starts_with {
        return Some(true);
    }
    let mut rest = text.chars();
    let found = query.chars().all(|wanted| {
        rest.any(|character| {
            if case_sensitive {
                character == wanted
            } else {
                character.eq_ignore_ascii_case(&wanted)
            }
        })
    });
    found.then_some(false)
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
    fn matches_all_terms_with_smart_case_and_ranks_word_starts_first() {
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
    fn matches_characters_in_order_across_a_path_with_fzf_terms() {
        let files = [
            "plugins/files/src/find_file.rs",
            "plugins/files/src/lib.rs",
            "crates/core/src/find.rs",
        ];
        let paths = |query: &str| {
            let query = FuzzyQuery::for_paths(query);
            files
                .iter()
                .filter(|file| query.score(file).is_some())
                .copied()
                .collect::<Vec<_>>()
        };
        assert_eq!(paths("plufifi"), ["plugins/files/src/find_file.rs"]);
        assert_eq!(paths("find !core"), ["plugins/files/src/find_file.rs"]);
        assert_eq!(paths("^crates"), ["crates/core/src/find.rs"]);
        assert_eq!(paths("lib.rs$"), ["plugins/files/src/lib.rs"]);
        assert_eq!(
            paths("'src/f"),
            ["plugins/files/src/find_file.rs", "crates/core/src/find.rs"]
        );

        let (_, ranges) = FuzzyQuery::new("fifi")
            .score_and_ranges("find_file")
            .unwrap();
        assert_eq!(ranges, [0..2, 5..7]);
        let (_, ranges) = FuzzyQuery::new("жx").score_and_ranges("ажbx").unwrap();
        assert_eq!(ranges, [2..4, 5..6]);
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

    #[test]
    fn flex_rank_agrees_with_flex_match() {
        let texts = ["last()", "size_hint()", "is_sorted", "start", "Start", "ажbx", "s", ""];
        for query in ["st", "sT", "St", "s", "жx", "", "start"] {
            for text in texts {
                let ranked = flex_match(query, text)
                    .map(|ranges| query.is_empty() || ranges.first() == Some(&(0..query.len())));
                assert_eq!(flex_rank(query, text), ranked, "{query:?} in {text:?}");
            }
        }
    }
}
