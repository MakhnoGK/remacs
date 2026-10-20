use std::borrow::Cow;
use std::char::{DecodeUtf16Error, REPLACEMENT_CHARACTER};
use std::io;
use std::str;

use memchr::{memchr, memchr_iter};

/// A NUL among a file's first bytes makes it binary, as Git judges it.
const BINARY_SNIFF_LEN: usize = 8000;

/// How a file's bytes are read as the UTF-8 text a [`crate::Text`] holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Utf8,
    /// With a byte order mark, which the file keeps and the text leaves out.
    Utf16Le,
    Utf16Be,
    /// What a file that is not UTF-8 is read as: each byte is one character, so saving
    /// writes back the very bytes read. Windows-1251 and other 8-bit encodings can't be
    /// told from it without guessing, so they show as Latin-1 letters.
    Latin1,
    /// Read as Latin-1 and opened read-only.
    Binary,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Format {
    pub encoding: Encoding,
    /// Every line of the file ends in CR LF; the text holds LF alone and saving writes
    /// CR LF for each.
    pub crlf: bool,
}

/// The format of a file's `bytes`, and its text when that differs from the bytes.
pub(crate) fn decode_file(
    bytes: &[u8],
    is_utf8: impl FnOnce() -> bool,
) -> (Format, Option<Vec<u8>>) {
    let (encoding, decoded) = if let Some(utf16) = strict_utf16(bytes) {
        utf16
    } else if memchr(0, &bytes[..bytes.len().min(BINARY_SNIFF_LEN)]).is_some() {
        (Encoding::Binary, Some(latin1(bytes)))
    } else if !is_utf8() {
        (Encoding::Latin1, Some(latin1(bytes)))
    } else {
        (Encoding::Utf8, None)
    };
    let text = decoded.as_deref().unwrap_or(bytes);
    // A file with some lines ending in LF alone keeps its CRs, so saving changes no line end.
    let crlf = encoding != Encoding::Binary && has_only_crlf(text);
    let format = Format { encoding, crlf };
    (
        format,
        if crlf {
            Some(without_cr(text))
        } else {
            decoded
        },
    )
}

impl Format {
    /// The text of a file in this format, as opening it reads it, except that UTF-16 that
    /// isn't valid is read with replacement characters and each CR LF becomes LF.
    pub fn decode(self, bytes: &[u8]) -> Cow<'_, [u8]> {
        let decoded = match self.encoding {
            Encoding::Utf8 => None,
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let units = bytes.strip_prefix(self.byte_order_mark()).unwrap_or(bytes);
                let characters = utf16_characters(self.encoding, units);
                Some(
                    characters
                        .map(|character| character.unwrap_or(REPLACEMENT_CHARACTER))
                        .collect::<String>()
                        .into_bytes(),
                )
            }
            Encoding::Latin1 | Encoding::Binary => Some(latin1(bytes)),
        };
        match (self.crlf, decoded) {
            (true, decoded) => Cow::Owned(without_cr(decoded.as_deref().unwrap_or(bytes))),
            (false, Some(decoded)) => Cow::Owned(decoded),
            (false, None) => Cow::Borrowed(bytes),
        }
    }

    /// What a file in this format starts with before its text.
    pub(crate) fn byte_order_mark(self) -> &'static [u8] {
        match self.encoding {
            Encoding::Utf16Le => &[0xFF, 0xFE],
            Encoding::Utf16Be => &[0xFE, 0xFF],
            _ => &[],
        }
    }

    /// `chunk` of a text, which pieces keep whole characters in, as the file holds it.
    pub(crate) fn encode(self, chunk: &[u8]) -> io::Result<Cow<'_, [u8]>> {
        if self.encoding == Encoding::Utf8 {
            return Ok(if self.crlf {
                Cow::Owned(with_cr(chunk))
            } else {
                Cow::Borrowed(chunk)
            });
        }
        let text = str::from_utf8(chunk)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut bytes = Vec::with_capacity(chunk.len() * 2);
        for character in text.chars() {
            if character == '\n' && self.crlf {
                self.push(&mut bytes, '\r')?;
            }
            self.push(&mut bytes, character)?;
        }
        Ok(Cow::Owned(bytes))
    }

    fn push(self, bytes: &mut Vec<u8>, character: char) -> io::Result<()> {
        let mut units = [0; 2];
        match self.encoding {
            Encoding::Utf8 => {
                bytes.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes())
            }
            Encoding::Utf16Le => {
                for unit in character.encode_utf16(&mut units) {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
            }
            Encoding::Utf16Be => {
                for unit in character.encode_utf16(&mut units) {
                    bytes.extend_from_slice(&unit.to_be_bytes());
                }
            }
            Encoding::Latin1 | Encoding::Binary => {
                bytes.push(u8::try_from(character).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{character:?} can't be saved in this file's Latin-1 encoding"),
                    )
                })?)
            }
        }
        Ok(())
    }
}

/// UTF-16 after a byte order mark, if all of it is valid.
fn strict_utf16(bytes: &[u8]) -> Option<(Encoding, Option<Vec<u8>>)> {
    let (encoding, units) = match bytes {
        [0xFF, 0xFE, units @ ..] => (Encoding::Utf16Le, units),
        [0xFE, 0xFF, units @ ..] => (Encoding::Utf16Be, units),
        _ => return None,
    };
    if units.len() % 2 != 0 {
        return None;
    }
    let text: String = utf16_characters(encoding, units)
        .collect::<Result<_, _>>()
        .ok()?;
    Some((encoding, Some(text.into_bytes())))
}

fn utf16_characters(
    encoding: Encoding,
    units: &[u8],
) -> impl Iterator<Item = Result<char, DecodeUtf16Error>> + '_ {
    char::decode_utf16(units.as_chunks().0.iter().map(move |&unit| {
        if encoding == Encoding::Utf16Le {
            u16::from_le_bytes(unit)
        } else {
            u16::from_be_bytes(unit)
        }
    }))
}

fn latin1(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|&byte| char::from(byte))
        .collect::<String>()
        .into_bytes()
}

fn has_only_crlf(text: &[u8]) -> bool {
    let mut newlines = memchr_iter(b'\n', text).peekable();
    newlines.peek().is_some() && newlines.all(|newline| text[..newline].ends_with(b"\r"))
}

fn without_cr(text: &[u8]) -> Vec<u8> {
    let mut lf = Vec::with_capacity(text.len());
    let mut from = 0;
    for newline in memchr_iter(b'\n', text) {
        if text[..newline].ends_with(b"\r") {
            lf.extend_from_slice(&text[from..newline - 1]);
            from = newline;
        }
    }
    lf.extend_from_slice(&text[from..]);
    lf
}

fn with_cr(text: &[u8]) -> Vec<u8> {
    let mut crlf = Vec::with_capacity(text.len() + text.len() / 16);
    let mut from = 0;
    for newline in memchr_iter(b'\n', text) {
        crlf.extend_from_slice(&text[from..newline]);
        crlf.extend_from_slice(b"\r\n");
        from = newline + 1;
    }
    crlf.extend_from_slice(&text[from..]);
    crlf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8]) -> (Format, Vec<u8>) {
        let (format, text) = decode_file(bytes, || str::from_utf8(bytes).is_ok());
        (format, text.unwrap_or_else(|| bytes.to_vec()))
    }

    fn written(format: Format, text: &[u8]) -> Vec<u8> {
        [format.byte_order_mark(), &format.encode(text).unwrap()].concat()
    }

    fn assert_round_trip(file: &[u8], encoding: Encoding, crlf: bool, text: &str) {
        let (format, read_text) = read(file);
        assert_eq!(format, Format { encoding, crlf });
        assert_eq!(String::from_utf8(read_text).unwrap(), text);
        assert_eq!(written(format, text.as_bytes()), file);
        assert_eq!(&*format.decode(file), text.as_bytes());
    }

    #[test]
    fn utf8_is_kept_as_read() {
        let (format, text) = decode_file("é\n".as_bytes(), || true);
        assert_eq!(format, Format::default());
        assert_eq!(text, None);
        assert_round_trip(
            "\u{feff}a\nb".as_bytes(),
            Encoding::Utf8,
            false,
            "\u{feff}a\nb",
        );
    }

    #[test]
    fn latin1_reads_each_byte_as_a_character_and_writes_it_back() {
        assert_round_trip(b"caf\xe9\n", Encoding::Latin1, false, "café\n");
        // Windows-1251 "Привет": Latin-1 letters, the same bytes back.
        assert_round_trip(
            b"\xcf\xf0\xe8\xe2\xe5\xf2",
            Encoding::Latin1,
            false,
            "Ïðèâåò",
        );
        let error = Format {
            encoding: Encoding::Latin1,
            crlf: false,
        }
        .encode("Ж".as_bytes())
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn utf16_with_a_byte_order_mark_is_decoded_and_encoded_back() {
        let text = "a€😀\n";
        let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_round_trip(
            &[&[0xFF, 0xFE], &le[..]].concat(),
            Encoding::Utf16Le,
            false,
            text,
        );
        assert_round_trip(
            &[&[0xFE, 0xFF], &be[..]].concat(),
            Encoding::Utf16Be,
            false,
            text,
        );
        let crlf: Vec<u8> = "x\r\ny\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_round_trip(
            &[&[0xFF, 0xFE], &crlf[..]].concat(),
            Encoding::Utf16Le,
            true,
            "x\ny\n",
        );
    }

    #[test]
    fn invalid_utf16_is_binary() {
        // An unpaired surrogate.
        let (format, _) = read(&[0xFF, 0xFE, 0x00, 0xD8, b'a', 0]);
        assert_eq!(format.encoding, Encoding::Binary);
    }

    #[test]
    fn a_nul_byte_makes_a_file_binary_and_its_bytes_are_kept() {
        let file = b"\x7fELF\x02\x01\x00\x00\xff\r\n";
        assert_round_trip(
            file,
            Encoding::Binary,
            false,
            "\u{7f}ELF\u{2}\u{1}\0\0ÿ\r\n",
        );
    }

    #[test]
    fn crlf_files_are_read_with_lf_and_written_with_crlf() {
        assert_round_trip(b"a\r\nb\r\n", Encoding::Utf8, true, "a\nb\n");
        assert_round_trip(b"caf\xe9\r\n", Encoding::Latin1, true, "café\n");
        let format = Format {
            encoding: Encoding::Utf8,
            crlf: true,
        };
        assert_eq!(written(format, b"new\nline\n"), b"new\r\nline\r\n");
    }

    #[test]
    fn mixed_line_ends_keep_their_crs() {
        assert_round_trip(b"a\r\nb\nc\r\n", Encoding::Utf8, false, "a\r\nb\nc\r\n");
        assert_round_trip(b"no line end\r", Encoding::Utf8, false, "no line end\r");
    }
}
