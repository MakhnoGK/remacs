use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, BufWriter};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The file a text was loaded from or saved to, as it looked at that moment.
pub(crate) struct DiskFile {
    pub path: PathBuf,
    stamp: Stamp,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    inode: u64,
    len: u64,
    modified: SystemTime,
}

impl Stamp {
    fn of(metadata: &Metadata) -> io::Result<Self> {
        Ok(Self {
            inode: metadata.ino(),
            len: metadata.len(),
            modified: metadata.modified()?,
        })
    }
}

impl DiskFile {
    pub fn opened(path: &Path, file: &File) -> io::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            stamp: Stamp::of(&file.metadata()?)?,
        })
    }

    pub fn changed(&self) -> io::Result<bool> {
        match fs::metadata(&self.path) {
            Ok(metadata) => Ok(Stamp::of(&metadata)? != self.stamp),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// Writes a temporary file and renames it over `path`: readers never see a partial
    /// file, and the old inode — possibly still mapped — is never truncated.
    pub fn write(
        path: &Path,
        contents: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
    ) -> io::Result<Self> {
        let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let temporary = temporary_path(&target);
        let written = write_and_rename(&target, &temporary, contents);
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        Ok(Self {
            path: path.to_path_buf(),
            stamp: written?,
        })
    }
}

/// Opens `path` for reading if it is a regular file, or a symlink to one; anything else
/// is refused, since a FIFO never ends opening or reading and a device such as
/// `/dev/zero` reports no length and never ends.
pub fn open_regular_file(path: &Path) -> io::Result<File> {
    let refused = || {
        let message = format!("{} is not a regular file", path.display());
        io::Error::new(io::ErrorKind::InvalidInput, message)
    };
    // Checked before opening too, since opening a device may fail or act, like a serial
    // port resetting what is attached to it.
    if !fs::metadata(path)?.is_file() {
        return Err(refused());
    }
    // Non-blocking in case the path became a FIFO since, so opening it returns at once.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(refused());
    }
    let descriptor = file.as_raw_fd();
    // SAFETY: `file` owns the descriptor and outlives both calls.
    let blocking = unsafe {
        let flags = libc::fcntl(descriptor, libc::F_GETFL);
        flags != -1 && libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK) != -1
    };
    if !blocking {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

fn write_and_rename(
    target: &Path,
    temporary: &Path,
    contents: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<Stamp> {
    let file = File::create(temporary)?;
    if let Ok(metadata) = fs::metadata(target) {
        file.set_permissions(metadata.permissions())?;
    }
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    contents(&mut writer)?;
    let file = writer
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    file.sync_all()?;
    fs::rename(temporary, target)?;
    Stamp::of(&file.metadata()?)
}

fn temporary_path(target: &Path) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.remacs-save"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_regular_file_opens_for_blocking_reads() {
        let path = std::env::temp_dir().join(format!("remacs-regular-{}", std::process::id()));
        fs::write(&path, "text").unwrap();
        let file = open_regular_file(&path).unwrap();
        // SAFETY: `file` owns the descriptor.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        assert_eq!(flags & libc::O_NONBLOCK, 0);
        fs::remove_file(&path).unwrap();
    }
}
