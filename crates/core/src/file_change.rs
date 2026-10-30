use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::buffer::FileEdits;
use crate::editor::Editor;

/// A change a refactoring makes to the project's files, such as renaming a symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileChange {
    Edit(FileEdits),
    /// An empty file, unless one is already there.
    Create(PathBuf),
    /// Fails rather than replace what is already at `to`.
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
}

/// What [`Editor::change_files`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ChangedFiles {
    /// Names of the buffers edited and saved, in the order first edited.
    pub edited: Vec<String>,
    /// Files now elsewhere, from and to.
    pub moved: Vec<(PathBuf, PathBuf)>,
}

impl Editor {
    /// Makes `changes` in order, buffers following renamed files, and saves every file
    /// edited, unsaved changes it had included, so that the files on disk agree with each
    /// other. On a failure, what was edited so far is saved too.
    pub fn change_files(&mut self, changes: Vec<FileChange>) -> Result<ChangedFiles, String> {
        let mut changed = ChangedFiles::default();
        let mut edited: Vec<PathBuf> = Vec::new();
        let result = (changes.into_iter())
            .try_for_each(|change| self.change_file(change, &mut edited, &mut changed.moved));
        for path in edited {
            let buffer = self.find_file_noselect(&path).map_err(|error| failed(&path, error))?;
            buffer.save().map_err(|error| failed(&path, error))?;
            changed.edited.push(buffer.name().to_owned());
        }
        if !changed.moved.is_empty() {
            self.record_moves(&changed.moved);
        }
        result.map(|()| changed)
    }

    fn change_file(
        &mut self,
        change: FileChange,
        edited: &mut Vec<PathBuf>,
        moved: &mut Vec<(PathBuf, PathBuf)>,
    ) -> Result<(), String> {
        match change {
            FileChange::Edit(edits) => {
                let path = edits.path.clone();
                let buffer = self.edit_file(edits).map_err(|error| failed(&path, error))?;
                let path = buffer.path().map_or(path, Path::to_path_buf);
                if !edited.contains(&path) {
                    edited.push(path);
                }
            }
            FileChange::Create(path) => create(&path).map_err(|error| failed(&path, error))?,
            FileChange::Rename { from, to } => {
                // As buffers have them, e.g. /private/tmp for /tmp.
                let from = fs::canonicalize(&from).unwrap_or(from);
                rename(&from, &to).map_err(|error| failed(&from, error))?;
                let to = fs::canonicalize(&to).unwrap_or(to);
                self.file_moved(&from, &to);
                for path in edited.iter_mut() {
                    if let Ok(rest) = path.strip_prefix(&from) {
                        *path =
                            if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) };
                    }
                }
                moved.push((from, to));
            }
        }
        Ok(())
    }
}

fn create(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::File::create_new(path) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => Err(error),
        _ => Ok(()),
    }
}

fn rename(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        let message = format!("{} already exists", to.display());
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, message));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(from, to)
}

fn failed(path: &Path, error: io::Error) -> String {
    format!("{}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;

    fn edit(path: &Path, range: Range<usize>, text: &str) -> FileChange {
        FileChange::Edit(FileEdits { path: path.to_owned(), edits: vec![(range, text.into())] })
    }

    fn project(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("remacs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory.canonicalize().unwrap()
    }

    #[test]
    fn edits_follow_renames_and_every_edited_file_is_saved() {
        let root = project("change-files");
        let (lib, old, new) = (root.join("lib.rs"), root.join("a.rs"), root.join("b/c.rs"));
        fs::write(&lib, "mod a;\n").unwrap();
        fs::write(&old, "fn a() {}\n").unwrap();
        let mut editor = Editor::new(&[]);
        editor.visit_file(&old).unwrap();
        editor.window_buffer_mut().insert("// mine\n");

        let changed = editor
            .change_files(vec![
                edit(&lib, 4..5, "c"),
                FileChange::Rename { from: old.clone(), to: new.clone() },
                edit(&new, 11..12, "c"),
                FileChange::Create(root.join("d.rs")),
            ])
            .unwrap();

        assert_eq!(changed.edited, ["lib.rs", "c.rs"]);
        assert_eq!(changed.moved, [(old.clone(), new.clone())]);
        assert_eq!(fs::read_to_string(&lib).unwrap(), "mod c;\n");
        assert_eq!(fs::read_to_string(&new).unwrap(), "// mine\nfn c() {}\n");
        assert!(!old.exists() && root.join("d.rs").exists());
        assert_eq!(editor.window_buffer().path(), Some(new.as_path()));
        assert!(!editor.window_buffer().is_modified());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rename_onto_an_existing_file_fails_after_saving_what_was_edited() {
        let root = project("change-files-clash");
        let (lib, taken) = (root.join("lib.rs"), root.join("b.rs"));
        fs::write(&lib, "mod a;\n").unwrap();
        fs::write(root.join("a.rs"), "").unwrap();
        fs::write(&taken, "").unwrap();
        let mut editor = Editor::new(&[]);

        let error = editor
            .change_files(vec![
                edit(&lib, 4..5, "b"),
                FileChange::Rename { from: root.join("a.rs"), to: taken },
            ])
            .unwrap_err();

        assert!(error.contains("already exists"), "{error}");
        assert_eq!(fs::read_to_string(&lib).unwrap(), "mod b;\n");
        assert!(root.join("a.rs").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
