# 0017 — Git: the history beside the status

Status: accepted, 2026-10-04 (Git, step 3)

## Decision

- **Outlines gain a history**: `Outline::history` is drawn right of the pane when the
  window has room: a header (All branches / Current, `/` Search history) over one 32 px
  row per commit — graph, ref chips, subject, short author, relative date, short id.
  Rows are not selectable yet; commits get actions with later steps.
- **Read with `git log --date-order`** on the monitor thread, `--branches --remotes
  --tags HEAD` or only `HEAD`, 200 commits a page. The log is kept until a ref, `HEAD`
  or the base changes (`git for-each-ref` is the fingerprint), so file edits do not
  read it again.
- **Lanes are laid out in the plugin** (`history::layout`), like `git log --graph`: a
  commit sits in the lane a child left for it, else in a new one, and leaves lanes for
  its parents. When several lanes wait for one commit the base's wins, so shared
  history goes on in the base's lane. Colors index the theme's graph palette: the
  checked out branch's, the base's, then the others' in turn. The UI draws each row's
  upper and lower edges with a canvas; curves join lanes.
- **Search** (`/`) shows commits whose message holds the words (`--grep -i -F`); they
  are not connected, so the graph is dots. An empty search shows everything again.
- **Scrolling**: the wheel over the history, or `C-M-v` / `C-M-V` (20 commits, like
  `scroll-other-window`), moves the first commit shown; nearing the last one read asks
  for another page. `l` switches All branches and Current.

## Later

Selecting a commit and acting on it come with the steps that need them (branches,
rebase).
