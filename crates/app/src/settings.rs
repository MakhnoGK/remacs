use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, FontWeight, SharedString, TextStrokes, WeakEntity, WindowBackgroundAppearance,
    set_family_weight, set_text_strokes,
};
use remacs_config::{Section, Settings, SettingsError, config_dir, home, watch};
use remacs_core::Editor;
use remacs_theme::font::{self, CodeFont};
use remacs_theme::{cursor, glass};
use serde::Deserialize;

use crate::editor_view::EditorView;
use crate::transition::{Side, Transition};

const FONT_SIZES: RangeInclusive<f32> = 6.0..=72.0;
const LINE_HEIGHTS: RangeInclusive<f32> = 1.0..=3.0;
const FONT_WEIGHTS: RangeInclusive<f32> = 100.0..=900.0;
/// Pixels; past half a narrow font's character the block is a pill.
const CURSOR_RADII: RangeInclusive<f32> = 0.0..=4.0;
/// Milliseconds.
const DRAG_SCROLL_INTERVALS: RangeInclusive<u64> = 5..=1000;
/// Milliseconds; 0 turns the animation off.
const SCROLL_ANIMATION_DURATIONS: RangeInclusive<u64> = 0..=1000;
const GLASS_OPACITIES: RangeInclusive<f32> = 0.0..=1.0;
/// Pixels; the window server blurs no more than 64.
const GLASS_BLURS: RangeInclusive<u32> = 0..=64;
const ECHO_LINES: RangeInclusive<usize> = 3..=40;
const ECHO_HISTORIES: RangeInclusive<usize> = 10..=100_000;
/// Pixels.
const HINTS_SIDE_ROOMS: RangeInclusive<f32> = 0.0..=800.0;
/// Milliseconds.
const PREFIX_KEYS_DELAYS: RangeInclusive<u64> = 0..=5000;
const PREFIX_KEYS_COLUMNS: RangeInclusive<usize> = 1..=8;
/// Pixels.
const DOCUMENTATION_MAX_WIDTHS: RangeInclusive<f32> = 240.0..=2000.0;
/// Pixels.
const DOCUMENTATION_MAX_HEIGHTS: RangeInclusive<f32> = 120.0..=2000.0;
/// Milliseconds; past 100 a surface would feel slow to come.
const TRANSITION_DURATIONS: RangeInclusive<u64> = 0..=100;
/// Pixels.
const TRANSITION_SLIDES: RangeInclusive<f32> = 0.0..=32.0;
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
    let code_font = ui.font.code_font();
    set_family_weight(Some((
        SharedString::from(Arc::clone(&code_font.family)),
        FontWeight(code_font.weight),
    )));
    font::set_code(code_font);
    set_text_strokes(ui.font.strokes.into());
    glass::set_window_opacity(ui.glass.opacity);
    glass::set_blurs(ui.glass.blur > 0);
    cursor::set_radius(ui.cursor.radius);
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
        editor.set_error(error.to_string());
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
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct UiSettings {
    pub font: FontSettings,
    pub cursor: CursorSettings,
    pub mouse: MouseSettings,
    pub scroll: ScrollSettings,
    pub glass: GlassSettings,
    pub echo: EchoSettings,
    pub hints: HintsSettings,
    pub prefix_keys: PrefixKeysSettings,
    pub documentation: DocumentationSettings,
    pub transitions: TransitionsSettings,
}

impl UiSettings {
    pub fn bottom(&self) -> BottomSettings {
        BottomSettings {
            echo: self.echo,
            hints: self.hints,
            prefix_keys: self.prefix_keys,
        }
    }
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
        check_range("font.weight", font.weight.value(), &FONT_WEIGHTS)?;
        check_range("cursor.radius", self.cursor.radius, &CURSOR_RADII)?;
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
        check_range("echo.lines", self.echo.lines, &ECHO_LINES)?;
        check_range("echo.history", self.echo.history, &ECHO_HISTORIES)?;
        check_range("hints.side-room", self.hints.side_room, &HINTS_SIDE_ROOMS)?;
        let prefix_keys = &self.prefix_keys;
        check_range("prefix-keys.delay", prefix_keys.delay, &PREFIX_KEYS_DELAYS)?;
        check_range(
            "prefix-keys.columns",
            prefix_keys.columns,
            &PREFIX_KEYS_COLUMNS,
        )?;
        let documentation = &self.documentation;
        check_range(
            "documentation.max-width",
            documentation.max_width,
            &DOCUMENTATION_MAX_WIDTHS,
        )?;
        check_range(
            "documentation.max-height",
            documentation.max_height,
            &DOCUMENTATION_MAX_HEIGHTS,
        )?;
        let transitions = &self.transitions;
        for (kind, duration, slide) in [
            ("pane", transitions.pane_duration, transitions.pane_slide),
            (
                "prompt",
                transitions.prompt_duration,
                transitions.prompt_slide,
            ),
            ("popup", transitions.popup_duration, transitions.popup_slide),
        ] {
            let name = |setting| format!("transitions.{kind}-{setting}");
            check_range(&name("duration"), duration, &TRANSITION_DURATIONS)?;
            check_range(&name("slide"), slide, &TRANSITION_SLIDES)?;
        }
        Ok(())
    }
}

fn check_range<T: PartialOrd + std::fmt::Display>(
    name: &str,
    value: T,
    range: &RangeInclusive<T>,
) -> Result<(), String> {
    if range.contains(&value) {
        return Ok(());
    }
    Err(format!(
        "{name} {value} is outside {}..={}",
        range.start(),
        range.end()
    ))
}

/// The font of code.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct FontSettings {
    pub family: String,
    pub size: f32,
    /// Times the size.
    pub line_height: f32,
    pub weight: Weight,
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
            weight: self.weight.value(),
            ligatures: self.ligatures,
        }
    }
}

/// A font's weight by name, or by number from 100 to 900.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Weight {
    Named(WeightName),
    Number(f32),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WeightName {
    Thin,
    ExtraLight,
    Light,
    Regular,
    Medium,
    Semibold,
    Bold,
    ExtraBold,
    Black,
}

impl Weight {
    pub fn value(self) -> f32 {
        let name = match self {
            Weight::Number(weight) => return weight,
            Weight::Named(name) => name,
        };
        match name {
            WeightName::Thin => 100.0,
            WeightName::ExtraLight => 200.0,
            WeightName::Light => 300.0,
            WeightName::Regular => 400.0,
            WeightName::Medium => 500.0,
            WeightName::Semibold => 600.0,
            WeightName::Bold => 700.0,
            WeightName::ExtraBold => 800.0,
            WeightName::Black => 900.0,
        }
    }
}

impl Default for Weight {
    fn default() -> Self {
        Weight::Named(WeightName::Regular)
    }
}

impl Default for FontSettings {
    fn default() -> Self {
        let code_font = CodeFont::default();
        Self {
            family: code_font.family.to_string(),
            size: code_font.size,
            line_height: 1.7,
            weight: Weight::default(),
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

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CursorSettings {
    pub height: CursorHeight,
    /// Pixels, of the block's corners in text, tables, prompts and terminals.
    pub radius: f32,
}

impl Default for CursorSettings {
    fn default() -> Self {
        Self {
            height: CursorHeight::default(),
            radius: cursor::DEFAULT_RADIUS,
        }
    }
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
    /// Pixels the desktop behind the window is blurred by; 0 leaves it sharp and the
    /// surfaces over the editor unblurred.
    pub blur: u32,
}

impl GlassSettings {
    /// Opaque when the tint hides the desktop, so the window server neither blends nor
    /// blurs what is behind the window.
    pub fn window_background(&self) -> WindowBackgroundAppearance {
        if self.opacity >= *GLASS_OPACITIES.end() {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Transparent
        }
    }

    /// Of the desktop behind the window.
    pub fn window_blur(&self) -> u32 {
        match self.window_background() {
            WindowBackgroundAppearance::Opaque => 0,
            _ => self.blur,
        }
    }
}

impl Default for GlassSettings {
    fn default() -> Self {
        Self {
            opacity: 0.8,
            blur: 60,
        }
    }
}

/// What the row along the bottom of the window shows, from three sections.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BottomSettings {
    pub echo: EchoSettings,
    pub hints: HintsSettings,
    pub prefix_keys: PrefixKeysSettings,
}

/// The echo area's messages.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct EchoSettings {
    /// Of a message opened whole from its pill; more scroll.
    pub lines: usize,
    /// Messages kept in `*Messages*`.
    pub history: usize,
}

impl Default for EchoSettings {
    fn default() -> Self {
        Self {
            lines: 12,
            history: 1000,
        }
    }
}

/// The keys of the view, under it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct HintsSettings {
    pub show: bool,
    /// Pixels kept on each side of the keys for the echo area and the status; keys that
    /// do not fit between go under a `+N` chip.
    pub side_room: f32,
}

impl Default for HintsSettings {
    fn default() -> Self {
        Self {
            show: true,
            side_room: 280.0,
        }
    }
}

/// The keys that may follow a prefix typed, listed after a pause, like Emacs `which-key`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PrefixKeysSettings {
    pub show: bool,
    /// Milliseconds after the prefix.
    pub delay: u64,
    pub columns: usize,
}

impl PrefixKeysSettings {
    pub fn delay(&self) -> Duration {
        Duration::from_millis(self.delay)
    }
}

impl Default for PrefixKeysSettings {
    fn default() -> Self {
        Self {
            show: true,
            delay: 600,
            columns: 4,
        }
    }
}

/// The documentation of a symbol, alone or beside completions: it grows with its code up
/// to `max_width`, wrapping the code past it, and scrolls past `max_height`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct DocumentationSettings {
    /// Pixels.
    pub max_width: f32,
    /// Pixels.
    pub max_height: f32,
}

impl Default for DocumentationSettings {
    fn default() -> Self {
        Self {
            max_width: 760.0,
            max_height: 480.0,
        }
    }
}

/// How the pane under the text, prompts and the popups by the text come and go.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TransitionsSettings {
    /// Milliseconds; 0 shows and hides it at once.
    pub pane_duration: u64,
    /// Pixels it rises by as it comes.
    pub pane_slide: f32,
    pub prompt_duration: u64,
    /// Pixels it comes down by, or up by from the status pill.
    pub prompt_slide: f32,
    pub popup_duration: u64,
    /// Pixels it comes down by.
    pub popup_slide: f32,
}

impl TransitionsSettings {
    pub fn pane(&self) -> Transition {
        transition(self.pane_duration, self.pane_slide, Side::Below)
    }

    pub fn prompt(&self) -> Transition {
        transition(self.prompt_duration, self.prompt_slide, Side::Above)
    }

    pub fn popup(&self) -> Transition {
        transition(self.popup_duration, self.popup_slide, Side::Above)
    }
}

fn transition(duration: u64, slide: f32, from: Side) -> Transition {
    Transition {
        duration: Duration::from_millis(duration),
        slide,
        from,
    }
}

impl Default for TransitionsSettings {
    fn default() -> Self {
        Self {
            pane_duration: 90,
            pane_slide: 16.0,
            prompt_duration: 80,
            prompt_slide: 8.0,
            popup_duration: 60,
            popup_slide: 0.0,
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
            "[font]\nfamily = \"Menlo\"\nsize = 15\nline-height = 1.5\nweight = \"light\"\n\
             strokes = \"thin\"\n\
             ligatures = false\n\
             [cursor]\nheight = \"font\"\nradius = 2.5\n[mouse]\ndrag-scroll-interval = 60\n\
             [scroll]\nanimation-duration = 0\n[glass]\nopacity = 0.5\nblur = 0\n\
             [echo]\nlines = 6\nhistory = 50\n[hints]\nshow = false\nside-room = 200\n\
             [prefix-keys]\nshow = false\ndelay = 0\ncolumns = 3\n\
             [documentation]\nmax-width = 900\nmax-height = 300\n\
             [transitions]\npane-duration = 0\npane-slide = 0\nprompt-duration = 100\n\
             prompt-slide = 12\npopup-duration = 40\npopup-slide = 4\n",
        )
        .unwrap();
        assert_eq!(settings.transitions.pane().duration, Duration::ZERO);
        assert_eq!(
            settings.transitions.prompt(),
            Transition {
                duration: Duration::from_millis(100),
                slide: 12.0,
                from: Side::Above
            }
        );
        assert_eq!(settings.transitions.popup().slide, 4.0);
        assert_eq!(
            settings.documentation,
            DocumentationSettings {
                max_width: 900.0,
                max_height: 300.0
            }
        );
        assert_eq!(
            settings.bottom(),
            BottomSettings {
                echo: EchoSettings {
                    lines: 6,
                    history: 50
                },
                hints: HintsSettings {
                    show: false,
                    side_room: 200.0
                },
                prefix_keys: PrefixKeysSettings {
                    show: false,
                    delay: 0,
                    columns: 3
                },
            }
        );
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
        assert_eq!(settings.font.code_font().weight, 300.0);
        assert_eq!(settings.font.strokes, Strokes::Thin);
        assert!(!settings.font.code_font().ligatures);
        assert_eq!(settings.cursor.height, CursorHeight::Font);
        assert_eq!(settings.cursor.radius, 2.5);
    }

    #[test]
    fn an_opaque_tint_makes_the_window_opaque_and_unblurred() {
        let glass = |opacity| GlassSettings { opacity, blur: 30 };
        assert_eq!(
            glass(1.0).window_background(),
            WindowBackgroundAppearance::Opaque
        );
        assert_eq!(glass(1.0).window_blur(), 0);
        assert_eq!(
            glass(0.9).window_background(),
            WindowBackgroundAppearance::Transparent
        );
        assert_eq!(glass(0.9).window_blur(), 30);
    }

    #[test]
    fn rejects_sizes_out_of_range_and_unknown_values() {
        assert!(parse("[font]\nsize = 2\n").is_err());
        assert!(parse("[font]\nline-height = 0.5\n").is_err());
        assert!(parse("[font]\nweight = 950\n").is_err());
        assert!(parse("[font]\nweight = \"heavy\"\n").is_err());
        assert_eq!(
            parse("[font]\nweight = 550\n").unwrap().font.weight.value(),
            550.0
        );
        assert_eq!(
            parse("[font]\nweight = \"extra-bold\"\n")
                .unwrap()
                .font
                .weight
                .value(),
            800.0
        );
        assert!(parse("[cursor]\nheight = \"half\"\n").is_err());
        assert!(parse("[cursor]\nradius = 5\n").is_err());
        assert!(parse("[mouse]\ndrag-scroll-interval = 0\n").is_err());
        assert!(parse("[scroll]\nanimation-duration = 5000\n").is_err());
        assert!(parse("[glass]\nopacity = 1.5\n").is_err());
        assert!(parse("[glass]\nblur = 65\n").is_err());
        assert!(parse("[echo]\nlines = 1\n").is_err());
        assert!(parse("[hints]\nside-room = -1\n").is_err());
        assert!(parse("[prefix-keys]\ncolumns = 0\n").is_err());
        assert!(parse("[documentation]\nmax-width = 100\n").is_err());
        assert!(parse("[documentation]\nmax-height = 5000\n").is_err());
        assert!(parse("[transitions]\npane-duration = 101\n").is_err());
        assert!(parse("[transitions]\npopup-slide = -1\n").is_err());
        assert!(parse("[transitions]\nduration = 50\n").is_err());
    }
}
