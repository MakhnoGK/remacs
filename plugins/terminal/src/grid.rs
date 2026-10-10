//! The emulator's grid as remacs draws it: the rows in view, runs of cells drawn alike.

use std::collections::{BTreeMap, HashSet};
use std::ops::RangeInclusive;

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape as Shape, NamedColor};
use remacs_core::{
    CellFace, CellHighlight, CellRun, ConsoleColor, CursorShape, ScreenCursor, ScreenRow,
};

use crate::history::Commands;
use crate::scan::prompt_id;
use crate::session::Listener;

/// What is drawn over the grid's own colors.
pub struct Overlay<'a> {
    /// The keys are elsewhere: the cursor is hollow.
    pub focused: bool,
    pub matches: &'a [RangeInclusive<Point>],
    pub current_match: Option<&'a RangeInclusive<Point>>,
    pub commands: &'a Commands,
}

pub fn rows(term: &Term<Listener>, overlay: &Overlay) -> (Vec<ScreenRow>, Option<ScreenCursor>) {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    let selection = term
        .selection
        .as_ref()
        .and_then(|selection| selection.to_range(term));
    let colors = term.colors();
    let highlight = |point: Point| -> Option<CellHighlight> {
        if overlay
            .current_match
            .is_some_and(|current| current.contains(&point))
        {
            Some(CellHighlight::SearchCurrent)
        } else if selection.is_some_and(|selection| selection.contains(point)) {
            Some(CellHighlight::Selection)
        } else if overlay.matches.iter().any(|found| found.contains(&point)) {
            Some(CellHighlight::SearchMatch)
        } else {
            None
        }
    };
    // A prompt of several lines is marked on its first only.
    let mut marked = HashSet::new();
    let rows = (0..grid.screen_lines() as i32)
        .map(|row| {
            let line = Line(row - offset);
            let mut runs: Vec<CellRun> = Vec::new();
            // A wide character is a run of its own, drawn over its two cells.
            let mut joinable = false;
            let mut prompt = None;
            for column in 0..grid.columns() {
                let point = Point::new(line, Column(column));
                let cell = &grid[point];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                if prompt.is_none() && cell.extra.is_some() {
                    prompt = cell.hyperlink().and_then(|link| prompt_id(link.uri()));
                }
                let face = CellFace {
                    highlight: highlight(point),
                    ..face(cell, colors)
                };
                let cells = if cell.flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                let character = if cell.flags.contains(Flags::HIDDEN) {
                    ' '
                } else {
                    cell.c
                };
                match runs.last_mut() {
                    Some(run) if joinable && cells == 1 && run.face == face => {
                        run.text.push(character);
                        run.cells += 1;
                    }
                    _ => runs.push(CellRun {
                        text: character.to_string(),
                        cells,
                        face,
                    }),
                }
                joinable = cells == 1;
                if let Some(zero_width) = cell.zerowidth() {
                    runs.last_mut()
                        .expect("a run was just added to")
                        .text
                        .extend(zero_width);
                }
            }
            let prompt = prompt.filter(|&id| marked.insert(id));
            ScreenRow {
                runs,
                mark: prompt.and_then(|id| overlay.commands.mark(id)),
                prompt: prompt.is_some(),
            }
        })
        .collect();
    (rows, cursor(term, overlay.focused))
}

fn cursor(term: &Term<Listener>, focused: bool) -> Option<ScreenCursor> {
    let content = term.renderable_content();
    let shape = match content.cursor.shape {
        Shape::Hidden => return None,
        _ if !focused => CursorShape::Hollow,
        Shape::Block => CursorShape::Block,
        Shape::Underline => CursorShape::Underline,
        Shape::Beam => CursorShape::Bar,
        Shape::HollowBlock => CursorShape::Hollow,
    };
    let row = content.cursor.point.line.0 + content.display_offset as i32;
    let rows = term.grid().screen_lines() as i32;
    (0..rows).contains(&row).then_some(ScreenCursor {
        row: row as usize,
        column: content.cursor.point.column.0,
        shape,
    })
}

fn face(cell: &Cell, colors: &Colors) -> CellFace {
    let flags = cell.flags;
    let (mut foreground, foreground_dim) = color(cell.fg, colors);
    let (mut background, _) = color(cell.bg, colors);
    // Black behind text is the glass, as programs mean it.
    if background == Some(ConsoleColor::Indexed(0)) {
        background = None;
    }
    if flags.contains(Flags::INVERSE) {
        let text = remacs_theme::color::FG_BODY;
        (foreground, background) = (
            Some(background.unwrap_or(ConsoleColor::Rgb(0x000000))),
            Some(foreground.unwrap_or(ConsoleColor::Rgb(text))),
        );
    }
    CellFace {
        foreground,
        background,
        bold: flags.contains(Flags::BOLD),
        dim: flags.contains(Flags::DIM) || foreground_dim,
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        strikeout: flags.contains(Flags::STRIKEOUT),
        highlight: None,
    }
}

/// The color and whether it is a dim one; none for the theme's own text or glass.
fn color(color: Color, colors: &Colors) -> (Option<ConsoleColor>, bool) {
    let rgb = |index: usize| {
        colors[index].map(|rgb| {
            ConsoleColor::Rgb(u32::from(rgb.r) << 16 | u32::from(rgb.g) << 8 | u32::from(rgb.b))
        })
    };
    match color {
        Color::Spec(spec) => (
            Some(ConsoleColor::Rgb(
                u32::from(spec.r) << 16 | u32::from(spec.g) << 8 | u32::from(spec.b),
            )),
            false,
        ),
        Color::Indexed(index) => (
            rgb(usize::from(index)).or(Some(ConsoleColor::Indexed(index))),
            false,
        ),
        Color::Named(named) => {
            let index = named as usize;
            match named {
                NamedColor::Foreground
                | NamedColor::Background
                | NamedColor::Cursor
                | NamedColor::BrightForeground => (rgb(index), false),
                NamedColor::DimForeground => (rgb(index), true),
                _ if index < 16 => (
                    rgb(index).or(Some(ConsoleColor::Indexed(index as u8))),
                    false,
                ),
                // The dim ones follow the eight normal ones in the same order.
                _ => {
                    let base = index - NamedColor::DimBlack as usize;
                    (rgb(base).or(Some(ConsoleColor::Indexed(base as u8))), true)
                }
            }
        }
    }
}

/// The prompts in the grid, top to bottom, by the line they start on.
pub fn prompt_lines(term: &Term<Listener>) -> BTreeMap<i32, u64> {
    let grid = term.grid();
    let top = -(grid.history_size() as i32);
    let bottom = grid.screen_lines() as i32;
    let mut prompts = BTreeMap::new();
    for line in top..bottom {
        let row = &grid[Line(line)];
        let id = (0..grid.columns())
            .map(|column| &row[Column(column)])
            .filter(|cell| cell.extra.is_some())
            .find_map(|cell| cell.hyperlink().and_then(|link| prompt_id(link.uri())));
        if let Some(id) = id
            && !prompts.values().any(|&known| known == id)
        {
            prompts.insert(line, id);
        }
    }
    prompts
}
