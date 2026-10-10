//! Files and folders to open: from the command line, or from Finder and the Dock icon.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use remacs_core::Editor;
use remacs_dired::list_directory;

/// Visits a file, or lists a directory.
pub fn open(editor: &mut Editor, path: &Path) -> Result<(), String> {
    if path.is_dir() {
        list_directory(editor, path).map_err(|error| error.0)
    } else {
        editor.visit_file(path).map_err(|error| error.to_string())
    }
}

/// The paths of the `file://` URLs among `urls`.
pub fn file_paths(urls: &[String]) -> Vec<PathBuf> {
    urls.iter()
        .filter_map(|url| url.strip_prefix("file://"))
        // A host, if any, is the local one: `file://localhost/…`.
        .filter_map(|rest| rest.find('/').map(|slash| &rest[slash..]))
        .map(|path| PathBuf::from(OsString::from_vec(percent_decoded(path))))
        .collect()
}

fn percent_decoded(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_become_paths() {
        let urls = [
            "file:///Users/me/My%20Notes/%D1%82%D0%B5%D0%BA%D1%81%D1%82.md".to_owned(),
            "file://localhost/tmp/a%2Fb/".to_owned(),
            "https://example.com/x".to_owned(),
            "file:///100%".to_owned(),
        ];
        assert_eq!(
            file_paths(&urls),
            [
                PathBuf::from("/Users/me/My Notes/текст.md"),
                PathBuf::from("/tmp/a/b/"),
                PathBuf::from("/100%"),
            ]
        );
    }
}
