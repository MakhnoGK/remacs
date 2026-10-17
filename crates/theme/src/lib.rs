//! Design tokens, free of any UI dependency, read from the theme put in place. Colors are
//! `0xRRGGBB`, or `0xRRGGBBAA` where they are translucent.

mod theme;

use std::sync::Once;

use serde::de::{Deserialize, Deserializer, Error};

pub use theme::{Appearance, BUILT_IN, DEFAULT, Theme, ThemeError, appearance, install, names};

/// A table of a theme whose values are read while drawing. They live in atomics: reading
/// one is a load, and a theme can be put in place from any thread.
macro_rules! tokens {
    (
        $(#[$table_doc:meta])*
        pub struct $Table:ident: $Value:ty => $Read:ty {
            $($(#[$doc:meta])* $token:ident,)*
        }
    ) => {
        $(#[$table_doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "kebab-case")]
        pub struct $Table {
            $(pub $token: $Value,)*
        }

        #[allow(non_camel_case_types)]
        enum Slot {
            $($token,)*
        }

        const SLOTS: usize = [$(Slot::$token as usize,)*].len();

        static VALUES: [std::sync::atomic::AtomicU32; SLOTS] =
            [const { std::sync::atomic::AtomicU32::new(0) }; SLOTS];

        impl $Table {
            pub(crate) fn put_in_place(&self) {
                $(
                    VALUES[Slot::$token as usize].store(
                        crate::Token::bits(self.$token),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                )*
            }
        }

        $(
            $(#[$doc])*
            pub fn $token() -> $Read {
                crate::ensure_theme();
                <$Value as crate::Token>::read(
                    VALUES[Slot::$token as usize].load(std::sync::atomic::Ordering::Relaxed),
                )
            }
        )*
    };
}

/// What a token's value is stored as.
trait Token: Copy {
    type Read;

    fn bits(self) -> u32;
    fn read(bits: u32) -> Self::Read;
}

static DEFAULT_THEME: Once = Once::new();

/// Puts the default theme in place before the first token is read, so nothing is drawn
/// black before the settings are.
fn ensure_theme() {
    DEFAULT_THEME.call_once(|| Theme::built_in(DEFAULT).put_in_place());
}

/// `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u32);

/// `#rrggbbaa`, or `#rrggbb` for an opaque one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u32);

impl Token for Rgb {
    type Read = u32;

    fn bits(self) -> u32 {
        self.0
    }

    fn read(bits: u32) -> u32 {
        bits
    }
}

impl Token for Rgba {
    type Read = u32;

    fn bits(self) -> u32 {
        self.0
    }

    fn read(bits: u32) -> u32 {
        bits
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text, false).map(Rgb).map_err(D::Error::custom)
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text, true).map(Rgba).map_err(D::Error::custom)
    }
}

fn parse_hex(text: &str, alpha: bool) -> Result<u32, String> {
    let digits = text
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6 || alpha && digits.len() == 8)
        .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let Some(digits) = digits else {
        let forms = if alpha {
            "#rrggbb or #rrggbbaa"
        } else {
            "#rrggbb"
        };
        return Err(format!("{text:?} is not a color: {forms}"));
    };
    let value = u32::from_str_radix(digits, 16).map_err(|error| error.to_string())?;
    Ok(if alpha && digits.len() == 6 {
        value << 8 | 0xff
    } else {
        value
    })
}

pub mod color {
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::Rgb;

    tokens! {
        /// The theme's opaque colors.
        pub struct Colors: Rgb => u32 {
            /// Under everything in the window, as its tint over the desktop.
            bg_main,
            bg_popup,
            bg_kbd_selected,

            fg_main,
            fg_body,
            fg_kbd,
            fg_alt,
            fg_dim,
            fg_faint,
            fg_placeholder,
            fg_line_number,
            fg_muted,
            fg_directory,

            accent,
            accent_on_selection,

            error,
            /// Text about something to be destroyed, e.g. the flagged count.
            danger,
            /// Names of files flagged for deletion.
            fg_flagged,
            bg_danger,
            bg_danger_kbd,
            fg_on_danger,
            warning,
            /// Text on a warning tint, e.g. the editing notice.
            fg_on_warning,
            info,
            /// Notes about the text at point, e.g. who reported a problem, on the note tint.
            fg_note_source,
            fg_note_code,

            fg_added,
            fg_removed,

            /// Version control: new files and added lines.
            vc_added,
            vc_modified,
            /// Deleted files and removed lines.
            vc_removed,
            vc_added_on_selection,
            vc_removed_on_selection,

            /// Branch and tag chips: the one checked out, the main one, others, remote ones.
            fg_ref_head,
            fg_ref_main,
            fg_ref_branch,
            fg_ref_remote,

            /// Links in a program's output, e.g. a `file:line`.
            link,

            keyword,
            type_name,
            function,
            constant,
            builtin,
            docstring,
            string,
            comment,
        }
    }

    /// Lines of the commit graph: the checked out branch's, its base's, then the others'
    /// in turn.
    pub fn graph(index: usize) -> u32 {
        let colors = [
            accent(),
            keyword(),
            type_name(),
            function(),
            warning(),
            info(),
            builtin(),
            string(),
        ];
        match index {
            0 | 1 => colors[index],
            _ => colors[2 + (index - 2) % (colors.len() - 2)],
        }
    }

    pub const ANSI_COLORS: usize = 16;

    static ANSI: [AtomicU32; ANSI_COLORS] = [const { AtomicU32::new(0) }; ANSI_COLORS];

    /// One of the 16 colors programs name with ANSI escape codes, normal then bright.
    pub fn ansi(index: usize) -> u32 {
        crate::ensure_theme();
        ANSI[index].load(Ordering::Relaxed)
    }

    pub(crate) fn put_ansi_in_place(colors: &[Rgb; ANSI_COLORS]) {
        for (slot, color) in ANSI.iter().zip(colors) {
            slot.store(color.0, Ordering::Relaxed);
        }
    }
}

/// How the syntax colors of `color` are drawn besides their color.
pub mod syntax {
    use serde::Deserialize;

    use crate::Token;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Style {
        pub bold: bool,
        pub italic: bool,
        pub underline: bool,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Trait {
        Bold,
        Italic,
        Underline,
    }

    /// A list of `"bold"`, `"italic"` and `"underline"`, any of them.
    impl<'de> Deserialize<'de> for Style {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let mut style = Style::default();
            for one in Vec::<Trait>::deserialize(deserializer)? {
                match one {
                    Trait::Bold => style.bold = true,
                    Trait::Italic => style.italic = true,
                    Trait::Underline => style.underline = true,
                }
            }
            Ok(style)
        }
    }

    const BOLD: u32 = 1;
    const ITALIC: u32 = 2;
    const UNDERLINE: u32 = 4;

    impl Token for Style {
        type Read = Style;

        fn bits(self) -> u32 {
            [
                (self.bold, BOLD),
                (self.italic, ITALIC),
                (self.underline, UNDERLINE),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .fold(0, |bits, (_, bit)| bits | bit)
        }

        fn read(bits: u32) -> Style {
            Style {
                bold: bits & BOLD != 0,
                italic: bits & ITALIC != 0,
                underline: bits & UNDERLINE != 0,
            }
        }
    }

    tokens! {
        /// Of the syntax colors of the same names.
        pub struct Styles: Style => Style {
            keyword,
            type_name,
            function,
            constant,
            builtin,
            docstring,
            string,
            comment,
        }
    }
}

/// Translucent surfaces over the blurred desktop or editor.
pub mod glass {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

    use serde::Deserialize;

    use crate::{Rgb, Rgba};

    tokens! {
        /// The theme's translucent colors.
        pub struct Glass: Rgba => u32 {
            /// The row of the cursor.
            line_highlight,
            /// The text between mark and point.
            region,

            /// Gutter marks of lines changed since the last commit.
            vc_added_mark,
            vc_modified_mark,
            vc_removed_mark,

            /// Floating surfaces, e.g. prompts and completion, over the blurred editor.
            surface,
            surface_border,

            /// Notes at point, e.g. problems: a surface tinted like info.
            note,
            note_border,
            note_separator,
            note_code,

            /// Dims the editor under a prompt.
            scrim,

            /// Questions that must be answered, e.g. confirming a deletion: thicker glass,
            /// over a darker scrim that also blurs the editor a little.
            modal,
            modal_border,
            modal_scrim,

            /// The status pill, floating over the bottom of the window.
            pill,
            pill_border,

            /// Panes in a buffer, e.g. the git status sections; not blurred, on the
            /// window's glass.
            pane,
            pane_border,
            /// Code shown in a pane or surface.
            well,
            well_border,
            /// A card over code in a well, e.g. a conflict's sides: surface glass, blurred
            /// less.
            card_border,
            /// The strip under a card's content, holding its buttons.
            card_footer,
            /// Between the parts of a card.
            divider,
            /// A key cap on a button, sunk into it.
            key_on_control,
            added,
            removed,

            /// Sunk into a surface: fields, code.
            field,
            /// Raised from a surface: buttons and key caps.
            control,
            control_border,
            /// A label in a field.
            chip,
            /// The selected row of a surface.
            selection,
            /// The selected segment of a segmented control.
            segment,
            /// A control that cannot be used now.
            disabled,
        }
    }

    /// A color drawn with the alphas the interface asks for, made stronger or weaker.
    #[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Ink {
        pub color: Rgb,
        /// Times the alpha asked for, up to opaque.
        pub strength: f32,
    }

    impl Ink {
        fn bits(self) -> u64 {
            u64::from(self.color.0) << 32 | u64::from(self.strength.to_bits())
        }
    }

    static INK: AtomicU64 = AtomicU64::new(0);
    static SHADOW: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn put_inks_in_place(ink: Ink, shadow: Ink) {
        INK.store(ink.bits(), Ordering::Relaxed);
        SHADOW.store(shadow.bits(), Ordering::Relaxed);
    }

    fn with_alpha(ink: &AtomicU64, alpha: u32) -> u32 {
        crate::ensure_theme();
        let bits = ink.load(Ordering::Relaxed);
        let strength = f32::from_bits(bits as u32);
        let alpha = (alpha as f32 * strength).round().min(255.0) as u32;
        ((bits >> 32) as u32) << 8 | alpha
    }

    /// Lines, hovers and tints over the glass: white on a dark theme, black on a light one.
    pub fn ink(alpha: u32) -> u32 {
        with_alpha(&INK, alpha)
    }

    /// Shadows under raised things.
    pub fn shadow(alpha: u32) -> u32 {
        with_alpha(&SHADOW, alpha)
    }

    static BLURS: AtomicBool = AtomicBool::new(true);

    /// Whether surfaces blur what is under them; off, they skip the blur's render pass.
    pub fn blurs() -> bool {
        BLURS.load(Ordering::Relaxed)
    }

    pub fn set_blurs(blurs: bool) {
        BLURS.store(blurs, Ordering::Relaxed);
    }

    static WINDOW_ALPHA: AtomicU32 = AtomicU32::new(0xcc);

    /// Over the blurred desktop, under everything in the window: the theme's background,
    /// its opacity set from the config.
    pub fn window_tint() -> u32 {
        super::color::bg_main() << 8 | WINDOW_ALPHA.load(Ordering::Relaxed)
    }

    /// From 0.0, the desktop as it is, to 1.0, the tint alone.
    pub fn set_window_opacity(opacity: f32) {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        WINDOW_ALPHA.store(alpha, Ordering::Relaxed);
    }

    pub const SURFACE_BLUR: f32 = 30.0;
    pub const SURFACE_SATURATION: f32 = 1.5;
    pub const SURFACE_RADIUS: f32 = 14.0;
    /// Surfaces at point, e.g. completion and documentation.
    pub const HINT_RADIUS: f32 = 12.0;
    pub const MODAL_BLUR: f32 = 40.0;
    pub const MODAL_SCRIM_BLUR: f32 = 3.0;
    pub const PILL_BLUR: f32 = 20.0;
    pub const CARD_BLUR: f32 = 20.0;
    pub const CARD_SATURATION: f32 = 1.4;
}

/// The block cursor, wherever text has one.
pub mod cursor {
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Pixels.
    pub const DEFAULT_RADIUS: f32 = 1.0;
    /// Pixels, as `f32` bits; set from the config.
    static RADIUS: AtomicU32 = AtomicU32::new(DEFAULT_RADIUS.to_bits());

    /// Of the block's corners.
    pub fn radius() -> f32 {
        f32::from_bits(RADIUS.load(Ordering::Relaxed))
    }

    pub fn set_radius(radius: f32) {
        RADIUS.store(radius.to_bits(), Ordering::Relaxed);
    }
}

pub mod font {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, LazyLock, PoisonError, RwLock};

    pub const DEFAULT_UI: &str = "SF Pro Text";
    pub const DEFAULT_UI_SIZE: f32 = 13.0;
    /// Of text on surfaces.
    pub const LINE_HEIGHT: f32 = 22.0;

    static UI: LazyLock<RwLock<Arc<str>>> = LazyLock::new(|| RwLock::new(Arc::from(DEFAULT_UI)));
    /// As `f32` bits; set from the config.
    static UI_SIZE: AtomicU32 = AtomicU32::new(DEFAULT_UI_SIZE.to_bits());

    /// The family of the interface's own text, set from the config.
    pub fn ui() -> Arc<str> {
        Arc::clone(&UI.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Of the interface's text, set from the config; the other sizes follow it.
    pub fn ui_size() -> f32 {
        f32::from_bits(UI_SIZE.load(Ordering::Relaxed))
    }

    pub fn status_size() -> f32 {
        ui_size() - 1.0
    }

    pub fn input_size() -> f32 {
        ui_size() + 1.0
    }

    pub fn small_size() -> f32 {
        ui_size() - 2.0
    }

    /// Of monospace text on surfaces, e.g. prompts and tables.
    pub fn mono_size() -> f32 {
        ui_size()
    }

    pub fn set_ui(family: &str, size: f32) {
        *UI.write().unwrap_or_else(PoisonError::into_inner) = Arc::from(family);
        UI_SIZE.store(size.to_bits(), Ordering::Relaxed);
    }

    /// The font of code, set from the config. Its family is the monospace one everywhere;
    /// its size and line height are the window buffer's text's.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CodeFont {
        pub family: Arc<str>,
        pub size: f32,
        /// In pixels.
        pub line_height: f32,
        /// Of its normal text, 100 to 900; bold text is as much heavier.
        pub weight: f32,
        /// Whether the font may join characters into one glyph, e.g. `=>` into an arrow.
        pub ligatures: bool,
    }

    impl CodeFont {
        pub fn line_number_size(&self) -> f32 {
            self.size - 1.0
        }
    }

    impl Default for CodeFont {
        fn default() -> Self {
            Self {
                family: Arc::from("Monaco"),
                size: 13.0,
                line_height: 22.0,
                weight: 400.0,
                ligatures: true,
            }
        }
    }

    static CODE_FONT: LazyLock<RwLock<CodeFont>> = LazyLock::new(Default::default);

    pub fn code() -> CodeFont {
        CODE_FONT
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The family of monospace text.
    pub fn mono() -> Arc<str> {
        Arc::clone(
            &CODE_FONT
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .family,
        )
    }

    pub fn set_code(font: CodeFont) {
        *CODE_FONT.write().unwrap_or_else(PoisonError::into_inner) = font;
    }
}
