//! The acts only a sealed share has (martha-board#2 design §5): inviting a named node, letting in
//! one waiting at the door, revoking a holder, and the share's numbers. The same names and params
//! as the other craft apps' (`collab.invite {role, ttlMs, forNode, forName}`), so an agent that
//! drives one drives this one.

use super::{CommandSpec, Result, Session, Value, bad, json, num, req_string, spec, string};

/// How long an invite is good for when the caller names no `ttlMs`: one day.
const INVITE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// The open file's share, for command `cmd`.
fn session_of(s: &Session, cmd: &str) -> Result<super::SessionId> {
    let live = s.live();
    if live.status == super::Status::Off {
        return Err(bad(cmd, "the open file isn't shared"));
    }
    Ok(live.id)
}

fn link_of<'a>(s: &'a mut Session, cmd: &str) -> Result<&'a mut (dyn super::LiveLink + 'static)> {
    s.live.link().ok_or_else(|| super::host::unavailable(cmd))
}

fn invite(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.invite";
    let role = match string(p, "role").as_deref().unwrap_or("editor") {
        r @ ("editor" | "viewer") => r.to_string(),
        other => return Err(bad(CMD, format!("unknown role `{other}` (editor or viewer)"))),
    };
    let node = req_string(CMD, p, "forNode").map_err(|_| bad(CMD, "a sealed share invites one named node: give `forNode`, its id"))?;
    let name = string(p, "forName").unwrap_or_default();
    let ttl_ms = match num(p, "ttlMs") {
        None => INVITE_TTL_MS,
        Some(ms) if ms.is_finite() && ms >= 1.0 && ms <= u64::MAX as f64 => ms as u64,
        Some(_) => return Err(bad(CMD, "`ttlMs` must be a positive number of milliseconds")),
    };
    let id = session_of(s, CMD)?;
    let line = link_of(s, CMD)?.invite(id, &node, &name, &role, ttl_ms).map_err(|e| bad(CMD, e))?;
    Ok(json!({"invite": line, "forNode": node, "role": role, "ttlMs": ttl_ms}))
}

fn approve(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.approve";
    let node = req_string(CMD, p, "node")?;
    let id = session_of(s, CMD)?;
    link_of(s, CMD)?.approve(id, &node).map_err(|e| bad(CMD, e))?;
    Ok(json!({"approved": node}))
}

fn revoke(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.revoke";
    let node = req_string(CMD, p, "node")?;
    let id = session_of(s, CMD)?;
    link_of(s, CMD)?.revoke(id, &node).map_err(|e| bad(CMD, e))?;
    Ok(json!({"revoked": node}))
}

fn stats(s: &mut Session) -> Result<Value> {
    const CMD: &str = "collab.stats";
    let id = session_of(s, CMD)?;
    link_of(s, CMD)?.stats(id).map_err(|e| bad(CMD, e))
}

/// The sealed share's commands.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "collab.invite",
            "Invite to board",
            "{forNode (the invited node's id, hex), forName? (the name shown for them), role?: \"editor\" (default) | \"viewer\", ttlMs? (default one day)} → {invite (the line to give them: it carries their credential), forNode, role, ttlMs}; only the host of the open file's share",
            false,
            invite
        ),
        spec!("collab.approve", "Let in", "{node} → {approved}: a node waiting at the door of the open file's share comes in; only the host", false, approve),
        spec!(
            "collab.revoke",
            "Revoke",
            "{node} → {revoked}: the node's grant is revoked and it leaves the open file's share (it keeps its copy); only the host",
            false,
            revoke
        ),
        spec!("collab.stats", "Share status", "{} → the open file's share: role, node, holders, pending, what it refused, the seam's numbers", false, |s, _| stats(s)),
    ]
}
