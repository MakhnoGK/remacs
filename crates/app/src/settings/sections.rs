use std::sync::Arc;
use std::time::Duration;

use gpui::{TextStrokes, WindowBackgroundAppearance};
use remacs_theme::font::{self, CodeFont};
use remacs_theme::{Appearance, DEFAULT, LIGHT, SYSTEM, cursor};
use serde::Deserialize;

use super::ui::GLASS_OPACITIES;
use crate::transition::{Side, Transition};

/// Declares sections of settings files: their keys in kebab-case, unknown ones refused, and
/// a key left out the default given after its field.
macro_rules! settings_sections {
    ($(
        $(#[$meta:meta])*
        pub struct $name:ident {
            $($(#[$field_meta:meta])* pub $field:ident: $type:ty = $default:expr,)*
        }
    )*) => {$(
        $(#[$meta])*
        #[derive(Debug, PartialEq, serde::Deserialize)]
        #[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
        pub struct $name {
            $($(#[$field_meta])* pub $field: $type,)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $($field: $default,)*
                }
            }
        }
    )*};
}
pub(crate) use settings_sections;

settings_sections! {
    /// The look of the whole interface.
    pub struct ThemeSettings {
        /// A built-in theme, a file `<name>.toml` in the themes directory, or `system`.
        pub name: String = DEFAULT.to_owned(),
        /// Shown under `system` while macOS is light.
        pub light: String = LIGHT.to_owned(),
        /// Shown under `system` while macOS is dark.
        pub dark: String = DEFAULT.to_owned(),
    }

    /// The font of code.
    pub struct FontSettings {
        pub family: String = CodeFont::default().family.to_string(),
        pub size: f32 = CodeFont::default().size,
        /// Times the size.
        pub line_height: f32 = 1.7,
        pub weight: Weight = Weight::default(),
        pub strokes: Strokes = Strokes::default(),
        pub ligatures: bool = CodeFont::default().ligatures,
    }

    /// The font of the interface's own text: the mode line, menus, popups' prose, chips.
    pub struct UiFontSettings {
        pub family: String = font::DEFAULT_UI.to_owned(),
        pub size: f32 = font::DEFAULT_UI_SIZE,
        pub weight: Weight = Weight::default(),
    }

    /// How ⌘= and ⌘-, or ⌘ with the wheel, size the window buffer's text.
    #[derive(Clone, Copy)]
    pub struct ZoomSettings {
        /// Points a step changes the size by.
        pub step: f32 = 1.0,
        pub min_size: f32 = 8.0,
        pub max_size: f32 = 36.0,
        /// Pixels a trackpad or a smooth-scrolling wheel moves with ⌘ held for a step; a
        /// notch of a plain wheel is one.
        pub wheel_step: f32 = 100.0,
    }

    pub struct CursorSettings {
        pub height: CursorHeight = CursorHeight::default(),
        /// Pixels, of the block's corners in text, tables, prompts and terminals.
        pub radius: f32 = cursor::DEFAULT_RADIUS,
    }

    #[derive(Clone, Copy, Eq)]
    pub struct KeyboardSettings {
        pub meta: MetaOption = MetaOption::default(),
    }

    #[derive(Clone, Copy, Eq)]
    pub struct MouseSettings {
        /// Milliseconds between the lines scrolled while a selection is dragged past the
        /// text's top or bottom.
        pub drag_scroll_interval: u64 = 40,
    }

    #[derive(Clone, Copy, Eq)]
    pub struct ScrollSettings {
        /// Milliseconds the text takes to glide to where a command or a wheel notch scrolled
        /// it; 0 jumps.
        pub animation_duration: u64 = 150,
        /// Lines or rows a mouse wheel's notch scrolls, or columns' worth sideways.
        pub lines_per_notch: usize = 3,
    }

    /// The window's glass over the desktop.
    #[derive(Clone, Copy)]
    pub struct GlassSettings {
        /// Of the window's tint: 0.0 shows the desktop as it is, 1.0 hides it. Left out, the
        /// theme's, filled in once the theme is read.
        pub opacity: Option<f32> = None,
        /// Pixels the desktop behind the window is blurred by; 0 leaves it sharp and the
        /// surfaces over the editor unblurred.
        pub blur: u32 = 60,
    }

    /// The echo area's messages.
    #[derive(Clone, Copy, Eq)]
    pub struct EchoSettings {
        /// Of a message opened whole from its pill; more scroll.
        pub lines: usize = 12,
        /// Messages kept in `*Messages*`.
        pub history: usize = 1000,
        /// Milliseconds an info or done message stays before it goes; 0 keeps it. Errors
        /// stay.
        pub hide_after: u64 = 4000,
    }

    /// The keys of the view, under it.
    #[derive(Clone, Copy)]
    pub struct HintsSettings {
        pub show: bool = true,
        /// Pixels kept on each side of the keys for the echo area and the status; keys that
        /// do not fit between go under a `+N` chip.
        pub side_room: f32 = 280.0,
    }

    /// The keys that may follow a prefix typed, listed after a pause, like Emacs `which-key`.
    #[derive(Clone, Copy, Eq)]
    pub struct PrefixKeysSettings {
        pub show: bool = true,
        /// Milliseconds after the prefix.
        pub delay: u64 = 600,
        pub columns: usize = 4,
    }

    /// The documentation of a symbol, alone or beside completions: it grows with its code up
    /// to `max_width`, wrapping the code past it, and scrolls past `max_height`.
    #[derive(Clone, Copy)]
    pub struct DocumentationSettings {
        /// Pixels.
        pub max_width: f32 = 760.0,
        /// Pixels.
        pub max_height: f32 = 480.0,
    }

    /// The problems at the cursor, shown by the text: their messages wrap at `max_width`, and
    /// they scroll past `max_height`.
    #[derive(Clone, Copy)]
    pub struct ProblemsSettings {
        /// Pixels.
        pub max_width: f32 = 560.0,
        /// Pixels.
        pub max_height: f32 = 360.0,
    }

    /// A diff drawn as text, like a commit's: the column of files beside it and the bands of
    /// its added and removed lines.
    #[derive(Clone, Copy)]
    pub struct DiffSettings {
        /// Pixels.
        pub side_width: f32 = 480.0,
        /// Percent.
        pub band_opacity: u8 = 15,
    }

    /// The gutter's controls of blocks that can be hidden.
    #[derive(Clone, Copy, Eq)]
    pub struct FoldsSettings {
        pub toggles: FoldToggles = FoldToggles::default(),
    }

    /// What a prompt's input keeps in view as it scrolls sideways, and where the directory it
    /// acts in goes.
    #[derive(Clone, Copy)]
    pub struct PromptsSettings {
        /// Pixels kept in view on each side of the cursor, where there is text to show.
        pub input_margin: f32 = 64.0,
        pub directory: DirectoryPosition = DirectoryPosition::default(),
    }

    /// How the colors that text names, as language servers find them, are shown: under the
    /// text in their color, and in place of a completion's kind.
    #[derive(Clone, Copy)]
    pub struct SwatchesSettings {
        pub inline: bool = true,
        /// Pixels, of the line under the text.
        pub thickness: f32 = 3.0,
        pub completion: bool = true,
    }

    /// How the pane under the text, prompts and popups come and go: those by the text, and
    /// the cards over the bottom row, like the keys after a prefix.
    #[derive(Clone, Copy)]
    pub struct TransitionsSettings {
        /// Milliseconds; 0 shows and hides it at once.
        pub pane_duration: u64 = 120,
        /// Pixels it rises by as it comes.
        pub pane_slide: f32 = 16.0,
        pub prompt_duration: u64 = 110,
        /// Pixels it comes down by, or up by from the status pill.
        pub prompt_slide: f32 = 8.0,
        pub popup_duration: u64 = 100,
        /// Pixels it comes down by, or up by from the bottom row.
        pub popup_slide: f32 = 4.0,
    }
}

impl ThemeSettings {
    pub fn follows_system(&self) -> bool {
        self.name == SYSTEM
    }

    /// The theme shown while macOS is `system`.
    pub fn shown(&self, system: Appearance) -> &str {
        match (self.follows_system(), system) {
            (false, _) => &self.name,
            (true, Appearance::Light) => &self.light,
            (true, Appearance::Dark) => &self.dark,
        }
    }
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

impl Default for Weight {
    fn default() -> Self {
        Weight::Named(WeightName::Regular)
    }
}

impl Weight {
    pub fn value(self) -> f32 {
        match self {
            Weight::Number(weight) => weight,
            Weight::Named(name) => f32::from(name as u16),
        }
    }
}

/// Each name's number is its weight.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WeightName {
    Thin = 100,
    ExtraLight = 200,
    Light = 300,
    Regular = 400,
    Medium = 500,
    Semibold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CursorHeight {
    /// The whole row, line spacing included.
    #[default]
    Line,
    /// The font's ascent and descent, centered in the row.
    Font,
}

/// The Option keys that are Meta; the others type what the keyboard layout gives them,
/// like `@` or a dead key's accent.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetaOption {
    #[default]
    Both,
    Left,
    Right,
}

impl MouseSettings {
    pub fn drag_scroll_interval(&self) -> Duration {
        Duration::from_millis(self.drag_scroll_interval)
    }
}

impl ScrollSettings {
    pub fn animation_duration(&self) -> Duration {
        Duration::from_millis(self.animation_duration)
    }
}

impl GlassSettings {
    /// Opaque when the tint hides the desktop, so the window server neither blends nor
    /// blurs what is behind the window.
    pub fn window_background(&self) -> WindowBackgroundAppearance {
        if self.opacity.unwrap_or(*GLASS_OPACITIES.end()) >= *GLASS_OPACITIES.end() {
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

/// What the row along the bottom of the window shows, from three sections.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BottomSettings {
    pub echo: EchoSettings,
    pub hints: HintsSettings,
    pub prefix_keys: PrefixKeysSettings,
}

impl EchoSettings {
    pub fn hide_after(&self) -> Option<Duration> {
        (self.hide_after > 0).then(|| Duration::from_millis(self.hide_after))
    }
}

impl PrefixKeysSettings {
    pub fn delay(&self) -> Duration {
        Duration::from_millis(self.delay)
    }
}

/// When a shown block's toggle is in the gutter; a hidden block's always is.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FoldToggles {
    /// While the mouse is over the gutter, and on point's line.
    #[default]
    Hover,
    Always,
}

/// Where a prompt's directory line goes, beside its input.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DirectoryPosition {
    #[default]
    Above,
    Below,
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
