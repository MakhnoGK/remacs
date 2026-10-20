//! Splitting the frame into windows and moving between them (ADR 0073).

use remacs_core::{Axis, CommandResult, Editor, Section};
use serde::Deserialize;

/// What a window shows besides its text, in lines (its title) and columns (its line
/// numbers), about; `min-lines` and `min-columns` are of text.
const CHROME_LINES: usize = 3;
const CHROME_COLUMNS: usize = 8;

/// `windows.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct WindowsSettings {
    /// Lines of text each half of a window split one above the other keeps at least.
    pub min_lines: usize,
    /// Columns each half of a window split side by side keeps at least.
    pub min_columns: usize,
    /// Pixels of the strip naming the buffer at the top of a window under another.
    pub header_height: f32,
    /// Lines or columns `C-x ^`, `C-x }` and `C-x {` move a window's edge by.
    pub resize_step: usize,
    /// Pixels of the strip around the 1px line between windows where the mouse drags it.
    pub divider_grab_width: f32,
    /// Opacity of what windows but the selected one show, which dims them a little.
    pub inactive_opacity: f32,
}

impl Default for WindowsSettings {
    fn default() -> Self {
        Self {
            min_lines: 4,
            min_columns: 20,
            header_height: 28.0,
            resize_step: 2,
            divider_grab_width: 6.0,
            inactive_opacity: 0.85,
        }
    }
}

impl Section for WindowsSettings {
    const NAME: &str = "windows";

    fn check(&self) -> Result<(), String> {
        if self.min_lines == 0 || self.min_columns == 0 || self.resize_step == 0 {
            return Err("min-lines, min-columns and resize-step must be at least 1".into());
        }
        if !(16.0..=64.0).contains(&self.header_height) {
            return Err("header-height must be 16 to 64".into());
        }
        if !(1.0..=16.0).contains(&self.divider_grab_width) {
            return Err("divider-grab-width must be 1 to 16".into());
        }
        if !(0.5..=1.0).contains(&self.inactive_opacity) {
            return Err("inactive-opacity must be 0.5 to 1.0".into());
        }
        Ok(())
    }
}

impl WindowsSettings {
    /// The least text a window keeps along `axis`, and what else it shows along it.
    fn min_text_and_chrome(&self, axis: Axis) -> (usize, usize) {
        match axis {
            Axis::Rows => (self.min_lines, CHROME_LINES),
            Axis::Columns => (self.min_columns, CHROME_COLUMNS),
        }
    }
}

pub fn split_window_below(editor: &mut Editor) -> CommandResult {
    split(editor, Axis::Rows)
}

pub fn split_window_right(editor: &mut Editor) -> CommandResult {
    split(editor, Axis::Columns)
}

/// The window commands act on the selected window, even from the pane, which gives the
/// keys back.
fn split(editor: &mut Editor, axis: Axis) -> CommandResult {
    editor.focus_pane(false);
    let (min, chrome) = editor
        .plugin_state::<WindowsSettings>()
        .min_text_and_chrome(axis);
    let size = match axis {
        Axis::Rows => editor.viewport.height,
        Axis::Columns => editor.viewport.width,
    };
    // 0 until the window is first laid out. The new window's title or line numbers take
    // from the text the two share.
    if size != 0 && size < 2 * min + chrome {
        return Err("Window too small for splitting".into());
    }
    editor.split_window(axis);
    Ok(())
}

pub fn delete_window(editor: &mut Editor) -> CommandResult {
    editor.focus_pane(false);
    editor.delete_window()
}

pub fn delete_other_windows(editor: &mut Editor) -> CommandResult {
    editor.focus_pane(false);
    editor.delete_other_windows();
    Ok(())
}

/// The windows in order on screen, then the pane if one is shown, then the first window.
pub fn other_window(editor: &mut Editor) -> CommandResult {
    let windows = editor.layout().windows();
    if editor.pane_focused() {
        editor.focus_pane(false);
        editor.select_window(windows[0]);
        return Ok(());
    }
    let selected = editor.selected_window();
    let next = windows
        .iter()
        .position(|&window| window == selected)
        .and_then(|index| windows.get(index + 1));
    match next {
        Some(&window) => editor.select_window(window),
        None if editor.shown_pane().is_some() => editor.focus_pane(true),
        None if windows.len() > 1 => editor.select_window(windows[0]),
        None => return Err("No other window, nor a pane".into()),
    }
    Ok(())
}

/// Selects the window selected before, splitting the selected one first if it is alone,
/// where Emacs `*-other-window` commands show buffers.
pub(crate) fn pop_to_other_window(editor: &mut Editor) -> CommandResult {
    editor.focus_pane(false);
    if editor.window_count() == 1 {
        let viewport = editor.viewport;
        // Along the longer side on screen, a cell being about twice as tall as wide.
        let (longer, shorter) = if viewport.width >= 2 * viewport.height {
            (Axis::Columns, Axis::Rows)
        } else {
            (Axis::Rows, Axis::Columns)
        };
        split(editor, longer).or_else(|_| split(editor, shorter))?;
    }
    if let Some(window) = editor.previous_window() {
        editor.select_window(window);
    }
    Ok(())
}

pub fn enlarge_window(editor: &mut Editor) -> CommandResult {
    resize(editor, Axis::Rows, 1)
}

pub fn enlarge_window_horizontally(editor: &mut Editor) -> CommandResult {
    resize(editor, Axis::Columns, 1)
}

pub fn shrink_window_horizontally(editor: &mut Editor) -> CommandResult {
    resize(editor, Axis::Columns, -1)
}

fn resize(editor: &mut Editor, axis: Axis, direction: isize) -> CommandResult {
    editor.focus_pane(false);
    let settings = editor.plugin_state::<WindowsSettings>().clone();
    let (min, chrome) = settings.min_text_and_chrome(axis);
    let cells = direction * settings.resize_step as isize;
    if editor.resize_window(axis, cells, min + chrome) {
        Ok(())
    } else if direction > 0 {
        Err("Cannot enlarge the window".into())
    } else {
        Err("Cannot shrink the window".into())
    }
}

pub fn balance_windows(editor: &mut Editor) -> CommandResult {
    editor.focus_pane(false);
    editor.balance_windows();
    Ok(())
}

pub fn has_other_windows(editor: &Editor) -> bool {
    editor.window_count() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_window_goes_through_the_windows_in_order_on_screen() {
        let mut editor = Editor::new(&[&crate::FilesPlugin]);
        editor.split_window(Axis::Rows);
        editor.split_window(Axis::Columns);
        let order = editor.layout().windows();
        let mut selected = Vec::new();
        for _ in 0..3 {
            editor.run_command("other-window");
            selected.push(editor.selected_window());
        }
        assert_eq!(selected, [order[1], order[2], order[0]]);
    }

    #[test]
    fn a_buffer_switched_to_in_other_window_splits_a_lone_window_and_selects_it() {
        let mut editor = Editor::new(&[
            &remacs_editing::EditingPlugin,
            &remacs_minibuffer::MinibufferPlugin,
            &crate::FilesPlugin,
        ]);
        let first = editor.selected_window();
        let shown = editor.window_buffer().name().to_owned();
        for key in remacs_core::parse_sequence("C-x 4 b n e w RET").unwrap() {
            editor.handle_key(key);
        }
        let [_, second] = editor.layout().windows()[..] else {
            panic!("two windows");
        };
        assert_eq!(editor.selected_window(), second);
        assert_eq!(editor.window_buffer().name(), "new");
        editor.select_window(first);
        assert_eq!(editor.window_buffer().name(), shown);
    }

    #[test]
    fn a_split_leaves_both_windows_min_lines_of_text_besides_the_new_title() {
        let mut editor = Editor::new(&[&crate::FilesPlugin]);
        editor.viewport.height = 2 * 4 + CHROME_LINES - 1;
        editor.run_command("split-window-below");
        assert_eq!(editor.window_count(), 1, "too small");
        editor.viewport.height += 1;
        editor.run_command("split-window-below");
        assert_eq!(editor.window_count(), 2);
    }

    #[test]
    fn killing_a_buffer_another_window_shows_takes_it_off_this_window_only() {
        let mut editor = Editor::new(&[
            &remacs_editing::EditingPlugin,
            &remacs_minibuffer::MinibufferPlugin,
            &crate::FilesPlugin,
        ]);
        let shared = editor.window_buffer().name().to_owned();
        let mode = editor.mode_for(None);
        let blank = remacs_text::Text::default();
        editor.add_buffer_noselect(remacs_core::Buffer::new("other", blank, mode));
        editor.split_window(Axis::Rows);
        let below = editor.layout().windows()[1];
        for key in remacs_core::parse_sequence("C-x k RET").unwrap() {
            editor.handle_key(key);
        }
        assert_eq!(editor.window_buffer().name(), "other");
        assert_eq!(editor.buffers().len(), 2, "not killed");
        editor.select_window(below);
        assert_eq!(editor.window_buffer().name(), shared);
    }

    #[test]
    fn the_default_settings_file_reads_as_the_defaults() {
        remacs_core::assert_default_file::<super::WindowsSettings>();
    }
}
