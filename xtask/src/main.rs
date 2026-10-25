//! `cargo xtask install` builds remacs, bundles it as `Remacs.app` and puts it in
//! `~/Applications`, keeping the app it replaces; `cargo xtask rollback` swaps them back.
//! The installed app is a snapshot: building the workspace afterwards does not touch it.
//! On Linux, the bundle is a folder laid out as a prefix, installed under `~/.local`.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const APP: &str = "Remacs.app";
const PREVIOUS: &str = "Remacs.previous.app";
const EXECUTABLE: &str = "remacs";
/// Info.plist's bundle identifier; on Linux, the window's app id and the names of the
/// menu entry and icon.
const APP_ID: &str = "io.github.makhnogk.remacs";
/// Linux's icons, `crates/app/bundle/linux/icons/<size>.png`, rendered from the macOS
/// masters.
const LINUX_ICON_SIZES: [u32; 7] = [16, 32, 48, 64, 128, 256, 512];
/// In the bundle's Resources, where `view-third-party-notices` finds it.
const NOTICES: &str = "THIRD-PARTY-NOTICES";
const USAGE: &str =
    "usage: cargo xtask <bundle | install | rollback | changelog [version] | grammars <dir>>
  bundle     build target/bundle/Remacs.app; on Linux, target/bundle/remacs-<version>-<cpu>-linux
  install    bundle, then install it in ~/Applications and `remacs` in ~/.local/bin;
             on Linux, all of it under ~/.local
  rollback   swap the installed app with the one it replaced (macOS)
  changelog  write CHANGELOG.md from the history, what is untagged under `version`
  grammars   build grammars.toml's grammars to WebAssembly in `dir`, with their licenses";

/// Builds grammars to WebAssembly; it fetches the WASI SDK it compiles with itself.
const TREE_SITTER_CLI: &str = "tree-sitter-cli@0.27.1";

/// The commit types that make a changelog's groups, in its order; the others (chore, ci,
/// docs, refactor, style, test) change nothing people see.
const CHANGELOG_GROUPS: &[(&[&str], &str)] = &[
    (&["feat", "feature"], "Added"),
    (&["fix"], "Fixed"),
    (&["perf"], "Faster"),
];

/// The iconset's images: its name, the master it comes from, its size in pixels. 16 and
/// 32 px have masters of their own, drawn heavier.
const ICONS: &[(&str, &str, u32)] = &[
    ("icon_16x16.png", "icon-16.png", 16),
    ("icon_16x16@2x.png", "icon-32.png", 32),
    ("icon_32x32.png", "icon-32.png", 32),
    ("icon_32x32@2x.png", "icon-1024.png", 64),
    ("icon_128x128.png", "icon-1024.png", 128),
    ("icon_128x128@2x.png", "icon-1024.png", 256),
    ("icon_256x256.png", "icon-1024.png", 256),
    ("icon_256x256@2x.png", "icon-1024.png", 512),
    ("icon_512x512.png", "icon-1024.png", 512),
    ("icon_512x512@2x.png", "icon-1024.png", 1024),
];

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["bundle"] => bundle().map(|app| println!("{}", app.display())),
        ["install"] => install(),
        ["rollback"] => rollback(),
        ["changelog"] => changelog(None),
        ["changelog", version] => changelog(Some(version)),
        ["grammars", dir] => grammars(Path::new(dir)),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is inside the workspace")
        .to_owned()
}

fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

fn bundle() -> Result<PathBuf> {
    let root = root();
    let version = Version::read(&root)?;
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(Command::new(&cargo)
        .current_dir(&root)
        .args(["build", "--release", "--package", "remacs-app"])
        .env("REMACS_VERSION", &version.version)
        .env("REMACS_COMMIT", &version.commit))?;

    let sources = root.join("crates/app/bundle");
    let out = root.join("target/bundle");
    let notices = notices(&cargo, &root, &sources, &out)?;
    if cfg!(target_os = "linux") {
        return bundle_linux(&root, &sources, &out, &notices, &version);
    }
    let app = out.join(APP);
    remove(&app)?;
    let contents = app.join("Contents");
    let resources = contents.join("Resources");
    create(&contents.join("MacOS"))?;
    create(&resources)?;

    fs::copy(
        root.join("target/release").join(EXECUTABLE),
        contents.join("MacOS").join(EXECUTABLE),
    )
    .map_err(|error| format!("copying the binary: {error}"))?;
    fs::copy(&notices, resources.join(NOTICES))
        .map_err(|error| format!("copying the notices: {error}"))?;
    let plist = read(&sources.join("Info.plist"))?;
    write(&contents.join("Info.plist"), &version.fill(&plist))?;
    icon(
        &sources,
        &out,
        &resources.join(format!("{EXECUTABLE}.icns")),
    )?;

    // Ad hoc: enough for a Mac that built it, and seals Info.plist and the icon.
    run(Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&app))?;
    Ok(app)
}

/// `remacs-<version>-<cpu>-linux`, laid out as a prefix: the binary and its notices in
/// lib/remacs, the launcher in bin, the menu entry and icons in share; its `install.sh`
/// copies them under one, `~/.local` by default (ADR 0089).
fn bundle_linux(
    root: &Path,
    sources: &Path,
    out: &Path,
    notices: &Path,
    version: &Version,
) -> Result<PathBuf> {
    let linux = sources.join("linux");
    let desktop = format!("{APP_ID}.desktop");
    let folder = out.join(format!(
        "{EXECUTABLE}-{}-{}-linux",
        version.version,
        env::consts::ARCH
    ));
    remove(&folder)?;
    let (bin, lib) = (folder.join("bin"), folder.join("lib").join(EXECUTABLE));
    let applications = folder.join("share/applications");
    for dir in [&bin, &lib, &applications] {
        create(dir)?;
    }
    copy(
        &root.join("target/release").join(EXECUTABLE),
        &lib.join(EXECUTABLE),
    )?;
    copy(notices, &lib.join(NOTICES))?;
    copy(&linux.join("launcher.sh"), &bin.join(EXECUTABLE))?;
    copy(&linux.join("install.sh"), &folder.join("install.sh"))?;
    copy(&linux.join(&desktop), &applications.join(&desktop))?;
    for size in LINUX_ICON_SIZES {
        let apps = folder.join(format!("share/icons/hicolor/{size}x{size}/apps"));
        create(&apps)?;
        copy(
            &linux.join(format!("icons/{size}.png")),
            &apps.join(format!("{APP_ID}.png")),
        )?;
    }
    Ok(folder)
}

/// The licenses of every crate built in, the tree-sitter grammars among them, from their
/// own files (`--offline`); made again only when the lock file or the template changed,
/// as it takes half a minute.
fn notices(cargo: &str, root: &Path, sources: &Path, out: &Path) -> Result<PathBuf> {
    let notices = out.join(NOTICES);
    let (config, template) = (sources.join("about.toml"), sources.join("notices.hbs"));
    let made = modified(&notices);
    if made.is_some()
        && [root.join("Cargo.lock"), config.clone(), template.clone()]
            .iter()
            .all(|input| modified(input) < made)
    {
        return Ok(notices);
    }
    create(out)?;
    // Written aside first, so that a failure leaves no half file to be taken as made.
    let written = out.join(format!(".{NOTICES}.new"));
    run(Command::new(cargo)
        .current_dir(root)
        .args(["about", "generate", "--offline", "--locked", "--config"])
        .arg(&config)
        .arg("--manifest-path")
        .arg(root.join("crates/app/Cargo.toml"))
        .arg("--output-file")
        .arg(&written)
        .arg(&template))
    .map_err(|error| {
        format!("{error}; it is installed by `cargo install cargo-about --locked --features cli`")
    })?;
    rename(&written, &notices)?;
    Ok(notices)
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn icon(sources: &Path, out: &Path, icns: &Path) -> Result {
    let iconset = out.join(format!("{EXECUTABLE}.iconset"));
    remove(&iconset)?;
    create(&iconset)?;
    for &(name, master, size) in ICONS {
        let size = size.to_string();
        run(Command::new("sips")
            .args(["--resampleHeightWidth", &size, &size])
            .arg(sources.join(master))
            .arg("--out")
            .arg(iconset.join(name)))?;
    }
    run(Command::new("iconutil")
        .args(["--convert", "icns", "--output"])
        .arg(icns)
        .arg(&iconset))
}

fn install() -> Result {
    let built = bundle()?;
    if cfg!(target_os = "linux") {
        run(Command::new("sh").arg(built.join("install.sh")))?;
        let version = Version::read(&root())?;
        println!(
            "installed remacs {} ({}) in ~/.local/lib/{EXECUTABLE}\n`{EXECUTABLE}` starts it: ~/.local/bin/{EXECUTABLE}",
            version.version, version.commit
        );
        return Ok(());
    }
    let home = home()?;
    let applications = home.join("Applications");
    create(&applications)?;
    let installed = applications.join(APP);
    let previous = applications.join(PREVIOUS);

    // Copied next to its place first, so the swap below is two renames on one volume.
    let staged = applications.join(format!(".{APP}.new"));
    remove(&staged)?;
    run(Command::new("ditto").arg(&built).arg(&staged))?;
    if installed.exists() {
        remove(&previous)?;
        rename(&installed, &previous)?;
    }
    rename(&staged, &installed)?;

    let launcher = home.join(".local/bin").join(EXECUTABLE);
    create(launcher.parent().expect("a file in a directory"))?;
    write(&launcher, &launcher_script(&installed))?;
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("{}: {error}", launcher.display()))?;

    let version = Version::read(&root())?;
    println!(
        "installed remacs {} ({}) in {}\n`{EXECUTABLE}` starts it: {}",
        version.version,
        version.commit,
        installed.display(),
        launcher.display()
    );
    Ok(())
}

/// Starts the installed app with this shell's environment and directory, apart from the
/// terminal, so that closing its tab does not quit remacs; the app sends what it prints to
/// its log.
fn launcher_script(app: &Path) -> String {
    let binary = app.join("Contents/MacOS").join(EXECUTABLE);
    format!(
        "#!/bin/sh\n\
         # Written by `cargo xtask install`.\n\
         [ \"$1\" = --version ] && exec '{binary}' --version\n\
         nohup '{binary}' \"$@\" </dev/null >/dev/null 2>&1 &\n",
        binary = binary.display(),
    )
}

fn rollback() -> Result {
    if cfg!(target_os = "linux") {
        return Err("on Linux, install keeps no previous remacs to go back to".to_owned());
    }
    let applications = home()?.join("Applications");
    let (installed, previous) = (applications.join(APP), applications.join(PREVIOUS));
    if !previous.exists() {
        return Err(format!("no app to go back to: {}", previous.display()));
    }
    let swapping = applications.join(format!(".{APP}.swap"));
    remove(&swapping)?;
    if installed.exists() {
        rename(&installed, &swapping)?;
    }
    rename(&previous, &installed)?;
    if swapping.exists() {
        rename(&swapping, &previous)?;
    }
    println!(
        "{} is back; the one it replaced is {}",
        installed.display(),
        previous.display()
    );
    Ok(())
}

/// Writes CHANGELOG.md from the history: a section per `v*` tag, newest first, of the
/// commits people notice; those after the last tag go under `version`, or "Unreleased".
fn changelog(version: Option<&str>) -> Result {
    let root = root();
    // Newest version first, with `0.1.0-alpha.1` before `0.1.0`.
    let tags = git(
        &root,
        &[
            "-c",
            "versionsort.suffix=-",
            "tag",
            "--list",
            "v[0-9]*",
            "--merged",
            "HEAD",
            "--sort=-v:refname",
        ],
    )?;
    let mut title = match version {
        Some(version) => format!("{} — {}", version.trim_start_matches('v'), today()?),
        None => "Unreleased".to_owned(),
    };
    let mut upper = "HEAD".to_owned();
    let mut text = "# Changelog\n\nWhat changed for people using remacs, release by release; \
                    written by `cargo xtask changelog` from the commit subjects.\n"
        .to_owned();
    for tag in tags.lines().map(Some).chain([None]) {
        let range = tag.map_or(upper.clone(), |tag| format!("{tag}..{upper}"));
        let subjects = git(&root, &["log", "--no-merges", "--format=%s", &range])?;
        text.push_str(&changelog_section(&title, subjects.lines()));
        if let Some(tag) = tag {
            let date = git(&root, &["log", "-1", "--format=%cs", tag])?;
            title = format!("{} — {date}", &tag[1..]);
            upper = tag.to_owned();
        }
    }
    write(&root.join("CHANGELOG.md"), &text)
}

/// `title`'s section of the subjects in their groups, or nothing when none is in one.
fn changelog_section<'a>(title: &str, subjects: impl Iterator<Item = &'a str>) -> String {
    let mut groups = vec![String::new(); CHANGELOG_GROUPS.len()];
    for subject in subjects {
        let Some((kind, change)) = subject.split_once(": ") else {
            continue;
        };
        let kind = kind.split(['(', '!']).next().unwrap_or(kind);
        if let Some(group) = CHANGELOG_GROUPS
            .iter()
            .position(|(kinds, _)| kinds.contains(&kind))
        {
            groups[group].push_str(&format!("- {change}\n"));
        }
    }
    if groups.iter().all(String::is_empty) {
        return String::new();
    }
    let mut section = format!("\n## {title}\n");
    for ((_, name), lines) in CHANGELOG_GROUPS.iter().zip(&groups) {
        if !lines.is_empty() {
            section.push_str(&format!("\n### {name}\n\n{lines}"));
        }
    }
    section
}

fn today() -> Result<String> {
    let output = Command::new("date")
        .arg("+%F")
        .output()
        .map_err(|error| format!("date: {error}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

struct Version {
    /// The tag's, without its `v`, and how far past it when not on it, e.g.
    /// `0.1.0-alpha.1-5-gabc1234`; the workspace's before the first tag.
    version: String,
    /// The number of commits, growing with every one.
    build: String,
    /// The commit's short hash, marked when the tree has changes on top of it.
    commit: String,
}

impl Version {
    fn read(root: &Path) -> Result<Self> {
        // A source archive (Homebrew's) has no history: its packager says what it is.
        if let Ok(version) = env::var("REMACS_VERSION") {
            return Ok(Self {
                version,
                build: "0".to_owned(),
                commit: env::var("REMACS_COMMIT").unwrap_or_else(|_| "unknown".to_owned()),
            });
        }
        let build = git(root, &["rev-list", "--count", "HEAD"])?;
        let hash = git(root, &["rev-parse", "--short", "HEAD"])?;
        let dirty = !git(root, &["status", "--porcelain"])?.is_empty();
        // Fails only when no tag is reachable.
        let tag = git(root, &["describe", "--tags", "--match", "v[0-9]*"]).ok();
        Ok(Self {
            version: tag.map_or_else(
                || env!("CARGO_PKG_VERSION").to_owned(),
                |tag| tag[1..].to_owned(),
            ),
            build,
            commit: if dirty { format!("{hash}-dirty") } else { hash },
        })
    }

    fn fill(&self, plist: &str) -> String {
        plist
            .replace("{version}", &self.version)
            .replace("{build}", &self.build)
            .replace("{commit}", &self.commit)
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `plugins/languages/grammars.toml`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Grammars {
    #[allow(dead_code, reason = "named by the release, read by CI")]
    release: String,
    grammars: Vec<Grammar>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Grammar {
    name: String,
    repository: String,
    revision: String,
}

/// `tree-sitter-<name>.wasm` and `.LICENSE` in `out` for each grammar, from its sources at
/// its revision; their queries are left out, unread (ADR 0081).
fn grammars(out: &Path) -> Result {
    let list: Grammars = toml::from_str(&read(&root().join("plugins/languages/grammars.toml"))?)
        .map_err(|error| format!("grammars.toml: {error}"))?;
    create(out)?;
    let out = out
        .canonicalize()
        .map_err(|error| format!("{}: {error}", out.display()))?;
    let sources = root().join("target/grammar-sources");
    for grammar in &list.grammars {
        let source = sources.join(&grammar.name);
        remove(&source)?;
        create(&source)?;
        let archive = source.with_extension("tar.gz");
        run(Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--output",
            ])
            .arg(&archive)
            .arg(format!(
                "https://codeload.github.com/{}/tar.gz/{}",
                grammar.repository, grammar.revision
            )))?;
        run(Command::new("tar")
            .args(["-xzf"])
            .arg(&archive)
            .args(["--strip-components=1", "--exclude=*/queries", "-C"])
            .arg(&source))?;
        let file = format!("tree-sitter-{}", grammar.name);
        run(Command::new("npx")
            .current_dir(&source)
            .args(["--yes", TREE_SITTER_CLI, "build", "--wasm", "--output"])
            .arg(out.join(format!("{file}.wasm")))
            .arg("."))?;
        fs::copy(source.join("LICENSE"), out.join(format!("{file}.LICENSE")))
            .map_err(|error| format!("{}'s license: {error}", grammar.name))?;
        eprintln!("built {file}.wasm");
    }
    Ok(())
}

/// Shows what the tool says on stderr (cargo's progress, anyone's errors), not its chatter.
fn run(command: &mut Command) -> Result {
    let program = command.get_program().display().to_string();
    let status = command
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("{program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed ({status})"))
    }
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn write(path: &Path, contents: &str) -> Result {
    fs::write(path, contents).map_err(|error| format!("{}: {error}", path.display()))
}

fn create(path: &Path) -> Result {
    fs::create_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Keeps the file's permissions, so scripts stay executable.
fn copy(from: &Path, to: &Path) -> Result {
    fs::copy(from, to)
        .map(drop)
        .map_err(|error| format!("{} → {}: {error}", from.display(), to.display()))
}

fn rename(from: &Path, to: &Path) -> Result {
    fs::rename(from, to).map_err(|error| format!("{} → {}: {error}", from.display(), to.display()))
}

fn remove(path: &Path) -> Result {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {error}", path.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::changelog_section;

    #[test]
    fn a_changelog_section_groups_what_people_notice() {
        let subjects = [
            "fix(git): a conflict no longer panics (#78)",
            "chore: release v0.1.0-alpha.1",
            "feat!: dired creates files through the prompt",
            "Merge branch 'x'",
            "perf: faster index",
            "feature: avy",
        ];
        assert_eq!(
            changelog_section("0.1.0 — 2026-10-20", subjects.into_iter()),
            "\n## 0.1.0 — 2026-10-20\n\
             \n### Added\n\n- dired creates files through the prompt\n- avy\n\
             \n### Fixed\n\n- a conflict no longer panics (#78)\n\
             \n### Faster\n\n- faster index\n"
        );
        assert_eq!(changelog_section("Unreleased", ["ci: x"].into_iter()), "");
    }
}
