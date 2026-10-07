//! Installing a tool on a background thread: its release looked up on GitHub, the asset
//! downloaded with curl, checked against the release's checksum and unpacked beside the
//! other versions, which it then replaces.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use remacs_core::Notifier;
use serde::Deserialize;

use crate::catalog::{Archive, Tool};

/// How often a download's progress is looked at.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const UNPACKING: &str = ".partial";

pub enum Event {
    /// The release to install was found.
    Found {
        version: String,
    },
    /// Percent of the asset downloaded.
    Progress(u8),
    Unpacking,
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
            let release = release(tool, None, &place.api)?;
            Ok(Event::Latest {
                version: tool.version_of(&release.tag_name),
            })
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
        Self {
            kind,
            events,
            cancelled,
        }
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

/// The versions of `tool` unpacked in `dir`, any order.
pub fn installed_versions(tool: &Tool, dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir.join(tool.name)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join(tool.program).is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.ends_with(UNPACKING))
        .collect()
}

fn install(
    tool: &'static Tool,
    version: Option<&str>,
    force: bool,
    place: &Place,
    report: &dyn Fn(Event),
    cancelled: &AtomicBool,
) -> Result<Event, String> {
    let release = release(tool, version, &place.api)?;
    let version = tool.version_of(&release.tag_name);
    let tool_dir = place.dir.join(tool.name);
    let target = tool_dir.join(&version);
    if !force && target.join(tool.program).is_file() {
        return Ok(Event::Installed { version });
    }
    let asset_name = tool.asset_name(&release.tag_name);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| format!("release {} has no {asset_name}", release.tag_name))?;
    report(Event::Found {
        version: version.clone(),
    });
    fs::create_dir_all(&tool_dir).map_err(|error| error.to_string())?;
    let download = tool_dir.join(format!("{version}.download"));
    let downloaded = download_with_progress(asset, &download, report, cancelled)
        .and_then(|()| check_digest(asset, &download))
        .and_then(|()| {
            report(Event::Unpacking);
            unpack(tool, &download, &target)
        });
    let _ = fs::remove_file(&download);
    downloaded?;
    remove_other_versions(&tool_dir, &version);
    Ok(Event::Installed { version })
}

/// `version`'s release of `tool`, else its latest.
fn release(tool: &Tool, version: Option<&str>, api: &str) -> Result<Release, String> {
    let which = match version {
        Some(version) => format!("tags/{}{version}", tool.tag_prefix),
        None => "latest".to_owned(),
    };
    let url = format!("{api}/repos/{}/releases/{which}", tool.repository);
    let output = Command::new("curl")
        .args(["-fsSL", "-H", "Accept: application/vnd.github+json"])
        .arg(&url)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not run curl: {error}"))?;
    if !output.status.success() {
        return Err(match version {
            Some(version) => format!("no release {version} of {}", tool.name),
            None => curl_error(&output.stderr),
        });
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

fn download_with_progress(
    asset: &Asset,
    to: &Path,
    report: &dyn Fn(Event),
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut curl = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(to)
        .arg(&asset.browser_download_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run curl: {error}"))?;
    let mut reported = None;
    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = curl.kill();
            let _ = curl.wait();
            return Err("cancelled".into());
        }
        if let Some(status) = curl.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        let size = fs::metadata(to).map_or(0, |metadata| metadata.len());
        let percent = (size * 100 / asset.size.max(1)).min(100) as u8;
        if reported != Some(percent) {
            report(Event::Progress(percent));
            reported = Some(percent);
        }
        thread::sleep(PROGRESS_INTERVAL);
    };
    if status.success() {
        return Ok(());
    }
    let output = curl.wait_with_output().map_err(|error| error.to_string())?;
    Err(format!("download failed: {}", curl_error(&output.stderr)))
}

fn check_digest(asset: &Asset, file: &Path) -> Result<(), String> {
    let Some(expected) = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
    else {
        return Ok(());
    };
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .map_err(|error| format!("could not run shasum: {error}"))?;
    let actual = String::from_utf8_lossy(&output.stdout);
    match actual.split_whitespace().next() {
        Some(actual) if actual.eq_ignore_ascii_case(expected) => Ok(()),
        _ => Err(format!("checksum mismatch for {}", asset.name)),
    }
}

/// Unpacks next to `target` first, so that a half-unpacked tool is never taken for one.
fn unpack(tool: &Tool, archive: &Path, target: &Path) -> Result<(), String> {
    let partial = PathBuf::from(format!("{}{UNPACKING}", target.display()));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|error| error.to_string())?;
    let mut command = match tool.archive {
        Archive::Zip => {
            let mut unzip = Command::new("unzip");
            unzip
                .args(["-q", "-o"])
                .arg(archive)
                .arg("-d")
                .arg(&partial);
            unzip
        }
        Archive::TarGz => {
            let mut tar = Command::new("tar");
            tar.arg("-xzf").arg(archive).arg("-C").arg(&partial);
            tar
        }
    };
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not unpack: {error}"))?;
    if !output.status.success() {
        let _ = fs::remove_dir_all(&partial);
        return Err(format!(
            "could not unpack: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let program = partial.join(tool.program);
    let metadata =
        fs::metadata(&program).map_err(|_| format!("the release has no {}", tool.program))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(&program, permissions).map_err(|error| error.to_string())?;
    let _ = fs::remove_dir_all(target);
    fs::rename(&partial, target).map_err(|error| error.to_string())
}

fn remove_other_versions(tool_dir: &Path, version: &str) {
    let Ok(entries) = fs::read_dir(tool_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != version && entry.path().is_dir() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
