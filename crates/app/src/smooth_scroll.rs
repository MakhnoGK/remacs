use std::time::{Duration, Instant};

/// Where the text is drawn, in lines from the top of the buffer: at the viewport's first
/// line, eased toward it when it moves, or a fraction of a line past it after scrolling
/// by pixels.
#[derive(Default)]
pub struct SmoothScroll {
    /// Lines scrolled past the first line, which is kept with them: once the viewport
    /// moves elsewhere, the text rests on a whole line again.
    fraction: Option<(usize, f32)>,
    shown: f32,
    /// Where the text was last headed.
    target: f32,
    animation: Option<Animation>,
}

#[derive(Clone, Copy)]
struct Animation {
    from: f32,
    started: Instant,
}

impl SmoothScroll {
    /// Where the text rests with the viewport's first line at `first_line`.
    pub fn target(&self, first_line: usize) -> f32 {
        let fraction = self.fraction.filter(|&(line, _)| line == first_line);
        first_line as f32 + fraction.map_or(0.0, |(_, fraction)| fraction)
    }

    /// Rests `fraction` of a line past `first_line`; `at_once` moves the text there
    /// without easing, as a trackpad does by moving it under the fingers.
    pub fn rest_at(&mut self, first_line: usize, fraction: f32, at_once: bool) {
        self.fraction = (fraction > 0.0).then_some((first_line, fraction));
        if at_once {
            self.place(self.target(first_line));
        }
    }

    /// Shows `first_line` at once, e.g. in another buffer.
    pub fn jump(&mut self, first_line: usize) {
        self.fraction = None;
        self.place(first_line as f32);
    }

    /// Where to draw the text at `now`. A new target eases in over `duration`, starting
    /// at most `max_distance` lines away from it, so a long jump does not race past
    /// pages of text.
    pub fn advance(
        &mut self,
        first_line: usize,
        now: Instant,
        duration: Duration,
        max_distance: f32,
    ) -> f32 {
        let target = self.target(first_line);
        if target != self.target {
            if duration.is_zero() {
                self.place(target);
            } else {
                self.target = target;
                let from = self
                    .shown
                    .clamp(target - max_distance, target + max_distance);
                self.animation = Some(Animation { from, started: now });
            }
        }
        if let Some(Animation { from, started }) = self.animation {
            let progress = now.saturating_duration_since(started).as_secs_f32()
                / duration.as_secs_f32().max(f32::EPSILON);
            if progress >= 1.0 {
                self.place(target);
            } else {
                self.shown = from + (target - from) * ease_out(progress);
            }
        }
        self.shown
    }

    pub fn shown(&self) -> f32 {
        self.shown
    }

    pub fn is_animating(&self) -> bool {
        self.animation.is_some()
    }

    fn place(&mut self, position: f32) {
        self.shown = position;
        self.target = position;
        self.animation = None;
    }
}

/// Fast at first, settling gently: a cubic ease-out.
fn ease_out(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATION: Duration = Duration::from_millis(100);

    #[test]
    fn eases_toward_a_new_first_line_and_settles_on_it() {
        let mut scroll = SmoothScroll::default();
        let start = Instant::now();
        assert_eq!(scroll.advance(10, start, DURATION, 50.0), 0.0);
        let halfway = scroll.advance(10, start + DURATION / 2, DURATION, 50.0);
        assert!(halfway > 5.0 && halfway < 10.0, "{halfway}");
        assert!(scroll.is_animating());
        assert_eq!(scroll.advance(10, start + DURATION, DURATION, 50.0), 10.0);
        assert!(!scroll.is_animating());
    }

    #[test]
    fn a_long_jump_starts_close_to_its_target() {
        let mut scroll = SmoothScroll::default();
        assert_eq!(scroll.advance(1000, Instant::now(), DURATION, 40.0), 960.0);
    }

    #[test]
    fn without_a_duration_the_text_moves_at_once() {
        let mut scroll = SmoothScroll::default();
        assert_eq!(scroll.advance(7, Instant::now(), Duration::ZERO, 40.0), 7.0);
        assert!(!scroll.is_animating());
    }

    #[test]
    fn pixel_scrolling_rests_between_lines_until_the_viewport_moves() {
        let mut scroll = SmoothScroll::default();
        scroll.rest_at(3, 0.25, true);
        assert_eq!(scroll.shown(), 3.25);
        assert_eq!(scroll.advance(3, Instant::now(), DURATION, 40.0), 3.25);
        assert_eq!(scroll.target(4), 4.0, "a command moved the viewport");
    }
}
