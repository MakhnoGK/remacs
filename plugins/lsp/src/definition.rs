use std::path::PathBuf;

use lsp_types::{GotoDefinitionResponse, Position};
use remacs_core::{Editor, embedded_file};
use remacs_xref::Location;

use crate::position::{Encoding, from_lsp, text_of};
use crate::uri;

/// Where each definition starts: a link's name rather than its whole item.
pub(crate) fn targets(response: Option<GotoDefinitionResponse>) -> Vec<(PathBuf, Position)> {
    let targets: Vec<_> = match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => {
            vec![(location.uri, location.range.start)]
        }
        Some(GotoDefinitionResponse::Array(locations)) => locations
            .into_iter()
            .map(|location| (location.uri, location.range.start))
            .collect(),
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| (link.target_uri, link.target_selection_range.start))
            .collect(),
    };
    targets
        .into_iter()
        .filter_map(|(uri, position)| Some((uri::to_path(&uri)?, position)))
        .collect()
}

/// Offsets in the text the server knows: the buffer visiting the file, else the file.
pub(crate) fn locations(
    editor: &Editor,
    targets: Vec<(PathBuf, Position)>,
    encoding: Encoding,
) -> Vec<Location> {
    targets
        .into_iter()
        .filter_map(|(path, position)| {
            let offset = from_lsp(&text_of(editor, &path)?, position, encoding);
            let path = embedded_file(&path).unwrap_or(path);
            Some(Location { path, offset })
        })
        .collect()
}
