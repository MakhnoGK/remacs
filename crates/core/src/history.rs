use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::push_new;

const MAX_ANSWERS: usize = 100;

/// Answers given to the minibuffer, newest first, by completion category; kept in a file
/// across sessions once given one, like Emacs savehist.
#[derive(Default)]
pub(crate) struct History {
    answers: BTreeMap<String, Vec<String>>,
    file: Option<PathBuf>,
}

impl History {
    pub fn get(&self, category: &str) -> &[String] {
        self.answers.get(category).map_or(&[], Vec::as_slice)
    }

    /// Takes in the answers of earlier sessions from `path`, after this session's, and
    /// writes all to it after each answer from now on.
    pub fn keep_in(&mut self, path: PathBuf) -> Result<(), String> {
        let earlier = read(&path).map_err(|error| describe(&path, &error))?;
        for (category, answers) in earlier {
            let kept = self.answers.entry(category).or_default();
            push_new(kept, answers);
            kept.truncate(MAX_ANSWERS);
        }
        self.file = Some(path);
        Ok(())
    }

    pub fn add(&mut self, category: &str, answer: String) -> Result<(), String> {
        let answers = self.answers.entry(category.to_owned()).or_default();
        answers.retain(|earlier| *earlier != answer);
        answers.insert(0, answer);
        answers.truncate(MAX_ANSWERS);
        match &self.file {
            Some(path) => write(path, &self.answers).map_err(|error| describe(path, &error)),
            None => Ok(()),
        }
    }
}

fn read(path: &Path) -> io::Result<BTreeMap<String, Vec<String>>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error),
    }
}

/// Through a temporary file, so that a crash never leaves half a history.
fn write(path: &Path, answers: &BTreeMap<String, Vec<String>>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(answers)?)?;
    fs::rename(temporary, path)
}

fn describe(path: &Path, error: &io::Error) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str());
    format!("{}: {error}", name.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_file(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("remacs-history-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("state").join("history.json")
    }

    #[test]
    fn answers_last_across_sessions_newest_first_without_repeats() {
        let path = history_file("sessions");
        let mut first = History::default();
        first.keep_in(path.clone()).unwrap();
        first.add("command", "find-file".into()).unwrap();
        first.add("command", "save-buffer".into()).unwrap();
        first.add("command", "find-file".into()).unwrap();

        let mut second = History::default();
        second.add("command", "dired".into()).unwrap();
        second.keep_in(path.clone()).unwrap();
        assert_eq!(second.get("command"), ["dired", "find-file", "save-buffer"]);
        assert!(second.get("file").is_empty());
        fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_broken_file_is_reported_and_left_alone() {
        let path = history_file("broken");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{").unwrap();
        let mut history = History::default();
        let error = history.keep_in(path.clone()).unwrap_err();
        assert!(error.starts_with("history.json: "), "{error}");
        history.add("command", "dired".into()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{");
        fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }
}
