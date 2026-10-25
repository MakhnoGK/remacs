mod chrome;
mod input;
mod mouse;
mod screen;
mod text;
mod windows;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, AnyWindowHandle, Bounds, Context, FocusHandle, MouseButton, MouseDownEvent, Pixels,
    Point, Render, ScrollHandle, Subscription, Task, Window, div, point, prelude::*, px, rgb, rgba,
    size,
};

use remacs_core::{
    Action, Axis, CommandError, EchoMessage, Editor, Minibuffer, MouseSelection, Pane, Popup,
    Scroll, WindowId,
};
use remacs_files::{QUIT, WindowsSettings};
use remacs_theme::font::CodeFont;
use remacs_theme::{Appearance, color, font, glass};

use crate::bottom_row::{FittedHints, Hints, PrefixKeys};
use crate::composition::Composition;
use crate::frame;
use crate::highlights::Highlights;
use crate::menu_bar;
use crate::minibuffer_view::draws_input;
use crate::opening::open;
use crate::pane_view::{PANE_BOTTOM, PANE_GAP, PaneChrome};
use crate::popup_view::DocumentationScroll;
use crate::settings::{
    BottomSettings, CursorHeight, DiffSettings, DocumentationSettings, FoldsSettings,
    GlassSettings, KeyboardSettings, MouseSettings, ProblemsSettings, PromptsSettings,
    ReadSettings, ScrollSettings, SwatchesSettings, TransitionsSettings, UiSettings, ZoomSettings,
    read_settings, report,
};
use crate::smooth_scroll::{Glide, SmoothScroll};
use crate::text_zoom::TextZoom;
use crate::transition::Retained;
use crate::widgets::{Scrolled, code_font_features};
use screen::ScreenArea;
use windows::{lay_out, render_dividers};

const TITLE_BAR_HEIGHT: f32 = 44.0;
/// Over the status pill; point stays above it.
const FOOTER_FADE_HEIGHT: f32 = 56.0;
const TEXT_TOP: f32 = 64.0;
const ROW_MARGIN: f32 = 8.0;
const ROW_RADIUS: f32 = 6.0;
/// While a message is open whole: Emacs's copy and quit keys.
const COPY_MESSAGE_KEY: &str = "M-w";
const CLOSE_KEY: &str = "C-g";

pub struct EditorView {
    editor: Editor,
    focus_handle: FocusHandle,
    cursor_height: CursorHeight,
    keyboard: KeyboardSettings,
    composition: Composition,
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
    /// `ui.toml` names the theme `system`: the theme follows macOS, not macOS the theme.
    follows_system: bool,
    /// The appearance macOS was last told to draw its own parts in, `None` the system's.
    applied_appearance: Option<Option<Appearance>>,
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
    folds: FoldsSettings,
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
    _appearance: Subscription,
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
    /// The lines folds hid in it; folded or unfolded, it is shown at once, rows having
    /// moved under the text without it scrolling.
    scrolled_hidden: usize,
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

/// The code font, and the UI font's family and size.
type MeasuredFonts = (CodeFont, Arc<str>, f32);

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
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                // KWin offers the clipboard only after the window has the keyboard.
                cx.spawn(async move |view, cx| {
                    view.update(cx, |view, cx| view.receive_clipboard(cx))
                })
                .detach();
            }
        });
        // Seen only while following the system, when macOS is left to draw as it is.
        let appearance = cx.observe_window_appearance(window, |view, window, cx| {
            if view.follows_system {
                let system = frame::appearance_of(window.appearance());
                view.apply_settings(read_settings(system), cx);
            }
        });
        let state_window = editor.selected_window();
        let mut view = Self {
            editor,
            focus_handle: cx.focus_handle(),
            cursor_height: ui.cursor.height,
            keyboard: ui.keyboard,
            composition: Composition::default(),
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
            follows_system: ui.theme.follows_system(),
            applied_appearance: None,
            state_window,
            window_state: WindowState::default(),
            other_states: HashMap::new(),
            documentation_scroll: DocumentationScroll::default(),
            documentation: ui.documentation,
            problems: ui.problems,
            diff: ui.diff,
            folds: ui.folds,
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
            _appearance: appearance,
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
        self.follows_system = read.ui.theme.follows_system();
        self.cursor_height = read.ui.cursor.height;
        self.keyboard = read.ui.keyboard;
        self.mouse = read.ui.mouse;
        self.zoom = read.ui.zoom;
        self.scroll = read.ui.scroll;
        self.glass = read.ui.glass;
        self.documentation = read.ui.documentation;
        self.problems = read.ui.problems;
        self.diff = read.ui.diff;
        self.folds = read.ui.folds;
        self.prompts = read.ui.prompts;
        self.swatches = read.ui.swatches;
        self.bottom = read.ui.bottom();
        self.transitions = read.ui.transitions;
        self.editor
            .message_log_mut()
            .set_limit(self.bottom.echo.history);
        self.editor.set_settings(read.settings);
        // Their styles hold the colors of the theme they were drawn with.
        self.code_faces.get_mut().clear();
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
        let appearance = (!self.follows_system).then(remacs_theme::appearance);
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
                let popups = div().absolute().inset_0().children(parts.popups);
                selected_parts = Some((parts.hints, parts.text_shown, popups));
            }
            self.leave_window();
        }
        let grab = px(settings.divider_grab_width);
        let text_bottom = frame_size.height - reserved;
        let dividers = render_dividers(&dividers, grab, (area.bottom(), text_bottom), cx);
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
            // Before any click is handled, anywhere.
            .capture_any_mouse_down(cx.listener(|view, _: &MouseDownEvent, _, cx| {
                view.cancel_composition(cx);
            }))
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
            .child(self.render_text_input(cx))
    }
}
