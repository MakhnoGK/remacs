use std::ops::RangeInclusive;
use std::sync::Arc;

use gpui::TextStrokes;
use remacs_config::{Section, Settings, SettingsError, config_dir, home};
use remacs_theme::font::CodeFont;
use serde::Deserialize;

const FONT_SIZES: RangeInclusive<f32> = 6.0..=72.0;
const LINE_HEIGHTS: RangeInclusive<f32> = 1.0..=3.0;

/// The `ui` section of the config, defaults where it is wrong, and what is wrong with it.
pub fn load_ui_settings() -> (UiSettings, Vec<SettingsError>) {
    let Some(home) = home() else {
        return (UiSettings::default(), Vec::new());
    };
    let (settings, mut errors) = Settings::load(&config_dir(&home));
    let ui = settings
        .section::<UiSettings>()
        .unwrap_or_else(|(error, defaults)| {
            errors.push(error);
            defaults
        });
    (ui, errors)
}

/// `ui.toml`.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct UiSettings {
    pub font: FontSettings,
    pub cursor: CursorSettings,
}

impl Section for UiSettings {
    const NAME: &str = "ui";

    fn check(&self) -> Result<(), String> {
        let font = &self.font;
        if font.family.trim().is_empty() {
            return Err("font.family is empty".to_owned());
        }
        if !FONT_SIZES.contains(&font.size) {
            return Err(format!(
                "font.size {} is outside {}..={}",
                font.size,
                FONT_SIZES.start(),
                FONT_SIZES.end()
            ));
        }
        if !LINE_HEIGHTS.contains(&font.line_height) {
            return Err(format!(
                "font.line-height {} is outside {}..={}",
                font.line_height,
                LINE_HEIGHTS.start(),
                LINE_HEIGHTS.end()
            ));
        }
        Ok(())
    }
}

/// The font of code.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct FontSettings {
    pub family: String,
    pub size: f32,
    /// Times the size.
    pub line_height: f32,
    pub strokes: Strokes,
}

impl FontSettings {
    pub fn code_font(&self) -> CodeFont {
        CodeFont {
            family: Arc::from(self.family.as_str()),
            size: self.size,
            // Whole pixels keep rows from blurring at their edges.
            line_height: (self.size * self.line_height).round(),
        }
    }
}

impl Default for FontSettings {
    fn default() -> Self {
        let code_font = CodeFont::default();
        Self {
            family: code_font.family.to_string(),
            size: code_font.size,
            line_height: 1.7,
            strokes: Strokes::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Strokes {
    Thin,
    #[default]
    Thick,
}

impl From<Strokes> for TextStrokes {
    fn from(strokes: Strokes) -> Self {
        match strokes {
            Strokes::Thin => TextStrokes::Thin,
            Strokes::Thick => TextStrokes::Thick,
        }
    }
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CursorSettings {
    pub height: CursorHeight,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CursorHeight {
    /// The whole row, line spacing included.
    #[default]
    Line,
    /// The font's ascent and descent, centered in the row.
    Font,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<UiSettings, String> {
        let settings: UiSettings = toml::from_str(text).map_err(|error| error.to_string())?;
        settings.check().map(|()| settings)
    }

    #[test]
    fn defaults_match_the_built_in_code_font() {
        assert_eq!(UiSettings::default().font.code_font(), CodeFont::default());
    }

    #[test]
    fn reads_every_setting() {
        let settings = parse(
            "[font]\nfamily = \"Menlo\"\nsize = 15\nline-height = 1.5\nstrokes = \"thin\"\n\
             [cursor]\nheight = \"font\"\n",
        )
        .unwrap();
        assert_eq!(settings.font.family, "Menlo");
        assert_eq!(settings.font.code_font().line_height, 23.0);
        assert_eq!(settings.font.strokes, Strokes::Thin);
        assert_eq!(settings.cursor.height, CursorHeight::Font);
    }

    #[test]
    fn rejects_sizes_out_of_range_and_unknown_values() {
        assert!(parse("[font]\nsize = 2\n").is_err());
        assert!(parse("[font]\nline-height = 0.5\n").is_err());
        assert!(parse("[cursor]\nheight = \"half\"\n").is_err());
    }
}
