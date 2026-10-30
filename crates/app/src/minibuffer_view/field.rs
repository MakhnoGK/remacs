use super::*;

pub(super) fn render_field(
    minibuffer: &Minibuffer,
    input_scroll: &ScrollHandle,
    composition: Option<&Composition>,
    input_margin: Pixels,
) -> impl IntoElement {
    let prompt = minibuffer.prompt();
    let label = match minibuffer.label() {
        keys if parse_sequence(keys).is_some() => quote(keys),
        label => label.to_owned(),
    };
    let label = plain_text(&label, font::status_size(), color::fg_alt())
        .flex()
        .flex_none()
        .items_center()
        .h(px(24.0))
        .px(px(8.0))
        .rounded_fill(6.0, glass::chip());
    let end = if is_problem_list(minibuffer) {
        render_severity_counts(minibuffer.candidates()).into_any_element()
    } else {
        let (family, text_color, text) = match &prompt.message {
            Some(message) => {
                let length = minibuffer.input_text().chars().count();
                let over = length > message.subject_limit;
                let text_color = if over { color::warning() } else { color::fg_dim() };
                let text = format!("{length}/{}", message.subject_limit);
                (font::mono(), text_color, text)
            }
            None => {
                let position = prompt.count == Count::Position;
                let family = if position { font::mono() } else { font::ui() };
                let text = minibuffer.note().map_or_else(|| count_text(minibuffer), str::to_owned);
                (family, color::fg_dim(), text)
            }
        };
        div().flex_none().font_family(family).status_text(text_color).child(text).into_any_element()
    };
    let input = render_input(minibuffer, input_scroll, composition, input_margin);
    field_frame(!minibuffer.in_description() && !minibuffer.in_list())
        .row(10.0)
        .h(px(FIELD_HEIGHT))
        .child(label)
        .child(input)
        .child(end)
}

/// A field's well, lit while it is the one being typed in.
fn field_frame(active: bool) -> Div {
    let border = if active { color::accent() << 8 | 0x59 } else { glass::ink(0x0f) };
    div().px(px(12.0)).rounded_fill(8.0, glass::field()).bordered(border)
}

fn placeholder(text: &'static str) -> Div {
    div().fg(color::fg_placeholder()).child(text)
}

/// How many of the listed problems are errors, warnings and notes.
fn render_severity_counts(candidates: &[Candidate]) -> impl IntoElement {
    let count = |severity| {
        let of_severity = |candidate: &&Candidate| candidate.icon == Some(severity);
        candidates.iter().filter(of_severity).count().to_string()
    };
    let counts = SEVERITY_ICONS.map(|severity| {
        div()
            .row(6.0)
            .h(px(26.0))
            .px(px(9.0))
            .rounded_fill(6.0, glass::control())
            .bordered(glass::ink(0x12))
            .mono_status(color::fg_kbd())
            .child(icon(severity, color::fg_kbd()))
            .child(count(severity))
    });
    div().flex().flex_none().gap(px(4.0)).children(counts)
}

/// Scrolled sideways just enough to keep the cursor and `margin` on each side of it in
/// view, like Emacs `hscroll`: the text before it, the cursor and the text after it are
/// separate children so the view can scroll to the cursor's.
fn render_input(
    minibuffer: &Minibuffer,
    scroll: &ScrollHandle,
    composition: Option<&Composition>,
    margin: Pixels,
) -> impl IntoElement {
    let input = minibuffer.input();
    let mut line = DisplayLine::new(&minibuffer.input_text());
    let cursor = line.cursor_cell(input.point_column());
    let part = |range: Range<usize>| div().flex_none().child(line.text[range].to_owned());
    let typed = !minibuffer.in_description() && minibuffer.is_typed();
    let composition = composition.filter(|_| typed);
    let cursor_part = if typed {
        block_cursor().children(composition.map(Composition::track_caret))
    } else {
        div().flex_none()
    };
    // Padding the view scrolls into sight with the cursor, taken back by as much margin so
    // the text does not move; none past the end, where the text before fills the field.
    let text_after = cursor.end < line.text.len();
    let cursor_with_margin = div()
        .flex_none()
        .pl(margin)
        .ml(-margin)
        .when(text_after, |part| part.pr(margin).mr(-margin))
        .flex()
        // Scrolled into sight with the cursor.
        .children(composition.and_then(Composition::render_marked))
        .child(cursor_part.child(line.text[cursor.clone()].to_owned()));
    scroll.scroll_to_item(CURSOR_CHILD);
    div()
        .id("minibuffer-input")
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_x_scroll()
        .track_scroll(scroll)
        .whitespace_nowrap()
        .font_family(font::mono())
        .text_size(px(font::input_size()))
        .child(part(0..cursor.start))
        .child(cursor_with_margin)
        .child(part(cursor.end..line.text.len()))
        .when(input.text().is_empty(), |field| {
            field.child(placeholder(minibuffer.prompt().placeholder))
        })
}

/// A message's description under its subject: a few lines, scrolled to the cursor, with
/// `composition` before it while typed in.
pub(super) fn render_description(
    minibuffer: &Minibuffer,
    composition: Option<&Composition>,
) -> Option<impl IntoElement> {
    let description = minibuffer.description()?;
    let active = minibuffer.in_description();
    let text = description.contents(0..description.text().len());
    let point_line = description.point_line();
    let first = if active { point_line.saturating_sub(DESCRIPTION_LINES - 1) } else { 0 };
    let lines =
        text.split('\n').enumerate().skip(first).take(DESCRIPTION_LINES).map(|(index, source)| {
            let mut line = DisplayLine::new(source);
            let text = if active && index == point_line {
                let cursor = line.cursor_cell(description.point_column());
                text_with_cursor(&line.text, &Vec::new(), cursor, composition)
            } else {
                div().child(line.text)
            };
            div().h(px(DESCRIPTION_LINE_HEIGHT)).whitespace_nowrap().overflow_hidden().child(text)
        });
    let message = minibuffer.prompt().message.as_ref();
    let placeholder_text = message.map_or("", |message| message.placeholder);
    let field = field_frame(active)
        .column()
        .h(px(DESCRIPTION_LINE_HEIGHT * DESCRIPTION_LINES as f32 + 16.0))
        .py(px(8.0))
        .font_family(font::mono())
        .text_size(px(font::mono_size()))
        .line_height(px(DESCRIPTION_LINE_HEIGHT))
        .fg(color::fg_body())
        .children(lines)
        .when(text.is_empty() && !active, |field| field.child(placeholder(placeholder_text)));
    let label = render_group_title("Description", None, false);
    Some(div().child(label).child(field))
}

fn count_text(minibuffer: &Minibuffer) -> String {
    let total = minibuffer.total();
    match minibuffer.prompt().count {
        Count::Position => {
            format!("{}/{total}", minibuffer.selected().map_or(0, |index| index + 1))
        }
        Count::Matches if total == 1 => "1 match".into(),
        Count::Matches => format!("{total} matches"),
        Count::Total => total.to_string(),
    }
}
