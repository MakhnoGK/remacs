# 0046 — What the project's file index leaves out

Status: accepted, 2026-10-06

## Decision

- **Patterns of remacs' own, not the project's `.gitignore`** (user: `.env` files and the
  like are searched too, and Git ignores them). `files.toml`:

  ```toml
  ignore = ["node_modules/", "target/", "dist/", "build/", ".next/", ".DS_Store"]
  use-gitignore = false   # true also leaves out what .gitignore and .ignore files do
  ```

  Patterns are `.gitignore` syntax (`ignore::gitignore`), matched from the project root,
  so `name/`, `/name` and globs work; a bad one is reported and the defaults used.
  `.git/` is always left out.
- **Hidden files are listed**: `.env`, `.github/`, `.eslintrc` are found; what should not
  be goes in `ignore`.
- **One index for find-file and project-search**, so both follow the setting. A change
  drops the indexes, and the next prompt scans again. The cache file is named after a
  hash of the settings (`files-<hash>`), so a cache of other settings is never shown.
- File system events under ignored paths are dropped; `.gitignore` edits rescan
  everything only while `use-gitignore` is on.
