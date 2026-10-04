use std::iter::Peekable;
use std::mem;
use std::str::Chars;

use remacs_core::DocBlock;

/// Paragraphs and code blocks of Markdown documentation; inline markup is reduced to its
/// text, keeping `code` spans.
pub(crate) fn blocks(markdown: &str) -> Vec<DocBlock> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut code: Option<String> = None;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            match code.take() {
                Some(code) => blocks.push(DocBlock::Code(code)),
                None => {
                    flush(&mut blocks, &mut paragraph);
                    code = Some(String::new());
                }
            }
            continue;
        }
        if let Some(code) = &mut code {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
            continue;
        }
        if is_link_definition(trimmed) {
            continue;
        }
        if trimmed.is_empty() || is_rule(trimmed) {
            flush(&mut blocks, &mut paragraph);
        } else if let Some(heading) = heading(trimmed) {
            flush(&mut blocks, &mut paragraph);
            let heading = inline(heading);
            if !heading.is_empty() {
                blocks.push(DocBlock::Heading(heading));
            }
        } else if let Some(item) = list_item(trimmed) {
            flush(&mut blocks, &mut paragraph);
            paragraph.push_str("• ");
            paragraph.push_str(item);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            let hard_break = trimmed.strip_suffix('\\');
            paragraph.push_str(hard_break.unwrap_or(trimmed));
            if hard_break.is_some() || line.ends_with("  ") {
                flush(&mut blocks, &mut paragraph);
            }
        }
    }
    flush(&mut blocks, &mut paragraph);
    blocks.extend(code.map(DocBlock::Code));
    blocks
}

/// Plain text documentation, a paragraph per line.
pub(crate) fn plain_blocks(text: &str) -> Vec<DocBlock> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(|line| DocBlock::Text(line.to_owned()))
        .collect()
}

fn flush(blocks: &mut Vec<DocBlock>, paragraph: &mut String) {
    let text = inline(&mem::take(paragraph));
    let text = text.trim();
    if !text.is_empty() {
        blocks.push(DocBlock::Text(text.to_owned()));
    }
}

fn is_rule(line: &str) -> bool {
    let mark = line.chars().next();
    line.len() >= 3
        && matches!(mark, Some('-' | '*' | '_'))
        && line.chars().all(|character| Some(character) == mark)
}

/// `[Range]: std::ops::Range`, the target of a reference link.
fn is_link_definition(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once("]: "))
        .is_some_and(|(label, _)| !label.is_empty() && !label.contains(']'))
}

fn heading(line: &str) -> Option<&str> {
    let text = line.trim_start_matches('#');
    (text.len() < line.len() && (text.is_empty() || text.starts_with(' '))).then(|| text.trim())
}

fn list_item(line: &str) -> Option<&str> {
    ["- ", "* ", "+ "]
        .iter()
        .find_map(|bullet| line.strip_prefix(bullet))
}

const ENTITIES: [(&str, &str); 6] = [
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&nbsp;", " "),
    ("&amp;", "&"),
];

/// Links become their text; images, emphasis, escapes and entities go; code spans stay
/// as written.
fn inline(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_code = false;
    while let Some(character) = chars.next() {
        let inside_word = |plain: &String, next: Option<&char>| {
            plain.chars().last().is_some_and(char::is_alphanumeric)
                && next.is_some_and(|next| next.is_alphanumeric())
        };
        match character {
            '`' => {
                in_code = !in_code;
                plain.push(character);
            }
            _ if in_code => plain.push(character),
            '\\' if chars.peek().is_some_and(char::is_ascii_punctuation) => {
                plain.extend(chars.next());
            }
            '!' if chars.peek() == Some(&'[') => {
                skip_past(&mut chars, ']');
                skip_link_target(&mut chars);
            }
            '*' | '_' if !inside_word(&plain, chars.peek()) => {}
            '[' => {}
            ']' => skip_link_target(&mut chars),
            _ => plain.push(character),
        }
    }
    ENTITIES.iter().fold(plain, |plain, (entity, character)| {
        plain.replace(entity, character)
    })
}

/// `(url)` or `[reference]` after a link's text.
fn skip_link_target(chars: &mut Peekable<Chars>) {
    match chars.peek() {
        Some('(') => skip_past(chars, ')'),
        Some('[') => skip_past(chars, ']'),
        _ => {}
    }
}

fn skip_past(chars: &mut Peekable<Chars>, close: char) {
    for character in chars.by_ref() {
        if character == close {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_paragraphs_and_code() {
        let markdown = "The lower bound of the **range**\n(inclusive), see [`Range`](std::ops::Range).\n\n\
            # Examples\n\n```rust\nlet r = 1..2;\n\nassert_eq!(r.start, 1);\n```\n---\n- one\n- `two` \\[x\\]\n\n\
            ![Baseline icon](data:image/svg) _Widely available_ in snake_case: &lt;color&gt;\n\n\
            [`Range`]: std::ops::Range\n[two]: two";
        assert_eq!(
            blocks(markdown),
            [
                DocBlock::Text("The lower bound of the range (inclusive), see `Range`.".into()),
                DocBlock::Heading("Examples".into()),
                DocBlock::Code("let r = 1..2;\n\nassert_eq!(r.start, 1);".into()),
                DocBlock::Text("• one".into()),
                DocBlock::Text("• `two` [x]".into()),
                DocBlock::Text("Widely available in snake_case: <color>".into()),
            ]
        );
        assert_eq!(
            plain_blocks("int f(int x)\n\nParameters:"),
            [
                DocBlock::Text("int f(int x)".into()),
                DocBlock::Text("Parameters:".into())
            ]
        );
    }
}
