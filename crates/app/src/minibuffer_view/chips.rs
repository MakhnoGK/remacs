use super::*;

/// Choices drawn as segments, every value shown, the chosen one lit; a click picks the
/// next.
pub(super) fn render_segmented_choices(
    minibuffer: &Minibuffer,
    cx: &mut Context<EditorView>,
) -> Option<Div> {
    let choices: Vec<Div> = shown_choices(minibuffer, true)
        .map(|(index, choice)| {
            let selected = choice.selected.get();
            let segments = choice
                .values
                .iter()
                .enumerate()
                .map(|(position, value)| segment(value, position == selected));
            choice_click(segmented().children(segments), index, cx)
        })
        .collect();
    (!choices.is_empty()).then(|| div().flex().gap(px(8.0)).px(px(10.0)).children(choices))
}

/// Switches between two settings, then what the prompt acts on, above the input.
pub(super) fn render_switches(minibuffer: &Minibuffer, cx: &mut Context<EditorView>) -> Div {
    let switches = shown_options(minibuffer, true).map(|(index, option)| {
        let segments = option.labels.iter().enumerate().map(|(position, label)| {
            let selected = (position == 1) == option.is_on();
            option_click(segment(label, selected), index, cx)
        });
        div()
            .row(6.0)
            .flex_none()
            .child(segmented().children(segments))
            .child(kbd(&format!("M-{}", option.key), false))
    });
    let subject = plain_text(minibuffer.subject(), font::ui_size(), color::fg_body());
    div().row(12.0).pad(2.0, 2.0, 10.0).children(switches).child(subject.min_w_0().truncate())
}

/// Choices, then settings that are on or off, under the lists; then the completions'
/// footnote, or what the prompt acts on if `with_subject`.
pub(super) fn render_chips(
    minibuffer: &Minibuffer,
    with_subject: bool,
    cx: &mut Context<EditorView>,
) -> Option<Div> {
    let footnote = minibuffer.completions().footnote.clone();
    let mut chips: Vec<Div> = shown_choices(minibuffer, false)
        .map(|(index, choice)| {
            let chip = choice_chip(choice.label, choice.value().unwrap_or_default());
            choice_click(chip, index, cx)
        })
        .collect();
    chips.extend(shown_options(minibuffer, false).map(|(index, option)| {
        let key = format!("M-{}", option.key);
        let chip = option_chip(&option.labels[0], &key, option.is_on());
        option_click(chip, index, cx)
    }));
    (!chips.is_empty()).then(|| {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .pad(12.0, 10.0, 2.0)
            .children(chips)
            .when_some(footnote, |chips, footnote| {
                let mark = div()
                    .size(px(9.0))
                    .rounded_full()
                    .border(px(1.6))
                    .border_color(rgb(color::info()));
                chips.child(div().flex_1()).child(
                    div()
                        .row(6.0)
                        .flex_none()
                        .status_text(color::fg_dim())
                        .child(mark)
                        .child(footnote),
                )
            })
            .when(with_subject && !minibuffer.subject().is_empty(), |chips| {
                let subject =
                    plain_text(minibuffer.subject(), font::status_size(), color::fg_faint());
                chips.child(div().flex_1()).child(subject.flex_none())
            })
    })
}

/// The prompt's choices, with their indexes, drawn as segments or not.
fn shown_choices(
    minibuffer: &Minibuffer,
    segmented: bool,
) -> impl Iterator<Item = (usize, &PromptChoice)> {
    let choices = minibuffer.prompt().choices.iter().enumerate();
    choices.filter(move |(_, choice)| choice.segmented == segmented)
}

/// The prompt's settings, with their indexes, that are shown and are switches or not.
fn shown_options(
    minibuffer: &Minibuffer,
    switches: bool,
) -> impl Iterator<Item = (usize, &PromptOption)> {
    let options = minibuffer.prompt().options.iter().enumerate();
    options.filter(move |(_, option)| option.is_switch() == switches && option.is_shown())
}

/// What choosing would make: a summary, then the base, a dot per commit and the tip.
pub(super) fn render_outcome((summary, chain): (String, CommitChain)) -> impl IntoElement {
    let dot = |color_hex: u32| {
        div()
            .flex_none()
            .size(px(10.0))
            .rounded_full()
            .bg(rgb(color_hex))
            .border_2()
            .border_color(rgb(color::bg_popup()))
    };
    let line = |from: u32, to: u32| {
        div().w(px(36.0)).h(px(2.0)).bg(linear_gradient(
            90.0,
            linear_color_stop(hsla(from), 0.0),
            linear_color_stop(hsla(to), 1.0),
        ))
    };
    // A remote's commit, like a merge base, is grey; what comes in is the base's color.
    let base_color = match chain.base.kind {
        RefKind::Remote => color::fg_line_number(),
        _ => color::graph(1),
    };
    let (incoming_color, tip_color) = (color::graph(1), color::graph(0));
    let mut chain_row = div()
        .flex()
        .items_center()
        .py(px(6.0))
        .px(px(10.0))
        .child(ref_chip(chain.base).mr(px(12.0)))
        .child(dot(base_color));
    let mut previous = base_color;
    let colors = std::iter::repeat_n(incoming_color, chain.incoming)
        .chain(std::iter::repeat_n(tip_color, chain.commits));
    for dot_color in colors {
        chain_row = chain_row.child(line(previous, dot_color)).child(dot(dot_color));
        previous = dot_color;
    }
    let summary = Some(summary).filter(|summary| !summary.is_empty());
    let note = |note| div().px(px(10.0)).status_text(color::fg_dim()).child(note);
    div()
        .child(render_group_title("Result", summary, false))
        .child(chain_row.child(ref_chip(chain.tip).ml(px(12.0))))
        .children(chain.note.map(note))
}

fn option_click(element: Div, index: usize, cx: &mut Context<EditorView>) -> Div {
    element.on_press(cx, move |view, cx| view.toggle_prompt_option(index, cx))
}

fn choice_click(element: Div, index: usize, cx: &mut Context<EditorView>) -> Div {
    element.on_press(cx, move |view, cx| view.next_prompt_choice(index, cx))
}
