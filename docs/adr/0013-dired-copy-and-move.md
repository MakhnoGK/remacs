# 0013 — Dired: copy and move

Status: accepted, 2026-10-04 (dired, step 4)

## Decision

- **One prompt for both**, `C` opening it on Copy and `R` on Move, like Emacs
  `dired-do-copy` / `dired-do-rename`; the input is the destination, starting at the
  listed directory. It acts on the marked entries, or the one at point.
- **Prompts gain options**: `Prompt::options` are `PromptOption`s whose state the reading
  code shares (`Rc<Cell<bool>>`); two labels draw a switch above the input (Copy / Move,
  `M-m`), one a chip under the lists (Overwrite existing, `M-o`). `M-` and the key, or a
  click, switch them and the candidates are listed again. Prompts also list items for
  reference under the candidates (`Prompt::items`, "Files"), and `Completions` may carry a
  `note` in place of the count ("New folder") and their own `hints` ("↵ Move").
- **Where the files go**: into the destination when it is a directory, ends in `/` or
  several files go, creating it if needed (the "Create … and move the files there" row);
  else to that name ("Rename to" / "Copy as"). Moving or copying onto or into itself is
  refused. Files already there are skipped unless Overwrite existing is on.
- **Recent destinations** are the prompt's own history (`dired-destination`), kept for
  the session.
- **Background, like deletion**: `jobs::run` runs a file operation on a thread and
  `jobs::pick_up` reports it, re-reads the listings it changed and makes buffers visiting
  moved files follow them (`Editor::file_moved`, renamed after them). A move across
  volumes copies and then removes; copies keep permissions and modified times, links stay
  links.

## Later

"Move with git mv" comes with the Git milestone; recent destinations are not saved
between sessions until the minibuffer history is.
