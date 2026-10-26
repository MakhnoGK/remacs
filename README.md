# remacs

[![CI](https://github.com/MakhnoGK/remacs/actions/workflows/ci.yml/badge.svg)](https://github.com/MakhnoGK/remacs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/tag/MakhnoGK/remacs?filter=v*&sort=semver&label=release)](https://github.com/MakhnoGK/remacs/releases)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/MakhnoGK/remacs/badge)](https://scorecard.dev/viewer/?uri=github.com/MakhnoGK/remacs)
[![macOS](https://img.shields.io/badge/macOS_14+-Apple_Silicon-lightgrey?logo=apple)](#install)
[![Linux](https://img.shields.io/badge/Linux-x86__64-lightgrey?logo=linux&logoColor=white)](#linux)
[![Homebrew](https://img.shields.io/badge/brew-makhnogk%2Fremacs-orange?logo=homebrew)](https://github.com/MakhnoGK/homebrew-remacs)
[![License](https://img.shields.io/badge/license-Apache--2.0_+_Commons_Clause-blue)](LICENSE)

An Emacs-inspired IDE for macOS and Linux, written from scratch in Rust on [gpui](https://www.gpui.rs).
Emacs keys, language servers, tree-sitter, Git, a debugger, terminals; everything is a
plugin. Alpha: expect rough edges, and report them in the
[issues](https://github.com/MakhnoGK/remacs/issues).

## Install

On macOS 14 or later on Apple Silicon, from Homebrew, into /Applications (Launchpad and
Spotlight find it) with `remacs` on the command line:

```sh
brew install --cask makhnogk/remacs/remacs
```

`brew upgrade remacs` updates it to the next release. The app is not notarized, so the
cask removes macOS's quarantine from it. `brew install --HEAD makhnogk/remacs/remacs`
builds `dev` instead (needs Xcode); uninstall one before installing the other.

### From source

On macOS 14, or to change remacs. Needs stable Rust (CI builds with 1.98) and Xcode with
its Metal toolchain (gpui compiles its shaders at build time):

```sh
xcodebuild -downloadComponent MetalToolchain   # Xcode 26 and later ship it separately
git clone --branch main https://github.com/MakhnoGK/remacs && cd remacs
cargo xtask install
```

`main` is the latest release; `dev`, the default branch, has what comes next. This
builds `Remacs.app` into `~/Applications` and a `remacs` launcher into `~/.local/bin`
(put it on your `PATH`). `git pull` and `cargo xtask install` again update it;
`cargo xtask rollback` brings back the app it replaced. Without the Metal toolchain,
`cargo build --release -p remacs-app --features runtime-shaders` builds a binary that compiles
the shaders at startup instead. Its launcher comes before Homebrew's on a usual `PATH`,
so have one of the two, not both.

There is no .dmg: the app is signed ad hoc, not notarized, so macOS would block one
downloaded from a browser; the cask removes the quarantine that blocks it.

### Linux

x86_64 with glibc 2.35 or later and a Vulkan driver; checked on Arch with KDE Plasma on
Wayland. Download `remacs-<version>-x86_64-linux.tar.gz` from the
[releases](https://github.com/MakhnoGK/remacs/releases), then:

```sh
tar -xzf remacs-*-x86_64-linux.tar.gz
remacs-*-x86_64-linux/install.sh     # or install.sh <prefix>; ~/.local by default
```

It puts `remacs` in `~/.local/bin` and Remacs in the applications menu. From source,
`cargo fetch --locked && cargo xtask install` does the same (needs `cargo install
cargo-about --locked --features cli` and the Wayland, X11, Vulkan and fontconfig headers).

## First run

```sh
remacs ~/src/my-project      # a folder opens its listing; a file opens the file
```

or open the app from Finder or the applications menu, or drop files onto its window. A project's open files come
back when you open it again, or on a start without arguments.

- Language servers: when a project needs one that is not installed, remacs offers to
  download it (Edit ▸ Download Language Servers offers them again). Debug adapters download
  on the first debug. `C-c t` lists what was downloaded.
- Grammars for syntax highlighting are downloaded when a file first needs one.
- Logs go to `~/.config/remacs/state/remacs.log`; a crash writes a report to
  `~/.config/remacs/state/crashes/`, and the next start says where. Attach it to an issue.

## Keys

Emacs keys, with the ⌘ ones of Emacs on macOS. Option is Meta (`M-`; `ui.toml`
`[keyboard] meta` keeps one Option key for typing), Command is `s-`. On Linux, Alt is Meta,
Super is `s-`, and `C-S-c` / `C-S-v` copy and paste, as in a terminal.

| Keys | Does |
| --- | --- |
| `M-x`, `⌘⇧P` | run any command by name |
| `C-g` | cancel |
| `C-x C-f`, `⌘O` | open a file |
| `C-x C-s`, `⌘S` | save |
| `C-x b` / `C-x k` | switch to / kill a buffer |
| `C-x d` | a folder's listing (dired) |
| `C-s` / `C-r`, `⌘F` | search the buffer |
| `⌘⇧F`, `M-s g` | search the project |
| `M-.` / `M-?` | go to definition / find references |
| `C-c C-r` | rename the symbol at point |
| `C-x g` | Git status |
| `C-x 2` / `C-x 3` / `C-x o` / `C-x 0` | split below / right, next window, close it |
| `⇧F10` / `⇧F9` | run / debug the project |
| ``C-` ``, `⌘T` | show a terminal / a new one |
| `C-h k` / `C-h x` / `C-h b` | describe a key / a command, list the view's keys |
| `C-x C-c` | quit |

The menus show the main commands with their keys; Linux has no menu bar yet, so there
`M-x` runs them. `keys.toml` changes any binding (see Config); `C-c` and a letter is left
for your own.

## Config

Settings are TOML files in `~/.config/remacs/config/` (`$XDG_CONFIG_HOME/remacs` if set),
one per section: `ui.toml` for the theme, font and Option keys, `keys.toml` for key
bindings, `lsp.toml` for language servers, and so on. Each file holds only what you change,
and changes apply when it is saved.

- `⌘,` lists the settings files.
- `M-x view-default-settings` (Help menu) shows a section's defaults, every setting
  documented; `M-x write-default-settings` (Remacs ▸ Default Settings File…) copies them
  into your config folder to edit. The same files
  are in [`config/defaults/`](config/defaults).
- Themes: `M-x select-theme` (View ▸ Theme…); your own go in `~/.config/remacs/themes/`.

For example, `~/.config/remacs/config/keys.toml`:

```toml
[bindings]
"C-c f" = "find-file"
"s-k" = ""   # unbind
```

## Docs

Design decisions are in [`docs/adr/`](docs/adr), changes in [`CHANGELOG.md`](CHANGELOG.md).

## License

Apache-2.0 with the Commons Clause: free to use, change and share, not to sell. See
[`LICENSE`](LICENSE) and [`LICENSE-APACHE`](LICENSE-APACHE).
