//! Task runners' files as entry points: each target of a Makefile, each recipe of a
//! justfile, run by make or just from the file's directory.

use std::path::{Path, PathBuf};

use crate::configuration::{Configuration, Kind, Task};
use crate::entry_points::{EntryPoint, relative, script_name};

const MAKEFILES: &[&str] = &["GNUmakefile", "makefile", "Makefile"];
const JUSTFILES: &[&str] = &["justfile", "Justfile", ".justfile"];
/// Words that start a justfile line which is not a recipe.
const JUST_KEYWORDS: &[&str] = &["alias", "export", "import", "mod", "set", "unexport"];

pub fn is_task_file(name: &str) -> bool {
    MAKEFILES.contains(&name) || JUSTFILES.contains(&name)
}

/// Each task of `text`, the file at `path`, if it is a Makefile or a justfile.
pub fn task_entry_points(path: &Path, text: &str, root: &Path) -> Option<Vec<EntryPoint>> {
    let name = path.file_name()?.to_str()?;
    let (tasks, kind): (_, fn(Task) -> Kind) = if MAKEFILES.contains(&name) {
        (make_targets(text), Kind::Make)
    } else if JUSTFILES.contains(&name) {
        (just_recipes(text), Kind::Just)
    } else {
        return None;
    };
    let file = relative(path, root);
    let entry = |(line, task): (usize, &str)| {
        let task = Task { file: file.clone(), task: task.to_owned() };
        EntryPoint::at(line, Configuration::new(script_name(path, root, &task.task), kind(task)))
    };
    Some(tasks.into_iter().map(entry).collect())
}

/// What runs `task`: `program`, the file given by `flag`, the task, from the file's
/// directory.
pub fn task_command(
    program: &str,
    flag: &str,
    task: &Task,
    root: &Path,
) -> (String, Vec<String>, PathBuf) {
    let path = root.join(&task.file);
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let args = vec![flag.to_owned(), name, task.task.clone()];
    (program.to_owned(), args, file_directory(&path, root))
}

/// The directory of `path`, a file of the project at `root`.
pub fn file_directory(path: &Path, root: &Path) -> PathBuf {
    path.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf)
}

/// The targets a person runs, at their lines: not special (`.PHONY`), pattern (`%.o`),
/// variable (`$(BIN)`) or file targets (`build/app`, `main.o`).
// shortcut: included makefiles and conditionals are not followed; use `make -pRrq` if
// targets they declare are missed.
fn make_targets(text: &str) -> Vec<(usize, &str)> {
    let mut targets: Vec<(usize, &str)> = Vec::new();
    let mut continued = false;
    let mut in_define = false;
    for (index, line) in text.lines().enumerate() {
        let was_continued = std::mem::replace(&mut continued, line.ends_with('\\'));
        let first_word = line.split_whitespace().next();
        if in_define {
            in_define = first_word != Some("endef");
            continue;
        }
        if first_word == Some("define") {
            in_define = true;
            continue;
        }
        if was_continued || line.starts_with(['\t', ' ', '#']) {
            continue;
        }
        let Some((left, right)) = line.split_once(':') else {
            continue;
        };
        // `A := b`, `A ::= b` and `A = b:c` set variables.
        if left.contains('=') || right.trim_start_matches(':').starts_with('=') {
            continue;
        }
        for target in left.split_whitespace() {
            let runnable = !target.contains(['.', '%', '$', '/']);
            if runnable && !targets.iter().any(|&(_, known)| known == target) {
                targets.push((index, target));
            }
        }
    }
    targets
}

/// The public recipes, at their lines: not those named `_…` or marked `[private]`.
fn just_recipes(text: &str) -> Vec<(usize, &str)> {
    let mut private = false;
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            if line.starts_with('[') {
                private |= line.contains("private");
                return None;
            }
            let marked_private = std::mem::take(&mut private);
            let header = line.strip_prefix('@').unwrap_or(line);
            let end = header
                .find(|character: char| {
                    !(character.is_ascii_alphanumeric() || "_-".contains(character))
                })
                .unwrap_or(header.len());
            let (name, rest) = header.split_at(end);
            let recipe = !name.is_empty()
                && !name.starts_with(['_', '-'])
                && !marked_private
                && !JUST_KEYWORDS.contains(&name)
                && rest.starts_with([' ', ':'])
                // `name := value` sets a variable.
                && rest.split_once(':').is_some_and(|(_, after)| !after.starts_with('='));
            recipe.then_some((index, name))
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::launch::launch;
    use crate::settings::RunSettings;

    const MAKEFILE: &str = "\
CC := cc
URL = http://example.com
.PHONY: all test
all: build test
build: $(BIN)
\tcc -o app main.c
main.o: main.c
%.o: %.c
$(BIN): main.o
define HELP
usage: make all
endef
SOURCES = a.c \\
  b.c
test lint:: all
# done: yes
install :
";

    #[test]
    fn makefile_targets_leave_out_special_pattern_file_and_variable_ones() {
        assert_eq!(
            make_targets(MAKEFILE),
            [(3, "all"), (4, "build"), (14, "test"), (14, "lint"), (16, "install")]
        );
    }

    const JUSTFILE: &str = "\
set shell := [\"bash\", \"-c\"]
alias b := build
version := \"1.0\"
export RUST_LOG := \"debug\"

# Builds it.
build target=\"debug\":
    cargo build
@serve url=\"http://localhost:3000\": build
    open {{url}}
_helper:
    true
[private]
hidden:
    true
[group('ci')]
test *args: (build \"release\")
    cargo test {{args}}
mod tools
";

    #[test]
    fn justfile_recipes_leave_out_settings_variables_and_private_ones() {
        assert_eq!(just_recipes(JUSTFILE), [(6, "build"), (8, "serve"), (16, "test")]);
    }

    #[test]
    fn tasks_are_entry_points_run_from_their_file_directory() {
        let root = Path::new("/p");
        let points = task_entry_points(&root.join("tools/Makefile"), "build:\n", root).unwrap();
        let task = task_of("tools/Makefile", "build");
        assert_eq!(
            points,
            [EntryPoint::at(0, Configuration::new("tools: build", Kind::Make(task)))]
        );
        assert!(task_entry_points(&root.join("README"), "build:\n", root).is_none());

        let mut make = points[0].configuration.clone();
        make.args = vec!["DEBUG=1".into()];
        let launched = launch(&make, root, &RunSettings::default()).unwrap();
        assert_eq!(launched.command_line(), "make -f Makefile build DEBUG=1");
        assert_eq!(launched.cwd, root.join("tools"));

        let just = Configuration::new("test", Kind::Just(task_of("justfile", "test")));
        let launched = launch(&just, root, &RunSettings::default()).unwrap();
        assert_eq!(launched.command_line(), "just --justfile justfile test");
        assert_eq!(launched.cwd, root);
    }

    #[test]
    fn a_task_is_kept_in_the_run_file_by_its_runner_file_and_name() {
        let make = Configuration::new("build", Kind::Make(task_of("Makefile", "build")));
        let text = toml::to_string(&make).unwrap();
        assert_eq!(
            text,
            "name = \"build\"\nkind = \"make\"\nfile = \"Makefile\"\ntask = \"build\"\n"
        );
        assert_eq!(toml::from_str::<Configuration>(&text).unwrap(), make);
    }

    fn task_of(file: &str, task: &str) -> Task {
        Task { file: file.into(), task: task.into() }
    }
}
