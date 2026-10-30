//! Finds what a shell reports of itself in its output, which the emulator ignores: its
//! working directory (OSC 7) and where commands start and end (OSC 133, as fish, iTerm2's
//! and WezTerm's integrations print). The bytes still all go to the emulator.

use std::path::PathBuf;

/// Longer escape sequences are not ours; they are left to the emulator alone.
const MAX_SEQUENCE: usize = 4096;
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    Directory(PathBuf),
    /// A prompt starts.
    Prompt,
    /// What is typed after the prompt starts, or the command it ran starts.
    Input,
    Started,
    /// The command ended, with its exit code if the shell said.
    Finished(Option<i32>),
}

/// How a command went, in the order the shell reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A prompt starts; its cells carry the id.
    Prompt(u64),
    Started,
    Finished(Option<i32>),
}

pub struct Piece<'a> {
    pub bytes: &'a [u8],
    /// Reported by the escape sequence `bytes` ends with.
    pub report: Option<Report>,
}

#[derive(Default)]
pub struct Scanner {
    state: State,
    sequence: Vec<u8>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    Escape,
    Sequence,
    /// An ESC inside the sequence, maybe the start of its terminator.
    SequenceEscape,
}

impl Scanner {
    /// `bytes` cut after each report; a sequence split across reads is still found.
    pub fn split<'a>(&mut self, bytes: &'a [u8]) -> Vec<Piece<'a>> {
        let mut pieces = Vec::new();
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(report) = self.feed(byte) {
                pieces.push(Piece { bytes: &bytes[start..=index], report: Some(report) });
                start = index + 1;
            }
        }
        if start < bytes.len() {
            pieces.push(Piece { bytes: &bytes[start..], report: None });
        }
        pieces
    }

    fn feed(&mut self, byte: u8) -> Option<Report> {
        match (self.state, byte) {
            (State::Ground, ESC) => self.state = State::Escape,
            (State::Ground, _) => {}
            (State::Escape, b']') => {
                self.sequence.clear();
                self.state = State::Sequence;
            }
            (State::Escape, ESC) => {}
            (State::Escape, _) => self.state = State::Ground,
            (State::Sequence, BEL) => return self.finish(),
            (State::Sequence, ESC) => self.state = State::SequenceEscape,
            (State::Sequence, _) if self.sequence.len() < MAX_SEQUENCE => {
                self.sequence.push(byte);
            }
            (State::Sequence, _) => self.state = State::Ground,
            (State::SequenceEscape, b'\\') => return self.finish(),
            (State::SequenceEscape, b']') => {
                self.sequence.clear();
                self.state = State::Sequence;
            }
            (State::SequenceEscape, _) => self.state = State::Ground,
        }
        None
    }

    fn finish(&mut self) -> Option<Report> {
        self.state = State::Ground;
        parse(&String::from_utf8_lossy(&self.sequence))
    }
}

fn parse(sequence: &str) -> Option<Report> {
    let (code, rest) = sequence.split_once(';').unwrap_or((sequence, ""));
    match code {
        "7" => directory(rest).map(Report::Directory),
        "133" => {
            let mut fields = rest.split(';');
            match fields.next()? {
                "A" => Some(Report::Prompt),
                "B" => Some(Report::Input),
                "C" => Some(Report::Started),
                "D" => Some(Report::Finished(fields.next().and_then(|code| code.parse().ok()))),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The path of a `file://host/path` URL, percent-decoded.
fn directory(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let mut bytes = Vec::with_capacity(path.len());
    let mut chars = path.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let high = chars.next()?;
            let low = chars.next()?;
            let hex = [high, low];
            let value = u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?;
            bytes.push(value);
        } else {
            bytes.push(byte);
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

/// The hyperlink a prompt's cells carry: drawn as a mark beside its row, not as a link.
pub fn prompt_link(id: u64) -> String {
    format!("\x1b]8;id=remacs-prompt-{id};{PROMPT_SCHEME}{id}\x1b\\")
}

pub const END_LINK: &[u8] = b"\x1b]8;;\x1b\\";
pub const PROMPT_SCHEME: &str = "remacs-prompt:";

/// The prompt id a hyperlink's URI names, if it is a prompt's.
pub fn prompt_id(uri: &str) -> Option<u64> {
    uri.strip_prefix(PROMPT_SCHEME)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reports(chunks: &[&[u8]]) -> Vec<Report> {
        let mut scanner = Scanner::default();
        chunks
            .iter()
            .flat_map(|chunk| {
                scanner
                    .split(chunk)
                    .into_iter()
                    .filter_map(|piece| piece.report)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn finds_reports_split_across_reads() {
        assert_eq!(
            reports(&[b"a\x1b]133;A\x07$ ls\x1b]13", b"3;C\x1b\\out\x1b]133;D;2\x07"]),
            vec![Report::Prompt, Report::Started, Report::Finished(Some(2))]
        );
        assert_eq!(
            reports(&[b"\x1b]7;file://mbp/Users/hk/my%20dir\x07"]),
            vec![Report::Directory(PathBuf::from("/Users/hk/my dir"))]
        );
        assert_eq!(reports(&[b"\x1b]0;title\x07\x1b[31mred"]), vec![]);
    }

    #[test]
    fn cuts_pieces_after_each_report() {
        let mut scanner = Scanner::default();
        let pieces = scanner.split(b"x\x1b]133;A\x07y");
        let cut: Vec<_> = pieces.iter().map(|piece| piece.bytes).collect();
        assert_eq!(cut, vec![&b"x\x1b]133;A\x07"[..], b"y"]);
    }
}
