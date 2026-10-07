use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{App, TextStrokes, WeakEntity, set_text_strokes};
use remacs_config::{Section, Settings, SettingsError, config_dir, home, watch};
use remacs_core::Editor;
use remacs_theme::font::{self, CodeFont};
use remacs_theme::glass;
use serde::Deserialize;

use crate::editor_view::EditorView;

const FONT_SIZES: RangeInclusive<f32> = 6.0..=72.0;
const LINE_HEIGHTS: RangeInclusive<f32> = 1.0..=3.0;
/// Milliseconds.
const DRAG_SCROLL_INTERVALS: RangeInclusive<u64> = 5..=1000;
/// Milliseconds; 0 turns the animation off.
const SCROLL_ANIMATION_DURATIONS: RangeInclusive<u64> = 0..=1000;
const GLASS_OPACITIES: RangeInclusive<f32> = 0.0..=1.0;
/// Pixels; the window server blurs no more than 64.
const GLASS_BLURS: RangeInclusive<u32> = 0..=64;
/// Changes this close together are read once, e.g. an editor's save done in steps.
const SETTLE_TIME: Duration = Duration::from_millis(100);

/// What the settings files hold, with the `ui` section read, and what is wrong with them.
pub struct ReadSettings {
    /// For the plugins.
    pub settings: Settings,
    pub ui: UiSettings,
    pub errors: Vec<SettingsError>,
}

/// Reads the settings and applies the process-wide part of `ui`; the view and the
/// plugins apply the rest.
pub fn read_settings() -> ReadSettings {
    let (settings, mut errors) = home()
        .map(|home| Settings::load(&config_dir(&home)))
        .unwrap_or_default();
    let ui = settings
        .section::<UiSettings>()
        .unwrap_or_else(|(error, defaults)| {
            errors.push(error);
            defaults
        });
    font::set_code(ui.font.code_font());
    set_text_strokes(ui.font.strokes.into());
    glass::set_window_opacity(ui.glass.opacity);
    ReadSettings {
        settings,
        ui,
        errors,
    }
}

/// Shows the first error; all go to stderr.
pub fn report(errors: &[SettingsError], editor: &mut Editor) {
    for error in errors {
        eprintln!("remacs: {error}");
    }
    if let Some(error) = errors.first() {
        editor.set_message(error.to_string());
    }
}

/// Applies the settings again whenever their files change, for as long as `view` lives.
pub fn watch_settings(view: WeakEntity<EditorView>, cx: &mut App) {
    let Some(home) = home() else {
        return;
    };
    let (changed, mut changes) = mpsc::unbounded();
    let watcher = match watch(&config_dir(&home), move || {
        let _ = changed.unbounded_send(());
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("remacs: settings are not watched: {error}");
            return;
        }
    };
    cx.spawn(async move |cx| {
        let _watcher = watcher;
        while changes.next().await.is_some() {
            cx.background_executor().timer(SETTLE_TIME).await;
            while changes.try_recv().is_ok() {}
            let read = read_settings();
            if view
                .update(cx, |view, cx| view.apply_settings(read, cx))
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

/// `ui.toml`.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct UiSettings {
    pub font: FontSettings,
    pub cursor: CursorSettings,
    pub mouse: MouseSettings,
    pub scroll: ScrollSettings,
    pub glass: GlassSettings,
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
        let interval = self.mouse.drag_scroll_interval;
        if !DRAG_SCROLL_INTERVALS.contains(&interval) {
            return Err(format!(
                "mouse.drag-scroll-interval {interval} is outside {}..={}",
                DRAG_SCROLL_INTERVALS.start(),
                DRAG_SCROLL_INTERVALS.end()
            ));
        }
        let duration = self.scroll.animation_duration;
        if !SCROLL_ANIMATION_DURATIONS.contains(&duration) {
            return Err(format!(
                "scroll.animation-duration {duration} is outside {}..={}",
                SCROLL_ANIMATION_DURATIONS.start(),
                SCROLL_ANIMATION_DURATIONS.end()
            ));
        }
        let glass = &self.glass;
        if !GLASS_OPACITIES.contains(&glass.opacity) {
            return Err(format!(
                "glass.opacity {} is outside {}..={}",
                glass.opacity,
                GLASS_OPACITIES.start(),
                GLASS_OPACITIES.end()
            ));
        }
        if !GLASS_BLURS.contains(&glass.blur) {
            return Err(format!(
                "glass.blur {} is outside {}..={}",
                glass.blur,
                GLASS_BLURS.start(),
                GLASS_BLURS.end()
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
    pub ligatures: bool,
}

impl FontSettings {
    pub fn code_font(&self) -> CodeFont {
        CodeFont {
            family: Arc::from(self.family.as_str()),
            size: self.size,
            // Whole pixels keep rows from blurring at their edges.
            line_height: (self.size * self.line_height).round(),
            ligatures: self.ligatures,
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
            ligatures: code_font.ligatures,
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct MouseSettings {
    /// Milliseconds between the lines scrolled while a selection is dragged past the
    /// text's top or bottom.
    pub drag_scroll_interval: u64,
}

impl MouseSettings {
    pub fn drag_scroll_interval(&self) -> Duration {
        Duration::from_millis(self.drag_scroll_interval)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ScrollSettings {
    /// Milliseconds the text takes to glide to where a command or a wheel notch scrolled
    /// it; 0 jumps.
    pub animation_duration: u64,
}

impl ScrollSettings {
    pub fn animation_duration(&self) -> Duration {
        Duration::from_millis(self.animation_duration)
    }
}

impl Default for ScrollSettings {
    fn default() -> Self {
        Self {
            animation_duration: 150,
        }
    }
}

impl Default for MouseSettings {
    fn default() -> Self {
        Self {
            drag_scroll_interval: 40,
        }
    }
}

/// The window's glass over the desktop.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct GlassSettings {
    /// Of the window's tint: 0.0 shows the desktop as it is, 1.0 hides it.
    pub opacity: f32,
    /// Pixels the desktop behind the window is blurred by; 0 leaves it sharp.
    pub blur: u32,
}

impl Default for GlassSettings {
    fn default() -> Self {
        Self {
            opacity: 0.8,
            blur: 60,
        }
    }
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
             ligatures = false\n\
             [cursor]\nheight = \"font\"\n[mouse]\ndrag-scroll-interval = 60\n\
             [scroll]\nanimation-duration = 0\n[glass]\nopacity = 0.5\nblur = 0\n",
        )
        .unwrap();
        assert_eq!(
            settings.glass,
            GlassSettings {
                opacity: 0.5,
                blur: 0
            }
        );
        assert_eq!(settings.mouse.drag_scroll_interval, 60);
        assert_eq!(settings.scroll.animation_duration, 0);
        assert_eq!(settings.font.family, "Menlo");
        assert_eq!(settings.font.code_font().line_height, 23.0);
        assert_eq!(settings.font.strokes, Strokes::Thin);
        assert!(!settings.font.code_font().ligatures);
        assert_eq!(settings.cursor.height, CursorHeight::Font);
    }

    #[test]
    fn rejects_sizes_out_of_range_and_unknown_values() {
        assert!(parse("[font]\nsize = 2\n").is_err());
        assert!(parse("[font]\nline-height = 0.5\n").is_err());
        assert!(parse("[cursor]\nheight = \"half\"\n").is_err());
        assert!(parse("[mouse]\ndrag-scroll-interval = 0\n").is_err());
        assert!(parse("[scroll]\nanimation-duration = 5000\n").is_err());
        assert!(parse("[glass]\nopacity = 1.5\n").is_err());
        assert!(parse("[glass]\nblur = 65\n").is_err());
    }
}
