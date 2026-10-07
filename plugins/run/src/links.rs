//! Places a program's output points at: `file:line:column` as compilers and stack traces
//! print them, and web addresses.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// As printed: relative to the program's directory, or absolute. Line and column count
    /// from 1.
    File {
        path: String,
        line: usize,
        column: Option<usize>,
    },
    Url(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Byte range in the line.
    pub range: Range<usize>,
    pub target: Target,
}

/// The links in `line`, in order.
pub fn links_in(line: &str) -> Vec<Link> {
    let mut links = urls(line);
    let mut files = file_positions(line, &links);
    links.append(&mut files);
    links.sort_by_key(|link| link.range.start);
    links
}

/// Characters a path is made of.
fn is_path_char(character: char) -> bool {
    character.is_alphanumeric() || "_-./~@+\\".contains(character)
}

fn urls(line: &str) -> Vec<Link> {
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(found) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| line[from..].find(scheme))
        .min()
    {
        let start = from + found;
        let length = line[start..]
            .find(|character: char| character.is_whitespace() || "<>\"'`)]".contains(character))
            .unwrap_or(line.len() - start);
        let url = line[start..start + length].trim_end_matches(['.', ',', ';', ':']);
        let end = start + url.len();
        links.push(Link {
            range: start..end,
            target: Target::Url(url.to_owned()),
        });
        from = end.max(start + 1);
    }
    links
}

/// `path:line` and `path:line:column`, where the path has a directory or an extension and
/// is more than numbers, e.g. not `127.0.0.1:3000`; a `file://` URL's path too, but no
/// other scheme's.
fn file_positions(line: &str, urls: &[Link]) -> Vec<Link> {
    let mut links = Vec::new();
    let in_url = |offset: usize| urls.iter().any(|url| url.range.contains(&offset));
    for (colon, _) in line.match_indices(':') {
        let Some(line_number) = number_at(line, colon + 1) else {
            continue;
        };
        let start = line[..colon]
            .char_indices()
            .rev()
            .take_while(|&(_, character)| is_path_char(character))
            .last()
            .map_or(colon, |(index, _)| index);
        let path = &line[start..colon];
        let is_path = (path.contains('/') || path.contains('.'))
            && !path
                .chars()
                .all(|character| character.is_ascii_digit() || character == '.');
        if !is_path || in_url(start) || links.iter().any(|link: &Link| link.range.contains(&start))
        {
            continue;
        }
        let (number, mut end) = line_number;
        let column = line[end..]
            .strip_prefix(':')
            .and_then(|_| number_at(line, end + 1))
            .map(|(column, column_end)| {
                end = column_end;
                column
            });
        let (start, path) = match (line[..start].strip_suffix("file:"), path.strip_prefix("//")) {
            (Some(before), Some(absolute)) => (before.len(), absolute),
            // Another scheme's, like Node's own `node:internal/…`, is no file here.
            _ if line[..start].ends_with(':') => continue,
            _ => (start, path),
        };
        links.push(Link {
            range: start..end,
            target: Target::File {
                path: path.to_owned(),
                line: number,
                column,
            },
        });
    }
    links
}

/// The decimal number starting at `offset`, and where it ends.
fn number_at(line: &str, offset: usize) -> Option<(usize, usize)> {
    let digits = line[offset..]
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(line.len() - offset);
    let number = line[offset..offset + digits].parse().ok()?;
    Some((number, offset + digits))
}
