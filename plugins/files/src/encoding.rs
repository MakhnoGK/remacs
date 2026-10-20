use remacs_core::{Candidate, CommandResult, Completions, Editor, Prompt, filter_candidates};
use remacs_text::{Encoding, Text, encodings};

/// Reads the window buffer's file again in an encoding chosen by name, like Emacs
/// `revert-buffer-with-coding-system`: for when the one guessed on opening it is wrong.
/// Saving then writes that encoding.
pub fn revert_buffer_with_coding_system(editor: &mut Editor) -> CommandResult {
    let buffer = editor.window_buffer();
    let path = buffer
        .path()
        .ok_or("The buffer visits no file")?
        .to_path_buf();
    if buffer.is_modified() {
        return Err("The buffer has unsaved changes; save or undo them first".into());
    }
    let prompt = Prompt {
        label: "Read the file as".into(),
        placeholder: "Encoding…",
        default: Some(buffer.text().format().encoding.name().to_owned()),
        hints: &[("↵", "Read again"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, encoding_candidates, move |editor, name| {
        let encoding = Encoding::for_label(name.trim().as_bytes())
            .ok_or_else(|| format!("No encoding is called {name}"))?;
        let text = Text::open_with_encoding(&path, encoding)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let buffer = editor
            .find_file_noselect(&path)
            .map_err(|error| error.to_string())?;
        buffer.replace_text(text);
        buffer.set_read_only(false);
        editor.set_message(format!("Read as {}", encoding.name()));
        Ok(())
    })
}

pub fn visits_file(editor: &Editor) -> bool {
    editor.window_buffer().path().is_some()
}

fn encoding_candidates(_: &Editor, input: &str) -> Completions {
    let candidates = encodings()
        .into_iter()
        .map(|encoding| Candidate::new(encoding.name()))
        .collect();
    filter_candidates(input, candidates, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_read_in_the_wrong_encoding_is_read_again_in_the_one_chosen() {
        let path = std::env::temp_dir().join(format!("remacs-reread-{}.txt", std::process::id()));
        // "Привет" in windows-1251, too short to be guessed right.
        std::fs::write(&path, b"\xcf\xf0\xe8\xe2\xe5\xf2\n").unwrap();
        let mut editor = Editor::new(&[]);
        editor.visit_file(&path).unwrap();
        revert_buffer_with_coding_system(&mut editor).unwrap();
        editor.minibuffer_mut().unwrap().set_input("windows-1251");
        editor.exit_minibuffer(true).unwrap();
        let buffer = editor.window_buffer();
        assert_eq!(buffer.contents(0..buffer.text().len()), "Привет\n");

        editor.window_buffer_mut().insert("Ж");
        editor.window_buffer_mut().save().unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"\xc6\xcf\xf0\xe8\xe2\xe5\xf2\n"
        );

        // "Пока", read again in the encoding chosen.
        std::fs::write(&path, b"\xcf\xee\xea\xe0\n").unwrap();
        crate::revert::revert_buffer(&mut editor).unwrap();
        let buffer = editor.window_buffer();
        assert_eq!(buffer.contents(0..buffer.text().len()), "Пока\n");
        std::fs::remove_file(&path).unwrap();
    }
}
