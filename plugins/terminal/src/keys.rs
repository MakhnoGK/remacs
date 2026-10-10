//! What a key sends to a terminal's program, as xterm encodes it; Option is Meta.

use remacs_core::{Key, KeyCode, Modifiers};

/// The bytes for `key`; none for keys a terminal has no code for, like ⌘ chords.
/// `application_cursor` is the mode in which programs like `less` want arrows as SS3.
pub fn encode(key: Key, application_cursor: bool) -> Option<Vec<u8>> {
    let Modifiers {
        control,
        meta,
        shift,
        super_key,
    } = key.modifiers;
    if super_key {
        return None;
    }
    // xterm's modifier parameter: 1 plus shift 1, alt 2, control 4.
    let parameter = 1 + u8::from(shift) + 2 * u8::from(meta) + 4 * u8::from(control);
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
        encode(Key::parse(notation).unwrap(), false).unwrap()
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
        assert_eq!(
            encode(Key::parse("<down>").unwrap(), true).unwrap(),
            b"\x1bOB"
        );
        assert_eq!(encode(Key::parse("s-c").unwrap(), false), None);
    }

    #[test]
    fn pastes_bracketed_when_asked() {
        assert_eq!(paste("a\nb", false), b"a\rb");
        assert_eq!(paste("x\x1b[201~y", true), b"\x1b[200~xy\x1b[201~");
    }
}
