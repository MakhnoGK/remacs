# 0015 — Git: status and file states

Status: accepted, 2026-10-04 (Git, step 1)

## Decision

- **Core knows version control, not Git**: `VersionControl` (`name`, `move_command`,
  `controls`, `status`, `record_moves`) is registered in `Registry::version_controls`;
  `Editor::version_control`, `file_status` and `record_moves` ask the first that controls
  a path. `FileStatus` is a `FileState` (Added, Modified, Renamed, Deleted, Conflicted,
  Untracked) with the lines added and removed since the last commit.
- **Reads go through the `git` command**, not a library: `git status --porcelain=v2 -z
  --branch --show-stash` and `git diff --numstat -z HEAD`, with `GIT_OPTIONAL_LOCKS=0` so
  that reading never writes the index or blocks the user's own commands. Git itself gets
  fsmonitor, the untracked cache, sparse checkouts, submodules and filters right, and the
  reads are off the UI thread. A library (gix) stays an option for walking history.
- **One monitor per working tree a buffer is in**: `plugins/git` finds the tree by the
  nearest `.git`, a thread reads its status and reads it again on FSEvents, settled 100
  ms and at most 1 s apart. Events for ignored paths (each directory's `.gitignore`,
  `.git/info/exclude`, `core.excludesFile`) and for `objects/`, `logs/` and `*.lock` in
  the Git directory are dropped, so a build does not cause reads. A tree no buffer is in
  is no longer watched.
- **Dired shows the states**: a `CellStyle::State` column after the name (`Row::state`),
  the letter beside the preview's title and a "Git" line after the details
  (`Preview::version`). `TableProvider` takes the `Editor` to ask.
- **Moves are recorded as `git mv` would**: after dired moves files, the index entries at
  or under each source move to the destination (`git ls-files --stage` into `git
  update-index --index-info`), so what was staged stays staged and the rest unstaged.
  "Move with git mv" (`M-g`, shown while moving) and "Rename with git mv" (wdired) are on
  by default and offered only in a working tree; `PromptOption::shown_while` hides an
  option while another is off, and its labels are owned strings.

## Later

The branch in the status pill comes with the Git screen (step 2), as the design shows it
there only. `git` is found on `PATH`, like the language servers.
