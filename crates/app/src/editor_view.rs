use std::ops::Range;

use gpui::{
    Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px, rgb,
    uniform_list,
};
use remacs_text::Text;
use remacs_theme::{color, font};

/// Minified files can hold megabytes on one line; layout only what fits on screen.
const MAX_RENDERED_LINE_BYTES: usize = 2048;
const TITLE_BAR_HEIGHT: f32 = 44.0;
const LINE_NUMBER_WIDTH: f32 = 64.0;

pub struct EditorView {
    text: Text,
    title: SharedString,
}

impl EditorView {
    pub fn new(text: Text, title: impl Into<SharedString>) -> Self {
        Self {
            text,
            title: title.into(),
        }
    }

    fn render_lines(&self, lines: Range<usize>) -> Vec<impl IntoElement + use<>> {
        lines
            .map(|line| {
                let range = self.text.line_range(line).unwrap_or_default();
                let visible = range.start..range.end.min(range.start + MAX_RENDERED_LINE_BYTES);
                let content = String::from_utf8_lossy(&self.text.bytes_in(visible)).into_owned();
                render_line(line + 1, content)
            })
            .collect()
    }

    fn render_title_bar(&self) -> impl IntoElement {
        div()
            .h(px(TITLE_BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .font_family(font::UI)
            .text_size(px(font::UI_SIZE))
            .text_color(rgb(color::FG_MAIN))
            .child(self.title.clone())
    }

    fn render_status(&self) -> impl IntoElement {
        div()
            .absolute()
            .right(px(16.0))
            .bottom(px(14.0))
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(9.0))
            .bg(rgb(color::BG_POPUP))
            .border_1()
            .border_color(rgb(color::BG_CHIP))
            .font_family(font::MONO)
            .text_size(px(font::STATUS_SIZE))
            .text_color(rgb(color::FG_FAINT))
            .child(format!("{} lines", self.text.line_count()))
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let line_list = uniform_list(
            "lines",
            self.text.line_count(),
            cx.processor(|view, lines, _window, _cx| view.render_lines(lines)),
        )
        .flex_1();

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(color::BG_MAIN))
            .child(self.render_title_bar())
            .child(line_list)
            .child(self.render_status())
    }
}

fn render_line(number: usize, content: String) -> impl IntoElement {
    div()
        .flex()
        .h(px(font::CODE_LINE_HEIGHT))
        .mx(px(8.0))
        .font_family(font::MONO)
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .w(px(LINE_NUMBER_WIDTH))
                .pr(px(20.0))
                .text_right()
                .text_size(px(font::LINE_NUMBER_SIZE))
                .line_height(px(font::CODE_LINE_HEIGHT))
                .text_color(rgb(color::FG_LINE_NUMBER))
                .child(number.to_string()),
        )
        .child(
            div()
                .text_size(px(font::CODE_SIZE))
                .line_height(px(font::CODE_LINE_HEIGHT))
                .text_color(rgb(color::FG_MAIN))
                .child(content),
        )
}
