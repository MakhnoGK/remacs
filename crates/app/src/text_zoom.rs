//! The size of the window buffer's text, changed by ⌘= and ⌘- or ⌘ with the wheel over
//! the configured one; prompts, popups, panes and terminals keep the configured size.

use gpui::{ScrollDelta, px};
use remacs_core::TextSize;
use remacs_theme::font::CodeFont;

use crate::settings::ZoomSettings;

#[derive(Debug, Default)]
pub struct TextZoom {
    /// Points over the configured size, kept when the settings change.
    offset: f32,
    /// Pixels scrolled with ⌘ held toward a step not taken yet; positive is larger.
    scrolled: f32,
}

impl TextZoom {
    /// The configured code font `code` at the zoomed size, its rows as many times the
    /// size as before.
    pub fn font(&self, code: CodeFont) -> CodeFont {
        if self.offset == 0.0 {
            return code;
        }
        let size = code.size + self.offset;
        CodeFont {
            line_height: (code.line_height / code.size * size).round(),
            size,
            ..code
        }
    }

    /// The zoomed size in percent of `configured`, none while not zoomed.
    pub fn percent(&self, configured: f32) -> Option<u32> {
        (self.offset != 0.0)
            .then(|| ((configured + self.offset) / configured * 100.0).round() as u32)
    }

    /// Changes the size of text configured `configured` points; false when it stays.
    pub fn change(&mut self, change: TextSize, configured: f32, settings: ZoomSettings) -> bool {
        let before = self.offset;
        self.offset = match change {
            TextSize::Larger => self.stepped(1.0, configured, settings),
            TextSize::Smaller => self.stepped(-1.0, configured, settings),
            TextSize::Configured => 0.0,
        };
        self.offset != before
    }

    /// Steps once for every `wheel-step` pixels scrolled with ⌘ held, or every notch;
    /// up is larger. False when the size stays.
    pub fn scroll(&mut self, delta: ScrollDelta, configured: f32, settings: ZoomSettings) -> bool {
        let pixels = match delta {
            ScrollDelta::Pixels(delta) => delta.y / px(1.0),
            ScrollDelta::Lines(lines) => lines.y * settings.wheel_step,
        };
        if pixels * self.scrolled < 0.0 {
            self.scrolled = 0.0;
        }
        self.scrolled += pixels;
        let steps = (self.scrolled / settings.wheel_step).trunc();
        self.scrolled -= steps * settings.wheel_step;
        let change = if steps > 0.0 {
            TextSize::Larger
        } else {
            TextSize::Smaller
        };
        let mut changed = false;
        for _ in 0..steps.abs() as usize {
            changed |= self.change(change, configured, settings);
        }
        changed
    }

    /// The offset one step toward `direction`, within the limits but never back.
    fn stepped(&self, direction: f32, configured: f32, settings: ZoomSettings) -> f32 {
        let size = configured + self.offset;
        let target = (size + direction * settings.step).clamp(settings.min_size, settings.max_size);
        if (target - size) * direction < 0.0 {
            self.offset
        } else {
            target - configured
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::point;

    use super::*;

    const SETTINGS: ZoomSettings = ZoomSettings {
        step: 1.0,
        min_size: 8.0,
        max_size: 16.0,
        wheel_step: 100.0,
    };

    fn code(size: f32, line_height: f32) -> CodeFont {
        CodeFont {
            size,
            line_height,
            ..CodeFont::default()
        }
    }

    fn size(zoom: &TextZoom) -> f32 {
        zoom.font(code(13.0, 26.0)).size
    }

    #[test]
    fn steps_stop_at_the_limits_and_go_back_to_the_configured_size() {
        let mut zoom = TextZoom::default();
        for _ in 0..5 {
            zoom.change(TextSize::Larger, 13.0, SETTINGS);
        }
        assert_eq!(size(&zoom), 16.0);
        assert!(!zoom.change(TextSize::Larger, 13.0, SETTINGS));
        for _ in 0..10 {
            zoom.change(TextSize::Smaller, 13.0, SETTINGS);
        }
        assert_eq!(size(&zoom), 8.0);
        assert!(zoom.change(TextSize::Configured, 13.0, SETTINGS));
        assert_eq!(size(&zoom), 13.0);
    }

    #[test]
    fn the_percent_is_shown_only_while_zoomed() {
        let mut zoom = TextZoom::default();
        assert_eq!(zoom.percent(13.0), None);
        zoom.change(TextSize::Larger, 13.0, SETTINGS);
        zoom.change(TextSize::Larger, 13.0, SETTINGS);
        assert_eq!(zoom.percent(13.0), Some(115));
    }

    #[test]
    fn rows_keep_their_proportion_in_whole_pixels() {
        let mut zoom = TextZoom::default();
        zoom.change(TextSize::Larger, 13.0, SETTINGS);
        let font = zoom.font(code(13.0, 22.0));
        assert_eq!((font.size, font.line_height), (14.0, 24.0));
    }

    #[test]
    fn a_size_configured_past_a_limit_is_not_stepped_away_from() {
        let mut zoom = TextZoom::default();
        assert!(!zoom.change(TextSize::Larger, 20.0, SETTINGS));
        assert!(zoom.change(TextSize::Smaller, 20.0, SETTINGS));
        assert_eq!(zoom.font(code(20.0, 40.0)).size, 16.0);
    }

    #[test]
    fn scrolling_steps_once_per_wheel_step_and_restarts_on_turning_back() {
        let mut zoom = TextZoom::default();
        let pixels = |y: f32| ScrollDelta::Pixels(point(px(0.0), px(y)));
        assert!(!zoom.scroll(pixels(60.0), 13.0, SETTINGS));
        assert!(zoom.scroll(pixels(60.0), 13.0, SETTINGS));
        assert_eq!(size(&zoom), 14.0);
        // The 20 pixels left over toward larger do not count once it turns back.
        assert!(!zoom.scroll(pixels(-90.0), 13.0, SETTINGS));
        assert!(zoom.scroll(pixels(-10.0), 13.0, SETTINGS));
        assert_eq!(size(&zoom), 13.0);
        assert!(zoom.scroll(ScrollDelta::Lines(point(0.0, -2.0)), 13.0, SETTINGS));
        assert_eq!(size(&zoom), 11.0);
    }
}
