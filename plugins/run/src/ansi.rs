//! A program's output as a terminal would show it: text in the styles its SGR escape codes
//! ask for, carriage returns that redraw the line, and the rest of the escape codes left
//! out.

use remacs_core::{ConsoleColor, ConsoleStyle};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// Text, newlines included, in one style.
    Text(String, ConsoleStyle),
    /// A carriage return not followed by a newline: what follows replaces the line.
    Return,
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    /// Inside `ESC [`, its parameters so far.
    Csi(String),
    /// Inside `ESC ]`, up to BEL or `ESC \`.
    Osc,
    OscEscape,
}

/// Turns output, fed in chunks as it comes, into styled segments; escape codes and
/// characters split across chunks are put together.
#[derive(Default)]
pub struct Parser {
    state: State,
    style: ConsoleStyle,
    /// Bytes of text not yet in a segment, possibly ending in part of a character.
    text: Vec<u8>,
    /// A carriage return was last, so a newline may still make it a line end.
    after_return: bool,
}

impl Parser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Segment> {
        let mut segments = Vec::new();
        for &byte in bytes {
            match std::mem::take(&mut self.state) {
                State::Ground => self.ground(byte, &mut segments),
                State::Escape => {
                    self.state = match byte {
                        b'[' => State::Csi(String::new()),
                        b']' => State::Osc,
                        _ => State::Ground,
                    }
                }
                State::Csi(mut parameters) => {
                    if (0x40..=0x7e).contains(&byte) {
                        if byte == b'm' {
                            self.flush(&mut segments, false);
                            self.select_graphics(&parameters);
                        }
                    } else {
                        parameters.push(char::from(byte));
                        self.state = State::Csi(parameters);
                    }
                }
                State::Osc => {
                    self.state = match byte {
                        0x07 => State::Ground,
                        0x1b => State::OscEscape,
                        _ => State::Osc,
                    }
                }
                State::OscEscape => {
                    self.state = if byte == b'\\' {
                        State::Ground
                    } else {
                        State::Osc
                    }
                }
            }
        }
        self.flush(&mut segments, true);
        segments
    }

    fn ground(&mut self, byte: u8, segments: &mut Vec<Segment>) {
        if std::mem::take(&mut self.after_return) && byte != b'\n' {
            self.flush(segments, false);
            segments.push(Segment::Return);
        }
        match byte {
            0x1b => self.state = State::Escape,
            b'\r' => self.after_return = true,
            b'\n' | b'\t' => self.text.push(byte),
            // Other control characters, e.g. a bell or a backspace, draw nothing.
            0x00..=0x1f | 0x7f => {}
            _ => self.text.push(byte),
        }
    }

    /// Ends the text so far as a segment; at the end of a chunk, part of a character is
    /// kept for the next one.
    fn flush(&mut self, segments: &mut Vec<Segment>, end_of_chunk: bool) {
        if self.text.is_empty() {
            return;
        }
        let kept = match std::str::from_utf8(&self.text) {
            Err(error) if end_of_chunk && error.error_len().is_none() => {
                self.text.split_off(error.valid_up_to())
            }
            _ => Vec::new(),
        };
        let text = String::from_utf8_lossy(&self.text).into_owned();
        self.text = kept;
        if text.is_empty() {
            return;
        }
        match segments.last_mut() {
            Some(Segment::Text(last, style)) if *style == self.style => last.push_str(&text),
            _ => segments.push(Segment::Text(text, self.style)),
        }
    }

    fn select_graphics(&mut self, parameters: &str) {
        let codes: Vec<u32> = parameters
            .split([';', ':'])
            .map(|code| code.parse().unwrap_or(0))
            .collect();
        let mut codes = codes.iter().copied();
        while let Some(code) = codes.next() {
            let style = &mut self.style;
            match code {
                0 => *style = ConsoleStyle::default(),
                1 => style.bold = true,
                2 => style.dim = true,
                3 => style.italic = true,
                4 => style.underline = true,
                22 => (style.bold, style.dim) = (false, false),
                23 => style.italic = false,
                24 => style.underline = false,
                30..=37 => style.color = Some(ConsoleColor::Indexed((code - 30) as u8)),
                90..=97 => style.color = Some(ConsoleColor::Indexed((code - 90 + 8) as u8)),
                39 => style.color = None,
                38 => style.color = extended_color(&mut codes),
                // Backgrounds are left out, but their colors' codes must be skipped.
                48 => {
                    extended_color(&mut codes);
                }
                _ => {}
            }
        }
    }
}

/// The color after `38` or `48`: `5;n` from the 256, or `2;r;g;b`.
fn extended_color(codes: &mut impl Iterator<Item = u32>) -> Option<ConsoleColor> {
    let byte = |code: Option<u32>| u8::try_from(code?).ok();
    match codes.next()? {
        5 => byte(codes.next()).map(ConsoleColor::Indexed),
        2 => {
            let (red, green, blue) = (
                byte(codes.next())?,
                byte(codes.next())?,
                byte(codes.next())?,
            );
            Some(ConsoleColor::Rgb(
                u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue),
            ))
        }
        _ => None,
    }
}
