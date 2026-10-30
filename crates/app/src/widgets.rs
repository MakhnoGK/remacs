use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, BoxShadow, Context, Div, Font, FontFeatures, FontWeight, HighlightStyle, Hsla,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Rgba, Styled,
    StyledText, Svg, TextRun, Window, canvas, div, fill, linear_color_stop, linear_gradient, point,
    prelude::FluentBuilder, px, rgb, rgba, svg, transparent_black,
};
use remacs_core::{FileState, Icon, LineCounts, ListItem, Mark, Tone, quoted_parts};
use remacs_theme::{color, cursor, font, glass};

use crate::assets::{
    BRANCH_ICON, CHECK_ICON, CHEVRON_DOWN_ICON, ERROR_ICON, FILE_ICON, FOLDER_ICON, INFO_ICON,
    PACKAGE_ICON, PLUS_ICON, PULL_ICON, PUSH_ICON, REMOTE_ICON, RERUN_ICON, RUN_ICON, SCRIPT_ICON,
    STOP_ICON, TERMINAL_ICON, WARNING_ICON,
};
use crate::composition::Composition;
use crate::highlights::{Highlights, slice};

pub fn hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// A run of `len` bytes of text in `font` and `color`.
pub fn text_run(len: usize, font: Font, color: Hsla) -> TextRun {
    TextRun { len, font, color, background_color: None, underline: None, strikethrough: None }
}

/// Interface text whose names, as `quote` marks them, are set in the code font; it wraps
/// as one text, breaking a name too long for a line.
pub fn ui_text(text: &str, weight: FontWeight, color: u32) -> StyledText {
    let mut shown = String::with_capacity(text.len());
    let mut runs = Vec::new();
    for (part, quoted) in quoted_parts(text) {
        let family = if quoted { font::mono() } else { font::ui() };
        shown.push_str(&part);
        let font = Font { weight, ..gpui::font(family) };
        runs.push(text_run(part.len(), font, hsla(color)));
    }
    StyledText::new(shown).with_runs(runs)
}

pub trait GlassBlur: Styled {
    /// Blurs what is under the element by `radius` pixels, unless `ui.toml` turned
    /// blurring off.
    fn glass_blur(self, radius: f32, saturation: f32) -> Self {
        if glass::blurs() { self.backdrop_filter(px(radius), saturation) } else { self }
    }
}

impl<E: Styled> GlassBlur for E {}

/// Shorthands for the style combinations surfaces repeat.
pub trait StyleExt: Styled + Sized {
    /// Text `size` pixels high in `color`.
    fn text(self, size: f32, color: u32) -> Self {
        self.text_size(px(size)).text_color(rgb(color))
    }

    /// Interface text at the status size in `color`.
    fn status_text(self, color: u32) -> Self {
        self.text(font::status_size(), color)
    }

    /// Code-font text at the status size in `color`.
    fn mono_status(self, color: u32) -> Self {
        self.font_family(font::mono()).status_text(color)
    }

    /// Children in a row, centered across it, `gap` pixels apart.
    fn row(self, gap: f32) -> Self {
        self.flex().items_center().gap(px(gap))
    }

    /// Takes the room left over, its text cut short with an ellipsis.
    fn fill_truncated(self) -> Self {
        self.flex_1().min_w_0().truncate()
    }

    /// A one-pixel border in `color`, with alpha.
    fn bordered(self, color: u32) -> Self {
        self.border_1().border_color(rgba(color))
    }

    /// `height` pixels tall, its content `padding` in from its sides.
    fn bar(self, height: f32, padding: f32) -> Self {
        self.h(px(height)).px(px(padding))
    }

    /// Corners rounded by `radius` over a fill of `color`, with alpha.
    fn rounded_fill(self, radius: f32, color: u32) -> Self {
        self.rounded(px(radius)).bg(rgba(color))
    }

    /// In the code font.
    fn mono(self) -> Self {
        self.font_family(font::mono())
    }

    /// In the interface font.
    fn ui(self) -> Self {
        self.font_family(font::ui())
    }

    /// Placed out of the flow, across its parent from side to side.
    fn across(self) -> Self {
        self.absolute().left_0().right_0()
    }

    /// A column of flex children.
    fn column(self) -> Self {
        self.flex().flex_col()
    }

    /// `top` and `bottom` padding, and `x` on either side.
    fn pad(self, top: f32, x: f32, bottom: f32) -> Self {
        self.pt(px(top)).px(px(x)).pb(px(bottom))
    }

    fn fg(self, color: u32) -> Self {
        self.text_color(rgb(color))
    }

    /// A one-pixel line along the top in `color`, with alpha.
    fn top_border(self, color: u32) -> Self {
        self.border_t_1().border_color(rgba(color))
    }
}

impl<E: Styled> StyleExt for E {}

pub trait PressExt: InteractiveElement + Sized {
    /// Runs `action` on the view on a left press, which goes no further.
    fn on_press<V: 'static>(
        self,
        cx: &mut Context<V>,
        action: impl Fn(&mut V, &mut Context<V>) + 'static,
    ) -> Self {
        self.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                action(view, cx);
            }),
        )
    }
}

impl<E: InteractiveElement> PressExt for E {}

/// What the code font may do with its characters; fonts join them through either feature.
pub fn code_font_features() -> FontFeatures {
    if font::code().ligatures {
        FontFeatures::default()
    } else {
        FontFeatures(Arc::new(vec![("calt".into(), 0), ("liga".into(), 0)]))
    }
}

pub const SCROLL_FADE_SIZE: f32 = 32.0;

/// Whether there is more of a list than is shown, before and after it.
#[derive(Clone, Copy, Default)]
pub struct Scrolled {
    pub above: bool,
    pub below: bool,
}

#[derive(Clone, Copy)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// `list`, its content dissolving into `under`, what it is drawn on, at the edges with more
/// beyond them.
pub fn fade_scrolled(list: Div, scrolled: Scrolled, under: Hsla) -> Div {
    let fade = |edge| edge_fade(edge, SCROLL_FADE_SIZE, under);
    list.relative()
        .when(scrolled.above, |list| list.child(fade(Edge::Top)))
        .when(scrolled.below, |list| list.child(fade(Edge::Bottom)))
}

/// `list` moved up by `shift` pixels, its first row partly scrolled past, in a box that
/// takes the room left and clips it.
pub fn scrolled_by(list: Div, shift: f32) -> Div {
    let list = list.relative().top(px(-shift));
    div().flex_1().min_h_0().overflow_hidden().child(list)
}

/// Mixes what its parent drew within `size` of `edge` toward `under`: wholly at the edge,
/// less and less away from it. Unlike a gradient over the content, it lets the glass show.
pub fn edge_fade(edge: Edge, size: f32, under: Hsla) -> impl IntoElement {
    let angle = match edge {
        Edge::Top => 180.0,
        Edge::Bottom => 0.0,
        Edge::Left => 90.0,
        Edge::Right => 270.0,
    };
    let gradient = |from: Hsla, to: Hsla| {
        linear_gradient(angle, linear_color_stop(from, 0.0), linear_color_stop(to, 1.0))
    };
    let amount = gradient(hsla(0x000000), rgba(0x00000000).into());
    // Only Metal mixes (ADR 0038); elsewhere surfaces are opaque, so a gradient over the
    // content looks the same.
    let over_content = gradient(under, under.opacity(0.0));
    let paint = move |bounds, _, window: &mut Window, _: &mut App| {
        if cfg!(target_os = "macos") {
            window.paint_fade(bounds, amount, under);
        } else {
            window.paint_quad(fill(bounds, over_content));
        }
    };
    let fade = canvas(|_, _, _| {}, paint).absolute();
    match edge {
        Edge::Top => fade.left_0().right_0().h(px(size)).top_0(),
        Edge::Bottom => fade.left_0().right_0().h(px(size)).bottom_0(),
        Edge::Left => fade.top_0().bottom_0().w(px(size)).left_0(),
        Edge::Right => fade.top_0().bottom_0().w(px(size)).right_0(),
    }
}

/// `top` drawn over `bottom`, as one color.
pub fn over(top: u32, bottom: u32) -> Hsla {
    let (top, bottom) = (rgba(top), rgba(bottom));
    let alpha = top.a + bottom.a * (1.0 - top.a);
    if alpha == 0.0 {
        return top.into();
    }
    let mix = |top_channel: f32, bottom_channel: f32| {
        (top_channel * top.a + bottom_channel * bottom.a * (1.0 - top.a)) / alpha
    };
    Rgba { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a: alpha }
        .into()
}

/// The icon at `path`, `size` pixels square, in `color`.
pub fn svg_icon(path: &'static str, size: f32, color: u32) -> Svg {
    svg().path(path).size(px(size)).text_color(rgb(color))
}

/// File icons take the text color they stand next to; severity icons keep their own.
pub fn icon(icon: Icon, text_color: u32) -> Svg {
    let (path, size, color) = match icon {
        Icon::File => (FILE_ICON, 16.0, text_color),
        Icon::Directory => (FOLDER_ICON, 16.0, text_color),
        Icon::Error => (ERROR_ICON, 10.0, color::error()),
        Icon::Warning => (WARNING_ICON, 10.0, color::warning()),
        Icon::Info => (INFO_ICON, 10.0, color::info()),
        Icon::Branch => (BRANCH_ICON, 12.0, text_color),
        Icon::Remote => (REMOTE_ICON, 14.0, text_color),
        Icon::Create => (PLUS_ICON, 14.0, text_color),
        Icon::Pull => (PULL_ICON, 12.0, text_color),
        Icon::Push => (PUSH_ICON, 12.0, text_color),
        Icon::Run => (RUN_ICON, 12.0, color::vc_added()),
        Icon::Stop => (STOP_ICON, 12.0, text_color),
        Icon::Rerun => (RERUN_ICON, 12.0, text_color),
        Icon::Package => (PACKAGE_ICON, 12.0, text_color),
        Icon::Script => (SCRIPT_ICON, 12.0, text_color),
        Icon::Terminal => (TERMINAL_ICON, 12.0, text_color),
    };
    svg_icon(path, size, color).flex_none()
}

/// The theme's color for `tone`; a plain one takes `plain`.
pub fn tone_color(tone: Tone, plain: u32) -> u32 {
    match tone {
        Tone::Plain => plain,
        Tone::Accent => color::accent(),
        Tone::Danger => color::danger(),
        Tone::Warning => color::warning(),
        Tone::Faint => color::fg_faint(),
        Tone::Special => color::function(),
        Tone::Success => color::fg_added(),
    }
}

/// The small shadow that lifts controls off what they sit on.
fn lift() -> Vec<BoxShadow> {
    vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)]
}

/// A check for a marked row, a `D` for one flagged for deletion.
pub fn mark_badge(mark: Mark) -> Div {
    let badge = div().row(0.0).flex_none().justify_center().rounded(px(4.0)).shadow(lift());
    let check = || svg_icon(CHECK_ICON, 10.0, color::bg_main());
    match mark {
        Mark::Marked => badge.size(px(14.0)).bg(rgb(color::accent())).child(check()),
        Mark::Flagged => badge
            .size(px(16.0))
            .bg(rgb(color::bg_danger()))
            .mono()
            .text(10.0, color::fg_on_danger())
            .line_height(px(16.0))
            .child("D"),
    }
}

/// The color of a file's version control state; on a selected row a modified one takes
/// the selection's text color.
pub fn state_color(state: FileState, selected: bool) -> u32 {
    match state {
        FileState::Added | FileState::Resolved => color::vc_added(),
        FileState::Modified | FileState::Renamed if selected => color::fg_alt(),
        FileState::Modified | FileState::Renamed => color::vc_modified(),
        FileState::Deleted => color::vc_removed(),
        FileState::Conflicted => color::error(),
        FileState::Untracked => color::fg_dim(),
    }
}

/// A file's version control state as its letter, centered in its slot.
pub fn state_letter(state: Option<FileState>, selected: bool) -> Div {
    let letter = |slot: Div, state: FileState| {
        slot.text_color(rgb(state_color(state, selected))).child(state.letter())
    };
    div()
        .flex_none()
        .text_center()
        .mono()
        .text_size(px(font::status_size()))
        .when_some(state, letter)
}

/// `+24 −3`, the zero side left out.
pub fn line_counts(lines: LineCounts, selected: bool) -> Div {
    let (added, removed) = if selected {
        let added = color::vc_added_on_selection();
        (added, color::vc_removed_on_selection())
    } else {
        (color::vc_added(), color::vc_removed())
    };
    let count = |text: String, text_color| div().text_color(rgb(text_color)).child(text);
    let added = (lines.added > 0).then(|| count(format!("+{}", lines.added), added));
    let removed = (lines.removed > 0).then(|| count(format!("−{}", lines.removed), removed));
    let counts = div().flex().flex_none().gap(px(10.0)).mono().text_size(px(font::status_size()));
    counts.children(added).children(removed)
}

/// Rows of `items` `row_height` tall, at most `max` of them and a line saying how many
/// more there are.
pub fn list_items(items: Vec<ListItem>, max: usize, row_height: f32) -> Vec<Div> {
    let hidden = items.len().saturating_sub(max);
    let rows = items.into_iter().take(max).map(|item| {
        let row = div()
            .row(10.0)
            .bar(row_height, 10.0)
            .children(item.mark.map(mark_badge))
            .children(item.icon.map(|item_icon| icon(item_icon, color::fg_dim())));
        let label = div().mono().child(item.label);
        if let Some(state) = item.state {
            // A changed file: its name, then where it is and how many lines changed.
            let detail = div().fill_truncated().status_text(color::fg_dim());
            return row
                .child(state_letter(Some(state), false).w(px(12.0)))
                .child(label.flex_none().text_size(px(font::mono_size())))
                .child(detail.child(item.detail))
                .children(item.lines.map(|lines| line_counts(lines, false)));
        }
        let detail = div().flex_none().mono_status(color::fg_dim());
        let label = label.fill_truncated().text(font::mono_size(), color::fg_kbd());
        row.child(label).child(detail.child(item.detail))
    });
    let more = div().px(px(10.0)).py(px(4.0)).status_text(color::fg_dim());
    rows.chain((hidden > 0).then(|| more.child(format!("and {hidden} more")))).collect()
}

/// A sunken strip of `segment`s, one of them selected.
pub fn segmented() -> Div {
    div().flex().flex_none().p(px(2.0)).rounded_fill(8.0, glass::field()).bordered(glass::ink(0x0f))
}

pub fn segment(label: &str, selected: bool) -> Div {
    let selected_look = |segment: Div| {
        segment.bg(rgba(glass::segment())).text_color(rgb(color::fg_main())).shadow(lift())
    };
    let plain = div().row(0.0).bar(24.0, 10.0).rounded(px(6.0)).ui().status_text(color::fg_dim());
    plain.when(selected, selected_look).child(label.to_owned())
}

/// A setting's chip, lit while it is on, with the key that switches it.
pub fn option_chip(label: &str, key: &str, on: bool) -> Div {
    toggle_chip(label, on).child(kbd(key, false))
}

/// A choice's chip: its label and value, which a click changes.
pub fn choice_chip(label: &str, value: &str) -> Div {
    let value = div().mono().text_color(rgb(color::fg_main())).child(value.to_owned());
    let chevron = svg_icon(CHEVRON_DOWN_ICON, 10.0, color::fg_kbd());
    toggle_chip(&format!("{label}:"), false).child(value).child(chevron)
}

/// A setting's chip, lit and checked while it is on.
pub fn toggle_chip(label: &str, on: bool) -> Div {
    let (background, border, text) = if on {
        let accent = color::accent() << 8;
        (accent | 0x29, accent | 0x66, color::fg_alt())
    } else {
        (glass::control(), glass::control_border(), color::fg_kbd())
    };
    div()
        .row(6.0)
        .flex_none()
        .bar(26.0, 10.0)
        .rounded_fill(6.0, background)
        .bordered(border)
        .ui()
        .status_text(text)
        .when(on, |chip| chip.child(svg_icon(CHECK_ICON, 10.0, text)))
        .child(label.to_owned())
}

/// A cap for each key of a sequence, e.g. two for `C-c C-c`.
pub fn key_caps(sequence: &str) -> impl Iterator<Item = Div> + '_ {
    sequence.split_whitespace().map(|key| kbd(key, false))
}

/// The caps of `keys` and then what they do, `gap` pixels apart.
pub fn key_hint(keys: &str, label: &'static str, gap: f32) -> Div {
    div().row(gap).children(key_caps(keys)).child(label)
}

/// `child` centered across its parent, `bottom` pixels over the parent's bottom.
pub fn bottom_centered(bottom: f32, child: impl IntoElement) -> Div {
    div().across().bottom(px(bottom)).flex().justify_center().child(child)
}

/// The block cursor's quad, which a text highlight cannot round; text in it is inverted.
pub fn block_cursor() -> Div {
    div()
        .flex_none()
        .rounded(px(cursor::radius()))
        .bg(rgb(color::fg_main()))
        .text_color(rgb(color::bg_main()))
}

/// Where point is in a window that is not selected, like Emacs
/// `cursor-in-non-selected-windows`: an outline, the text under it left as it is.
pub fn hollow_cursor() -> Div {
    div().flex_none().rounded(px(cursor::radius())).border_1().border_color(rgb(color::fg_dim()))
}

/// `text` with the block cursor over its bytes `at`, the text on either side of it
/// styled by `highlights`; `composition`, where the cursor takes typed text, is drawn
/// before it.
pub fn text_with_cursor(
    text: &str,
    highlights: &Highlights,
    at: Range<usize>,
    composition: Option<&Composition>,
) -> Div {
    let part = |range: Range<usize>| {
        let styled = slice(highlights, range.clone());
        let text = StyledText::new(text[range].to_owned()).with_highlights(styled);
        div().flex_none().child(text)
    };
    let cursor = block_cursor()
        .children(composition.map(Composition::track_caret))
        .child(text[at.clone()].to_owned());
    div()
        .flex()
        .child(part(0..at.start))
        .children(composition.and_then(Composition::render_marked))
        .child(cursor)
        .child(part(at.end..text.len()))
}

/// The character under a block cursor drawn on its own, under the text: clear of any
/// background, like the region's, that would cover the cursor.
pub fn cursor_text_style() -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(color::bg_main())),
        background_color: Some(transparent_black()),
        ..Default::default()
    }
}

/// Glass of `fill` blurring what is under it by `blur`, lifted and casting `outer`.
fn floating(fill: u32, border: u32, blur: f32, outer: BoxShadow) -> Div {
    let mut shadows = lift();
    shadows.push(outer);
    div()
        .bg(rgba(fill))
        .glass_blur(blur, glass::SURFACE_SATURATION)
        .bordered(border)
        .shadow(shadows)
}

/// A floating surface of glass, showing the editor under it blurred.
pub fn glass_surface(radius: f32) -> Div {
    let outer = shadow(glass::shadow(0x80), 24.0, 60.0, 0.0);
    let (fill, border) = (glass::surface(), glass::surface_border());
    floating(fill, border, glass::SURFACE_BLUR, outer).rounded(px(radius))
}

/// A modal surface: thicker glass than a floating one, lifted further.
pub fn modal_surface() -> Div {
    let outer = shadow(glass::shadow(0x99), 30.0, 80.0, 0.0);
    let (fill, border) = (glass::modal(), glass::modal_border());
    floating(fill, border, glass::MODAL_BLUR, outer).rounded(px(glass::SURFACE_RADIUS))
}

pub fn shadow(color: u32, y: f32, blur: f32, spread: f32) -> BoxShadow {
    BoxShadow {
        color: rgba(color).into(),
        offset: point(px(0.0), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
    }
}

pub fn modified_dot() -> impl IntoElement {
    let glow = vec![shadow(color::warning() << 8 | 0x2e, 0.0, 0.0, 2.0)];
    div().flex_none().size(px(7.0)).rounded_full().bg(rgb(color::warning())).shadow(glow)
}

/// A translucent floating bar over the bottom of the window, like the status pill.
pub fn pill() -> Div {
    div()
        .row(2.0)
        .p(px(3.0))
        .rounded_fill(9.0, glass::pill())
        .glass_blur(glass::PILL_BLUR, 1.0)
        .bordered(glass::pill_border())
        .shadow(vec![shadow(glass::shadow(0x59), 4.0, 14.0, 0.0)])
        .status_text(color::fg_faint())
}

pub fn pill_separator() -> Div {
    div().w(px(1.0)).h(px(14.0)).bg(rgba(glass::ink(0x14)))
}

/// A key cap, e.g. `C-x`.
pub fn kbd(key: &str, selected: bool) -> Div {
    let (background, foreground) = if selected {
        (rgb(color::bg_kbd_selected()), color::fg_main())
    } else {
        (rgba(glass::control()), color::fg_kbd())
    };
    div()
        .row(0.0)
        .flex_none()
        .bar(20.0, 6.0)
        .rounded(px(5.0))
        .bg(background)
        .bordered(glass::control_border())
        .shadow(vec![shadow(glass::shadow(0x99), 1.0, 0.0, 0.0)])
        .mono()
        .text(font::small_size(), foreground)
        .child(key_label(key))
}

/// The key as the system names it: macOS's symbols, ⌘ being Super, spelled out elsewhere,
/// and ↩, which desktop fonts draw as an emoji, as ↵.
fn key_label(key: &str) -> String {
    if cfg!(target_os = "macos") {
        return key.to_owned();
    }
    [("⌘", "Super+"), ("⌃", "Ctrl+"), ("⌥", "Alt+"), ("⇧", "Shift+"), ("↩", "↵")]
        .iter()
        .fold(key.to_owned(), |label, (symbol, name)| label.replace(symbol, name))
}
