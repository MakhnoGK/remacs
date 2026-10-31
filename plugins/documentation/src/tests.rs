use remacs_core::{
    Buffer, Code, DocBlock, Documentation, Editor, Face, Popup, Span, parse_sequence,
};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::{
    Answer, DocumentationPlugin, Request, SignatureTrigger, Signatures, deliver,
    deliver_signatures, take_requests, take_signature_requests,
};

const SOURCE: &str = "self.text.copy_from_slice(s);";

fn editor() -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &DocumentationPlugin]);
    editor.add_buffer(Buffer::new("buffer.rs", Text::from(SOURCE), "Rust"));
    editor.window_buffer_mut().set_point(12);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn answer() -> Answer {
    Answer {
        range: Some(10..25),
        documentation: Documentation {
            container: "core::slice".into(),
            kind: "method".into(),
            signature: Some(Code::new(
                "pub fn copy_from_slice(&mut self, src: &[T])",
                Some("Rust"),
            )),
            blocks: vec![
                DocBlock::Text("Copies all elements from `src` into `self`.".into()),
                DocBlock::Heading("Panics".into()),
                DocBlock::Text("If the two slices have different lengths.".into()),
            ],
        },
    }
}

fn popup(editor: &Editor) -> Option<Popup> {
    editor.popups().pop()
}

#[test]
fn shows_the_summary_then_everything_and_highlights_the_symbol() {
    let mut editor = editor();
    press(&mut editor, "C-h .");
    assert_eq!(take_requests(&mut editor), [Request { id: 1, offset: 12 }]);
    deliver(&mut editor, 1, Some(answer()));
    let shown = popup(&editor).unwrap();
    assert_eq!((shown.anchor, shown.rows.len()), (10, 0));
    let documentation = shown.documentation.unwrap();
    assert_eq!(documentation.blocks.len(), 1);
    assert_eq!(shown.hints.len(), 2);
    assert_eq!(
        editor.decorations(0..SOURCE.len()),
        [vec![Span { range: 10..25, face: Face::Highlight }]]
    );

    press(&mut editor, "C-h .");
    assert_eq!(take_requests(&mut editor), []);
    let shown = popup(&editor).unwrap();
    assert_eq!(shown.documentation.unwrap().blocks.len(), 3);
    assert_eq!(shown.hints.len(), 1);

    press(&mut editor, "C-f");
    assert!(popup(&editor).is_none());
    assert_eq!(editor.decorations(0..SOURCE.len()), [vec![]]);
}

#[test]
fn scrolling_keeps_it_shown_and_shows_all_of_it() {
    let mut editor = editor();
    press(&mut editor, "C-h .");
    deliver(&mut editor, 1, Some(answer()));
    press(&mut editor, "C-M-v C-M-v C-M-V");
    let shown = popup(&editor).unwrap();
    assert_eq!(shown.documentation_pages, 1);
    assert_eq!(shown.documentation.unwrap().blocks.len(), 3);
}

#[test]
fn escape_and_mouse_motion_hide_it() {
    let mut editor = editor();
    press(&mut editor, "C-h .");
    deliver(&mut editor, 1, Some(answer()));
    editor.set_point_at(0, 0);
    assert!(popup(&editor).is_none());
    editor.set_point_at(0, 12);
    assert!(popup(&editor).is_some());
    press(&mut editor, "ESC");
    assert!(popup(&editor).is_none());
}

#[test]
fn late_and_empty_answers() {
    let mut editor = editor();
    press(&mut editor, "C-h .");
    press(&mut editor, "C-b");
    deliver(&mut editor, 1, Some(answer()));
    assert!(popup(&editor).is_none(), "a command ran since the request");

    press(&mut editor, "C-h .");
    deliver(&mut editor, 2, None);
    assert_eq!(editor.echo().as_deref(), Some("No documentation at point"));
}

fn signature(label: &str, parameter: &str) -> Code {
    let start = label.find(parameter).unwrap();
    Code { emphasis: Some(start..start + parameter.len()), ..Code::new(label, Some("Rust")) }
}

fn signatures(parameter: &str) -> Signatures {
    let label = "fn copy_from_slice(&mut self, src: &[T])";
    Signatures { list: vec![signature(label, parameter)], active: 0 }
}

fn signature_asked(editor: &mut Editor) -> Vec<SignatureTrigger> {
    let requests = take_signature_requests(editor);
    requests.iter().map(|request| request.trigger).collect()
}

#[test]
fn signatures_follow_the_call_being_typed() {
    let mut editor = editor();
    editor.window_buffer_mut().set_point(25);
    press(&mut editor, "C-f");
    assert_eq!(signature_asked(&mut editor), []);
    press(&mut editor, "x");
    assert_eq!(signature_asked(&mut editor), [SignatureTrigger::Typed('x')]);
    deliver_signatures(&mut editor, 1, None);
    assert!(popup(&editor).is_none());
    assert_eq!(editor.echo(), None, "typing says nothing");

    press(&mut editor, "(");
    assert_eq!(signature_asked(&mut editor), [SignatureTrigger::Typed('(')]);
    deliver_signatures(&mut editor, 2, Some(signatures("&mut self")));
    let shown = popup(&editor).unwrap();
    assert!(shown.above);
    assert_eq!(shown.anchor, 28);
    assert_eq!(shown.hints.len(), 0);
    let label = shown.documentation.unwrap().signature.unwrap();
    assert_eq!(&label.text[label.emphasis.unwrap()], "&mut self");

    press(&mut editor, ",");
    let asked = take_signature_requests(&mut editor);
    assert_eq!(asked[0].trigger, SignatureTrigger::Update(Some(',')));
    deliver_signatures(&mut editor, asked[0].id, Some(signatures("src: &[T]")));
    let shown = popup(&editor).unwrap();
    assert_eq!(shown.anchor, 28, "the same call's signature stays put");
    let label = shown.documentation.unwrap().signature.unwrap();
    assert_eq!(&label.text[label.emphasis.unwrap()], "src: &[T]");

    let point = editor.window_buffer().point();
    editor.set_point_at(0, 0);
    assert!(popup(&editor).is_none(), "a click moved point away");
    editor.window_buffer_mut().set_point(point);
    assert!(popup(&editor).is_some());

    press(&mut editor, "C-b");
    assert_eq!(signature_asked(&mut editor), [SignatureTrigger::Update(None)]);
    deliver_signatures(&mut editor, 4, None);
    assert!(popup(&editor).is_none(), "point left the call");
}

#[test]
fn overloads_are_stepped_through_and_escape_hides_them() {
    let mut editor = editor();
    press(&mut editor, "s-p");
    assert_eq!(signature_asked(&mut editor), [SignatureTrigger::Explicit]);
    let overloads = Signatures {
        list: vec![
            Code::new("fn new() -> Self", Some("Rust")),
            Code::new("fn new(capacity: usize) -> Self", Some("Rust")),
        ],
        active: 1,
    };
    deliver_signatures(&mut editor, 1, Some(overloads.clone()));
    let kind = |editor: &Editor| popup(editor).unwrap().documentation.unwrap().kind;
    assert_eq!(kind(&editor), "2 of 2");
    press(&mut editor, "s-p");
    assert_eq!(kind(&editor), "1 of 2");
    assert_eq!(signature_asked(&mut editor), []);

    press(&mut editor, "C-f");
    deliver_signatures(&mut editor, 2, Some(overloads));
    assert_eq!(kind(&editor), "1 of 2", "the overload chosen stays");
    press(&mut editor, "ESC");
    assert!(popup(&editor).is_none());

    press(&mut editor, "s-p");
    deliver_signatures(&mut editor, 3, None);
    assert_eq!(editor.echo().as_deref(), Some("No signature at point"));
}
