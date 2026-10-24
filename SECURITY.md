# Security

## Supported versions

remacs is in alpha: only the latest release, and `main`, get fixes.

## Reporting a vulnerability

Report it privately through
[GitHub's security advisories](https://github.com/MakhnoGK/remacs/security/advisories/new),
not in a public issue. Say what an attacker can do, and how to reproduce it: a file or
project that triggers it, the steps, the version (`remacs --version`).

You get an answer within a week. Once a fix is released, the advisory is published with
credit to you, unless you ask otherwise.

## What remacs trusts

What is worth reporting is anything that runs code or reaches the network beyond these:

- Opening a project runs its language servers, linters and formatters, some from its own
  `node_modules`, as you would in a terminal there.
- Run configurations, compile commands and terminals run what you tell them to.
- Language servers, debug adapters and grammars are downloaded from their GitHub releases
  and checked against the release's checksum, into `~/.config/remacs/cache/`; installers
  (npm, pip, cargo, …) fetch packages from their registries.
- The settings in `~/.config/remacs/` are trusted as your own.
