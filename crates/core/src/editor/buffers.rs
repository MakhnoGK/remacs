use std::io;
use std::mem;
use std::path::{Path, PathBuf};

use remacs_text::{Text, UTF_8};

use super::{Editor, SCRATCH_BUFFER};
use crate::buffer::{Buffer, BufferId, FileEdits};

impl Editor {
    /// The buffer commands act on: the minibuffer input, or a message's description,
    /// while a prompt is open.
    pub fn buffer(&self) -> &Buffer {
        match &self.minibuffer {
            Some(minibuffer) => minibuffer.editing(),
            None => &self.buffers[0],
        }
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        match &mut self.minibuffer {
            Some(minibuffer) => minibuffer.editing_mut(),
            None => &mut self.buffers[0],
        }
    }

    pub fn window_buffer(&self) -> &Buffer {
        &self.buffers[0]
    }

    pub fn window_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[0]
    }

    /// All buffers, most recently selected first.
    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }

    pub fn buffers_mut(&mut self) -> &mut [Buffer] {
        &mut self.buffers
    }

    pub fn buffer_named_mut(&mut self, name: &str) -> Option<&mut Buffer> {
        self.buffers.iter_mut().find(|buffer| buffer.name() == name)
    }

    pub(super) fn buffer_named(&self, name: &str) -> Option<&Buffer> {
        self.buffers.iter().find(|buffer| buffer.name() == name)
    }

    pub(super) fn buffer_with_id_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.iter_mut().find(|buffer| buffer.id() == id)
    }

    /// Shows the buffer visiting `path`, opening it first if needed; a missing file
    /// becomes an empty buffer that creates it on save.
    pub fn visit_file(&mut self, path: &Path) -> io::Result<()> {
        let index = self.find_file(path)?;
        self.select_buffer(index);
        Ok(())
    }

    /// The buffer visiting `path`, opened without showing it if needed, like Emacs
    /// `find-file-noselect`.
    pub fn find_file_noselect(&mut self, path: &Path) -> io::Result<&mut Buffer> {
        let index = self.find_file(path)?;
        Ok(&mut self.buffers[index])
    }

    /// Makes `edits` in the buffer visiting their file, opened without showing it if
    /// needed; they undo together and are left unsaved.
    pub fn edit_file(&mut self, edits: FileEdits) -> io::Result<&mut Buffer> {
        let buffer = self.find_file_noselect(&edits.path)?;
        buffer.add_undo_boundary();
        buffer.apply_edits(edits.edits);
        Ok(buffer)
    }

    /// Makes the window buffer visit `path` from now on, renamed after it, like Emacs
    /// `set-visited-file-name`; saving is up to the caller.
    pub fn set_visited_file(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize().or_else(|_| std::path::absolute(path))?;
        if let Some(other) = self.buffers[1..].iter().find(|buffer| buffer.path() == Some(&path)) {
            let message = format!("Buffer {} is already visiting {}", other.name(), path.display());
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, message));
        }
        self.set_buffer_file(0, path);
        Ok(())
    }

    /// Makes the buffer at `index` visit `path`, renamed after it.
    fn set_buffer_file(&mut self, index: usize, path: PathBuf) {
        let name = file_buffer_name(&path);
        if name != self.buffers[index].name() {
            let name = self.unique_name(&name);
            self.buffers[index].set_name(name);
        }
        let mode = self.mode_for_file(&path, self.buffers[index].text());
        self.buffers[index].set_file(path, mode);
    }

    /// Files moved since the last call, from and to, for backends such as language
    /// servers to be told.
    pub fn take_moved_files(&mut self) -> Vec<(PathBuf, PathBuf)> {
        mem::take(&mut self.moved_files)
    }

    /// Makes buffers visiting `from`, or files under it, visit them at `to`, renamed
    /// after them, as Emacs does after `dired-rename-file`.
    pub fn file_moved(&mut self, from: &Path, to: &Path) {
        self.moved_files.push((from.to_path_buf(), to.to_path_buf()));
        for index in 0..self.buffers.len() {
            let Some(rest) =
                self.buffers[index].path().and_then(|path| path.strip_prefix(from).ok())
            else {
                continue;
            };
            let path = if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) };
            self.set_buffer_file(index, path);
        }
    }

    /// Adds `buffer` under a unique name (`name<2>`) and shows it.
    pub fn add_buffer(&mut self, buffer: Buffer) {
        let index = self.push_buffer(buffer);
        self.select_buffer(index);
    }

    /// Adds `buffer` under a unique name without showing it, as the least recent one;
    /// returns the name.
    pub fn add_buffer_noselect(&mut self, buffer: Buffer) -> String {
        let index = self.push_buffer(buffer);
        self.buffers[index].name().to_owned()
    }

    pub fn switch_to_buffer(&mut self, name: &str) -> bool {
        self.buffer_index(name).inspect(|&index| self.select_buffer(index)).is_some()
    }

    /// Removes a buffer; the window falls back to the next most recent one, or `*scratch*`.
    pub fn kill_buffer(&mut self, name: &str) -> bool {
        let Some(index) = self.buffer_index(name) else {
            return false;
        };
        let killed = self.buffers.remove(index).id();
        if self.buffers.is_empty() {
            let mode = self.mode_for(None);
            self.buffers.push(Buffer::new(SCRATCH_BUFFER, Text::default(), mode));
        }
        if index == 0 {
            self.viewport.first_line = self.buffers[0].window_start;
        }
        self.replace_in_windows(killed);
        true
    }

    /// Renames the buffer named `name`, as `name<2>` if the new name is taken.
    pub fn rename_buffer(&mut self, name: &str, new_name: &str) -> Option<String> {
        let index = self.buffer_index(name)?;
        if name == new_name {
            return Some(new_name.to_owned());
        }
        let unique = self.unique_name(new_name);
        self.buffers[index].set_name(unique.clone());
        Some(unique)
    }

    /// Index of the buffer visiting `path`, opened as the least recent one if needed.
    pub(super) fn find_file(&mut self, path: &Path) -> io::Result<usize> {
        let path = path.canonicalize().or_else(|_| std::path::absolute(path))?;
        if let Some(index) = self.buffers.iter().position(|buffer| buffer.path() == Some(&path)) {
            self.sync_with_disk(index);
            return Ok(index);
        }
        let text = match Text::open(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.set_message("(New file)");
                Text::default()
            }
            Err(error) => return Err(error),
        };
        let format = text.format();
        let mode = self.mode_for_file(&path, &text);
        let mut buffer = Buffer::new(file_buffer_name(&path), text, mode).with_file(path);
        if format.binary {
            buffer.set_read_only(true);
            self.set_message("Binary file: not shown");
        } else if format.encoding != UTF_8 {
            self.set_message(format!(
                "Read as {}, and saved so; if that's wrong, M-x revert-buffer-with-coding-system",
                format.encoding.name()
            ));
        }
        Ok(self.push_buffer(buffer))
    }

    /// Brings the buffers visiting the files `which` picks up to date with them, after
    /// those changed on disk.
    pub fn sync_buffers_with_disk(&mut self, which: impl Fn(&Path) -> bool) {
        for index in 0..self.buffers.len() {
            if self.buffers[index].path().is_some_and(&which) {
                self.sync_with_disk(index);
            }
        }
    }

    /// If the buffer's file changed on disk since it was read or saved, reads an unmodified
    /// buffer again; a modified one keeps its edits, which saving asks before writing over
    /// the file. A file gone is reported, not read; each is reported once.
    fn sync_with_disk(&mut self, index: usize) {
        /// Marks a buffer already reported to be behind its file.
        struct Reported;

        let buffer = &mut self.buffers[index];
        if !buffer.text().changed_on_disk().unwrap_or(false) {
            buffer.remove_local::<Reported>();
            return;
        }
        let message = if !buffer.path().is_some_and(Path::exists) {
            "its file was deleted".to_owned()
        } else if buffer.is_modified() {
            "its file changed on disk; saving asks before writing over it".to_owned()
        } else {
            match buffer.revert() {
                Ok(()) => {
                    buffer.remove_local::<Reported>();
                    return;
                }
                Err(error) => format!("reading its file again: {error}"),
            }
        };
        if buffer.local::<Reported>().is_some() {
            return;
        }
        buffer.set_local(Reported);
        let message = format!("{}: {message}", buffer.name());
        self.set_error(message);
    }

    /// Adds `buffer` as the least recent one under a unique name and returns its index.
    fn push_buffer(&mut self, mut buffer: Buffer) -> usize {
        buffer.set_name(self.unique_name(buffer.name()));
        self.buffers.push(buffer);
        self.buffers.len() - 1
    }

    fn buffer_index(&self, name: &str) -> Option<usize> {
        self.buffers.iter().position(|buffer| buffer.name() == name)
    }

    pub(super) fn select_buffer(&mut self, index: usize) {
        self.buffers[0].window_start = self.viewport.first_line;
        let buffer = self.buffers.remove(index);
        self.viewport.first_line = buffer.window_start;
        // Left as it was, a short buffer would stay scrolled sideways.
        self.viewport.first_column = 0;
        self.buffers.insert(0, buffer);
    }

    fn unique_name(&self, name: &str) -> String {
        let taken = |candidate: &str| self.buffer_index(candidate).is_some();
        if !taken(name) {
            return name.to_owned();
        }
        (2..)
            .map(|number| format!("{name}<{number}>"))
            .find(|candidate| !taken(candidate))
            .expect("unbounded")
    }
}

fn file_buffer_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::tests::{buffer_names, buffer_text, temp_dir};

    #[test]
    fn buffers_are_most_recent_first_with_unique_names() {
        let mut editor = Editor::new(&[]);
        editor.add_buffer(Buffer::new("a", Text::default(), "Fundamental"));
        editor.add_buffer(Buffer::new("a", Text::default(), "Fundamental"));
        assert_eq!(buffer_names(&editor), ["a<2>", "a", SCRATCH_BUFFER]);
        assert!(editor.switch_to_buffer(SCRATCH_BUFFER));
        assert_eq!(buffer_names(&editor), [SCRATCH_BUFFER, "a<2>", "a"]);
        assert!(editor.kill_buffer(SCRATCH_BUFFER));
        assert!(editor.kill_buffer("a<2>") && editor.kill_buffer("a"));
        assert_eq!(buffer_names(&editor), [SCRATCH_BUFFER]);
    }

    #[test]
    fn a_binary_file_opens_read_only_and_one_not_utf8_says_how_it_was_read() {
        let mut editor = Editor::new(&[]);
        let directory = temp_dir("encodings");
        let mut visit = |name: &str, contents: &[u8]| {
            let path = directory.join(name);
            std::fs::write(&path, contents).unwrap();
            editor.visit_file(&path).unwrap();
            let message = editor.message().map(|message| message.text.clone());
            (editor.buffer().is_read_only(), message)
        };
        let (read_only, message) = visit("tool", b"\x7fELF\x00\x00\xff");
        assert!(read_only);
        assert_eq!(message.as_deref(), Some("Binary file: not shown"));
        let (read_only, message) = visit("latin1.txt", b"caf\xe9");
        assert!(!read_only);
        assert!(message.unwrap().starts_with("Read as windows-1252"));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn only_regular_files_open_and_others_are_refused_without_blocking() {
        let directory = temp_dir("file-types");
        let fifo = directory.join("pipe");
        let fifo_name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        let file = directory.join("file.txt");
        std::fs::write(&file, "text").unwrap();
        let link = directory.join("link.txt");
        std::os::unix::fs::symlink(&file, &link).unwrap();

        let mut editor = Editor::new(&[]);
        for refused in [fifo.as_path(), "/dev/zero".as_ref(), "/dev/null".as_ref()] {
            let error = editor.visit_file(refused).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{refused:?}");
            assert!(error.to_string().ends_with("is not a regular file"));
        }
        assert_eq!(editor.buffers().len(), 1);
        editor.visit_file(&link).unwrap();
        assert_eq!(editor.window_buffer().contents(0..4), "text");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn buffers_follow_their_files_when_moved() {
        let mut editor = Editor::new(&[]);
        let file = |path: &str| {
            Buffer::new("x", Text::default(), "Fundamental").with_file(PathBuf::from(path))
        };
        editor.add_buffer(file("/a/one.rs"));
        editor.add_buffer(file("/a/b/two.rs"));
        editor.add_buffer(file("/ab/three.rs"));
        editor.file_moved(Path::new("/a"), Path::new("/c"));
        editor.file_moved(Path::new("/c/one.rs"), Path::new("/c/renamed.rs"));
        let visited: Vec<_> = editor
            .buffers()
            .iter()
            .filter_map(|buffer| Some((buffer.name(), buffer.path()?.to_str()?)))
            .collect();
        assert_eq!(
            visited,
            [("x<3>", "/ab/three.rs"), ("two.rs", "/c/b/two.rs"), ("renamed.rs", "/c/renamed.rs")]
        );
    }

    #[test]
    fn buffers_remember_their_scroll_position() {
        let mut editor = Editor::new(&[]);
        editor.viewport.first_line = 7;
        editor.add_buffer(Buffer::new("other", Text::default(), "Fundamental"));
        assert_eq!(editor.viewport.first_line, 0);
        editor.switch_to_buffer(SCRATCH_BUFFER);
        assert_eq!(editor.viewport.first_line, 7);
    }

    #[test]
    fn a_buffer_visits_a_file_under_its_name_unless_another_buffer_visits_it() {
        let mut editor = Editor::new(&[]);
        let file = |name: &str, path: &str| {
            Buffer::new(name, Text::default(), "Fundamental").with_file(PathBuf::from(path))
        };
        editor.add_buffer(file("b.rs", "/remacs-test/old/b.rs"));
        editor.add_buffer(file("a.rs", "/remacs-test/a.rs"));
        editor.add_buffer(Buffer::new("new", Text::default(), "Fundamental"));
        let mut visit = |path: &str| editor.set_visited_file(Path::new(path));
        let error = visit("/remacs-test/a.rs").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        visit("/remacs-test/b.rs").unwrap();
        let buffer = editor.window_buffer();
        assert_eq!(buffer.name(), "b.rs<2>");
        assert_eq!(buffer.path(), Some(Path::new("/remacs-test/b.rs")));

        let mut rename = |name, new_name| editor.rename_buffer(name, new_name);
        assert_eq!(rename("a.rs", "a.rs").as_deref(), Some("a.rs"));
        assert_eq!(rename("a.rs", "b.rs").as_deref(), Some("b.rs<3>"));
        assert_eq!(rename("missing", "c.rs"), None);
    }

    #[test]
    fn a_buffer_follows_its_file_on_disk_unless_it_has_edits_which_are_reported_once() {
        let directory = temp_dir("sync");
        let path = directory.join("file.txt");
        std::fs::write(&path, "one").unwrap();
        let mut editor = Editor::new(&[]);
        editor.visit_file(&path).unwrap();
        std::fs::write(&path, "two, longer").unwrap();
        editor.sync_buffers_with_disk(|_| true);
        assert_eq!(buffer_text(&editor), "two, longer");
        assert!(editor.message().is_none());

        editor.window_buffer_mut().insert("edit ");
        std::fs::write(&path, "three, longer still").unwrap();
        editor.sync_buffers_with_disk(|_| true);
        assert_eq!(buffer_text(&editor), "edit two, longer");
        let message = editor.message().map(|message| message.text.clone());
        let expected = "file.txt: its file changed on disk; saving asks before writing over it";
        assert_eq!(message.as_deref(), Some(expected));
        editor.clear_message();
        editor.sync_buffers_with_disk(|_| true);
        assert!(editor.message().is_none(), "reported once");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
