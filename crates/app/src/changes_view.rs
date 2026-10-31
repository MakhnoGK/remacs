use gpui::{Context, Div, FontWeight, ParentElement, div, prelude::*, px, rgba, svg};
use remacs_core::{Changes, DiffLine, Elsewhere, FileDiff};
use remacs_theme::{color, font, glass};

use crate::assets::ARROW_ICON;
use crate::editor_view::EditorView;
use crate::preview_view::{code_well, pane};
use crate::table_view::clickable;
use crate::widgets::{StyleExt, toggle_chip};

const HEADER_HEIGHT: f32 = 36.0;
const EDIT_HEIGHT: f32 = 32.0;
const DIFF_LINE_HEIGHT: f32 = 20.0;
const DIFF_PREFIX_WIDTH: f32 = 16.0;

/// Edits to a table's rows waiting to be applied, what they also change in other files and
/// how they are applied; as tall as its contents.
pub fn render_changes(changes: Changes, cx: &mut Context<EditorView>) -> Div {
    let title =
        div().flex_1().text(font::ui_size(), color::fg_main()).font_weight(FontWeight::SEMIBOLD);
    let summary = div().status_text(color::fg_faint()).child(changes.summary);
    let header = div().row(10.0).flex_none().bar(HEADER_HEIGHT, 8.0).ui();
    let header = header.child(title.child("Pending changes")).child(summary);
    let edits = changes.edits.into_iter().map(|(before, after)| render_edit(before, after));
    let options: Vec<_> = (changes.options.into_iter())
        .map(|option| clickable(toggle_chip(&option.label, option.selected), option.action, cx))
        .collect();
    let options = (!options.is_empty())
        .then(|| div().row(6.0).flex_wrap().pad(6.0, 2.0, 2.0).children(options));
    let frame = pane().child(header).child(div().column().gap(px(4.0)).children(edits));
    let frame = frame.children(render_elsewhere(changes.elsewhere)).children(options);
    // The column stretches beside the rows; the pane inside keeps its own height.
    div().column().flex_none().child(frame)
}

fn render_edit(before: String, after: String) -> Div {
    let arrow = svg().path(ARROW_ICON).flex_none().size(px(12.0)).fg(color::fg_faint());
    let before = div().min_w_0().truncate().fg(color::fg_dim()).child(before);
    let after = div().min_w_0().truncate().child(after);
    let row = div().row(10.0).flex_none().bar(EDIT_HEIGHT, 10.0).rounded_fill(8.0, glass::pane());
    row.mono_status(color::fg_main()).child(before).child(arrow).child(after)
}

/// "Also updates" and the lines that change in each file, or that they are still being
/// worked out.
fn render_elsewhere(elsewhere: Elsewhere) -> Vec<Div> {
    let lines: Vec<Div> = match elsewhere {
        Elsewhere::Nothing => return Vec::new(),
        Elsewhere::Checking => {
            vec![div().px(px(10.0)).ui().fg(color::fg_faint()).child("Checking other files…")]
        }
        Elsewhere::Files(files) => files.into_iter().flat_map(render_file_diff).collect(),
    };
    let title = div().pad(10.0, 10.0, 2.0).ui().font_weight(FontWeight::SEMIBOLD);
    let title = title.text(font::small_size(), color::fg_faint()).child("ALSO UPDATES");
    let code = code_well(6.0, DIFF_LINE_HEIGHT).whitespace_nowrap().fg(color::fg_kbd());
    vec![title, code.children(lines)]
}

fn render_file_diff(file: FileDiff) -> Vec<Div> {
    let header = diff_line("", file.name, color::docstring()).bg(rgba(glass::ink(0x0a)));
    let lines = file.lines.into_iter().map(|line| match line {
        DiffLine::Removed(text) => {
            diff_line("-", text, color::fg_removed()).bg(rgba(glass::removed()))
        }
        DiffLine::Added(text) => diff_line("+", text, color::fg_added()).bg(rgba(glass::added())),
    });
    std::iter::once(header).chain(lines).collect()
}

/// A diff's line: its `+` or `-` in a column of its own, then its text.
pub(crate) fn diff_line(prefix: &'static str, text: String, text_color: u32) -> Div {
    let prefix = div().flex_none().w(px(DIFF_PREFIX_WIDTH)).text_center().child(prefix);
    div().flex().flex_none().fg(text_color).child(prefix).child(text)
}
