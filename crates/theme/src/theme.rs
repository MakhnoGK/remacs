use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use toml::{Table, Value};

use crate::color::{self, ANSI_COLORS, Colors};
use crate::glass::{self, Glass, Ink};
use crate::syntax::Styles;
use crate::{Rgb, ensure_theme};

/// Themes compiled in, each complete; a theme of the user's takes what it leaves out from
/// one of them.
pub const BUILT_IN: &[(&str, &str)] = &[
    (DEFAULT, include_str!("../themes/modus-vivendi.toml")),
    (LIGHT, include_str!("../themes/modus-operandi.toml")),
];

pub const DEFAULT: &str = "modus-vivendi";
/// What a light theme of the user's builds on.
pub const LIGHT: &str = "modus-operandi";
/// Not a theme: the light or the dark one, as macOS is.
pub const SYSTEM: &str = "system";

/// How far one theme may build on another, which builds on another…
const MAX_INHERITANCE: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    #[default]
    Dark,
}

/// Every token of the interface's look.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Theme {
    /// For people, e.g. `Modus Operandi`.
    pub name: String,
    pub appearance: Appearance,
    /// Of the window's tint over the desktop, unless `ui.toml` sets one.
    pub opacity: f32,
    pub colors: Colors,
    /// The 16 colors programs name with ANSI escape codes, normal then bright.
    pub ansi: [Rgb; ANSI_COLORS],
    pub styles: Styles,
    pub glass: Glass,
    pub ink: Ink,
    pub shadow: Ink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeError {
    /// The theme's file, when the theme is the user's.
    pub file: Option<PathBuf>,
    /// 1-based.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ThemeError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        if let Some(name) = self.file.as_deref().and_then(Path::file_name) {
            write!(formatter, "{}", name.display())?;
            if let Some(line) = self.line {
                write!(formatter, ":{line}")?;
            }
            write!(formatter, ": ")?;
        }
        write!(formatter, "{}", self.message)
    }
}

impl Theme {
    /// `name` is one of `BUILT_IN`.
    pub fn built_in(name: &str) -> Theme {
        built_in_table(name)
            .and_then(|table| from_table(table, None))
            .unwrap_or_else(|error| panic!("built-in theme {name}: {error}"))
    }

    /// The theme called `name`: the user's file `<name>.toml` in `dir`, else a built-in
    /// one. A file of the user's takes what it leaves out from the theme it `inherits`,
    /// else from the built-in one of its appearance.
    pub fn load(name: &str, dir: Option<&Path>) -> Result<Theme, ThemeError> {
        let file = user_file(name, dir);
        from_table(resolve(name, dir, 0)?, file)
    }

    pub(crate) fn put_in_place(&self) {
        self.colors.put_in_place();
        color::put_ansi_in_place(&self.ansi);
        self.styles.put_in_place();
        self.shown_glass().put_in_place();
        glass::put_inks_in_place(self.ink, self.shadow);
        LIGHT_APPEARANCE.store(self.appearance == Appearance::Light, Ordering::Relaxed);
    }

    /// Only Metal blurs (ADR 0035): elsewhere panes, popups and modals are their tint over
    /// the background, opaque, so that what is under them does not show through.
    fn shown_glass(&self) -> Glass {
        let mut glass = self.glass;
        if cfg!(not(target_os = "macos")) {
            let Glass { surface, note, modal, pill, pane, .. } = &mut glass;
            for tint in [surface, note, modal, pill, pane] {
                *tint = tint.over(self.colors.bg_main);
            }
        }
        glass
    }
}

static LIGHT_APPEARANCE: AtomicBool = AtomicBool::new(false);

/// Draws everything from `theme` from now on.
pub fn install(theme: &Theme) {
    ensure_theme();
    theme.put_in_place();
}

pub fn appearance() -> Appearance {
    ensure_theme();
    if LIGHT_APPEARANCE.load(Ordering::Relaxed) { Appearance::Light } else { Appearance::Dark }
}

/// The built-in themes' names and those of the user's files in `dir`, sorted.
pub fn names(dir: Option<&Path>) -> Vec<String> {
    let mut names: Vec<String> = BUILT_IN.iter().map(|(name, _)| name.to_string()).collect();
    let paths = dir.and_then(|dir| fs::read_dir(dir).ok()).into_iter().flatten().flatten();
    let files = paths.map(|entry| entry.path());
    let themes = files.filter(|path| path.extension() == Some(OsStr::new("toml")));
    names.extend(themes.filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned())));
    names.sort();
    names.dedup();
    names
}

fn user_file(name: &str, dir: Option<&Path>) -> Option<PathBuf> {
    dir.map(|dir| dir.join(format!("{name}.toml"))).filter(|path| path.is_file())
}

fn built_in_table(name: &str) -> Result<Table, ThemeError> {
    let parsed = match BUILT_IN.iter().find(|(built_in, _)| *built_in == name) {
        Some((_, text)) => toml::from_str(text).map_err(|error| error.message().to_owned()),
        None => Err(format!("there is no theme {name}")),
    };
    parsed.map_err(|message| ThemeError { file: None, line: None, message })
}

/// The theme's whole table, what it inherits merged under its own.
fn resolve(name: &str, dir: Option<&Path>, depth: usize) -> Result<Table, ThemeError> {
    let Some(path) = user_file(name, dir) else {
        return built_in_table(name);
    };
    let error = |line, message| ThemeError { file: Some(path.clone()), line, message };
    if depth == MAX_INHERITANCE {
        return Err(error(None, format!("inherits more than {MAX_INHERITANCE} themes deep")));
    }
    let text = fs::read_to_string(&path).map_err(|read| error(None, read.to_string()))?;
    let mut own: Table = toml::from_str(&text).map_err(|parse| {
        let line = parse.span().map(|span| line_at(&text, span.start));
        error(line, parse.message().to_owned())
    })?;
    // A file named like a built-in theme changes that theme.
    let built_in = BUILT_IN.iter().any(|(built_in, _)| *built_in == name);
    let base = match own.remove("inherits") {
        None if built_in => built_in_table(name)?,
        None => built_in_table(match own.get("appearance").and_then(Value::as_str) {
            Some("light") => LIGHT,
            _ => DEFAULT,
        })?,
        Some(Value::String(base)) if base == name => built_in_table(&base)?,
        Some(Value::String(base)) => resolve(&base, dir, depth + 1)?,
        Some(_) => return Err(error(None, "inherits is not a theme's name".to_owned())),
    };
    Ok(merged(base, own))
}

/// `over`'s values in place of `under`'s, table by table.
fn merged(mut under: Table, over: Table) -> Table {
    for (key, value) in over {
        let value = match (under.remove(&key), value) {
            (Some(Value::Table(under)), Value::Table(over)) => Value::Table(merged(under, over)),
            (_, value) => value,
        };
        under.insert(key, value);
    }
    under
}

fn from_table(table: Table, file: Option<PathBuf>) -> Result<Theme, ThemeError> {
    let error = |message| ThemeError { file: file.clone(), line: None, message };
    let theme: Theme = Value::Table(table)
        .try_into()
        .map_err(|parse: toml::de::Error| error(parse.message().to_owned()))?;
    if !(0.0..=1.0).contains(&theme.opacity) {
        return Err(error(format!("opacity {} is outside 0..=1", theme.opacity)));
    }
    for (name, ink) in [("ink", theme.ink), ("shadow", theme.shadow)] {
        if !(0.0..=4.0).contains(&ink.strength) {
            return Err(error(format!("{name}.strength {} is outside 0..=4", ink.strength)));
        }
    }
    Ok(theme)
}

fn line_at(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Style;

    /// `test` keeps the directories of tests running at once apart.
    fn themes_dir(test: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("remacs-themes-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn a_tint_over_the_background_is_one_opaque_color() {
        use crate::Rgba;
        assert_eq!(Rgba(0xffffff80).over(Rgb(0x000000)), Rgba(0x808080ff));
        assert_eq!(Rgba(0x123456ff).over(Rgb(0xffffff)), Rgba(0x123456ff));
        assert_eq!(Rgba(0x12345600).over(Rgb(0xabcdef)), Rgba(0xabcdefff));
    }

    #[test]
    fn built_in_themes_are_complete() {
        for (name, _) in BUILT_IN {
            let theme = Theme::load(name, None).unwrap();
            assert_eq!(theme, Theme::built_in(name));
        }
        assert_eq!(Theme::built_in(DEFAULT).appearance, Appearance::Dark);
        assert_eq!(Theme::built_in(LIGHT).appearance, Appearance::Light);
    }

    #[test]
    fn modus_operandi_keeps_its_colors_and_plain_styles() {
        let theme = Theme::built_in(LIGHT);
        assert_eq!(theme.colors.bg_main, Rgb(0xffffff));
        assert_eq!(theme.colors.keyword, Rgb(0x531ab6));
        assert_eq!(theme.colors.string, Rgb(0x3548cf));
        assert_eq!(theme.glass.region.0, 0xbdbdbdff);
        assert_eq!(theme.glass.line_highlight.0, 0xdae5ecff);
        assert_eq!(theme.styles.comment, Style::default());
        assert_eq!(theme.styles.keyword, Style::default());
    }

    #[test]
    fn a_users_theme_takes_what_it_leaves_out_from_the_built_in_one_of_its_appearance() {
        let paper = "name = \"Paper\"\nappearance = \"light\"\n[colors]\nkeyword = \"#123456\"\n";
        let night = "name = \"Night\"\n[glass]\nregion = \"#11223344\"\n";
        let dir = themes_dir("appearance", &[("paper.toml", paper), ("night.toml", night)]);
        let paper = Theme::load("paper", Some(&dir)).unwrap();
        assert_eq!(paper.name, "Paper");
        assert_eq!(paper.colors.keyword, Rgb(0x123456));
        assert_eq!(paper.colors.string, Theme::built_in(LIGHT).colors.string);
        let night = Theme::load("night", Some(&dir)).unwrap();
        assert_eq!(night.appearance, Appearance::Dark);
        assert_eq!(night.glass.region.0, 0x11223344);
        assert_eq!(night.colors, Theme::built_in(DEFAULT).colors);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_users_theme_may_inherit_another_and_change_a_built_in_one() {
        let files = [
            ("base.toml", "name = \"Base\"\n[colors]\nstring = \"#00ff00\"\n"),
            ("child.toml", "inherits = \"base\"\nname = \"Child\"\n"),
            ("modus-operandi.toml", "opacity = 0.5\n"),
            ("loop.toml", "inherits = \"loop-too\"\n"),
            ("loop-too.toml", "inherits = \"loop\"\n"),
        ];
        let dir = themes_dir("inherits", &files);
        let child = Theme::load("child", Some(&dir)).unwrap();
        assert_eq!((child.name.as_str(), child.colors.string), ("Child", Rgb(0x00ff00)));
        let changed = Theme::load(LIGHT, Some(&dir)).unwrap();
        assert_eq!(changed.opacity, 0.5);
        assert_eq!(changed.colors, Theme::built_in(LIGHT).colors);
        let error = Theme::load("loop", Some(&dir)).unwrap_err();
        assert!(error.message.contains("deep"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_unknown_tokens_bad_colors_and_missing_themes() {
        let files = [
            ("unknown.toml", "[colors]\nkeywords = \"#000000\"\n"),
            ("alpha.toml", "[colors]\nkeyword = \"#00000080\"\n"),
            ("syntax.toml", "name = \"A\"\n[colors\n"),
            ("opacity.toml", "opacity = 2.0\n"),
        ];
        let dir = themes_dir("errors", &files);
        let message = |name| Theme::load(name, Some(&dir)).unwrap_err().to_string();
        for (name, part) in
            [("unknown", "keywords"), ("alpha", "not a color"), ("opacity", "outside")]
        {
            assert!(message(name).contains(part), "{}", message(name));
        }
        let syntax = message("syntax");
        assert!(syntax.starts_with("syntax.toml:2: "), "{syntax}");
        assert_eq!(message("absent"), "there is no theme absent");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lists_built_in_themes_and_the_users() {
        let dir = themes_dir("names", &[("paper.toml", ""), ("notes.txt", "")]);
        assert_eq!(names(Some(&dir)), ["modus-operandi", "modus-vivendi", "paper"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn inks_scale_the_alpha_asked_for() {
        let ink = Ink { color: Rgb(0x000000), strength: 0.5 };
        install(&Theme::built_in(LIGHT));
        glass::put_inks_in_place(ink, Ink { strength: 4.0, ..ink });
        assert_eq!(glass::ink(0x40), 0x00000020);
        assert_eq!(glass::shadow(0x80), 0x000000ff);
        install(&Theme::built_in(DEFAULT));
        assert_eq!(glass::ink(0x14), 0xffffff14);
    }
}
