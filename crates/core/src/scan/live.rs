//! Live Claude Code processes, from ~/.claude/sessions/<pid>.json.

use crate::paths;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveSession {
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub started_at: i64,
    pub host_session_id: Option<String>,
    pub status: Option<String>,
    pub entrypoint: Option<String>,
    pub name: Option<String>,
    #[serde(skip)]
    pub alive: bool,
}

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if r == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn load() -> Vec<LiveSession> {
    let dir = paths::claude_dir().join("sessions");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        if let Ok(mut s) = serde_json::from_str::<LiveSession>(&text) {
            s.alive = pid_alive(s.pid);
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_pid_is_alive_and_bogus_pid_is_not() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(u32::MAX - 7));
    }
}
