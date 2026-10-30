use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;

use gpui::{
    AnyElement, Div, ElementId, FontWeight, HighlightStyle, IntoElement, ParentElement, Pixels,
    Point, ScrollHandle, Stateful, StyledText, div, prelude::*, px, rgba,
};
use remacs_core::{Candidate, Code, DocBlock, Documentation, Face, Note, Popup, SymbolKind};
use remacs_theme::{color, font, glass};

use crate::code_wrap::{wrap_code, wrapped_offset};
use crate::highlights::{Highlights, combine, face_style};
use crate::minibuffer_view::{matches_in, secondary_color};
use crate::outline_view::{chip_frame, lift_selected};
use crate::widgets::{StyleExt, glass_surface, hsla, icon, key_hint};

const LIST_WIDTH: f32 = 440.0;
/// Of documentation beside the list, which grows wider with its code.
const DOCUMENTATION_WIDTH: f32 = 360.0;
const DOCUMENTATION_PADDING_X: f32 = 14.0;
const BODY_LINE_HEIGHT: f32 = 20.0;
const CODE_BLOCK_PADDING_X: f32 = 8.0;
const ROW_HEIGHT: f32 = 28.0;
const ROW_GAP: f32 = 2.0;
const LIST_PADDING: f32 = 6.0;
const ROW_PADDING: f32 = 6.0;
const BADGE_SIZE: f32 = 18.0;
const BADGE_GAP: f32 = 8.0;
const DETAIL_MAX_WIDTH: f32 = 200.0;
/// Between the line and the popup.
const POPUP_GAP: f32 = 6.0;
/// Between the list and the documentation.
const PANEL_GAP: f32 = 8.0;
const WINDOW_MARGIN: f32 = 8.0;
/// From the list's edge to its labels, which line up with the completed word.
const LABEL_INSET: f32 = 1.0 + LIST_PADDING + ROW_PADDING + BADGE_SIZE + BADGE_GAP;
/// Of documentation alone, which grows wider with its code.
const HOVER_WIDTH: f32 = 520.0;
const HOVER_PADDING: f32 = 16.0;
const HOVER_SIGNATURE_PADDING_X: f32 = 12.0;
/// From the documentation's edge to its text, which lines up with the symbol.
const HOVER_INSET: f32 = 1.0 + HOVER_PADDING;
/// Room below the line that is enough for documentation, which is otherwise put where
/// there is more room.
const HOVER_ENOUGH_ROOM: f32 = 320.0;
/// Room above the line that is enough for a popup that goes above, like a signature.
const ABOVE_ENOUGH_ROOM: f32 = 120.0;
const NOTES_PADDING: f32 = 4.0;
const NOTE_PADDING_X: f32 = 10.0;
const NOTE_PADDING_Y: f32 = 8.0;
const NOTE_ICON_SIZE: f32 = 10.0;
const NOTE_ICON_GAP: f32 = 10.0;
/// From the notes' edge to their messages, which line up with the anchor.
const NOTES_INSET: f32 = 1.0 + NOTES_PADDING + NOTE_PADDING_X + NOTE_ICON_SIZE + NOTE_ICON_GAP;
/// Of the notes beside their messages: borders, paddings and icons.
const NOTES_INSETS_X: f32 = NOTES_INSET + NOTE_PADDING_X + NOTES_PADDING + 1.0;
const NOTE_LINE_HEIGHT: f32 = 20.0;
const NOTE_LINE_GAP: f32 = 2.0;
const NOTE_SOURCE_LINE_HEIGHT: f32 = 16.0;
/// Of code quoted in a note.
const CHIP_PADDING: f32 = 5.0;
const CHIP_RADIUS: f32 = 4.0;

/// Of a code block, beside its code.
const CODE_BLOCK_INSET: f32 = 2.0 * CODE_BLOCK_PADDING_X;

/// How wide a documentation panel is at least, and the horizontal room around its code:
/// the panel's padding and border, and those of the signature's box.
struct CodeInsets {
    min_width: f32,
    panel: f32,
    signature: f32,
}

const HOVER_INSETS: CodeInsets = CodeInsets {
    min_width: HOVER_WIDTH,
    panel: 2.0 * (1.0 + HOVER_PADDING),
    signature: 2.0 * (1.0 + HOVER_SIGNATURE_PADDING_X),
};
const DOCUMENTATION_INSETS: CodeInsets = CodeInsets {
    min_width: DOCUMENTATION_WIDTH,
    panel: 2.0 * (1.0 + DOCUMENTATION_PADDING_X),
    signature: 0.0,
};

/// Where a popup goes: `anchor_left` is where the anchor is drawn, `top`/`bottom` the
/// line's; nothing goes above `area_top`, e.g. the title bar.
pub struct Placement {
    pub anchor_left: Pixels,
    pub line_top: Pixels,
    pub line_bottom: Pixels,
    pub area_top: Pixels,
    pub window_width: Pixels,
    pub window_height: Pixels,
}

impl Placement {
    /// The popup's left edge, `inset` before the anchor and inside the window.
    fn left(&self, inset: f32, width: f32) -> Pixels {
        (self.anchor_left - px(inset))
            .min(self.window_width - px(width + WINDOW_MARGIN))
            .max(px(WINDOW_MARGIN))
    }

    /// The room below the line, or above it.
    fn room(&self, below: bool) -> Pixels {
        let (top, bottom) = if below {
            (self.line_bottom, self.window_height)
        } else {
            (self.area_top, self.line_top)
        };
        bottom - top - px(POPUP_GAP + WINDOW_MARGIN)
    }

    /// At most `max_width`, and no wider than the window.
    fn max_width(&self, max_width: Pixels) -> Pixels {
        max_width.min(self.window_width - px(2.0 * WINDOW_MARGIN))
    }

    /// `panels` with their left edge at `left`, below the line or above it.
    fn place(&self, left: Pixels, below: bool, panels: Div) -> AnyElement {
        let panels = panels.absolute().left(left).flex();
        if below {
            panels.top(self.line_bottom + px(POPUP_GAP)).items_start()
        } else {
            let bottom = self.window_height - self.line_top + px(POPUP_GAP);
            panels.bottom(bottom).items_end()
        }
        .into_any_element()
    }
}

/// What popups take from the view: faces for documentation's code, the scroll position of
/// its body, which the mouse wheel and the plugin's page keys move, how wide a character of
/// text is, and how large documentation and notes grow before they wrap and scroll.
pub struct PopupView<'a> {
    pub code_faces: &'a dyn Fn(&Code) -> Highlights,
    pub scroll: &'a ScrollHandle,
    /// Of signatures.
    pub mono_width: Pixels,
    /// Of code blocks in documentation and code quoted in notes.
    pub small_mono_width: Pixels,
    /// About, of text in notes.
    pub ui_char_width: Pixels,
    pub max_width: Pixels,
    pub max_height: Pixels,
    pub notes_max_width: Pixels,
    pub notes_max_height: Pixels,
    /// Completions naming a color show it in place of their kind.
    pub completion_swatches: bool,
}

/// Keeps the shown documentation's scroll position between frames: new documentation
/// starts at the top, and each page the plugin scrolled by moves it a body height.
#[derive(Default)]
pub struct DocumentationScroll {
    pub handle: ScrollHandle,
    /// Says which documentation is shown, and its pages as last applied.
    shown: Option<(DocumentationKey, i32)>,
}

/// The same symbol's documentation, even when more of it is shown.
#[derive(PartialEq)]
struct DocumentationKey {
    anchor: usize,
    container: String,
    kind: String,
    signature: Option<String>,
}

impl DocumentationScroll {
    pub fn update<'a>(&mut self, popups: impl IntoIterator<Item = &'a Popup>) {
        let shown = popups.into_iter().find_map(|popup| {
            let documentation = popup.documentation.as_ref()?;
            let key = DocumentationKey {
                anchor: popup.anchor,
                container: documentation.container.clone(),
                kind: documentation.kind.clone(),
                signature: documentation.signature.as_ref().map(|code| code.text.clone()),
            };
            Some((key, popup.documentation_pages))
        });
        let applied = match (&shown, &self.shown) {
            (Some((key, _)), Some((shown_key, applied))) if key == shown_key => *applied,
            _ => {
                self.handle.set_offset(Point::default());
                0
            }
        };
        if let Some((_, pages)) = &shown {
            let page =
                (self.handle.bounds().size.height - px(BODY_LINE_HEIGHT)).max(px(BODY_LINE_HEIGHT));
            let mut offset = self.handle.offset();
            // gpui clamps the offset to the content when it lays the body out.
            offset.y -= page * (pages - applied) as f32;
            self.handle.set_offset(offset);
        }
        self.shown = shown;
    }
}

/// A list with the selected row's documentation beside it, documentation alone, or notes.
pub fn render_popup(popup: &Popup, placement: Placement, view: &PopupView) -> AnyElement {
    match &popup.documentation {
        Some(documentation) if popup.rows.is_empty() => {
            render_hover(documentation, popup, &placement, view)
        }
        None if popup.rows.is_empty() => render_notes(&popup.notes, &placement, view),
        _ => render_list_popup(popup, &placement, view),
    }
}

/// An id for an element showing `contents`, which a change of them changes.
fn content_id(name: &'static str, contents: impl IntoIterator<Item = impl Hash>) -> ElementId {
    let mut hasher = DefaultHasher::new();
    contents.into_iter().for_each(|content| content.hash(&mut hasher));
    ElementId::from((name, hasher.finish()))
}

/// A column that scrolls, giving way first when its panel is short of room.
fn scrolling_column(id: ElementId) -> Stateful<Div> {
    div().id(id).flex().flex_col().flex_shrink().min_h_0().overflow_y_scroll()
}

/// Above the line, their messages in line with the anchor, or below when they fit there
/// and not above, or there is more room; leaves the text after the anchor in view. Longer
/// than the room, they scroll.
fn render_notes(notes: &[Note], placement: &Placement, view: &PopupView) -> AnyElement {
    let width = placement.max_width(view.notes_max_width);
    let text_width = width - px(NOTES_INSETS_X);
    let code_chars = chip_chars(text_width, view);
    let height = notes_height(notes, text_width, code_chars, view).min(view.notes_max_height);
    let (below, above) = (placement.room(true), placement.room(false));
    let fits_above = above >= height || above >= below;
    // New notes start at the top.
    let id = content_id("notes", notes.iter().map(|note| (&note.message, &note.source)));
    let notes = notes.iter().enumerate();
    let notes = notes.map(|(index, note)| render_note(note, index > 0, code_chars));
    let panel = panel()
        .bg(rgba(glass::note()))
        .border_color(rgba(glass::note_border()))
        .max_w(width)
        .max_h(placement.room(!fits_above).min(view.notes_max_height))
        .overflow_hidden()
        .p(px(NOTES_PADDING))
        .child(scrolling_column(id).children(notes));
    let left = placement.left(NOTES_INSET, width.into());
    placement.place(left, !fits_above, div().flex_col().child(panel))
}

/// The note's icon by its message and source, `ruled` off from the note before it.
fn render_note(note: &Note, ruled: bool, code_chars: usize) -> Div {
    let lines = note.message.lines().map(|line| {
        render_text_line(line, code_chars, &NOTE_CHIP)
            .text_size(px(font::ui_size()))
            .line_height(px(NOTE_LINE_HEIGHT))
    });
    let source = (!note.source.is_empty()).then(|| {
        let source = div().text(font::small_size(), color::fg_note_source());
        source.line_height(px(NOTE_SOURCE_LINE_HEIGHT)).child(note.source.clone())
    });
    let note_icon = div().flex().flex_none().pt(px(4.0));
    let text = div().column().min_w_0().gap(px(NOTE_LINE_GAP));
    let frame = div().flex().flex_none().gap(px(NOTE_ICON_GAP)).px(px(NOTE_PADDING_X));
    frame
        .py(px(NOTE_PADDING_Y))
        .when(ruled, |note| note.top_border(glass::note_separator()))
        .child(note_icon.child(icon(note.icon, color::fg_main())))
        .child(text.children(lines).children(source))
}

/// Colors of code quoted in a line of text.
struct ChipColors {
    background: fn() -> u32,
    text: fn() -> u32,
}

const NOTE_CHIP: ChipColors =
    ChipColors { background: glass::note_code, text: color::fg_note_code };

const DOCUMENTATION_CHIP: ChipColors =
    ChipColors { background: || glass::ink(0x12), text: color::fg_alt };

/// Wraps between words, and inside a word wider than the line; quoted code is a chip in
/// the code font, which text highlights cannot give, that wraps between its words like
/// the text, each line of it a strip. A code word longer than `code_chars`, what fits on a
/// line, is broken into pieces that fit.
fn render_text_line(line: &str, code_chars: usize, chip: &ChipColors) -> Div {
    let (background, text_color) = ((chip.background)(), (chip.text)());
    let (padding, radius) = (px(CHIP_PADDING), px(CHIP_RADIUS));
    let pieces = line_pieces(line, code_chars).into_iter();
    let pieces = pieces.map(|piece| match piece {
        Piece::Word(word) => div().max_w_full().child(word.to_owned()),
        Piece::Code { text, first, last } => div()
            .flex_none()
            .whitespace_nowrap()
            .when(first, |chip| chip.pl(padding).rounded_l(radius))
            .when(last, |chip| chip.pr(padding).rounded_r(radius))
            .bg(rgba(background))
            .mono_status(text_color)
            .child(text.to_owned()),
    });
    div().flex().flex_wrap().items_center().children(pieces)
}

/// Characters of quoted code that fit on a line of text `text_width` wide.
fn chip_chars(text_width: Pixels, view: &PopupView) -> usize {
    let chars = ((text_width - px(2.0 * CHIP_PADDING)) / view.small_mono_width).floor();
    (chars as usize).max(1)
}

/// About how tall the notes are drawn with messages `text_width` wide, known before they
/// are laid out, to choose the side of the line with room for them.
fn notes_height(notes: &[Note], text_width: Pixels, code_chars: usize, view: &PopupView) -> Pixels {
    let line_count = |line: &str| {
        let pieces = line_pieces(line, code_chars).into_iter();
        let widths = pieces.map(|piece| piece.width(view.ui_char_width, view.small_mono_width));
        wrapped_lines(widths, text_width)
    };
    let note_height = |note: &Note| {
        let lines: usize = note.message.lines().map(line_count).sum();
        let source =
            if note.source.is_empty() { 0.0 } else { NOTE_LINE_GAP + NOTE_SOURCE_LINE_HEIGHT };
        let gaps = lines.saturating_sub(1) as f32 * NOTE_LINE_GAP;
        1.0 + 2.0 * NOTE_PADDING_Y + lines as f32 * NOTE_LINE_HEIGHT + gaps + source
    };
    px(notes.iter().map(note_height).sum::<f32>() + 2.0 * (1.0 + NOTES_PADDING))
}

/// Lines that pieces as wide as `widths` take, each wrapped where the next one would go
/// past `line_width`, or inside one wider than a line.
fn wrapped_lines(widths: impl IntoIterator<Item = Pixels>, line_width: Pixels) -> usize {
    let mut lines = 1;
    let mut filled = px(0.0);
    for width in widths {
        if filled > px(0.0) && filled + width > line_width {
            lines += 1;
            filled = px(0.0);
        }
        let whole_lines = (width / line_width).ceil().max(1.0) as usize;
        lines += whole_lines - 1;
        filled += width - line_width * (whole_lines - 1) as f32;
    }
    lines
}

#[derive(Debug, PartialEq)]
enum Piece<'a> {
    /// With the spaces after it, or a run of spaces alone.
    Word(&'a str),
    /// A word of quoted code with the spaces after it, or `code_chars` of a longer one;
    /// `first` and `last` of the quote.
    Code { text: &'a str, first: bool, last: bool },
}

impl Piece<'_> {
    /// About, drawn with characters of text and of code this wide.
    fn width(&self, char_width: Pixels, code_width: Pixels) -> Pixels {
        match *self {
            Piece::Word(word) => char_width * word.chars().count() as f32,
            Piece::Code { text, first, last } => {
                let padding = [first, last].into_iter().filter(|&end| end).count() as f32;
                code_width * text.chars().count() as f32 + px(CHIP_PADDING * padding)
            }
        }
    }
}

/// The line's words and those of the code its backticks quote, none of the code's longer
/// than `code_chars`.
fn line_pieces(line: &str, code_chars: usize) -> Vec<Piece<'_>> {
    line.split('`')
        .enumerate()
        .filter(|(_, part)| !part.is_empty())
        .flat_map(|(index, part)| -> Vec<Piece> {
            if index % 2 == 0 {
                return part.split_inclusive(' ').map(Piece::Word).collect();
            }
            let words = part.split_inclusive(' ');
            let texts: Vec<&str> = words.flat_map(|word| chunks(word, code_chars)).collect();
            let count = texts.len();
            let piece =
                |(index, text)| Piece::Code { text, first: index == 0, last: index + 1 == count };
            texts.into_iter().enumerate().map(piece).collect()
        })
        .collect()
}

/// `text` in pieces of `chars` characters, the last one shorter.
fn chunks(text: &str, chars: usize) -> Vec<&str> {
    let starts = text.char_indices().map(|(start, _)| start);
    let starts: Vec<usize> = starts.step_by(chars.max(1)).chain([text.len()]).collect();
    starts.windows(2).map(|pair| &text[pair[0]..pair[1]]).collect()
}

/// Below the symbol, its text in line with the symbol's, or above when there is more room;
/// the other way round for a popup that goes above.
fn render_hover(
    documentation: &Documentation,
    popup: &Popup,
    placement: &Placement,
    view: &PopupView,
) -> AnyElement {
    let (below, above) = (placement.room(true), placement.room(false));
    let fits_below = if popup.above {
        above < px(ABOVE_ENOUGH_ROOM) && below > above
    } else {
        below >= px(HOVER_ENOUGH_ROOM) || below >= above
    };
    let max_width = placement.max_width(view.max_width);
    let (width, documentation, code_chars) = fit(documentation, &HOVER_INSETS, max_width, view);
    let has_header = !documentation.container.is_empty() || !documentation.kind.is_empty();
    let signature = documentation.signature.as_ref().map(|signature| {
        let signature = signature_box(signature, font::LINE_HEIGHT, view);
        let signature = signature.px(px(HOVER_SIGNATURE_PADDING_X)).py(px(8.0));
        signature.rounded_fill(8.0, glass::field()).bordered(glass::ink(0x0f)).into_any_element()
    });
    let container = div().min_w_0().truncate().child(documentation.container.clone());
    let kind = div().flex_none().child(documentation.kind.clone());
    let header = div().flex().flex_none().justify_between().gap(px(12.0)).child(container);
    let header = header.text(font::small_size(), color::fg_dim()).child(kind);
    let panel = panel()
        .w(width)
        .max_h(placement.room(fits_below).min(view.max_height))
        .overflow_hidden()
        .pad(14.0, HOVER_PADDING, 12.0)
        .gap(px(10.0))
        .when(has_header, |panel| panel.child(header))
        .when(!documentation.is_empty(), |panel| {
            // A signature's popup leaves the scroll position to other documentation shown.
            if popup.above && documentation.blocks.is_empty() {
                return panel.children(signature);
            }
            let blocks = &documentation.blocks;
            panel.child(render_body(signature, blocks, 10.0, code_chars, view))
        })
        .when(!popup.hints.is_empty(), |panel| {
            panel.child(render_hints(popup.hints).gap(px(18.0)).pt(px(10.0)))
        });
    let left = placement.left(HOVER_INSET, width.into());
    placement.place(left, fits_below, div().flex_col().child(panel))
}

/// The list below the line, or above it when it does not fit; the documentation beside
/// it, on the right if there is room.
fn render_list_popup(popup: &Popup, placement: &Placement, view: &PopupView) -> AnyElement {
    let rows = popup.rows.len() as f32;
    let list_height =
        rows * ROW_HEIGHT + (rows - 1.0).max(0.0) * ROW_GAP + 2.0 * (LIST_PADDING + 1.0);
    let below = placement.room(true) >= px(list_height);
    let list_left = placement.left(LABEL_INSET, LIST_WIDTH);
    let list_right = list_left + px(LIST_WIDTH + PANEL_GAP);
    let room_right = placement.window_width - px(WINDOW_MARGIN) - list_right;
    let room_left = list_left - px(PANEL_GAP + WINDOW_MARGIN);
    let room_beside = room_right.max(room_left).max(px(DOCUMENTATION_WIDTH));
    let max_width = view.max_width.min(room_beside);
    let documentation = popup.documentation.as_ref().map(|documentation| {
        let (width, documentation, code_chars) =
            fit(documentation, &DOCUMENTATION_INSETS, max_width, view);
        let room = placement.room(below);
        let panel = render_documentation(&documentation, popup.hints, room, code_chars, view);
        (width, panel.w(width).into_any_element())
    });
    let list = render_list(popup, view);
    let (left, children) = match documentation {
        Some((width, panel)) if width > room_right => {
            (list_left - width - px(PANEL_GAP), vec![panel, list])
        }
        Some((_, panel)) => (list_left, vec![list, panel]),
        None => (list_left, vec![list]),
    };
    placement.place(left, below, div().gap(px(PANEL_GAP)).children(children))
}

/// Takes the mouse, so the wheel scrolls the popup and not the buffer behind it.
fn panel() -> Div {
    glass_surface(glass::HINT_RADIUS).occlude().column().flex_none().ui().fg(color::fg_main())
}

fn render_list(popup: &Popup, view: &PopupView) -> AnyElement {
    let rows = popup.rows.iter().enumerate().map(|(index, row)| {
        let swatch = row.swatch.filter(|_| view.completion_swatches);
        render_row(row, swatch, popup.selected == Some(index))
    });
    panel().w(px(LIST_WIDTH)).p(px(LIST_PADDING)).gap(px(ROW_GAP)).children(rows).into_any_element()
}

fn render_row(row: &Candidate, swatch: Option<u32>, selected: bool) -> impl IntoElement {
    let label_matches = matches_in(&row.matches, 0..row.label.len(), selected);
    let label = StyledText::new(row.label.clone()).with_highlights(label_matches);
    let badge = match (swatch, row.kind) {
        (Some(color), _) => render_swatch(color),
        (None, Some(kind)) => render_badge(kind),
        (None, None) => div().flex_none().w(px(BADGE_SIZE)),
    };
    let detail = row.detail.clone().map(|text| {
        let detail = div().flex_none().max_w(px(DETAIL_MAX_WIDTH)).truncate();
        detail.status_text(secondary_color(selected)).child(text.into_owned())
    });
    // The label takes the row's text size; the badge and the detail set their own.
    lift_selected(chip_frame(ROW_HEIGHT, ROW_PADDING, 6.0), selected)
        .gap(px(BADGE_GAP))
        .mono()
        .text_size(px(font::mono_size()))
        .child(badge)
        .child(div().fill_truncated().child(label))
        .children(detail)
}

/// The color, `0xRRGGBBAA`, in place of the kind's badge; outlined, so white and
/// translucent colors show on any glass.
fn render_swatch(color: u32) -> Div {
    let badge = div().flex_none().size(px(BADGE_SIZE)).rounded(px(5.0));
    badge.bg(rgba(color)).bordered(color::fg_dim() << 8 | 0x40)
}

/// A letter on a tint of the color code of its kind takes, like `f` for a field.
fn render_badge(kind: SymbolKind) -> Div {
    let (letter, tint) = match kind {
        SymbolKind::Function => ("λ", color::function()),
        SymbolKind::Method => ("m", color::function()),
        SymbolKind::Field => ("f", color::type_name()),
        SymbolKind::Variable => ("v", color::string()),
        SymbolKind::Constant => ("c", color::constant()),
        SymbolKind::EnumMember => ("e", color::constant()),
        SymbolKind::Type => ("t", color::type_name()),
        SymbolKind::Interface => ("i", color::type_name()),
        SymbolKind::Module => ("n", color::keyword()),
        SymbolKind::Keyword => ("k", color::keyword()),
        SymbolKind::Snippet => ("s", color::builtin()),
        SymbolKind::Text => ("w", color::fg_dim()),
    };
    let badge = div().row(0.0).flex_none().justify_center().size(px(BADGE_SIZE));
    badge.rounded_fill(5.0, tint << 8 | 0x24).text(font::small_size(), tint).child(letter)
}

/// At most `room` tall: the body gives way first, so the hints stay in view.
fn render_documentation(
    documentation: &Documentation,
    hints: &'static [(&'static str, &'static str)],
    room: Pixels,
    code_chars: usize,
    view: &PopupView,
) -> Div {
    let parts = [documentation.container.as_str(), &documentation.kind];
    let title = parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");
    let title_text = div().flex_none().text(font::small_size(), color::fg_dim());
    let title = (!title.is_empty()).then(|| title_text.child(title));
    let signature = documentation.signature.as_ref();
    let signature = signature.map(|code| signature_box(code, BODY_LINE_HEIGHT, view));
    let blocks = &documentation.blocks;
    panel()
        .max_h(room.min(view.max_height))
        .overflow_hidden()
        .px(px(DOCUMENTATION_PADDING_X))
        .py(px(12.0))
        .gap(px(8.0))
        .children(title)
        .when(!documentation.is_empty(), |panel| {
            let signature = signature.map(IntoElement::into_any_element);
            panel.child(render_body(signature, blocks, 8.0, code_chars, view))
        })
        .when(!hints.is_empty(), |panel| panel.child(render_hints(hints)))
}

/// The documentation with its code wrapped to fit `max_width`, how wide it is drawn: as
/// wide as its widest code, from the insets' `min_width` up to `max_width`, and how many
/// characters of quoted code fit on a line of its text.
fn fit(
    documentation: &Documentation,
    insets: &CodeInsets,
    max_width: Pixels,
    view: &PopupView,
) -> (Pixels, Documentation, usize) {
    let mut widest = px(0.0);
    let mut fit_code = |code: &Code, inset: f32, char_width: Pixels| {
        let room = max_width - px(insets.panel + inset);
        let text = wrap_code(&code.text, (room / char_width).floor().max(1.0) as usize);
        let longest = text.lines().map(|line| line.chars().count()).max();
        let width = char_width * longest.unwrap_or(0) as f32 + px(insets.panel + inset);
        widest = widest.max(width.ceil());
        let offset = |at| wrapped_offset(&code.text, &text, at);
        let emphasis = code.emphasis.as_ref();
        let emphasis = emphasis.map(|range| offset(range.start)..offset(range.end));
        Code { emphasis, ..Code::new(text, code.mode) }
    };
    let signature = documentation.signature.as_ref();
    let signature = signature.map(|code| fit_code(code, insets.signature, view.mono_width));
    let code_width = view.small_mono_width;
    let blocks = (documentation.blocks.iter())
        .map(|block| match block {
            DocBlock::Code(code) => DocBlock::Code(fit_code(code, CODE_BLOCK_INSET, code_width)),
            block => block.clone(),
        })
        .collect();
    let fitted = Documentation {
        container: documentation.container.clone(),
        kind: documentation.kind.clone(),
        signature,
        blocks,
    };
    let width = widest.max(px(insets.min_width)).min(max_width);
    (width, fitted, chip_chars(width - px(insets.panel), view))
}

/// The signature's code in a box that keeps its height, lines `line_height` tall.
fn signature_box(signature: &Code, line_height: f32, view: &PopupView) -> Stateful<Div> {
    code_box("documentation-signature", 0, signature, view)
        .flex_none()
        .text_size(px(font::mono_size()))
        .line_height(px(line_height))
}

/// Code as it was wrapped; a line still longer than the box scrolls sideways. `index` tells
/// boxes of the same code apart, which keep where they were scrolled to.
fn code_box(name: &'static str, index: usize, code: &Code, view: &PopupView) -> Stateful<Div> {
    let id = content_id(name, [(index, &code.text)]);
    let mut code_box = div().id(id).flex().overflow_x_scroll();
    // So a vertical wheel over code scrolls the documentation, not the code sideways.
    code_box.style().restrict_scroll_to_axis = Some(true);
    let code = div().flex_none().child(render_code(code, view));
    code_box.whitespace_nowrap().font_family(font::mono()).child(code)
}

/// The signature and the blocks; gives way first when the panel is short of room, so the
/// hints stay in view, and scrolls. A heading stays close to the block it introduces.
fn render_body(
    signature: Option<AnyElement>,
    blocks: &[DocBlock],
    gap: f32,
    code_chars: usize,
    view: &PopupView,
) -> Stateful<Div> {
    let mut sections: Vec<AnyElement> = signature.into_iter().collect();
    let mut blocks = blocks.iter().enumerate().peekable();
    while let Some((index, block)) = blocks.next() {
        let section = match block {
            DocBlock::Heading(heading) => {
                let next = blocks.next_if(|(_, next)| !matches!(next, DocBlock::Heading(_)));
                let next = next.map(|(index, next)| render_block(index, next, code_chars, view));
                let section =
                    div().column().flex_none().gap(px(2.0)).child(render_heading(heading));
                section.children(next).into_any_element()
            }
            block => render_block(index, block, code_chars, view),
        };
        sections.push(section);
    }
    scrolling_column("documentation-body".into())
        .gap(px(gap))
        .track_scroll(view.scroll)
        .children(sections)
}

/// Colored like the buffer when its mode has a grammar.
fn render_code(code: &Code, view: &PopupView) -> StyledText {
    let mut faces = (view.code_faces)(code);
    if let Some(emphasis) = &code.emphasis {
        let emphasized =
            HighlightStyle { font_weight: Some(FontWeight::BOLD), ..face_style(Face::Highlight) };
        faces = combine(&[faces, vec![(emphasis.clone(), emphasized)]]);
    }
    StyledText::new(code.text.clone()).with_highlights(faces)
}

fn render_heading(heading: &str) -> impl IntoElement {
    let (text, code) = code_spans(heading);
    div()
        .flex_none()
        .text_size(px(font::status_size()))
        .font_weight(FontWeight::SEMIBOLD)
        .child(StyledText::new(text).with_highlights(code))
}

/// Text that quotes code wraps a word at a time to hold its chips; other text is one run.
fn render_block(index: usize, block: &DocBlock, code_chars: usize, view: &PopupView) -> AnyElement {
    match block {
        DocBlock::Heading(heading) => render_heading(heading).into_any_element(),
        DocBlock::Text(text) => {
            let paragraph = if text.contains('`') {
                render_text_line(text, code_chars, &DOCUMENTATION_CHIP)
            } else {
                div().child(text.clone())
            };
            paragraph
                .flex_none()
                .text(font::ui_size(), color::fg_body())
                .line_height(px(BODY_LINE_HEIGHT))
                .into_any_element()
        }
        DocBlock::Code(code) => code_box("documentation-code", index, code, view)
            .flex_none()
            .px(px(CODE_BLOCK_PADDING_X))
            .py(px(6.0))
            .rounded_fill(6.0, glass::field())
            .status_text(color::fg_alt())
            .line_height(px(18.0))
            .into_any_element(),
    }
}

/// The text without its backticks, and the ranges they quoted as code.
fn code_spans(text: &str) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let style = HighlightStyle {
        color: Some(hsla(color::fg_alt())),
        background_color: Some(rgba(glass::ink(0x12)).into()),
        ..Default::default()
    };
    let mut plain = String::with_capacity(text.len());
    let mut code = Vec::new();
    for (index, part) in text.split('`').enumerate() {
        let start = plain.len();
        plain.push_str(part);
        if index % 2 == 1 && !part.is_empty() {
            code.push((start..plain.len(), style));
        }
    }
    (plain, code)
}

/// The keys and what they do, in a row over a rule.
pub(crate) fn render_hints(hints: &'static [(&'static str, &'static str)]) -> Div {
    let hints = hints.iter().map(|&(keys, action)| key_hint(keys, action, 6.0));
    let bar = div().flex().flex_none().gap(px(14.0)).pt(px(8.0)).top_border(glass::ink(0x0f));
    bar.status_text(color::fg_dim()).children(hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(text: &str, first: bool, last: bool) -> Piece<'_> {
        Piece::Code { text, first, last }
    }

    #[test]
    fn code_spans_lose_their_backticks() {
        let (text, code) = code_spans("see `Range` and `a`.");
        assert_eq!(text, "see Range and a.");
        let ranges: Vec<_> = code.into_iter().map(|(range, _)| range).collect();
        assert_eq!(ranges, [4..9, 14..15]);
    }

    #[test]
    fn note_lines_break_into_words_and_code() {
        assert_eq!(
            line_pieces("find `SOme` in `a b`", 8),
            [
                Piece::Word("find "),
                code("SOme", true, true),
                Piece::Word(" "),
                Piece::Word("in "),
                code("a ", true, false),
                code("b", false, true),
            ]
        );
    }

    #[test]
    fn pieces_wrap_before_the_one_that_goes_past_the_line() {
        let widths = |widths: &[f32]| widths.iter().map(|&width| px(width)).collect::<Vec<_>>();
        assert_eq!(wrapped_lines(widths(&[]), px(100.0)), 1);
        assert_eq!(wrapped_lines(widths(&[40.0, 60.0]), px(100.0)), 1);
        assert_eq!(wrapped_lines(widths(&[40.0, 61.0, 39.0]), px(100.0)), 2);
        assert_eq!(wrapped_lines(widths(&[30.0, 250.0, 50.0]), px(100.0)), 4);
    }

    #[test]
    fn code_longer_than_a_line_breaks_into_pieces_that_fit() {
        let text = |piece| match piece {
            Piece::Code { text, .. } | Piece::Word(text) => text,
        };
        let pieces = line_pieces("`ab abcdefg`", 3);
        let texts: Vec<_> = pieces.into_iter().map(text).collect();
        assert_eq!(texts, ["ab ", "abc", "def", "g"]);
    }
}
