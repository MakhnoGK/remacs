use std::borrow::Cow;
use std::io::{self, Read, Write};
use std::mem;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{
    BIG5, CoderResult, EUC_JP, EUC_KR, Encoder, EncoderResult, Encoding, GB18030, GBK, IBM866,
    ISO_2022_JP, ISO_8859_2, ISO_8859_3, ISO_8859_4, ISO_8859_5, ISO_8859_6, ISO_8859_7,
    ISO_8859_8, ISO_8859_10, ISO_8859_13, ISO_8859_14, ISO_8859_15, ISO_8859_16, KOI8_R, KOI8_U,
    MACINTOSH, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_874, WINDOWS_1250, WINDOWS_1251,
    WINDOWS_1252, WINDOWS_1253, WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257,
    WINDOWS_1258, X_MAC_CYRILLIC,
};
use memchr::{memchr, memchr_iter};

/// A NUL among a file's first bytes makes it binary, as Git judges it.
pub(crate) const BINARY_SNIFF_LEN: usize = 8000;
/// How much of a file guessing its encoding reads.
pub(crate) const DETECTION_SAMPLE_LEN: usize = 64 * 1024;
/// Bytes read, decoded or encoded at a time, which bounds the memory doing so takes.
const CHUNK_LEN: usize = 1 << 20;

/// How a file's bytes are read as the UTF-8 text a [`crate::Text`] holds, and written back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub encoding: &'static Encoding,
    /// The file starts with a UTF-16 byte order mark, which the text leaves out.
    pub byte_order_mark: bool,
    /// Every line of the file ends in CR LF; the text holds LF alone.
    pub crlf: bool,
    /// The text is only a notice: a binary file is neither read nor saved.
    pub binary: bool,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            encoding: UTF_8,
            byte_order_mark: false,
            crlf: false,
            binary: false,
        }
    }
}

impl Format {
    /// UTF-8 with every line ending in CR LF.
    pub(crate) fn crlf() -> Self {
        Self {
            crlf: true,
            ..Self::default()
        }
    }
}

/// Encodings a file can be read in, for choosing one by name.
pub fn encodings() -> [&'static Encoding; 37] {
    [
        UTF_8,
        UTF_16LE,
        UTF_16BE,
        WINDOWS_1250,
        WINDOWS_1251,
        WINDOWS_1252,
        WINDOWS_1253,
        WINDOWS_1254,
        WINDOWS_1255,
        WINDOWS_1256,
        WINDOWS_1257,
        WINDOWS_1258,
        WINDOWS_874,
        ISO_8859_2,
        ISO_8859_3,
        ISO_8859_4,
        ISO_8859_5,
        ISO_8859_6,
        ISO_8859_7,
        ISO_8859_8,
        ISO_8859_10,
        ISO_8859_13,
        ISO_8859_14,
        ISO_8859_15,
        ISO_8859_16,
        KOI8_R,
        KOI8_U,
        IBM866,
        X_MAC_CYRILLIC,
        MACINTOSH,
        SHIFT_JIS,
        EUC_JP,
        ISO_2022_JP,
        EUC_KR,
        GBK,
        GB18030,
        BIG5,
    ]
}

pub(crate) fn is_binary(head: &[u8]) -> bool {
    memchr(0, &head[..head.len().min(BINARY_SNIFF_LEN)]).is_some()
}

/// The UTF-16 encoding whose byte order mark `head` starts with.
pub(crate) fn utf16_byte_order_mark(head: &[u8]) -> Option<&'static Encoding> {
    Encoding::for_bom(head)
        .map(|(encoding, _)| encoding)
        .filter(|&encoding| encoding != UTF_8)
}

pub(crate) fn first_line_ends_in_crlf(head: &[u8]) -> bool {
    memchr(b'\n', head).is_some_and(|newline| head[..newline].ends_with(b"\r"))
}

/// The legacy encoding `sample`, which isn't UTF-8, is likeliest in; `whole` when it is
/// the whole file.
pub(crate) fn detect(sample: &[u8], whole: bool) -> &'static Encoding {
    // ISO-2022-JP is safe to guess where nothing runs scripts, as 0.1 always did.
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    detector.feed(sample, whole);
    detector.guess(None, Utf8Detection::Deny)
}

/// Why reading a file in a format stopped.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// An LF without a CR, reading a file as CRLF.
    LoneLf,
    /// Bytes the encoding has no character for.
    Malformed,
}

/// Writes the text of `input`, a file in `format`, to `output` a chunk at a time; whether
/// its lines end in CR LF, when `format.crlf` asks to drop those CRs.
pub(crate) fn decode(
    mut input: impl Read,
    format: Format,
    output: &mut impl Write,
) -> io::Result<Result<bool, Stop>> {
    let mut decoder = if format.byte_order_mark {
        format.encoding.new_decoder_with_bom_removal()
    } else {
        format.encoding.new_decoder_without_bom_handling()
    };
    let mut raw = vec![0; CHUNK_LEN];
    let mut text = vec![0; 2 * CHUNK_LEN];
    let mut line_ends = LineEnds::default();
    loop {
        let read = read_full(&mut input, &mut raw)?;
        let last = read < raw.len();
        let mut unread = &raw[..read];
        loop {
            let (result, consumed, written, malformed) =
                decoder.decode_to_utf8(unread, &mut text, last);
            if malformed {
                return Ok(Err(Stop::Malformed));
            }
            let decoded = &text[..written];
            if !format.crlf {
                output.write_all(decoded)?;
            } else if !line_ends.write_without_cr(decoded, output)? {
                return Ok(Err(Stop::LoneLf));
            }
            unread = &unread[consumed..];
            if result == CoderResult::InputEmpty {
                break;
            }
        }
        if last {
            break;
        }
    }
    if line_ends.cr_pending {
        output.write_all(b"\r")?;
    }
    Ok(Ok(line_ends.crlf_seen))
}

#[derive(Default)]
struct LineEnds {
    /// The last chunk ended in a CR, written or dropped once the next shows what follows.
    cr_pending: bool,
    crlf_seen: bool,
}

impl LineEnds {
    /// Writes `text` without the CR of each CR LF; false at an LF without one.
    fn write_without_cr(&mut self, text: &[u8], output: &mut impl Write) -> io::Result<bool> {
        if text.is_empty() {
            return Ok(true);
        }
        let cr_before = mem::take(&mut self.cr_pending);
        if cr_before && text[0] != b'\n' {
            output.write_all(b"\r")?;
        }
        let mut from = 0;
        for newline in memchr_iter(b'\n', text) {
            let has_cr = match newline {
                0 => cr_before,
                _ => text[newline - 1] == b'\r',
            };
            if !has_cr {
                return Ok(false);
            }
            self.crlf_seen = true;
            output.write_all(&text[from..newline.saturating_sub(1)])?;
            from = newline;
        }
        let rest = &text[from..];
        self.cr_pending = rest.ends_with(b"\r");
        output.write_all(&rest[..rest.len() - usize::from(self.cr_pending)])?;
        Ok(true)
    }
}

fn read_full(input: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

impl Format {
    /// The text of a file in this format as opening reads it, except that bytes the
    /// encoding has no character for become replacement characters and every CR LF an LF.
    pub fn decode(self, bytes: &[u8]) -> Cow<'_, [u8]> {
        let text = if self.encoding == UTF_8 || self.binary {
            Cow::Borrowed(bytes)
        } else {
            match self.encoding.decode_with_bom_removal(bytes).0 {
                Cow::Borrowed(text) => Cow::Borrowed(text.as_bytes()),
                Cow::Owned(text) => Cow::Owned(text.into_bytes()),
            }
        };
        if self.crlf {
            Cow::Owned(without_cr(&text))
        } else {
            text
        }
    }

    /// Writes `chunks` of a text, whole characters each, as a file in this format holds
    /// them; fails at a character the encoding has none for.
    pub fn write<'a>(
        self,
        chunks: impl Iterator<Item = &'a [u8]>,
        output: &mut impl Write,
    ) -> io::Result<()> {
        if self.binary {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "A binary file isn't shown, so it can't be saved",
            ));
        }
        if self.encoding == UTF_8 {
            for chunk in chunks {
                if self.crlf {
                    write_with_cr(chunk, output)?;
                } else {
                    output.write_all(chunk)?;
                }
            }
            return Ok(());
        }
        let mut encoded = Encoded {
            format: self,
            encoder: self.encoding.new_encoder(),
            buffer: vec![0; CHUNK_LEN],
            output,
        };
        if self.byte_order_mark {
            encoded.put("\u{feff}", false)?;
        }
        for chunk in chunks {
            let text = simdutf8::basic::from_utf8(chunk)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            for line in text.split_inclusive('\n') {
                match line.strip_suffix('\n').filter(|_| self.crlf) {
                    Some(content) => {
                        encoded.put(content, false)?;
                        encoded.put("\r\n", false)?;
                    }
                    None => encoded.put(line, false)?,
                }
            }
        }
        encoded.put("", true)
    }
}

struct Encoded<'a, W> {
    format: Format,
    /// Unused for UTF-16, which encoding_rs only decodes.
    encoder: Encoder,
    buffer: Vec<u8>,
    output: &'a mut W,
}

impl<W: Write> Encoded<'_, W> {
    fn put(&mut self, mut text: &str, last: bool) -> io::Result<()> {
        let encoding = self.format.encoding;
        if encoding == UTF_16LE || encoding == UTF_16BE {
            let unit_bytes =
                [u16::to_be_bytes, u16::to_le_bytes][usize::from(encoding == UTF_16LE)];
            for unit in text.encode_utf16() {
                self.output.write_all(&unit_bytes(unit))?;
            }
            return Ok(());
        }
        loop {
            let (result, read, written) =
                self.encoder
                    .encode_from_utf8_without_replacement(text, &mut self.buffer, last);
            self.output.write_all(&self.buffer[..written])?;
            text = &text[read..];
            match result {
                EncoderResult::InputEmpty => return Ok(()),
                EncoderResult::OutputFull => {}
                EncoderResult::Unmappable(character) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{character:?} can't be saved in this file's encoding, {}",
                            self.format.encoding.name()
                        ),
                    ));
                }
            }
        }
    }
}

fn write_with_cr(text: &[u8], output: &mut impl Write) -> io::Result<()> {
    let mut from = 0;
    for newline in memchr_iter(b'\n', text) {
        output.write_all(&text[from..newline])?;
        output.write_all(b"\r\n")?;
        from = newline + 1;
    }
    output.write_all(&text[from..])
}

/// `text` with the CR of each CR LF dropped, whether or not every LF has one.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8], format: Format) -> Result<(bool, Vec<u8>), Stop> {
        let mut text = Vec::new();
        let crlf = decode(bytes, format, &mut text).unwrap()?;
        Ok((crlf, text))
    }

    fn written(format: Format, chunks: &[&str]) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        format.write(chunks.iter().map(|chunk| chunk.as_bytes()), &mut bytes)?;
        Ok(bytes)
    }

    fn format(encoding: &'static Encoding, crlf: bool) -> Format {
        Format {
            encoding,
            crlf,
            byte_order_mark: encoding == UTF_16LE || encoding == UTF_16BE,
            ..Format::default()
        }
    }

    fn assert_round_trip(file: &[u8], format: Format, text: &str) {
        assert_eq!(
            read(file, format),
            Ok((format.crlf, text.as_bytes().to_vec()))
        );
        assert_eq!(written(format, &[text]).unwrap(), file);
        let (head, tail) = text.split_at(text.char_indices().nth(2).map_or(0, |(at, _)| at));
        assert_eq!(
            written(format, &[head, tail]).unwrap(),
            file,
            "split at {}",
            head.len()
        );
        assert_eq!(&*format.decode(file), text.as_bytes());
    }

    #[test]
    fn cyrillic_in_windows_1251_is_detected_and_saved_back() {
        let text = "Съешь же ещё этих мягких французских булок, да выпей чаю.\n".repeat(4);
        let (file, _, _) = WINDOWS_1251.encode(&text);
        assert_eq!(detect(&file, true), WINDOWS_1251);
        assert_round_trip(&file, format(WINDOWS_1251, false), &text);
        let error = written(format(WINDOWS_1251, false), &["日本"]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn latin1_is_read_as_windows_1252_byte_for_byte() {
        let text = "Le cœur a ses raisons que la raison ne connaît point, déjà.\n".repeat(4);
        let (file, _, _) = WINDOWS_1252.encode(&text);
        assert_eq!(detect(&file, true), WINDOWS_1252);
        assert_round_trip(&file, format(WINDOWS_1252, false), &text);
        // Every byte is a character, so any bytes read come back as they were.
        let every_byte: Vec<u8> = (1..=255).collect();
        let (_, text) = read(&every_byte, format(WINDOWS_1252, false)).unwrap();
        let text = String::from_utf8(text).unwrap();
        assert_eq!(
            written(format(WINDOWS_1252, false), &[&text]).unwrap(),
            every_byte
        );
    }

    #[test]
    fn utf16_with_a_byte_order_mark_is_decoded_and_encoded_back() {
        let text = "a€😀\n";
        let utf16 = |bom: [u8; 2], text: &str, unit_bytes: fn(u16) -> [u8; 2]| -> Vec<u8> {
            bom.into_iter()
                .chain(text.encode_utf16().flat_map(unit_bytes))
                .collect()
        };
        let le = utf16([0xFF, 0xFE], text, u16::to_le_bytes);
        let be = utf16([0xFE, 0xFF], text, u16::to_be_bytes);
        assert_eq!(utf16_byte_order_mark(&le), Some(UTF_16LE));
        assert_round_trip(&le, format(UTF_16LE, false), text);
        assert_round_trip(&be, format(UTF_16BE, false), text);
        let crlf = utf16([0xFF, 0xFE], "x\r\ny\r\n", u16::to_le_bytes);
        assert_round_trip(&crlf, format(UTF_16LE, true), "x\ny\n");
        // An unpaired surrogate.
        assert_eq!(
            read(&[0xFF, 0xFE, 0x00, 0xD8, b'a', 0], format(UTF_16LE, false)),
            Err(Stop::Malformed)
        );
    }

    #[test]
    fn crlf_is_dropped_on_reading_and_written_back() {
        assert_round_trip(b"a\r\nb\r\n", format(UTF_8, true), "a\nb\n");
        assert_eq!(
            read(b"a\r\nb\nc\r\n", format(UTF_8, true)),
            Err(Stop::LoneLf)
        );
        assert_eq!(
            read(b"no line end\r", format(UTF_8, true)),
            Ok((false, b"no line end\r".to_vec()))
        );
        assert!(first_line_ends_in_crlf(b"a\r\nb") && !first_line_ends_in_crlf(b"a\nb\r\n"));
    }

    #[test]
    fn a_cr_lf_split_between_chunks_is_still_one_line_end() {
        let mut file = vec![b'x'; CHUNK_LEN - 1];
        file.extend_from_slice(b"\r\nend\r");
        let (crlf, text) = read(&file, format(UTF_8, true)).unwrap();
        assert!(crlf);
        assert_eq!(&text[CHUNK_LEN - 2..], b"x\nend\r");
    }

    #[test]
    fn binary_files_are_told_by_a_nul_and_never_saved() {
        assert!(is_binary(b"\x7fELF\x02\x01\x00"));
        assert!(!is_binary("plain é".as_bytes()));
        let binary = Format {
            binary: true,
            ..Format::default()
        };
        assert_eq!(
            written(binary, &["x"]).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
