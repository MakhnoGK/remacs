//! Restarting a language server by hand, e.g. after its program was updated or it exited.

use remacs_core::{Candidate, CommandResult, Count, Editor, Prompt, filter_candidates};

use crate::document::{self, Clients};

/// Stops a server everywhere and starts it again where it is wanted; the servers of the
/// window buffer are offered first.
pub(crate) fn restart_server(editor: &mut Editor) -> CommandResult {
    let candidates = candidates(editor);
    if candidates.is_empty() {
        return Err("No language server has started".into());
    }
    let prompt = Prompt {
        label: "Restart language server".into(),
        placeholder: "Language server…",
        require_match: true,
        count: Count::Total,
        hints: &[("↵", "Restart"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    let complete =
        move |_: &Editor, input: &str| filter_candidates(input, candidates.clone(), false);
    editor.read(prompt, complete, |editor, answer| {
        let name = editor
            .plugin_state::<Clients>()
            .started()
            .into_keys()
            .find(|name| *name == answer)
            .ok_or("No such language server")?;
        document::restart(editor, name);
        editor.set_done(format!("Restarted {name}"));
        Ok(())
    })
}

/// The started servers, the window buffer's first; an exited one says so, a running one
/// where it runs.
fn candidates(editor: &mut Editor) -> Vec<Candidate> {
    let attached = document::window_servers(editor);
    let mut candidates: Vec<Candidate> = editor
        .plugin_state::<Clients>()
        .started()
        .into_iter()
        .map(|(name, roots)| Candidate {
            annotation: roots
                .iter()
                .filter_map(|root| root.file_name().map(|name| name.to_string_lossy()))
                .collect::<Vec<_>>()
                .join(", "),
            tag: if roots.is_empty() {
                Some("exited")
            } else if attached.contains(&name) {
                Some("this buffer")
            } else {
                None
            },
            ..Candidate::new(name)
        })
        .collect();
    candidates.sort_by_key(|candidate| !attached.contains(&candidate.value.as_str()));
    candidates
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;
    use std::{env, fs};

    use remacs_core::Buffer;
    use remacs_text::Text;

    use crate::{LanguageServer, LspPlugin};

    use super::*;

    static FAILING: [LanguageServer; 1] = [LanguageServer {
        name: "failing",
        command: "sh",
        args: &["-c", "echo 'cannot serve' >&2; exit 1"],
        languages: &[("Fundamental", "plaintext")],
        root_markers: &[],
        initialization_options: None,
        configuration: None,
        pull_diagnostics: false,
        reports_loading: false,
        node_modules: false,
        required_files: &[],
    }];
    static PLUGIN: LspPlugin = LspPlugin::new(&FAILING);

    /// Refreshes once the server has exited, as the app does when it is notified.
    fn wait_for_exit(editor: &mut Editor, notified: &Receiver<()>) {
        notified.recv_timeout(Duration::from_secs(5)).unwrap();
        editor.refresh();
    }

    #[test]
    fn a_server_that_exited_is_reported_and_restarted_by_hand() {
        let (sender, notified) = mpsc::channel();
        let mut editor = Editor::new(&[&PLUGIN]);
        editor.set_notifier(Arc::new(move || {
            let _ = sender.send(());
        }));
        let project = env::temp_dir().join("remacs-lsp-restart");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("notes.txt");
        editor.add_buffer(Buffer::new("notes.txt", Text::default(), "Fundamental").with_file(path));
        editor.refresh();
        wait_for_exit(&mut editor, &notified);
        assert_eq!(
            editor.echo().as_deref(),
            Some("failing exited: cannot serve")
        );

        editor.run_command("lsp-restart");
        let candidates = editor.minibuffer().unwrap().candidates();
        assert_eq!(candidates[0].value, "failing");
        assert_eq!(candidates[0].tag, Some("exited"));
        editor.exit_minibuffer(false).unwrap();
        assert_eq!(editor.echo().as_deref(), Some("Restarted failing"));
        editor.refresh();
        wait_for_exit(&mut editor, &notified);
        assert_eq!(
            editor.echo().as_deref(),
            Some("failing exited: cannot serve")
        );
    }
}
