//! Hiding blocks of code, like Emacs `hs-minor-mode`: a fold hides a block's lines after
//! its first, until it is shown again, point goes into it or an edit touches it.

use remacs_core::{CommandResult, Editor};

pub const HS_TOGGLE_HIDING: &str = "hs-toggle-hiding";
pub const HS_HIDE_ALL: &str = "hs-hide-all";
pub const HS_SHOW_ALL: &str = "hs-show-all";

/// Shows the block whose first line point is on, else hides the block starting there, or
/// the innermost one point is in, putting point on its first line.
pub fn hs_toggle_hiding(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer_mut();
    let line = buffer.point_line();
    if buffer.unfold_at(line) {
        return Ok(());
    }
    let blocks = (editor.blocks(editor.window_buffer(), line..line + 1))
        .ok_or("Nothing here can be hidden")?;
    let block = (blocks.iter().find(|block| block.lines.start == line))
        .or_else(|| {
            (blocks.iter().filter(|block| block.lines.contains(&line)))
                .min_by_key(|block| block.lines.len())
        })
        .filter(|block| !block.hidden.is_empty())
        .ok_or("No block here to hide")?;
    let (header, hidden) = (block.lines.start, block.hidden.clone());
    let buffer = editor.window_buffer_mut();
    buffer.fold([hidden]);
    move_out_of_folds(editor, header);
    Ok(())
}

/// Hides every block, those within others too, so that showing one shows its blocks hidden.
pub fn hs_hide_all(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let line_count = buffer.text().line_count();
    let blocks = editor
        .blocks(buffer, 0..line_count)
        .ok_or("Nothing here can be hidden")?;
    let line = buffer.point_line();
    editor
        .window_buffer_mut()
        .fold(blocks.into_iter().map(|block| block.hidden));
    move_out_of_folds(editor, line);
    Ok(())
}

pub fn hs_show_all(editor: &mut Editor) -> CommandResult {
    editor.window_buffer_mut().unfold_all();
    Ok(())
}

/// Puts point on the line shown for `line`, its fold's first, unless point is shown.
fn move_out_of_folds(editor: &mut Editor, line: usize) {
    let buffer = editor.window_buffer_mut();
    let shown = buffer.shown_lines();
    if !shown.is_hidden(buffer.point_line()) {
        return;
    }
    let header = shown.line_at(shown.row_of(line));
    let start = buffer.text().line_start(header).unwrap_or(0);
    buffer.set_point(start);
}
