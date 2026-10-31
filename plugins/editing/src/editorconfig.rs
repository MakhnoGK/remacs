use std::fs;
use std::path::Path;

use globset::GlobBuilder;
use remacs_core::IndentUnit;

/// The indentation `.editorconfig` files set for `file`: a nearer file's properties
/// before those farther up, which stop at one saying `root = true`.
pub(crate) fn indent_unit(file: &Path) -> Option<IndentUnit> {
    let mut properties = Properties::default();
    for directory in file.ancestors().skip(1) {
        let Ok(config) = fs::read_to_string(directory.join(".editorconfig")) else {
            continue;
        };
        let relative = file.strip_prefix(directory).ok()?;
        let (found, root) = read(&config, relative);
        properties = Properties {
            style: properties.style.or(found.style),
            size: properties.size.or(found.size),
            tab_width: properties.tab_width.or(found.tab_width),
        };
        if root {
            break;
        }
    }
    match properties.style.as_deref() {
        Some("tab") => Some(IndentUnit::Tabs),
        Some("space") | None => {
            // `tab_width` alone is how wide a tab is drawn, not a level.
            let size = match properties.size.as_deref() {
                Some("tab") => properties.tab_width.as_deref(),
                size => size,
            };
            let width = size?.parse().ok()?;
            (width > 0).then_some(IndentUnit::Spaces(width))
        }
        Some(_) => None,
    }
}

#[derive(Default)]
struct Properties {
    style: Option<String>,
    size: Option<String>,
    tab_width: Option<String>,
}

/// The properties of the sections of `config` matching `relative`, later ones overriding
/// earlier, and whether its preamble says `root = true`.
fn read(config: &str, relative: &Path) -> (Properties, bool) {
    let mut properties = Properties::default();
    let mut root = false;
    let mut section: Option<bool> = None;
    for line in config.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(glob) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            section = Some(matches(glob, relative));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_lowercase(), value.trim().to_lowercase());
        match (section, key.as_str()) {
            (None, "root") => root = value == "true",
            (Some(true), "indent_style") => properties.style = Some(value),
            (Some(true), "indent_size") => properties.size = Some(value),
            (Some(true), "tab_width") => properties.tab_width = Some(value),
            _ => {}
        }
    }
    (properties, root)
}

/// Whether a section's glob takes `relative`: one without a `/` matches the file's name
/// in any directory, as EditorConfig's globs do.
// shortcut: `{1..3}` number ranges match nothing; upgrade if a project relies on them.
fn matches(glob: &str, relative: &Path) -> bool {
    let glob = match glob.strip_prefix('/') {
        Some(anchored) => anchored.to_owned(),
        None if glob.contains('/') => glob.to_owned(),
        None => format!("**/{glob}"),
    };
    let glob = GlobBuilder::new(&glob).literal_separator(true).build();
    glob.is_ok_and(|glob| glob.compile_matcher().is_match(relative))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use remacs_core::IndentUnit;

    use super::indent_unit;

    #[test]
    fn nearer_files_and_later_sections_win_up_to_the_root() {
        let base = std::env::temp_dir().join(format!("remacs-editorconfig-{}", std::process::id()));
        let web = base.join("project/web");
        fs::create_dir_all(&web).unwrap();
        fs::write(base.join(".editorconfig"), "[*]\nindent_style = tab\n").unwrap();
        let project = "root = true\n[*]\nindent_style = space\nindent_size = 4\n\
            [*.{ts,tsx}]\nindent_size = 2\n[Makefile]\nindent_style = tab\n\
            [*.c]\nindent_size = tab\ntab_width = 8\n";
        fs::write(base.join("project/.editorconfig"), project).unwrap();
        fs::write(web.join(".editorconfig"), "[/lib/*.js]\nindent_size = 3\n").unwrap();
        let unit = |path: &str| indent_unit(&web.join(path));
        assert_eq!(unit("a.rs"), Some(IndentUnit::Spaces(4)));
        assert_eq!(unit("src/a.tsx"), Some(IndentUnit::Spaces(2)));
        assert_eq!(unit("Makefile"), Some(IndentUnit::Tabs));
        assert_eq!(unit("lib/a.js"), Some(IndentUnit::Spaces(3)));
        assert_eq!(unit("src/lib/a.js"), Some(IndentUnit::Spaces(4)));
        assert_eq!(unit("a.c"), Some(IndentUnit::Spaces(8)));
        assert_eq!(indent_unit(&base.join("a.rs")), Some(IndentUnit::Tabs));
        assert_eq!(indent_unit(Path::new("/a.rs")), None);

        let widths_only = base.join("widths-only");
        fs::create_dir_all(&widths_only).unwrap();
        fs::write(widths_only.join(".editorconfig"), "root = true\n[*]\ntab_width = 8\n").unwrap();
        assert_eq!(indent_unit(&widths_only.join("a.ts")), None);
        fs::remove_dir_all(base).unwrap();
    }
}
