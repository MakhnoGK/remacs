//! What a key sends to a terminal's program: as xterm encodes it, or by the kitty
//! keyboard protocol once the program asks for it; Option is Meta.

use alacritty_terminal::term::TermMode;
use remacs_core::{Key, KeyCode, Modifiers};

/// The bytes for `key` in the terminal's `mode`; none for keys a terminal has no code
/// for, like ⌘ chords.
pub fn encode(key: Key, mode: TermMode) -> Option<Vec<u8>> {
    if key.modifiers.super_key {
        return None;
    }
    kitty(key, mode).or_else(|| legacy(key, mode.contains(TermMode::APP_CURSOR)))
}

/// xterm's modifier parameter: 1 plus shift 1, alt 2, control 4.
fn parameter(
    Modifiers {
        control,
        meta,
        shift,
        ..
    }: Modifiers,
) -> u8 {
    1 + u8::from(shift) + 2 * u8::from(meta) + 4 * u8::from(control)
}

/// The kitty keyboard protocol's `CSI code[:shifted];modifiers[;text] u`, for the keys
/// legacy encodings confuse; none where the legacy bytes are what the protocol wants
/// too: arrows and function keys, and text typed with no more than shift.
fn kitty(key: Key, mode: TermMode) -> Option<Vec<u8>> {
    let every_key = mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC);
    if !every_key && !mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
        return None;
    }
    let mut modifiers = key.modifiers;
    let commanded = modifiers.control || modifiers.meta;
    let (code, shifted, text) = match key.code {
        KeyCode::Char(character) => {
            if !every_key && !commanded {
                return None;
            }
            // Shift is folded into a letter; the protocol reports the key unshifted.
            let base = character.to_lowercase().next().unwrap_or(character);
            modifiers.shift |= base != character;
            let shifted = (base != character).then_some(character);
            (base, shifted, (!commanded).then_some(character))
        }
        KeyCode::Escape => ('\x1b', None, None),
        KeyCode::Return | KeyCode::Tab | KeyCode::Backspace
            if every_key || modifiers != Modifiers::default() =>
        {
            let code = match key.code {
                KeyCode::Return => '\r',
                KeyCode::Tab => '\t',
                _ => '\x7f',
            };
            (code, None, None)
        }
        _ => return None,
    };
    let mut sequence = format!("\x1b[{}", u32::from(code));
    if let Some(shifted) = shifted.filter(|_| mode.contains(TermMode::REPORT_ALTERNATE_KEYS)) {
        sequence += &format!(":{}", u32::from(shifted));
    }
    let parameter = parameter(modifiers);
    let text = text.filter(|_| every_key && mode.contains(TermMode::REPORT_ASSOCIATED_TEXT));
    if parameter > 1 || text.is_some() {
        sequence += &format!(";{parameter}");
    }
    if let Some(text) = text {
        sequence += &format!(";{}", u32::from(text));
    }
    sequence.push('u');
    Some(sequence.into_bytes())
}

/// As xterm encodes `key`; `application_cursor` is the mode in which programs like
/// `less` want arrows as SS3.
fn legacy(key: Key, application_cursor: bool) -> Option<Vec<u8>> {
    let Modifiers {
        control,
        meta,
        shift,
        ..
    } = key.modifiers;
    let parameter = parameter(key.modifiers);
    let modified = parameter > 1;
    let csi = |final_byte: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{parameter}{final_byte}").into_bytes()
        } else if application_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |number: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{number};{parameter}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        }
    };
    let with_meta = |bytes: Vec<u8>| -> Vec<u8> {
        if meta {
            [b"\x1b".as_slice(), &bytes].concat()
        } else {
            bytes
        }
    };
    let bytes = match key.code {
        KeyCode::Char(character) if control => with_meta(vec![control_byte(character)?]),
        KeyCode::Char(character) => with_meta(character.to_string().into_bytes()),
        KeyCode::Return => with_meta(b"\r".to_vec()),
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => with_meta(b"\t".to_vec()),
        KeyCode::Backspace if control => with_meta(vec![0x08]),
        KeyCode::Backspace => with_meta(vec![0x7f]),
        KeyCode::Escape => with_meta(vec![0x1b]),
        KeyCode::Up => csi('A'),
        KeyCode::Down => csi('B'),
        KeyCode::Right => csi('C'),
        KeyCode::Left => csi('D'),
        KeyCode::Home => csi('H'),
        KeyCode::End => csi('F'),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::Function(number @ 1..=4) => {
            let final_byte = (b'P' + number - 1) as char;
            if modified {
                format!("\x1b[1;{parameter}{final_byte}").into_bytes()
            } else {
                format!("\x1bO{final_byte}").into_bytes()
            }
        }
        KeyCode::Function(number) => tilde(match number {
            5 => 15,
            6..=10 => number + 11,
            _ => number + 12,
        }),
    };
    Some(bytes)
}

/// The control character a `C-` chord types, as on a VT220 keyboard.
fn control_byte(character: char) -> Option<u8> {
    let byte = match character.to_ascii_lowercase() {
        letter @ 'a'..='z' => letter as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '/' | '-' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    };
    Some(byte)
}

/// Pasted text: bracketed when the program asked for it, so a shell does not run its
/// lines as they come; without its own bracket markers, so it cannot end the paste.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let text = text.replace("\x1b[201~", "");
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(notation: &str) -> Vec<u8> {
        in_mode(notation, TermMode::empty())
    }

    fn in_mode(notation: &str, mode: TermMode) -> Vec<u8> {
        encode(Key::parse(notation).unwrap(), mode).unwrap()
    }

    fn kitty(notation: &str) -> String {
        String::from_utf8(in_mode(notation, TermMode::DISAMBIGUATE_ESC_CODES)).unwrap()
    }

    #[test]
    fn tells_keys_apart_by_the_kitty_protocol() {
        assert_eq!(kitty("S-RET"), "\x1b[13;2u");
        assert_eq!(kitty("C-RET"), "\x1b[13;5u");
        assert_eq!(kitty("RET"), "\r");
        assert_eq!(kitty("TAB"), "\t");
        assert_eq!(kitty("S-TAB"), "\x1b[9;2u");
        assert_eq!(kitty("ESC"), "\x1b[27u");
        assert_eq!(kitty("C-c"), "\x1b[99;5u");
        assert_eq!(kitty("C-i"), "\x1b[105;5u");
        assert_eq!(kitty("M-b"), "\x1b[98;3u");
        assert_eq!(kitty("C-M-B"), "\x1b[98;8u");
        assert_eq!(kitty("A"), "A");
        assert_eq!(kitty("ж"), "ж");
        assert_eq!(kitty("C-<left>"), "\x1b[1;5D");
        let every_key = TermMode::REPORT_ALL_KEYS_AS_ESC
            | TermMode::REPORT_ALTERNATE_KEYS
            | TermMode::REPORT_ASSOCIATED_TEXT;
        let every = |notation| String::from_utf8(in_mode(notation, every_key)).unwrap();
        assert_eq!(every("a"), "\x1b[97;1;97u");
        assert_eq!(every("A"), "\x1b[97:65;2;65u");
        assert_eq!(every("RET"), "\x1b[13u");
    }

    #[test]
    fn encodes_keys_as_xterm_does() {
        assert_eq!(bytes("a"), b"a");
        assert_eq!(bytes("ж"), "ж".as_bytes());
        assert_eq!(bytes("C-c"), [0x03]);
        assert_eq!(bytes("C-SPC"), [0x00]);
        assert_eq!(bytes("M-b"), b"\x1bb");
        assert_eq!(bytes("C-M-h"), b"\x1b\x08");
        assert_eq!(bytes("RET"), b"\r");
        assert_eq!(bytes("DEL"), [0x7f]);
        assert_eq!(bytes("S-TAB"), b"\x1b[Z");
        assert_eq!(bytes("<up>"), b"\x1b[A");
        assert_eq!(bytes("C-<left>"), b"\x1b[1;5D");
        assert_eq!(bytes("M-<right>"), b"\x1b[1;3C");
        assert_eq!(bytes("<prior>"), b"\x1b[5~");
        assert_eq!(bytes("<f1>"), b"\x1bOP");
        assert_eq!(bytes("<f5>"), b"\x1b[15~");
        assert_eq!(bytes("<f12>"), b"\x1b[24~");
        assert_eq!(bytes("S-<f8>"), b"\x1b[19;2~");
        assert_eq!(in_mode("<down>", TermMode::APP_CURSOR), b"\x1bOB");
        assert_eq!(encode(Key::parse("s-c").unwrap(), TermMode::empty()), None);
    }

    #[test]
    fn pastes_bracketed_when_asked() {
        assert_eq!(paste("a\nb", false), b"a\rb");
        assert_eq!(paste("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
    }
}
