use remacs_core::{Buffer, DocBlock, Documentation, Editor, Face, Popup, Span, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::{Answer, DocumentationPlugin, Request, deliver, take_requests};

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
            signature: Some("pub fn copy_from_slice(&mut self, src: &[T])".into()),
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
        [vec![Span {
            range: 10..25,
            face: Face::Highlight
        }]]
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
