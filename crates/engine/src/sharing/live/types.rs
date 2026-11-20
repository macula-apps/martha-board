use super::{Access, Arc, ClientMsg, Code, Document, IpAddr, Peer, Presence, Resume, Serialize, ServerMsg, Signal, code};

/// The server a join code from a server is asked about unless Preferences name another: the only
/// place its address is written.
///
/// A build can ship another one, or none: set `FIGCRAFT_DEFAULT_SERVER` when compiling to a
/// server's address (`wss://host/`), or to nothing (`FIGCRAFT_DEFAULT_SERVER=`) for a build whose
/// server join codes stay off until the user enters a server in Preferences.
pub const DEFAULT_SERVER: &str = match option_env!("FIGCRAFT_DEFAULT_SERVER") {
    Some(server) => server,
    None => "wss://collab.craftapps.dev/",
};

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
    /// The invite link (`figcraft://join?host=…&file=…&token=…`) and the same as a plain
    /// WebSocket URL. Both carry the session's secret token: share them only with people you
    /// invite.
    pub link: String,
    pub url: String,
    /// `ip:port` the session listens on (host) or connects to (guest); a server's URL when it
    /// isn't plain `ws://` at `/`.
    pub address: String,
    /// Reachable from other machines (the host listens on a network address, not loopback).
    pub network: bool,
    /// What you may do: edit, only view (a view link or code), or view and comment (a server's
    /// comment link).
    pub access: Access,
    /// A session this app hosts with a way in for looking only ([`HostOptions::view`]): its
    /// invite link and WebSocket URL, which carry the view secret (`None`: everyone who joins
    /// edits, or you joined).
    pub view: Option<WayIn>,
}

impl SessionInfo {
    /// The invite link and WebSocket URL that let people into a session this app hosts with
    /// `access` (its own link and URL edit). `None`: no way in gives that.
    pub fn way_in(&self, access: Access) -> Option<(&str, &str)> {
        match access {
            Access::Edit => Some((&self.link, &self.url)),
            Access::View => self.view.as_ref().map(|v| (v.link.as_str(), v.url.as_str())),
            Access::Comment => None,
        }
    }
}

/// A way into a session this app hosts: an invite link and the WebSocket URL it connects to,
/// which carry the same secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WayIn {
    pub link: String,
    pub url: String,
}

/// How a file is shared with a join code: from this app, to people on the same network (sharing
/// through a server left with martha-board#41).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Via {
    Local,
}

/// One way into a shared file: an invite link, and the join code that stands for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SharedLink {
    /// The invite link (it carries its secret).
    pub link: String,
    /// Its join code's symbols. `None`: there is none (a host without an IPv4 address): the link
    /// is shared instead.
    pub code: Option<String>,
}

/// What others join a shared file with: to edit it and, when the share has one, to only view it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Shared {
    pub edit: SharedLink,
    pub view: Option<SharedLink>,
    /// `None` for a share on the local network (comment links came from a server).
    pub comment: Option<SharedLink>,
}

impl Shared {
    /// The way in that gives `access`, if there is one.
    pub fn of(&self, access: Access) -> Option<&SharedLink> {
        match access {
            Access::Edit => Some(&self.edit),
            Access::View => self.view.as_ref(),
            Access::Comment => self.comment.as_ref(),
        }
    }
}

/// A file shared with a join code (`collab.share`): the session this app hosts for it, and the
/// codes others join with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    pub via: Via,
    /// What the code shown lets people do.
    pub access: Access,
    /// What others join with now. `None`: on its way.
    pub links: Option<Shared>,
}

impl Share {
    /// The way in to show: the one for [`access`](Self::access).
    fn shown(&self) -> Option<&SharedLink> {
        self.links.as_ref()?.of(self.access)
    }

    /// The code to show, grouped (`K7M-2QX-9TD`).
    pub fn code(&self) -> Option<String> {
        self.shown()?.code.as_deref().map(code::grouped)
    }

    /// The invite link to give with (or in place of) the code.
    pub fn link(&self) -> Option<&str> {
        self.shown().map(|l| l.link.as_str())
    }
}

/// What someone typed to join, as what the link joins: an invite link as it is, a local network
/// code as the link it stands for, a server's code with `server` to ask about it.
pub fn join_target(server: &str, typed: &str) -> std::result::Result<String, String> {
    let typed = typed.trim();
    if typed.contains("://") || typed.contains('?') {
        return Ok(typed.to_string());
    }
    match code::parse(typed)? {
        Code::Local(local) => Ok(local.link()),
        Code::Online(_) if server.trim().is_empty() => Err(format!("that is an online join code, and {ONLINE_OFF}")),
        Code::Online(code) => Ok(format!("figcraft://join?server={}&code={code}", server.trim())),
    }
}

/// Why a server's join code can't be used.
pub const ONLINE_OFF: &str = "online sharing is off: choose a server in Preferences (or share on the local network)";

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

/// Which network interface a hosted session listens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bind {
    /// This machine only (127.0.0.1): the default.
    Local,
    /// The machine's address on the local network (only when the user chooses to share there).
    Lan,
    /// A given interface address (e.g. a VPN's).
    Addr(IpAddr),
}

/// What [`LiveLink::host`] needs to start a session for the open file.
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// `None`: the default port, or any free one when it's taken.
    pub port: Option<u16>,
    pub bind: Bind,
    /// Your name (empty: the host picks one).
    pub name: String,
    /// The shared file (the session's authoritative copy starts from it).
    pub doc: Arc<Document>,
    /// Shared with a join code (`collab.share`): the session's secret is short enough to type.
    pub short: bool,
    /// Also make a second secret whose holders only look: they see every change, and the session
    /// refuses theirs ([`SessionInfo::view`]).
    pub view: bool,
}

/// The networking behind live sessions, installed by the app ([`Lives::install`]). It carries
/// any number of sessions at once, each under the [`SessionId`] it was started with. Calls return
/// at once; what happens later arrives through [`poll`](Self::poll).
pub trait LiveLink {
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
}
