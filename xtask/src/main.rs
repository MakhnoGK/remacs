//! `cargo xtask install` builds remacs, bundles it as `Remacs.app` and puts it in
//! `~/Applications`, keeping the app it replaces; `cargo xtask rollback` swaps them back.
//! The installed app is a snapshot: building the workspace afterwards does not touch it.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const APP: &str = "Remacs.app";
const PREVIOUS: &str = "Remacs.previous.app";
const EXECUTABLE: &str = "remacs";
const USAGE: &str = "usage: cargo xtask <bundle | install | rollback>
  bundle    build target/bundle/Remacs.app
  install   bundle, then install it in ~/Applications and `remacs` in ~/.local/bin
  rollback  swap the installed app with the one it replaced";

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
    let result = match env::args().nth(1).as_deref() {
        Some("bundle") => bundle().map(|app| println!("{}", app.display())),
        Some("install") => install(),
        Some("rollback") => rollback(),
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
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    run(Command::new(cargo).current_dir(&root).args([
        "build",
        "--release",
        "--package",
        "remacs-app",
    ]))?;

    let sources = root.join("crates/app/bundle");
    let out = root.join("target/bundle");
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
    let plist = read(&sources.join("Info.plist"))?;
    let version = Version::read(&root)?;
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
    write(&launcher, &launcher_script(&installed, &home))?;
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
/// terminal, so that closing its tab does not quit remacs.
fn launcher_script(app: &Path, home: &Path) -> String {
    let binary = app.join("Contents/MacOS").join(EXECUTABLE);
    let log = home.join(".config/remacs/state/remacs.log");
    format!(
        "#!/bin/sh\n\
         # Written by `cargo xtask install`.\n\
         mkdir -p '{dir}'\n\
         nohup '{binary}' \"$@\" </dev/null >>'{log}' 2>&1 &\n",
        dir = log.parent().expect("a file in a directory").display(),
        binary = binary.display(),
        log = log.display(),
    )
}

fn rollback() -> Result {
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

struct Version {
    /// The workspace's.
    version: String,
    /// The number of commits, growing with every one.
    build: String,
    /// The commit's short hash, marked when the tree has changes on top of it.
    commit: String,
}

impl Version {
    fn read(root: &Path) -> Result<Self> {
        let build = git(root, &["rev-list", "--count", "HEAD"])?;
        let hash = git(root, &["rev-parse", "--short", "HEAD"])?;
        let dirty = !git(root, &["status", "--porcelain"])?.is_empty();
        Ok(Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
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
