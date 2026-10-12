//! The row along the bottom of the window: the echo area on the left, the view's keys in
//! the middle, the status on the right. Each keeps its place, so what one shows never
//! moves another: the keys are centered on the window and how many fit depends on its
//! width only, and a message is cut where the keys begin.

use gpui::{
    AnyElement, Context, Div, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels,
    SharedString, TextRun, Window, black, div, font, prelude::*, px, rgb, rgba,
};
use remacs_core::{Binding, EchoMessage, Key, MessageKind, describe_sequence};
use remacs_theme::{color, font as theme_font, glass};

use crate::editor_view::EditorView;
use crate::widgets::{glass_surface, kbd, key_caps, pill, pill_separator};

/// Keys and what they do.
pub type Hints = &'static [(&'static str, &'static str)];

const SIDE: f32 = 16.0;
const BOTTOM: f32 = 14.0;
const GAP: f32 = 12.0;
/// Where what opens from the row starts, clear of its pills.
const ABOVE_ROW: f32 = 52.0;
const ITEM_HEIGHT: f32 = 24.0;
const ITEM_PADDING: f32 = 8.0;
const ITEM_GAP: f32 = 6.0;
/// A pill's padding and border, and the gap between its items.
const PILL_INSET: f32 = 4.0;
const PILL_ITEM_GAP: f32 = 2.0;
/// A key cap's padding and border on each side.
const KEY_CAP_INSET: f32 = 7.0;
const MARK_SIZE: f32 = 8.0;
/// Between a message's colored mark and its text.
const MARK_GAP: f32 = 8.0;
const CARD_WIDTH: f32 = 760.0;
const CARD_LINE_HEIGHT: f32 = 20.0;
const LIST_WIDTH: f32 = 300.0;
const LIST_ROW_HEIGHT: f32 = 28.0;
const PREFIX_ROW_HEIGHT: f32 = 26.0;
/// Of the window, kept clear above the prefix's keys.
const PREFIX_TOP_ROOM: f32 = 240.0;
const MIN_PREFIX_ROWS: usize = 3;
const DIVIDER: u32 = 0xffffff12;

/// The view's keys and how many of them fit under it.
#[derive(Clone, Copy)]
pub struct FittedHints {
    pub hints: Hints,
    pub shown: usize,
    /// Of their pill, `+N` chip included.
    pub width: Pixels,
}

impl FittedHints {
    fn hidden(&self) -> Hints {
        &self.hints[self.shown..]
    }

    /// Those a list over the row shows: the ones that did not fit, or all when none
    /// show.
    pub fn listed(&self) -> Hints {
        if self.width == px(0.0) {
            self.hints
        } else {
            self.hidden()
        }
    }
}

/// The keys that may follow a prefix typed, in the order they are listed.
pub struct PrefixKeys {
    pub pending: Vec<Key>,
    pub continuations: Vec<(Key, Binding)>,
}

impl PrefixKeys {
    pub fn new(pending: &[Key], mut continuations: Vec<(Key, Binding)>) -> Self {
        continuations
            .sort_by_key(|(key, _)| (key.modifiers != Default::default(), key.to_string()));
        Self {
            pending: pending.to_vec(),
            continuations,
        }
    }
}

/// As many of `hints` as fit between `side_room` kept on each side of a window this
/// wide; the rest go under a chip.
pub fn fit_hints(hints: Hints, show: bool, side_room: f32, window: &Window) -> FittedHints {
    if !show || hints.is_empty() {
        return FittedHints {
            hints,
            shown: 0,
            width: px(0.0),
        };
    }
    let room = window.viewport_size().width - px(2.0 * (SIDE + GAP + side_room));
    let widths: Vec<Pixels> = hints
        .iter()
        .map(|&(keys, label)| hint_width(keys, label, window))
        .collect();
    let pill_width = |items: &[Pixels], extra: Option<Pixels>| {
        let count = items.len() + usize::from(extra.is_some());
        let gaps = PILL_ITEM_GAP * count.saturating_sub(1) as f32;
        items
            .iter()
            .chain(extra.as_ref())
            .fold(px(2.0 * PILL_INSET + gaps), |sum, &width| sum + width)
    };
    let all = pill_width(&widths, None);
    if all <= room {
        return FittedHints {
            hints,
            shown: hints.len(),
            width: all,
        };
    }
    let fitting = (0..hints.len())
        .rev()
        .map(|shown| {
            let chip = chip_width(&chip_label(shown, hints.len() - shown), window);
            (shown, pill_width(&widths[..shown], Some(chip)))
        })
        .find(|&(shown, width)| width <= room || shown == 0);
    let (shown, width) = fitting.unwrap_or((0, px(0.0)));
    FittedHints {
        hints,
        shown,
        width,
    }
}

/// The row, its three slots side by side; the middle one stays centered while the
/// status fits its side.
pub fn render_row(echo: Option<AnyElement>, hints: Option<Div>, status: AnyElement) -> Div {
    div()
        .absolute()
        .left(px(SIDE))
        .right(px(SIDE))
        .bottom(px(BOTTOM))
        .flex()
        .items_center()
        .gap(px(GAP))
        .child(div().flex_1().min_w_0().flex().children(echo))
        .child(div().flex_none().flex().children(hints))
        .child(div().flex_1().flex().justify_end().child(status))
}

/// The room a message has: its side of the row.
pub fn echo_room(fitted: &FittedHints, window: &Window) -> Pixels {
    (window.viewport_size().width - px(2.0 * (SIDE + GAP)) - fitted.width) / 2.0
}

/// The keys of a prefix typed so far, with a caret waiting for the next one.
pub fn render_prefix_echo(pending: &[Key]) -> Div {
    pill().child(
        div()
            .flex()
            .items_center()
            .gap(px(ITEM_GAP))
            .h(px(ITEM_HEIGHT))
            .px(px(ITEM_PADDING))
            .children(pending.iter().map(|key| kbd(&key.to_string(), true)))
            .child(
                div()
                    .w(px(7.0))
                    .h(px(14.0))
                    .rounded(px(1.0))
                    .bg(rgba(color::FG_MAIN << 8 | 0xd9)),
            ),
    )
}

/// A message on one line, cut to `room` with the key that shows it whole; a click opens
/// it above.
pub fn render_message_echo(
    message: &EchoMessage,
    room: Pixels,
    open: bool,
    whole_key: Option<&str>,
    window: &Window,
    cx: &mut Context<EditorView>,
) -> Div {
    let text = one_line(&message.text);
    let overhead = px(2.0 * (PILL_INSET + ITEM_PADDING) + MARK_SIZE + MARK_GAP);
    let full_width = text_width(&text, theme_font::UI, theme_font::STATUS_SIZE, window);
    let cut = message.text.contains('\n') || full_width + overhead > room;
    let whole_key = whole_key.filter(|_| cut);
    let border = match message.kind {
        MessageKind::Error => rgba(color::ERROR << 8 | 0x66),
        _ => rgba(glass::PILL_BORDER),
    };
    pill()
        .min_w_0()
        .border_color(border)
        .when(open, |pill| pill.bg(rgba(glass::SELECTION)))
        .font_family(theme_font::UI)
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|view, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                view.toggle_message_open(cx);
            }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(MARK_GAP))
                .min_w_0()
                .h(px(ITEM_HEIGHT))
                .px(px(ITEM_PADDING))
                .children(kind_mark(message.kind))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(color::FG_BODY))
                        .child(text),
                ),
        )
        .when_some(whole_key, |pill, key| {
            pill.child(pill_separator().flex_none()).child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(ITEM_GAP))
                    .h(px(ITEM_HEIGHT))
                    .px(px(ITEM_PADDING))
                    .children(key_caps(key)),
            )
        })
}

/// The keys that fit, and a chip for the rest that lists them.
pub fn render_hints(
    fitted: &FittedHints,
    listed: bool,
    cx: &mut Context<EditorView>,
) -> Option<Div> {
    let hidden = fitted.hidden().len();
    if fitted.shown == 0 && (hidden == 0 || fitted.width == px(0.0)) {
        return None;
    }
    let items = fitted.hints[..fitted.shown].iter().map(|&(keys, label)| {
        div()
            .flex()
            .items_center()
            .gap(px(ITEM_GAP))
            .h(px(ITEM_HEIGHT))
            .px(px(ITEM_PADDING))
            .children(key_caps(keys))
            .child(label)
    });
    let chip = (hidden > 0).then(|| {
        div()
            .flex()
            .items_center()
            .h(px(ITEM_HEIGHT))
            .px(px(ITEM_PADDING))
            .rounded(px(6.0))
            .bg(if listed {
                rgba(glass::SELECTION)
            } else {
                rgba(glass::CONTROL)
            })
            .text_color(rgb(color::FG_MAIN))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    view.toggle_hints_open(cx);
                }),
            )
            .child(chip_label(fitted.shown, hidden))
    });
    Some(
        pill()
            .font_family(theme_font::UI)
            .children(items)
            .children(chip),
    )
}

/// The keys that did not fit, or all of them when none show, over the row.
pub fn render_hints_list(fitted: &FittedHints, title: &str) -> Option<impl IntoElement> {
    let listed = fitted.listed();
    if listed.is_empty() {
        return None;
    }
    let rows = listed.iter().map(|&(keys, label)| {
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(LIST_ROW_HEIGHT))
            .px(px(ITEM_PADDING))
            .children(key_caps(keys))
            .child(label)
    });
    let list = glass_surface(glass::HINT_RADIUS)
        .w(px(LIST_WIDTH))
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(1.0))
        .font_family(theme_font::UI)
        .text_size(px(theme_font::STATUS_SIZE))
        .text_color(rgb(color::FG_BODY))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(section_heading(title))
        .children(rows);
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(ABOVE_ROW))
            .flex()
            .justify_center()
            .child(list),
    )
}

/// What the keys of the card do.
pub struct CardKeys<'a> {
    pub all_messages: Option<&'a str>,
    pub copy: &'a str,
    pub close: &'a str,
}

/// The message whole, above its pill, scrolling past `lines`.
pub fn render_message_card(
    message: &EchoMessage,
    lines: usize,
    keys: CardKeys,
    window: &Window,
) -> impl IntoElement {
    let width = px(CARD_WIDTH).min(window.viewport_size().width - px(2.0 * SIDE));
    let kind = match message.kind {
        MessageKind::Info => "Message",
        MessageKind::Done => "Done",
        MessageKind::Error => "Error",
    };
    let title = div()
        .flex()
        .items_center()
        .gap(px(MARK_GAP))
        .children(kind_mark(message.kind))
        .child(div().child(kind))
        .children(message.command.map(|command| {
            div()
                .font_family(theme_font::mono())
                .text_size(px(theme_font::STATUS_SIZE))
                .text_color(rgb(color::FG_DIM))
                .child(command)
        }));
    let header = card_heading(title).child(
        div()
            .font_family(theme_font::mono())
            .text_size(px(theme_font::STATUS_SIZE))
            .text_color(rgb(color::FG_PLACEHOLDER))
            .child(message.clock()),
    );
    let body = div()
        .id("echo-message")
        .max_h(px(CARD_LINE_HEIGHT * lines as f32 + 20.0))
        .overflow_y_scroll()
        .px(px(14.0))
        .py(px(10.0))
        .font_family(theme_font::mono())
        .text_size(px(theme_font::STATUS_SIZE))
        .line_height(px(CARD_LINE_HEIGHT))
        .text_color(rgb(color::FG_BODY))
        .child(message.text.clone());
    let key = |keys: &str, label: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(ITEM_GAP))
            .children(key_caps(keys))
            .child(label)
            .into_any_element()
    };
    let footer = div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .h(px(36.0))
        .px(px(14.0))
        .border_t_1()
        .border_color(rgba(DIVIDER))
        .text_size(px(theme_font::STATUS_SIZE))
        .text_color(rgb(color::FG_DIM))
        .children(keys.all_messages.map(|keys| key(keys, "All messages")))
        .child(key(keys.copy, "Copy"))
        .child(key(keys.close, "Close"));
    glass_surface(glass::HINT_RADIUS)
        .absolute()
        .left(px(SIDE))
        .bottom(px(ABOVE_ROW))
        .w(width)
        .flex()
        .flex_col()
        .overflow_hidden()
        .font_family(theme_font::UI)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(header)
        .child(body)
        .child(footer)
}

/// What each key after the prefix typed does, in `columns` filled top to bottom.
pub fn render_prefix_keys(
    keys: &PrefixKeys,
    columns: usize,
    cancel_key: &str,
    window: &Window,
) -> impl IntoElement {
    let PrefixKeys {
        pending,
        continuations,
    } = keys;
    let max_rows = ((window.viewport_size().height - px(PREFIX_TOP_ROOM)) / px(PREFIX_ROW_HEIGHT))
        .floor()
        .max(MIN_PREFIX_ROWS as f32) as usize;
    let count = continuations.len();
    let capacity = columns * max_rows;
    let more = count.saturating_sub(capacity);
    let mut entries: Vec<AnyElement> = continuations
        .iter()
        .take(if more > 0 { capacity - 1 } else { capacity })
        .map(|&(key, binding)| {
            let label = match binding {
                Binding::Command(command) => div().text_color(rgb(color::FG_BODY)).child(command),
                _ => {
                    let mut sequence = pending.to_vec();
                    sequence.push(key);
                    div()
                        .text_color(rgb(color::FG_DIRECTORY))
                        .child(format!("{} …", describe_sequence(&sequence)))
                }
            };
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .h(px(PREFIX_ROW_HEIGHT))
                .px(px(ITEM_PADDING))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .min_w(px(40.0))
                        .child(kbd(&key.to_string(), false)),
                )
                .child(label.min_w_0().truncate())
                .into_any_element()
        })
        .collect();
    if more > 0 {
        entries.push(
            div()
                .flex()
                .items_center()
                .h(px(PREFIX_ROW_HEIGHT))
                .px(px(ITEM_PADDING))
                .text_color(rgb(color::FG_DIM))
                .child(format!("and {} more", more + 1))
                .into_any_element(),
        );
    }
    let rows = entries.len().div_ceil(columns).max(1);
    let mut entries = entries.into_iter();
    let grid = div()
        .flex()
        .gap(px(24.0))
        .px(px(16.0))
        .py(px(12.0))
        .children((0..columns).map(|_| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .children(entries.by_ref().take(rows).collect::<Vec<_>>())
        }));
    let title = div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .children(pending.iter().map(|key| kbd(&key.to_string(), true)))
        .child(div().child("then"));
    let header = card_heading(title).child(
        div()
            .text_size(px(theme_font::STATUS_SIZE))
            .text_color(rgb(color::FG_PLACEHOLDER))
            .child(format!("{count} keys")),
    );
    let footer = div()
        .flex()
        .items_center()
        .gap(px(ITEM_GAP))
        .h(px(34.0))
        .px(px(16.0))
        .border_t_1()
        .border_color(rgba(DIVIDER))
        .text_size(px(theme_font::STATUS_SIZE))
        .text_color(rgb(color::FG_DIM))
        .children(key_caps(cancel_key))
        .child("Cancel");
    glass_surface(glass::HINT_RADIUS)
        .absolute()
        .left(px(SIDE))
        .right(px(SIDE))
        .bottom(px(ABOVE_ROW))
        .flex()
        .flex_col()
        .overflow_hidden()
        .font_family(theme_font::UI)
        .text_size(px(theme_font::STATUS_SIZE))
        .child(header)
        .child(grid)
        .child(footer)
}

fn kind_mark(kind: MessageKind) -> Option<Div> {
    let mark_color = match kind {
        MessageKind::Info => return None,
        MessageKind::Done => color::VC_ADDED,
        MessageKind::Error => color::ERROR,
    };
    Some(
        div()
            .flex_none()
            .size(px(MARK_SIZE))
            .rounded_full()
            .bg(rgb(mark_color)),
    )
}

/// The top line of a card: `title` on the left, then what is added on the right.
fn card_heading(title: Div) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(40.0))
        .px(px(14.0))
        .border_b_1()
        .border_color(rgba(DIVIDER))
        .text_size(px(theme_font::UI_SIZE))
        .text_color(rgb(color::FG_MAIN))
        .child(title)
}

fn section_heading(title: &str) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(ITEM_PADDING))
        .text_size(px(theme_font::SMALL_SIZE))
        .text_color(rgb(color::FG_PLACEHOLDER))
        .child(title.to_uppercase())
}

/// `+3` beside some keys, `Keys · 8` alone.
fn chip_label(shown: usize, hidden: usize) -> String {
    if shown == 0 {
        format!("Keys · {hidden}")
    } else {
        format!("+{hidden}")
    }
}

fn chip_width(label: &str, window: &Window) -> Pixels {
    text_width(label, theme_font::UI, theme_font::STATUS_SIZE, window) + px(2.0 * ITEM_PADDING)
}

fn hint_width(keys: &str, label: &str, window: &Window) -> Pixels {
    let caps: Vec<&str> = keys.split_whitespace().collect();
    let caps_width = caps.iter().fold(px(0.0), |sum, cap| {
        sum + text_width(cap, theme_font::mono(), theme_font::SMALL_SIZE, window)
            + px(2.0 * KEY_CAP_INSET)
    });
    px(2.0 * ITEM_PADDING + ITEM_GAP * caps.len() as f32)
        + caps_width
        + text_width(label, theme_font::UI, theme_font::STATUS_SIZE, window)
}

fn text_width(text: &str, family: impl Into<SharedString>, size: f32, window: &Window) -> Pixels {
    if text.is_empty() {
        return px(0.0);
    }
    let run = TextRun {
        len: text.len(),
        font: font(family),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text.to_owned()), px(size), &[run], None)
        .width
}

/// Lines and runs of spaces joined by one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
