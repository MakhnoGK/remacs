# 0060 — The macOS menu bar, filled from commands

Status: accepted, 2026-10-11 (issues #18, #19)

## Decision

- **The app is called Remacs** wherever a person reads its name: `CFBundleName` and
  `CFBundleDisplayName` (menu bar, Dock, ⌘-Tab, Activity Monitor, About) and the window
  title, not drawn but shown in Mission Control and the Window menu. The `remacs` command,
  the executable and `~/.config/remacs` stay lowercase.
- **Menus are declared by plugins, next to the commands they run.** A
  `remacs_core::MenuItem` names its menu, its group, its title and its command, and
  optionally `enabled_when(fn(&Editor) -> bool)`; plugins push them to
  `Registry::menu_items`. Every item runs a command, so a menu shows nothing a key or
  `M-x` cannot do.
- **The layout is the `frame` plugin's**, registered first: `Registry::menus` lists
  Remacs, File, Edit, View, Go, Git, Run, then Window and Help (`trailing`), each with the
  order of its groups. Within a group items keep the order they were added in, so plugins
  share groups by name (dired's "Open Directory…" joins files' "open" group). A menu or
  group nobody declared follows the declared ones, before the trailing menus. Groups are
  apart by separators.
- **Keys come from the keymap as bound now**, `keys.toml` included; the menus are made
  again when the settings change. A single chord or function key is shown by macOS at
  the item's right (⌘S, ⇧F9); a sequence is put after the title, as GNU Emacs does on
  macOS: "Save All… (C-x s)". ⌘ chords are preferred, then the shortest sequence.
- **The keys stay with the keymaps.** gpui shows only key equivalents bound in its own
  keymap, so each is bound there under a context no element has (`RemacsMenu`): it shows,
  and never matches. A key the editor takes stops there, so the menu's key equivalent
  does not run the command again; only a single key nothing binds goes on to macOS
  (`Editor::handle_key` returns false), e.g. ⌘\` to cycle windows.
- **Disabled items**: an item is disabled while its command is missing or its
  `enabled_when` is false, asked when a menu opens. gpui decides by action type only, so
  the vendored gpui has `App::on_validate_menu_action` (see ADR 0034's list of patches).
  Used for: Cut/Copy/Shrink Selection without a region, Paste Earlier after no paste,
  code items (definition, rename, format, actions, completion, documentation, problems,
  run/debug at point, breakpoints) outside a file buffer or with a prompt open, Git items
  other than Status without a status buffer shown, Stop with nothing running, stepping
  and Evaluate while not paused, Pause while not debugging.
- **The application and its window are commands** of the `frame` plugin, Emacs-named:
  `about-remacs` (macOS's about panel, through objc2), `open-settings` (⌘, — the
  settings directory in dired), `hide-remacs` (⌘H), `hide-others` (⌥⌘H), `show-all`,
  `iconify-frame` (⌘M), `toggle-frame-maximized` (Zoom), `toggle-frame-fullscreen` (⌃⌘F).
  Commands ask through `Editor::request_frame`; the view does it once the update is over.

## Consequences

- A new command that a person may look for gets a menu item in its plugin; CLAUDE.md
  says so.
- Help → Search finds menu items, so commands can be found by name with the mouse too.
- An item cannot show a check mark (gpui has none): toggles are titled "… On or Off".
- Menus run commands with no prefix pending in the editor; one typed before choosing an
  item stays pending.
