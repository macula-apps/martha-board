use super::host::{host, unavailable};
use super::types::{Role, SessionId, Status};
use super::{CommandSpec, EngineError, Live, Result, Session, Value, bad, display_name, get, json, num, req_string, spec};

fn join(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.join";
    let link = req_string(CMD, p, "link").map_err(|_| bad(CMD, "give the invite `link` you were given"))?;
    let id = start_join(s, CMD, link.trim())?;
    Ok(s.live.get(id).map_or(Value::Null, |l| s.summary(l)))
}

/// Start joining the session behind `link` for command `cmd`: the new session.
fn start_join(s: &mut Session, cmd: &str, link: &str) -> Result<SessionId> {
    let (name, tab) = (display_name(&s.prefs), s.tab());
    let (id, ()) = s.live.start(cmd, |l, id| l.join(id, link, &name).map_err(|e| bad(cmd, e)))?;
    // The session's file opens in a tab of its own; until then the join shows on this one.
    s.live.open(Live { id, status: Status::Connecting, tab, ..Live::default() });
    Ok(id)
}

fn leave(s: &mut Session, p: &Value) -> Result<Value> {
    // A join on its way is called off by its id, whichever tab is in front (the Join dialog's
    // Cancel); one that already ended is no error.
    let id = match get(p, "session") {
        Some(v) => Some(v.as_u64().ok_or_else(|| bad("collab.leave", "`session` must be a session id (`collab.peers` lists them)"))?),
        None => s.live.shown(s.tab()),
    };
    let ended = id.and_then(|id| s.live.end(id));
    if let Some(l) = ended.as_ref().filter(|l| l.syncing() && l.role() == Some(Role::Guest)) {
        // What was still on its way may be held back for what's before it (an undo waits for its edit).
        s.message = Some(if l.client().is_some_and(|c| c.pending() > 0) {
            "You left before your last changes were confirmed: others may not see them all, and your copy stays open here".into()
        } else {
            "You left the session; your copy stays open here".into()
        });
    }
    Ok(json!({"ended": ended.is_some_and(|l| l.status.active())}))
}

/// Back into the open file's session now: a reconnect that is waiting tries at once, and a session
/// that couldn't go on is joined again in its tab.
fn reconnect(s: &mut Session, _: &Value) -> Result<Value> {
    const CMD: &str = "collab.reconnect";
    let live = s.live();
    let (id, reconnecting, rejoinable) = (live.id, matches!(live.status, Status::Reconnecting { .. }), live.rejoinable());
    if reconnecting {
        if let Some(why) = crate::file_access::network_refused() {
            return Err(EngineError::Disabled(CMD.to_string(), why));
        }
        if let Some(net) = s.live.link() {
            net.retry(id);
        }
    } else if rejoinable {
        s.live_rejoin(CMD, id)?;
    } else {
        return Err(bad(CMD, "this file has no lost live session to reconnect"));
    }
    Ok(json!({"status": s.live().status.id()}))
}

fn follow(s: &mut Session, p: &Value) -> Result<Value> {
    match get(p, "client") {
        None => {
            s.live_unfollow();
        }
        Some(v) => {
            let id = v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad("collab.follow", "`client` must be a client id"))?;
            s.live_follow(id)?;
        }
    }
    Ok(json!({"follow": s.live().follow}))
}

/// Remove someone from the open file's session, which this app hosts: the hub drops them. They are
/// told the session is over for them, keep their copy, and can't join again from their address
/// while the session runs.
fn remove_person(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "collab.removePerson";
    let client =
        get(p, "client").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(CMD, "`client` must be a client id"))?;
    let live = s.live();
    if live.status != Status::Live {
        return Err(bad(CMD, "the open file isn't in a live session"));
    }
    let name =
        live.peers.get(&client).map(|p| p.name.clone()).ok_or_else(|| bad(CMD, format!("nobody with client id {client} is in the session")))?;
    if live.role() != Some(Role::Host) {
        return Err(bad(CMD, "only the person hosting the session can remove people"));
    }
    let id = live.id;
    let link = s.live.link.as_deref_mut().ok_or_else(|| unavailable(CMD))?;
    link.remove(id, client).map_err(EngineError::Other)?;
    s.message = Some(format!("{name} was removed from the session"));
    Ok(json!({"removed": client, "name": name}))
}

/// You may remove others from the open file's session: you host it.
pub fn can_remove_people(s: &Session) -> bool {
    let live = s.live();
    live.status == Status::Live && live.role() == Some(Role::Host)
}

fn cursor(s: &mut Session, p: &Value) -> Result<Value> {
    let at = match (num(p, "x"), num(p, "y")) {
        (Some(x), Some(y)) => Some((x, y)),
        (None, None) => None,
        _ => return Err(bad("collab.cursor", "give both `x` and `y` (page coordinates), or neither to hide the cursor")),
    };
    if let Some(l) = s.live_mut() {
        l.cursor = at;
    }
    s.live_sync();
    Ok(json!({"cursor": s.live().cursor.map(|(x, y)| [x, y])}))
}

fn cursor_chat(s: &mut Session, p: &Value) -> Result<Value> {
    let Some(v) = get(p, "text") else {
        // The app's chat box (programmatic calls never open dialogs).
        let open = s.signal_blocker().is_none();
        if open {
            s.request_ui("dialog.cursorChat", json!({}));
        }
        return Ok(json!({"open": open, "chat": s.live().chat}));
    };
    let text = v.as_str().ok_or_else(|| bad("collab.cursorChat", "`text` must be a string (empty closes the chat)"))?;
    Ok(json!({"chat": s.live_chat(text)?}))
}

fn spotlight(s: &mut Session, p: &Value) -> Result<Value> {
    let on = match get(p, "on") {
        None => !s.live().spotlighting,
        Some(v) => v.as_bool().ok_or_else(|| bad("collab.spotlight", "`on` must be true or false"))?,
    };
    s.live_spotlight(on)?;
    Ok(json!({"spotlighting": s.live().spotlighting}))
}

/// The live session commands (`collab.*`).
pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "collab.host",
            "Start live session",
            "{} → the open file's session: {session, link, url (invites: they carry the secret token), address, …}; sessions are per file, so other files' sessions go on (a session this file was in ends)",
            false,
            host
        ),
        spec!(
            "collab.stop",
            "Stop live session",
            "{} → {ended}: the open file's session ends (a session this app hosts stops for everyone; a joined one is left)",
            false,
            leave
        ),
        spec!(
            "collab.join",
            "Join live session",
            "{link (the invite line you were given, passed to the link as it is)} → the new session: {session, status: \"connecting\", …}; its file opens in a tab of its own (other files' sessions go on) and `collab.peers` tells when it's live (`sessions` lists it until then, and if it fails)",
            false,
            join
        ),
        spec!(
            "collab.reconnect",
            "Reconnect live session",
            "{} → {status}: for the open file's session when it is disconnected: a reconnect that is waiting tries now, and a session that couldn't go on (`failed`, the file open in its tab) is joined again there, what you changed since going on top (your whole version opening beside it when some of it can't). Refused when nothing was lost",
            false,
            reconnect
        ),
        spec!(
            "collab.leave",
            "Leave live session",
            "{session? (a session's id from `collab.peers`'s `sessions`, e.g. a join still on its way; default: the open file's session)} → {ended}: the synced file stays open as a local copy (also how a tab stops showing a session that couldn't go on)",
            false,
            leave
        ),
        spec!(
            "collab.peers",
            "People in this file",
            "{} → the open file's session (each open file has its own or none): {available, canHost (false in the browser: it can only join), session (its id; null: this file isn't in a session), status: off|connecting|live|reconnecting|failed, reason?, role, me, name, color, fileName, pages, link, url, address, network, access: edit|view|comment (view: a viewer's invite, edits are refused; comment: a server's comment link, edits other than to comment threads are refused), readOnly (the open file can't be edited now: view only, or comment only), follow, syncing, document (index of the synced open document), editing (the text layer you type in: {node, layer, caret, selection}), peers: [{id, name, color, editing (theirs, same shape, or null), presence: {cursor: [page, x, y], selection, viewport: [page, x, y, w, h], editing_text}}], sessions: [{session, document (index of its open document; null for a join on its way), status, reason, role, fileName, link, peers (how many others)}] (every session of the app)}",
            false,
            |s, _| Ok(s.live_summary())
        ),
        spec!("collab.follow", "Follow", "{client? (a peer's id; omit to stop following)} → {follow}", false, follow),
        spec!(
            "collab.removePerson",
            "Remove from session",
            "{client (a peer's id)} → {removed, name}: they are disconnected (they keep their copy) and can't join again from their address while the session runs; only the person hosting can do it",
            false,
            remove_person
        ),
        spec!(
            "collab.cursor",
            "Show cursor at",
            "{x?, y? (page coordinates on the current page; omit both to hide)} → {cursor} (agents; in the app the pointer drives it): your pointer in the open file's session (null when the file isn't in one)",
            false,
            cursor
        ),
        spec!(
            "collab.cursorChat",
            "Cursor chat",
            "{text? (shown next to your cursor for everyone, at most 100 characters; empty closes it; omit to open the chat box in the app)} → {chat} (or {open}); others see it fade after 5 s without changes",
            false,
            cursor_chat
        ),
        spec!(
            "collab.spotlight",
            "Spotlight me",
            "{on? (default: toggle)} → {spotlighting}: everyone in the session follows your view and page until they move theirs",
            false,
            spotlight
        ),
    ]
}
