use std::cell::Cell;
use std::rc::Rc;

use gpui::{Bounds, Div, IntoElement, Pixels, Styled, canvas, div, prelude::*};

/// Text an input method is composing, e.g. a dead key's accent or kana before they are
/// converted, drawn at the cursor until it is committed.
#[derive(Default)]
pub struct Composition {
    pub marked: String,
    /// Where the cursor taking typed text was last drawn, for the input method's
    /// candidate window.
    caret: Rc<Cell<Bounds<Pixels>>>,
}

impl Composition {
    /// Inside the cursor taking typed text: records where it is drawn.
    pub fn track_caret(&self) -> impl IntoElement {
        let caret = self.caret.clone();
        canvas(move |bounds, _, _| caret.set(bounds), |_, _, _, _| {})
            .absolute()
            .size_full()
    }

    /// The cursor's bounds, or `fallback` before one was drawn.
    pub fn caret_or(&self, fallback: Bounds<Pixels>) -> Bounds<Pixels> {
        let caret = self.caret.get();
        if caret.size.width > Pixels::ZERO {
            caret
        } else {
            fallback
        }
    }

    /// The marked text underlined, as macOS draws it; none while nothing is composed.
    pub fn render_marked(&self) -> Option<Div> {
        (!self.marked.is_empty()).then(|| div().flex_none().underline().child(self.marked.clone()))
    }
}
