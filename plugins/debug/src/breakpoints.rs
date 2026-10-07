//! Where programs stop: breakpoints by file and line, each maybe with a condition, a hit
//! count or a message to log instead of stopping. They are kept across sessions and move
//! with the lines they are on as the text is edited.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use remacs_text::{Edit, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Breakpoint {
    /// From 0.
    pub line: usize,
    /// Stops only while this expression is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Stops only once hit so many times, e.g. `3` or `>= 3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Logged instead of stopping; `{expression}`s are filled in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
    pub enabled: bool,
    /// What the adapter calls it, and said of it: whether it found code there, and why
    /// not.
    #[serde(skip)]
    pub id: Option<i64>,
    #[serde(skip)]
    pub verified: Option<bool>,
    #[serde(skip)]
    pub message: Option<String>,
}

impl Breakpoint {
    pub fn at(line: usize) -> Self {
        Self {
            line,
            enabled: true,
            ..Self::default()
        }
    }
}

#[derive(Default)]
pub struct Breakpoints {
    pub files: BTreeMap<PathBuf, Vec<Breakpoint>>,
    /// The text of each file with breakpoints as last seen, to move them along edits.
    seen: HashMap<PathBuf, Snapshot>,
    /// Where they are kept between sessions.
    store: Option<PathBuf>,
}

impl Breakpoints {
    /// Keeps them in `store`, reading those kept there.
    pub fn keep_in(&mut self, store: PathBuf) {
        if let Ok(text) = fs::read_to_string(&store)
            && let Ok(files) = serde_json::from_str(&text)
        {
            self.files = files;
        }
        self.store = Some(store);
    }

    pub fn save(&self) {
        let Some(store) = &self.store else {
            return;
        };
        if let Some(dir) = store.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.files) {
            let _ = fs::write(store, text);
        }
    }

    /// Adds one at `line` of `file`, or removes the one there; true when added.
    pub fn toggle(&mut self, file: &Path, line: usize) -> bool {
        if self.remove(file, line) {
            return false;
        }
        self.add(file, Breakpoint::at(line));
        true
    }

    pub fn add(&mut self, file: &Path, breakpoint: Breakpoint) {
        let breakpoints = self.files.entry(file.to_path_buf()).or_default();
        breakpoints.retain(|other| other.line != breakpoint.line);
        breakpoints.push(breakpoint);
        breakpoints.sort_by_key(|breakpoint| breakpoint.line);
    }

    /// True when there was one.
    pub fn remove(&mut self, file: &Path, line: usize) -> bool {
        let Some(breakpoints) = self.files.get_mut(file) else {
            return false;
        };
        let before = breakpoints.len();
        breakpoints.retain(|breakpoint| breakpoint.line != line);
        let removed = breakpoints.len() < before;
        if breakpoints.is_empty() {
            self.files.remove(file);
            self.seen.remove(file);
        }
        removed
    }

    pub fn of(&self, file: &Path) -> &[Breakpoint] {
        self.files.get(file).map_or(&[], Vec::as_slice)
    }

    pub fn at(&self, file: &Path, line: usize) -> Option<&Breakpoint> {
        self.of(file)
            .iter()
            .find(|breakpoint| breakpoint.line == line)
    }

    pub fn at_mut(&mut self, file: &Path, line: usize) -> Option<&mut Breakpoint> {
        self.files
            .get_mut(file)?
            .iter_mut()
            .find(|breakpoint| breakpoint.line == line)
    }

    /// The `setBreakpoints` request's arguments for `file`: its enabled breakpoints.
    pub fn request(&self, file: &Path) -> Value {
        let breakpoints: Vec<Value> = self
            .of(file)
            .iter()
            .filter(|breakpoint| breakpoint.enabled)
            .map(|breakpoint| {
                let mut source = json!({ "line": breakpoint.line + 1 });
                let optional = [
                    ("condition", &breakpoint.condition),
                    ("hitCondition", &breakpoint.hit_condition),
                    ("logMessage", &breakpoint.log_message),
                ];
                for (key, value) in optional {
                    if let Some(value) = value {
                        source[key] = value.clone().into();
                    }
                }
                source
            })
            .collect();
        json!({
            "source": { "path": file, "name": file.file_name().map(|name| name.to_string_lossy()) },
            "breakpoints": breakpoints,
        })
    }

    /// Takes what the adapter said of `file`'s enabled breakpoints, in the order sent.
    pub fn verify(&mut self, file: &Path, answers: &[Value]) {
        let Some(breakpoints) = self.files.get_mut(file) else {
            return;
        };
        let enabled = breakpoints
            .iter_mut()
            .filter(|breakpoint| breakpoint.enabled);
        for (breakpoint, answer) in enabled.zip(answers) {
            take_answer(breakpoint, answer);
        }
    }

    /// Takes what the adapter says of one breakpoint later, e.g. once code is loaded.
    pub fn changed(&mut self, answer: &Value) {
        let Some(id) = answer["id"].as_i64() else {
            return;
        };
        let breakpoint = self
            .files
            .values_mut()
            .flatten()
            .find(|breakpoint| breakpoint.id == Some(id));
        if let Some(breakpoint) = breakpoint {
            take_answer(breakpoint, answer);
        }
    }

    /// Files that have breakpoints.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.keys().cloned().collect()
    }

    /// Moves `file`'s breakpoints along the edits that made `text` of the text last seen;
    /// true when any moved. Breakpoints on lines that went join the line the edit is on.
    pub fn follow(&mut self, file: &Path, text: Snapshot) -> bool {
        let Some(breakpoints) = self.files.get_mut(file) else {
            return false;
        };
        let Some(old) = self.seen.insert(file.to_path_buf(), text.clone()) else {
            return false;
        };
        let Some(edit) = text.edit_from(&old) else {
            return false;
        };
        let mut moved = false;
        for breakpoint in breakpoints.iter_mut() {
            let line = follow_line(breakpoint.line, &old, &text, edit);
            moved |= line != breakpoint.line;
            breakpoint.line = line;
        }
        breakpoints.dedup_by_key(|breakpoint| breakpoint.line);
        moved
    }
}

/// Where `line` of `old` is in `new` after `edit`: lines after the edit move by the lines
/// it added or removed, and so does a line whose code starts only after the edit, e.g.
/// one a newline was typed before; a line whose code it changed stays.
fn follow_line(line: usize, old: &Snapshot, new: &Snapshot, edit: Edit) -> usize {
    let first = old.line_of(edit.start);
    let last = old.line_of(edit.old_end);
    if line < first {
        return line;
    }
    let shifted = (line + new.line_of(edit.new_end)).saturating_sub(last);
    if line > last {
        return shifted;
    }
    let Some(range) = old.line_range(line) else {
        return line;
    };
    let indent = old
        .bytes_in(range.clone())
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    if range.start + indent >= edit.old_end {
        shifted
    } else {
        line.min(new.line_count().saturating_sub(1))
    }
}

fn take_answer(breakpoint: &mut Breakpoint, answer: &Value) {
    breakpoint.id = answer["id"].as_i64().or(breakpoint.id);
    breakpoint.verified = answer["verified"].as_bool();
    breakpoint.message = answer["message"].as_str().map(str::to_owned);
}
