use std::cmp::Ordering;
use std::fs::{self, FileType, Metadata};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PARENT: &str = "..";

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One file of a listing, with its columns formatted when it is read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_directory: bool,
    /// Bytes, or items for a directory.
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub size_label: String,
    pub modified_label: String,
    pub permissions: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
}

impl SortKey {
    pub const ALL: [SortKey; 3] = [SortKey::Name, SortKey::Modified, SortKey::Size];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Modified => "Modified",
            SortKey::Size => "Size",
        }
    }

    pub fn next(self) -> Self {
        match self {
            SortKey::Name => SortKey::Modified,
            SortKey::Modified => SortKey::Size,
            SortKey::Size => SortKey::Name,
        }
    }
}

impl Entry {
    fn read(path: &Path, name: String, this_year: i32) -> io::Result<Self> {
        let link = fs::symlink_metadata(path)?;
        // A broken link is listed as itself.
        let target = fs::metadata(path).unwrap_or_else(|_| link.clone());
        let is_directory = target.is_dir();
        let size = if is_directory {
            fs::read_dir(path).map_or(0, |items| items.count() as u64)
        } else {
            target.len()
        };
        let size_label = match (is_directory, name == PARENT) {
            (_, true) => String::new(),
            (true, false) => items_label(size),
            (false, false) => size_label(size),
        };
        let modified = target.modified().ok();
        Ok(Self {
            modified_label: modified
                .and_then(local_time)
                .map(|time| date_label(time, this_year))
                .unwrap_or_default(),
            permissions: permissions_label(link.file_type(), &link),
            name,
            is_directory,
            size,
            modified,
            size_label,
        })
    }
}

/// Usually ignored: hidden, backup and auto-save files.
pub fn is_dim_name(name: &str) -> bool {
    (name.starts_with('.') && name != PARENT) || is_backup_name(name) || is_auto_save_name(name)
}

/// `name~`, as Emacs saves the previous version of a file.
pub fn is_backup_name(name: &str) -> bool {
    name.ends_with('~')
}

/// `#name#`, as Emacs auto-saves unsaved changes.
pub fn is_auto_save_name(name: &str) -> bool {
    name.len() > 1 && name.starts_with('#') && name.ends_with('#')
}

/// The entries of `directory`, its parent first, in no particular order after that.
pub fn read_directory(directory: &Path) -> io::Result<Vec<Entry>> {
    let this_year = local_time(SystemTime::now()).map_or(0, |now| now.year);
    let mut entries = Vec::new();
    if let Some(parent) = directory.parent() {
        entries.push(Entry::read(parent, PARENT.to_owned(), this_year)?);
    }
    for item in fs::read_dir(directory)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        // Gone since the directory was read.
        if let Ok(entry) = Entry::read(&item.path(), name, this_year) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// `..` first, then directories, then files; newest and biggest first, ties by name.
pub fn sort(entries: &mut [Entry], key: SortKey) {
    entries.sort_by(|a, b| {
        let group = |entry: &Entry| (entry.name != PARENT, !entry.is_directory);
        let by_key = match key {
            SortKey::Name => Ordering::Equal,
            SortKey::Modified => b.modified.cmp(&a.modified),
            SortKey::Size => b.size.cmp(&a.size),
        };
        group(a)
            .cmp(&group(b))
            .then(by_key)
            .then_with(|| compare_names(&a.name, &b.name))
    });
}

/// Case-folded, then exact.
pub fn compare_names(a: &str, b: &str) -> Ordering {
    folded(a).cmp(folded(b)).then_with(|| a.cmp(b))
}

fn folded(name: &str) -> impl Iterator<Item = char> {
    name.chars().flat_map(char::to_lowercase)
}

/// Decimal units, as Finder shows them.
pub fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 999.95 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value < 99.95 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

/// `buffer.rs`, `2 files`, `3 flagged folders` or `4 marked items`.
pub fn describe_entries(entries: &[Entry], adjective: &str) -> String {
    if let [single] = entries {
        return single.name.clone();
    }
    let directories = entries.iter().filter(|entry| entry.is_directory).count();
    let noun = match directories {
        0 => "file",
        count if count == entries.len() => "folder",
        _ => "item",
    };
    format!(
        "{} {adjective}{}",
        entries.len(),
        plural(entries.len(), noun)
    )
}

/// `name/` for a directory, as listings show it.
pub fn display_name(entry: &Entry) -> String {
    if entry.is_directory && entry.name != PARENT {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    }
}

pub fn items_label(count: u64) -> String {
    if count == 1 {
        "1 item".into()
    } else {
        format!("{count} items")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    /// 0 for January.
    pub month: usize,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
}

/// `Oct 4, 10:41` this year, `Oct 4, 2024` before, like `ls -l`.
pub fn date_label(time: LocalTime, this_year: i32) -> String {
    let LocalTime {
        year,
        month,
        day,
        hour,
        minute,
    } = time;
    let month = MONTHS[month.min(11)];
    if year == this_year {
        format!("{month} {day}, {hour:02}:{minute:02}")
    } else {
        format!("{month} {day}, {year}")
    }
}

/// `Today, 10:36`, `Yesterday, 18:20`, else the date and time, with the year when it is
/// not this year.
pub fn full_date_label(time: LocalTime, today: LocalTime, yesterday: LocalTime) -> String {
    let same_day = |other: LocalTime| {
        (time.year, time.month, time.day) == (other.year, other.month, other.day)
    };
    let clock = format!("{:02}:{:02}", time.hour, time.minute);
    let (month, day) = (MONTHS[time.month.min(11)], time.day);
    if same_day(today) {
        format!("Today, {clock}")
    } else if same_day(yesterday) {
        format!("Yesterday, {clock}")
    } else if time.year == today.year {
        format!("{month} {day}, {clock}")
    } else {
        format!("{month} {day}, {}, {clock}", time.year)
    }
}

/// [`full_date_label`] of `time`, as of now.
pub fn full_modified_label(time: Option<SystemTime>) -> String {
    let now = SystemTime::now();
    let labelled = || {
        let yesterday = local_time(now.checked_sub(Duration::from_secs(24 * 60 * 60))?)?;
        Some(full_date_label(
            local_time(time?)?,
            local_time(now)?,
            yesterday,
        ))
    };
    labelled().unwrap_or_default()
}

fn local_time(time: SystemTime) -> Option<LocalTime> {
    let seconds = libc::time_t::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let mut fields = MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers are valid for the call, and localtime_r keeps neither.
    let filled = unsafe { libc::localtime_r(&seconds, fields.as_mut_ptr()) };
    if filled.is_null() {
        return None;
    }
    // SAFETY: a non-null result means localtime_r filled every field.
    let fields = unsafe { fields.assume_init() };
    Some(LocalTime {
        year: fields.tm_year + 1900,
        month: usize::try_from(fields.tm_mon).ok()?,
        day: fields.tm_mday,
        hour: fields.tm_hour,
        minute: fields.tm_min,
    })
}

/// `drwxr-xr-x`, as `ls -l` shows it.
pub fn permissions_label(file_type: FileType, metadata: &Metadata) -> String {
    let kind = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else if file_type.is_fifo() {
        'p'
    } else if file_type.is_socket() {
        's'
    } else if file_type.is_char_device() {
        'c'
    } else if file_type.is_block_device() {
        'b'
    } else {
        '-'
    };
    format!("{kind}{}", mode_label(metadata.permissions().mode()))
}

/// The nine permission characters, with setuid, setgid and sticky bits folded in.
pub fn mode_label(mode: u32) -> String {
    let triple = |shift: u32, special: u32, set: char| {
        let bits = mode >> shift;
        let read = if bits & 0o4 != 0 { 'r' } else { '-' };
        let write = if bits & 0o2 != 0 { 'w' } else { '-' };
        let execute = match (bits & 0o1 != 0, mode & special != 0) {
            (true, true) => set,
            (false, true) => set.to_ascii_uppercase(),
            (true, false) => 'x',
            (false, false) => '-',
        };
        [read, write, execute]
    };
    [
        triple(6, 0o4000, 's'),
        triple(3, 0o2000, 's'),
        triple(0, 0o1000, 't'),
    ]
    .concat()
    .into_iter()
    .collect()
}
