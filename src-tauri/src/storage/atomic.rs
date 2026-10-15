//! Crash-safe file writes: write `<file>.tmp`, flush to disk, then rename over the target.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::StorageError;

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Atomically replaces `path` with `bytes`.
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| StorageError::io(parent, e))?;
    }
    let tmp = tmp_path(path);
    {
        let mut file = fs::File::create(&tmp).map_err(|e| StorageError::io(&tmp, e))?;
        file.write_all(bytes)
            .map_err(|e| StorageError::io(&tmp, e))?;
        file.sync_all().map_err(|e| StorageError::io(&tmp, e))?;
    }
    // On Windows `fs::rename` uses MoveFileEx with REPLACE_EXISTING.
    fs::rename(&tmp, path).map_err(|e| StorageError::io(path, e))
}

/// Serializes `value` as pretty JSON and writes it atomically.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes(path, &bytes)
}

/// Reads JSON from `path`. Returns `Ok(None)` if the file does not exist.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, StorageError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StorageError::io(path, e)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|source| StorageError::Parse {
            path: path.display().to_string(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_roundtrip_and_no_tmp_left() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("data.json");
        write_json(&path, &vec![1, 2, 3]).unwrap();
        write_json(&path, &vec![4, 5]).unwrap();
        let back: Option<Vec<i32>> = read_json(&path).unwrap();
        assert_eq!(back, Some(vec![4, 5]));
        assert!(!tmp_path(&path).exists());
    }

    #[test]
    fn missing_file_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let back: Option<Vec<i32>> = read_json(&dir.path().join("nope.json")).unwrap();
        assert!(back.is_none());
    }
}
