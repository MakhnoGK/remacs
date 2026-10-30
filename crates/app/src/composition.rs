use std::cell::Cell;
use std::rc::Rc;

use gpui::{Bounds, Div, IntoElement, Pixels, Styled, canvas, div, prelude::*};
#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
#[cfg(target_os = "macos")]
use objc2::{class, msg_send};

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
        Some(self.caret.get())
            .filter(|caret| caret.size.width > Pixels::ZERO)
            .unwrap_or(fallback)
    }

    /// The marked text underlined, as macOS draws it; none while nothing is composed.
    pub fn render_marked(&self) -> Option<Div> {
        (!self.marked.is_empty()).then(|| div().flex_none().underline().child(self.marked.clone()))
    }
}

/// Has the input method drop what it is composing, e.g. once the view committed it itself.
#[cfg(target_os = "macos")]
pub fn discard_marked_text() {
    // SAFETY: called on the main thread, as every gpui callback is; a nil context, with no
    // text input in focus, is checked before it is sent the message.
    unsafe {
        let context: *mut AnyObject = msg_send![class!(NSTextInputContext), currentInputContext];
        if !context.is_null() {
            let _: () = msg_send![context, discardMarkedText];
        }
    }
}

// shortcut: gpui's Linux input methods cannot be told to drop their text, so a composition
// committed by the view may be typed again; find a way if IME users hit it.
#[cfg(not(target_os = "macos"))]
pub fn discard_marked_text() {}
