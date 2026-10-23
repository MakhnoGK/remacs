//! Showing files in the window while a prompt is open, like Emacs consult's preview:
//! quitting puts back what the window showed, and closes the files opened only to be
//! shown.

use std::io;
use std::path::{Path, PathBuf};

use remacs_core::Editor;

/// What the window showed before, and the files opened since.
#[derive(Default)]
pub struct FilePreview {
    origin: Option<Origin>,
    /// Files that had no buffer before being shown.
    opened: Vec<PathBuf>,
}

struct Origin {
    buffer: String,
    point: usize,
    first_line: usize,
}

impl FilePreview {
    pub fn begin(editor: &Editor) -> Self {
        let buffer = editor.window_buffer();
        Self {
            origin: Some(Origin {
                buffer: buffer.name().to_owned(),
                point: buffer.point(),
                first_line: editor.viewport.first_line,
            }),
            opened: Vec::new(),
        }
    }

    /// Shows the file at `path` in the window.
    pub fn visit(&mut self, editor: &mut Editor, path: &Path) -> io::Result<()> {
        if !editor
            .buffers()
            .iter()
            .any(|buffer| buffer.path() == Some(path))
        {
            self.opened.push(path.to_path_buf());
        }
        editor.visit_file(path)
    }

    /// Shows the file at `path`, point at `offset`, centered when it was off screen.
    pub fn show_at(&mut self, editor: &mut Editor, path: &Path, offset: usize) -> io::Result<()> {
        self.visit(editor, path)?;
        let buffer = editor.window_buffer_mut();
        buffer.deactivate_mark();
        buffer.set_point(offset);
        let line = buffer.point_line();
        editor.show_line(line);
        Ok(())
    }

    /// Closes the file at `path` if it was opened to be shown.
    pub fn close(&mut self, editor: &mut Editor, path: &Path) {
        if let Some(index) = self.opened.iter().position(|opened| opened == path) {
            self.opened.swap_remove(index);
            kill_file_buffer(editor, path);
        }
    }

    /// Shows what the window showed before.
    pub fn restore(&self, editor: &mut Editor) {
        let Some(origin) = &self.origin else {
            return;
        };
        if editor.switch_to_buffer(&origin.buffer) {
            editor.window_buffer_mut().set_point(origin.point);
            editor.viewport.first_line = origin.first_line;
        }
    }

    /// Puts back what the window showed unless a file was `chosen`, and closes the files
    /// opened only to be shown.
    pub fn end(self, editor: &mut Editor, chosen: Option<&Path>) {
        if chosen.is_none() {
            self.restore(editor);
        }
        for path in self.opened {
            if Some(path.as_path()) != chosen {
                kill_file_buffer(editor, &path);
            }
        }
    }
}

fn kill_file_buffer(editor: &mut Editor, path: &Path) {
    let name = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.path() == Some(path))
        .map(|buffer| buffer.name().to_owned());
    if let Some(name) = name {
        editor.kill_buffer(&name);
    }
}
