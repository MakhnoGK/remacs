use std::collections::BTreeMap;
use std::path::Path;

use remacs_text::Text;
use serde::Deserialize;

use crate::Section;

pub const FUNDAMENTAL_MODE: &str = "Fundamental";

/// Lines read from each end of a file for a modeline, as vim's `modelines`.
const MODELINE_LINES: usize = 5;
/// Bytes read of each of those lines, so that a minified file costs nothing.
const MODELINE_BYTES: usize = 1024;

#[derive(Clone, Copy, Default)]
pub struct Mode {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// Whole file names, for files without a telling extension like `Makefile`.
    pub file_names: &'static [&'static str],
    /// Programs a `#!` line runs, without their version: `python` for `python3.12`.
    pub interpreters: &'static [&'static str],
    /// Other names a `-*- mode: … -*-` or vim modeline gives it, like `sh` or `cpp`.
    pub aliases: &'static [&'static str],
    /// How a file without a telling name begins, ignoring case, like `<?xml`.
    pub first_lines: &'static [&'static str],
    /// Brackets and quotes typed in pairs, opener first.
    pub pairs: &'static [(char, char)],
    pub comment: Option<CommentSyntax>,
    /// For lines of markup embedded in code, like JSX children.
    pub markup_comment: Option<CommentSyntax>,
}

/// How a line is commented out: `start` before it, and `end` after it for languages
/// with block comments only; empty for line comments. Both include their padding space.
#[derive(Clone, Copy)]
pub struct CommentSyntax {
    pub start: &'static str,
    pub end: &'static str,
}

/// `modes.toml`: modes chosen ahead of the built-in choice, like Emacs's
/// `auto-mode-alist` and `interpreter-mode-alist`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ModesSettings {
    /// A file name, or `*` and the end of one, to the mode's name.
    pub files: BTreeMap<String, String>,
    /// A program a `#!` line runs to the mode's name.
    pub interpreters: BTreeMap<String, String>,
}

impl Section for ModesSettings {
    const NAME: &str = "modes";
}

#[derive(Default)]
pub struct Modes {
    modes: Vec<Mode>,
    files: Vec<(String, &'static str)>,
    interpreters: Vec<(String, &'static str)>,
}

impl Modes {
    pub fn add(&mut self, mode: Mode) {
        self.modes.push(mode);
    }

    pub fn get(&self, name: &str) -> Option<&Mode> {
        self.modes.iter().find(|mode| mode.name == name)
    }

    /// The name of the first mode `matches`.
    fn find(&self, matches: impl FnMut(&&Mode) -> bool) -> Option<&'static str> {
        Some(self.modes.iter().find(matches)?.name)
    }

    /// Takes `modes.toml`'s choices; the errors are those naming no mode.
    pub fn set_settings(&mut self, settings: &ModesSettings) -> Vec<String> {
        let mut errors = Vec::new();
        let mut resolve = |table: &BTreeMap<String, String>| -> Vec<(String, &'static str)> {
            let named = table.iter().filter_map(|(key, name)| {
                let mode = self.get(name).map(|mode| mode.name);
                if mode.is_none() {
                    errors.push(format!("modes.toml: no mode {name}"));
                }
                Some((key.clone(), mode?))
            });
            named.collect()
        };
        let (files, interpreters) = (resolve(&settings.files), resolve(&settings.interpreters));
        (self.files, self.interpreters) = (files, interpreters);
        errors
    }

    /// The mode of a file at `path`, if it has one, by its name alone.
    pub fn name_for(&self, path: Option<&Path>) -> &'static str {
        self.named(path).unwrap_or(FUNDAMENTAL_MODE)
    }

    /// The mode of the file at `path` holding `text`, as Emacs `set-auto-mode` tells it: a
    /// `-*- mode: … -*-` or vim modeline, else the program its `#!` line runs, else its
    /// name, else how it begins.
    pub fn name_for_file(&self, path: &Path, text: &Text) -> &'static str {
        let lines = head_and_tail(text);
        let first = lines.first().map_or("", String::as_str);
        let cookie_line = if first.starts_with("#!") { lines.get(1) } else { lines.first() };
        let declared = cookie_line
            .and_then(|line| emacs_mode(line))
            .or_else(|| lines.iter().find_map(|line| vim_filetype(line)))
            .and_then(|name| self.declared(name));
        declared
            .or_else(|| self.interpreted(first))
            .or_else(|| self.named(Some(path)))
            .or_else(|| self.begun(first))
            .unwrap_or(FUNDAMENTAL_MODE)
    }

    fn named(&self, path: Option<&Path>) -> Option<&'static str> {
        let file_name = path?.file_name()?.to_str()?;
        let chosen = self.files.iter().find(|(pattern, _)| match pattern.strip_prefix('*') {
            Some(end) => file_name.ends_with(end),
            None => file_name == pattern,
        });
        let extension = Path::new(file_name).extension().and_then(|extension| extension.to_str());
        let extended =
            |mode: &&Mode| extension.is_some_and(|extension| mode.extensions.contains(&extension));
        (chosen.map(|&(_, name)| name))
            .or_else(|| self.find(|mode| mode.file_names.contains(&file_name)))
            .or_else(|| self.find(extended))
    }

    fn declared(&self, name: &str) -> Option<&'static str> {
        let same = |known: &str| known.eq_ignore_ascii_case(name);
        self.find(|mode| same(mode.name))
            .or_else(|| self.find(|mode| mode.aliases.iter().any(|alias| same(alias))))
    }

    fn interpreted(&self, first: &str) -> Option<&'static str> {
        let program = interpreter(first)?;
        let chosen = self.interpreters.iter().find(|(name, _)| *name == program);
        (chosen.map(|&(_, name)| name))
            .or_else(|| self.find(|mode| mode.interpreters.contains(&program)))
    }

    fn begun(&self, first: &str) -> Option<&'static str> {
        let first = first.trim_start_matches('\u{feff}').trim_start();
        let begins = |start: &&str| {
            first.get(..start.len()).is_some_and(|head| head.eq_ignore_ascii_case(start))
        };
        self.find(|mode| mode.first_lines.iter().any(begins))
    }
}

/// The first and last lines a modeline may be on, each cut short.
fn head_and_tail(text: &Text) -> Vec<String> {
    let count = text.line_count();
    let tail = count.saturating_sub(MODELINE_LINES).max(MODELINE_LINES.min(count));
    let cut_short = |line| {
        let range = text.line_range(line)?;
        let end = range.end.min(range.start + MODELINE_BYTES);
        Some(String::from_utf8_lossy(&text.bytes_in(range.start..end)).into_owned())
    };
    (0..MODELINE_LINES.min(count)).chain(tail..count).filter_map(cut_short).collect()
}

/// The mode of `-*- mode: python -*-` or `-*- python -*-`, without Emacs's `-mode` or `-ts`.
fn emacs_mode(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("-*-")?;
    let (cookie, _) = rest.split_once("-*-")?;
    let name = if cookie.contains(':') {
        cookie.split(';').find_map(|variable| {
            let (name, value) = variable.split_once(':')?;
            name.trim().eq_ignore_ascii_case("mode").then_some(value)
        })?
    } else {
        cookie
    };
    let name = name.trim();
    let name = name.strip_suffix("-mode").unwrap_or(name);
    Some(name.strip_suffix("-ts").unwrap_or(name)).filter(|name| !name.is_empty())
}

/// The `filetype` of a vim modeline: `vim: set ft=python :` or `vi: filetype=sh`.
fn vim_filetype(line: &str) -> Option<&str> {
    let start = ["vim:", "vi:", "ex:"].iter().find_map(|marker| {
        let at = line.find(marker)?;
        let after_space = at == 0 || line[..at].ends_with(char::is_whitespace);
        after_space.then(|| at + marker.len())
    })?;
    line[start..]
        .split([' ', '\t', ':'])
        .find_map(|option| option.strip_prefix("ft=").or_else(|| option.strip_prefix("filetype=")))
        .filter(|name| !name.is_empty())
}

/// The program a `#!` line runs, through `env` too, without its version digits.
fn interpreter(first: &str) -> Option<&str> {
    let mut words = first.strip_prefix("#!")?.split_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        program = words.find(|word| !word.starts_with('-') && !word.contains('='))?;
    }
    Some(program.trim_end_matches(|char: char| char.is_ascii_digit() || char == '.'))
        .filter(|program| !program.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_config::assert_default_file::<ModesSettings>();
    }

    fn modes() -> Modes {
        let mut modes = Modes::default();
        let none: &[&str] = &[];
        for (name, [extensions, interpreters, aliases, first_lines]) in [
            ("Python", [&["py"][..], &["python"], none, none]),
            ("Bash", [&["sh"], &["bash", "sh"], &["sh", "shell-script"], none]),
            ("C++", [&["cpp", "h"], none, &["cpp"], none]),
            ("XML", [none, none, none, &["<?xml"]]),
        ] {
            modes.add(Mode {
                name,
                extensions,
                interpreters,
                aliases,
                first_lines,
                ..Mode::default()
            });
        }
        modes
    }

    fn mode_of(modes: &Modes, path: &str, text: &str) -> &'static str {
        modes.name_for_file(Path::new(path), &Text::from(text))
    }

    #[test]
    fn a_file_says_its_mode_by_modeline_then_interpreter_then_name_then_first_line() {
        let modes = modes();
        let cases = [
            ("a.h", "// -*- mode: c++; coding: utf-8 -*-\n", "C++"),
            ("a.h", "/* -*- C++ -*- */\n", "C++"),
            ("x", "#!/bin/sh\n# -*- mode: python-ts -*-\n", "Python"),
            ("x", "# -*- coding: utf-8 -*-\n", FUNDAMENTAL_MODE),
            ("a.py", "x = 1\n\n\n\n\n\n\n# vim: set ft=sh :\n", "Bash"),
            ("a.py", "x = 1\n\n\n\n\n\n\nvim: ft=sh\n", "Bash"),
            ("a.py", "x = 1  # novim: ft=sh\n", "Python"),
            ("run", "#!/usr/bin/env -S python3.12 -u\n", "Python"),
            ("run", "#!/bin/bash -e\n", "Bash"),
            ("run.py", "#!/usr/bin/env node\n", "Python"),
            ("feed", "\u{feff}<?XML version=\"1.0\"?>\n", "XML"),
            ("a.cpp", "<?xml\n", "C++"),
            ("a.txt", "", FUNDAMENTAL_MODE),
        ];
        for (path, text, mode) in cases {
            assert_eq!(mode_of(&modes, path, text), mode, "{path}: {text:?}");
        }
    }

    #[test]
    fn modes_toml_comes_before_the_modes_own_files_and_interpreters() {
        let mut modes = modes();
        let settings = ModesSettings {
            files: [("*.h".into(), "Bash".into()), ("Jenkinsfile".into(), "Python".into())].into(),
            interpreters: [("deno".into(), "Bash".into()), ("x".into(), "Cobol".into())].into(),
        };
        assert_eq!(modes.set_settings(&settings), ["modes.toml: no mode Cobol"]);
        assert_eq!(modes.name_for(Some(Path::new("a/b.h"))), "Bash");
        assert_eq!(modes.name_for(Some(Path::new("Jenkinsfile"))), "Python");
        assert_eq!(mode_of(&modes, "run", "#!/usr/bin/env deno\n"), "Bash");
        assert_eq!(mode_of(&modes, "a.h", "// -*- c++ -*-\n"), "C++");
    }
}
