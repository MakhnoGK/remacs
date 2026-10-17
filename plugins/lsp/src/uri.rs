use std::ffi::OsStr;
use std::fmt::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use lsp_types::Uri;

use crate::embedded;

/// The `file://` URI of an absolute path.
pub(crate) fn from_path(path: &Path) -> Uri {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(byte as char);
        } else {
            write!(uri, "%{byte:02X}").expect("writing to a String");
        }
    }
    uri.parse().expect("percent-encoded paths are valid URIs")
}

/// Servers may encode a path differently than we did, so URIs compare as paths.
pub(crate) fn to_path(uri: &Uri) -> Option<PathBuf> {
    let mut encoded = uri.as_str().strip_prefix("file://")?.as_bytes();
    let mut path = Vec::with_capacity(encoded.len());
    while let Some((&byte, rest)) = encoded.split_first() {
        let escaped = (byte == b'%')
            .then(|| rest.get(..2))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(decoded) => {
                path.push(decoded);
                encoded = &rest[2..];
            }
            None => {
                path.push(byte);
                encoded = rest;
            }
        }
    }
    Some(PathBuf::from(OsStr::from_bytes(&path)))
}

/// The file a URI names: a file of embedded code is found in, for its document's.
pub(crate) fn to_file(uri: &Uri) -> Option<PathBuf> {
    let path = to_path(uri)?;
    Some(embedded::file_of(&path).unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_survive_the_round_trip() {
        let path = Path::new("/tmp/a b/c#d%/файл.rs");
        let uri = from_path(path);
        assert_eq!(
            uri.as_str(),
            "file:///tmp/a%20b/c%23d%25/%D1%84%D0%B0%D0%B9%D0%BB.rs"
        );
        assert_eq!(to_path(&uri).as_deref(), Some(path));
        let lowercase: Uri = "file:///tmp/a%2fb".parse().unwrap();
        assert_eq!(to_path(&lowercase), Some(PathBuf::from("/tmp/a/b")));
    }
}
