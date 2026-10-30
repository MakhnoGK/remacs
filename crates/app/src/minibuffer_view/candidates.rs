use super::*;

pub(super) fn render_candidates(
    minibuffer: &Minibuffer,
    cx: &mut Context<EditorView>,
) -> impl IntoElement {
    let candidates = minibuffer.candidates();
    let is_problem_list = is_problem_list(minibuffer);
    let mut list = div().column().gap(px(2.0));
    let mut group = None;
    for index in minibuffer.visible() {
        let candidate = &candidates[index];
        let candidate_group = candidate.group.as_deref();
        if let Some(title) = candidate_group.filter(|_| candidate_group != group) {
            // Problems are grouped by file: names keep their case and show their count.
            let counted = is_problem_list || minibuffer.completions().count_groups;
            let count = counted.then(|| {
                let in_group = |other: &&Candidate| other.group.as_deref() == Some(title);
                candidates.iter().filter(in_group).count().to_string()
            });
            list = list.child(render_group_title(title, count, is_problem_list));
        }
        group = candidate_group;
        let selected = minibuffer.selected() == Some(index);
        let row = match (&candidate.expansion, candidate.action) {
            (Some(expansion), _) if !is_problem_list => {
                let in_list = minibuffer.in_list();
                render_entry(candidate, expansion, index, selected, in_list, cx)
            }
            (_, action) => {
                let row = candidate_row(index, selected, cx);
                match action {
                    _ if is_problem_list => render_problem(row, candidate, selected),
                    Some(action) => render_step(row, candidate, action, selected),
                    None => render_candidate(row, minibuffer.prompt(), candidate, selected),
                }
            }
        };
        list = list.child(row);
    }
    list.when(group.is_none(), |list| list.pt(px(6.0)))
}

/// A row that answers the prompt with candidate `index` when clicked.
fn candidate_row(index: usize, selected: bool, cx: &mut Context<EditorView>) -> Div {
    let row = div().flex().flex_none().items_center().px(px(10.0)).rounded(px(8.0));
    lift_selected(row, selected).on_press(cx, move |view, cx| view.choose_candidate(index, cx))
}

pub(crate) fn secondary_color(selected: bool) -> u32 {
    if selected { color::fg_alt() } else { color::fg_dim() }
}

/// `matches` within `range` of a label, relative to the range's start.
pub(crate) fn matches_in(
    matches: &[Range<usize>],
    range: Range<usize>,
    selected: bool,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let accent = if selected { color::accent_on_selection() } else { color::accent() };
    let style = HighlightStyle::color(hsla(accent));
    matches
        .iter()
        .filter_map(|matched| {
            let (start, end) = (matched.start.max(range.start), matched.end.min(range.end));
            (start < end).then(|| (start - range.start..end - range.start, style))
        })
        .collect()
}

/// Status-size text that keeps its width.
fn status_label(text: impl IntoElement, text_color: u32) -> Div {
    div().flex_none().status_text(text_color).child(text)
}

/// What a step or an entry is, taking the room left.
fn annotation(candidate: &Candidate) -> Div {
    let annotation = candidate.annotation.clone();
    div().fill_truncated().text_size(px(font::ui_size())).child(annotation)
}

/// A problem: severity icon, message, source and `line:column`.
fn render_problem(row: Div, candidate: &Candidate, selected: bool) -> Div {
    let secondary = secondary_color(selected);
    let source = candidate.detail.clone().map(Cow::into_owned);
    let position = status_label(candidate.annotation.clone(), secondary).w(px(POSITION_WIDTH));
    let position = position.text_right().mono();
    row.gap(px(12.0))
        .h(px(WIDE_ROW_HEIGHT))
        .children(candidate.icon.map(|severity| icon(severity, secondary)))
        .child(render_message(candidate, selected))
        .children(source.map(|source| status_label(source, secondary)))
        .child(position)
}

/// A step of a plan: the action on it as a button, its id and what it is.
fn render_step(
    row: Div,
    candidate: &Candidate,
    (action, tone): (&'static str, Tone),
    selected: bool,
) -> Div {
    let action_color = tone_color(tone, color::fg_body());
    let id_color = if selected { color::fg_alt() } else { color::fg_line_number() };
    let chevron = svg().path(CHEVRON_DOWN_ICON).size(px(10.0)).fg(action_color);
    let button = div().row(0.0).flex_none().justify_between().w(px(88.0)).bar(24.0, 8.0);
    let button = button.rounded_fill(6.0, glass::control()).bordered(glass::ink(0x14));
    let button = button.mono_status(action_color).child(action).child(chevron);
    let id = div().flex_none().w(px(64.0)).mono_status(id_color);
    row.gap(px(12.0))
        .h(px(WIDE_ROW_HEIGHT))
        .child(button)
        .child(id.child(candidate.label.clone()))
        .child(annotation(candidate))
}

/// An entry: its id, what it is and when; selected, it opens up to show its items and
/// the buttons that act on it, lit while keys go to the list. A click selects it.
fn render_entry(
    candidate: &Candidate,
    expansion: &Expansion,
    index: usize,
    selected: bool,
    in_list: bool,
    cx: &mut Context<EditorView>,
) -> Div {
    let id = div().flex_none().px(px(6.0)).py(px(2.0)).rounded_fill(5.0, glass::ink(0x0f));
    let id = id.mono_status(color::fg_alt()).child(candidate.label.clone());
    let detail = match selected {
        true => expansion.detail.clone(),
        false => candidate.detail.clone().unwrap_or_default().into_owned(),
    };
    let header = div().row(10.0).flex_none().child(id).child(annotation(candidate));
    let header = header.child(status_label(detail, color::fg_faint()));
    let header = header.on_press(cx, move |view, cx| view.focus_candidate(index, cx));
    if !selected {
        return header.h(px(WIDE_ROW_HEIGHT)).px(px(10.0)).rounded(px(8.0));
    }
    let (border, background) = if in_list {
        (color::accent() << 8 | 0x61, color::accent() << 8 | 0x14)
    } else {
        (glass::ink(0x14), glass::ink(0x08))
    };
    let (destructive, actions): (Vec<_>, Vec<_>) =
        expansion.actions.iter().partition(|action| action.destructive);
    let mut button = |action: &EntryAction| {
        let command = action.command;
        let (border, text_color) = if action.destructive {
            (color::error() << 8 | 0x4d, color::fg_flagged())
        } else {
            (glass::ink(0x14), color::fg_main())
        };
        div()
            .row(8.0)
            .flex_none()
            .h(px(28.0))
            .pl(px(10.0))
            .pr(px(5.0))
            .rounded_fill(7.0, glass::control())
            .bordered(border)
            .shadow(vec![shadow(glass::shadow(0x80), 1.0, 2.0, 0.0)])
            .status_text(text_color)
            .child(action.label)
            .child(kbd(action.key, false).h(px(18.0)))
            .on_press(cx, move |view, cx| view.run_command(command, cx))
    };
    let buttons = div()
        .row(6.0)
        .children(actions.into_iter().map(&mut button))
        .child(div().flex_1())
        .children(destructive.into_iter().map(&mut button));
    div()
        .column()
        .flex_none()
        .gap(px(8.0))
        .pad(8.0, 10.0, 10.0)
        .rounded_fill(10.0, background)
        .bordered(border)
        .child(header.h(px(28.0)))
        .when(!expansion.items.is_empty(), |entry| {
            let items = list_items(expansion.items.clone(), MAX_LISTED_ITEMS, 26.0);
            let well = div().column().py(px(4.0)).rounded_fill(8.0, glass::field());
            entry.child(well.children(items))
        })
        .child(buttons)
}

/// A message whose `quoted` parts, as diagnostics write code, are set in the code font.
fn render_message(candidate: &Candidate, selected: bool) -> impl IntoElement {
    let (code_color, code_background) = if selected {
        (color::fg_main(), glass::ink(0x1f))
    } else {
        (color::fg_alt(), glass::ink(0x12))
    };
    let message = div().row(0.0).flex_1().min_w_0().overflow_hidden().whitespace_nowrap();
    let mut message = message.text_size(px(font::ui_size()));
    let mut start = 0;
    for (index, part) in candidate.label.split('`').enumerate() {
        let range = start..start + part.len();
        start = range.end + 1;
        if part.is_empty() {
            continue;
        }
        let highlights = matches_in(&candidate.matches, range, selected);
        let text = StyledText::new(part.to_owned()).with_highlights(highlights);
        // Odd parts are what the backticks quote.
        let piece = div().flex_none().when(index % 2 == 1, |code| {
            code.px(px(5.0)).rounded_fill(4.0, code_background).mono_status(code_color)
        });
        message = message.child(piece.child(text));
    }
    message
}

fn render_candidate(row: Div, prompt: &Prompt, candidate: &Candidate, selected: bool) -> Div {
    let is_command = prompt.category == COMMAND_CATEGORY;
    let secondary = secondary_color(selected);
    let matches = matches_in(&candidate.matches, 0..candidate.label.len(), selected);
    // Gives way to nothing but its own length: a long path is cut short.
    let name_color = if candidate.dim { color::fg_dim() } else { color::fg_main() };
    let label = StyledText::new(candidate.label.clone()).with_highlights(matches);
    let name = div().flex_initial().min_w_0().truncate().mono().text(font::mono_size(), name_color);
    let name = name.when(is_command, |name| name.w(px(COMMAND_NAME_WIDTH))).child(label);
    let annotation = div().fill_truncated().status_text(secondary);
    let detail = candidate.detail.clone().map(Cow::into_owned);
    let keys = div().flex().flex_none().gap(px(4.0));
    row.gap(px(if is_command { 14.0 } else { 10.0 }))
        .h(px(ROW_HEIGHT))
        .when(prompt.wide, |row| row.h(px(WIDE_ROW_HEIGHT)))
        .children(candidate.icon.map(|file| icon(file, secondary)))
        .when_some(candidate.verb, |row, verb| {
            row.child(div().flex_none().text_size(px(font::ui_size())).child(verb))
        })
        .child(name)
        .when(candidate.modified, |row| row.child(modified_dot()))
        .child(annotation.child(candidate.annotation.clone()))
        .children(candidate.tag.map(|tag| render_tag(tag, selected)))
        .children(detail.map(|detail| status_label(detail, secondary)))
        .when(!candidate.keys.is_empty(), |row| {
            row.child(keys.children(candidate.keys.iter().map(|key| kbd(key, selected))))
        })
}

fn render_tag(tag: &'static str, selected: bool) -> impl IntoElement {
    let (background, foreground) = if selected {
        (glass::ink(0x1f), color::fg_main())
    } else {
        (color::accent() << 8 | 0x24, color::fg_alt())
    };
    let chip = div().flex_none().px(px(7.0)).py(px(2.0)).rounded_fill(5.0, background);
    chip.text(font::small_size(), foreground).child(tag)
}
