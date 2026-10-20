use std::mem;
use std::time::Instant;

use gpui::{ClipboardItem, Context, KeyDownEvent, Window};
use remacs_core::{FrameRequest, Key, MessageKind};
use remacs_theme::font;

use super::{CLOSE_KEY, COPY_MESSAGE_KEY, EditorView};
use crate::frame;
use crate::key_input::to_key;
use crate::menu_bar::RunCommand;

impl EditorView {
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = to_key(&event.keystroke) else {
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
        // A key taken goes no further: its menu item's key equivalent would run it again.
        if self.editor.handle_key(key) {
            cx.stop_propagation();
        }
        self.after_input(cx);
    }

    pub(super) fn run_menu_command(
        &mut self,
        action: &RunCommand,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        if self
            .message_timer
            .as_ref()
            .is_some_and(|&(timed, _)| timed == at)
        {
            return;
        }
        let delay = hide_after.saturating_sub(at.elapsed().unwrap_or_default());
        let timer = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(delay).await;
            let _ = view.update(cx, |view, cx| {
                view.message_timer = None;
                let current = view
                    .editor
                    .message()
                    .is_some_and(|message| message.at == at);
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
        self.prefix_since
            .is_some_and(|since| since.elapsed() >= self.bottom.prefix_keys.delay())
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
                self.text_zoom.change(change, font::code().size, self.zoom);
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
        if self
            .refresh_timer
            .as_ref()
            .is_some_and(|&(pending, _)| pending <= at)
        {
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
