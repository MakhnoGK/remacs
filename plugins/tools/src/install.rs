//! Installing a tool on a background thread. A GitHub release's asset is downloaded with
//! curl, checked against the release's checksum and unpacked beside the other versions
//! first; a package is installed by its installer straight into its version's directory,
//! marked as unfinished until it is done. Either then replaces the other versions.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use remacs_core::Notifier;
use serde::Deserialize;

use crate::catalog::{Archive, Source, Tool, fill};
use crate::installers::{Installer, Vars};

/// How often a running download or install is looked at.
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const UNPACKING: &str = ".partial";
/// In a version's directory while its installer runs.
const UNFINISHED: &str = ".installing";
/// Releases looked through for a tool's latest, in a repository that releases several.
const RELEASES_PER_PAGE: usize = 100;
/// Prints a file's SHA-256 first; Arch keeps Perl's `shasum` off `PATH`.
pub(crate) const SHA256: (&str, &[&str]) =
    if cfg!(target_os = "macos") { ("shasum", &["-a", "256"]) } else { ("sha256sum", &[]) };

pub enum Event {
    /// The release to install was found.
    Found {
        version: String,
    },
    /// Percent of the asset downloaded.
    Progress(u8),
    Installing,
    Installed {
        version: String,
    },
    /// The latest release, when only that was asked for.
    Latest {
        version: String,
    },
    Cancelled,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Install,
    /// Looks up the latest release, nothing more.
    Check,
}

/// Where a job looks for releases and puts what it installs.
#[derive(Clone)]
pub struct Place {
    /// GitHub's REST API.
    pub api: String,
    /// One directory per tool, one per version in each.
    pub dir: PathBuf,
    pub installers: BTreeMap<String, Installer>,
}

pub struct Job {
    pub kind: JobKind,
    events: Receiver<Event>,
    cancelled: Arc<AtomicBool>,
}

impl Job {
    /// Installs `version` of `tool`, else its latest release; one already installed is
    /// kept unless `force`.
    pub fn install(
        tool: &'static Tool,
        version: Option<String>,
        force: bool,
        place: Place,
        notifier: Notifier,
    ) -> Self {
        Self::spawn(JobKind::Install, notifier, move |send, cancelled| {
            install(tool, version.as_deref(), force, &place, send, cancelled)
        })
    }

    pub fn check(tool: &'static Tool, place: Place, notifier: Notifier) -> Self {
        Self::spawn(JobKind::Check, notifier, move |_, _| {
            Ok(Event::Latest { version: find_version(tool, None, &place)? })
        })
    }

    fn spawn(
        kind: JobKind,
        notifier: Notifier,
        work: impl FnOnce(&dyn Fn(Event), &AtomicBool) -> Result<Event, String> + Send + 'static,
    ) -> Self {
        let (send, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&cancelled);
        thread::Builder::new()
            .name("tool-install".into())
            .spawn(move || {
                let report = |event: Event| {
                    let _ = send.send(event);
                    notifier();
                };
                let last = work(&report, &shared).unwrap_or_else(|error| {
                    if shared.load(Ordering::Relaxed) {
                        Event::Cancelled
                    } else {
                        Event::Failed(error)
                    }
                });
                report(last);
            })
            .expect("spawning a tool thread");
        Self { kind, events, cancelled }
    }

    pub fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    /// `sha256:<hex>`; older releases have none.
    digest: Option<String>,
    browser_download_url: String,
}

impl Place {
    fn installer(&self, tool: &Tool) -> Result<Option<&Installer>, String> {
        match tool.source {
            Source::GitHub { .. } => Ok(None),
            Source::Package { installer, .. } => self
                .installers
                .get(installer)
                .map(Some)
                .ok_or_else(|| format!("no installer is called {installer}")),
        }
    }
}

/// The program of `tool`'s `version`, relative to the tools' directory.
pub fn program_path(tool: &Tool, version: &str, installer: Option<&Installer>) -> PathBuf {
    let program = match installer {
        None => tool.fill(tool.program).replace("{version}", version),
        Some(installer) => fill(&installer.program).replace("{program}", tool.program),
    };
    [tool.home().as_str(), version, &program].iter().collect()
}

/// The versions of `tool` installed in `dir`, any order.
pub fn installed_versions(tool: &Tool, dir: &Path, installer: Option<&Installer>) -> Vec<String> {
    let home = dir.join(tool.home());
    let Ok(entries) = fs::read_dir(&home) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|version| !version.ends_with(UNPACKING))
        .filter(|version| !home.join(version).join(UNFINISHED).exists())
        .filter(|version| dir.join(program_path(tool, version, installer)).is_file())
        .collect()
}

/// The environment `tool`'s program runs with, installed at `version_dir`.
pub fn run_env(tool: &Tool, version_dir: &Path, installer: &Installer) -> Vec<(String, String)> {
    let Source::Package { package, .. } = tool.source else {
        return Vec::new();
    };
    installer.run_env(&Vars { dir: version_dir, package, version: "", program: tool.program })
}

fn install(
    tool: &'static Tool,
    version: Option<&str>,
    force: bool,
    place: &Place,
    report: &dyn Fn(Event),
    cancelled: &AtomicBool,
) -> Result<Event, String> {
    let installer = place.installer(tool)?;
    let release = match tool.source {
        Source::GitHub { .. } => Some(release(tool, version, &place.api)?),
        Source::Package { .. } => None,
    };
    let version = match (&release, version) {
        (Some(release), _) => tool.version_of(&release.tag_name),
        (None, Some(version)) => version.to_owned(),
        (None, None) => find_version(tool, None, place)?,
    };
    let program = place.dir.join(program_path(tool, &version, installer));
    let home = place.dir.join(tool.home());
    let target = home.join(&version);
    if !force && program.is_file() && !target.join(UNFINISHED).exists() {
        return Ok(Event::Installed { version });
    }
    report(Event::Found { version: version.clone() });
    fs::create_dir_all(&home).map_err(|error| error.to_string())?;
    match (release, installer, tool.source) {
        (Some(release), _, _) => {
            install_release(tool, &release, &version, &target, report, cancelled)?
        }
        (None, Some(installer), Source::Package { package, with, .. }) => {
            report(Event::Installing);
            let _ = fs::remove_dir_all(&target);
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            fs::write(target.join(UNFINISHED), "").map_err(|error| error.to_string())?;
            let vars = Vars { dir: &target, package, version: &version, program: tool.program };
            let installed = installer.install(&vars, with, cancelled).and_then(|()| {
                if program.is_file() {
                    Ok(())
                } else {
                    Err(format!("{package} {version} has no {}", tool.program))
                }
            });
            if installed.is_err() {
                let _ = fs::remove_dir_all(&target);
            }
            installed?;
            let _ = fs::remove_file(target.join(UNFINISHED));
        }
        _ => unreachable!("a tool comes from GitHub or from an installer"),
    }
    remove_other_versions(&home, &version);
    Ok(Event::Installed { version })
}

/// Downloads and unpacks `release` beside `target` first, so that a half-unpacked tool
/// is never taken for one.
fn install_release(
    tool: &Tool,
    release: &Release,
    version: &str,
    target: &Path,
    report: &dyn Fn(Event),
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let partial = PathBuf::from(format!("{}{UNPACKING}", target.display()));
    let download = PathBuf::from(format!("{}.download", target.display()));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|error| error.to_string())?;
    let program = tool.fill(tool.program).replace("{version}", version);
    let asset_name = tool.asset_name(&release.tag_name);
    let installed = download_asset(release, &asset_name, &download, report, cancelled)
        .and_then(|()| {
            report(Event::Installing);
            unpack(Archive::of(&asset_name), &download, &partial, &program)
        })
        .and_then(|()| make_executable(&partial.join(&program), tool))
        .and_then(|()| {
            let _ = fs::remove_dir_all(target);
            fs::rename(&partial, target).map_err(|error| error.to_string())
        });
    let _ = fs::remove_file(&download);
    if installed.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    installed
}

/// `version` if asked for, else the latest.
fn find_version(tool: &Tool, version: Option<&str>, place: &Place) -> Result<String, String> {
    match (tool.source, place.installer(tool)?) {
        (Source::Package { package, .. }, Some(installer)) => match version {
            Some(version) => Ok(version.to_owned()),
            None => installer.latest(&Vars {
                dir: &place.dir,
                package,
                version: "",
                program: tool.program,
            }),
        },
        _ => {
            let release = release(tool, version, &place.api)?;
            Ok(tool.version_of(&release.tag_name))
        }
    }
}

/// `version`'s release of `tool`, else its latest.
fn release(tool: &Tool, version: Option<&str>, api: &str) -> Result<Release, String> {
    let Source::GitHub { repository, tag_prefix, .. } = tool.source else {
        unreachable!("only GitHub tools have releases");
    };
    let releases = format!("{api}/repos/{repository}/releases");
    if let Some(version) = version {
        let url = format!("{releases}/tags/{}", tool.tag_of(version));
        return get_json(&url).map_err(|_| format!("no release {version} of {}", tool.name));
    }
    let latest: Release = get_json(&format!("{releases}/latest"))?;
    if latest.tag_name.starts_with(tag_prefix) {
        return Ok(latest);
    }
    let all: Vec<Release> = get_json(&format!("{releases}?per_page={RELEASES_PER_PAGE}"))?;
    all.into_iter()
        .find(|release| {
            !release.draft && !release.prerelease && release.tag_name.starts_with(tag_prefix)
        })
        .ok_or_else(|| format!("no release of {} in {repository}", tool.name))
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "-H", "Accept: application/vnd.github+json"])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not run curl: {error}"))?;
    if !output.status.success() {
        return Err(curl_error(&output.stderr));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("unexpected answer from GitHub: {error}"))
}

fn curl_error(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr);
    let message = message.trim();
    message
        .strip_prefix("curl: ")
        .and_then(|message| message.split_once(") ").map(|(_, rest)| rest))
        .unwrap_or(message)
        .to_owned()
}

fn download_asset(
    release: &Release,
    name: &str,
    to: &Path,
    report: &dyn Fn(Event),
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| format!("release {} has no {name}", release.tag_name))?;
    let curl = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(to)
        .arg(&asset.browser_download_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run curl: {error}"))?;
    let mut reported = None;
    let output = run(curl, cancelled, || {
        let size = fs::metadata(to).map_or(0, |metadata| metadata.len());
        let percent = (size * 100 / asset.size.max(1)).min(100) as u8;
        if reported != Some(percent) {
            report(Event::Progress(percent));
            reported = Some(percent);
        }
    })?;
    if !output.status.success() {
        return Err(format!("download failed: {}", curl_error(&output.stderr)));
    }
    check_digest(asset, to)
}

/// Waits for `child`, calling `tick` meanwhile; kills it once `cancelled`.
/// Only its stderr is collected.
pub fn run(
    mut child: Child,
    cancelled: &AtomicBool,
    mut tick: impl FnMut(),
) -> Result<Output, String> {
    // Drained while it runs: a child blocks once it fills the pipe nobody reads.
    let stderr = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            let stderr = stderr.map_or_else(Vec::new, |reader| reader.join().unwrap_or_default());
            return Ok(Output { status, stdout: Vec::new(), stderr });
        }
        tick();
        thread::sleep(POLL_INTERVAL);
    }
}

fn check_digest(asset: &Asset, file: &Path) -> Result<(), String> {
    let Some(expected) = asset.digest.as_deref().and_then(|digest| digest.strip_prefix("sha256:"))
    else {
        return Ok(());
    };
    let (program, args) = SHA256;
    let output = Command::new(program)
        .args(args)
        .arg(file)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    let actual = String::from_utf8_lossy(&output.stdout);
    match actual.split_whitespace().next() {
        Some(actual) if actual.eq_ignore_ascii_case(expected) => Ok(()),
        _ => Err(format!("checksum mismatch for {}", asset.name)),
    }
}

/// Unpacks `archive` into `into`, where a lone program becomes `program`.
fn unpack(kind: Archive, archive: &Path, into: &Path, program: &str) -> Result<(), String> {
    let tar = |flags: &str| {
        Command::new("tar")
            .arg(flags)
            .arg(archive)
            .arg("-C")
            .arg(into)
            .stdin(Stdio::null())
            .output()
    };
    let lone_program = || {
        let path = into.join(program);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        File::create(path)
    };
    let output = match kind {
        Archive::Zip => Command::new("unzip")
            .args(["-q", "-o"])
            .arg(archive)
            .arg("-d")
            .arg(into)
            .stdin(Stdio::null())
            .output(),
        Archive::TarGz => tar("-xzf"),
        Archive::TarXz => tar("-xJf"),
        Archive::Gzip => {
            let file = lone_program().map_err(|error| error.to_string())?;
            Command::new("gunzip").arg("-c").arg(archive).stdin(Stdio::null()).stdout(file).output()
        }
        Archive::Program => {
            drop(lone_program().map_err(|error| error.to_string())?);
            return fs::copy(archive, into.join(program))
                .map(drop)
                .map_err(|error| format!("could not copy the program: {error}"));
        }
    }
    .map_err(|error| format!("could not unpack: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!("could not unpack: {}", String::from_utf8_lossy(&output.stderr).trim()))
}

fn make_executable(program: &Path, tool: &Tool) -> Result<(), String> {
    let metadata =
        fs::metadata(program).map_err(|_| format!("the release has no {}", tool.program))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(program, permissions).map_err(|error| error.to_string())
}

fn remove_other_versions(home: &Path, version: &str) {
    let Ok(entries) = fs::read_dir(home) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != version && entry.path().is_dir() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
