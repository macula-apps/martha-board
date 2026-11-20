use super::types::{HostOptions, Status};
use super::{EngineError, Live, Result, Session, Value, display_name};

pub(super) fn unavailable(cmd: &str) -> EngineError {
    EngineError::Disabled(cmd.to_string(), "live sessions aren't available here (the desktop app hosts and joins them; the web build joins)".into())
}

/// `collab.host`: host the open file; others join with the session's invite link.
pub(super) fn host(s: &mut Session, _: &Value) -> Result<Value> {
    const CMD: &str = "collab.host";
    let (doc, uid) = s.ds().map(|ds| (ds.doc.clone(), ds.uid))?;
    let opts = HostOptions { name: display_name(&s.prefs), doc };
    let (id, info) = s.live.start(CMD, |l, id| l.host(id, opts).map_err(|e| EngineError::Other(format!("couldn't start the live session: {e}"))))?;
    s.live.open(Live { id, status: Status::Live, info: Some(info), doc_uid: Some(uid), tab: Some(uid), ..Live::default() });
    Ok(s.live_summary())
}
