# 0063 — Language servers for every language, offered for download

Status: accepted, 2026-10-14 (issue #10, tier 2 of ADR 0052's plan; the server part of #50)

## Decision

- **Language servers are the third level of help** (user, 2026-10-14): grammars come
  built in, servers are offered for download, never shipped. Every mode with a server
  worth running has one built in: 29 of the 31 (Make and Diff have none).
- **The servers' catalog is data**: `plugins/languages/servers.toml`, compiled in, lists
  each server in the order servers serve a file — the language's own first, add-ons
  (linters, Tailwind) after. `lsp.toml` `[servers.<name>]` changes a built-in server
  field by field, turns it off (`enabled = false`) or adds a server of one's own in the
  same form, with no rebuild. Built servers are leaked: everything that talks to a server
  holds `&'static LanguageServer`, and only a changed `lsp.toml` builds them again, so the
  leak is a few kilobytes a change.
- **Where a server's program is looked for**: `lsp.toml` `command`, run as it is; the
  project's `node_modules/.bin` for servers that prefer it (linters); `PATH`; then the
  copy remacs downloaded. A server's own install always wins over a download.
- **Add-on servers say when they serve** (`when`, any of): `files` in the file's directory
  or above, up to the Git work tree, `*` matching part of a name (`tailwind.config.*`);
  `dependencies` of a `package.json` there, `@scope/*` a whole scope (Tailwind 4's
  `@tailwindcss/vite`); text a file `contains` (`[tool.ruff` in `pyproject.toml`). It
  replaces `required_files`. Root markers take `*` too (`*.csproj`).
- **Alternatives form a group** (Python: basedpyright, pyright, ty, pylsp; also TOML, SQL,
  C#, PHP, Ruby, Nix): the first of the group whose program is found serves; a project
  lacking all of them is offered a choice of those remacs can download (user,
  2026-10-14).
- **Offers, once per project and server or group** (user, 2026-10-14): a dialog lists the
  single servers a project wants and lacks; a group asks which one. Either answer is kept
  in `~/.config/remacs/state/language-servers.json`; a project is its Git work tree, else
  the server's root. Downloaded, a server serves every project and starts at once where
  it is wanted. Edit ▸ Download Language Servers (`lsp-download-servers`) offers the
  window buffer's again; the Tools panel (C-c t) downloads any. `lsp.toml`
  `offer-downloads = false` turns offers off.
- **A server remacs cannot download is passed over in silence** (user, 2026-10-14):
  nothing is shown and nothing holds the user up; a missing program is no error any more.
  An `lsp.toml` `command` that does not run still is one.
- **Downloads come from GitHub releases or an installer.** GitHub: the asset's name tells
  how to unpack it (zip/vsix, tar.gz, tar.xz, gz, a bare program); `{tag}`, `{version}`,
  `{arch}`, `{cpu}`, `{goarch}` and `{target}` fill in a build's name, and a tool can name
  the architectures its own way (lemminx's `aarch_64`); a tag prefix picks a tool's
  releases out of a repository that releases several (oxc, biome). Installers are data
  too: `plugins/tools/installers.toml` (npm, go, pip, gem, dotnet, cargo, coursier, url)
  with `tools.toml` `[installers]` over it — what must be on `PATH`, how the latest
  version is looked up (a command, or a JSON answer and a pointer), the install commands,
  where the program lands and the environment it runs with (gem's `GEM_PATH`). A package
  is installed straight into its version's directory, marked unfinished until done, as
  venvs and gem stubs are not relocatable. Tools of one package share one install
  (vscode-langservers-extracted serves HTML, CSS, JSON and eslint).
- **A tool needing what is not here is not offered**: an installer's `needs` (npm, go,
  python3, gem, dotnet, cargo, cs) and a server's own (`java` for jdtls and metals,
  `elixir` for expert, `zig` for zls, `php` for phpactor, `dotnet` for OmniSharp).
- **Servers only on `PATH`**: those that come with their language's tools —
  sourcekit-lsp (Xcode), `dart language-server` (Dart/Flutter SDK),
  haskell-language-server (GHCup), nil and nixd (Nix).
- **Off until turned on**: harper-ls (prose) and emmet, as no project says it wants them.

## Consequences

- `remacs-lsp` depends on `remacs-tools`; the LSP plugin hands its servers' tools to the
  tools plugin (`provide`), which lists them in the Tools panel beside the debug
  adapters. `tools.toml` naming a tool that is none of them is reported then.
- `LanguageServer` lost `required_files` for `when`, gained `group`, `tool` and
  `own_program`; initialization options and configuration are JSON values.
- `Client::start` gets the program already looked up, with its environment.
- Missing servers are remembered per directory, like those a project does not use, so a
  sync looks for a program once; a download, `lsp-restart` or `lsp.toml` change looks
  again. A group's next server is tried in the same sync.

## Later

- More grammars (D and others have none): how to support many more is still open.
- zls must suit the project's Zig; the latest is downloaded.
- jdtls is unversioned (Eclipse's snapshot); its launcher's `-data` default is unchecked.
- Tailwind 4 projects without a `package.json` name the server in `lsp.toml` `when`.
- Debuggers already download on run/debug (tier 3, ADR 0051).
