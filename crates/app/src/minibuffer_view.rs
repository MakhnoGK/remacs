mod candidates;
mod chips;
mod field;

use std::borrow::Cow;
use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, FontWeight, HighlightStyle, IntoElement, ParentElement, Pixels,
    ScrollHandle, StyledText, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba,
    svg,
};
use remacs_core::{
    BannerPart, Candidate, CommitChain, Count, EntryAction, Expansion, Icon, Minibuffer, Prompt,
    PromptChoice, PromptOption, RefKind, Tone, parse_sequence, quote,
};
use remacs_theme::{color, font, glass};

use crate::assets::CHEVRON_DOWN_ICON;
use crate::composition::Composition;
use crate::dialog_view::render_dialog;
use crate::display_line::DisplayLine;
use crate::editor_view::EditorView;
use crate::outline_view::{lift_selected, ref_chip};
use crate::popup_view::render_hints;
use crate::settings::{DirectoryPosition, PromptsSettings};
use crate::widgets::{
    GlassBlur, PressExt, StyleExt, choice_chip, glass_surface, hsla, icon, inline_cursor, kbd,
    list_items, modal_surface, modified_dot, option_chip, segment, segmented, shadow,
    text_with_cursor, tone_color, ui_text,
};

use candidates::render_candidates;
pub(crate) use candidates::{matches_in, secondary_color};
use chips::{render_chips, render_outcome, render_segmented_choices, render_switches};
use field::{render_description, render_field};

const POPOVER_TOP: f32 = 60.0;
/// The problem list opens from the status pill, above it.
const STATUS_POPOVER_RIGHT: f32 = 16.0;
const STATUS_POPOVER_BOTTOM: f32 = 56.0;
const PROBLEM_CATEGORY: &str = "problem";
const SEVERITY_ICONS: [Icon; 3] = [Icon::Error, Icon::Warning, Icon::Info];
const POSITION_WIDTH: f32 = 44.0;
const POPOVER_WIDTH: f32 = 680.0;
/// Opened from the toolbar, under its right end.
const TOOLBAR_POPOVER_WIDTH: f32 = 500.0;
const TOOLBAR_POPOVER_TOP: f32 = 44.0;
const TOOLBAR_POPOVER_RIGHT: f32 = 14.0;
const WIDE_POPOVER_WIDTH: f32 = 760.0;
const FIELD_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 34.0;
const WIDE_ROW_HEIGHT: f32 = 36.0;
/// M-x lines command names up in a column, with their documentation beside them.
const COMMAND_CATEGORY: &str = "command";
const COMMAND_NAME_WIDTH: f32 = 232.0;
/// The input's children are the text before the cursor, the cursor and the rest.
const CURSOR_CHILD: usize = 1;
/// Items listed for reference give way to the candidates beyond this many.
const MAX_LISTED_ITEMS: usize = 5;
/// Of a message's description, shown at once.
const DESCRIPTION_LINES: usize = 4;
const DESCRIPTION_LINE_HEIGHT: f32 = 22.0;

/// Dims the editor behind an open prompt; a modal one also blurs it a little.
pub fn render_scrim(modal: bool) -> impl IntoElement {
    let scrim = div().absolute().inset_0();
    if modal {
        scrim.bg(rgba(glass::modal_scrim())).glass_blur(glass::MODAL_SCRIM_BLUR, 1.0)
    } else {
        scrim.bg(rgba(glass::scrim()))
    }
}

/// The problem list: rows of messages with their positions, anchored to the status pill.
pub fn is_problem_list(minibuffer: &Minibuffer) -> bool {
    minibuffer.prompt().category == PROBLEM_CATEGORY
}

/// A floating panel, like the prompt's.
pub fn popover(width: f32) -> Div {
    panel_on(glass_surface(glass::SURFACE_RADIUS), width)
}

/// A panel for a question that must be answered.
pub fn modal(width: f32) -> Div {
    panel_on(modal_surface(), width)
}

fn panel_on(surface: Div, width: f32) -> Div {
    surface.w(px(width)).p(px(8.0)).font_family(font::ui()).fg(color::fg_main())
}

/// A title as dialogs and titled prompts set it.
pub(crate) fn title_text(title: &str) -> Div {
    let title = ui_text(title, FontWeight::SEMIBOLD, color::fg_main());
    div().text_size(px(font::title_size())).child(title)
}

/// Interface text `size` pixels high in `text_color`, its names in the code font.
pub(crate) fn plain_text(text: &str, size: f32, text_color: u32) -> Div {
    let text = ui_text(text, FontWeight::NORMAL, text_color);
    div().text_size(px(size)).child(text)
}

/// Whether the prompt shows the editable input, which dialogs, titled prompts and
/// questions leave out.
pub fn draws_input(minibuffer: &Minibuffer) -> bool {
    minibuffer.dialog().is_none()
        && minibuffer.prompt().title.is_none()
        && !minibuffer.is_question()
}

/// `input_scroll` keeps the input's sideways scroll between frames; it keeps
/// `prompts.input_margin` in view on each side of the cursor. `composition` is drawn in
/// the input that takes typed text.
pub fn render_minibuffer(
    minibuffer: &Minibuffer,
    input_scroll: &ScrollHandle,
    composition: Option<&Composition>,
    prompts: PromptsSettings,
    cx: &mut Context<EditorView>,
) -> AnyElement {
    if let Some(dialog) = minibuffer.dialog() {
        return centered(render_dialog(dialog, cx));
    }
    let prompt = minibuffer.prompt();
    let width = match () {
        _ if prompt.from_toolbar => TOOLBAR_POPOVER_WIDTH,
        _ if prompt.wide => WIDE_POPOVER_WIDTH,
        _ => POPOVER_WIDTH,
    };
    let has_switch = prompt.options.iter().any(PromptOption::is_switch);
    let has_chip = prompt.choices.iter().any(|choice| !choice.segmented)
        || prompt.options.iter().any(|option| !option.is_switch());
    let field = match &prompt.title {
        Some((title, note)) => render_title(title, note).into_any_element(),
        None if minibuffer.is_question() => render_title(&prompt.label, "").into_any_element(),
        None => {
            let margin = px(prompts.input_margin);
            render_field(minibuffer, input_scroll, composition, margin).into_any_element()
        }
    };
    let directory = |position| {
        (prompts.directory == position && !prompt.directory.is_empty())
            .then(|| render_directory(&prompt.directory, position))
    };
    let chips = render_chips(minibuffer, !has_switch, cx);
    let (chips_first, chips_last) = if prompt.chips_first { (chips, None) } else { (None, chips) };
    let popover = popover(width)
        .when(has_switch || (!has_chip && !minibuffer.subject().is_empty()), |popover| {
            popover.child(render_switches(minibuffer, cx))
        })
        .children(directory(DirectoryPosition::Above))
        .child(field)
        .children(directory(DirectoryPosition::Below))
        .when(!prompt.summary.is_empty(), |popover| popover.child(render_summary(&prompt.summary)))
        .children(chips_first)
        .children(render_segmented_choices(minibuffer, cx))
        .children(render_description(minibuffer, composition))
        .when(!minibuffer.candidates().is_empty(), |popover| {
            popover.child(render_candidates(minibuffer, cx))
        })
        .children(minibuffer.completions().outcome.clone().map(render_outcome))
        .when(!prompt.items.is_empty(), |popover| {
            let summary = Some(prompt.items_summary.clone());
            let items = list_items(prompt.items.clone(), MAX_LISTED_ITEMS, 30.0);
            popover.child(render_group_title(prompt.items_title, summary, false)).children(items)
        })
        .children(chips_last)
        .child(render_hints(minibuffer.hints()).gap(px(18.0)).mt(px(6.0)).pad(10.0, 10.0, 4.0));
    let corner = if is_problem_list(minibuffer) {
        div().right(px(STATUS_POPOVER_RIGHT)).bottom(px(STATUS_POPOVER_BOTTOM))
    } else if prompt.from_toolbar {
        div().top(px(TOOLBAR_POPOVER_TOP)).right(px(TOOLBAR_POPOVER_RIGHT))
    } else {
        return centered(popover);
    };
    corner.absolute().child(popover).into_any_element()
}

/// In place of the field: what the prompt does or asks, as a dialog's title, and a note
/// on the right.
fn render_title(title: &str, note: &str) -> Div {
    let row = div().flex().items_baseline().justify_between().gap(px(12.0)).pad(6.0, 10.0, 8.0);
    let note = plain_text(note, font::status_size(), color::fg_dim()).flex_none();
    row.child(title_text(title).min_w_0()).child(note)
}

/// `in <directory>`: the parents give way to an ellipsis so the last directory stays
/// whole.
fn render_directory(directory: &str, position: DirectoryPosition) -> Div {
    let (parents, last) = split_last_directory(directory);
    let (top, bottom) = match position {
        DirectoryPosition::Above => (2.0, 8.0),
        DirectoryPosition::Below => (8.0, 2.0),
    };
    let parents = div().min_w_0().truncate().child(parents.to_owned());
    let path = div().flex().min_w_0().mono().fg(color::fg_prompt_directory()).child(parents);
    let path = path.child(div().flex_none().child(last.to_owned()));
    let label = div().flex_none().fg(color::fg_dim()).child("in");
    let row = div().flex().items_baseline().gap(px(6.0)).pad(top, 10.0, bottom).overflow_hidden();
    row.whitespace_nowrap().text_size(px(font::status_size())).child(label).child(path)
}

/// `~/a/b/` into `~/a` and `/b/`: the slash stays with the last directory, so the
/// ellipsis of cut parents never runs into its name.
fn split_last_directory(directory: &str) -> (&str, &str) {
    let trimmed = directory.strip_suffix('/').unwrap_or(directory);
    directory.split_at(trimmed.rfind('/').unwrap_or(0))
}

/// Chips and words under the title, e.g. how far two branches are apart.
fn render_summary(parts: &[BannerPart]) -> Div {
    let row = div().row(8.0).flex_wrap().px(px(10.0)).pb(px(10.0)).status_text(color::fg_dim());
    row.children(parts.iter().map(|part| match part {
        BannerPart::Chip(chip) => ref_chip(chip.clone()),
        BannerPart::Word(word) => div().child(word.clone()),
    }))
}

/// Near the top of the window, centered across it.
fn centered(popover: impl IntoElement) -> AnyElement {
    div().across().top(px(POPOVER_TOP)).flex().justify_center().child(popover).into_any_element()
}

/// Upper case, unless it is a name; `summary` goes on the right.
fn render_group_title(title: &str, summary: Option<String>, is_name: bool) -> impl IntoElement {
    let title = if is_name { title.to_owned() } else { title.to_uppercase() };
    let row = div().flex().justify_between().pad(12.0, 10.0, 4.0).font_weight(FontWeight::SEMIBOLD);
    row.text(font::small_size(), color::fg_faint()).child(title).children(summary)
}

#[cfg(test)]
mod tests {
    use remacs_core::{Editor, Prompt};

    use super::{draws_input, split_last_directory};

    #[test]
    fn the_last_directory_is_split_off_its_parents() {
        assert_eq!(split_last_directory("~/a/b/"), ("~/a", "/b/"));
        assert_eq!(split_last_directory("~/"), ("", "~/"));
        assert_eq!(split_last_directory("/"), ("", "/"));
    }

    /// Until the input is laid out, every frame asks for another; a question must not.
    #[test]
    fn only_prompts_with_an_input_wait_for_its_layout() {
        let mut asking = Editor::new(&[]);
        asking.ask_y_or_n("Proceed?", |_, _| Ok(())).unwrap();
        assert!(!draws_input(asking.minibuffer().unwrap()));

        let mut reading = Editor::new(&[]);
        reading.read(Prompt::default(), |_, _| Default::default(), |_, _| Ok(())).unwrap();
        assert!(draws_input(reading.minibuffer().unwrap()));
    }
}
