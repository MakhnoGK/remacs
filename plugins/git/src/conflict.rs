//! The conflicts in a file, between the markers Git writes where both sides changed it.

/// A file cut into the lines both sides agree on and the conflicts between them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictFile {
    pub regions: Vec<Region>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Region {
    Text(Vec<String>),
    Conflict(Conflict),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// Between `<<<<<<<` and `=======`: what was there, the upstream when rebasing.
    pub ours: Vec<String>,
    /// diff3 style: what both started from.
    pub base: Option<Vec<String>>,
    /// Up to `>>>>>>>`: what comes in, the commit replayed when rebasing.
    pub theirs: Vec<String>,
}

impl Conflict {
    /// Its lines in the file, markers included.
    fn len(&self) -> usize {
        let base = self.base.as_ref().map_or(0, |base| base.len() + 1);
        self.ours.len() + base + self.theirs.len() + 3
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Ours,
    Theirs,
    Both,
}

impl Keep {
    /// As the conflict view says it, when rebasing or not.
    pub fn said(self, rebasing: bool) -> &'static str {
        match (self, rebasing) {
            (Keep::Ours, true) => "kept upstream",
            (Keep::Theirs, true) | (Keep::Ours, false) => "kept yours",
            (Keep::Theirs, false) => "kept theirs",
            (Keep::Both, _) => "kept both",
        }
    }
}

impl ConflictFile {
    pub fn conflicts(&self) -> impl Iterator<Item = &Conflict> {
        self.regions.iter().filter_map(|region| match region {
            Region::Conflict(conflict) => Some(conflict),
            Region::Text(_) => None,
        })
    }

    pub fn count(&self) -> usize {
        self.conflicts().count()
    }

    /// The line, from 0, each region starts at.
    pub fn starts(&self) -> Vec<usize> {
        let mut line = 0;
        self.regions
            .iter()
            .map(|region| {
                let start = line;
                line += match region {
                    Region::Text(lines) => lines.len(),
                    Region::Conflict(conflict) => conflict.len(),
                };
                start
            })
            .collect()
    }
}

/// Cuts `text` at its markers; an unfinished conflict is taken as text.
pub fn parse(text: &str) -> ConflictFile {
    #[derive(PartialEq)]
    enum Part {
        Ours,
        Base,
        Theirs,
    }
    let mut regions: Vec<Region> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    let mut open: Option<(Part, Conflict, Vec<String>)> = None;
    for line in text.split('\n') {
        // On bytes: byte 7 can fall inside a character of a line that is not a marker.
        let marker = |sign: u8| {
            let bytes = line.as_bytes();
            bytes.len() >= 7
                && bytes[..7].iter().all(|&byte| byte == sign)
                && bytes.get(7).is_none_or(|&next| next == b' ')
        };
        let mut finished = false;
        match &mut open {
            None if marker(b'<') => {
                open = Some((
                    Part::Ours,
                    Conflict {
                        ours: Vec::new(),
                        base: None,
                        theirs: Vec::new(),
                    },
                    vec![line.to_owned()],
                ));
            }
            None => plain.push(line.to_owned()),
            Some((part, conflict, raw)) => {
                raw.push(line.to_owned());
                if marker(b'|') && *part == Part::Ours {
                    *part = Part::Base;
                    conflict.base = Some(Vec::new());
                } else if line == "=======" && *part != Part::Theirs {
                    *part = Part::Theirs;
                } else if marker(b'>') && *part == Part::Theirs {
                    finished = true;
                } else {
                    match part {
                        Part::Ours => conflict.ours.push(line.to_owned()),
                        Part::Base => conflict.base.get_or_insert_default().push(line.to_owned()),
                        Part::Theirs => conflict.theirs.push(line.to_owned()),
                    }
                }
            }
        }
        if finished && let Some((_, conflict, _)) = open.take() {
            if !plain.is_empty() {
                regions.push(Region::Text(std::mem::take(&mut plain)));
            }
            regions.push(Region::Conflict(conflict));
        }
    }
    if let Some((_, _, raw)) = open {
        plain.extend(raw);
    }
    if !plain.is_empty() {
        regions.push(Region::Text(plain));
    }
    ConflictFile { regions }
}

/// A file with a conflict resolved.
#[derive(Debug, PartialEq, Eq)]
pub struct Resolved {
    pub text: String,
    /// Where the lines kept start, from 0.
    pub start: usize,
    /// Lines the conflict took, markers included.
    pub replaced: usize,
    pub kept: usize,
}

/// `text` with its conflict `index` resolved as `keep` says.
pub fn resolve(text: &str, index: usize, keep: Keep) -> Option<Resolved> {
    let file = parse(text);
    let starts = file.starts();
    let (position, conflict) = file
        .regions
        .iter()
        .enumerate()
        .filter_map(|(position, region)| match region {
            Region::Conflict(conflict) => Some((position, conflict)),
            Region::Text(_) => None,
        })
        .nth(index)?;
    let kept: Vec<String> = match keep {
        Keep::Ours => conflict.ours.clone(),
        Keep::Theirs => conflict.theirs.clone(),
        Keep::Both => conflict
            .ours
            .iter()
            .chain(&conflict.theirs)
            .cloned()
            .collect(),
    };
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let start = starts[position];
    let (replaced, kept_count) = (conflict.len(), kept.len());
    lines.splice(start..start + replaced, kept);
    Some(Resolved {
        text: lines.join("\n"),
        start,
        replaced,
        kept: kept_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "a\n<<<<<<< HEAD\nbee\n=======\nB\n>>>>>>> 7eea87b (feat1)\nc\n\
                        <<<<<<< HEAD\nx\n||||||| base\no\n=======\ny\n>>>>>>> 7eea87b\n";

    #[test]
    fn conflicts_are_cut_out_with_both_sides() {
        let file = parse(TEXT);
        assert_eq!(file.count(), 2);
        // The last line break leaves an empty line after the second conflict.
        assert_eq!(file.starts(), [0, 1, 6, 7, 14]);
        let second = file.conflicts().nth(1).unwrap();
        assert_eq!(second.base.as_deref(), Some(&["o".to_owned()][..]));
        assert_eq!(second.theirs, ["y"]);
        assert_eq!(parse("a\n<<<<<<< HEAD\nb\n").count(), 0);
    }

    #[test]
    fn a_conflict_resolves_to_the_side_kept() {
        let first = resolve(TEXT, 0, Keep::Theirs).unwrap();
        assert!(first.text.starts_with("a\nB\nc\n<<<<<<< HEAD\n"));
        assert_eq!((first.start, first.replaced, first.kept), (1, 5, 1));
        let second = resolve(&first.text, 0, Keep::Both).unwrap();
        assert_eq!(second.text, "a\nB\nc\nx\ny\n");
        assert_eq!((second.start, second.replaced, second.kept), (3, 7, 2));
        assert_eq!(resolve(&second.text, 0, Keep::Ours), None);
    }

    #[test]
    fn lines_with_multibyte_characters_are_not_markers() {
        let text = "Привет мир\n<<<<<<< HEAD\nПривет\n=======\nмир\n>>>>>>> feat\n<<<<<<<é\n";
        let file = parse(text);
        assert_eq!(file.count(), 1);
        assert_eq!(file.conflicts().next().unwrap().theirs, ["мир"]);
        let resolved = resolve(text, 0, Keep::Ours).unwrap();
        assert_eq!(resolved.text, "Привет мир\nПривет\n<<<<<<<é\n");
    }
}
