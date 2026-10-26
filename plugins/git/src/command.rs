use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

/// `git` run in `root`, never prompting and never taking locks it can do without, so that
/// reading the status does not get in the way of the user's own git commands.
pub fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        // Diffs are read and applied by their `a/` and `b/`, whatever the user's config says.
        .args([
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicPrefix=false",
        ])
        .args(["-c", "diff.srcPrefix=a/", "-c", "diff.dstPrefix=b/"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::null());
    command
}

/// What the command prints; its first line of errors when it fails.
pub fn output(command: &mut Command) -> io::Result<Vec<u8>> {
    finished(command.output()?)
}

/// Like [`output`], `read` taking what the command prints as it prints it, so that none
/// of it has to be kept whole.
pub fn read_output<T>(
    command: &mut Command,
    read: impl FnOnce(&mut dyn BufRead) -> T,
) -> io::Result<T> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // The reader is dropped before the wait, so a command printing past what `read` took
    // stops instead of blocking.
    let read = read(&mut BufReader::new(
        child.stdout.take().expect("stdout is piped"),
    ));
    finished(child.wait_with_output()?)?;
    Ok(read)
}

/// Like [`output`], with `input` written to the command.
pub fn output_with_input(command: &mut Command, input: &[u8]) -> io::Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input)?;
    finished(child.wait_with_output()?)
}

fn finished(output: std::process::Output) -> io::Result<Vec<u8>> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let errors = String::from_utf8_lossy(&output.stderr);
    let first = errors.lines().find(|line| !line.trim().is_empty());
    Err(io::Error::other(
        first.map_or_else(|| output.status.to_string(), str::to_owned),
    ))
}
