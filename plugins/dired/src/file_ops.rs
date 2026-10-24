use std::ffi::{CString, OsString};
use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::Path;

use trash::TrashContext;

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

/// `from` in place of what is at `to`, which stays until its replacement is complete: a
/// move is one rename, a copy is made under a temporary name and renamed over it. A
/// directory on either side cannot be swapped in one step, so `discard` takes the old one
/// first.
pub fn replace(
    from: &Path,
    to: &Path,
    moving: bool,
    discard: impl Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if fs::symlink_metadata(from)?.is_dir() || fs::symlink_metadata(to)?.is_dir() {
        discard(to)?;
        return if moving {
            move_path(from, to)
        } else {
            copy_path(from, to)
        };
    }
    if moving {
        match fs::rename(from, to) {
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {}
            result => return result,
        }
    }
    let mut temporary_name = OsString::from(format!(".remacs-replace-{}-", std::process::id()));
    temporary_name.push(to.file_name().unwrap_or_default());
    let temporary = to.with_file_name(temporary_name);
    let copied = copy_path(from, &temporary).and_then(|()| fs::rename(&temporary, to));
    if copied.is_err() {
        let _ = remove(&temporary);
    }
    copied?;
    if moving { remove(from) } else { Ok(()) }
}

/// Moves `path` to the Trash.
pub fn trash(path: &Path) -> io::Result<()> {
    #[allow(unused_mut)]
    let mut trash = TrashContext::default();
    // Finder's way asks for permission to script it, and plays a sound.
    #[cfg(target_os = "macos")]
    trash::macos::TrashContextExtMacos::set_delete_method(
        &mut trash,
        trash::macos::DeleteMethod::NsFileManager,
    );
    trash.delete(path).map_err(io::Error::other)
}

/// `from` to `to` on one volume, failing rather than replacing what is at `to`.
pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from_c, to_c) = (path_c_string(from)?, path_c_string(to)?);
    // SAFETY: both are NUL-terminated strings that outlive the call.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    // SAFETY: both are NUL-terminated strings that outlive the call, relative to the
    // current directory as `rename` is.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from_c.as_ptr(),
            libc::AT_FDCWD,
            to_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // Some network volumes cannot rename exclusively; Linux file systems say so with EINVAL.
    if !matches!(error.raw_os_error(), Some(libc::ENOTSUP | libc::EINVAL)) {
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
