//! Links in a terminal's output: hyperlinks programs print (OSC 8), and the
//! `file:line:column`s and web addresses found in its text as in a run's console.

use std::ops::RangeInclusive;
use std::path::Path;

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Point};
use alacritty_terminal::term::cell::Flags;
use remacs_files::links::{Target, links_in, resolve};

use crate::scan::prompt_id;
use crate::session::Listener;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenLink {
    /// The cells it takes.
    pub cells: RangeInclusive<Point>,
    pub target: Target,
}

/// The link over `point`; a file's only if it is there, looked for from `directory`.
pub fn link_at(term: &Term<Listener>, point: Point, directory: &Path) -> Option<ScreenLink> {
    hyperlink_at(term, point).or_else(|| {
        let link = text_link_at(term, point)?;
        match &link.target {
            Target::File { path, .. } => {
                resolve(path, &[directory.to_path_buf()])?;
                Some(link)
            }
            Target::Url(_) => Some(link),
        }
    })
}

/// A program's own hyperlink, over every cell next to `point` that carries it.
fn hyperlink_at(term: &Term<Listener>, point: Point) -> Option<ScreenLink> {
    let grid = term.grid();
    let link = grid[point].hyperlink()?;
    if prompt_id(link.uri()).is_some() {
        return None;
    }
    let carries = |point: Point| grid[point].hyperlink().as_ref() == Some(&link);
    let (start, end) = (term.line_search_left(point), term.line_search_right(point));
    let mut first = point;
    while first > start && carries(first.sub(grid, Boundary::Grid, 1)) {
        first = first.sub(grid, Boundary::Grid, 1);
    }
    let mut last = point;
    while last < end && carries(last.add(grid, Boundary::Grid, 1)) {
        last = last.add(grid, Boundary::Grid, 1);
    }
    let target = match link.uri().strip_prefix("file://") {
        Some(rest) => Target::File {
            // After the host, if one is named.
            path: rest[rest.find('/')?..].to_owned(),
            line: 1,
            column: None,
        },
        None => Target::Url(link.uri().to_owned()),
    };
    Some(ScreenLink { cells: first..=last, target })
}

/// A `file:line` or a web address in the text of the line under `point`, wrapped rows
/// taken as one.
fn text_link_at(term: &Term<Listener>, point: Point) -> Option<ScreenLink> {
    let grid = term.grid();
    let (start, end) = (term.line_search_left(point), term.line_search_right(point));
    let mut text = String::new();
    // Where each character starts in `text`, and its cell.
    let mut cells: Vec<(usize, Point)> = Vec::new();
    let mut at = start;
    loop {
        let cell = &grid[at];
        if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            cells.push((text.len(), at));
            text.push(cell.c);
            if let Some(zero_width) = cell.zerowidth() {
                text.extend(zero_width);
            }
        }
        if at >= end {
            break;
        }
        at = if at.column + 1 < grid.columns() {
            Point::new(at.line, at.column + 1)
        } else {
            Point::new(at.line + 1, Column(0))
        };
    }
    let offset = cells.iter().rev().find(|(_, cell)| *cell <= point).map(|&(offset, _)| offset)?;
    let link = links_in(&text).into_iter().find(|link| link.range.contains(&offset))?;
    let cell_of = |offset: usize| {
        cells.iter().rev().find(|(start, _)| *start <= offset).map(|&(_, cell)| cell)
    };
    Some(ScreenLink {
        cells: cell_of(link.range.start)?..=cell_of(link.range.end - 1)?,
        target: link.target,
    })
}
