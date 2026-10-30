# remacs

Emacs-inspired IDE from scratch in Rust + gpui. Everything is a plugin.

## Code guidelines

- Concise code, minimal comments: only the non-obvious "why", never the "what".
- Separated responsibilities; DRY and SOLID.
- Readable, descriptive names for variables, types and functions.
- Everything tunable goes into config, not code constants: colors, sizes, timings, limits,
  key bindings, behavior toggles. When adding or changing a feature, expose its settings
  and document them, with their `Default` values, in the section's file in
  `config/defaults/` (a new section adds its file there and to `DEFAULT_FILES` in
  `crates/config`); each section's test checks its file reads as its `Default`.
  `write-default-settings` copies a file into the user's `~/.config/remacs/config/`.
- Write it compact the first time; a refactor that only cuts lines means the first draft
  was wrong:
  - Before writing a helper, grep `crates/` and `plugins/` for one that exists. The second
    copy of any logic goes into one shared function, a table, or a `macro_rules!` when a
    function can't (see `settings_sections!`, `check_ranges!`, `icons!`, `hex_colors!`).
  - Functions that differ by one value become one function taking it; parallel match arms
    become a table iterated in a loop.
  - Prefer `#[derive(Default, Clone, Copy)]` and `..Default::default()` over hand-written
    impls and field-by-field copies; store a settings sub-struct whole.
  - Use `let … else`, `?` and `Option`/`Result` combinators over nested `match`/`if let`,
    and iterator adaptors over hand-written loops.
  - No dead code: every `pub` item has a caller somewhere in the workspace.
  - Keep files under 1000 lines. Past ~700, split by responsibility into `foo.rs` +
    `foo/<part>.rs` with `pub(super)` items, as `minibuffer_view/`, `outline_view/` and
    `settings/` do.
  - Tests share setup through helpers in the test module. Never weaken an assertion to
    save lines: compare whole structs with `assert_eq!`.
- gpui views (`crates/app`): build elements from `widgets.rs`.
  - `StyleExt` replaces common chains: `.text(size, color)`, `.status_text`,
    `.mono_status`, `.row(gap)`, `.column()`, `.pad(top, x, bottom)`, `.bar(height, padding)`,
    `.rounded_fill(radius, rgba)`, `.bordered(rgba)`, `.top_border(rgba)`, `.fg(color)`,
    `.mono()`, `.ui()`, `.fill_truncated()`, `.across()`.
  - `PressExt::on_press(cx, |view, cx| …)` replaces `on_mouse_down` + `stop_propagation` +
    `cx.listener`.
  - Reuse the existing builders: `kbd`, `icon`, `pill`, `segment`, `option_chip`,
    `list_items`, `glass_surface`, `modal_surface`, `shadow`, `outline_view::chip_frame`,
    `outline_view::lift_selected`, `preview_view::code_well`, `changes_view::diff_line`.
  - A style shorthand two views would use goes into `StyleExt`, never a second trait.
- Shared non-UI helpers live in their crates; reuse them instead of rewriting:
  `remacs_core::local_tm` for local time, `remacs_text::Edit::moved` to shift an offset
  past an edit.
- Formatting is `rustfmt.toml` (`use_small_heuristics = "Max"`); run `cargo fmt --all`
  before every commit.
- Menus follow commands: when adding, renaming or removing a command a person would look
  for, update the menu items its plugin declares (`MenuItem` in `Registry::menu_items`,
  with `enabled_when` if it cannot always run); a new group or menu goes into the `frame`
  plugin's layout (ADR 0060). The test in `crates/app/src/menu_bar.rs` checks that every
  item runs an existing command.
- Performance changes are always measured: benchmark first (divan with its allocation
  profiler in the crate's `benches/`, dhat for peak heap, or a test counting calls or
  frames), record the baseline, change the code, measure again with the same benchmark.
  Keep a change only if it gains at least 10% in time, allocations or peak heap;
  otherwise revert it. The benchmark stays in the repo.

## Fixed decisions

- Platform: macOS first; a minimal Linux build (ADR 0089), checked on Arch + KDE Wayland.
- Third-party plugins: WASM (wasmtime). Built-in plugins: Rust crates, registered statically.
- License: Apache-2.0 + Commons Clause (open source, no paid redistribution by third parties).
- Priorities: latency, memory, files measured in gigabytes.
- Initial languages (tree-sitter + LSP): Rust, C/C++, TypeScript/JavaScript, JSX/TSX, HTML, CSS.

## Workflow

- Commits: Conventional Commits, subject line only.
- Every change goes through a pull request into `dev`, the default branch: a branch from
  `origin/dev` (`git switch -c <type>/<name> --no-track origin/dev`), pushed under its own
  name, a PR titled like a commit with `Closes #N` for its issue. Never push to `dev` or
  `main`; only the owner merges, once CI's `macos`, `linux` and `audit` pass. The issue closes
  with the merge. `main` takes only releases, merged from `dev` (ADR 0056).
- Actions in `.github/workflows` are pinned by commit SHA with the version in a comment
  (`uses: owner/action@<sha> # v1.2.3`); Dependabot bumps both.
- GitHub: the repo is `MakhnoGK/remacs`; run `gh` as the `MakhnoGK` account, which the
  active `gh` account may not be: `GH_TOKEN=$(gh auth token --user MakhnoGK) gh … -R MakhnoGK/remacs`.
- After every completed action, re-index the repo with Cerebrium `code_index`.
- The installed app is the Homebrew release (`/opt/homebrew/opt/remacs/Remacs.app`,
  `remacs` on the command line, from the tap `MakhnoGK/homebrew-remacs`): update it only
  when asked, with `brew upgrade remacs`. Do not run `cargo xtask install`: its launcher
  in `~/.local/bin` comes first on `PATH` and hides the Homebrew one.

ADRs live in `docs/adr/`.
