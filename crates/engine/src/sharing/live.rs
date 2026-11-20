//! Live sessions (milestone M9, step 2 of `docs/collab-protocol.md`): people edit the same file
//! together and see each other's cursors, selections and viewports, and can follow one another.
//!
//! The engine holds what everyone sees ([`Live`]: status, peers and their presence, who you
//! follow) and the `collab.*` commands. The networking lives in the app (a share over the mesh),
//! which installs a [`LiveLink`]: the engine never opens a
//! socket, so nothing below the UI layer depends on networking. Without a link (the CLI, tests)
//! the commands say live sessions aren't available.
//!
//! Peers, presence and document changes are the wire types of the collaboration core
//! ([`figcraft_collab::message`]). Each participant runs a core [`Client`]: the host's session
//! document is the one it shared, a guest gets the host's file as a new tab from the welcome.
//! Every command, gesture or group on the session document is synced as one batch and is one step
//! of the user's own undo history ([`Session::record`]); undo and redo there take back only the
//! user's own steps ([`figcraft_collab::Client::undo`]). Batches from the host's server are applied
//! through the client and installed with [`Session::apply_remote`], which never touches the undo
//! history. After a dropped connection the client resumes with its id and catches up; what it
//! made meanwhile goes out then.
//!
//! A view link (or a comment link) makes the synced document read-only here (every edit is refused
//! before it is sent; the host refuses it too).
//!
//! Sessions are per document ([`Lives`]): each open document has its own session or none, several
//! run at once (host one file, join another), and they keep syncing while their
//! tab isn't in front. The commands act on the active document's session ([`Session::live`]);
//! closing a document ends its session only. One link carries them all, each under its
//! [`SessionId`].
//!
//! `collab.host` shares the open file; `collab.join {link}` takes an invite line and hands it to
//! the link as it is.
//!
//! Cursor chat (`collab.cursorChat`) and spotlights (`collab.spotlight`) travel as signals:
//! short-lived presence the host passes on to everyone, never stored. A spotlight makes everyone
//! follow the person who started it; moving your own view stops following, as with any follow.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use figcraft_collab::Client;
pub use figcraft_collab::message::Signal;
use figcraft_collab::message::{ClientMsg, EditingText, ServerMsg};

pub use figcraft_collab::message::{Access, Resume};
use figcraft_doc::{Document, NodeId};
use serde::Serialize;
use serde_json::{Value, json};

pub use figcraft_collab::message::{Peer, Presence};

use crate::cmd::CommandSpec;
use crate::collab::EngineRecompute;
use crate::params::{get, num, req_string, string};
use crate::tools::ToolId;
use crate::{DocState, EngineError, Result, Session, TextEdit, ViewState, bad, spec};

mod events;
mod host;
mod join;
mod sealed;
mod sync;
mod types;

use host::unavailable;
pub use join::can_remove_people;

/// The live session commands (`collab.*`), the sealed share's among them.
pub fn specs() -> Vec<CommandSpec> {
    let mut all = join::specs();
    all.extend(sealed::specs());
    all
}
pub use types::{
    Event, HostOptions, LiveLink, MAX_NAME, MAX_PEERS, MAX_SELECTION, NO_SESSION, PEOPLE, PageInfo, Role, SessionId, SessionInfo, Status,
    TOO_LARGE_TO_JOIN, parse_color,
};

/// The view a follow last applied: any other view means the user moved.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FollowedView {
    page: NodeId,
    cx: f64,
    cy: f64,
    zoom: f64,
}

/// One live session (see the module docs): a document's, or a join on its way to its own tab.
#[derive(Default)]
pub struct Live {
    /// Its number on the link (0: no session, the state of a document that isn't in one).
    pub id: SessionId,
    pub status: Status,
    pub info: Option<SessionInfo>,
    /// Everyone else in the session, by client id.
    pub peers: BTreeMap<u32, Peer>,
    /// Their latest presence (only while they're in the session).
    pub presence: BTreeMap<u32, Presence>,
    /// The peer whose viewport you follow.
    pub follow: Option<u32>,
    followed: Option<FollowedView>,
    /// Your pointer over the canvas (page coordinates): the UI sets it, or `collab.cursor`.
    pub cursor: Option<(f64, f64)>,
    /// The presence last handed to the link.
    sent: Option<Presence>,
    /// The document sync client (from the welcome).
    client: Option<Client>,
    /// The open document the session syncs (its `DocState::uid`).
    pub doc_uid: Option<u64>,
    /// The document whose tab shows the session: `doc_uid`, or for a join the document it was
    /// started from, until the session's file opens in a tab of its own (`None`: none was open).
    pub tab: Option<u64>,
    /// The session document as last sent or installed: a different one has local edits to send.
    synced: Option<Arc<Document>>,
    /// Server batches waiting for a gesture or preview on the session document to end.
    deferred: Vec<ServerMsg>,
    /// Nesting of [`Session::grouped`] on the session document: its edits sync once, at the end.
    hold: u32,
    /// The host passes cursor chat and spotlights on (it said so after the welcome).
    pub signals: bool,
    /// Others' open cursor chat messages.
    pub chats: BTreeMap<u32, String>,
    /// Your cursor chat message (empty: none).
    pub chat: String,
    /// Who spotlights (you follow them until you move your view).
    pub spotlight: Option<u32>,
    /// You spotlight: everyone follows you.
    pub spotlighting: bool,
    /// The branches of the session's file made or last updated from main in this session: every
    /// layer of their base was in it (see [`crate::branches`]).
    branch_bases: BTreeSet<String>,
    /// You were in this session and aren't now: what joining it again takes (`collab.reconnect`).
    stranded: Option<Stranded>,
}

/// What's kept of a session you joined once it can't go on by itself. The tab stays yours to work
/// in, and says so.
#[derive(Clone)]
struct Stranded {
    /// The invite link you joined with (it holds the token: never shown).
    link: String,
    /// The server's state when it stopped: what's different from it in the tab is your own work.
    confirmed: Box<figcraft_collab::Replica>,
    /// Who you were: the layers you made are in that client's id space.
    resume: Resume,
}

impl Live {
    pub fn role(&self) -> Option<Role> {
        self.info.as_ref().map(|i| i.role)
    }

    /// The session's invite link and URL as `collab.peers` gives them.
    pub fn invite(&self) -> (Option<&str>, Option<&str>) {
        let info = self.info.as_ref();
        (info.map(|i| i.link.as_str()), info.map(|i| i.url.as_str()))
    }

    /// Stop following. False when you weren't following anyone.
    fn unfollow(&mut self) -> bool {
        self.followed = None;
        self.follow.take().is_some()
    }

    /// Back to no session, still shown on its tab (a failure's reason stays there).
    fn reset(&mut self) {
        *self = Live { id: self.id, tab: self.tab, ..Live::default() };
    }

    /// Remember what joining this session again takes, if it is one you joined (see [`Stranded`]).
    fn hold(&mut self) {
        let joined = self.info.as_ref().filter(|i| i.role == Role::Guest && !i.link.is_empty());
        if let (None, Some(client), Some(info)) = (&self.stranded, &self.client, joined) {
            self.stranded = Some(Stranded { link: info.link.clone(), confirmed: Box::new(client.confirmed().clone()), resume: client.resume() });
        }
    }

    /// Keep this session on its tab for joining again: a session you joined, or one that already
    /// is; any other goes back to no session.
    fn strand(&mut self) {
        self.hold();
        let Some(held) = self.stranded.take() else { return self.reset() };
        *self = Live { id: self.id, tab: self.tab, doc_uid: self.doc_uid, info: self.info.take(), stranded: Some(held), ..Live::default() };
    }

    /// You were in this session and aren't now: the connection dropped and it is trying again, or
    /// it can't go on by itself. What you do in its tab isn't reaching anyone.
    pub fn disconnected(&self) -> bool {
        matches!(self.status, Status::Reconnecting { .. }) || self.stranded.is_some()
    }

    /// The session can be joined again in its tab (`collab.reconnect`).
    pub fn rejoinable(&self) -> bool {
        self.stranded.is_some() && !self.status.active()
    }

    /// The document sync client (once the welcome arrived): peers, pending batches, the user's
    /// undo history.
    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    /// Document changes are syncing (the welcome arrived).
    pub fn syncing(&self) -> bool {
        self.client.is_some()
    }

    /// What the file's tab says about its session: `Some(true)` others get in (a session this app
    /// hosts, or one you joined), `None` no session (or one still on its way).
    pub fn shared(&self) -> Option<bool> {
        matches!(self.status, Status::Live | Status::Reconnecting { .. }).then_some(true)
    }

    /// The name of the session's file, once known.
    fn file_name(&self) -> Option<&str> {
        self.info.as_ref().map(|i| i.file_name.as_str()).filter(|n| !n.is_empty())
    }
}

/// The app's live sessions, one per document at most, and what they share: the networking and
/// the invite-link registration.
#[derive(Default)]
pub struct Lives {
    link: Option<Box<dyn LiveLink>>,
    sessions: BTreeMap<SessionId, Live>,
    /// The last session id handed out.
    last: SessionId,
    /// What a document that isn't in a session shows.
    off: Live,
}

impl Lives {
    /// Install the networking (the app does this once at startup).
    pub fn install(&mut self, link: Box<dyn LiveLink>) {
        self.link = Some(link);
    }

    /// Some session runs (hosted, joined or on its way) on a link that wakes no window: the app
    /// polls it until the session ends.
    pub fn needs_polling(&self) -> bool {
        !self.sessions.is_empty() && self.link.as_ref().is_some_and(|l| l.polled())
    }

    /// The installed networking, if any.
    pub(crate) fn link(&mut self) -> Option<&mut (dyn LiveLink + 'static)> {
        self.link.as_deref_mut()
    }

    /// A link is installed: sessions can be joined (and hosted, when [`can_host`](Self::can_host)).
    pub fn available(&self) -> bool {
        self.link.is_some()
    }

    /// Sessions can be hosted from here (a link is installed and it isn't the browser's).
    pub fn can_host(&self) -> bool {
        self.link.as_ref().is_some_and(|l| l.can_host())
    }

    /// Session `id`, while it runs or shows why it failed.
    pub fn get(&self, id: SessionId) -> Option<&Live> {
        self.sessions.get(&id)
    }

    /// Every session, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Live> {
        self.sessions.values()
    }

    /// The session the tab of document `tab` shows (`None`: no document is open): the one that
    /// syncs it, else the latest join started from it.
    fn shown(&self, tab: Option<u64>) -> Option<SessionId> {
        self.sessions.values().filter(|l| l.tab == tab).max_by_key(|l| (l.doc_uid.is_some(), l.id)).map(|l| l.id)
    }

    /// What the tab of document `tab` shows: its session, or no session.
    fn on(&self, tab: Option<u64>) -> &Live {
        self.shown(tab).and_then(|id| self.sessions.get(&id)).unwrap_or(&self.off)
    }

    /// The session of document `uid`: the one that syncs it.
    fn doc(&self, uid: u64) -> Option<&Live> {
        self.sessions.values().find(|l| l.doc_uid == Some(uid))
    }

    fn doc_mut(&mut self, uid: u64) -> Option<&mut Live> {
        self.sessions.values_mut().find(|l| l.doc_uid == Some(uid))
    }

    /// Start a session on the link for command `cmd`: `start` gets the link and the new
    /// session's id, which comes back with what it returned.
    fn start<T>(&mut self, cmd: &str, start: impl FnOnce(&mut dyn LiveLink, SessionId) -> Result<T>) -> Result<(SessionId, T)> {
        let link = self.link.as_deref_mut().ok_or_else(|| unavailable(cmd))?;
        if let Some(why) = crate::file_access::network_refused() {
            return Err(EngineError::Disabled(cmd.to_string(), why));
        }
        let id = self.last.saturating_add(1);
        let out = start(link, id)?;
        self.last = id;
        Ok((id, out))
    }

    /// Session `live` started on the link: it takes its tab over from what was there (a session
    /// that failed; for a document's own session, the one it was in). Joins on their way from
    /// that tab go on.
    fn open(&mut self, live: Live) {
        let own = live.doc_uid.is_some();
        let over: Vec<SessionId> =
            self.sessions.values().filter(|l| l.tab == live.tab && (!l.status.active() || (own && l.doc_uid.is_some()))).map(|l| l.id).collect();
        for id in over {
            self.end(id);
        }
        self.sessions.insert(live.id, live);
    }

    /// End session `id` (stop hosting or leave) and forget it.
    pub(crate) fn end(&mut self, id: SessionId) -> Option<Live> {
        self.leave(id);
        self.sessions.remove(&id)
    }

    /// Tell the link session `id` is over.
    fn leave(&mut self, id: SessionId) {
        if let Some(l) = self.link.as_mut() {
            l.leave(id);
        }
    }

    /// Messages for session `id`'s server, in order.
    fn send(&mut self, id: SessionId, msgs: impl IntoIterator<Item = ClientMsg>) {
        if let Some(l) = self.link.as_mut() {
            for m in msgs {
                l.send(id, m);
            }
        }
    }
}

impl Drop for Lives {
    fn drop(&mut self) {
        for id in std::mem::take(&mut self.sessions).into_keys() {
            self.leave(id);
        }
    }
}

/// The page a presence is on (its viewport's, else its cursor's).
pub fn presence_page(p: &Presence) -> Option<NodeId> {
    p.viewport.map(|v| v.0).or(p.cursor.map(|c| c.0))
}

/// Presence ready to send: non-finite numbers and empty viewports dropped, the selection capped.
pub fn sanitize(mut p: Presence) -> Presence {
    p.cursor = p.cursor.filter(|c| c.1.is_finite() && c.2.is_finite());
    p.viewport = p.viewport.filter(|v| v.1.is_finite() && v.2.is_finite() && v.3.is_finite() && v.4.is_finite() && v.3 > 0.0 && v.4 > 0.0);
    if let Some(s) = p.selection.as_mut() {
        s.truncate(MAX_SELECTION);
    }
    p
}

/// The name others see: the name from Preferences, or empty for the default ("You"), which the
/// host replaces.
pub fn display_name(prefs: &crate::Prefs) -> String {
    let n = prefs.user_name.trim();
    if n == crate::cmd::comment::DEFAULT_AUTHOR { String::new() } else { n.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect() }
}

/// Initials for an avatar ("Ada Lovelace" → "AL"; "Claude (agent)" → "C": words that start with a
/// letter or digit, when there are any; "?" for none).
pub fn initials(name: &str) -> String {
    let firsts = |alnum: bool| -> String {
        name.split_whitespace().filter_map(|w| w.chars().next()).filter(|c| !alnum || c.is_alphanumeric()).take(2).collect()
    };
    let mut out = firsts(true);
    if out.is_empty() {
        out = firsts(false);
    }
    if out.is_empty() {
        out.push('?');
    }
    out.to_uppercase()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// What others are told about your text editing: the layer, caret and selected characters.
fn editing_of(t: &TextEdit) -> EditingText {
    let (a, b) = (t.anchor.min(t.caret), t.anchor.max(t.caret));
    EditingText { node: t.id, caret: t.caret, selection: (a != b).then_some((a, b)) }
}

/// The note shown while others type in the text layer you look at: "Hana is editing this text",
/// "Hana and Ada are…", "Hana and 2 others are…" (`None`: nobody).
pub fn editing_hint(names: &[&str]) -> Option<String> {
    let verb = "editing this text";
    Some(match names {
        [] => return None,
        [one] => format!("{one} is {verb}"),
        [a, b] => format!("{a} and {b} are {verb}"),
        [first, rest @ ..] => format!("{first} and {} others are {verb}", rest.len()),
    })
}

/// Why an edit of a server file you joined with a view link is refused.
pub const VIEW_ONLY: &str = "View only: the link you joined with lets you see this file, not change it";
/// Why an edit other than to comment threads of a server file you joined with a comment link is
/// refused.
pub const COMMENT_ONLY: &str = "Can comment: the link you joined with lets you see this file and comment on it, not change it";

/// Why a session document can't be edited with `access` (`None`: it can).
pub(crate) fn limited_by(access: Access) -> Option<&'static str> {
    match access {
        Access::Edit => None,
        Access::View => Some(VIEW_ONLY),
        Access::Comment => Some(COMMENT_ONLY),
    }
}

/// Whether `tool` is one a file you can't edit now leaves you (`why`: what
/// [`Session::live_read_only`] says): moving around and, with a comment link, commenting.
pub fn tool_left(why: &str, tool: ToolId) -> bool {
    matches!(tool, ToolId::Move | ToolId::Hand) || (why == COMMENT_ONLY && tool == ToolId::Comment)
}

/// A gesture, version preview or live text preview shows a document that isn't the edit to
/// sync yet.
fn busy(ds: &DocState) -> bool {
    ds.interaction.is_some() || ds.preview.is_some() || ds.text_preview.is_some()
}
