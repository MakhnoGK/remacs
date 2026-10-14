//! Installs every built-in language server this Mac can install, from its real source:
//! `cargo test -p remacs-languages --test downloads -- --ignored --nocapture`.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::Editor;
use remacs_languages::SERVERS;
use remacs_tools::{ToolKind, ToolsPlugin, can_install, ensure, keep_tools_in, provide};

const TIMEOUT: Duration = Duration::from_secs(900);

#[test]
#[ignore = "downloads every language server"]
fn every_installable_language_server_installs() {
    let home = std::env::temp_dir().join(format!("remacs-downloads-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let mut editor = Editor::new(&[&ToolsPlugin]);
    keep_tools_in(&mut editor, &home);
    let servers: &'static [_] = remacs_lsp::catalog_servers(SERVERS).unwrap().leak();
    let tools: Vec<_> = servers.iter().filter_map(|server| server.tool).collect();
    provide(&mut editor, ToolKind::LanguageServer, &tools);
    let mut failed = Vec::new();
    for tool in tools {
        if !can_install(&editor, tool) {
            eprintln!("{}: skipped, not installable here", tool.name);
            continue;
        }
        let got: Rc<RefCell<Option<Result<PathBuf, String>>>> = Rc::default();
        let shared = Rc::clone(&got);
        ensure(&mut editor, tool.name, move |_, result| {
            *shared.borrow_mut() = Some(result);
        });
        let start = Instant::now();
        while got.borrow().is_none() {
            assert!(start.elapsed() < TIMEOUT, "{} took too long", tool.name);
            thread::sleep(Duration::from_millis(100));
            editor.refresh();
        }
        match got.borrow_mut().take().unwrap() {
            Ok(program) if program.is_file() => eprintln!("{}: {}", tool.name, program.display()),
            Ok(program) => failed.push(format!("{}: no {}", tool.name, program.display())),
            Err(error) => failed.push(format!("{}: {error}", tool.name)),
        }
    }
    let _ = fs::remove_dir_all(&home);
    assert!(failed.is_empty(), "{failed:#?}");
}
