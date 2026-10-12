use std::time::{Duration, Instant};

use gpui::{Pixels, Point, ScrollDelta, ScrollWheelEvent};

/// What the wheel moved, in pixels of rows `row_height` tall, and whether the content
/// moves there at once: a trackpad moves it under the fingers, momentum included, while a
/// mouse wheel glides, a notch going `lines_per_notch` rows. A wheel some drivers turn
/// into pixels, like Logi Options+, goes as far as they say.
pub fn wheel_delta(
    event: &ScrollWheelEvent,
    row_height: Pixels,
    lines_per_notch: usize,
) -> (Point<Pixels>, bool) {
    match event.delta {
        ScrollDelta::Pixels(delta) => (delta, !event.from_wheel),
        ScrollDelta::Lines(lines) => (
            ScrollDelta::Lines(lines * lines_per_notch as f32).pixel_delta(row_height),
            false,
        ),
    }
}

/// Where the text is drawn, in lines from the top of the buffer: at the viewport's first
/// line, eased toward it when it moves, or a fraction of a line past it after scrolling
/// by pixels.
#[derive(Default)]
pub struct SmoothScroll {
    /// Lines scrolled past the first line, which is kept with them: once the viewport
    /// moves elsewhere, the text rests on a whole line again.
    fraction: Option<(usize, f32)>,
    glide: Glide,
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
            self.glide.place(self.target(first_line));
        }
    }

    /// Shows `first_line` at once, e.g. in another buffer.
    pub fn jump(&mut self, first_line: usize) {
        self.fraction = None;
        self.glide.place(first_line as f32);
    }

    /// Where to draw the text at `now`, gliding toward the viewport's first line.
    pub fn advance(
        &mut self,
        first_line: usize,
        now: Instant,
        duration: Duration,
        max_distance: f32,
    ) -> f32 {
        let target = self.target(first_line);
        self.glide.advance(target, now, duration, max_distance)
    }

    pub fn shown(&self) -> f32 {
        self.glide.shown()
    }

    pub fn is_animating(&self) -> bool {
        self.glide.is_animating()
    }
}

/// A position easing toward where it is headed. A new target sets off from the speed
/// already reached, so targets set in quick succession, like a mouse wheel's notches, add
/// up into one glide without a jolt.
#[derive(Default)]
pub struct Glide {
    shown: f32,
    /// Where it was last headed.
    target: f32,
    animation: Option<Animation>,
}

#[derive(Clone, Copy)]
struct Animation {
    from: f32,
    /// Per second, at `started`.
    velocity: f32,
    started: Instant,
}

impl Animation {
    fn progress(&self, now: Instant, duration: Duration) -> f32 {
        now.saturating_duration_since(self.started).as_secs_f32()
            / duration.as_secs_f32().max(f32::EPSILON)
    }

    /// A cubic from `from` at `velocity` to rest at `target`: a cubic ease-out when it
    /// sets off as fast as one would.
    fn position(&self, progress: f32, target: f32, duration: Duration) -> f32 {
        let t = progress;
        let launch = self.velocity * duration.as_secs_f32();
        self.from
            + launch * (t * t * t - 2.0 * t * t + t)
            + (target - self.from) * t * t * (3.0 - 2.0 * t)
    }

    fn velocity(&self, progress: f32, target: f32, duration: Duration) -> f32 {
        let t = progress;
        self.velocity * (3.0 * t * t - 4.0 * t + 1.0)
            + (target - self.from) * 6.0 * t * (1.0 - t) / duration.as_secs_f32()
    }
}

impl Glide {
    /// Where to be at `now`. A new target eases in over `duration`, starting at most
    /// `max_distance` away from it, so a long jump does not race past pages of text.
    pub fn advance(
        &mut self,
        target: f32,
        now: Instant,
        duration: Duration,
        max_distance: f32,
    ) -> f32 {
        if target != self.target {
            if duration.is_zero() {
                self.place(target);
            } else {
                self.head_for(target, now, duration, max_distance);
            }
        }
        if let Some(animation) = self.animation {
            let progress = animation.progress(now, duration);
            if progress >= 1.0 {
                self.place(target);
            } else {
                self.shown = animation.position(progress, target, duration);
            }
        }
        self.shown
    }

    fn head_for(&mut self, target: f32, now: Instant, duration: Duration, max_distance: f32) {
        let mut reached = 0.0;
        // From where the glide is now, not where it was last drawn, or the frame a notch
        // comes in would stand still.
        if let Some(animation) = self.animation {
            let progress = animation.progress(now, duration).min(1.0);
            self.shown = animation.position(progress, self.target, duration);
            reached = animation.velocity(progress, self.target, duration);
        }
        let from = self
            .shown
            .clamp(target - max_distance, target + max_distance);
        // An ease-out's: the fastest that comes to rest on the target without passing it.
        let eased = 3.0 * (target - from) / duration.as_secs_f32();
        // Turning back, or from rest, it sets off at once.
        let velocity = if reached * eased > 0.0 && reached.abs() < eased.abs() {
            reached
        } else {
            eased
        };
        self.target = target;
        self.animation = Some(Animation {
            from,
            velocity,
            started: now,
        });
    }

    pub fn shown(&self) -> f32 {
        self.shown
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_animating(&self) -> bool {
        self.animation.is_some()
    }

    /// Moves it, its target and its glide by `distance`, as when what it counts from
    /// moves the other way.
    pub fn shift(&mut self, distance: f32) {
        self.shown += distance;
        self.target += distance;
        if let Some(animation) = &mut self.animation {
            animation.from += distance;
        }
    }

    pub fn place(&mut self, position: f32) {
        self.shown = position;
        self.target = position;
        self.animation = None;
    }
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
    fn sets_off_as_a_cubic_ease_out() {
        let mut glide = Glide::default();
        let start = Instant::now();
        glide.advance(8.0, start, DURATION, 50.0);
        let halfway = glide.advance(8.0, start + DURATION / 2, DURATION, 50.0);
        assert!(
            (halfway - 8.0 * (1.0 - 0.5f32.powi(3))).abs() < 1e-4,
            "{halfway}"
        );
    }

    #[test]
    fn notches_in_quick_succession_add_up_without_a_jolt() {
        let mut glide = Glide::default();
        let start = Instant::now();
        let step = Duration::from_millis(5);
        glide.advance(3.0, start, DURATION, 50.0);
        let mut previous = 0.0;
        let mut previous_speed = 0.0;
        let mut target = 3.0;
        for frame in 1..60 {
            // A notch every 30 ms.
            if frame % 6 == 0 {
                target += 3.0;
            }
            let shown = glide.advance(target, start + step * frame, DURATION, 50.0);
            let speed = shown - previous;
            assert!(speed >= 0.0, "turned back at frame {frame}");
            assert!(shown <= target, "passed the target at frame {frame}");
            // Neither speeds up nor slows down by more than a little from frame to frame.
            if frame > 1 {
                assert!(
                    (speed - previous_speed).abs() < 0.2,
                    "jolt at frame {frame}: {previous_speed} then {speed}"
                );
            }
            (previous, previous_speed) = (shown, speed);
        }
        assert_eq!(
            glide.advance(target, start + step * 60 + DURATION, DURATION, 50.0),
            target
        );
    }

    #[test]
    fn turning_back_sets_off_at_once() {
        let mut glide = Glide::default();
        let start = Instant::now();
        glide.advance(6.0, start, DURATION, 50.0);
        let shown = glide.advance(6.0, start + DURATION / 4, DURATION, 50.0);
        glide.advance(0.0, start + DURATION / 4, DURATION, 50.0);
        let next = glide.advance(0.0, start + DURATION / 4 + DURATION / 20, DURATION, 50.0);
        assert!(next < shown, "{next} after {shown}");
    }

    #[test]
    fn shifting_moves_the_glide_along() {
        let mut glide = Glide::default();
        let start = Instant::now();
        glide.advance(2.5, start, DURATION, 50.0);
        let shown = glide.advance(2.5, start + DURATION / 2, DURATION, 50.0);
        glide.shift(-2.0);
        let target = glide.target();
        assert_eq!(target, 0.5);
        let shifted = glide.advance(target, start + DURATION / 2, DURATION, 50.0);
        assert!((shifted - (shown - 2.0)).abs() < 1e-5, "{shifted}");
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

    #[test]
    fn a_notch_goes_the_lines_set_and_glides() {
        let event = |delta, from_wheel| ScrollWheelEvent {
            delta,
            from_wheel,
            ..Default::default()
        };
        let notch = event(ScrollDelta::Lines(gpui::point(0.0, -1.0)), true);
        let (delta, at_once) = wheel_delta(&notch, gpui::px(20.0), 3);
        assert_eq!(delta.y, gpui::px(-60.0));
        assert!(!at_once);
        let pixels = ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(-103.0)));
        let (delta, at_once) = wheel_delta(&event(pixels, true), gpui::px(20.0), 3);
        assert_eq!(delta.y, gpui::px(-103.0), "as far as the driver says");
        assert!(!at_once, "a wheel in pixels glides");
        let (_, at_once) = wheel_delta(&event(pixels, false), gpui::px(20.0), 3);
        assert!(at_once, "a trackpad moves the content under the fingers");
    }
}
