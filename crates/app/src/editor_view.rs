use std::cell::RefCell;
use std::collections::HashMap;
use std::iter;
use std::mem;
use std::ops::{Range, RangeInclusive};
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
    Action, Axis, BreakpointIcon, Buffer, ChangedLines, Code, CommandError, Cursor, EchoMessage,
    Editor, Face, FrameRequest, GutterIcon, GutterMark, Icon, Key, Layout, LineChange, LineExtra,
    MessageKind, Minibuffer, MouseSelection, Outline, Pane, PaneBody, PaneButton, Popup, Preview,
    ScreenButton, ScreenMouse, ScreenPointer, ScreenSize, Scroll, Span, StatusItem, StatusSegment,
    Tabs, TextLabel, WindowId, describe_sequence,
};
use remacs_echo::VIEW_MESSAGES;
use remacs_files::{QUIT, WindowsSettings};
use remacs_frame::TEXT_CONFIGURED;
use remacs_theme::font::CodeFont;
use remacs_theme::{Appearance, color, font, glass};

use crate::bottom_row::{
    CardKeys, FittedHints, Hints, PrefixKeys, echo_room, fit_hints, render_hints,
    render_hints_list, render_message_card, render_message_echo, render_prefix_echo,
    render_prefix_keys, render_row,
};
use crate::changes_view::render_changes;
use crate::display_line::{DisplayLine, display_width, slice_line};
use crate::frame;
use crate::highlights::{Highlights, combine, face_style, region_style, swatch_style};
use crate::key_input::{to_key, to_modifiers};
use crate::menu_bar::{self, RunCommand};
use crate::minibuffer_view::{draws_input, is_problem_list, render_minibuffer, render_scrim};
use crate::opening::open;
use crate::outline_view::{
    COMMIT_HEIGHT, OutlineScrolled, fits_beside, fits_history, fitting_commits, fitting_rows,
    max_outline_lines, render_outline, render_text_side, render_toolbar, row_height, text_side_end,
};
use crate::pane_view::{
    PANE_BOTTOM, PANE_GAP, PaneChrome, render_button, render_pane, screen_bounds,
};
use crate::popup_view::{DocumentationScroll, Placement, PopupView, render_popup};
use crate::preview_view::render_preview;
use crate::screen_view::{Cell as ScreenCell, cell_at, render_screen, screen_size};
use crate::settings::{
    BottomSettings, CursorHeight, DiffSettings, DocumentationSettings, GlassSettings,
    MouseSettings, ProblemsSettings, PromptsSettings, ReadSettings, ScrollSettings,
    SwatchesSettings, TransitionsSettings, UiSettings, ZoomSettings, report,
};
use crate::smooth_scroll::{Glide, SmoothScroll, wheel_delta};
use crate::table_view::{
    LIST_BOTTOM, ROW_PITCH, ROWS_TOP, TableCursor, fits_preview, render_table,
};
use crate::tabs_view::render_tabs;
use crate::text_zoom::TextZoom;
use crate::transition::{Retained, Side};
use crate::widgets::{
    Edge, SCROLL_FADE_SIZE, Scrolled, block_cursor, code_font_features, cursor_text_style,
    edge_fade, hollow_cursor, icon, modified_dot, over, pill, pill_separator, tone_color,
};

/// Minified files can hold megabytes on one line; lay out only what can be on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
const TITLE_BAR_HEIGHT: f32 = 44.0;
/// Around the window buffer's screen: its sides, and under it, over the hints and pill.
const SCREEN_SIDE: f32 = 24.0;
const SCREEN_BOTTOM: f32 = 64.0;
/// Where chips in the title bar start, after the window's buttons.
const HEADER_TABS_LEFT: f32 = 92.0;
/// Where chips start in the title bar of a window clear of the frame's buttons.
const HEADER_TABS_SIDE: f32 = 16.0;
/// Kept clear of chips on the title bar's right, for the terminal's one button.
const HEADER_BUTTONS_WIDTH: f32 = 200.0;
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
    /// The divider between windows being dragged.
    divider_drag: Option<Divider>,
    /// Of the window buffer's text, and the fonts they were measured from.
    metrics: Option<(MeasuredFonts, Metrics)>,
    /// Of screens, at the configured size, and the fonts they were measured from.
    screen_metrics: Option<(MeasuredFonts, Metrics)>,
    zoom: ZoomSettings,
    text_zoom: TextZoom,
    scroll: ScrollSettings,
    /// The window's glass, and the glass it last had; the window server is told only of a
    /// change.
    glass: GlassSettings,
    applied_glass: Option<GlassSettings>,
    /// The theme's appearance macOS last drew its own parts in.
    applied_appearance: Option<Appearance>,
    /// Of the window the editor displays: the selected one, but while another is drawn or
    /// handles the mouse.
    window_state: WindowState,
    /// Whose `window_state` is.
    state_window: WindowId,
    /// The other windows'.
    other_states: HashMap<WindowId, WindowState>,
    documentation_scroll: DocumentationScroll,
    documentation: DocumentationSettings,
    problems: ProblemsSettings,
    diff: DiffSettings,
    prompts: PromptsSettings,
    swatches: SwatchesSettings,
    /// How far the minibuffer input is scrolled sideways.
    input_scroll: ScrollHandle,
    /// The buffer the window title names.
    titled_buffer: String,
    /// Faces of code shown in popups and side panels, by mode and text, parsed when first
    /// drawn.
    code_faces: RefCell<HashMap<&'static str, HashMap<String, Highlights>>>,
    /// The refresh plugins asked for after a delay, and when it is due.
    refresh_timer: Option<(Instant, Task<()>)>,
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

/// What the view keeps of a window between frames (ADR 0073).
#[derive(Default)]
struct WindowState {
    /// Where the window is laid out as if it were the whole frame, its title bar and the
    /// bottom row's room included; `clip` is what of it shows.
    frame: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    /// Its share of the frame above the pane and the bottom row, where the mouse is its.
    area: Bounds<Pixels>,
    /// At the frame's bottom: the room the pane takes under the text, as one frame had.
    reserved: Pixels,
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
    /// Where the text's rows start: past the side column a plugin put beside them.
    text_left: Pixels,
    /// Characters of the line numbers a plugin gives in place of the buffer's.
    number_width: Option<usize>,
    /// The window buffer was last drawn as rows, a table's or an outline's, so input maps
    /// to rows, not text.
    showing_rows: bool,
    /// The window buffer was last drawn as a screen.
    showing_screen: bool,
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

/// Where the window buffer's screen is drawn in a window laid out in `frame`, clear of the
/// title bar, and of the pill or the pane under it.
fn full_screen_bounds(frame: Size<Pixels>, reserved: Pixels) -> Bounds<Pixels> {
    let origin = point(px(SCREEN_SIDE), px(TITLE_BAR_HEIGHT));
    Bounds {
        origin,
        size: size(
            frame.width - px(2.0 * SCREEN_SIDE),
            frame.height - origin.y - px(SCREEN_BOTTOM).max(reserved),
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
        let state_window = editor.selected_window();
        let mut view = Self {
            editor,
            focus_handle: cx.focus_handle(),
            cursor_height: ui.cursor.height,
            mouse: ui.mouse,
            mouse_selection: None,
            drag_position: Point::default(),
            drag_scroll: None,
            divider_drag: None,
            metrics: None,
            screen_metrics: None,
            zoom: ui.zoom,
            text_zoom: TextZoom::default(),
            scroll: ui.scroll,
            glass: ui.glass,
            applied_glass: None,
            applied_appearance: None,
            state_window,
            window_state: WindowState::default(),
            other_states: HashMap::new(),
            documentation_scroll: DocumentationScroll::default(),
            documentation: ui.documentation,
            problems: ui.problems,
            diff: ui.diff,
            prompts: ui.prompts,
            swatches: ui.swatches,
            input_scroll: ScrollHandle::new(),
            titled_buffer: String::new(),
            code_faces: RefCell::default(),
            refresh_timer: None,
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
        self.zoom = read.ui.zoom;
        self.scroll = read.ui.scroll;
        self.glass = read.ui.glass;
        self.documentation = read.ui.documentation;
        self.problems = read.ui.problems;
        self.diff = read.ui.diff;
        self.prompts = read.ui.prompts;
        self.swatches = read.ui.swatches;
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
            self.editor.receive_clipboard(text);
        }
    }

    fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.platform {
            if self
                .text_zoom
                .scroll(event.delta, font::code().size, self.zoom)
            {
                cx.notify();
            }
            return;
        }
        // The wheel scrolls the window under the mouse, selected or not.
        self.in_window_at(event.position, |view| {
            let row_height = px(if view.window_state.showing_rows {
                ROW_PITCH
            } else {
                view.text_font().line_height
            });
            let (delta, at_once) = wheel_delta(event, row_height, view.scroll.lines_per_notch);
            // A swipe is never quite straight: it scrolls along its main direction only.
            if delta.x.abs() > delta.y.abs() {
                view.scroll_sideways(-delta.x, at_once, cx);
                return;
            }
            view.scroll_vertically(-delta.y / row_height, at_once);
            if view.drawing_selected() {
                view.drag_selection();
            }
            cx.notify();
        });
    }

    /// Makes the editor display `window`, and `window_state` its, until
    /// [`EditorView::leave_window`]; the selected window needs neither.
    fn enter_window(&mut self, window: WindowId) {
        self.editor.display_window(window);
        self.switch_state(window);
    }

    fn leave_window(&mut self) {
        self.editor.end_display();
        self.follow_selection();
    }

    /// Runs `act` with the window drawn at `position` entered, if there is one.
    fn in_window_at<R>(&mut self, position: Point<Pixels>, act: impl FnOnce(&mut Self) -> R) -> R {
        let under = iter::once((&self.state_window, &self.window_state))
            .chain(&self.other_states)
            .find(|(_, state)| state.area.contains(&position))
            .map(|(window, _)| *window);
        if let Some(window) = under {
            self.enter_window(window);
        }
        let result = act(self);
        self.leave_window();
        result
    }

    /// Makes `window_state` the selected window's again, after a command or a click
    /// selected another.
    fn follow_selection(&mut self) {
        self.switch_state(self.editor.selected_window());
    }

    fn switch_state(&mut self, window: WindowId) {
        if window == self.state_window {
            return;
        }
        let state = self.other_states.remove(&window).unwrap_or_default();
        let left = mem::replace(&mut self.window_state, state);
        let left_window = mem::replace(&mut self.state_window, window);
        self.other_states.insert(left_window, left);
    }

    /// Whether the window being drawn or handled is the selected one.
    fn drawing_selected(&self) -> bool {
        self.editor.displayed_window() == self.editor.selected_window()
    }

    /// A click on a window not selected selects it, before what was clicked takes it.
    fn select_clicked(&mut self, window: WindowId, cx: &mut Context<Self>) {
        if window != self.editor.selected_window() {
            self.editor.select_window(window);
            self.editor.focus_pane(false);
            self.follow_selection();
            cx.notify();
        }
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
        let position =
            (self.window_state.vertical.target(first_line) + rows).clamp(0.0, last_start as f32);
        self.editor.scroll(position as isize - first_line as isize);
        let first_line = self.editor.viewport.first_line;
        self.window_state
            .vertical
            .rest_at(first_line, position - first_line as f32, at_once);
    }

    /// Scrolls the text sideways by `delta`, no further right than the drawn lines go,
    /// leaving point where it is.
    fn scroll_sideways(&mut self, delta: Pixels, at_once: bool, cx: &mut Context<Self>) {
        let Some(char_width) = self
            .metrics
            .as_ref()
            .map(|(_, metrics)| metrics.char_width)
            .filter(|_| !self.window_state.showing_rows)
        else {
            return;
        };
        let columns = delta / char_width;
        if columns > 0.0 && !self.window_state.more_on_right {
            return;
        }
        let viewport = &mut self.editor.viewport;
        let position =
            (self.window_state.sideways.target(viewport.first_column) + columns).max(0.0);
        viewport.first_column = position as usize;
        self.window_state
            .sideways
            .rest_at(viewport.first_column, position.fract(), at_once);
        self.window_state.scrolled_sideways = true;
        cx.notify();
    }

    /// Scrolls an outline's history by what the wheel moved, part of a commit included;
    /// the plugin is told as whole commits are scrolled past.
    pub fn scroll_history(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        self.in_window_at(event.position, |view| view.scroll_history_here(event, cx));
    }

    fn scroll_history_here(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let row_height = px(COMMIT_HEIGHT);
        let (delta, at_once) = wheel_delta(event, row_height, self.scroll.lines_per_notch);
        let mut position = self.window_state.history.target() - delta.y / row_height;
        // Nothing is above the newest commit or below the oldest.
        if !self.window_state.history_scrolled.above {
            position = position.max(0.0);
        }
        if !self.window_state.history_scrolled.below {
            position = position.min(0.0);
        }
        self.window_state.history_target = position;
        if at_once {
            self.window_state.history.place(position);
        }
        cx.notify();
    }

    /// Glides the history toward where the wheel scrolled it and moves its first commit
    /// past those scrolled over; returns commits drawn past the first, a fraction of one.
    fn advance_history(&mut self, window: &Window, cx: &mut Context<Self>) -> f32 {
        let duration = self.scroll.animation_duration();
        let max_distance = fitting_commits(self.window_state.frame.size.height, false) as f32;
        let target = self.window_state.history_target;
        let shown =
            self.window_state
                .history
                .advance(target, Instant::now(), duration, max_distance);
        if self.window_state.history.is_animating() {
            window.request_animation_frame();
        }
        let whole = shown.floor();
        if whole != 0.0
            && let Some(scroll) = self.window_state.history_scroll.clone()
        {
            let rows = whole as isize;
            let action: Action = Rc::new(move |editor| scroll(editor, rows));
            self.run_action(&action, cx);
            self.window_state.history.shift(-whole);
            self.window_state.history_target -= whole;
        }
        self.window_state.history.shown().max(0.0)
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
        // The window under the mouse was selected as the click went down to it.
        self.follow_selection();
        // An open prompt is modal: clicks choose candidates or do nothing. Rows handle
        // their own clicks.
        let Some(metrics) = self
            .metrics
            .as_ref()
            .map(|(_, metrics)| *metrics)
            .filter(|_| self.editor.minibuffer().is_none() && !self.window_state.showing_rows)
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
        self.window_state.scrolled_sideways = false;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some(divider) = self.divider_drag {
            if event.pressed_button != Some(MouseButton::Left) {
                self.divider_drag = None;
                return;
            }
            let Bounds { origin, size } = divider.bounds;
            let (offset, length) = match divider.axis {
                Axis::Rows => (event.position.y - origin.y, size.height),
                Axis::Columns => (event.position.x - origin.x, size.width),
            };
            self.editor
                .set_split_ratio(divider.split, offset / length, divider.min / length);
            cx.notify();
            return;
        }
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
        if event.button == MouseButton::Left && self.divider_drag.take().is_some() {
            cx.notify();
            return;
        }
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
        self.window_state.scrolled_sideways = false;
        // Past the last line there is nothing more to scroll to.
        let last_line = line_count.saturating_sub(1);
        under.min(last_line).cmp(&line.min(last_line)) as isize
    }

    /// The line drawn at `y` in the frame, none above the text.
    fn line_at(&self, y: Pixels) -> Option<usize> {
        let y = y - self.window_state.frame.origin.y;
        let rows = (y - px(TEXT_TOP)) / px(self.text_font().line_height);
        (rows >= 0.0).then(|| (self.window_state.vertical.shown() + rows) as usize)
    }

    /// The offset in the window buffer nearest to `x` on `line`; past the last line, the
    /// end of the text.
    fn offset_at(&self, line: usize, x: Pixels, metrics: Metrics) -> usize {
        let x = x - self.window_state.frame.origin.x;
        let text_left = self.window_state.text_left + px(ROW_MARGIN) + self.gutter_width(metrics);
        let columns = (x - text_left) / metrics.char_width + self.window_state.sideways.shown();
        let display_column = columns.round().max(0.0) as usize;
        let column = self.display_line(line).0.char_column_at(display_column);
        let text = self.editor.window_buffer().text();
        text.offset_at_char_column(line, column)
    }

    /// The buffer named `name` drawn as a screen in `bounds` over `under`, taking the mouse
    /// over it; `bounds` are within what is drawn at `offset` in the frame.
    fn render_screen_in(
        &mut self,
        name: &str,
        bounds: Bounds<Pixels>,
        offset: Point<Pixels>,
        under: Hsla,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let code_font = font::code();
        let metrics = cached_metrics(&mut self.screen_metrics, code_font.clone(), window);
        let cell = ScreenCell {
            width: metrics.char_width,
            height: px(code_font.line_height),
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
            origin: offset + bounds.origin + inset,
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
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if tabs.is_none() && buttons.is_empty() {
            return None;
        }
        let clip = self.window_state.clip;
        // Only the window in the top left corner has the frame's buttons in its title bar.
        let tabs_left = if clip.origin == Point::default() {
            HEADER_TABS_LEFT
        } else {
            HEADER_TABS_SIDE
        };
        let room = f32::from(clip.size.width) - tabs_left - HEADER_BUTTONS_WIDTH;
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
                    .left(px(tabs_left))
            }))
            .child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(14.0))
                    .flex()
                    .gap(px(6.0))
                    .font_family(font::ui())
                    .text_size(px(font::status_size()))
                    .children(buttons.into_iter().map(|button| render_button(button, cx))),
            );
        Some(controls.into_any_element())
    }

    /// Follows the mouse anywhere, even outside the window, while a selection is dragged.
    fn render_drag_listeners(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.mouse_selection.is_none()
            && self.screen_press.is_none()
            && self.divider_drag.is_none()
        {
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

    /// The font of the window buffer's text: the code font at the zoomed size.
    fn text_font(&self) -> CodeFont {
        self.text_zoom.font(font::code())
    }

    /// Of the window buffer's text.
    fn metrics(&mut self, window: &Window) -> Metrics {
        let text_font = self.text_font();
        cached_metrics(&mut self.metrics, text_font, window)
    }

    fn gutter_width(&self, metrics: Metrics) -> Pixels {
        let digits = self
            .editor
            .window_buffer()
            .text()
            .line_count()
            .to_string()
            .len()
            .max(self.window_state.number_width.unwrap_or(0));
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
        self.window_state.sideways.shown() as usize
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

    /// Each decorator's `decorations` of the `visible` bytes, mapped onto their display text.
    fn face_layers(
        visible: &Range<usize>,
        decorations: Vec<Vec<Span>>,
        display_line: &DisplayLine,
        swatches: SwatchesSettings,
    ) -> Vec<Highlights> {
        let to_line = |offset: usize| offset.clamp(visible.start, visible.end) - visible.start;
        decorations
            .into_iter()
            .map(|spans| {
                spans
                    .into_iter()
                    .filter_map(|span| {
                        let style = match span.face {
                            Face::Swatch(_) if !swatches.inline => return None,
                            Face::Swatch(color) => swatch_style(color, swatches.thickness),
                            face => face_style(face),
                        };
                        let range = to_line(span.range.start)..to_line(span.range.end);
                        Some((display_line.display_range(range), style))
                    })
                    .collect()
            })
            .collect()
    }

    /// Characters of `line` inside the active region; past the end means the newline too.
    fn region_columns(&self, line: usize) -> Option<Range<usize>> {
        let buffer = self.editor.window_buffer();
        region_columns(buffer, buffer.region()?, line)
    }

    /// The row of `line`, shown as `display_line` with the decorators' faces on its
    /// `visible` bytes, its gutter mark in `mark_slot` when the gutter has a column for
    /// marks, `labels` over its text and the part of the `rectangle` region on it, and
    /// whether the line goes on past its right edge.
    #[allow(clippy::too_many_arguments)]
    fn render_line(
        &self,
        line: usize,
        (mut display_line, visible): (DisplayLine, Range<usize>),
        decorations: Vec<Vec<Span>>,
        metrics: Metrics,
        gutter_width: Pixels,
        mark_slot: Option<Div>,
        extra: Option<LineExtra>,
        labels: Vec<TextLabel>,
        rectangle: Option<&(RangeInclusive<usize>, Range<usize>)>,
    ) -> (AnyElement, bool) {
        let LineExtra {
            band,
            fill,
            number,
            note,
            ..
        } = extra.unwrap_or_default();
        let fill_alpha = u32::from(self.diff.band_opacity) * 0xff / 100;
        let fill = fill.map(|tone| (tone_color(tone, color::fg_main()) << 8) | fill_alpha);
        let code_font = self.text_font();
        let row_height = px(code_font.line_height);
        let buffer = self.editor.window_buffer();
        // Another window shows only where its point is, as Emacs does: a hollow cursor, no
        // current line, no region, no other cursors.
        let selected = self.drawing_selected();
        let on_point = line == buffer.point_line();
        let is_current = on_point && selected;
        let mut layers = Self::face_layers(&visible, decorations, &display_line, self.swatches);
        let line_range = buffer.text().line_range(line).unwrap_or_default();
        let cursors = if selected { buffer.cursors() } else { &[] };
        let rectangle = rectangle
            .filter(|_| selected)
            .and_then(|rectangle| rectangle_columns(buffer, rectangle, line, &line_range));
        let regions: Vec<_> = buffer
            .region()
            .filter(|_| selected && !buffer.is_rectangle())
            .into_iter()
            .chain(cursors.iter().filter_map(Cursor::region))
            .filter_map(|region| region_columns(buffer, region, line))
            .chain(rectangle)
            .map(|columns| (display_line.cell_range(columns), region_style()))
            .collect();
        if !regions.is_empty() {
            layers.push(regions);
        }
        let on_line = cursors.partition_point(|cursor| cursor.point < line_range.start)
            ..cursors.partition_point(|cursor| cursor.point <= line_range.end);
        let points = cursors[on_line]
            .iter()
            .map(|cursor| cursor.point)
            .chain(on_point.then(|| buffer.point()));
        let height = match self.cursor_height {
            CursorHeight::Line => row_height,
            CursorHeight::Font => metrics.glyph_height.min(row_height),
        };
        let mut cursor_cells = Vec::new();
        let cursor: Vec<_> = points
            .map(|point| {
                let cell = display_line.cursor_cell(buffer.text().char_column(point));
                let column = display_line.text[..cell.start].chars().count();
                let block = if selected {
                    cursor_cells.push((cell, cursor_text_style()));
                    block_cursor()
                } else {
                    hollow_cursor()
                };
                block
                    .absolute()
                    .left(metrics.char_width * column as f32)
                    .top((row_height - height) / 2.0)
                    .w(metrics.char_width)
                    .h(height)
            })
            .collect();
        layers.push(cursor_cells);
        // Over the characters they stand on, as avy draws them; found from the bytes drawn,
        // as the faces are, not counted from the start of a line that may be megabytes.
        let label_elements: Vec<_> = labels
            .into_iter()
            // At the end too: a blank line's label is on its newline.
            .filter(|label| visible.start <= label.offset && label.offset <= visible.end)
            .map(|label| {
                let at = label.offset - visible.start;
                let shown = display_line.display_range(at..at + 1).start;
                let column = display_line.text[..shown].chars().count();
                div()
                    .absolute()
                    .top_0()
                    .left(metrics.char_width * column as f32)
                    .h(row_height)
                    .bg(rgb(color::accent()))
                    .text_color(rgb(color::bg_main()))
                    .child(label.text)
            })
            .collect();
        let marks_width = if mark_slot.is_some() {
            px(MARKS_WIDTH)
        } else {
            px(0.0)
        };
        let scrolled_past = self.window_state.sideways.shown() - display_line.first_column() as f32;
        let (hidden_start, continues) = (display_line.has_hidden_start(), display_line.continues);
        let content = StyledText::new(display_line.text).with_highlights(combine(&layers));
        let number_color = if is_current {
            color::fg_main()
        } else {
            color::fg_line_number()
        };
        // The text fades into what the row is drawn on.
        let under = if is_current {
            over(glass::line_highlight(), glass::window_tint())
        } else {
            rgba(glass::window_tint()).into()
        };

        let row = div()
            .flex()
            .flex_none()
            .h(row_height)
            .mx(px(ROW_MARGIN))
            // Filled rows run together, like a diff's added lines.
            .when(fill.is_none(), |row| row.rounded(px(ROW_RADIUS)))
            .when_some(fill, |row, fill| row.bg(rgba(fill)))
            .when(is_current, |row| {
                row.bg(match fill {
                    Some(fill) => over(glass::line_highlight(), fill),
                    None => rgba(glass::line_highlight()).into(),
                })
            })
            .when_some(band, |row, tone| {
                let band = tone_color(tone, color::fg_main());
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
                    .child(number.unwrap_or_else(|| (line + 1).to_string())),
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
                                    .text_size(px(font::status_size()))
                                    .text_color(rgb(color::fg_faint()))
                                    .child(note)
                            })))
                            .children(label_elements),
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
    fn render_popups(&mut self, metrics: Metrics, gutter_width: Pixels) -> Vec<AnyElement> {
        let window_size = self.window_state.frame.size;
        let line_height = px(self.text_font().line_height);
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
            completion_swatches: self.swatches.completion,
        };
        self.surfaces
            .popups
            .iter()
            .filter_map(|retained| retained.shown(transition))
            .filter_map(|shown| {
                let popup = shown.content;
                let line = self.editor.window_buffer().text().line_of(popup.anchor);
                let row = line as f32 - self.window_state.vertical.shown();
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
                    - self.window_state.sideways.shown();
                let anchor_left = self.window_state.text_left
                    + px(ROW_MARGIN)
                    + gutter_width
                    + metrics.char_width * column.max(0.0);
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
        screen_owners: &HashMap<String, WindowId>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let transition = self.transitions.pane();
        self.surfaces
            .pane
            .update(current, Instant::now(), transition.duration);
        let (pane, _) = self.surfaces.pane.shown(transition)?.content;
        // A screen is sized to one place a frame: a window's, if one shows it too.
        let screen = match &pane.body {
            PaneBody::Screen(name) if !screen_owners.contains_key(name) => {
                Some((name.clone(), pane.height))
            }
            _ => None,
        };
        let screen = screen.and_then(|(name, height)| {
            let bounds = screen_bounds(window.viewport_size(), height);
            let under = over(glass::pane(), glass::window_tint());
            self.render_screen_in(&name, bounds, Point::default(), under, window, cx)
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
                self.prompts,
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

    /// The window buffer's name in the middle of the title bar, unless chips name it; with
    /// other windows, its own name, bright only in the selected one.
    fn render_header(&self, titled: bool, split: bool) -> Div {
        let buffer = self.editor.window_buffer();
        let header = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TITLE_BAR_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .font_family(font::ui())
            .text_size(px(font::ui_size()));
        if !titled {
            return header;
        }
        if split {
            let selected = self.drawing_selected();
            return header.child(
                div()
                    .when(selected, |name| name.font_weight(gpui::FontWeight::MEDIUM))
                    .text_color(rgb(if selected {
                        color::fg_main()
                    } else {
                        color::fg_dim()
                    }))
                    .child(buffer.name().to_owned()),
            );
        }
        header
            .child(
                div()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(buffer.name().to_owned()),
            )
            .child(div().text_color(rgb(color::fg_dim())).child("— Remacs"))
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Vec<AnyElement>) {
        let metrics = self.metrics(window);
        let (frame, reserved) = (self.window_state.frame.size, self.window_state.reserved);
        // Point stays above the footer fade; lines under it are drawn but not counted.
        let line_height = px(self.text_font().line_height);
        let text_height = frame.height - px(TEXT_TOP) - reserved;
        let clear_rows = ((text_height - px(FOOTER_FADE_HEIGHT)) / line_height)
            .floor()
            .max(1.0) as usize;
        let drawn_rows = (text_height / line_height).ceil() as usize;
        self.editor.viewport.set_height(clear_rows);
        // Only the top fade is over lines point can be on.
        self.editor.viewport.margins = (1, 0);
        let gutter_width = self.gutter_width(metrics);
        let text_width = frame.width
            - self.window_state.text_left
            - px(2.0 * ROW_MARGIN + ROW_RADIUS)
            - gutter_width;
        self.editor.viewport.width = (text_width / metrics.char_width).floor().max(1.0) as usize;
        self.editor.viewport.column_margin = SIDE_MARGIN_COLUMNS;
        if !self.window_state.scrolled_sideways {
            let point_column = self.point_display_column();
            self.editor.viewport.ensure_column_visible(point_column);
        }

        let top = self.advance_scroll(clear_rows, window);
        let first_line = top as usize;
        let line_count = self.editor.window_buffer().text().line_count();
        // One more for the line partly scrolled in at the bottom.
        let last_line = (first_line + drawn_rows + 1).min(line_count);
        // Popups belong to the selected window.
        let popups = if self.drawing_selected() {
            self.render_popups(metrics, gutter_width)
        } else {
            Vec::new()
        };
        let mut mark_slots = self.render_mark_slots(first_line..last_line, cx);
        let mut extras: HashMap<usize, LineExtra> = self
            .editor
            .line_extras(first_line..last_line)
            .into_iter()
            .map(|extra| (extra.line, extra))
            .collect();
        let mut labels: HashMap<usize, Vec<TextLabel>> = HashMap::new();
        let text = self.editor.window_buffer().text();
        for label in self.editor.text_labels(first_line..last_line) {
            labels
                .entry(text.line_of(label.offset))
                .or_default()
                .push(label);
        }
        let shown: Vec<_> = (first_line..last_line)
            .map(|line| self.display_line(line))
            .collect();
        let visible: Vec<_> = shown.iter().map(|(_, visible)| visible.clone()).collect();
        let decorations = self.editor.decorations_by_line(&visible);
        // Once a frame: finding the columns reads the lines of point and mark.
        let buffer = self.editor.window_buffer();
        let rectangle = buffer.is_rectangle().then(|| buffer.rectangle()).flatten();
        let (lines, continued): (Vec<_>, Vec<_>) = (first_line..last_line)
            .zip(shown)
            .zip(decorations)
            .map(|((line, shown), decorations)| {
                let slot = mark_slots.as_mut().and_then(|slots| slots.remove(&line));
                let extra = extras.remove(&line);
                let labels = labels.remove(&line).unwrap_or_default();
                self.render_line(
                    line,
                    shown,
                    decorations,
                    metrics,
                    gutter_width,
                    slot,
                    extra,
                    labels,
                    rectangle.as_ref(),
                )
            })
            .unzip();
        self.window_state.more_on_right = continued.contains(&true);
        let text_left = px(ROW_MARGIN) + gutter_width;
        let changes = self
            .editor
            .line_changes(first_line..last_line)
            .into_iter()
            .filter_map(|changed| {
                render_line_change(changed, first_line..last_line, text_left, line_height)
            });
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
            .left(self.window_state.text_left)
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
        if self.window_state.scrolled_buffer != name {
            self.window_state.scrolled_buffer = name.to_owned();
            self.window_state.vertical.jump(viewport.first_line);
            self.window_state.sideways.jump(viewport.first_column);
            self.window_state.history.place(0.0);
            self.window_state.history_target = 0.0;
        }
        let now = Instant::now();
        let duration = self.scroll.animation_duration();
        let top = self.window_state.vertical.advance(
            viewport.first_line,
            now,
            duration,
            screen_rows as f32,
        );
        self.window_state.sideways.advance(
            viewport.first_column,
            now,
            duration,
            viewport.width as f32,
        );
        if self.window_state.vertical.is_animating() || self.window_state.sideways.is_animating() {
            window.request_animation_frame();
        }
        top
    }

    pub fn run_command(&mut self, name: &str, cx: &mut Context<Self>) {
        self.editor.run_command(name);
        self.after_input(cx);
    }

    /// Whether the app may quit now, for the close button, the Dock and logout: once the
    /// user agreed, or when quitting as ⌘Q does has nothing to ask.
    pub fn may_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.editor.quit_requested() {
            // The gate asks in the minibuffer: an open prompt, its own included, gives way.
            self.editor.cancel_minibuffer();
            self.run_command(QUIT, cx);
        }
        self.editor.quit_requested()
    }

    pub fn show_error(&mut self, text: String, cx: &mut Context<Self>) {
        self.editor.set_error(text);
        cx.notify();
    }

    /// Opens what Finder or the Dock hand over, the last one shown.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let action: Action = Rc::new(move |editor| open(editor, &path).map_err(CommandError));
            self.editor.run_action(&action);
        }
        self.after_input(cx);
    }

    /// Not while another window than the selected one is drawn: hooks would run in it.
    pub fn run_action(&mut self, action: &Action, cx: &mut Context<Self>) {
        if self.editor.minibuffer().is_none() && self.drawing_selected() {
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
        self.window_state.history_scroll = outline
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
        if !fits_history(self.window_state.frame.size.width) {
            outline.history = None;
            outline.conflict = None;
        }
        let has_banner = outline.banner.is_some();
        if let Some(history) = &mut outline.history {
            let fitting = fitting_commits(height, has_banner);
            self.window_state.history_scrolled = Scrolled {
                above: history.scrolled,
                below: history.commits.len() > fitting,
            };
            // With the oldest commit in view, part of one scrolled past glides back.
            if !self.window_state.history_scrolled.below {
                self.window_state.history_target = self.window_state.history_target.min(0.0);
            }
            scrolled.history = Scrolled {
                above: history.scrolled || history_shown > 0.0,
                ..self.window_state.history_scrolled
            };
            scrolled.history_shift = COMMIT_HEIGHT * history_shown;
            history.commits.truncate(fitting + 1);
            // The selected commit is scrolled into sight for the next frame.
            if let Some(selected) = history
                .selected
                .filter(|&selected| fitting > 0 && selected >= fitting)
            {
                let (scroll, rows) = (history.scroll.clone(), (selected + 1 - fitting) as isize);
                let action: Action = Rc::new(move |editor| scroll(editor, rows));
                self.run_action(&action, cx);
            }
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
            (true, ..) => color::fg_main(),
            (false, Some(_), _) | (false, None, true) => color::fg_kbd(),
            (false, None, false) => color::fg_faint(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(24.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .when(segment.active, |button| button.bg(rgba(glass::selection())))
            .font_family(if segment.code {
                font::mono()
            } else {
                font::ui()
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

    /// Whether the keys listed are a shown pane's: always under text, under rows or a
    /// screen only while it has them, since theirs are what those are for.
    fn pane_keys_listed(&self) -> bool {
        let state = &self.window_state;
        self.editor.pane_focused() || (!state.showing_screen && !state.showing_rows)
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
        // Only text zooms; a click gives it the configured size back.
        let zoom = self
            .text_zoom
            .percent(font::code().size)
            .filter(|_| !self.window_state.showing_rows && !self.window_state.showing_screen)
            .map(|percent| StatusSegment {
                items: vec![StatusItem::new(None, format!("{percent}%"))],
                command: Some(TEXT_CONFIGURED),
                active: false,
                code: true,
            });
        let mut segments = self
            .editor
            .status_segments()
            .into_iter()
            .chain(zoom)
            .flat_map(|segment| {
                [
                    Self::render_segment(segment, cx).into_any_element(),
                    pill_separator().into_any_element(),
                ]
            })
            .collect::<Vec<_>>();
        if self.window_state.showing_screen {
            // A screen shows neither position nor mode after the segments.
            segments.pop();
        }

        pill()
            .flex_none()
            .children(segments)
            // The selected row is the position; a screen has none.
            .when(
                !self.window_state.showing_rows && !self.window_state.showing_screen,
                |status| {
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
                },
            )
            .when(!self.window_state.showing_screen, |status| {
                status.child(
                    div()
                        .pl(px(8.0))
                        .pr(px(10.0))
                        .font_family(font::ui())
                        .child(mode_label(
                            buffer.mode(),
                            self.editor.language_at(buffer, buffer.point()),
                        )),
                )
            })
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Setting it also updates the Window menu and lays out the title bar again.
        if self.titled_buffer != self.editor.window_buffer().name() {
            self.titled_buffer = self.editor.window_buffer().name().to_owned();
            window.set_window_title(&format!("{} — Remacs", self.titled_buffer));
        }
        if self.applied_glass != Some(self.glass) {
            window.set_background_appearance(self.glass.window_background());
            window.set_background_blur_radius(self.glass.window_blur());
            self.applied_glass = Some(self.glass);
        }
        let appearance = remacs_theme::appearance();
        if self.applied_appearance != Some(appearance) {
            frame::set_appearance(appearance);
            self.applied_appearance = Some(appearance);
        }
        self.follow_selection();
        let shown_pane = self.editor.pane();
        let reserved = shown_pane
            .as_ref()
            .map_or(px(0.0), |pane| px(pane.height + PANE_BOTTOM + PANE_GAP));
        let frame_size = window.viewport_size();
        let layout = self.editor.layout().clone();
        let windows = layout.windows();
        self.other_states.retain(|other, _| windows.contains(other));
        let split = windows.len() > 1;
        // Windows share what the pane and the bottom row leave.
        let area = Bounds {
            origin: Point::default(),
            size: size(
                frame_size.width,
                frame_size.height - reserved.max(px(FOOTER_FADE_HEIGHT)),
            ),
        };
        let (mut areas, mut dividers) = (Vec::new(), Vec::new());
        lay_out(&layout, area, &mut areas, &mut dividers);
        let settings = self
            .editor
            .find_plugin_state::<WindowsSettings>()
            .cloned()
            .unwrap_or_default();
        let header_height = settings.header_height;
        // The limits are of text: a window also has its title and line numbers.
        let min_height =
            px(self.text_font().line_height) * settings.min_lines as f32 + px(TEXT_TOP);
        let metrics = self.metrics(window);
        let min_width = metrics.char_width * settings.min_columns as f32
            + self.gutter_width(metrics)
            + px(2.0 * ROW_MARGIN + ROW_RADIUS);
        for divider in &mut dividers {
            divider.min = match divider.axis {
                Axis::Rows => min_height,
                Axis::Columns => min_width,
            };
        }
        let screen_owners = self.screen_owners(&windows);
        let selected = self.editor.selected_window();
        let mut bodies = Vec::new();
        let mut headers = Vec::new();
        let mut selected_parts = None;
        for (id, window_area) in areas {
            self.enter_window(id);
            let state = &mut self.window_state;
            state.area = window_area;
            let at_top = window_area.origin.y == px(0.0);
            let at_bottom = window_area.bottom() == area.bottom();
            // A window under another is drawn as one with a title bar, its top hidden
            // under the strip naming it; one over another as one with the bottom row
            // under it, hidden past its bottom.
            let title_height = if at_top {
                px(TITLE_BAR_HEIGHT)
            } else {
                px(header_height)
            };
            let hidden_top = px(TITLE_BAR_HEIGHT) - title_height;
            let clip_bottom = if at_bottom {
                frame_size.height
            } else {
                window_area.bottom()
            };
            state.clip =
                Bounds::from_corners(window_area.origin, point(window_area.right(), clip_bottom));
            let frame_bottom = if at_bottom {
                frame_size.height
            } else {
                window_area.bottom() + px(FOOTER_FADE_HEIGHT)
            };
            state.frame = Bounds::from_corners(
                point(window_area.origin.x, window_area.origin.y - hidden_top),
                point(window_area.right(), frame_bottom),
            );
            state.reserved = if at_bottom { reserved } else { px(0.0) };
            let parts = self.render_window(&screen_owners, window, cx);
            let state = &self.window_state;
            let (clip, frame) = (state.clip, state.frame);
            let header = self
                .render_header(!parts.tabbed, split)
                .children(parts.toolbar);
            bodies.push(
                div()
                    .absolute()
                    .left(clip.origin.x)
                    .top(clip.origin.y)
                    .w(clip.size.width)
                    .h(clip.size.height)
                    .overflow_hidden()
                    // The pane and the bottom row over a window's bottom take the mouse
                    // without blocking it, and with it the header over its top.
                    .capture_any_mouse_down(cx.listener(
                        move |view, event: &MouseDownEvent, _, cx| {
                            if window_area.contains(&event.position) {
                                view.select_clicked(id, cx);
                            }
                        },
                    ))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(-hidden_top)
                            .w(frame.size.width)
                            .h(frame.size.height)
                            .when(id != selected, |body| {
                                body.opacity(settings.inactive_opacity)
                            })
                            .child(parts.body),
                    ),
            );
            // A title bar's contents, centered on the strip that shows.
            headers.push(
                div()
                    .absolute()
                    .left(clip.origin.x)
                    .top(clip.origin.y)
                    .w(clip.size.width)
                    .h(title_height)
                    .overflow_hidden()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(-hidden_top / 2.0)
                            .w(clip.size.width)
                            .h(px(TITLE_BAR_HEIGHT))
                            .child(header),
                    ),
            );
            if id == selected {
                let popups = div()
                    .absolute()
                    .left(frame.origin.x)
                    .top(frame.origin.y)
                    .w(frame.size.width)
                    .h(frame.size.height)
                    .children(parts.popups);
                selected_parts = Some((parts.hints, parts.text_shown, popups));
            }
            self.leave_window();
        }
        let dividers = render_dividers(&dividers, px(settings.divider_grab_width), cx);
        let (hints, text_shown, popups) =
            selected_parts.unwrap_or_else(|| (Hints::default(), false, div()));
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
        let pane = self.render_pane(pane, &screen_owners, window, cx);
        let prompt = self.render_prompt(cx);
        // gpui scrolls the input to the cursor only once it has laid it out.
        if self.editor.minibuffer().is_some_and(draws_input)
            && self.input_scroll.bounds().size.width == px(0.0)
        {
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
            .text_color(rgb(color::fg_main()))
            .line_height(px(font::LINE_HEIGHT))
            .children(bodies)
            .children(dividers)
            .children(pane)
            .children(row_below_scrim)
            .children(scrim)
            .children(headers)
            .child(popups)
            .children(row_popups)
            .children(row_above_scrim)
            .children(prompt)
            .children(self.render_drag_listeners(cx))
    }
}

/// What a window is drawn as, and what of it goes elsewhere: its header's toolbar, its
/// keys for the bottom row, its popups.
struct WindowParts {
    body: AnyElement,
    toolbar: Option<AnyElement>,
    /// Chips name what it shows, in place of its title.
    tabbed: bool,
    hints: Hints,
    popups: Vec<AnyElement>,
    /// Drawn as text, whose popups they are.
    text_shown: bool,
}

impl EditorView {
    /// For each screen windows show, the window that sizes it: the selected one if it
    /// shows it, else the first in order on screen (ADR 0073).
    fn screen_owners(&mut self, windows: &[WindowId]) -> HashMap<String, WindowId> {
        let selected = self.editor.selected_window();
        let mut owners = HashMap::new();
        for &window in windows {
            self.editor.display_window(window);
            let name = self.editor.window_buffer().name();
            if self.editor.is_screen(name) {
                let name = name.to_owned();
                if window == selected {
                    owners.insert(name, window);
                } else {
                    owners.entry(name).or_insert(window);
                }
            }
            self.editor.end_display();
        }
        owners
    }

    /// The window the editor displays, laid out in `window_state.frame`.
    fn render_window(
        &mut self,
        screen_owners: &HashMap<String, WindowId>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> WindowParts {
        let window_name = self.editor.window_buffer().name().to_owned();
        self.window_state.showing_screen = self.editor.is_screen(&window_name);
        let frame_size = self.window_state.frame.size;
        let reserved = self.window_state.reserved;
        // Tables and outlines are laid out in the window above the pane.
        let rows_area_height = frame_size.height - (reserved - px(LIST_BOTTOM)).max(px(0.0));
        let first_line = self.editor.viewport.first_line;
        let table_rows = ((rows_area_height - px(ROWS_TOP) - px(LIST_BOTTOM)) / px(ROW_PITCH))
            .floor()
            .max(1.0) as usize;
        // One row more than fits is partly scrolled in; another tells whether there are
        // rows below.
        let table = (!self.window_state.showing_screen)
            .then(|| self.editor.table(first_line..first_line + table_rows + 2))
            .flatten();
        let outline = match table {
            Some(_) => None,
            None if self.window_state.showing_screen => None,
            None => {
                let lines = max_outline_lines(rows_area_height);
                self.render_outline(lines, rows_area_height, window, cx)
            }
        };
        self.window_state.showing_rows = table.is_some() || outline.is_some();
        let mut toolbar = None;
        let mut tabbed = false;
        let mut text_shown = false;
        self.window_state.text_left = px(0.0);
        self.window_state.number_width = None;
        let (body, hints, popups) = match (table, outline) {
            _ if self.window_state.showing_screen => {
                let controls = self.editor.text_controls().unwrap_or_default();
                tabbed = controls.tabs.is_some();
                toolbar = self.render_header_controls(controls.tabs, controls.buttons, cx);
                let bounds = full_screen_bounds(frame_size, reserved);
                let owner = screen_owners.get(&window_name).copied();
                let body = if owner.is_some_and(|owner| owner != self.editor.displayed_window()) {
                    owner.map(|owner| render_shown_elsewhere(owner, cx))
                } else {
                    let offset = self.window_state.frame.origin;
                    let under = rgba(glass::window_tint()).into();
                    self.render_screen_in(&window_name, bounds, offset, under, window, cx)
                };
                let body = body.map_or_else(
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
                let fits = fits_preview(frame_size.width);
                let side = match (table.changes.take(), table.preview.take()) {
                    _ if !fits => None,
                    (Some(changes), _) => Some(render_changes(changes, cx)),
                    (None, preview) => preview.map(|preview| self.render_preview(preview)),
                };
                let body = render_table(table, first_line, scrolled, top.fract(), cursor, side, cx);
                (above_pane(body, rows_area_height), hints, Vec::new())
            }
            (None, None) => {
                let mut controls = self.editor.text_controls().unwrap_or_default();
                let side_width = self.diff.side_width;
                let side = controls
                    .side
                    .take()
                    .filter(|_| fits_beside(frame_size.width, side_width));
                if side.is_some() {
                    self.window_state.text_left = text_side_end(side_width) - px(ROW_MARGIN);
                }
                self.window_state.number_width = controls.number_width;
                let (body, popups) = self.render_text(window, cx);
                let side = side.map(|side| {
                    let side = render_text_side(side, side_width, rows_area_height, cx);
                    above_pane(side, rows_area_height)
                });
                let body = div()
                    .absolute()
                    .inset_0()
                    .child(body)
                    .children(side)
                    .into_any_element();
                if !controls.toolbar.is_empty() {
                    toolbar = Some(render_toolbar(controls.toolbar, cx).into_any_element());
                }
                text_shown = true;
                (body, controls.hints, popups)
            }
        };
        WindowParts {
            body,
            toolbar,
            tabbed,
            hints,
            popups,
            text_shown,
        }
    }
}

/// The line between the two sides of a split, which dragging moves.
#[derive(Clone, Copy)]
struct Divider {
    /// The split's index, counted depth first, as `Editor::set_split_ratio` takes it.
    split: usize,
    axis: Axis,
    /// The split's.
    bounds: Bounds<Pixels>,
    /// Where the line is along the axis.
    at: Pixels,
    /// What each window keeps along the axis while it is dragged.
    min: Pixels,
}

/// Lays `layout` out in `bounds`, each window's share rounded to whole pixels, splits'
/// dividers depth first.
fn lay_out(
    layout: &Layout,
    bounds: Bounds<Pixels>,
    areas: &mut Vec<(WindowId, Bounds<Pixels>)>,
    dividers: &mut Vec<Divider>,
) {
    match layout {
        Layout::Window(window) => areas.push((*window, bounds)),
        Layout::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let Bounds { origin, size } = bounds;
            let (first_bounds, second_bounds) = match axis {
                Axis::Rows => {
                    let height = (size.height * *ratio).round();
                    (
                        Bounds::new(origin, gpui::size(size.width, height)),
                        Bounds::new(
                            point(origin.x, origin.y + height),
                            gpui::size(size.width, size.height - height),
                        ),
                    )
                }
                Axis::Columns => {
                    let width = (size.width * *ratio).round();
                    (
                        Bounds::new(origin, gpui::size(width, size.height)),
                        Bounds::new(
                            point(origin.x + width, origin.y),
                            gpui::size(size.width - width, size.height),
                        ),
                    )
                }
            };
            let at = match axis {
                Axis::Rows => second_bounds.top(),
                Axis::Columns => second_bounds.left(),
            };
            dividers.push(Divider {
                split: dividers.len(),
                axis: *axis,
                bounds,
                at,
                min: px(0.0),
            });
            lay_out(first, first_bounds, areas, dividers);
            lay_out(second, second_bounds, areas, dividers);
        }
    }
}

/// A 1px line centered in a strip `grab` wide that dragging moves.
fn render_dividers(dividers: &[Divider], grab: Pixels, cx: &mut Context<EditorView>) -> Vec<Div> {
    dividers
        .iter()
        .map(|&divider| {
            let Bounds { origin, size } = divider.bounds;
            let line = div().bg(rgba(glass::ink(0x14)));
            let strip = div().absolute().flex().items_center().justify_center();
            let strip = match divider.axis {
                Axis::Rows => strip
                    .left(origin.x)
                    .top(divider.at - grab / 2.0)
                    .w(size.width)
                    .h(grab)
                    .cursor_row_resize()
                    .child(line.w_full().h(px(1.0))),
                Axis::Columns => strip
                    .left(divider.at - grab / 2.0)
                    .top(origin.y)
                    .w(grab)
                    .h(size.height)
                    .cursor_col_resize()
                    .child(line.h_full().w(px(1.0))),
            };
            strip.occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.divider_drag = Some(divider);
                    cx.notify();
                }),
            )
        })
        .collect()
}

/// In place of a screen another window sizes; a click selects that window.
fn render_shown_elsewhere(owner: WindowId, cx: &mut Context<EditorView>) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(font::ui())
        .text_size(px(font::ui_size()))
        .text_color(rgb(color::fg_dim()))
        .child("Shown in another window")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                view.select_clicked(owner, cx);
            }),
        )
        .into_any_element()
}

/// The code font, and the UI font's family and size.
type MeasuredFonts = (CodeFont, Arc<str>, f32);

/// The metrics of `code_font` and the UI font, measured again only once either changes.
fn cached_metrics(
    cache: &mut Option<(MeasuredFonts, Metrics)>,
    code_font: CodeFont,
    window: &Window,
) -> Metrics {
    let fonts = (code_font, font::ui(), font::ui_size());
    if let Some((measured, metrics)) = cache
        && *measured == fonts
    {
        return *metrics;
    }
    let code_font = &fonts.0;
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
        glyph_height: text_system.ascent(font_id, size) + text_system.descent(font_id, size).abs(),
        digit_width: advance(code_font.line_number_size(), '0'),
        mono_width: advance(font::mono_size(), 'm'),
        small_mono_width: advance(font::status_size(), 'm'),
        // 'n' is about as wide as a letter of text on average, spaces counted.
        ui_char_width: text_system
            .advance(
                text_system.resolve_font(&font(fonts.1.clone())),
                px(fonts.2),
                'n',
            )
            .map_or(px(fonts.2 * 0.55), |advance| advance.width),
    };
    *cache = Some((fonts, metrics));
    metrics
}

/// Characters of `line` inside `region`; past the end means the newline too.
fn region_columns(buffer: &Buffer, region: Range<usize>, line: usize) -> Option<Range<usize>> {
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

/// Characters of `line`, at `line_range`, inside `rectangle`'s lines and columns, none
/// where the line is too short to reach it.
fn rectangle_columns(
    buffer: &Buffer,
    (lines, columns): &(RangeInclusive<usize>, Range<usize>),
    line: usize,
    line_range: &Range<usize>,
) -> Option<Range<usize>> {
    if !lines.contains(&line) {
        return None;
    }
    let line_len = buffer.text().char_column(line_range.end);
    let columns = columns.start.min(line_len)..columns.end.min(line_len);
    (!columns.is_empty()).then_some(columns)
}

/// The buffer's mode, and the language at point when it is another, like `HTML › CSS`.
fn mode_label(mode: &'static str, language: &'static str) -> SharedString {
    if language == mode {
        mode.into()
    } else {
        format!("{mode} › {language}").into()
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
        GutterIcon::Run => icon(Icon::Run, color::vc_added()).into_any_element(),
        GutterIcon::ExecutionPoint => svg()
            .path(EXECUTION_POINT_ICON)
            .w(px(15.0))
            .h(px(12.0))
            .text_color(rgb(color::warning()))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Plain) => {
            dot().bg(rgb(color::error())).into_any_element()
        }
        GutterIcon::Breakpoint(BreakpointIcon::Conditional) => dot()
            .bg(rgb(color::error()))
            .flex()
            .items_center()
            .justify_center()
            .font_family(font::mono())
            .text_size(px(9.0))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(rgb(color::fg_main()))
            .child("?")
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Log) => svg()
            .path(LOGPOINT_ICON)
            .size(px(12.0))
            .text_color(rgb(color::error()))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Disabled) => dot()
            .border(px(1.5))
            .border_color(rgb(color::fg_dim()))
            .into_any_element(),
        GutterIcon::Breakpoint(BreakpointIcon::Unverified) => dot()
            .border(px(1.5))
            .border_dashed()
            .border_color(rgb(color::error()))
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
    line_height: Pixels,
) -> Option<AnyElement> {
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
        LineChange::Added => glass::vc_added_mark(),
        LineChange::Modified => glass::vc_modified_mark(),
        LineChange::Removed => glass::vc_removed_mark(),
    };
    Some(
        mark.absolute()
            .rounded(px(2.0))
            .bg(rgba(color))
            .into_any_element(),
    )
}
