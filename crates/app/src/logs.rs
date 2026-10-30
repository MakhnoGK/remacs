//! What goes wrong is kept: remacs's own output goes to `state/remacs.log` however it was
//! started, and a panic on any thread is written, with its backtrace, the version and the
//! commit, to `state/crashes/<time>.log`; the next start, or the window while it is still
//! open, says where.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::os::fd::AsRawFd;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use futures::channel::mpsc::UnboundedSender;
use remacs_config::Section;
use remacs_core::local_tm;

use crate::settings::settings_sections;
use crate::version::{COMMIT, VERSION};

const LOG: &str = "remacs.log";
const OLD_LOG: &str = "remacs.log.old";
const CRASHES: &str = "crashes";
/// Touched when the reports written so far have been told about.
const SHOWN: &str = ".shown";

/// Where the panic hook sends each report it writes, once the window is open.
static REPORTED: OnceLock<UnboundedSender<PathBuf>> = OnceLock::new();

settings_sections! {
    /// `logs.toml`.
    pub struct LogSettings {
        /// Bytes `remacs.log` may reach before the next start moves it to `remacs.log.old`.
        pub log_size: u64 = 1024 * 1024,
        /// Crash reports kept; each start removes the oldest past it.
        pub crash_reports: usize = 20,
    }
}

impl Section for LogSettings {
    const NAME: &str = "logs";

    fn check(&self) -> Result<(), String> {
        if self.crash_reports == 0 {
            return Err("crash-reports is 0".to_owned());
        }
        Ok(())
    }
}

/// Sends stdout and stderr to the log unless they are a terminal, and has every panic
/// write a report. Returns what to say of the reports written since the last start.
pub fn start(state: &Path, settings: &LogSettings) -> Option<String> {
    if let Err(error) = redirect_output(state, settings.log_size) {
        eprintln!("remacs: {}: {error}", state.join(LOG).display());
    }
    let crashes = state.join(CRASHES);
    let default_hook = panic::take_hook();
    let dir = crashes.clone();
    panic::set_hook(Box::new(move |info| {
        if let Some(report) = write_report(&dir, info)
            && let Some(reported) = REPORTED.get()
        {
            let _ = reported.unbounded_send(report);
        }
        // Prints the panic to stderr, the log.
        default_hook(info);
    }));
    let unseen = unseen_reports(&crashes);
    remove_old_reports(&crashes, settings.crash_reports);
    unseen
}

/// From now on, sends each report as soon as it is written, for a panic on another
/// thread, which leaves remacs running.
pub fn report_to(reported: UnboundedSender<PathBuf>) {
    let _ = REPORTED.set(reported);
}

/// What the echo area says of a report written while remacs runs.
pub fn reported(report: &Path) -> String {
    mark_shown(report.parent().unwrap_or(report));
    format!(
        "Part of remacs crashed and may not work until it starts again; report in {}",
        report.display()
    )
}

fn redirect_output(state: &Path, log_size: u64) -> io::Result<()> {
    if io::stderr().is_terminal() {
        return Ok(());
    }
    fs::create_dir_all(state)?;
    let log = state.join(LOG);
    // shortcut: only a start limits it, so one long session can grow it past the size.
    if fs::metadata(&log).is_ok_and(|metadata| metadata.len() > log_size) {
        fs::rename(&log, state.join(OLD_LOG))?;
    }
    let file = OpenOptions::new().create(true).append(true).open(&log)?;
    for output in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        // SAFETY: both descriptors are open; dup2 replaces `output` and leaves `file` open
        // until it is dropped.
        if unsafe { libc::dup2(file.as_raw_fd(), output) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Errors are dropped: the panic goes on to stderr all the same.
fn write_report(dir: &Path, info: &PanicHookInfo) -> Option<PathBuf> {
    let thread = std::thread::current();
    let report = format!(
        "remacs {VERSION} ({COMMIT})\nthread '{}' {info}\n\n{}\n",
        thread.name().unwrap_or("<unnamed>"),
        Backtrace::force_capture()
    );
    fs::create_dir_all(dir).ok()?;
    // Two panics in one second share a file.
    let path = dir.join(format!("{}.log", local_time(SystemTime::now())));
    let mut file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    file.write_all(report.as_bytes()).ok()?;
    Some(path)
}

fn unseen_reports(crashes: &Path) -> Option<String> {
    let shown = fs::metadata(crashes.join(SHOWN))
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let unseen = |(modified, path): (SystemTime, PathBuf)| (modified > shown).then_some(path);
    let unseen: Vec<PathBuf> = reports(crashes).into_iter().filter_map(unseen).collect();
    let newest = unseen.last()?;
    mark_shown(crashes);
    Some(match unseen.len() {
        1 => format!("remacs crashed last time; report in {}", newest.display()),
        count => format!(
            "remacs crashed {count} times since the last start; latest report in {}",
            newest.display()
        ),
    })
}

fn mark_shown(crashes: &Path) {
    if let Err(error) = fs::write(crashes.join(SHOWN), "") {
        eprintln!("remacs: {}: {error}", crashes.join(SHOWN).display());
    }
}

fn remove_old_reports(crashes: &Path, kept: usize) {
    let reports = reports(crashes);
    for (_, path) in &reports[..reports.len().saturating_sub(kept)] {
        if let Err(error) = fs::remove_file(path) {
            eprintln!("remacs: {}: {error}", path.display());
        }
    }
}

/// The reports and when each was last written, oldest first.
fn reports(crashes: &Path) -> Vec<(SystemTime, PathBuf)> {
    let paths = fs::read_dir(crashes).into_iter().flatten().flatten().map(|entry| entry.path());
    let logs = paths.filter(|path| path.extension().is_some_and(|extension| extension == "log"));
    let mut reports: Vec<_> =
        logs.filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path))).collect();
    reports.sort();
    reports
}

/// `2026-10-20-142903` in local time, which sorts as it goes.
fn local_time(at: SystemTime) -> String {
    let libc::tm { tm_year, tm_mon, tm_mday, tm_hour, tm_min, tm_sec, .. } = local_tm(at);
    format!("{}-{:02}-{tm_mday:02}-{tm_hour:02}{tm_min:02}{tm_sec:02}", tm_year + 1900, tm_mon + 1)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_config::assert_default_file::<LogSettings>();
    }

    #[test]
    fn reports_are_told_once_and_the_oldest_removed() {
        let crashes = std::env::temp_dir().join(format!("remacs-crashes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&crashes);
        fs::create_dir_all(&crashes).unwrap();
        assert_eq!(unseen_reports(&crashes), None);

        let past = SystemTime::now() - Duration::from_secs(60);
        for (name, age) in [("a.log", 3), ("b.log", 2), ("c.log", 1)] {
            let path = crashes.join(name);
            fs::write(&path, "").unwrap();
            let file = fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(past - Duration::from_secs(age)).unwrap();
        }
        let told = unseen_reports(&crashes).unwrap();
        assert!(told.contains("3 times") && told.ends_with("c.log"), "{told}");
        assert_eq!(unseen_reports(&crashes), None);

        remove_old_reports(&crashes, 2);
        let left: Vec<_> = reports(&crashes).into_iter().map(|(_, path)| path).collect();
        assert_eq!(left, [crashes.join("b.log"), crashes.join("c.log")]);
        fs::remove_dir_all(&crashes).unwrap();
    }

    #[test]
    fn local_time_sorts_as_it_goes() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000);
        let earlier = local_time(at);
        assert_eq!(earlier.len(), "2026-10-20-142903".len());
        assert!(earlier < local_time(at + Duration::from_secs(1)));
    }
}
