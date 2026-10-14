use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Buffer, Editor, Settings};
use remacs_minibuffer::MinibufferPlugin;
use remacs_text::Text;
use remacs_tools::{ToolsPlugin, keep_tools_in};

use crate::{LspPlugin, keep_offers_in};

use super::*;

/// Servers whose programs are nowhere: one remacs downloads, one it cannot, and two
/// alternatives for Markdown.
static PLUGIN: LspPlugin = LspPlugin::new(
    "[[servers]]\n\
     name = \"downloadable\"\n\
     command = \"remacs-test-no-such-server\"\n\
     languages = { Fundamental = \"plaintext\" }\n\
     install = { from = \"github\", package = \"rust-lang/rust-analyzer\", \
                 asset = \"rust-analyzer-{target}.gz\", program = \"rust-analyzer\" }\n\
     [[servers]]\n\
     name = \"nowhere\"\n\
     command = \"remacs-test-no-such-server\"\n\
     languages = { Fundamental = \"plaintext\" }\n\
     [[servers]]\n\
     name = \"first\"\n\
     command = \"remacs-test-no-such-server\"\n\
     languages = { Markdown = \"markdown\" }\n\
     group = \"markdown\"\n\
     install = { from = \"github\", package = \"o/first\", asset = \"first\" }\n\
     [[servers]]\n\
     name = \"second\"\n\
     command = \"remacs-test-no-such-server\"\n\
     languages = { Markdown = \"markdown\" }\n\
     group = \"markdown\"\n\
     install = { from = \"github\", package = \"o/second\", asset = \"second\" }\n",
);

/// An empty directory of its own for `test`.
fn directory(test: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("remacs-lsp-offer-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory.canonicalize().unwrap()
}

/// A Git work tree in `home`.
fn project(home: &Path, name: &str) -> PathBuf {
    let project = home.join(name);
    fs::create_dir_all(project.join(".git")).unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    project
}

fn editor(home: &Path, lsp_toml: &str) -> Editor {
    let mut editor = Editor::new(&[&MinibufferPlugin, &ToolsPlugin, &PLUGIN]);
    editor.set_settings(Settings::from_files([(
        PathBuf::from("lsp.toml"),
        lsp_toml.to_owned(),
    )]));
    keep_tools_in(&mut editor, home);
    keep_offers_in(&mut editor, home);
    editor
}

fn open(editor: &mut Editor, path: PathBuf, mode: &'static str) {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    editor.add_buffer(Buffer::new(name, Text::default(), mode).with_file(path));
    editor.refresh();
}

/// The open dialog's title and what it lists.
fn offered(editor: &Editor) -> Option<(String, Vec<String>)> {
    let dialog = editor.minibuffer()?.dialog()?;
    let items = dialog.items.into_iter().map(|item| item.label).collect();
    Some((dialog.title, items))
}

#[test]
fn a_project_is_offered_the_servers_remacs_can_download_once() {
    let home = directory("once");
    let remacs = project(&home, "remacs");
    let mut editor = editor(&home, "");

    open(&mut editor, remacs.join("src/main.rs"), "Fundamental");
    assert_eq!(
        offered(&editor),
        Some((
            "Download downloadable?".to_owned(),
            vec!["downloadable".to_owned()]
        )),
        "the server remacs cannot download is passed over in silence"
    );
    editor.answer_question(false);
    assert_eq!(
        editor.echo().as_deref(),
        Some("Edit ▸ Download Language Servers offers them again")
    );
    assert!(can_download(&editor), "the menu offers it again");

    open(&mut editor, remacs.join("src/lib.rs"), "Fundamental");
    assert_eq!(offered(&editor), None);
    let mut restarted = self::editor(&home, "");
    open(&mut restarted, remacs.join("src/main.rs"), "Fundamental");
    assert_eq!(offered(&restarted), None, "not even after a restart");
    restarted.run_command(DOWNLOAD);
    assert!(offered(&restarted).is_some(), "but from the menu");
    restarted.answer_question(false);

    let other = project(&home, "other");
    open(&mut restarted, other.join("src/main.rs"), "Fundamental");
    assert!(offered(&restarted).is_some(), "another project is asked");
}

#[test]
fn a_project_lacking_all_of_a_group_chooses_one() {
    let home = directory("group");
    let notes = project(&home, "notes");
    let mut editor = editor(&home, "");

    open(&mut editor, notes.join("README.md"), "Markdown");
    let minibuffer = editor.minibuffer().expect("a choice is offered");
    assert_eq!(minibuffer.dialog().map(|dialog| dialog.title), None);
    let choices: Vec<_> = minibuffer
        .candidates()
        .iter()
        .map(|candidate| candidate.value.clone())
        .collect();
    assert_eq!(choices, ["first", "second"]);
    editor.cancel_minibuffer();

    open(&mut editor, notes.join("CHANGES.md"), "Markdown");
    assert!(
        editor.minibuffer().is_none(),
        "the group is asked about once"
    );
}

#[test]
#[ignore = "downloads rust-analyzer from GitHub"]
fn a_server_downloaded_when_offered_starts_for_the_file() {
    let home = directory("download");
    let remacs = project(&home, "remacs");
    let mut editor = editor(&home, "");

    open(&mut editor, remacs.join("src/main.rs"), "Fundamental");
    editor.answer_question(true);
    let started = Instant::now();
    while document::window_servers(&editor) != ["downloadable"] {
        assert!(
            started.elapsed() < Duration::from_secs(300),
            "{:?}",
            editor.echo()
        );
        thread::sleep(Duration::from_millis(100));
        editor.refresh();
    }
    assert_eq!(editor.echo().as_deref(), Some("Downloaded downloadable"));
}

#[test]
fn lsp_toml_can_turn_offers_off() {
    let home = directory("off");
    let remacs = project(&home, "remacs");
    let mut editor = editor(&home, "offer-downloads = false\n");

    open(&mut editor, remacs.join("src/main.rs"), "Fundamental");
    assert_eq!(offered(&editor), None);
    assert_eq!(editor.echo(), None);
}
