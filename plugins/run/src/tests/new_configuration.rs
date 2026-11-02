use super::*;
use crate::commands::new_configuration;

fn point_line_text(editor: &Editor) -> Option<String> {
    contents(editor).lines().nth(editor.window_buffer().point_line()).map(str::to_owned)
}

#[test]
fn a_new_configuration_is_made_from_the_entry_point_at_point_else_the_compile_command() {
    let home = directory("new");
    let mut editor = editor(&home);
    let root = home.join("project");
    fs::create_dir_all(root.join(".git")).unwrap();
    write(&root.join("Cargo.toml"), "");
    write(&root.join(".env"), "PORT=3000\n");
    write(&root.join("README"), "");
    let script = root.join("hello.mjs");
    write(&script, "console.log('hello')\n");
    let node = Configuration::new("hello.mjs", Kind::Node { file: "hello.mjs".into() });
    configuration_store(&mut editor).find_or_add(&root, node.clone()).unwrap();
    editor.visit_file(&script).unwrap();
    wait_until(&mut editor, |editor| {
        editor.gutter_marks(0..10).is_some_and(|marks| !marks.is_empty())
    });

    // The project runs the script already; a new one is another to change.
    new_configuration(&mut editor, &root, None).unwrap();
    assert_eq!(point_line_text(&editor).as_deref(), Some("name = \"hello.mjs (2)\""));
    let comments = contents(&editor).lines().filter(|line| line.starts_with("# args = ")).count();
    assert_eq!(comments, 2, "each saved configuration lists what it leaves out");

    // Where nothing runs, it is made from the project's compile command.
    editor.visit_file(&root.join("README")).unwrap();
    new_configuration(&mut editor, &root, None).unwrap();
    assert_eq!(point_line_text(&editor).as_deref(), Some("name = \"cargo build\""));

    let with_env_file = |mut configuration: Configuration| {
        configuration.env_file = Some(".env".into());
        configuration
    };
    let copy = Configuration { name: "hello.mjs (2)".into(), ..node.clone() };
    let build = Configuration::new("cargo build", Kind::Shell { command: "cargo build".into() });
    assert_eq!(
        configuration_store(&mut editor).project(&root).0.configurations,
        [node, with_env_file(copy), with_env_file(build)]
    );
    fs::remove_dir_all(home).unwrap();
}
