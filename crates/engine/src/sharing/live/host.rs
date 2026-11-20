use super::types::{Bind, HostOptions, Share, Shared, SharedLink, Status, Via};
use super::{Access, EngineError, IpAddr, Live, Result, Session, SessionId, Value, bad, code, display_name, get, string};

pub(super) fn unavailable(cmd: &str) -> EngineError {
    EngineError::Disabled(
        cmd.to_string(),
        "live sessions aren't available here (the desktop app and the MCP server host and join them; the web build joins)".into(),
    )
}

/// `collab.host`: host the open file (on this machine unless `p` says where) with an invite link.
pub(super) fn host(s: &mut Session, p: &Value) -> Result<Value> {
    start_host(s, "collab.host", p, Bind::Local, false, false)?;
    Ok(s.live_summary())
}

/// Start hosting the open file for command `cmd` (on `bind` unless `p` says where; `short`: with
/// a secret a join code can carry; `view`: with a second secret for looking only): the new
/// session.
fn start_host(s: &mut Session, cmd: &str, p: &Value, bind: Bind, short: bool, view: bool) -> Result<SessionId> {
    let port = match get(p, "port") {
        None => None,
        Some(v) => Some(v.as_u64().and_then(|n| u16::try_from(n).ok()).ok_or_else(|| bad(cmd, "`port` must be a whole number from 0 to 65535"))?),
    };
    let bind = match string(p, "bind").as_deref().map(str::trim) {
        None => bind,
        Some("local") => Bind::Local,
        Some("lan") => Bind::Lan,
        Some(other) => other.parse::<IpAddr>().map(Bind::Addr).map_err(|_| bad(cmd, "`bind` must be \"local\", \"lan\" or an IP address"))?,
    };
    let (doc, uid) = s.ds().map(|ds| (ds.doc.clone(), ds.uid))?;
    let opts = HostOptions { port, bind, name: display_name(&s.prefs), doc, short, view };
    let (id, info) = s.live.start(cmd, |l, id| l.host(id, opts).map_err(|e| EngineError::Other(format!("couldn't start the live session: {e}"))))?;
    s.live.open(Live { id, status: Status::Live, info: Some(info), doc_uid: Some(uid), tab: Some(uid), ..Live::default() });
    Ok(id)
}

/// Why a local network share has no comment code.
const LOCAL_COMMENTS: &str = "a file shared on the local network has an edit code and a view-only code, not a comment code";

/// Why a share has no code for the access asked for.
const NO_SUCH_CODE: &str = "this share has no code for that access: stop sharing and share again";

/// The `access` parameter of `cmd`, if given.
fn access_param(cmd: &str, p: &Value) -> Result<Option<Access>> {
    string(p, "access").map(|a| Access::parse(&a).ok_or_else(|| bad(cmd, "`access` must be \"edit\", \"comment\" or \"view\""))).transpose()
}

/// `collab.share`: share the open file with join codes on the local network, or (shared already)
/// switch which code shows.
pub(super) fn share(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.share";
    match string(p, "via").as_deref().map(str::trim) {
        None | Some("local") => {}
        Some(_) => return Err(bad(CMD, "`via` must be \"local\": files are shared from this app, on the local network")),
    }
    let access = access_param(CMD, p)?;
    if access == Some(Access::Comment) {
        return Err(bad(CMD, LOCAL_COMMENTS));
    }
    // A share of this file is there already: only which code is shown changes.
    if s.live().share.is_some() && s.live().status.active() {
        if let (Some(a), Some(sh)) = (access, s.live_mut().and_then(|l| l.share.as_mut())) {
            if sh.links.as_ref().is_some_and(|l| l.of(a).is_none()) {
                return Err(bad(CMD, NO_SUCH_CODE));
            }
            sh.access = a;
        }
        return Ok(s.live_summary());
    }
    let id = start_host(s, CMD, p, Bind::Lan, true, true)?;
    if let Some(live) = s.live.sessions.get_mut(&id) {
        // The session's own links are what others join with (its view link only lets them look),
        // and each code says what its link says: where the session is and that link's secret.
        let links =
            live.info.as_ref().map(|i| Shared { edit: local_link(&i.link), view: i.view.as_ref().map(|v| local_link(&v.link)), comment: None });
        live.share = Some(Share { via: Via::Local, access: access.unwrap_or_default(), links });
    }
    Ok(s.live_summary())
}

/// A way into a session this app hosts: its invite link and the join code that stands for it
/// (none when the link can't be put in one: a host without an IPv4 address).
fn local_link(link: &str) -> SharedLink {
    SharedLink { link: link.to_string(), code: code::Local::from_link(link).map(|l| l.code()) }
}
