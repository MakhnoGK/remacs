//! The ring before the status of work going on.

use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, IntoElement, PathBuilder, Pixels, Window, canvas, div, point, prelude::*, px, rgba,
};
use remacs_core::{Spinner, StatusSegment};
use remacs_theme::color;

use crate::settings::SpinnersSettings;
use crate::widgets::hsla;

/// Pixels, as the status's icons.
const SIZE: f32 = 12.0;
const STROKE: f32 = 1.5;
/// Degrees of a turning ring's arc.
const TURNING_SWEEP: f32 = 100.0;
/// Degrees of each straight piece the arc is drawn of.
const ARC_STEP: f32 = 10.0;

/// The segments without the items whose work has gone on for less than the delay, and
/// without those left empty; with how soon they must be drawn again, if they must.
pub fn shown_segments(
    segments: Vec<StatusSegment>,
    settings: SpinnersSettings,
    now: Instant,
) -> (Vec<StatusSegment>, Option<Duration>) {
    let mut redraw: Option<Duration> = None;
    let mut redraw_in = |after: Duration| redraw = Some(redraw.map_or(after, |due| due.min(after)));
    let shown = segments
        .into_iter()
        .filter_map(|mut segment| {
            segment.items.retain(|item| {
                let Some(spinner) = item.spinner else {
                    return true;
                };
                let left = settings.delay().saturating_sub(now - spinner.since);
                if !left.is_zero() {
                    redraw_in(left);
                } else if spinner.percent.is_none() {
                    redraw_in(settings.frame());
                }
                left.is_zero()
            });
            (!segment.items.is_empty()).then_some(segment)
        })
        .collect();
    (shown, redraw)
}

/// Where the ring's arc lies, `(start, sweep)` in degrees clockwise from the top.
fn arc(spinner: Spinner, turn: Duration, now: Instant) -> (f32, f32) {
    match spinner.percent {
        Some(percent) => (0.0, 3.6 * f32::from(percent.min(100))),
        None => {
            let turns = (now - spinner.since).as_secs_f32() / turn.as_secs_f32();
            (360.0 * turns.fract(), TURNING_SWEEP)
        }
    }
}

/// A faint ring with its arc in the accent color.
pub fn ring(spinner: Spinner, settings: SpinnersSettings, now: Instant) -> impl IntoElement {
    let (start, sweep) = arc(spinner, settings.turn(), now);
    let accent = color::accent();
    let track = (accent << 8) | (u32::from(settings.track_opacity) * 0xff / 100);
    let paint = move |bounds: Bounds<Pixels>, (), window: &mut Window, _: &mut App| {
        if sweep <= 0.0 {
            return;
        }
        let (center, radius) = (bounds.center(), (SIZE - STROKE) / 2.0);
        let steps = (sweep / ARC_STEP).ceil() as usize;
        let points: Vec<_> = (0..=steps)
            .map(|step| {
                let angle = (start + sweep * step as f32 / steps as f32 - 90.0).to_radians();
                center + point(px(radius * angle.cos()), px(radius * angle.sin()))
            })
            .collect();
        let mut path = PathBuilder::stroke(px(STROKE));
        path.add_polygon(&points, false);
        if let Ok(path) = path.build() {
            window.paint_path(path, hsla(accent));
        }
    };
    div()
        .relative()
        .flex_none()
        .size(px(SIZE))
        .rounded_full()
        .border(px(STROKE))
        .border_color(rgba(track))
        .child(canvas(|_, _, _| {}, paint).absolute().inset_0())
}

#[cfg(test)]
mod tests {
    use remacs_core::StatusItem;

    use super::*;

    const SETTINGS: SpinnersSettings =
        SpinnersSettings { delay: 1500, frame: 50, turn: 1000, track_opacity: 25 };

    fn segment(items: Vec<StatusItem>) -> StatusSegment {
        StatusSegment { items, command: None, active: false, code: true }
    }

    fn texts(segments: &[StatusSegment]) -> Vec<Vec<&str>> {
        segments
            .iter()
            .map(|segment| segment.items.iter().map(|item| item.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn work_shows_only_once_it_has_lasted_the_delay() {
        let now = Instant::now();
        let started = now - Duration::from_millis(1000);
        let segments = vec![
            segment(vec![StatusItem::working("starting", started, None)]),
            segment(vec![
                StatusItem::new(None, "main"),
                StatusItem::working("indexing", started, Some(10)),
            ]),
        ];
        let (shown, redraw) = shown_segments(segments, SETTINGS, now);
        assert_eq!(texts(&shown), vec![vec!["main"]]);
        assert_eq!(redraw, Some(Duration::from_millis(500)));
    }

    #[test]
    fn only_a_turning_ring_asks_for_frames() {
        let now = Instant::now();
        let started = now - Duration::from_secs(2);
        let filled = vec![segment(vec![StatusItem::working("indexing", started, Some(40))])];
        let (shown, redraw) = shown_segments(filled, SETTINGS, now);
        assert_eq!((texts(&shown), redraw), (vec![vec!["indexing"]], None));
        let turning = vec![segment(vec![StatusItem::working("starting", started, None)])];
        let (_, redraw) = shown_segments(turning, SETTINGS, now);
        assert_eq!(redraw, Some(Duration::from_millis(50)));
    }

    #[test]
    fn the_arc_fills_to_the_percent_or_turns_with_time() {
        let now = Instant::now();
        let turn = Duration::from_millis(1000);
        let filled = Spinner { since: now, percent: Some(50) };
        assert_eq!(arc(filled, turn, now), (0.0, 180.0));
        let turning = Spinner { since: now - Duration::from_millis(1250), percent: None };
        assert_eq!(arc(turning, turn, now), (90.0, TURNING_SWEEP));
    }
}
