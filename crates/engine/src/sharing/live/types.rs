use super::{Access, Arc, ClientMsg, Document, Peer, Presence, Resume, Serialize, ServerMsg, Signal};

/// Most selected ids one presence update carries (the rest aren't shown to others).
pub const MAX_SELECTION: usize = 10_000;

pub use figcraft_collab::message::MAX_PEERS;

/// Longest name shown to others (characters).
pub const MAX_NAME: usize = Signal::MAX_NAME;

/// The colours people get in a session, in the order the host hands them out (`#RRGGBB`, white
/// text reads on each).
pub const PEOPLE: [&str; 8] = ["#0D99FF", "#9747FF", "#14AE5C", "#F24822", "#D9730D", "#E83E8C", "#00A3A3", "#665CE0"];

/// `#RRGGBB` → (r, g, b); `None` for anything else.
pub fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let byte = |i: usize| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// Why a join failed when a message from the host is over this app's size limit (`{mb}`: the
/// limit in MB). The UI translates it.
pub const TOO_LARGE_TO_JOIN: &str = "This file is too large to join here (up to {mb} MB)";

/// The connection to a live session.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Status {
    /// Not in a session.
    #[default]
    Off,
    /// Joining (connecting to the host, waiting for its welcome).
    Connecting,
    /// In the session.
    Live,
    /// The connection dropped; trying again (`attempt` from 1).
    Reconnecting { attempt: u32 },
    /// Couldn't join, or the connection was lost for good.
    Failed { reason: String },
}

impl Status {
    pub fn id(&self) -> &'static str {
        match self {
            Status::Off => "off",
            Status::Connecting => "connecting",
            Status::Live => "live",
            Status::Reconnecting { .. } => "reconnecting",
            Status::Failed { .. } => "failed",
        }
    }

    /// In a session or on the way into one.
    pub fn active(&self) -> bool {
        matches!(self, Status::Connecting | Status::Live | Status::Reconnecting { .. })
    }

    /// What the Share popover says about it.
    pub fn label(&self) -> String {
        match self {
            Status::Off => "Not in a live session".into(),
            Status::Connecting => "Joining…".into(),
            Status::Live => "Live".into(),
            Status::Reconnecting { attempt } => format!("Connection lost, reconnecting (attempt {attempt})…"),
            Status::Failed { reason } => reason.clone(),
        }
    }
}

/// Your part in the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Host,
    Guest,
}

/// A page of the shared file.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageInfo {
    pub id: u64,
    pub name: String,
}

/// The session you're in.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub role: Role,
    /// Your client id, name and colour as the others see them.
    pub me: u32,
    pub name: String,
    pub color: String,
    /// The shared file's name and pages (the host's).
    pub file_name: String,
    pub pages: Vec<PageInfo>,
    /// The invite link (what `collab.join {link}` takes, passed to [`LiveLink::join`] as it is)
    /// and the URL the link gives for it. Both carry the session's secret token: share them only with people you
    /// invite.
    pub link: String,
    pub url: String,
    /// `ip:port` the session listens on (host) or connects to (guest); a server's URL when it
    /// isn't plain `ws://` at `/`.
    pub address: String,
    /// Reachable from other machines (the host listens on a network address, not loopback).
    pub network: bool,
    /// What you may do: edit, only view (a view link), or view and comment (a server's
    /// comment link).
    pub access: Access,
}

/// A session's number on the link: the engine picks it (from 1) when it starts the session.
pub type SessionId = u64;

/// The [`SessionId`] of events that aren't about a session.
pub const NO_SESSION: SessionId = 0;

/// What happened in a session since the last [`LiveLink::poll`].
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Status(Status),
    /// Joined, or joined again after a reconnect: who you are and who else is here.
    Welcome {
        info: SessionInfo,
        peers: Vec<Peer>,
    },
    PeerJoined(Peer),
    PeerLeft(u32),
    Presence(u32, Presence),
    /// Someone's cursor chat or spotlight, or (about you) the host's [`Signal::READY`].
    Signal(u32, Signal),
    /// For the document sync client: the welcome (with the whole document), or a batch of
    /// changes the server ordered, or the rejection of one of yours.
    Sync(ServerMsg),
    /// The host ended the session.
    Ended {
        reason: String,
    },
}

/// What [`LiveLink::host`] needs to start a session for the open file.
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// Your name (empty: the host picks one).
    pub name: String,
    /// The shared file (the session's authoritative copy starts from it).
    pub doc: Arc<Document>,
}

/// What a link without sealed shares answers for the acts only they have.
pub const NOT_SEALED: &str = "this app shares no board over the mesh";

/// The networking behind live sessions, installed by the app ([`Lives::install`]). It carries
/// any number of sessions at once, each under the [`SessionId`] it was started with. Calls return
/// at once; what happens later arrives through [`poll`](Self::poll).
pub trait LiveLink {
    /// The link wakes no window when something arrives (it is only read when polled), so the app
    /// polls it while a session runs. No by default: a link that wakes the window costs no frames.
    fn polled(&self) -> bool {
        false
    }
    /// Sessions can be hosted here (the browser build can only join).
    fn can_host(&self) -> bool {
        true
    }
    /// Start hosting session `session` for `opts.doc`.
    fn host(&mut self, session: SessionId, opts: HostOptions) -> std::result::Result<SessionInfo, String>;
    /// Start joining the session behind an invite link, as session `session`. Errors are for
    /// links that can't be used; they never echo the link (it holds the token).
    fn join(&mut self, session: SessionId, link: &str, name: &str) -> std::result::Result<(), String>;
    /// [`join`](Self::join) as the client `resume` names, when the server still knows it (an app
    /// that starts over after a crash keeps its id, so what it made before is its own to send).
    /// A link that can't resume joins as someone new.
    fn join_resuming(&mut self, session: SessionId, link: &str, name: &str, _resume: Option<Resume>) -> std::result::Result<(), String> {
        self.join(session, link, name)
    }
    /// Remove client `client` from session `session`, which this link hosts: they are
    /// disconnected and can't join again from their address while it runs. Returns at once.
    fn remove(&mut self, _session: SessionId, _client: u32) -> std::result::Result<(), String> {
        Err("removing people needs the desktop app or the MCP server".into())
    }
    /// End session `session` (stop hosting or leave); the others go on. Nothing for a session
    /// that is over already.
    fn leave(&mut self, session: SessionId);
    /// Session `session` is waiting to reconnect: try now.
    fn retry(&mut self, _session: SessionId) {}
    /// Your latest presence in `session` (the link throttles and coalesces).
    fn presence(&mut self, session: SessionId, p: &Presence);
    /// A message for `session`'s server (document changes), sent in order.
    fn send(&mut self, session: SessionId, msg: ClientMsg);
    /// Events since the last call, each with its session ([`NO_SESSION`] for events about none);
    /// never blocks.
    fn poll(&mut self) -> Vec<(SessionId, Event)>;
    /// Whether `msg` may be applied and sent in `session`: asked before an act is kept, so a
    /// refused act leaves nothing behind (what a share carries, and how large). Yes by default.
    fn check(&mut self, _session: SessionId, _msg: &ClientMsg) -> std::result::Result<(), String> {
        Ok(())
    }
    /// An invite line for the node `node` (hex), shown as `name`, as `role` (`editor` or `viewer`),
    /// good for `ttl_ms`: only the host of a sealed share mints one.
    fn invite(&mut self, _session: SessionId, _node: &str, _name: &str, _role: &str, _ttl_ms: u64) -> std::result::Result<String, String> {
        Err(NOT_SEALED.into())
    }
    /// Lets in the node `node`, waiting at the door of `session`.
    fn approve(&mut self, _session: SessionId, _node: &str) -> std::result::Result<(), String> {
        Err(NOT_SEALED.into())
    }
    /// Takes the node `node` out of `session` for good: its grant is revoked.
    fn revoke(&mut self, _session: SessionId, _node: &str) -> std::result::Result<(), String> {
        Err(NOT_SEALED.into())
    }
    /// What the share behind `session` counts: who holds it, who waits, what it refused.
    fn stats(&self, _session: SessionId) -> std::result::Result<serde_json::Value, String> {
        Err(NOT_SEALED.into())
    }
    /// This app's own node id (hex): the one it signs with when it hosts or joins, which a host
    /// needs to invite it (martha-board#2 B8).
    fn own_node(&mut self) -> std::result::Result<String, String> {
        Err(NOT_SEALED.into())
    }
}
