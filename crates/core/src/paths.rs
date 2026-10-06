use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn claude_dir() -> PathBuf {
    home().join(".claude")
}

pub fn app_support() -> PathBuf {
    home().join("Library/Application Support/Claude")
}

/// Join key for paths that may differ in case or trailing slashes.
/// Canonicalizes when the path exists (fixes case on APFS), lowercases otherwise.
pub fn norm_key(p: &Path) -> String {
    let s = match std::fs::canonicalize(p) {
        Ok(c) => c.to_string_lossy().into_owned(),
        Err(_) => p.to_string_lossy().into_owned(),
    };
    s.trim_end_matches('/').to_lowercase()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
