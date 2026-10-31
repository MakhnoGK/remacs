use std::fmt::Display;
use std::ops::RangeInclusive;

use remacs_config::Section;
use serde::Deserialize;

use super::sections::*;

const FONT_SIZES: RangeInclusive<f32> = 6.0..=72.0;
const FONT_WEIGHTS: RangeInclusive<f32> = 100.0..=900.0;
pub(super) const GLASS_OPACITIES: RangeInclusive<f32> = 0.0..=1.0;
/// Pixels, of documentation and problems by the text.
const POPUP_MAX_WIDTHS: RangeInclusive<f32> = 240.0..=2000.0;
/// Pixels.
const POPUP_MAX_HEIGHTS: RangeInclusive<f32> = 120.0..=2000.0;
/// Milliseconds; past 250 a surface keeps one waiting to see it.
const TRANSITION_DURATIONS: RangeInclusive<u64> = 0..=250;
/// Pixels.
const TRANSITION_SLIDES: RangeInclusive<f32> = 0.0..=32.0;

/// `ui.toml`.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct UiSettings {
    pub theme: ThemeSettings,
    pub font: FontSettings,
    pub ui_font: UiFontSettings,
    pub zoom: ZoomSettings,
    pub cursor: CursorSettings,
    pub keyboard: KeyboardSettings,
    pub mouse: MouseSettings,
    pub scroll: ScrollSettings,
    pub glass: GlassSettings,
    pub echo: EchoSettings,
    pub hints: HintsSettings,
    pub prefix_keys: PrefixKeysSettings,
    pub spinners: SpinnersSettings,
    pub documentation: DocumentationSettings,
    pub problems: ProblemsSettings,
    pub diff: DiffSettings,
    pub folds: FoldsSettings,
    pub prompts: PromptsSettings,
    pub swatches: SwatchesSettings,
    pub transitions: TransitionsSettings,
}

impl UiSettings {
    pub fn bottom(&self) -> BottomSettings {
        BottomSettings {
            echo: self.echo,
            hints: self.hints,
            prefix_keys: self.prefix_keys,
            spinners: self.spinners,
        }
    }
}

/// Checks each `name: value, range;` in turn, returning the first one outside its range.
macro_rules! check_ranges {
    ($($name:literal: $value:expr, $range:expr;)*) => {
        $(check_range($name, $value, &$range)?;)*
    };
}

impl Section for UiSettings {
    const NAME: &str = "ui";

    fn check(&self) -> Result<(), String> {
        non_empty("theme.name", &self.theme.name)?;
        non_empty("theme.light", &self.theme.light)?;
        non_empty("theme.dark", &self.theme.dark)?;
        non_empty("font.family", &self.font.family)?;
        check_ranges! {
            "font.size": self.font.size, FONT_SIZES;
            "font.line-height": self.font.line_height, 1.0..=3.0;
            "font.weight": self.font.weight.value(), FONT_WEIGHTS;
        }
        non_empty("ui-font.family", &self.ui_font.family)?;
        let zoom = &self.zoom;
        check_ranges! {
            // Rows on surfaces keep their heights, which larger text would not fit.
            "ui-font.size": self.ui_font.size, 10.0..=16.0;
            "ui-font.weight": self.ui_font.weight.value(), FONT_WEIGHTS;
            // Points.
            "zoom.step": zoom.step, 0.5..=8.0;
            "zoom.min-size": zoom.min_size, FONT_SIZES;
            "zoom.max-size": zoom.max_size, FONT_SIZES;
        }
        let (min, max) = (zoom.min_size, zoom.max_size);
        if min > max {
            return Err(format!("zoom.min-size {min} is over zoom.max-size {max}"));
        }
        let (scroll, transitions) = (&self.scroll, &self.transitions);
        // Pixels and milliseconds, but for counts and the percent of `diff.band-opacity`.
        check_ranges! {
            "zoom.wheel-step": zoom.wheel_step, 10.0..=1000.0;
            // Past half a narrow font's character the block is a pill.
            "cursor.radius": self.cursor.radius, 0.0..=4.0;
            "mouse.drag-scroll-interval": self.mouse.drag_scroll_interval, 5..=1000;
            // 0 turns the animation off.
            "scroll.animation-duration": scroll.animation_duration, 0..=1000;
            "scroll.lines-per-notch": scroll.lines_per_notch, 1..=20;
            "glass.opacity": self.glass.opacity, GLASS_OPACITIES;
            // The window server blurs no more than 64.
            "glass.blur": self.glass.blur, 0..=64;
            "echo.lines": self.echo.lines, 3..=40;
            "echo.history": self.echo.history, 10..=100_000;
            // 0 keeps a message until the next key.
            "echo.hide-after": self.echo.hide_after, 0..=60_000;
            "hints.side-room": self.hints.side_room, 0.0..=800.0;
            "prefix-keys.delay": self.prefix_keys.delay, 0..=5000;
            "prefix-keys.columns": self.prefix_keys.columns, 1..=8;
            "spinners.delay": self.spinners.delay, 0..=10_000;
            // Under 16 is faster than the screen redraws.
            "spinners.frame": self.spinners.frame, 16..=500;
            "spinners.turn": self.spinners.turn, 200..=5000;
            "spinners.track-opacity": self.spinners.track_opacity, 0..=100;
            "documentation.max-width": self.documentation.max_width, POPUP_MAX_WIDTHS;
            "documentation.max-height": self.documentation.max_height, POPUP_MAX_HEIGHTS;
            "problems.max-width": self.problems.max_width, POPUP_MAX_WIDTHS;
            "problems.max-height": self.problems.max_height, POPUP_MAX_HEIGHTS;
            "diff.side-width": self.diff.side_width, 240.0..=900.0;
            "diff.band-opacity": self.diff.band_opacity, 0..=60;
            "prompts.input-margin": self.prompts.input_margin, 0.0..=400.0;
            "swatches.thickness": self.swatches.thickness, 1.0..=6.0;
            "transitions.pane-duration": transitions.pane_duration, TRANSITION_DURATIONS;
            "transitions.pane-slide": transitions.pane_slide, TRANSITION_SLIDES;
            "transitions.prompt-duration": transitions.prompt_duration, TRANSITION_DURATIONS;
            "transitions.prompt-slide": transitions.prompt_slide, TRANSITION_SLIDES;
            "transitions.popup-duration": transitions.popup_duration, TRANSITION_DURATIONS;
            "transitions.popup-slide": transitions.popup_slide, TRANSITION_SLIDES;
        }
        Ok(())
    }
}

fn non_empty(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() { Err(format!("{name} is empty")) } else { Ok(()) }
}

/// A value left out, `None`, is not checked.
fn check_range<T: PartialOrd + Display>(
    name: &str,
    value: impl Into<Option<T>>,
    range: &RangeInclusive<T>,
) -> Result<(), String> {
    match value.into() {
        Some(value) if !range.contains(&value) => {
            Err(format!("{name} {value} is outside {}..={}", range.start(), range.end()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::WindowBackgroundAppearance;
    use remacs_theme::font::CodeFont;
    use remacs_theme::{Appearance, DEFAULT};

    use super::*;
    use crate::transition::{Side, Transition};

    #[test]
    fn the_system_theme_is_the_light_or_the_dark_one_as_macos_is() {
        let fixed = parse("[theme]\nname = \"paper\"\n").unwrap().theme;
        assert!(!fixed.follows_system());
        assert_eq!(fixed.shown(Appearance::Light), "paper");
        let system = parse("[theme]\nname = \"system\"\nlight = \"paper\"\n").unwrap().theme;
        assert!(system.follows_system());
        assert_eq!(system.shown(Appearance::Light), "paper");
        assert_eq!(system.shown(Appearance::Dark), DEFAULT);
    }

    fn parse(text: &str) -> Result<UiSettings, String> {
        let settings: UiSettings = toml::from_str(text).map_err(|error| error.to_string())?;
        settings.check().map(|()| settings)
    }

    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_config::assert_default_file::<UiSettings>();
    }

    #[test]
    fn defaults_match_the_built_in_code_font() {
        assert_eq!(UiSettings::default().font.code_font(), CodeFont::default());
    }

    #[test]
    fn reads_every_setting() {
        let settings = parse(
            "[theme]\nname = \"modus-operandi\"\n\
             [font]\nfamily = \"Menlo\"\nsize = 15\nline-height = 1.5\nweight = \"light\"\n\
             strokes = \"thin\"\n\
             ligatures = false\n\
             [ui-font]\nfamily = \"Helvetica Neue\"\nsize = 14\nweight = \"medium\"\n\
             [zoom]\nstep = 2\nmin-size = 10\nmax-size = 24\nwheel-step = 50\n\
             [cursor]\nheight = \"font\"\nradius = 2.5\n[keyboard]\nmeta = \"left\"\n\
             [mouse]\ndrag-scroll-interval = 60\n\
             [scroll]\nanimation-duration = 0\nlines-per-notch = 5\n[glass]\nopacity = 0.5\nblur = 0\n\
             [echo]\nlines = 6\nhistory = 50\nhide-after = 0\n\
             [hints]\nshow = false\nside-room = 200\n\
             [prefix-keys]\nshow = false\ndelay = 0\ncolumns = 3\n\
             [spinners]\ndelay = 0\nframe = 100\nturn = 2000\ntrack-opacity = 40\n\
             [documentation]\nmax-width = 900\nmax-height = 300\n\
             [problems]\nmax-width = 700\nmax-height = 200\n\
             [prompts]\ninput-margin = 20\ndirectory = \"below\"\n\
             [swatches]\ninline = false\nthickness = 2\ncompletion = false\n\
             [transitions]\npane-duration = 0\npane-slide = 0\nprompt-duration = 100\n\
             prompt-slide = 12\npopup-duration = 40\npopup-slide = 4\n",
        )
        .unwrap();
        assert_eq!(settings.transitions.pane().duration, Duration::ZERO);
        assert_eq!(
            settings.transitions.prompt(),
            Transition { duration: Duration::from_millis(100), slide: 12.0, from: Side::Above }
        );
        assert_eq!(settings.transitions.popup().slide, 4.0);
        assert_eq!(settings.prompts.input_margin, 20.0);
        assert_eq!(settings.prompts.directory, DirectoryPosition::Below);
        assert_eq!(
            settings.swatches,
            SwatchesSettings { inline: false, thickness: 2.0, completion: false }
        );
        assert_eq!(
            settings.documentation,
            DocumentationSettings { max_width: 900.0, max_height: 300.0 }
        );
        assert_eq!(settings.problems, ProblemsSettings { max_width: 700.0, max_height: 200.0 });
        assert_eq!(
            settings.bottom(),
            BottomSettings {
                echo: EchoSettings { lines: 6, history: 50, hide_after: 0 },
                hints: HintsSettings { show: false, side_room: 200.0 },
                prefix_keys: PrefixKeysSettings { show: false, delay: 0, columns: 3 },
                spinners: SpinnersSettings { delay: 0, frame: 100, turn: 2000, track_opacity: 40 },
            }
        );
        assert_eq!(settings.glass, GlassSettings { opacity: Some(0.5), blur: 0 });
        assert_eq!(settings.theme.name, "modus-operandi");
        assert_eq!(settings.keyboard.meta, MetaOption::Left);
        assert_eq!(settings.mouse.drag_scroll_interval, 60);
        assert_eq!(settings.scroll.animation_duration, 0);
        assert_eq!(settings.scroll.lines_per_notch, 5);
        assert_eq!(settings.font.family, "Menlo");
        assert_eq!(settings.font.code_font().line_height, 23.0);
        assert_eq!(settings.font.code_font().weight, 300.0);
        assert_eq!(settings.font.strokes, Strokes::Thin);
        assert!(!settings.font.code_font().ligatures);
        assert_eq!(
            settings.ui_font,
            UiFontSettings {
                family: "Helvetica Neue".to_owned(),
                size: 14.0,
                weight: Weight::Named(WeightName::Medium)
            }
        );
        assert_eq!(settings.cursor.height, CursorHeight::Font);
        assert_eq!(settings.cursor.radius, 2.5);
        assert_eq!(
            settings.zoom,
            ZoomSettings { step: 2.0, min_size: 10.0, max_size: 24.0, wheel_step: 50.0 }
        );
    }

    #[test]
    fn the_smallest_zoomed_size_is_not_over_the_largest() {
        let error = parse("[zoom]\nmin-size = 30\nmax-size = 20\n").unwrap_err();
        assert_eq!(error, "zoom.min-size 30 is over zoom.max-size 20");
    }

    #[test]
    fn an_opaque_tint_makes_the_window_opaque_and_unblurred() {
        let glass = |opacity| GlassSettings { opacity: Some(opacity), blur: 30 };
        assert_eq!(glass(1.0).window_background(), WindowBackgroundAppearance::Opaque);
        assert_eq!(glass(1.0).window_blur(), 0);
        assert_eq!(glass(0.9).window_background(), WindowBackgroundAppearance::Transparent);
        assert_eq!(glass(0.9).window_blur(), 30);
    }

    #[test]
    fn rejects_sizes_out_of_range_and_unknown_values() {
        for invalid in [
            "[font]\nsize = 2\n",
            "[font]\nline-height = 0.5\n",
            "[font]\nweight = 950\n",
            "[font]\nweight = \"heavy\"\n",
            "[ui-font]\nfamily = \"\"\n",
            "[ui-font]\nsize = 18\n",
            "[ui-font]\nweight = 50\n",
            "[cursor]\nheight = \"half\"\n",
            "[cursor]\nradius = 5\n",
            "[keyboard]\nmeta = \"none\"\n",
            "[mouse]\ndrag-scroll-interval = 0\n",
            "[scroll]\nanimation-duration = 5000\n",
            "[scroll]\nlines-per-notch = 0\n",
            "[glass]\nopacity = 1.5\n",
            "[glass]\nblur = 65\n",
            "[theme]\nname = \" \"\n",
            "[theme]\ndark = \"\"\n",
            "[echo]\nlines = 1\n",
            "[echo]\nhide-after = 60001\n",
            "[prompts]\ninput-margin = -1\n",
            "[swatches]\nthickness = 0\n",
            "[hints]\nside-room = -1\n",
            "[prefix-keys]\ncolumns = 0\n",
            "[documentation]\nmax-width = 100\n",
            "[documentation]\nmax-height = 5000\n",
            "[problems]\nmax-height = 100\n",
            "[diff]\nside-width = 100\n",
            "[diff]\nband-opacity = 61\n",
            "[transitions]\npane-duration = 251\n",
            "[transitions]\npopup-slide = -1\n",
            "[transitions]\nduration = 50\n",
        ] {
            assert!(parse(invalid).is_err(), "{invalid}");
        }
        let weight = |text| parse(text).unwrap().font.weight.value();
        assert_eq!(weight("[font]\nweight = 550\n"), 550.0);
        assert_eq!(weight("[font]\nweight = \"extra-bold\"\n"), 800.0);
    }
}
