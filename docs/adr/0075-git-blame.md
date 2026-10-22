# 0075 — Git blame

Status: accepted, 2026-10-22 (issue #98)

## Decision

- **Blame opens in a buffer of its own**, like Emacs `vc-annotate`: `git-blame` (C-x v g,
  Git → Blame) shows the visited file's text in a read-only buffer `<path> (blame)`, the
  path from the working tree's root, in the file's mode, so it is highlighted and
  searched like the file. The file buffer is not touched.
- **It blames the buffer's text**, unsaved edits included (`git blame --porcelain
  --contents -`); lines not committed yet say so. Read on a thread of its own (#56). The
  text goes to git as saving would write it (`Format::write`), and what git prints back is
  read in the file's format, so files in CR LF or another encoding match their commits.
  Paths are printed unquoted (`core.quotePath=false`), so non-ASCII ones can be blamed
  again. Reading 100 000 lines from 5 000 commits takes 13 ms (`cargo bench -p remacs-git
  --bench blame`; 588 ms before commits were looked up by id in a map).
- **The gutter holds the annotation** (`LineExtra::number`, `TextControls::number_width`):
  on the first line of each run of lines from one commit, its short id, author (cut to
  `git.toml` `blame-author-width`) and age; the other lines of the run leave it blank.
- **Keys in it**: RET opens the line's commit in its commit buffer at the file (ADR 0064);
  `b` blames the file as it was before that commit, in `<file> @ <id> (blame)`, near the
  same line; `q` goes back.

## Later

- Blame inside the file buffer, like magit-blame.
- Colouring runs by age.
