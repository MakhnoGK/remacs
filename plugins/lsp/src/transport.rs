use std::io::{self, BufRead, Write};

use serde_json::Value;

/// Reads one base-protocol message; `None` once the server closed its output.
pub(crate) fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let length =
        length.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no Content-Length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

pub(crate) fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn messages_round_trip_through_the_framing() {
        let mut stream = Vec::new();
        write_message(&mut stream, &json!({"id": 1, "text": "ж"})).unwrap();
        write_message(&mut stream, &json!({"id": 2})).unwrap();
        let mut reader = stream.as_slice();
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({"id": 1, "text": "ж"})));
        assert_eq!(read_message(&mut reader).unwrap(), Some(json!({"id": 2})));
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }
}
