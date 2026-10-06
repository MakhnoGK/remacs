# 0044 — Renaming symbols and moving files across the workspace

Status: accepted, 2026-10-06 (issue #6)

## Decision

- **Every file a rename or move changes is saved**, unsaved edits it had included (user's
  choice), so the files on disk always agree with each other and the project builds.
  Core `Editor::change_files` makes `FileChange`s in order — edits, created files, renamed
  files that buffers follow — and saves every file edited, also what was edited before a
  failure. wdired's reference updates use it too, so they now save modified buffers.
- **`rename-symbol` (`C-c C-r`)** is a plugin of its own, like `eglot-rename`: it asks
  backends for the name at point, reads the new one with that as the input (refusing an
  empty or unchanged name), asks for the changes and makes them at once, with no preview
  (user's choice). The name is read only while point stays where the command left it.
  `Renamed a to b: 5 places in 2 files; moved a.rs to b.rs`.
- **lsp answers with the first server of the buffer**: `textDocument/prepareRename` for
  the name (the range's text, the placeholder, or the word at point for the default
  behaviour and for servers without `prepareProvider`), then `textDocument/rename`.
  Requests wait while the server loads the project: rust-analyzer then finds nothing or
  cancels with "content modified". Dropped if the buffer changed meanwhile.
- **Workspace edits may create and rename files** (`resourceOperations: create, rename`,
  `failureHandling: abort`), turned into `FileChange`s in their order; edits to a file
  renamed or created earlier refer to the text it will have. Deleting is not supported.
  Code actions still refuse file operations.
- **A rename that moves files asks `workspace/willRenameFiles` first**, as VS Code does:
  rust-analyzer leaves the edits of a module's references out of the rename when the
  client supports `willRenameFiles`, so renaming `mod util` would otherwise move the file
  and leave `mod util;` behind. Those edits are made before the changes.
- **Dired `R` updates references before moving**, like wdired (ADR 0014): it asks
  backends about the moves (files that would be skipped as already there are left out),
  says "Finding references to update…", then makes the edits, saves them and moves. One
  move waits at a time. Without a backend taking the question, it moves after the next
  command pass.
- **Servers hear of every move** (`workspace/didRenameFiles`): core keeps the files
  `Editor::file_moved` was told of until `take_moved_files`, so dired moves and the files
  a symbol's rename moves are both reported.

## Later

- rust-analyzer does not announce `experimental/serverStatus`, so a rename asked in the
  first moment after it starts, before it says it is loading, finds nothing.
