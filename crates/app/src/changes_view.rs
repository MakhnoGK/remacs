use gpui::{Context, Div, FontWeight, ParentElement, div, prelude::*, px, rgb, rgba, svg};
use remacs_core::{Changes, DiffLine, Elsewhere, FileDiff};
use remacs_theme::{color, font, glass};

use crate::assets::ARROW_ICON;
use crate::editor_view::EditorView;
use crate::preview_view::pane;
use crate::table_view::clickable;
use crate::widgets::toggle_chip;

const HEADER_HEIGHT: f32 = 36.0;
const EDIT_HEIGHT: f32 = 32.0;
const DIFF_LINE_HEIGHT: f32 = 20.0;
const DIFF_PREFIX_WIDTH: f32 = 16.0;

/// Edits to a table's rows waiting to be applied, what they also change in other files and
/// how they are applied; as tall as its contents.
pub fn render_changes(changes: Changes, cx: &mut Context<EditorView>) -> Div {
    let Changes {
        summary,
        edits,
        elsewhere,
        options,
    } = changes;
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(px(HEADER_HEIGHT))
        .px(px(8.0))
        .font_family(font::ui())
        .child(
            div()
                .flex_1()
                .text_size(px(font::ui_size()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(color::fg_main()))
                .child("Pending changes"),
        )
        .child(
            div()
                .text_size(px(font::status_size()))
                .text_color(rgb(color::fg_faint()))
                .child(summary),
        );
    let edits = div().flex().flex_col().gap(px(4.0)).children(
        edits
            .into_iter()
            .map(|(before, after)| render_edit(before, after)),
    );
    let options: Vec<_> = options
        .into_iter()
        .map(|option| {
            clickable(
                toggle_chip(&option.label, option.selected),
                option.action,
                cx,
            )
        })
        .collect();
    let frame = pane()
        .child(header)
        .child(edits)
        .children(render_elsewhere(elsewhere))
        .when(!options.is_empty(), |frame| {
            frame.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .pt(px(6.0))
                    .px(px(2.0))
                    .pb(px(2.0))
                    .children(options),
            )
        });
    // The column stretches beside the rows; the pane inside keeps its own height.
    div().flex().flex_col().flex_none().child(frame)
}

fn render_edit(before: String, after: String) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(px(EDIT_HEIGHT))
        .px(px(10.0))
        .rounded(px(8.0))
        .bg(rgba(glass::pane()))
        .font_family(font::mono())
        .text_size(px(font::status_size()))
        .text_color(rgb(color::fg_main()))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(rgb(color::fg_dim()))
                .child(before),
        )
        .child(
            svg()
                .path(ARROW_ICON)
                .flex_none()
                .size(px(12.0))
                .text_color(rgb(color::fg_faint())),
        )
        .child(div().min_w_0().truncate().child(after))
}

/// "Also updates" and the lines that change in each file, or that they are still being
/// worked out.
fn render_elsewhere(elsewhere: Elsewhere) -> Vec<Div> {
    let lines: Vec<Div> = match elsewhere {
        Elsewhere::Nothing => return Vec::new(),
        Elsewhere::Checking => vec![
            div()
                .px(px(10.0))
                .font_family(font::ui())
                .text_color(rgb(color::fg_faint()))
                .child("Checking other files…"),
        ],
        Elsewhere::Files(files) => files.into_iter().flat_map(render_file_diff).collect(),
    };
    let title = div()
        .pt(px(10.0))
        .px(px(10.0))
        .pb(px(2.0))
        .font_family(font::ui())
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(font::small_size()))
        .text_color(rgb(color::fg_faint()))
        .child("ALSO UPDATES");
    let code = div()
        .flex()
        .flex_col()
        .py(px(6.0))
        .rounded(px(8.0))
        .bg(rgba(glass::well()))
        .border_1()
        .border_color(rgba(glass::ink(0x0f)))
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(font::mono())
        .text_size(px(font::status_size()))
        .line_height(px(DIFF_LINE_HEIGHT))
        .text_color(rgb(color::fg_kbd()))
        .children(lines);
    vec![title, code]
}

fn render_file_diff(file: FileDiff) -> Vec<Div> {
    let header = diff_line("", file.name)
        .text_color(rgb(color::docstring()))
        .bg(rgba(glass::ink(0x0a)));
    let lines = file.lines.into_iter().map(|line| match line {
        DiffLine::Removed(text) => diff_line("-", text)
            .bg(rgba(glass::removed()))
            .text_color(rgb(color::fg_removed())),
        DiffLine::Added(text) => diff_line("+", text)
            .bg(rgba(glass::added()))
            .text_color(rgb(color::fg_added())),
    });
    std::iter::once(header).chain(lines).collect()
}

fn diff_line(prefix: &'static str, text: String) -> Div {
    div()
        .flex()
        .flex_none()
        .child(
            div()
                .flex_none()
                .w(px(DIFF_PREFIX_WIDTH))
                .text_center()
                .child(prefix),
        )
        .child(text)
}
