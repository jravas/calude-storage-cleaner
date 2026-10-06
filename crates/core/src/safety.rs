//! Conservative classification of a worktree. Pure function, table-tested.

use crate::model::{CautionReason, InUseReason, SafetyState, Unpushed};

#[derive(Debug, Default, Clone)]
pub struct SafetyInput {
    pub is_main: bool,
    pub live: Vec<(u32, String)>,
    pub locked: Option<String>,
    pub pooled: bool,
    pub dirty: u32,
    pub untracked: u32,
    pub unpushed: Unpushed,
    pub own_commits: u32,
    pub detached: bool,
    pub open_in_app: Option<String>,
    pub registered: bool,
    pub has_desktop_record: bool,
    pub orphan: bool,
    pub minutes_since_modified: i64,
    pub git_error: Option<String>,
}

pub const RECENT_MINUTES: i64 = 60;

pub fn classify(i: &SafetyInput) -> SafetyState {
    let mut in_use = Vec::new();
    if i.is_main {
        in_use.push(InUseReason::MainCheckout);
    }
    for (pid, session) in &i.live {
        in_use.push(InUseReason::LiveProcess {
            pid: *pid,
            session: session.clone(),
        });
    }
    if i.locked.is_some() {
        in_use.push(InUseReason::GitLocked {
            reason: i.locked.clone().filter(|s| !s.is_empty()),
        });
    }
    if !in_use.is_empty() {
        return SafetyState::InUse(in_use);
    }
    if i.pooled {
        return SafetyState::Pooled;
    }
    let mut c = Vec::new();
    if let Some(m) = &i.git_error {
        c.push(CautionReason::GitError { message: m.clone() });
    }
    if i.dirty > 0 {
        c.push(CautionReason::Dirty { files: i.dirty });
    }
    if i.untracked > 0 {
        c.push(CautionReason::Untracked { files: i.untracked });
    }
    match i.unpushed {
        Unpushed::Count(n) if n > 0 => c.push(CautionReason::Unpushed { commits: n }),
        Unpushed::NoUpstream if !i.detached && i.own_commits > 0 => {
            c.push(CautionReason::NoUpstreamWithCommits {
                commits: i.own_commits,
            })
        }
        _ => {}
    }
    if i.detached && i.own_commits > 0 {
        c.push(CautionReason::DetachedWithOwnCommits {
            commits: i.own_commits,
        });
    }
    if let Some(id) = &i.open_in_app {
        c.push(CautionReason::OpenInApp {
            desktop_id: id.clone(),
        });
    }
    if i.orphan {
        c.push(CautionReason::Orphan);
    } else if !i.registered && !i.has_desktop_record {
        c.push(CautionReason::Unregistered);
    }
    if i.minutes_since_modified >= 0 && i.minutes_since_modified < RECENT_MINUTES {
        c.push(CautionReason::ModifiedRecently {
            minutes: i.minutes_since_modified as u32,
        });
    }
    if c.is_empty() {
        SafetyState::Safe
    } else {
        SafetyState::Caution(c)
    }
}

pub fn prune_allowed(state: &SafetyState, is_main: bool) -> bool {
    // The main checkout is InUse for removal purposes but may still be pruned.
    is_main || !matches!(state, SafetyState::InUse(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SafetyInput {
        SafetyInput {
            registered: true,
            has_desktop_record: true,
            unpushed: Unpushed::Count(0),
            minutes_since_modified: 10_000,
            ..Default::default()
        }
    }

    #[test]
    fn clean_registered_old_is_safe() {
        assert_eq!(classify(&base()), SafetyState::Safe);
    }

    #[test]
    fn live_process_wins_over_everything() {
        let mut i = base();
        i.live = vec![(42, "s".into())];
        i.dirty = 5;
        assert!(matches!(classify(&i), SafetyState::InUse(_)));
    }

    #[test]
    fn main_is_in_use_but_prunable() {
        let mut i = base();
        i.is_main = true;
        let s = classify(&i);
        assert!(matches!(s, SafetyState::InUse(_)));
        assert!(prune_allowed(&s, true));
    }

    #[test]
    fn pooled_before_caution() {
        let mut i = base();
        i.pooled = true;
        i.dirty = 1;
        assert_eq!(classify(&i), SafetyState::Pooled);
    }

    #[test]
    fn each_caution_reason() {
        let mut i = base();
        i.dirty = 2;
        assert!(
            matches!(classify(&i), SafetyState::Caution(ref r) if r.contains(&CautionReason::Dirty { files: 2 }))
        );
        let mut i = base();
        i.unpushed = Unpushed::Count(7);
        assert!(
            matches!(classify(&i), SafetyState::Caution(ref r) if r.contains(&CautionReason::Unpushed { commits: 7 }))
        );
        let mut i = base();
        i.unpushed = Unpushed::NoUpstream;
        i.own_commits = 3;
        assert!(
            matches!(classify(&i), SafetyState::Caution(ref r) if r.contains(&CautionReason::NoUpstreamWithCommits { commits: 3 }))
        );
        let mut i = base();
        i.unpushed = Unpushed::NoUpstream;
        i.detached = true;
        assert_eq!(classify(&i), SafetyState::Safe);
        i.own_commits = 1;
        assert!(
            matches!(classify(&i), SafetyState::Caution(ref r) if r.contains(&CautionReason::DetachedWithOwnCommits { commits: 1 }))
        );
        let mut i = base();
        i.registered = false;
        i.has_desktop_record = false;
        assert!(
            matches!(classify(&i), SafetyState::Caution(ref r) if r.contains(&CautionReason::Unregistered))
        );
        let mut i = base();
        i.minutes_since_modified = 5;
        assert!(
            matches!(classify(&i), SafetyState::Caution(ref r) if r.contains(&CautionReason::ModifiedRecently { minutes: 5 }))
        );
        let mut i = base();
        i.open_in_app = Some("local_x".into());
        assert!(matches!(classify(&i), SafetyState::Caution(_)));
    }
}
