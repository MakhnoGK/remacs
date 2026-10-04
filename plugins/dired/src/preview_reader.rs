use std::fs::{self, File};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::SystemTime;

use remacs_core::Notifier;

use crate::entry::compare_names;

/// Enough to fill the preview of a tall window.
const HEAD_LINES: usize = 120;
/// Columns kept of a line; the preview shows far fewer.
const MAX_COLUMNS: usize = 240;
/// Read to find the first lines and to tell text from binary.
const HEAD_BYTES: usize = 32 * 1024;
const MAX_ITEMS: usize = 200;
const COUNT_CHUNK_BYTES: usize = 1024 * 1024;
const TAB_WIDTH: usize = 4;

/// A file or directory to preview; a change of its size or modified time asks again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub is_directory: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// The major mode the file is written in, to highlight it.
    pub mode: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contents {
    pub request: PreviewRequest,
    pub body: Body,
    /// Of a text file, once counted.
    pub line_count: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// The first lines, tabs expanded and long lines cut.
    Text(String),
    Binary,
    /// The first names of a directory, directories first, and whether each is one.
    Items(Vec<(String, bool)>),
    Failed(String),
}

/// Reads previews on a background thread: the head of a file at once, its line count
/// after, given up when another preview is asked for.
pub struct PreviewReader {
    requests: Sender<PreviewRequest>,
    contents: Receiver<Contents>,
}

impl PreviewReader {
    pub fn start(notifier: Notifier) -> Self {
        let (requests, received) = mpsc::channel();
        let (send_contents, contents) = mpsc::channel();
        thread::Builder::new()
            .name("dired-preview".into())
            .spawn(move || run(&received, &send_contents, &notifier))
            .expect("spawning the dired preview thread");
        Self { requests, contents }
    }

    pub fn request(&self, request: PreviewRequest) {
        let _ = self.requests.send(request);
    }

    /// The newest contents read since the last call, if any.
    pub fn latest(&self) -> Option<Contents> {
        self.contents.try_iter().last()
    }
}

enum LineCount {
    Counted(u64),
    Failed,
    Superseded(PreviewRequest),
    Stopped,
}

fn run(requests: &Receiver<PreviewRequest>, contents: &Sender<Contents>, notifier: &Notifier) {
    let mut next = requests.recv().ok();
    while let Some(request) = next.take() {
        let request = requests.try_iter().last().unwrap_or(request);
        let mut preview = read(request);
        let is_text = matches!(preview.body, Body::Text(_));
        if contents.send(preview.clone()).is_err() {
            return;
        }
        notifier();
        if is_text {
            match count_lines(&preview.request, requests) {
                LineCount::Counted(count) => {
                    preview.line_count = Some(count);
                    if contents.send(preview).is_err() {
                        return;
                    }
                    notifier();
                }
                LineCount::Failed => {}
                LineCount::Superseded(newer) => {
                    next = Some(newer);
                    continue;
                }
                LineCount::Stopped => return,
            }
        }
        next = requests.recv().ok();
    }
}

fn read(request: PreviewRequest) -> Contents {
    let body = if request.is_directory {
        read_items(&request)
    } else {
        read_head(&request)
    };
    Contents {
        body: body.unwrap_or_else(|error| Body::Failed(error.to_string())),
        request,
        line_count: None,
    }
}

fn read_items(request: &PreviewRequest) -> io::Result<Body> {
    let mut items: Vec<(String, bool)> = fs::read_dir(&request.path)?
        .filter_map(Result::ok)
        .map(|item| {
            let is_directory = item
                .file_type()
                .is_ok_and(|kind| kind.is_dir() || (kind.is_symlink() && item.path().is_dir()));
            (
                item.file_name().to_string_lossy().into_owned(),
                is_directory,
            )
        })
        .collect();
    items.sort_by(|(a, a_is_directory), (b, b_is_directory)| {
        b_is_directory
            .cmp(a_is_directory)
            .then_with(|| compare_names(a, b))
    });
    items.truncate(MAX_ITEMS);
    Ok(Body::Items(items))
}

fn read_head(request: &PreviewRequest) -> io::Result<Body> {
    let mut head = Vec::with_capacity(HEAD_BYTES);
    File::open(&request.path)?
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut head)?;
    if memchr::memchr(0, &head).is_some() {
        return Ok(Body::Binary);
    }
    Ok(Body::Text(head_lines(&head, head.len() == HEAD_BYTES)))
}

/// The first lines of `head`; when the file goes on, its last line may be cut short
/// mid-character, so it is left out.
fn head_lines(head: &[u8], truncated: bool) -> String {
    let text = String::from_utf8_lossy(head);
    let mut lines: Vec<&str> = text.lines().take(HEAD_LINES + 1).collect();
    if lines.len() > HEAD_LINES || (truncated && lines.len() > 1) {
        lines.pop();
    }
    let mut shown = String::with_capacity(text.len().min(HEAD_BYTES));
    for (index, line) in lines.into_iter().enumerate() {
        if index > 0 {
            shown.push('\n');
        }
        push_display(line, &mut shown);
    }
    shown
}

/// `line` as drawn: tabs expanded, other control characters blanked, cut at
/// `MAX_COLUMNS`.
fn push_display(line: &str, out: &mut String) {
    let mut column = 0;
    for character in line.chars() {
        if column >= MAX_COLUMNS {
            break;
        }
        if character == '\t' {
            let width = TAB_WIDTH - column % TAB_WIDTH;
            out.extend(std::iter::repeat_n(' ', width));
            column += width;
        } else {
            out.push(if character.is_control() {
                ' '
            } else {
                character
            });
            column += 1;
        }
    }
}

/// Lines of the file, counted a chunk at a time so a newer request can cut it short.
fn count_lines(request: &PreviewRequest, requests: &Receiver<PreviewRequest>) -> LineCount {
    let Ok(mut file) = File::open(&request.path) else {
        return LineCount::Failed;
    };
    let mut chunk = vec![0; COUNT_CHUNK_BYTES];
    let (mut newlines, mut last_byte) = (0, None);
    loop {
        match requests.try_recv() {
            Ok(newer) => return LineCount::Superseded(newer),
            Err(TryRecvError::Disconnected) => return LineCount::Stopped,
            Err(TryRecvError::Empty) => {}
        }
        let read = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return LineCount::Failed,
        };
        newlines += memchr::memchr_iter(b'\n', &chunk[..read]).count() as u64;
        last_byte = Some(chunk[read - 1]);
    }
    let unterminated = last_byte.is_some_and(|byte| byte != b'\n');
    LineCount::Counted(newlines + u64::from(unterminated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_expands_tabs_and_leaves_out_a_cut_last_line() {
        assert_eq!(head_lines(b"a\tb\n\tc\n", false), "a   b\n    c");
        assert_eq!(head_lines(b"one\ntwo\nthr", true), "one\ntwo");
        assert_eq!(head_lines(b"only", true), "only");
        let long = "x".repeat(MAX_COLUMNS + 10);
        assert_eq!(head_lines(long.as_bytes(), false).len(), MAX_COLUMNS);
    }
}
