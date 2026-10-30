use std::mem;

use crate::buffer::BufferId;
use crate::viewport::Viewport;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(u32);

impl WindowId {
    /// Holds a buffer's own point while another window's point takes its place.
    pub(crate) const PARKED: Self = Self(u32::MAX);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// One above the other, like `C-x 2`.
    Rows,
    /// Side by side, like `C-x 3`.
    Columns,
}

/// How the windows share the frame, like Emacs's window tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Window(WindowId),
    Split {
        axis: Axis,
        /// The share of the first, above or on the left.
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl Layout {
    /// Top to bottom and left to right, the order `other-window` goes in.
    pub fn windows(&self) -> Vec<WindowId> {
        match self {
            Layout::Window(id) => vec![*id],
            Layout::Split { first, second, .. } => [first.windows(), second.windows()].concat(),
        }
    }

    /// How many windows are stacked along `axis` at most, the ones a balanced layout gives
    /// equal room.
    fn stacked(&self, along: Axis) -> usize {
        match self {
            Layout::Window(_) => 1,
            Layout::Split {
                axis,
                first,
                second,
                ..
            } => {
                let (first, second) = (first.stacked(along), second.stacked(along));
                if *axis == along {
                    first + second
                } else {
                    first.max(second)
                }
            }
        }
    }

    /// The share of the whole along `axis` that `window` has.
    pub(crate) fn share(&self, window: WindowId, along: Axis) -> Option<f32> {
        match self {
            Layout::Window(id) => (*id == window).then_some(1.0),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let same = *axis == along;
                first
                    .share(window, along)
                    .map(|share| if same { share * ratio } else { share })
                    .or_else(|| {
                        let share = second.share(window, along)?;
                        Some(if same { share * (1.0 - ratio) } else { share })
                    })
            }
        }
    }

    /// Gives `window` `delta` more of the whole along `axis`, from the windows beside it in
    /// its nearest split along it, leaving each window at least `min` of the whole; false
    /// if there is no such split or no room to change.
    pub(crate) fn resize(&mut self, window: WindowId, along: Axis, delta: f32, min: f32) -> bool {
        self.resize_in(window, along, delta, min, 1.0) == Some(true)
    }

    /// `None` while `window` is not in `self`, which has `extent` of the whole.
    fn resize_in(
        &mut self,
        window: WindowId,
        along: Axis,
        delta: f32,
        min: f32,
        extent: f32,
    ) -> Option<bool> {
        let Layout::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        else {
            return (*self == Layout::Window(window)).then_some(false);
        };
        let same = *axis == along;
        let (first_extent, second_extent) = if same {
            (extent * *ratio, extent * (1.0 - *ratio))
        } else {
            (extent, extent)
        };
        let (in_first, inner) = match first.resize_in(window, along, delta, min, first_extent) {
            Some(resized) => (true, resized),
            None => (
                false,
                second.resize_in(window, along, delta, min, second_extent)?,
            ),
        };
        if inner || !same {
            return Some(inner);
        }
        let delta = if in_first { delta } else { -delta };
        let Some(new) = clamp_ratio(*ratio + delta / extent, first, second, along, min / extent)
        else {
            return Some(false);
        };
        let resized = new != *ratio;
        *ratio = new;
        Some(resized)
    }

    /// Sets the ratio of the `index`th split, counted depth first, keeping each window at
    /// least `min` of that split.
    pub(crate) fn set_ratio(&mut self, mut index: usize, new: f32, min: f32) {
        self.set_ratio_in(&mut index, new, min);
    }

    fn set_ratio_in(&mut self, index: &mut usize, new: f32, min: f32) -> bool {
        let Layout::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        else {
            return false;
        };
        if *index == 0 {
            if let Some(new) = clamp_ratio(new, first, second, *axis, min) {
                *ratio = new;
            }
            return true;
        }
        *index -= 1;
        first.set_ratio_in(index, new, min) || second.set_ratio_in(index, new, min)
    }

    /// Gives the windows stacked along each axis equal room, like Emacs `balance-windows`.
    pub(crate) fn balance(&mut self) {
        if let Layout::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        {
            let (before, after) = (first.stacked(*axis), second.stacked(*axis));
            *ratio = before as f32 / (before + after) as f32;
            first.balance();
            second.balance();
        }
    }

    fn split(&mut self, window: WindowId, axis: Axis, new: WindowId) {
        match self {
            Layout::Window(id) if *id == window => {
                *self = Layout::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Layout::Window(window)),
                    second: Box::new(Layout::Window(new)),
                };
            }
            Layout::Window(_) => {}
            Layout::Split { first, second, .. } => {
                first.split(window, axis, new);
                second.split(window, axis, new);
            }
        }
    }

    /// Gives the window's room to its sibling.
    fn remove(&mut self, window: WindowId) {
        let Layout::Split { first, second, .. } = self else {
            return;
        };
        if **first == Layout::Window(window) {
            *self = mem::replace(second, Layout::Window(window));
        } else if **second == Layout::Window(window) {
            *self = mem::replace(first, Layout::Window(window));
        } else {
            first.remove(window);
            second.remove(window);
        }
    }
}

/// `ratio` moved so each window of both sides keeps `min` of the split; `None` if there
/// is no room for that.
fn clamp_ratio(ratio: f32, first: &Layout, second: &Layout, axis: Axis, min: f32) -> Option<f32> {
    let low = first.stacked(axis) as f32 * min;
    let high = 1.0 - second.stacked(axis) as f32 * min;
    (low <= high).then(|| ratio.clamp(low, high))
}

/// A window but the selected one, which is the first buffer and `Editor::viewport`; its
/// point is kept by its buffer, which edits move.
pub(crate) struct Window {
    pub id: WindowId,
    pub buffer: BufferId,
    pub viewport: Viewport,
}

pub(crate) struct Windows {
    pub layout: Layout,
    /// Most recently selected first; the first is the selected window.
    pub recent: Vec<WindowId>,
    pub others: Vec<Window>,
    next_id: u32,
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            layout: Layout::Window(WindowId(0)),
            recent: vec![WindowId(0)],
            others: Vec::new(),
            next_id: 1,
        }
    }
}

impl Windows {
    pub fn selected(&self) -> WindowId {
        self.recent[0]
    }

    pub fn other(&self, id: WindowId) -> Option<&Window> {
        self.others.iter().find(|window| window.id == id)
    }

    pub fn other_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.others.iter_mut().find(|window| window.id == id)
    }

    /// The buffers the windows but the selected one show.
    pub fn other_buffers(&self) -> impl Iterator<Item = BufferId> {
        self.others.iter().map(|window| window.buffer)
    }

    /// The windows but the selected one that show `buffer`.
    pub fn showing(&self, buffer: BufferId) -> impl Iterator<Item = WindowId> {
        let showing = self
            .others
            .iter()
            .filter(move |window| window.buffer == buffer);
        showing.map(|window| window.id)
    }

    /// Splits the selected window; the new one comes second, and second most recent.
    pub fn split(&mut self, axis: Axis, buffer: BufferId, viewport: Viewport) -> WindowId {
        let new = WindowId(self.next_id);
        self.next_id += 1;
        let selected = self.selected();
        self.layout.split(selected, axis, new);
        self.recent.insert(1, new);
        self.others.push(Window {
            id: new,
            buffer,
            viewport,
        });
        new
    }

    /// Removes a window that is not selected.
    pub fn remove(&mut self, id: WindowId) -> Option<Window> {
        let index = self.others.iter().position(|window| window.id == id)?;
        self.layout.remove(id);
        self.recent.retain(|&window| window != id);
        Some(self.others.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_and_removing_keeps_the_order_on_screen() {
        let mut windows = Windows::default();
        let buffer = BufferId::default();
        let first = windows.selected();
        let below = windows.split(Axis::Rows, buffer, Viewport::default());
        let right = windows.split(Axis::Columns, buffer, Viewport::default());
        assert_eq!(windows.layout.windows(), [first, right, below]);
        assert_eq!(windows.recent, [first, right, below]);
        windows.remove(right);
        assert_eq!(windows.layout.windows(), [first, below]);
        windows.remove(below);
        assert_eq!(windows.layout, Layout::Window(first));
    }

    fn ratio(layout: &Layout) -> f32 {
        let Layout::Split { ratio, .. } = layout else {
            panic!("a split");
        };
        *ratio
    }

    /// `first` on the left of a column of `top` above `bottom`.
    fn three_windows() -> (Windows, [WindowId; 3]) {
        let mut windows = Windows::default();
        let buffer = BufferId::default();
        let first = windows.selected();
        let right = windows.split(Axis::Columns, buffer, Viewport::default());
        windows.recent.swap(0, 1);
        let bottom = windows.split(Axis::Rows, buffer, Viewport::default());
        (windows, [first, right, bottom])
    }

    #[test]
    fn resizing_moves_the_nearest_split_along_the_axis_within_the_minimum() {
        let (mut windows, [first, top, bottom]) = three_windows();
        let layout = &mut windows.layout;
        assert!(layout.resize(bottom, Axis::Rows, 0.25, 0.1));
        let Layout::Split { second, .. } = &*layout else {
            panic!("a split");
        };
        assert_eq!(ratio(second), 0.25, "top gave bottom a quarter");
        assert_eq!(ratio(layout), 0.5, "the columns stay");
        assert!(layout.resize(top, Axis::Columns, 0.1, 0.1));
        assert!((ratio(layout) - 0.4).abs() < 1e-6, "the right column grew");
        assert!(layout.resize(first, Axis::Columns, 1.0, 0.1));
        assert!((ratio(layout) - 0.9).abs() < 1e-6, "up to the minimum");
        assert!(!layout.resize(first, Axis::Columns, 0.1, 0.1));
        assert!(
            !layout.resize(first, Axis::Rows, 0.1, 0.1),
            "no split along it"
        );
        assert!(
            !layout.resize(top, Axis::Rows, 0.1, 0.6),
            "no room for both"
        );
        assert_eq!(layout.share(top, Axis::Rows), Some(0.25));
    }

    #[test]
    fn setting_a_ratio_counts_splits_depth_first_and_keeps_the_minimum() {
        let (mut windows, _) = three_windows();
        let layout = &mut windows.layout;
        layout.set_ratio(1, 0.95, 0.1);
        let Layout::Split { second, .. } = &*layout else {
            panic!("a split");
        };
        assert_eq!(ratio(second), 0.9);
        layout.set_ratio(0, 0.0, 0.1);
        assert_eq!(ratio(layout), 0.1);
    }

    #[test]
    fn balancing_gives_windows_stacked_along_an_axis_equal_room() {
        let (mut windows, [first, top, _]) = three_windows();
        windows.recent.retain(|&window| window != top);
        windows.recent.insert(0, top);
        let below_top = windows.split(Axis::Rows, BufferId::default(), Viewport::default());
        let layout = &mut windows.layout;
        layout.set_ratio(0, 0.2, 0.0);
        layout.balance();
        assert_eq!(ratio(layout), 0.5);
        for window in layout.windows().into_iter().filter(|&id| id != first) {
            let share = layout.share(window, Axis::Rows).unwrap();
            assert!(
                (share - 1.0 / 3.0).abs() < 1e-6,
                "{window:?} of {below_top:?}"
            );
        }
    }
}
