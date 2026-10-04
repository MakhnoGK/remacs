use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::Path;

/// A directory with everything in it; a link, not what it points to.
pub fn remove(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// `from` to `to`: renamed on the same volume, else copied and then removed.
pub fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_path(from, to)?;
            remove(from)
        }
        result => result,
    }
}

/// `from` to `to` on one volume, failing rather than replacing what is at `to`.
pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from_c, to_c) = (path_c_string(from)?, path_c_string(to)?);
    // SAFETY: both are NUL-terminated strings that outlive the call.
    let result = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // Some network volumes cannot rename exclusively.
    if error.raw_os_error() != Some(libc::ENOTSUP) {
        return Err(error);
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(from, to)
}

fn path_c_string(path: &Path) -> io::Result<CString> {
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

/// `from` to `to`, a directory with everything in it, links as links; files keep their
/// permissions and modified times, like Emacs `dired-copy-preserve-time`.
pub fn copy_path(from: &Path, to: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(from)?;
    if metadata.is_symlink() {
        symlink(fs::read_link(from)?, to)
    } else if metadata.is_dir() {
        fs::create_dir(to)?;
        for item in fs::read_dir(from)? {
            let item = item?;
            copy_path(&item.path(), &to.join(item.file_name()))?;
        }
        fs::set_permissions(to, metadata.permissions())
    } else {
        fs::copy(from, to)?;
        File::options()
            .write(true)
            .open(to)?
            .set_modified(metadata.modified()?)
    }
}
