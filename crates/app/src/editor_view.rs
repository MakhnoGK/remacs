use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, AnyWindowHandle, App, Bounds, ClipboardItem, Context, DispatchPhase, Div,
    FocusHandle, Hsla, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollHandle, ScrollWheelEvent,
    SharedString, Size, StyledText, Subscription, Task, Window, canvas, div, font, point,
    prelude::*, px, rgb, rgba, size, svg,
};

use crate::assets::{EXECUTION_POINT_ICON, LOGPOINT_ICON};
use remacs_core::{
    Action, BreakpointIcon, ChangedLines, Code, CommandError, EchoMessage, Editor, GutterIcon,
    GutterMark, Icon, Key, LineChange, LineExtra, MessageKind, Minibuffer, MouseSelection, Outline,
    Pane, PaneBody, PaneButton, Popup, Preview, ScreenButton, ScreenMouse, ScreenPointer,
    ScreenSize, Scroll, StatusSegment, Tabs, describe_sequence,
};
use remacs_echo::VIEW_MESSAGES;
use remacs_theme::font::CodeFont;
use remacs_theme::{color, font, glass};

use crate::bottom_row::{
    CardKeys, FittedHints, Hints, PrefixKeys, echo_room, fit_hints, render_hints,
    render_hints_list, render_message_card, render_message_echo, render_prefix_echo,
    render_prefix_keys, render_row,
};
use crate::changes_view::render_changes;
use crate::display_line::{DisplayLine, display_width, slice_line};
use crate::frame;
use crate::highlights::{Highlights, combine, face_style, region_style};
use crate::key_input::{to_key, to_modifiers};
use crate::menu_bar::{self, RunCommand};
use crate::minibuffer_view::{is_problem_list, render_minibuffer, render_scrim};
use crate::opening::open;
use crate::outline_view::{
    COMMIT_HEIGHT, OutlineScrolled, fits_history, fitting_commits, fitting_rows, max_outline_lines,
    render_outline, render_toolbar, row_height,
};
use crate::pane_view::{
    PANE_BOTTOM, PANE_GAP, PANE_TINT, PaneChrome, render_button, render_pane, screen_bounds,
};
use crate::popup_view::{DocumentationScroll, Placement, PopupView, render_popup};
use crate::preview_view::render_preview;
use crate::screen_view::{Cell as ScreenCell, cell_at, render_screen, screen_size};
use crate::settings::{
    BottomSettings, CursorHeight, DocumentationSettings, GlassSettings, MouseSettings,
    ProblemsSettings, PromptsSettings, ReadSettings, ScrollSettings, TransitionsSettings,
    UiSettings, report,
};
use crate::smooth_scroll::{Glide, SmoothScroll, wheel_delta};
use crate::table_view::{
    LIST_BOTTOM, ROW_PITCH, ROWS_TOP, TableCursor, fits_preview, render_table,
};
use crate::tabs_view::render_tabs;
use crate::transition::{Retained, Side};
use crate::widgets::{
    Edge, SCROLL_FADE_SIZE, Scrolled, block_cursor, code_font_features, cursor_text_style,
    edge_fade, hsla, icon, modified_dot, over, pill, pill_separator, tone_color,
};

/// Minified files can hold megabytes on one line; lay out only what can be on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
const TITLE_BAR_HEIGHT: f32 = 44.0;
/// Around the window buffer's screen: its sides, and under it, over the hints and pill.
const SCREEN_SIDE: f32 = 24.0;
const SCREEN_BOTTOM: f32 = 64.0;
/// Where chips in the title bar start, after the window's buttons.
const HEADER_TABS_LEFT: f32 = 92.0;
/// Over the status pill; point stays above it.
const FOOTER_FADE_HEIGHT: f32 = 56.0;
const TEXT_TOP: f32 = 64.0;
const ROW_MARGIN: f32 = 8.0;
const ROW_RADIUS: f32 = 6.0;
/// Columns point keeps from a side with text scrolled past it, clear of the side fades;
/// Zed keeps 5, Vim users often set `sidescrolloff` to 5–8.
const SIDE_MARGIN_COLUMNS: usize = 7;
const GUTTER_PADDING: f32 = 20.0;
const MIN_GUTTER_DIGITS: usize = 3;
/// The column of marks left of the line numbers, e.g. where a program can be run from.
const MARKS_WIDTH: f32 = 24.0;
/// Marks of changed lines are centered this far left of the text.
const CHANGE_MARK_CENTER: f32 = 8.5;
const CHANGE_BAR_WIDTH: f32 = 3.0;
/// Between a bar and the rows above and below it.
const CHANGE_BAR_GAP: f32 = 2.0;
const CHANGE_TICK_WIDTH: f32 = 9.0;
const CHANGE_TICK_HEIGHT: f32 = 3.0;
/// Per mode; one popup shows a handful.
const MAX_HIGHLIGHTED_SNIPPETS: usize = 64;
/// While a message is open whole: Emacs's copy and quit keys.
const COPY_MESSAGE_KEY: &str = "M-w";
const CLOSE_KEY: &str = "C-g";

pub struct EditorView {
    editor: Editor,
    focus_handle: FocusHandle,
    cursor_height: CursorHeight,
    mouse: MouseSettings,
    /// While the button pressed over the text is held.
    mouse_selection: Option<MouseSelection>,
    /// Where the mouse was last seen while selecting.
    drag_position: Point<Pixels>,
    /// Scrolls toward the mouse while it is held past the text's top or bottom.
    drag_scroll: Option<Task<()>>,
    /// And the code font they were measured from.
    metrics: Option<(CodeFont, Metrics)>,
    scroll: ScrollSettings,
    /// The window's glass, and the glass it last had; the window server is told only of a
    /// change.
    glass: GlassSettings,
    applied_glass: Option<GlassSettings>,
    /// Where the lines or rows are drawn, following the viewport's first line.
    vertical: SmoothScroll,
    /// Where the text is drawn sideways, following the viewport's first column.
    sideways: SmoothScroll,
    /// The buffer the scrolls follow; another one is shown at once.
    scrolled_buffer: String,
    /// Commits of an outline's history drawn past the first one the plugin shows, and
    /// where the wheel scrolled it to; the plugin moves its first one by whole commits.
    history: Glide,
    history_target: f32,
    /// Moves the plugin's first commit, from the history last drawn.
    history_scroll: Option<Scroll>,
    /// Whether the history was last drawn with commits above and below it.
    history_scrolled: Scrolled,
    /// The text was scrolled sideways by hand, so point may be out of view until the next
    /// input, as in VS Code; following point would undo the scroll.
    scrolled_sideways: bool,
    /// Some drawn line goes on past the right edge, so the text can scroll further right.
    more_on_right: bool,
    documentation_scroll: DocumentationScroll,
    documentation: DocumentationSettings,
    problems: ProblemsSettings,
    prompts: PromptsSettings,
    /// How far the minibuffer input is scrolled sideways.
    input_scroll: ScrollHandle,
    /// Faces of code shown in popups and side panels, by mode and text, parsed when first
    /// drawn.
    code_faces: RefCell<HashMap<&'static str, HashMap<String, Highlights>>>,
    /// The window buffer was last drawn as rows, a table's or an outline's, so input maps
    /// to rows, not text.
    showing_rows: bool,
    /// The refresh plugins asked for after a delay, and when it is due.
    refresh_timer: Option<(Instant, Task<()>)>,
    /// The window buffer was last drawn as a screen.
    showing_screen: bool,
    /// Whether each screen was last drawn with room for marks, by buffer, so it is sized
    /// for it before it is asked for.
    screen_gutters: HashMap<String, bool>,
    /// The screen the mouse was pressed on and the button, while it is held.
    screen_press: Option<(ScreenArea, MouseButton)>,
    /// Rows a screen was scrolled by the wheel, short of a whole one.
    screen_scroll: f32,
    /// The row of a screen whose command mark the mouse is over.
    hovered_mark: Option<usize>,
    bottom: BottomSettings,
    transitions: TransitionsSettings,
    surfaces: Surfaces,
    /// The echo area's message is shown whole above it, until the next key or click.
    message_open: bool,
    /// When the echo area's message came, and the timer that then takes it away.
    message_timer: Option<(SystemTime, Task<()>)>,
    /// The view's keys that do not fit under it are listed, until the next key or click.
    hints_open: bool,
    /// Since when a prefix has been pending, and the timer that then lists its keys.
    prefix_since: Option<Instant>,
    prefix_timer: Option<Task<()>>,
    window_handle: AnyWindowHandle,
    _activation: Subscription,
}

/// What comes and goes over the editor, each kept to be drawn as it goes.
#[derive(Default)]
struct Surfaces {
    /// The echo area's message in its pill.
    echo: Retained<EchoMessage>,
    /// The pane under the text and its frame.
    pane: Retained<(Pane, PaneChrome)>,
    /// Each plugin's popup by the text, in the order of `Editor::popup_slots`.
    popups: Vec<Retained<Popup>>,
    /// The editor hands it over once closed.
    prompt: Retained<Minibuffer>,
    /// Over the bottom row: the echo area's message whole, the keys that do not fit, the
    /// keys after a prefix.
    message: Retained<OpenMessage>,
    keys: Retained<(FittedHints, &'static str)>,
    prefix_keys: Retained<PrefixKeys>,
}

impl Surfaces {
    /// Some surface is on its way in or out, so another frame is due.
    fn are_changing(&self) -> bool {
        self.echo.is_changing()
            || self.pane.is_changing()
            || self.popups.iter().any(Retained::is_changing)
            || self.prompt.is_changing()
            || self.message.is_changing()
            || self.keys.is_changing()
            || self.prefix_keys.is_changing()
    }
}

/// The echo area's message opened whole, and the key that lists them all.
struct OpenMessage {
    message: EchoMessage,
    all_messages: Option<String>,
}

/// A prompt as drawn, over the scrim that dims the editor behind it.
struct PromptSurface {
    surface: AnyElement,
    scrim: AnyElement,
    /// Opened from the status pill, which stays lit above the scrim.
    problem_list: bool,
}

/// Where a screen's cells are drawn, to map the mouse to them.
#[derive(Clone)]
struct ScreenArea {
    buffer: String,
    origin: Point<Pixels>,
    cell: ScreenCell,
    gutter: bool,
    size: ScreenSize,
}

impl ScreenArea {
    /// The mouse at `position`, over the cell under it or the nearest one.
    fn pointer(
        &self,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
        mouse: ScreenMouse,
    ) -> ScreenPointer {
        let (row, column) = cell_at(
            (position.x - self.origin.x).max(px(0.0)),
            (position.y - self.origin.y).max(px(0.0)),
            self.cell,
            self.gutter,
        );
        ScreenPointer {
            row: row.min(self.size.rows.saturating_sub(1)),
            column: column.min(self.size.columns.saturating_sub(1)),
            modifiers: to_modifiers(modifiers),
            mouse,
        }
    }
}

fn screen_button(button: MouseButton) -> ScreenButton {
    match button {
        MouseButton::Right => ScreenButton::Right,
        MouseButton::Middle => ScreenButton::Middle,
        _ => ScreenButton::Left,
    }
}

/// Where the window buffer's screen is drawn, clear of the title bar and the pill.
fn full_screen_bounds(window: Size<Pixels>) -> Bounds<Pixels> {
    let origin = point(px(SCREEN_SIDE), px(TITLE_BAR_HEIGHT));
    Bounds {
        origin,
        size: size(
            window.width - px(2.0 * SCREEN_SIDE),
            window.height - origin.y - px(SCREEN_BOTTOM),
        ),
    }
}

/// Measured from the code font.
#[derive(Clone, Copy)]
struct Metrics {
    char_width: Pixels,
    /// Ascent and descent.
    glyph_height: Pixels,
    digit_width: Pixels,
    /// Of signatures in documentation.
    mono_width: Pixels,
    /// Of code blocks in documentation and code quoted in notes.
    small_mono_width: Pixels,
    /// About, of text in notes.
    ui_char_width: Pixels,
}

impl EditorView {
    pub fn new(
        mut editor: Editor,
        ui: &UiSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let bottom = ui.bottom();
        editor.message_log_mut().set_limit(bottom.echo.history);
        let (notify, mut notifications) = mpsc::unbounded();
        editor.set_notifier(Arc::new(move || {
            let _ = notify.unbounded_send(());
        }));
        cx.spawn(async move |view, cx| {
            while notifications.next().await.is_some() {
                while notifications.try_recv().is_ok() {}
                let refreshed = view.update(cx, |view, cx| view.refresh(cx));
                if refreshed.is_err() {
                    break;
                }
            }
        })
        .detach();
        // Another app may have changed the clipboard while this window was in the background.
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.receive_clipboard(cx);
            }
        });
        let mut view = Self {
            editor,
            focus_handle: cx.focus_handle(),
            cursor_height: ui.cursor.height,
            mouse: ui.mouse,
            mouse_selection: None,
            drag_position: Point::default(),
            drag_scroll: None,
            metrics: None,
            scroll: ui.scroll,
            glass: ui.glass,
            applied_glass: None,
            vertical: SmoothScroll::default(),
            sideways: SmoothScroll::default(),
            scrolled_buffer: String::new(),
            history: Glide::default(),
            history_target: 0.0,
            history_scroll: None,
            history_scrolled: Scrolled::default(),
            scrolled_sideways: false,
            more_on_right: false,
            documentation_scroll: DocumentationScroll::default(),
            documentation: ui.documentation,
            problems: ui.problems,
            prompts: ui.prompts,
            input_scroll: ScrollHandle::new(),
            code_faces: RefCell::default(),
            showing_rows: false,
            refresh_timer: None,
            showing_screen: false,
            screen_gutters: HashMap::new(),
            screen_press: None,
            screen_scroll: 0.0,
            hovered_mark: None,
            bottom,
            transitions: ui.transitions,
            surfaces: Surfaces::default(),
            message_open: false,
            message_timer: None,
            hints_open: false,
            prefix_since: None,
            prefix_timer: None,
            window_handle: window.window_handle(),
            _activation: activation,
        };
        let weak_view = cx.weak_entity();
        cx.on_validate_menu_action(move |action, cx| {
            weak_view
                .upgrade()
                .is_some_and(|view| menu_bar::enabled(&view.read(cx).editor, action))
        });
        menu_bar::set_menus(&view.editor, cx);
        view.receive_clipboard(cx);
        // Lets plugins pick up a file opened from the command line.
        view.editor.refresh();
        view
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Takes settings read again: the view's part, and the plugins' sections.
    pub fn apply_settings(&mut self, read: ReadSettings, cx: &mut Context<Self>) {
        self.cursor_height = read.ui.cursor.height;
        self.mouse = read.ui.mouse;
        self.scroll = read.ui.scroll;
        self.glass = read.ui.glass;
        self.documentation = read.ui.documentation;
        self.problems = read.ui.problems;
        self.prompts = read.ui.prompts;
        self.bottom = read.ui.bottom();
        self.transitions = read.ui.transitions;
        self.editor
            .message_log_mut()
            .set_limit(self.bottom.echo.history);
        self.editor.set_settings(read.settings);
        report(&read.errors, &mut self.editor);
        // Shows the keys `keys.toml` binds now.
        menu_bar::set_menus(&self.editor, cx);
        cx.notify();
    }

    pub fn choose_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.choose_candidate(index);
        self.after_input(cx);
    }

    pub fn focus_candidate(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.focus_candidate(index);
        cx.notify();
    }

    pub fn answer_question(&mut self, yes: bool, cx: &mut Context<Self>) {
        self.editor.answer_question(yes);
        self.after_input(cx);
    }

    pub fn toggle_dialog_option(&mut self, cx: &mut Context<Self>) {
        self.editor.toggle_dialog_option();
        cx.notify();
    }

    pub fn toggle_prompt_option(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.toggle_prompt_option(index);
        cx.notify();
    }

    pub fn next_prompt_choice(&mut self, index: usize, cx: &mut Context<Self>) {
        self.editor.next_prompt_choice(index);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
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

    fn run_menu_command(&mut self, action: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
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
    fn track_message(&mut self, cx: &mut Context<Self>) {
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

    fn prefix_keys_due(&self) -> bool {
        self.prefix_since
            .is_some_and(|since| since.elapsed() >= self.bottom.prefix_keys.delay())
    }

    fn after_input(&mut self, cx: &mut Context<Self>) {
        if let Some(killed) = self.editor.kill_ring.take_export() {
            cx.write_to_clipboard(ClipboardItem::new_string(killed));
        }
        if self.editor.quit_requested() {
            cx.quit();
        }
        frame::act(self.editor.take_frame_requests(), self.window_handle, cx);
        self.scrolled_sideways = false;
        self.track_prefix(cx);
        self.schedule_refresh(cx);
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
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

    fn receive_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.editor.kill_ring.receive(text);
        }
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_height = px(if self.showing_rows {
            ROW_PITCH
        } else {
            font::code().line_height
        });
        let (delta, at_once) = wheel_delta(event, row_height, self.scroll.lines_per_notch);
        // A swipe is never quite straight: it scrolls along its main direction only.
        if delta.x.abs() > delta.y.abs() {
            self.scroll_sideways(-delta.x, at_once, cx);
            return;
        }
        self.scroll_vertically(-delta.y / row_height, at_once);
        self.drag_selection();
        cx.notify();
    }

    /// Scrolls by `rows` lines or rows, fractions included, leaving point where it is
    /// unless it would leave the window; `at_once` skips the glide.
    fn scroll_vertically(&mut self, rows: f32, at_once: bool) {
        let first_line = self.editor.viewport.first_line;
        let last_start = self
            .editor
            .window_buffer()
            .text()
            .line_count()
            .saturating_sub(1);
        let position = (self.vertical.target(first_line) + rows).clamp(0.0, last_start as f32);
        self.editor.scroll(position as isize - first_line as isize);
        let first_line = self.editor.viewport.first_line;
        self.vertical
            .rest_at(first_line, position - first_line as f32, at_once);
    }

    /// Scrolls the text sideways by `delta`, no further right than the drawn lines go,
    /// leaving point where it is.
    fn scroll_sideways(&mut self, delta: Pixels, at_once: bool, cx: &mut Context<Self>) {
        let Some(char_width) = self
            .metrics
            .as_ref()
            .map(|(_, metrics)| metrics.char_width)
            .filter(|_| !self.showing_rows)
        else {
            return;
        };
        let columns = delta / char_width;
        if columns > 0.0 && !self.more_on_right {
            return;
        }
        let viewport = &mut self.editor.viewport;
        let position = (self.sideways.target(viewport.first_column) + columns).max(0.0);
        viewport.first_column = position as usize;
        self.sideways
            .rest_at(viewport.first_column, position.fract(), at_once);
        self.scrolled_sideways = true;
        cx.notify();
    }

    /// Scrolls an outline's history by what the wheel moved, part of a commit included;
    /// the plugin is told as whole commits are scrolled past.
    pub fn scroll_history(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let row_height = px(COMMIT_HEIGHT);
        let (delta, at_once) = wheel_delta(event, row_height, self.scroll.lines_per_notch);
        let mut position = self.history.target() - delta.y / row_height;
        // Nothing is above the newest commit or below the oldest.
        if !self.history_scrolled.above {
            position = position.max(0.0);
        }
        if !self.history_scrolled.below {
            position = position.min(0.0);
        }
        self.history_target = position;
        if at_once {
            self.history.place(position);
        }
        cx.notify();
    }

    /// Glides the history toward where the wheel scrolled it and moves its first commit
    /// past those scrolled over; returns commits drawn past the first, a fraction of one.
    fn advance_history(&mut self, window: &Window, cx: &mut Context<Self>) -> f32 {
        let duration = self.scroll.animation_duration();
        let max_distance = fitting_commits(window.viewport_size().height, false) as f32;
        let shown =
            self.history
                .advance(self.history_target, Instant::now(), duration, max_distance);
        if self.history.is_animating() {
            window.request_animation_frame();
        }
        let whole = shown.floor();
        if whole != 0.0
            && let Some(scroll) = self.history_scroll.clone()
        {
            let rows = whole as isize;
            let action: Action = Rc::new(move |editor| scroll(editor, rows));
            self.run_action(&action, cx);
            self.history.shift(-whole);
            self.history_target -= whole;
        }
        self.history.shown().max(0.0)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        if mem::take(&mut self.message_open) | mem::take(&mut self.hints_open) {
            cx.notify();
        }
        // An open prompt is modal: clicks choose candidates or do nothing. Rows handle
        // their own clicks.
        let Some(metrics) = self
            .metrics
            .as_ref()
            .map(|(_, metrics)| *metrics)
            .filter(|_| self.editor.minibuffer().is_none() && !self.showing_rows)
        else {
            return;
        };
        let Some(line) = self.line_at(event.position.y) else {
            return;
        };
        let offset = self.offset_at(line, event.position.x, metrics);
        let buffer = self.editor.window_buffer_mut();
        let (clicks, extend) = (event.click_count, event.modifiers.shift);
        self.mouse_selection = Some(MouseSelection::press(buffer, offset, clicks, extend));
        self.drag_position = event.position;
        self.scrolled_sideways = false;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some((area, button)) = self.screen_press.clone() {
            let mouse = if event.pressed_button == Some(button) {
                ScreenMouse::Drag
            } else {
                // Let go outside the window.
                self.screen_press = None;
                ScreenMouse::Release {
                    button: screen_button(button),
                }
            };
            let pointer = area.pointer(event.position, &event.modifiers, mouse);
            self.editor.screen_mouse(&area.buffer, pointer);
            cx.notify();
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.release_mouse();
            return;
        }
        self.drag_position = event.position;
        if self.drag_selection() != 0 && self.drag_scroll.is_none() {
            let interval = self.mouse.drag_scroll_interval();
            self.drag_scroll = Some(cx.spawn(async move |view, cx| {
                loop {
                    cx.background_executor().timer(interval).await;
                    let scrolled = view.update(cx, |view, cx| view.scroll_to_drag(cx));
                    if !matches!(scrolled, Ok(true)) {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        match self.screen_press.take() {
            Some((area, button)) if button == event.button => {
                let mouse = ScreenMouse::Release {
                    button: screen_button(button),
                };
                let pointer = area.pointer(event.position, &event.modifiers, mouse);
                self.editor.screen_mouse(&area.buffer, pointer);
            }
            pressed => {
                self.screen_press = pressed;
                if event.button != MouseButton::Left {
                    return;
                }
                self.release_mouse();
            }
        }
        cx.notify();
    }

    fn release_mouse(&mut self) {
        self.mouse_selection = None;
        self.drag_scroll = None;
    }

    /// Scrolls a line toward the mouse held past the text's top or bottom, extending the
    /// selection; false once the mouse is back over the text.
    fn scroll_to_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let direction = self.drag_selection();
        if direction == 0 {
            self.drag_scroll = None;
            return false;
        }
        let first_line = self.editor.viewport.first_line;
        self.editor.scroll(direction);
        if self.editor.viewport.first_line != first_line {
            self.drag_selection();
            cx.notify();
        }
        true
    }

    /// Extends the selection to the mouse, or to the nearest line point can be on without
    /// scrolling; returns which way the text must scroll to reach the mouse.
    fn drag_selection(&mut self) -> isize {
        let (Some(selection), Some((_, metrics))) = (&self.mouse_selection, &self.metrics) else {
            return 0;
        };
        let (selection, metrics) = (selection.clone(), *metrics);
        let viewport = &self.editor.viewport;
        let line_count = self.editor.window_buffer().text().line_count();
        // Above the text counts as the first line, which is above any line point is on
        // with the text scrolled.
        let under = self.line_at(self.drag_position.y).unwrap_or(0);
        let line = viewport.clamp(under, line_count);
        let offset = self.offset_at(line, self.drag_position.x, metrics);
        selection.drag(self.editor.window_buffer_mut(), offset);
        self.scrolled_sideways = false;
        // Past the last line there is nothing more to scroll to.
        let last_line = line_count.saturating_sub(1);
        under.min(last_line).cmp(&line.min(last_line)) as isize
    }

    /// The line drawn at `y` in the window, none above the text.
    fn line_at(&self, y: Pixels) -> Option<usize> {
        let rows = (y - px(TEXT_TOP)) / px(font::code().line_height);
        (rows >= 0.0).then(|| (self.vertical.shown() + rows) as usize)
    }

    /// The offset in the window buffer nearest to `x` on `line`; past the last line, the
    /// end of the text.
    fn offset_at(&self, line: usize, x: Pixels, metrics: Metrics) -> usize {
        let text_left = px(ROW_MARGIN) + self.gutter_width(metrics);
        let columns = (x - text_left) / metrics.char_width + self.sideways.shown();
        let display_column = columns.round().max(0.0) as usize;
        let column = self.display_line(line).0.char_column_at(display_column);
        let text = self.editor.window_buffer().text();
        text.offset_at_char_column(line, column)
    }

    /// The buffer named `name` drawn as a screen in `bounds` over `under`, taking the mouse
    /// over it.
    fn render_screen_in(
        &mut self,
        name: &str,
        bounds: Bounds<Pixels>,
        under: Hsla,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let metrics = self.metrics(window);
        let cell = ScreenCell {
            width: metrics.char_width,
            height: px(font::code().line_height),
            glyph: metrics.glyph_height,
        };
        let padding = self.editor.screen_padding(name);
        let inset = point(px(padding.horizontal), px(padding.vertical));
        let room = size(
            (bounds.size.width - inset.x * 2.0).max(px(0.0)),
            (bounds.size.height - inset.y * 2.0).max(px(0.0)),
        );
        let mut gutter = self.screen_gutters.get(name).copied().unwrap_or(false);
        let fit = |gutter| screen_size(room.width, room.height, cell, gutter);
        let mut screen = self.editor.screen(name, fit(gutter))?;
        if screen.gutter != gutter {
            gutter = screen.gutter;
            self.screen_gutters.insert(name.to_owned(), gutter);
            screen = self.editor.screen(name, fit(gutter))?;
        }
        let area = ScreenArea {
            buffer: name.to_owned(),
            origin: bounds.origin + inset,
            cell,
            gutter,
            size: fit(gutter),
        };
        let mut element = div()
            .id(SharedString::from(format!("screen {name}")))
            .size_full()
            .child(render_screen(
                screen,
                cell,
                inset,
                under,
                self.hovered_mark,
                cx,
            ));
        for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
            let area = area.clone();
            element = element.on_mouse_down(
                button,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&view.focus_handle);
                    view.press_screen(area.clone(), event, cx);
                }),
            );
        }
        let (moved, left, scrolled) = (area.clone(), area.clone(), area);
        let element = element
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button.is_none() {
                    view.point_at_screen(&moved, event.position, &event.modifiers, cx);
                }
            }))
            .on_hover(cx.listener(move |view, hovered: &bool, window, cx| {
                if !hovered {
                    let modifiers = window.modifiers();
                    let pointer = left.pointer(left.origin, &modifiers, ScreenMouse::Leave);
                    if view.editor.screen_mouse(&left.buffer, pointer) {
                        cx.notify();
                    }
                }
            }))
            .on_scroll_wheel(cx.listener(move |view, event: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
                view.scroll_screen(&scrolled, event, cx);
            }));
        Some(element.into_any_element())
    }

    fn press_screen(&mut self, area: ScreenArea, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let mouse = ScreenMouse::Press {
            button: screen_button(event.button),
            clicks: event.click_count,
        };
        let pointer = area.pointer(event.position, &event.modifiers, mouse);
        self.editor.screen_mouse(&area.buffer, pointer);
        self.screen_press = Some((area, event.button));
        self.after_input(cx);
    }

    /// The mouse moved over a screen with no button held.
    fn point_at_screen(
        &mut self,
        area: &ScreenArea,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let pointer = area.pointer(position, modifiers, ScreenMouse::Move);
        if self.editor.screen_mouse(&area.buffer, pointer) {
            cx.notify();
        }
    }

    fn scroll_screen(
        &mut self,
        area: &ScreenArea,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let row_height = area.cell.height;
        let delta = event.delta.pixel_delta(row_height);
        // macOS turns a mouse's wheel sideways while shift is held.
        let delta = if event.modifiers.shift && delta.y == px(0.0) {
            delta.x
        } else {
            delta.y
        };
        let rows = self.screen_scroll - delta / row_height;
        let whole = rows.trunc();
        self.screen_scroll = rows - whole;
        if whole != 0.0 {
            let mouse = ScreenMouse::Wheel {
                rows: whole as isize,
            };
            let pointer = area.pointer(event.position, &event.modifiers, mouse);
            self.editor.screen_mouse(&area.buffer, pointer);
        }
        cx.notify();
    }

    pub fn hover_command_mark(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        if row.is_some() || self.hovered_mark.is_some() {
            self.hovered_mark = row;
            cx.notify();
        }
    }

    /// Chips on the left of the title bar and buttons on its right, for a screen.
    fn render_header_controls(
        &self,
        tabs: Option<Tabs>,
        buttons: Vec<PaneButton>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if tabs.is_none() && buttons.is_empty() {
            return None;
        }
        let room = f32::from(window.viewport_size().width) / 2.0 - HEADER_TABS_LEFT - 120.0;
        let controls = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TITLE_BAR_HEIGHT))
            .children(tabs.map(|tabs| {
                render_tabs(tabs, room, cx)
                    .absolute()
                    .top(px(9.0))
                    .left(px(HEADER_TABS_LEFT))
            }))
            .child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(14.0))
                    .flex()
                    .gap(px(6.0))
                    .font_family(font::UI)
                    .text_size(px(font::STATUS_SIZE))
                    .children(buttons.into_iter().map(|button| render_button(button, cx))),
            );
        Some(controls.into_any_element())
    }

    /// Follows the mouse anywhere, even outside the window, while a selection is dragged.
    fn render_drag_listeners(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.mouse_selection.is_none() && self.screen_press.is_none() {
            return None;
        }
        let view = cx.entity().downgrade();
        let paint = move |_, _, window: &mut Window, _: &mut App| {
            let moved = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = moved.update(cx, |view, cx| view.on_mouse_move(event, cx));
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    let _ = view.update(cx, |view, cx| view.on_mouse_up(event, cx));
                }
            });
        };
        Some(canvas(|_, _, _| {}, paint).absolute().size_full())
    }

    fn metrics(&mut self, window: &Window) -> Metrics {
        let code_font = font::code();
        if let Some((measured, metrics)) = &self.metrics
            && *measured == code_font
        {
            return *metrics;
        }
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(code_font.family.clone()));
        let advance = |size: f32, character| {
            text_system
                .advance(font_id, px(size), character)
                .map_or(px(size * 0.6), |advance| advance.width)
        };
        let size = px(code_font.size);
        let metrics = Metrics {
            char_width: advance(code_font.size, 'm'),
            // The font's descent is negative here, unlike a laid-out line's.
            glyph_height: text_system.ascent(font_id, size)
                + text_system.descent(font_id, size).abs(),
            digit_width: advance(code_font.line_number_size(), '0'),
            mono_width: advance(font::MONO_SIZE, 'm'),
            small_mono_width: advance(font::STATUS_SIZE, 'm'),
            // 'n' is about as wide as a letter of text on average, spaces counted.
            ui_char_width: text_system
                .advance(
                    text_system.resolve_font(&font(font::UI)),
                    px(font::UI_SIZE),
                    'n',
                )
                .map_or(px(font::UI_SIZE * 0.55), |advance| advance.width),
        };
        self.metrics = Some((code_font, metrics));
        metrics
    }

    fn gutter_width(&self, metrics: Metrics) -> Pixels {
        let digits = self
            .editor
            .window_buffer()
            .text()
            .line_count()
            .to_string()
            .len();
        let marks = if self.has_marks_column() {
            px(MARKS_WIDTH)
        } else {
            px(0.0)
        };
        marks + metrics.digit_width * digits.max(MIN_GUTTER_DIGITS) as f32 + px(GUTTER_PADDING)
    }

    /// Some plugin may mark lines of the window buffer, so the gutter makes room for it.
    fn has_marks_column(&self) -> bool {
        self.editor.gutter_marks(0..0).is_some()
    }

    /// Puts point on `line` and runs the command of its gutter mark.
    fn click_gutter_mark(&mut self, line: usize, command: &'static str, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_some() {
            return;
        }
        self.editor.set_point_at(line, 0);
        self.editor.run_command(command);
        self.after_input(cx);
    }

    /// The part of `line` in the drawn columns, and the bytes of the buffer it shows.
    fn display_line(&self, line: usize) -> (DisplayLine, Range<usize>) {
        let text = self.editor.window_buffer().text();
        let range = text.line_range(line).unwrap_or_default();
        let first_column = self.first_drawn_column();
        // One more for the column partly scrolled in.
        let columns = first_column..first_column + self.editor.viewport.width + 1;
        let part = slice_line(text.chunks(range.clone()).flatten().copied(), columns);
        let start = range.start + part.bytes.start;
        let end = range.start + part.bytes.end;
        let visible = start..end.min(start + MAX_RENDERED_LINE_BYTES);
        let continues = part.continues || visible.end < end;
        let source = text.bytes_in(visible.clone());
        let source = String::from_utf8_lossy(&source);
        (DisplayLine::part(&source, part.start, continues), visible)
    }

    /// The display column at the left edge, partly scrolled past.
    fn first_drawn_column(&self) -> usize {
        self.sideways.shown() as usize
    }

    /// The display column of the window buffer's point.
    fn point_display_column(&self) -> usize {
        let buffer = self.editor.window_buffer();
        let text = buffer.text();
        let line_start = text
            .line_range(buffer.point_line())
            .unwrap_or_default()
            .start;
        display_width(text.chunks(line_start..buffer.point()).flatten().copied())
    }

    /// Each decorator's faces on the `visible` bytes, mapped onto their display text.
    fn face_layers(&self, visible: Range<usize>, display_line: &DisplayLine) -> Vec<Highlights> {
        let to_line = |offset: usize| offset.clamp(visible.start, visible.end) - visible.start;
        self.editor
            .decorations(visible.clone())
            .into_iter()
            .map(|spans| {
                spans
                    .into_iter()
                    .map(|span| {
                        let range = to_line(span.range.start)..to_line(span.range.end);
                        (display_line.display_range(range), face_style(span.face))
                    })
                    .collect()
            })
            .collect()
    }

    /// Characters of `line` inside the active region; past the end means the newline too.
    fn region_columns(&self, line: usize) -> Option<Range<usize>> {
        let buffer = self.editor.window_buffer();
        let region = buffer.region()?;
        let text = buffer.text();
        let line_range = text.line_range(line)?;
        if region.start > line_range.end || region.end <= line_range.start {
            return None;
        }
        let column_of =
            |offset: usize| text.char_column(offset.clamp(line_range.start, line_range.end));
        let end = if region.end > line_range.end {
            column_of(line_range.end) + 1
        } else {
            column_of(region.end)
        };
        Some(column_of(region.start)..end)
    }

    /// The row of `line`, its gutter mark in `mark_slot` when the gutter has a column for
    /// marks, and whether the line goes on past its right edge.
    fn render_line(
        &self,
        line: usize,
        metrics: Metrics,
        gutter_width: Pixels,
        mark_slot: Option<Div>,
        extra: Option<LineExtra>,
    ) -> (AnyElement, bool) {
        let (band, note) = extra.map_or((None, None), |extra| (extra.band, extra.note));
        let code_font = font::code();
        let row_height = px(code_font.line_height);
        let buffer = self.editor.window_buffer();
        let is_current = line == buffer.point_line();
        let (mut display_line, visible) = self.display_line(line);
        let mut layers = self.face_layers(visible, &display_line);
        if let Some(columns) = self.region_columns(line) {
            layers.push(vec![(display_line.cell_range(columns), region_style())]);
        }
        let mut cursor = None;
        if is_current {
            let cell = display_line.cursor_cell(buffer.point_column());
            let column = display_line.text[..cell.start].chars().count();
            layers.push(vec![(cell, cursor_text_style())]);
            let height = match self.cursor_height {
                CursorHeight::Line => row_height,
                CursorHeight::Font => metrics.glyph_height.min(row_height),
            };
            cursor = Some(
                block_cursor()
                    .absolute()
                    .left(metrics.char_width * column as f32)
                    .top((row_height - height) / 2.0)
                    .w(metrics.char_width)
                    .h(height),
            );
        }
        let marks_width = if mark_slot.is_some() {
            px(MARKS_WIDTH)
        } else {
            px(0.0)
        };
        let scrolled_past = self.sideways.shown() - display_line.first_column() as f32;
        let (hidden_start, continues) = (display_line.has_hidden_start(), display_line.continues);
        let content = StyledText::new(display_line.text).with_highlights(combine(&layers));
        let number_color = if is_current {
            color::FG_MAIN
        } else {
            color::FG_LINE_NUMBER
        };
        // The text fades into what the row is drawn on.
        let under = if is_current {
            hsla(color::BG_HL_LINE)
        } else {
            rgba(glass::window_tint()).into()
        };

        let row = div()
            .flex()
            .flex_none()
            .h(row_height)
            .mx(px(ROW_MARGIN))
            .rounded(px(ROW_RADIUS))
            .when(is_current, |row| row.bg(rgb(color::BG_HL_LINE)))
            .when_some(band, |row, tone| {
                let band = tone_color(tone, color::FG_MAIN);
                row.bg(rgba((band << 8) | 0x26))
                    .border_1()
                    .border_color(rgba((band << 8) | 0x4d))
            })
            .whitespace_nowrap()
            .line_height(row_height)
            .children(mark_slot.map(|slot| {
                slot.flex_none()
                    .w(px(MARKS_WIDTH))
                    .h(row_height)
                    .flex()
                    .items_center()
                    .justify_center()
            }))
            .child(
                div()
                    .flex_none()
                    .w(gutter_width - marks_width)
                    .pr(px(GUTTER_PADDING))
                    .text_right()
                    .text_size(px(code_font.line_number_size()))
                    .text_color(rgb(number_color))
                    .child((line + 1).to_string()),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    // Clear of the row's rounded end, which a fade would fill.
                    .mr(px(ROW_RADIUS))
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .left(-metrics.char_width * scrolled_past.max(0.0))
                            .text_size(px(code_font.size))
                            // Before the text, so that it is drawn under it.
                            .children(cursor)
                            .child(div().flex().child(content).children(note.map(|note| {
                                div()
                                    .ml(metrics.char_width * 4.0)
                                    .text_size(px(font::STATUS_SIZE))
                                    .text_color(rgb(color::FG_FAINT))
                                    .child(note)
                            }))),
                    )
                    .when(hidden_start, |text| {
                        text.child(edge_fade(Edge::Left, SCROLL_FADE_SIZE, under))
                    })
                    .when(continues, |text| {
                        text.child(edge_fade(Edge::Right, SCROLL_FADE_SIZE, under))
                    }),
            );
        (row.into_any_element(), continues)
    }

    /// Popups next to the text they belong to, e.g. completions under the word or the
    /// documentation of a symbol.
    fn render_popups(
        &mut self,
        metrics: Metrics,
        gutter_width: Pixels,
        window: &Window,
    ) -> Vec<AnyElement> {
        let window_size = window.viewport_size();
        let line_height = px(font::code().line_height);
        let transition = self.transitions.popup();
        let slots: Vec<Option<Popup>> = self.editor.popup_slots().collect();
        self.documentation_scroll.update(slots.iter().flatten());
        let popups = &mut self.surfaces.popups;
        popups.resize_with(slots.len(), Retained::default);
        let now = Instant::now();
        for (retained, popup) in popups.iter_mut().zip(slots) {
            retained.update(popup, now, transition.duration);
        }
        let code_faces = |code: &Code| self.faces_of(code);
        let view = PopupView {
            code_faces: &code_faces,
            scroll: &self.documentation_scroll.handle,
            mono_width: metrics.mono_width,
            small_mono_width: metrics.small_mono_width,
            ui_char_width: metrics.ui_char_width,
            max_width: px(self.documentation.max_width),
            max_height: px(self.documentation.max_height),
            notes_max_width: px(self.problems.max_width),
            notes_max_height: px(self.problems.max_height),
        };
        self.surfaces
            .popups
            .iter()
            .filter_map(|retained| retained.shown(transition))
            .filter_map(|shown| {
                let popup = shown.content;
                let line = self.editor.window_buffer().text().line_of(popup.anchor);
                let row = line as f32 - self.vertical.shown();
                if row < 0.0 {
                    return None;
                }
                let line_top = px(TEXT_TOP) + line_height * row;
                if line_top >= window_size.height {
                    return None;
                }
                let (display_line, visible) = self.display_line(line);
                let offset = popup.anchor.clamp(visible.start, visible.end) - visible.start;
                let byte = display_line.display_range(offset..offset).start;
                let column = (display_line.first_column()
                    + display_line.text[..byte].chars().count())
                    as f32
                    - self.sideways.shown();
                let anchor_left =
                    px(ROW_MARGIN) + gutter_width + metrics.char_width * column.max(0.0);
                let placement = Placement {
                    anchor_left,
                    line_top,
                    line_bottom: line_top + line_height,
                    area_top: px(TITLE_BAR_HEIGHT),
                    window_width: window_size.width,
                    window_height: window_size.height,
                };
                Some(shown.look.apply(render_popup(popup, placement, &view)))
            })
            .collect()
    }

    /// The pane under the text as it comes, is, or goes; `current` is the one shown now.
    fn render_pane(
        &mut self,
        current: Option<(Pane, PaneChrome)>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let transition = self.transitions.pane();
        self.surfaces
            .pane
            .update(current, Instant::now(), transition.duration);
        let (pane, _) = self.surfaces.pane.shown(transition)?.content;
        // A screen is sized to one place a frame: the window's, if it is drawn there too.
        let screen = match &pane.body {
            PaneBody::Screen(name)
                if !(self.showing_screen && self.editor.window_buffer().name() == name) =>
            {
                Some((name.clone(), pane.height))
            }
            _ => None,
        };
        let screen = screen.and_then(|(name, height)| {
            let bounds = screen_bounds(window.viewport_size(), height);
            let under = over(PANE_TINT, glass::window_tint());
            self.render_screen_in(&name, bounds, under, window, cx)
        });
        let shown = self.surfaces.pane.shown(transition)?;
        let (pane, chrome) = shown.content;
        let element = render_pane(pane, chrome, screen, cx)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        Some(shown.look.apply(element))
    }

    /// The prompt and the scrim behind it as they come, are, or go.
    fn render_prompt(&mut self, cx: &mut Context<Self>) -> Option<PromptSurface> {
        let transition = self.transitions.prompt();
        let closed = self.editor.take_closed_minibuffer();
        let open = self.editor.minibuffer().is_some();
        let retained = &mut self.surfaces.prompt;
        retained.update_owned(open, closed, Instant::now(), transition.duration);
        let minibuffer = self.editor.minibuffer().or(retained.leaving())?;
        let problem_list = is_problem_list(minibuffer);
        let side = if problem_list {
            Side::Below
        } else {
            Side::Above
        };
        let look = retained.look(transition.coming_from(side))?;
        let scrim = render_scrim(minibuffer.dialog().is_some());
        Some(PromptSurface {
            surface: look.apply(render_minibuffer(
                minibuffer,
                &self.input_scroll,
                px(self.prompts.input_margin),
                cx,
            )),
            scrim: look.in_place().apply(scrim),
            problem_list,
        })
    }

    /// Parses `code` the first time it is asked for; documentation's code is parsed as it
    /// was wrapped to fit, which only the drawing knows.
    fn faces_of(&self, code: &Code) -> Highlights {
        let Some(mode) = code.mode else {
            return Highlights::default();
        };
        let mut code_faces = self.code_faces.borrow_mut();
        let known = code_faces.entry(mode).or_default();
        if let Some(faces) = known.get(&code.text) {
            return faces.clone();
        }
        if known.len() >= MAX_HIGHLIGHTED_SNIPPETS {
            known.clear();
        }
        let faces: Highlights = self
            .editor
            .highlight_code(mode, &code.text)
            .into_iter()
            .map(|span| (span.range, face_style(span.face)))
            .collect();
        known.insert(code.text.clone(), faces.clone());
        faces
    }

    fn render_preview(&mut self, preview: Preview) -> Div {
        render_preview(preview, &|code| self.faces_of(code))
    }

    fn render_header(&self) -> Div {
        let buffer = self.editor.window_buffer();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TITLE_BAR_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .font_family(font::UI)
            .text_size(px(font::UI_SIZE))
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(buffer.name().to_owned()),
            )
            .child(div().text_color(rgb(color::FG_DIM)).child("— Remacs"))
    }

    /// The gutter's mark slots for `lines`, mapped by line, when some plugin may mark the
    /// window buffer; a click on a mark runs its command.
    fn render_mark_slots(
        &self,
        lines: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Option<HashMap<usize, Div>> {
        let marks = self.editor.gutter_marks(lines.clone())?;
        let mut by_line: HashMap<usize, Vec<GutterMark>> = HashMap::new();
        for mark in marks {
            by_line.entry(mark.line).or_default().push(mark);
        }
        let empty_click = self.editor.gutter_click();
        let slots = lines
            .map(|line| {
                let mut marks = by_line.remove(&line).unwrap_or_default();
                marks.sort_by_key(|mark| mark.icon);
                // A run mark gives way to a breakpoint; the execution point goes over both.
                let execution = marks
                    .iter()
                    .position(|mark| mark.icon == GutterIcon::ExecutionPoint)
                    .map(|index| marks.remove(index));
                let shown = marks.pop();
                let command = shown
                    .as_ref()
                    .map(|mark| mark.command)
                    .or(empty_click)
                    .or(execution.as_ref().map(|mark| mark.command));
                let slot = div()
                    .relative()
                    .children(shown.map(|mark| gutter_glyph(mark.icon)))
                    .children(execution.map(|_| {
                        div()
                            .absolute()
                            .left(px(4.0))
                            .top(px(5.0))
                            .child(gutter_glyph(GutterIcon::ExecutionPoint))
                    }));
                let slot = match command {
                    Some(command) => slot.cursor_pointer().on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            view.click_gutter_mark(line, command, cx);
                        }),
                    ),
                    None => slot,
                };
                (line, slot)
            })
            .collect();
        Some(slots)
    }

    /// The window buffer's lines from the viewport on, with the popups placed over them.
    fn render_text(
        &mut self,
        reserved: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Vec<AnyElement>) {
        let metrics = self.metrics(window);
        // Point stays above the footer fade; lines under it are drawn but not counted.
        let line_height = px(font::code().line_height);
        let text_height = window.viewport_size().height - px(TEXT_TOP) - reserved;
        let clear_rows = ((text_height - px(FOOTER_FADE_HEIGHT)) / line_height)
            .floor()
            .max(1.0) as usize;
        let drawn_rows = (text_height / line_height).ceil() as usize;
        self.editor.viewport.set_height(clear_rows);
        // Only the top fade is over lines point can be on.
        self.editor.viewport.margins = (1, 0);
        let gutter_width = self.gutter_width(metrics);
        let text_width =
            window.viewport_size().width - px(2.0 * ROW_MARGIN + ROW_RADIUS) - gutter_width;
        self.editor.viewport.width = (text_width / metrics.char_width).floor().max(1.0) as usize;
        self.editor.viewport.column_margin = SIDE_MARGIN_COLUMNS;
        if !self.scrolled_sideways {
            let point_column = self.point_display_column();
            self.editor.viewport.ensure_column_visible(point_column);
        }

        let top = self.advance_scroll(clear_rows, window);
        let first_line = top as usize;
        let line_count = self.editor.window_buffer().text().line_count();
        // One more for the line partly scrolled in at the bottom.
        let last_line = (first_line + drawn_rows + 1).min(line_count);
        let popups = self.render_popups(metrics, gutter_width, window);
        let mut mark_slots = self.render_mark_slots(first_line..last_line, cx);
        let mut extras: HashMap<usize, LineExtra> = self
            .editor
            .line_extras(first_line..last_line)
            .into_iter()
            .map(|extra| (extra.line, extra))
            .collect();
        let (lines, continued): (Vec<_>, Vec<_>) = (first_line..last_line)
            .map(|line| {
                let slot = mark_slots.as_mut().and_then(|slots| slots.remove(&line));
                let extra = extras.remove(&line);
                self.render_line(line, metrics, gutter_width, slot, extra)
            })
            .unzip();
        self.more_on_right = continued.contains(&true);
        let text_left = px(ROW_MARGIN) + gutter_width;
        let changes = self
            .editor
            .line_changes(first_line..last_line)
            .into_iter()
            .filter_map(|changed| render_line_change(changed, first_line..last_line, text_left));
        let glass = rgba(glass::window_tint()).into();
        let lines = div()
            .relative()
            .top(-line_height * top.fract())
            .flex()
            .flex_col()
            .children(lines)
            .children(changes);
        let body = div()
            .absolute()
            .top(px(TEXT_TOP))
            .left_0()
            .right_0()
            .bottom(reserved)
            .overflow_hidden()
            .child(lines)
            .when(top > 0.0, |body| {
                // Over the top margin's line.
                body.child(edge_fade(Edge::Top, line_height.into(), glass))
            })
            .when(first_line + clear_rows < line_count, |body| {
                body.child(edge_fade(Edge::Bottom, FOOTER_FADE_HEIGHT, glass))
            })
            .into_any_element();
        (body, popups)
    }

    /// Moves where the content is drawn toward the viewport, `screen_rows` tall, and
    /// returns where it is drawn from; another buffer is shown at once. Asks for frames
    /// while it glides.
    fn advance_scroll(&mut self, screen_rows: usize, window: &Window) -> f32 {
        let viewport = self.editor.viewport;
        let name = self.editor.window_buffer().name();
        if self.scrolled_buffer != name {
            self.scrolled_buffer = name.to_owned();
            self.vertical.jump(viewport.first_line);
            self.sideways.jump(viewport.first_column);
            self.history.place(0.0);
            self.history_target = 0.0;
        }
        let now = Instant::now();
        let duration = self.scroll.animation_duration();
        let top = self
            .vertical
            .advance(viewport.first_line, now, duration, screen_rows as f32);
        self.sideways
            .advance(viewport.first_column, now, duration, viewport.width as f32);
        if self.vertical.is_animating() || self.sideways.is_animating() {
            window.request_animation_frame();
        }
        top
    }

    pub fn run_command(&mut self, name: &str, cx: &mut Context<Self>) {
        self.editor.run_command(name);
        self.after_input(cx);
    }

    /// Opens what Finder or the Dock hand over, the last one shown.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let action: Action = Rc::new(move |editor| open(editor, &path).map_err(CommandError));
            self.editor.run_action(&action);
        }
        self.after_input(cx);
    }

    pub fn run_action(&mut self, action: &Action, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_none() {
            self.editor.run_action(action);
            self.after_input(cx);
        }
    }

    /// Selects a table row; a double click also runs the table's `activate` command.
    pub fn click_row(
        &mut self,
        line: usize,
        click_count: usize,
        activate: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.editor.minibuffer().is_some() {
            return;
        }
        self.editor.set_point_at(line, 0);
        if click_count == 2 {
            self.editor.run_command(activate);
        }
        self.after_input(cx);
    }

    /// Selects an outline's heading and runs `toggle`, folding or unfolding it.
    pub fn click_heading(&mut self, line: usize, toggle: &'static str, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_some() {
            return;
        }
        self.editor.set_point_at(line, 0);
        self.editor.run_command(toggle);
        self.after_input(cx);
    }

    /// The window buffer's outline in the rows that fit `height`, the viewport set to them.
    fn render_outline(
        &mut self,
        outline_lines: usize,
        height: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, Hints, Option<AnyElement>)> {
        let history_shown = self.advance_history(window, cx);
        let mut outline = self.editor.outline(
            self.editor.viewport.first_line..self.editor.viewport.first_line + outline_lines,
        )?;
        self.history_scroll = outline
            .history
            .as_ref()
            .map(|history| history.scroll.clone());
        let fit = |outline: &Outline| {
            let has_banner = outline.banner.is_some();
            fitting_rows(&outline.rows, outline.facts.len(), has_banner, height)
        };
        let mut fitting = fit(&outline);
        let point_line = self.editor.window_buffer().point_line();
        let shown_from = self.editor.viewport.first_line;
        self.editor.viewport.set_height(fitting);
        self.editor.viewport.margins = (1, 1);
        self.editor.show_point();
        if self.editor.viewport.first_line != shown_from {
            let first_line = self.editor.viewport.first_line;
            outline = self
                .editor
                .outline(first_line..first_line + outline_lines)?;
            fitting = fit(&outline);
        }
        self.editor.viewport.set_height(fitting);
        let top = self.advance_scroll(fitting, window);
        let first_line = top as usize;
        if first_line != self.editor.viewport.first_line {
            outline = self
                .editor
                .outline(first_line..first_line + outline_lines)?;
            fitting = fit(&outline);
        }
        let mut scrolled = OutlineScrolled {
            rows: Scrolled {
                above: top > 0.0,
                below: outline.rows.len() > fitting,
            },
            rows_shift: row_height(&outline.rows, 0) * top.fract(),
            history: Scrolled::default(),
            history_shift: 0.0,
        };
        // One more for the row partly scrolled in at the bottom.
        outline.rows.truncate(fitting + 1);
        if !fits_history(window.viewport_size().width) {
            outline.history = None;
            outline.conflict = None;
        }
        let has_banner = outline.banner.is_some();
        if let Some(history) = &mut outline.history {
            let fitting = fitting_commits(height, has_banner);
            self.history_scrolled = Scrolled {
                above: history.scrolled,
                below: history.commits.len() > fitting,
            };
            // With the oldest commit in view, part of one scrolled past glides back.
            if !self.history_scrolled.below {
                self.history_target = self.history_target.min(0.0);
            }
            scrolled.history = Scrolled {
                above: history.scrolled || history_shown > 0.0,
                ..self.history_scrolled
            };
            scrolled.history_shift = COMMIT_HEIGHT * history_shown;
            history.commits.truncate(fitting + 1);
        }
        let hints = outline.hints;
        let toolbar = std::mem::take(&mut outline.toolbar);
        let toolbar = (!toolbar.is_empty()).then(|| render_toolbar(toolbar, cx).into_any_element());
        let faces = |code: &Code| self.faces_of(code);
        let body = render_outline(outline, first_line, point_line, scrolled, &faces, cx)
            .into_any_element();
        Some((body, hints, toolbar))
    }

    /// A plugin's group of the status pill, e.g. problem counts; a click runs its command.
    /// One without a command is a plain label, like the mode.
    fn render_segment(segment: StatusSegment, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = match (segment.active, segment.command, segment.code) {
            (true, ..) => color::FG_MAIN,
            (false, Some(_), _) | (false, None, true) => color::FG_KBD,
            (false, None, false) => color::FG_FAINT,
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(24.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .when(segment.active, |button| button.bg(rgba(glass::SELECTION)))
            .font_family(if segment.code {
                font::mono()
            } else {
                font::UI.into()
            })
            .text_color(rgb(text_color))
            .when_some(segment.command, |button, command| {
                button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        view.run_command(command, cx);
                    }),
                )
            })
            .children(segment.items.into_iter().map(|item| {
                let item_color = tone_color(item.tone, text_color);
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .text_color(rgb(item_color))
                    .children(item.icon.map(|item_icon| icon(item_icon, item_color)))
                    .child(item.text)
            }))
    }

    /// The row along the bottom, and what opens from it over the view: the message whole,
    /// the keys that do not fit, the keys after a prefix.
    fn render_bottom_row(
        &mut self,
        hints: Hints,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Vec<AnyElement>) {
        let settings = self.bottom;
        let fitted = fit_hints(hints, settings.hints.show, settings.hints.side_room, window);
        let key_for = |editor: &Editor, command: &str| {
            editor
                .keymap()
                .sequences_for(command)
                .min_by_key(|keys| keys.len())
                .map(describe_sequence)
        };
        let whole_key = key_for(&self.editor, VIEW_MESSAGES);
        self.track_message(cx);
        let transition = self.transitions.popup().coming_from(Side::Below);
        let message = self.editor.message().cloned();
        let echo_pill = &mut self.surfaces.echo;
        echo_pill.update(message, Instant::now(), transition.duration);
        let pending = self.editor.pending_keys();
        // Keys typed show at once, in place of the message.
        let echo = if !pending.is_empty() {
            Some(render_prefix_echo(pending).into_any_element())
        } else {
            echo_pill.shown(transition).map(|shown| {
                let pill = render_message_echo(
                    shown.content,
                    echo_room(&fitted, window),
                    self.message_open,
                    whole_key.as_deref(),
                    window,
                    cx,
                );
                shown.look.apply_in_flow(pill)
            })
        };
        let listed = self.hints_open || self.editor.key_hints_listed();
        let hints_pill = render_hints(&fitted, listed, cx);
        let row = render_row(echo, hints_pill, self.render_status(cx).into_any_element());

        let message = self
            .editor
            .message()
            .filter(|_| self.message_open)
            .map(|message| OpenMessage {
                message: message.clone(),
                all_messages: whole_key,
            });
        let keys = (listed && !fitted.listed().is_empty()).then(|| {
            let title = match self.editor.shown_pane() {
                Some(pane) if self.pane_keys_listed() => pane,
                _ => self.editor.window_buffer().mode(),
            };
            (fitted, title)
        });
        let prefix_keys = (self.prefix_keys_due() && !pending.is_empty())
            .then(|| PrefixKeys::new(pending, self.editor.prefix_continuations()));
        (
            row.into_any_element(),
            self.render_cards(message, keys, prefix_keys, window),
        )
    }

    /// Whether the keys listed are a shown pane's: always under text, under rows only
    /// while it has them, since theirs are what the rows are for.
    fn pane_keys_listed(&self) -> bool {
        !self.showing_screen && (!self.showing_rows || self.editor.pane_focused())
    }

    /// Over the bottom row, rising from it, as they come, are, or go.
    fn render_cards(
        &mut self,
        message: Option<OpenMessage>,
        keys: Option<(FittedHints, &'static str)>,
        prefix_keys: Option<PrefixKeys>,
        window: &Window,
    ) -> Vec<AnyElement> {
        let transition = self.transitions.popup().coming_from(Side::Below);
        let (now, duration) = (Instant::now(), transition.duration);
        let surfaces = &mut self.surfaces;
        surfaces.message.update(message, now, duration);
        surfaces.keys.update(keys, now, duration);
        surfaces.prefix_keys.update(prefix_keys, now, duration);
        let settings = self.bottom;
        let message = surfaces.message.shown(transition).map(|shown| {
            let card = shown.content;
            let keys = CardKeys {
                all_messages: card.all_messages.as_deref(),
                copy: COPY_MESSAGE_KEY,
                close: CLOSE_KEY,
            };
            let lines = settings.echo.lines;
            shown
                .look
                .apply(render_message_card(&card.message, lines, keys, window))
        });
        let keys = surfaces.keys.shown(transition).and_then(|shown| {
            let (fitted, title) = shown.content;
            render_hints_list(fitted, title).map(|list| shown.look.apply(list))
        });
        let prefix_keys = surfaces.prefix_keys.shown(transition).map(|shown| {
            let columns = settings.prefix_keys.columns;
            let card = render_prefix_keys(shown.content, columns, CLOSE_KEY, window);
            shown.look.apply(card)
        });
        message.into_iter().chain(keys).chain(prefix_keys).collect()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buffer = self.editor.window_buffer();
        let position = format!("{}:{}", buffer.point_line() + 1, buffer.point_column() + 1);
        let mut segments = self
            .editor
            .status_segments()
            .into_iter()
            .flat_map(|segment| {
                [
                    Self::render_segment(segment, cx).into_any_element(),
                    pill_separator().into_any_element(),
                ]
            })
            .collect::<Vec<_>>();
        if self.showing_screen {
            // A screen shows neither position nor mode after the segments.
            segments.pop();
        }

        pill()
            .flex_none()
            .children(segments)
            // The selected row is the position; a screen has none.
            .when(!self.showing_rows && !self.showing_screen, |status| {
                status
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(8.0))
                            .font_family(font::mono())
                            .when(buffer.is_modified(), |position| {
                                position.child(modified_dot())
                            })
                            .child(position),
                    )
                    .child(pill_separator())
            })
            .when(!self.showing_screen, |status| {
                status.child(
                    div()
                        .pl(px(8.0))
                        .pr(px(10.0))
                        .font_family(font::UI)
                        .child(buffer.mode()),
                )
            })
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&format!("{} — Remacs", self.editor.window_buffer().name()));
        if self.applied_glass != Some(self.glass) {
            window.set_background_appearance(self.glass.window_background());
            window.set_background_blur_radius(self.glass.window_blur());
            self.applied_glass = Some(self.glass);
        }
        let window_name = self.editor.window_buffer().name().to_owned();
        self.showing_screen = self.editor.is_screen(&window_name);
        let shown_pane = (!self.showing_screen).then(|| self.editor.pane()).flatten();
        let reserved = shown_pane
            .as_ref()
            .map_or(px(0.0), |pane| px(pane.height + PANE_BOTTOM + PANE_GAP));
        // Tables and outlines are laid out in the window above the pane.
        let rows_area_height =
            window.viewport_size().height - (reserved - px(LIST_BOTTOM)).max(px(0.0));
        let first_line = self.editor.viewport.first_line;
        let table_rows = ((rows_area_height - px(ROWS_TOP) - px(LIST_BOTTOM)) / px(ROW_PITCH))
            .floor()
            .max(1.0) as usize;
        // One row more than fits is partly scrolled in; another tells whether there are
        // rows below.
        let table = (!self.showing_screen)
            .then(|| self.editor.table(first_line..first_line + table_rows + 2))
            .flatten();
        let outline = match table {
            Some(_) => None,
            None if self.showing_screen => None,
            None => {
                let lines = max_outline_lines(rows_area_height);
                self.render_outline(lines, rows_area_height, window, cx)
            }
        };
        self.showing_rows = table.is_some() || outline.is_some();
        let mut toolbar = None;
        let mut text_shown = false;
        let (body, hints, popups) = match (table, outline) {
            _ if self.showing_screen => {
                let controls = self.editor.text_controls().unwrap_or_default();
                toolbar = self.render_header_controls(controls.tabs, controls.buttons, window, cx);
                let bounds = full_screen_bounds(window.viewport_size());
                let body = self
                    .render_screen_in(
                        &window_name,
                        bounds,
                        rgba(glass::window_tint()).into(),
                        window,
                        cx,
                    )
                    .map_or_else(
                        || div().into_any_element(),
                        |screen| {
                            div()
                                .absolute()
                                .left(bounds.origin.x)
                                .top(bounds.origin.y)
                                .w(bounds.size.width)
                                .h(bounds.size.height)
                                .child(screen)
                                .into_any_element()
                        },
                    );
                (body, controls.hints, Vec::new())
            }
            (None, Some((body, hints, outline_toolbar))) => {
                toolbar = outline_toolbar;
                (above_pane(body, rows_area_height), hints, Vec::new())
            }
            (Some(mut table), _) => {
                self.editor.viewport.set_height(table_rows);
                self.editor.viewport.margins = (1, 1);
                let top = self.advance_scroll(table_rows, window);
                let first_line = top as usize;
                if first_line != self.editor.viewport.first_line
                    && let Some(shown) = self.editor.table(first_line..first_line + table_rows + 2)
                {
                    table = shown;
                }
                let scrolled = Scrolled {
                    above: top > 0.0,
                    below: table.rows.len() > table_rows + usize::from(top.fract() > 0.0),
                };
                table.rows.truncate(table_rows + 1);
                let buffer = self.editor.window_buffer();
                let cursor = TableCursor {
                    line: buffer.point_line(),
                    column: buffer.point_column(),
                    regions: (first_line..first_line + table.rows.len())
                        .map(|line| self.region_columns(line))
                        .collect(),
                };
                let hints = table.hints;
                let fits = fits_preview(window.viewport_size().width);
                let side = match (table.changes.take(), table.preview.take()) {
                    _ if !fits => None,
                    (Some(changes), _) => Some(render_changes(changes, cx)),
                    (None, preview) => preview.map(|preview| self.render_preview(preview)),
                };
                let body = render_table(table, first_line, scrolled, top.fract(), cursor, side, cx);
                (above_pane(body, rows_area_height), hints, Vec::new())
            }
            (None, None) => {
                let (body, popups) = self.render_text(reserved, window, cx);
                let controls = self.editor.text_controls().unwrap_or_default();
                if !controls.toolbar.is_empty() {
                    toolbar = Some(render_toolbar(controls.toolbar, cx).into_any_element());
                }
                text_shown = true;
                (body, controls.hints, popups)
            }
        };
        let hints = match &shown_pane {
            Some(pane) if self.pane_keys_listed() => pane.hints,
            _ => hints,
        };
        let pane = shown_pane.map(|shown| {
            let chrome = PaneChrome {
                switch: self.editor.pane_switch(),
                shown: self.editor.shown_pane().unwrap_or_default(),
                focused: self.editor.pane_focused(),
                window_width: window.viewport_size().width,
            };
            (shown, chrome)
        });
        // Popups belong to the text; drawn again, they come anew.
        if !text_shown {
            self.surfaces.popups.clear();
        }
        let pane = self.render_pane(pane, window, cx);
        let prompt = self.render_prompt(cx);
        // gpui scrolls the input to the cursor only once it has laid it out.
        if self.editor.minibuffer().is_some() && self.input_scroll.bounds().size.width == px(0.0) {
            window.request_animation_frame();
        }
        let (row, row_popups) = self.render_bottom_row(hints, window, cx);
        // The status pill stays lit above the scrim while the list it opened is shown.
        let (row_below_scrim, row_above_scrim) = match &prompt {
            Some(prompt) if prompt.problem_list => (None, Some(row)),
            _ => (Some(row), None),
        };
        let (scrim, prompt) = prompt.map_or((None, None), |prompt| {
            (Some(prompt.scrim), Some(prompt.surface))
        });
        if self.surfaces.are_changing() {
            window.request_animation_frame();
        }

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::run_menu_command))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgba(glass::window_tint()))
            .font_family(font::mono())
            .font_features(code_font_features())
            .text_color(rgb(color::FG_MAIN))
            .line_height(px(font::LINE_HEIGHT))
            .child(body)
            .children(pane)
            .children(row_below_scrim)
            .children(scrim)
            .child(self.render_header().children(toolbar))
            .children(popups)
            .children(row_popups)
            .children(row_above_scrim)
            .children(prompt)
            .children(self.render_drag_listeners(cx))
    }
}

/// Rows laid out in the top `height` of the window, clear of the pane under them.
fn above_pane(rows: impl IntoElement, height: Pixels) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(height)
        .child(rows)
        .into_any_element()
}

/// A gutter mark as drawn: run triangles and execution arrows are icons, breakpoints dots.
fn gutter_glyph(mark: GutterIcon) -> AnyElement {
    let dot = || div().flex_none().size(px(12.0)).rounded_full();
    match mark {
        GutterIcon::Run => icon(Icon::Run, color::VC_ADDED).into_any_element(),
        GutterIcon::ExecutionPoint => svg()
            .path(EXECUTION_POINT_ICON)
            .w(px(15.0))
            .h(px(12.0))
            .text_color(rgb(color::WARNING))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Plain) => {
            dot().bg(rgb(color::ERROR)).into_any_element()
        }
        GutterIcon::Breakpoint(BreakpointIcon::Conditional) => dot()
            .bg(rgb(color::ERROR))
            .flex()
            .items_center()
            .justify_center()
            .font_family(font::mono())
            .text_size(px(9.0))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(rgb(color::FG_MAIN))
            .child("?")
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Log) => svg()
            .path(LOGPOINT_ICON)
            .size(px(12.0))
            .text_color(rgb(color::ERROR))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Disabled) => dot()
            .border(px(1.5))
            .border_color(rgb(color::FG_DIM))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Unverified) => dot()
            .border(px(1.5))
            .border_dashed()
            .border_color(rgb(color::ERROR))
            .opacity(0.85)
            .into_any_element(),
    }
}

/// A bar in the gutter beside added or modified lines, or a tick between the lines where
/// some were removed; `shown` are the drawn lines, `text_left` where their text starts.
fn render_line_change(
    changed: ChangedLines,
    shown: Range<usize>,
    text_left: Pixels,
) -> Option<AnyElement> {
    let line_height = px(font::code().line_height);
    let top = |line: usize| line_height * (line - shown.start) as f32;
    let mark = if changed.lines.is_empty() {
        div()
            .top(top(changed.lines.start) - px(CHANGE_TICK_HEIGHT / 2.0))
            .left(text_left - px(CHANGE_MARK_CENTER + CHANGE_TICK_WIDTH / 2.0))
            .w(px(CHANGE_TICK_WIDTH))
            .h(px(CHANGE_TICK_HEIGHT))
    } else {
        let lines = changed.lines.start.max(shown.start)..changed.lines.end.min(shown.end);
        if lines.is_empty() {
            return None;
        }
        div()
            .top(top(lines.start) + px(CHANGE_BAR_GAP))
            .left(text_left - px(CHANGE_MARK_CENTER + CHANGE_BAR_WIDTH / 2.0))
            .w(px(CHANGE_BAR_WIDTH))
            .h(line_height * lines.len() as f32 - px(2.0 * CHANGE_BAR_GAP))
    };
    let color = match changed.change {
        LineChange::Added => color::VC_ADDED_MARK,
        LineChange::Modified => color::VC_MODIFIED_MARK,
        LineChange::Removed => color::VC_REMOVED_MARK,
    };
    Some(
        mark.absolute()
            .rounded(px(2.0))
            .bg(rgba(color))
            .into_any_element(),
    )
}
