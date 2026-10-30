use std::mem;
use std::ops::Range;
use std::time::Instant;

use gpui::{
    Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, KeyDownEvent, Pixels,
    Point, UTF16Selection, Window, canvas, prelude::*,
};
use remacs_core::{FrameRequest, Key, KeyOutcome, MessageKind, SELF_INSERT_COMMAND};
use remacs_theme::font;

use super::{CLOSE_KEY, COPY_MESSAGE_KEY, EditorView};
use crate::composition::discard_marked_text;
use crate::frame;
use crate::key_input::{option_is_meta, to_key};
use crate::menu_bar::RunCommand;

/// macOS input methods see only the text being composed, not the buffer: committed text
/// is typed at the cursor, as keys are.
impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.composition.marked.encode_utf16().count();
        Some(UTF16Selection { range: end..end, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = &self.composition.marked;
        (!marked.is_empty()).then(|| 0..marked.encode_utf16().count())
    }

    /// Accepts the marked text as it is, as AppKit asks.
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let marked = mem::take(&mut self.composition.marked);
        if !marked.is_empty() {
            self.commit_text(&marked, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_text(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.marked = text.to_owned();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(self.composition.caret_or(element_bounds))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl EditorView {
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = to_key(&event.keystroke, option_is_meta(self.keyboard.meta)) else {
            return;
        };
        self.hints_open = false;
        // Any key closes the message opened whole; its own keys go no further.
        if mem::take(&mut self.message_open) {
            if Key::parse(COPY_MESSAGE_KEY) == Some(key)
                && let Some(message) = self.editor.message()
            {
                let text = message.text.clone();
                self.editor.kill_ring.push(text);
                self.editor.set_done("Copied");
                cx.stop_propagation();
                return self.after_input(cx);
            }
            if Key::parse(CLOSE_KEY) == Some(key) {
                cx.stop_propagation();
                return cx.notify();
            }
        }
        // A key taken goes no further: its menu item's key equivalent would run it again,
        // and the input method would type an unbound Meta key's character.
        match self.editor.dispatch_key(key) {
            // The input method makes its text, e.g. composed with a dead key before it.
            KeyOutcome::Typed if !event.is_held => {}
            // Repeats, where the input method would offer accents instead.
            KeyOutcome::Typed => {
                self.editor.run_command(SELF_INSERT_COMMAND);
                cx.stop_propagation();
            }
            KeyOutcome::Unhandled if key.modifiers.super_key => {}
            _ => cx.stop_propagation(),
        }
        self.after_input(cx);
    }

    /// Takes text from macOS input methods while the view is focused.
    pub(super) fn render_text_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (view, focus) = (cx.entity(), self.focus_handle.clone());
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, view), cx);
            },
        )
        .absolute()
        .size_full()
    }

    fn commit_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.composition.marked.clear();
        self.editor.type_text(text);
        self.after_input(cx);
    }

    /// Drops the text being composed before a click, a scroll or a menu command moves
    /// where it would go or acts on what it would change, e.g. a list it would filter;
    /// the input method, which sees none of them, is told to drop it too.
    pub(super) fn cancel_composition(&mut self, cx: &mut Context<Self>) {
        if mem::take(&mut self.composition.marked).is_empty() {
            return;
        }
        cx.notify();
        // Later: the input method calls back into the view, which is being updated now.
        cx.defer(|_| discard_marked_text());
    }

    pub(super) fn run_menu_command(
        &mut self,
        action: &RunCommand,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_composition(cx);
        self.hints_open = false;
        self.message_open = false;
        self.editor.run_command(&action.name);
        self.after_input(cx);
    }

    /// Opens the echo area's message whole above it, or closes it.
    pub fn toggle_message_open(&mut self, cx: &mut Context<Self>) {
        self.message_open = !self.message_open && self.editor.message().is_some();
        self.hints_open = false;
        cx.notify();
    }

    /// Lists the view's keys that do not fit under it, or stops.
    pub fn toggle_hints_open(&mut self, cx: &mut Context<Self>) {
        self.hints_open = !self.hints_open;
        self.message_open = false;
        cx.notify();
    }

    /// Lists the keys after a prefix once it has been pending for a while.
    fn track_prefix(&mut self, cx: &mut Context<Self>) {
        if self.editor.pending_keys().is_empty() {
            self.prefix_since = None;
            self.prefix_timer = None;
            return;
        }
        if self.prefix_since.is_some() || !self.bottom.prefix_keys.show {
            return;
        }
        self.prefix_since = Some(Instant::now());
        let delay = self.bottom.prefix_keys.delay();
        self.prefix_timer = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |_, cx| cx.notify());
        }));
    }

    /// Takes an info or done message away once it has been shown for a while, though not
    /// while it is open whole; errors stay until the next key.
    pub(super) fn track_message(&mut self, cx: &mut Context<Self>) {
        if self.message_open {
            return;
        }
        let hide_after = self.bottom.echo.hide_after();
        let due = self
            .editor
            .message()
            .filter(|message| message.kind != MessageKind::Error)
            .zip(hide_after)
            .map(|(message, hide_after)| (message.at, hide_after));
        let Some((at, hide_after)) = due else {
            self.message_timer = None;
            return;
        };
        if self.message_timer.as_ref().is_some_and(|&(timed, _)| timed == at) {
            return;
        }
        let delay = hide_after.saturating_sub(at.elapsed().unwrap_or_default());
        let timer = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |view, cx| {
                view.message_timer = None;
                let current = view.editor.message().is_some_and(|message| message.at == at);
                // Opened whole, it goes once closed.
                if current && !view.message_open {
                    view.editor.clear_message();
                    cx.notify();
                }
            });
        });
        self.message_timer = Some((at, timer));
    }

    pub(super) fn prefix_keys_due(&self) -> bool {
        self.prefix_since.is_some_and(|since| since.elapsed() >= self.bottom.prefix_keys.delay())
    }

    pub(super) fn after_input(&mut self, cx: &mut Context<Self>) {
        if let Some(killed) = self.editor.kill_ring.take_export() {
            cx.write_to_clipboard(ClipboardItem::new_string(killed));
        }
        if self.editor.quit_requested() {
            cx.quit();
        }
        let requests = self.editor.take_frame_requests();
        for request in &requests {
            if let FrameRequest::TextSize(change) = *request {
                let settings = self.zoom;
                self.zoom_with_keys().change(change, font::code().size, settings);
            }
        }
        frame::act(requests, self.window_handle, cx);
        self.window_state.scrolled_sideways = false;
        self.track_prefix(cx);
        self.schedule_refresh(cx);
        cx.notify();
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.editor.refresh();
        // A program may have copied, e.g. a terminal's through OSC 52.
        if let Some(copied) = self.editor.kill_ring.take_export() {
            cx.write_to_clipboard(ClipboardItem::new_string(copied));
        }
        self.schedule_refresh(cx);
        cx.notify();
    }

    /// Refreshes when a plugin asked to, unless an earlier refresh is already due.
    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.editor.take_refresh_at() else {
            return;
        };
        if self.refresh_timer.as_ref().is_some_and(|&(pending, _)| pending <= at) {
            return;
        }
        let timer = cx.spawn(async move |view, cx| {
            let delay = at.saturating_duration_since(Instant::now());
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |view, cx| {
                view.refresh_timer = None;
                view.refresh(cx);
            });
        });
        self.refresh_timer = Some((at, timer));
    }

    pub(super) fn receive_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.editor.receive_clipboard(text);
        }
    }
}
