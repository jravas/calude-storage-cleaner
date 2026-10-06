use crate::model::Repo;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ScanEvent {
    Progress { phase: String, current: String },
    Repo { repo: Box<Repo> },
}

pub trait ProgressSink: Sync {
    fn on(&self, ev: ScanEvent);
}

pub struct NullSink;
impl ProgressSink for NullSink {
    fn on(&self, _ev: ScanEvent) {}
}
