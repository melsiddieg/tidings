//! Files the user saves: OPML and starred article exports, and backups.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Creates an empty file next to `path` that only this user can read, under a name
/// no other file has. The caller writes it and renames it to `path`.
///
/// Opening `path` itself would follow a symbolic link there and overwrite the file
/// it points to; a rename replaces the link. An existing file at `path` so also
/// gets the private mode, and is never left half written.
pub(crate) fn private_temp(path: &Path) -> Result<(PathBuf, File)> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| Error::msg("This is not a file name"))?.to_string_lossy();
    let mut options = std::fs::OpenOptions::new();
    // `create_new` fails on any existing name, also a symbolic link.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let mut attempt = 0;
    loop {
        let temp = dir.join(format!(".{name}.{}-{nanos}-{attempt}.tmp", std::process::id()));
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => attempt += 1,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Writes a file only this user can read. An OPML export lists every feed address,
/// and an address can hold a password.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let (temp, mut file) = private_temp(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all()).and_then(|()| std::fs::rename(&temp, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}
