//! Surfaces over the editor, like panes, popups and prompts, fade and slide in and out.
//! Only their drawing is: input reaches them, or what is under them, at once.

use std::time::{Duration, Instant};

use gpui::{AnyElement, Div, IntoElement, ParentElement, Styled, div, inert, px};

/// How a kind of surface comes and goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    /// 0 shows and hides it at once.
    pub duration: Duration,
    /// Pixels it travels while it fades.
    pub slide: f32,
    pub from: Side,
}

/// Where a surface slides in from and back out to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
}

impl Transition {
    /// The same, coming from `side` instead.
    pub fn coming_from(self, side: Side) -> Self {
        Self { from: side, ..self }
    }
}

/// How far a surface is shown as it comes and goes, from 0 (gone) to 1 (all there).
#[derive(Debug, Default)]
pub struct Presence {
    shown: bool,
    /// Since when it has been coming or going, and how far it was shown then; none once it
    /// is all there or gone.
    change: Option<(Instant, f32)>,
}

impl Presence {
    /// Follows whether the surface is `shown` at `now`; how far to draw it, none once it is
    /// gone. A change midway turns back from where it is.
    pub fn advance(&mut self, shown: bool, now: Instant, duration: Duration) -> Option<f32> {
        if shown != self.shown {
            let from = self.visibility(now, duration);
            self.shown = shown;
            self.change = Some((now, from));
        }
        let visibility = self.visibility(now, duration);
        if visibility == self.rest() {
            self.change = None;
        }
        (self.shown || visibility > 0.0).then_some(visibility)
    }

    pub fn is_changing(&self) -> bool {
        self.change.is_some()
    }

    fn rest(&self) -> f32 {
        if self.shown { 1.0 } else { 0.0 }
    }

    fn visibility(&self, now: Instant, duration: Duration) -> f32 {
        let Some((since, from)) = self.change else {
            return self.rest();
        };
        if duration.is_zero() {
            return self.rest();
        }
        let step = now.saturating_duration_since(since).as_secs_f32() / duration.as_secs_f32();
        if self.shown {
            (from + step).min(1.0)
        } else {
            (from - step).max(0.0)
        }
    }
}

/// A surface and how it is drawn this frame.
pub struct Shown<'a, T> {
    pub content: &'a T,
    pub look: Look,
}

/// What a surface keeps of itself to be drawn while it goes: it is described anew each
/// frame while shown, and its last description stays until it is gone.
pub struct Retained<T> {
    presence: Presence,
    visibility: Option<f32>,
    last: Option<T>,
}

impl<T> Default for Retained<T> {
    fn default() -> Self {
        Self {
            presence: Presence::default(),
            visibility: None,
            last: None,
        }
    }
}

impl<T> Retained<T> {
    /// Takes the surface as described at `now`, none when it is not shown.
    pub fn update(&mut self, current: Option<T>, now: Instant, duration: Duration) {
        self.visibility = self.presence.advance(current.is_some(), now, duration);
        self.last = match self.visibility {
            Some(_) => current.or(self.last.take()),
            None => None,
        };
    }

    /// For a surface owned elsewhere while shown, which hands it over once it closed:
    /// `closed`, if shown no more.
    pub fn update_owned(
        &mut self,
        shown: bool,
        closed: Option<T>,
        now: Instant,
        duration: Duration,
    ) {
        self.visibility = self.presence.advance(shown, now, duration);
        self.last = match self.visibility {
            Some(_) if !shown => closed.or(self.last.take()),
            _ => None,
        };
    }

    /// What to draw of it as it goes; nothing while it is shown or once it is gone.
    pub fn leaving(&self) -> Option<&T> {
        self.last.as_ref().filter(|_| !self.presence.shown)
    }

    /// How it is drawn as it comes, is, or goes; none once it is gone.
    pub fn look(&self, transition: Transition) -> Option<Look> {
        let visibility = self.visibility?;
        Some(Look::new(visibility, transition, !self.presence.shown))
    }

    /// The surface last taken, as it comes, is, or goes.
    pub fn shown(&self, transition: Transition) -> Option<Shown<'_, T>> {
        let content = self.last.as_ref()?;
        let look = self.look(transition)?;
        Some(Shown { content, look })
    }

    pub fn is_changing(&self) -> bool {
        self.presence.is_changing()
    }
}

/// How a surface is drawn partway in or out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub opacity: f32,
    /// Pixels down from where it rests.
    pub offset: f32,
    /// It is going: the mouse goes through it.
    pub leaving: bool,
}

impl Look {
    /// Eased, so a surface arrives quickly and settles, and leaves the other way round. It
    /// fades more evenly than it moves: eased as much, a fade looks done in a third of
    /// the time.
    pub fn new(visibility: f32, transition: Transition, leaving: bool) -> Self {
        let distance = transition.slide * (1.0 - ease_out_cubic(visibility));
        Self {
            opacity: ease_out_quad(visibility),
            offset: match transition.from {
                Side::Above => -distance,
                Side::Below => distance,
            },
            leaving,
        }
    }

    /// Fading as this one does, in place.
    pub fn in_place(self) -> Self {
        Self {
            offset: 0.0,
            ..self
        }
    }

    /// `surface`, laid out over the whole view as surfaces are, drawn as this look says.
    pub fn apply(self, surface: impl IntoElement) -> AnyElement {
        self.layer(div().absolute().left_0().size_full(), surface)
    }

    /// `surface` in its place in the layout, e.g. a pill in a row, drawn as this look says.
    pub fn apply_in_flow(self, surface: impl IntoElement) -> AnyElement {
        // A flex item, so a pill that truncates its text still can.
        self.layer(div().relative().flex().min_w_0(), surface)
    }

    fn layer(self, layer: Div, surface: impl IntoElement) -> AnyElement {
        let layer = layer
            .top(px(self.offset))
            .opacity(self.opacity)
            .child(surface);
        if self.leaving {
            inert(layer).into_any_element()
        } else {
            layer.into_any_element()
        }
    }
}

fn ease_out_quad(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(2)
}

fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATION: Duration = Duration::from_millis(80);
    const SLIDE: Transition = Transition {
        duration: DURATION,
        slide: 10.0,
        from: Side::Below,
    };

    #[test]
    fn comes_in_over_the_duration_and_rests() {
        let mut presence = Presence::default();
        let start = Instant::now();
        assert_eq!(presence.advance(true, start, DURATION), Some(0.0));
        let halfway = presence
            .advance(true, start + DURATION / 2, DURATION)
            .unwrap();
        assert!((halfway - 0.5).abs() < 1e-3, "{halfway}");
        assert!(presence.is_changing());
        assert_eq!(
            presence.advance(true, start + DURATION, DURATION),
            Some(1.0)
        );
        assert!(!presence.is_changing());
    }

    #[test]
    fn goes_out_over_the_duration_and_is_gone() {
        let mut presence = Presence::default();
        let start = Instant::now();
        presence.advance(true, start, Duration::ZERO);
        let leaving = presence.advance(false, start, DURATION);
        assert_eq!(leaving, Some(1.0));
        assert_eq!(presence.advance(false, start + DURATION, DURATION), None);
        assert!(!presence.is_changing());
    }

    #[test]
    fn turns_back_midway_from_where_it_is() {
        let mut presence = Presence::default();
        let start = Instant::now();
        presence.advance(true, start, DURATION);
        let quarter = start + DURATION / 4;
        presence.advance(false, quarter, DURATION);
        let back = presence.advance(true, quarter, DURATION).unwrap();
        assert!((back - 0.25).abs() < 1e-3, "{back}");
        let rest = presence.advance(true, quarter + DURATION * 3 / 4, DURATION);
        assert_eq!(rest, Some(1.0));
    }

    #[test]
    fn without_a_duration_it_comes_and_goes_at_once() {
        let mut presence = Presence::default();
        let now = Instant::now();
        assert_eq!(presence.advance(true, now, Duration::ZERO), Some(1.0));
        assert!(!presence.is_changing());
        assert_eq!(presence.advance(false, now, Duration::ZERO), None);
        assert!(!presence.is_changing());
    }

    #[test]
    fn keeps_the_last_description_while_it_goes() {
        let mut retained = Retained::default();
        let start = Instant::now();
        retained.update(Some("first"), start, Duration::ZERO);
        retained.update(Some("second"), start, DURATION);
        assert_eq!(retained.leaving(), None);
        retained.update(None, start, DURATION);
        assert_eq!(retained.leaving(), Some(&"second"));
        let shown = retained.shown(SLIDE).unwrap();
        assert_eq!((*shown.content, shown.look.leaving), ("second", true));
        retained.update(None, start + DURATION, DURATION);
        assert!(retained.shown(SLIDE).is_none());
        assert_eq!(retained.leaving(), None);
    }

    #[test]
    fn a_surface_owned_elsewhere_is_kept_only_once_closed() {
        let mut retained = Retained::default();
        let start = Instant::now();
        retained.update_owned(true, Some("stale"), start, Duration::ZERO);
        assert_eq!(retained.leaving(), None);
        retained.update_owned(false, Some("closed"), start, DURATION);
        assert_eq!(retained.leaving(), Some(&"closed"));
        retained.update_owned(false, None, start + DURATION / 2, DURATION);
        assert_eq!(retained.leaving(), Some(&"closed"), "still going");
        retained.update_owned(true, None, start + DURATION / 2, DURATION);
        assert_eq!(retained.leaving(), None, "another one opened");
    }

    #[test]
    fn slides_from_its_side_and_fades_eased() {
        let rest = Look::new(1.0, SLIDE, false);
        assert_eq!((rest.opacity, rest.offset), (1.0, 0.0));
        let start = Look::new(0.0, SLIDE, false);
        assert_eq!((start.opacity, start.offset), (0.0, 10.0));
        let above = Look::new(0.0, SLIDE.coming_from(Side::Above), true);
        assert_eq!((above.offset, above.leaving), (-10.0, true));
        assert_eq!(above.in_place().offset, 0.0);
        let halfway = Look::new(0.5, SLIDE, false);
        assert_eq!(halfway.opacity, 0.75, "eased out, evenly enough to be seen");
        assert_eq!(halfway.offset, 1.25, "moved further than faded");
    }
}
