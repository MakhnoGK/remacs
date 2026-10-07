use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Editor, Settings, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;

use super::*;
use crate::catalog::version_key;

const WAIT: Duration = Duration::from_secs(10);

/// An empty directory of its own for `test`.
fn directory(test: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("remacs-tools-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory.canonicalize().unwrap()
}

/// A fake GitHub API and releases of codelldb in `dir`.
struct Releases {
    dir: PathBuf,
}

impl Releases {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.join("api"),
        }
    }

    fn api(&self) -> String {
        format!("file://{}", self.dir.display())
    }

    /// Publishes `version` of codelldb, as the latest release if `latest`; `digest`
    /// replaces the asset's real checksum.
    fn publish(&self, version: &str, latest: bool, digest: Option<&str>) {
        let tag = format!("v{version}");
        let build = self.dir.join("build").join(version);
        let program = build.join(CODELLDB.program);
        fs::create_dir_all(program.parent().unwrap()).unwrap();
        fs::write(&program, format!("#!/bin/sh\necho codelldb {version}\n")).unwrap();
        let asset = self.dir.join("assets").join(&tag);
        fs::create_dir_all(&asset).unwrap();
        let asset = asset.join(CODELLDB.asset_name(&tag));
        let zipped = Process::new("zip")
            .arg("-qr")
            .arg(&asset)
            .arg("extension")
            .current_dir(&build)
            .status()
            .unwrap();
        assert!(zipped.success());
        let digest = digest.map_or_else(|| sha256(&asset), str::to_owned);
        let release = serde_json::json!({
            "tag_name": tag,
            "assets": [{
                "name": CODELLDB.asset_name(&tag),
                "size": fs::metadata(&asset).unwrap().len(),
                "digest": format!("sha256:{digest}"),
                "browser_download_url": format!("file://{}", asset.display()),
            }],
        })
        .to_string();
        let releases = self
            .dir
            .join("repos")
            .join(CODELLDB.repository)
            .join("releases");
        fs::create_dir_all(releases.join("tags")).unwrap();
        fs::write(releases.join("tags").join(&tag), &release).unwrap();
        if latest {
            fs::write(releases.join("latest"), &release).unwrap();
        }
    }
}

fn sha256(file: &Path) -> String {
    let output = Process::new("shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    output.split_whitespace().next().unwrap().to_owned()
}

fn editor(home: &Path, releases: &Releases, tools_toml: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &ToolsPlugin]);
    editor.set_settings(Settings::from_files([(
        PathBuf::from("tools.toml"),
        tools_toml.to_owned(),
    )]));
    state::tools(&mut editor).api = releases.api();
    keep_tools_in(&mut editor, home);
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

/// Refreshes as the UI would until `done` holds.
fn wait_until(editor: &mut Editor, done: impl Fn(&Editor) -> bool) {
    let start = Instant::now();
    while !done(editor) {
        assert!(start.elapsed() < WAIT, "gave up waiting");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

type Got = Rc<RefCell<Option<Result<PathBuf, String>>>>;

fn ensure_codelldb(editor: &mut Editor) -> Got {
    let got: Got = Rc::default();
    let shared = Rc::clone(&got);
    ensure(editor, "codelldb", move |_, result| {
        *shared.borrow_mut() = Some(result);
    });
    got
}

fn wait_for(editor: &mut Editor, got: &Got) -> Result<PathBuf, String> {
    wait_until(editor, |_| got.borrow().is_some());
    got.borrow_mut().take().unwrap()
}

/// The Tools panel's row of codelldb: what is installed and how it goes.
fn codelldb_row(editor: &Editor) -> (String, String) {
    let minibuffer = editor.minibuffer().expect("the Tools panel is open");
    let row = minibuffer
        .candidates()
        .iter()
        .find(|candidate| candidate.value == "codelldb")
        .unwrap();
    (
        row.annotation.clone(),
        row.detail.clone().unwrap_or_default().into_owned(),
    )
}

#[test]
fn a_tool_is_downloaded_checked_and_unpacked_the_first_time_it_is_needed() {
    let home = directory("first-use");
    let releases = Releases::new(&home);
    releases.publish("1.12.3", true, None);
    let mut editor = editor(&home, &releases, "");

    let got = ensure_codelldb(&mut editor);
    let program = wait_for(&mut editor, &got).unwrap();

    let installed = home.join("cache/tools/codelldb/1.12.3");
    assert_eq!(program, installed.join(CODELLDB.program));
    let output = Process::new(&program).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "codelldb 1.12.3\n");
    let left: Vec<_> = fs::read_dir(home.join("cache/tools/codelldb"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        left,
        ["1.12.3"],
        "no download or half-unpacked copy is left"
    );

    // Installed, it is there at once, with no download.
    fs::remove_dir_all(releases.dir.join("repos")).unwrap();
    let got = ensure_codelldb(&mut editor);
    assert_eq!(got.borrow_mut().take(), Some(Ok(program.clone())));
    let mut restarted = self::editor(&home, &releases, "");
    let got = ensure_codelldb(&mut restarted);
    assert_eq!(got.borrow_mut().take(), Some(Ok(program)));
}

#[test]
fn a_download_that_does_not_match_the_release_checksum_is_thrown_away() {
    let home = directory("checksum");
    let releases = Releases::new(&home);
    releases.publish("1.12.3", true, Some(&"0".repeat(64)));
    let mut editor = editor(&home, &releases, "");

    let got = ensure_codelldb(&mut editor);
    let error = wait_for(&mut editor, &got).unwrap_err();

    assert!(error.contains("checksum mismatch"), "{error}");
    assert!(!home.join("cache/tools/codelldb/1.12.3").exists());
    press(&mut editor, "C-c t");
    let (_, detail) = codelldb_row(&editor);
    assert!(detail.starts_with("Failed: checksum mismatch"), "{detail}");

    // Retried once the release is right.
    releases.publish("1.12.3", true, None);
    press(&mut editor, "r");
    wait_until(&mut editor, |editor| codelldb_row(editor).1 == "Installed");
}

#[test]
fn tools_toml_pins_a_version_or_names_a_program_of_ones_own() {
    let home = directory("settings");
    let releases = Releases::new(&home);
    releases.publish("1.10.0", false, None);
    releases.publish("1.12.3", true, None);

    let mut editor = editor(&home, &releases, "[tools.codelldb]\nversion = \"1.10.0\"\n");
    let got = ensure_codelldb(&mut editor);
    let program = wait_for(&mut editor, &got).unwrap();
    assert!(program.starts_with(home.join("cache/tools/codelldb/1.10.0")));

    let own = "[tools.codelldb]\npath = \"/opt/llvm/codelldb\"\n";
    let mut editor = self::editor(&home, &releases, own);
    let got = ensure_codelldb(&mut editor);
    assert_eq!(
        got.borrow_mut().take(),
        Some(Ok(PathBuf::from("/opt/llvm/codelldb")))
    );
    press(&mut editor, "C-c t");
    assert_eq!(
        codelldb_row(&editor),
        (
            "/opt/llvm/codelldb".to_owned(),
            "Set in tools.toml".to_owned()
        )
    );

    let unknown = "[tools.gdb]\npath = \"gdb\"\n".to_owned();
    let error = Settings::from_files([(PathBuf::from("tools.toml"), unknown)])
        .section::<ToolsSettings>()
        .map(drop)
        .map_err(|(error, _)| error.message);
    assert_eq!(error, Err("no tool is called gdb".to_owned()));
}

#[test]
fn the_tools_panel_finds_a_newer_release_and_updates_to_it() {
    let home = directory("update");
    let releases = Releases::new(&home);
    releases.publish("1.10.0", true, None);
    let mut editor = editor(&home, &releases, "");
    let got = ensure_codelldb(&mut editor);
    wait_for(&mut editor, &got).unwrap();

    releases.publish("1.12.3", true, None);
    press(&mut editor, "C-c t");
    wait_until(&mut editor, |editor| {
        codelldb_row(editor).1 == "Update available · 1.12.3"
    });
    assert_eq!(codelldb_row(&editor).0, "1.10.0 · GitHub releases");

    press(&mut editor, "u");
    wait_until(&mut editor, |editor| codelldb_row(editor).1 == "Installed");
    assert_eq!(codelldb_row(&editor).0, "1.12.3 · GitHub releases");
    assert!(!home.join("cache/tools/codelldb/1.10.0").exists());
    let got = ensure_codelldb(&mut editor);
    let program = got.borrow_mut().take().unwrap().unwrap();
    assert!(program.starts_with(home.join("cache/tools/codelldb/1.12.3")));
}

#[test]
fn versions_are_ordered_by_their_numbers() {
    assert!(version_key("1.9.0") < version_key("1.11.4"));
    assert!(version_key("2026-09-29") < version_key("2026-10-06"));
    assert_eq!(CODELLDB.version_of("v1.12.3"), "1.12.3");
    assert!(
        JS_DEBUG
            .asset_name("v1.140.0")
            .ends_with("js-debug-dap-v1.140.0.tar.gz")
    );
}
