# Performance baseline

The numbers the beta's performance work is judged against (#182). A change that claims to
improve one of them runs `crates/app/benches/baseline.sh` before and after, on the same
machine, and quotes both.

## How it is measured

`crates/app/benches/baseline.sh [rust|c|go|ts ...]` builds the release binary and, for each
language, opens a pinned project with default settings in a fresh config home, then:

- **Input latency**: after 90 s for the servers to index, System Events presses Down 150 times
  then Up 150 times, 50 ms apart. With `frame-latency = true` in `logs.toml`, each frame that
  shows a key logs the time from the first key it shows reaching the view to the end of its
  painting; the GPU's drawing and the display's refresh come after it and are not counted.
- **Idle CPU**: 5 s after the last key, the CPU time used over 60 s by the app and, separately,
  by its descendants (the language servers), as a percentage of one core.
- **Memory**: the physical footprint (`footprint`, as Activity Monitor's Memory column) of the
  app and of its descendants together, right after the idle minute.

It needs Accessibility for the terminal that runs it, the servers on `PATH`, and nobody using
the machine meanwhile.

| Language | Project | File opened |
|---|---|---|
| Rust | BurntSushi/ripgrep 14.1.1 | `crates/core/flags/defs.rs` (7675 lines) |
| C | redis/redis 7.2.4 | `src/server.c` (7365 lines) |
| Go | spf13/cobra v1.8.1 | `command.go` (1896 lines) |
| TypeScript | pmndrs/zustand v4.5.5 | `src/middleware/devtools.ts` (381 lines) |

## Baseline — 2026-11-01, `691353f` (v0.1.0-alpha.5 + the probe)

MacBook Pro M1 Pro (MacBookPro18,3), 16 GB, macOS 27.2, built-in display at 3024×1964.
Servers: rust-analyzer (Homebrew), clangd (Xcode), gopls v0.20.0, typescript-language-server
(Homebrew). Two runs each; the Go figures are from the second and third, as the first one
also downloaded cobra's modules (191 MB app, 407 MB servers).

| Language | Key to frame, median ms | p95 ms | max ms | Idle CPU app % | Idle CPU servers % | App MB | Servers MB | Servers |
|---|---|---|---|---|---|---|---|---|
| Rust | 3.6 / 3.5 | 5.4 / 4.8 | 10.7 / 10.8 | 0.87 / 0.82 | 0.00 / 0.00 | 214 / 217 | 672 / 639 | rust-analyzer, proc-macro-srv ×2 |
| C | 3.6 / 3.9 | 5.3 / 5.2 | 7.2 / 7.0 | 0.83 / 0.82 | 0.00 / 0.00 | 105 / 111 | 76 / 75 | clangd |
| Go | 3.7 / 3.5 | 5.3 / 5.0 | 10.6 / 11.5 | 0.87 / 1.07 | 0.05 / 0.05 | 102 / 102 | 156 / 156 | gopls ×2 |
| TypeScript | 4.1 / 3.8 | 5.8 / 5.3 | 11.0 / 8.0 | 2.13 / 0.95 | 0.05 / 0.07 | 104 / 104 | 77 / 78 | node, tsc |

Every key drew exactly one frame (300 of 300 in each run).

What the noise allows: latency medians move by up to 0.3 ms and p95 by 0.6 ms between runs;
idle CPU by about 0.2 points, with a single 2.1 % outlier; footprint by up to 6 MB for the
app and 35 MB for rust-analyzer. A change smaller than that is not measured by this.
