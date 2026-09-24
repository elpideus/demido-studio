//! Small filesystem helpers used on both sides of the install.

use std::io::Write;
use std::path::Path;

use anyhow::Context;

/// Writes `bytes` to `path` atomically: a sibling temp file is written, flushed and renamed over
/// the target, so a crash never leaves a half-written file behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Serializes `value` as pretty JSON and writes it atomically.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// Reads and deserializes a JSON file.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/value.json");
        write_json(&path, &serde_json::json!({"a": 1})).unwrap();
        write_json(&path, &serde_json::json!({"a": 2})).unwrap();
        let back: serde_json::Value = read_json(&path).unwrap();
        assert_eq!(back["a"], 2);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }
}
