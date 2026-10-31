use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Which paths of a working tree Git ignores, so that changes to them, such as a build's
/// output, do not cause the status to be read again.
pub struct IgnoreRules {
    root: PathBuf,
    /// `.git/info/exclude`, then the user's `core.excludesFile`.
    shared: [Gitignore; 2],
    /// By directory, read when first needed.
    directories: HashMap<PathBuf, Gitignore>,
}

impl IgnoreRules {
    pub fn new(root: &Path, git_directory: &Path) -> Self {
        let mut exclude = GitignoreBuilder::new(root);
        exclude.add(git_directory.join("info/exclude"));
        Self {
            root: root.to_path_buf(),
            shared: [
                exclude.build().unwrap_or_else(|_| Gitignore::empty()),
                // Rooted at the working tree, as matching asks of every path.
                GitignoreBuilder::new(root).build_global().0,
            ],
            directories: HashMap::new(),
        }
    }

    /// Whether `path`, under the root, is ignored, the deepest `.gitignore` deciding.
    pub fn is_ignored(&mut self, path: &Path) -> bool {
        let is_dir = path.is_dir();
        let directories: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .take_while(|directory| directory.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        for directory in directories {
            match self.matcher(&directory).matched_path_or_any_parents(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        self.shared.iter().any(|rules| rules.matched_path_or_any_parents(path, is_dir).is_ignore())
    }

    /// Forgets the rules of `directory`, whose `.gitignore` changed.
    pub fn forget(&mut self, directory: &Path) {
        self.directories.remove(directory);
    }

    fn matcher(&mut self, directory: &Path) -> &Gitignore {
        self.directories
            .entry(directory.to_path_buf())
            .or_insert_with(|| Gitignore::new(directory.join(".gitignore")).0)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn the_deepest_ignore_file_decides() {
        let root = std::env::temp_dir().join(format!("remacs-git-ignore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git/info")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        fs::write(root.join("logs/.gitignore"), "!keep.log\n").unwrap();
        fs::write(root.join(".git/info/exclude"), "scratch.txt\n").unwrap();
        let mut rules = IgnoreRules::new(&root, &root.join(".git"));

        assert!(rules.is_ignored(&root.join("target/debug/remacs")));
        assert!(rules.is_ignored(&root.join("target")));
        assert!(rules.is_ignored(&root.join("logs/run.log")));
        assert!(!rules.is_ignored(&root.join("logs/keep.log")));
        assert!(rules.is_ignored(&root.join("scratch.txt")));
        assert!(!rules.is_ignored(&root.join("src/lib.rs")));

        fs::write(root.join(".gitignore"), "").unwrap();
        assert!(rules.is_ignored(&root.join("target/debug/remacs")));
        rules.forget(&root);
        assert!(!rules.is_ignored(&root.join("target/debug/remacs")));
        let _ = fs::remove_dir_all(&root);
    }
}
