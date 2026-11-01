use super::*;

fn labels(editor: &Editor) -> Vec<String> {
    editor
        .minibuffer()
        .map(|minibuffer| minibuffer.candidates().iter().map(|c| c.label.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn the_chooser_opened_again_lists_what_was_found_at_once() {
    let home = directory("chooser-again");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(root.join(".git")).unwrap();
    write(&root.join("package.json"), "{\n  \"scripts\": {\n    \"dev\": \"vite\"\n  }\n}\n");
    editor.visit_file(&root.join("package.json")).unwrap();
    editor.refresh();
    let found = ["dev".to_owned(), "New configuration…".to_owned()];
    let search = |editor: &mut Editor| {
        let search = editor.plugin_state::<chooser::Chooser>().searches[&root].clone();
        let search = search.borrow();
        (search.running.is_some(), search.generation)
    };
    chooser::choose(&mut editor).unwrap();
    wait_until(&mut editor, |editor| labels(editor) == found);
    // Until the index is complete and searched.
    while search(&mut editor).0 {
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
    let searched = search(&mut editor);
    press(&mut editor, "C-g");

    // The project's search is kept: nothing to wait for, and no search started again.
    chooser::choose(&mut editor).unwrap();
    assert_eq!(labels(&editor), found);
    editor.refresh();
    assert_eq!(search(&mut editor), searched);
    fs::remove_dir_all(home).unwrap();
}
