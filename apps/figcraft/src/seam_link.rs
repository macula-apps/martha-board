//! MarthaBoard's live link over the craft-collab seam (martha-board#2 P2; design r=2 §5 and r=3
//! M1, M2), behind the engine's [`LiveLink`]. There is no server process: the host's app orders
//! the board's changes in-process ([`host`]), every holder applies the same ordered stream
//! ([`join`]), and the seam carries the bytes, sealed, to invited nodes only.
//!
//! How a share is opened (the mesh, or an in-memory bus in tests) is the [`Opener`]'s; everything
//! after that is here.

pub(crate) mod ceiling;
pub(crate) mod guard;
mod host;
mod join;
mod presence;
pub(crate) mod wire;

use std::collections::BTreeMap;

use figcraft_collab::ClientMsg;
use figcraft_engine::live::{Event, HostOptions, LiveLink, PageInfo, Presence, Role, SessionId, SessionInfo};
use macula_craft_seam::CollabSession;
use serde_json::{Value, json};

pub use ceiling::Ceilings;
pub use ceiling::measure as ceiling_measure;
pub use host::HostBoard;
pub use join::JoinBoard;

/// A share opened on the seam: its session, its ceilings and the host's node (the one whose
/// board and ordered stream a holder takes, B6).
pub struct Opened {
    pub session: CollabSession,
    pub ceilings: Ceilings,
    pub host_node: String,
}

/// Opens shares: on the mesh in the app (`collab_setup`), on an in-memory bus in tests.
pub trait Opener {
    /// Shares a board as its host, as `name`.
    fn host(&mut self, file_name: &str, name: &str) -> Result<Opened, String>;
    /// Joins the share behind an invite line, as `name`.
    fn join(&mut self, invite: &str, name: &str) -> Result<Opened, String>;
}

/// One running share.
pub enum Board {
    Host(Box<HostBoard>),
    Join(Box<JoinBoard>),
}

/// The live link over the seam: any number of shares, each under its engine session id.
pub struct SeamLink {
    opener: Box<dyn Opener>,
    boards: BTreeMap<SessionId, Board>,
    /// Events waiting for the next poll.
    waiting: Vec<(SessionId, Event)>,
}

impl SeamLink {
    pub fn new(opener: Box<dyn Opener>) -> SeamLink {
        SeamLink { opener, boards: BTreeMap::new(), waiting: Vec::new() }
    }

    fn hosted(&mut self, session: SessionId) -> Result<&mut HostBoard, String> {
        match self.boards.get_mut(&session) {
            Some(Board::Host(h)) => Ok(h),
            Some(Board::Join(_)) => Err("only the host of the board can do that".into()),
            None => Err("this board isn't shared".into()),
        }
    }

    /// `collab.stats`: the seam's numbers, the board's refusals and the gaps it asked about.
    fn stats_of(&self, session: SessionId) -> Result<Value, String> {
        match self.boards.get(&session) {
            Some(Board::Host(h)) => Ok(json!({
                "role": "host",
                "node": h.session.author(),
                "holders": h.session.holders(),
                "pending": h.session.pending().into_iter().map(|(node, name)| json!({"node": node, "name": name})).collect::<Vec<_>>(),
                "refused": h.refused,
                "lastRefusal": h.last_refusal,
                "seam": h.session.stats(),
            })),
            Some(Board::Join(j)) => Ok(json!({
                "role": "guest",
                "node": j.session.author(),
                "refused": j.refused,
                "lastRefusal": j.last_refusal,
                "gaps": j.gaps,
                "seam": j.session.stats(),
            })),
            None => Err("this board isn't shared".into()),
        }
    }
}

impl LiveLink for SeamLink {
    /// Whether `msg` may be applied and sent in `session` (B1 both ways, R3; the ceilings, M2):
    /// the engine asks before it keeps an act, so a refused act leaves nothing behind.
    fn check(&mut self, session: SessionId, msg: &ClientMsg) -> Result<(), String> {
        match self.boards.get_mut(&session) {
            Some(Board::Host(h)) => h.check(msg),
            Some(Board::Join(j)) => j.check(msg),
            None => Ok(()),
        }
    }

    /// `collab.invite`: an invite line for one named node (r=2 §5 "Invite"). The node gets the
    /// name the host gives it here, and that is the name shown for it (B4).
    fn invite(&mut self, session: SessionId, node: &str, name: &str, role: &str, ttl_ms: u64) -> Result<String, String> {
        let role = match role {
            "viewer" => macula_craft_seam::Role::Viewer,
            _ => macula_craft_seam::Role::Editor,
        };
        let host = self.hosted(session)?;
        let line = host.session.mint_invite_for(None, None, node, name, role, ttl_ms)?;
        host.invited(&node.trim().to_ascii_lowercase(), name, role);
        Ok(line)
    }

    /// `collab.approve`: lets in a node waiting at the door.
    fn approve(&mut self, session: SessionId, node: &str) -> Result<(), String> {
        self.hosted(session)?.session.approve(node)
    }

    /// `collab.revoke`: the node's grant is revoked and its seat goes.
    fn revoke(&mut self, session: SessionId, node: &str) -> Result<(), String> {
        self.hosted(session)?.revoke(node)
    }

    fn stats(&self, session: SessionId) -> Result<Value, String> {
        self.stats_of(session)
    }

    fn host(&mut self, session: SessionId, opts: HostOptions) -> Result<SessionInfo, String> {
        let doc = (*opts.doc).clone();
        let pages = doc.pages.iter().map(|p| PageInfo { id: p.0, name: doc.node(*p).map(|n| n.name.clone()).unwrap_or_default() }).collect();
        let file_name = doc.name.clone();
        let opened = self.opener.host(&file_name, &opts.name)?;
        let host = HostBoard::start(opened.session, opened.ceilings, doc, &opts.name)?;
        let (name, color) = host.my_name_and_color();
        let info = SessionInfo {
            role: Role::Host,
            me: host.me(),
            name,
            color,
            file_name,
            pages,
            link: String::new(),
            url: String::new(),
            address: String::new(),
            network: true,
            access: figcraft_collab::Access::Edit,
            view: None,
        };
        self.boards.insert(session, Board::Host(Box::new(host)));
        Ok(info)
    }

    fn join(&mut self, session: SessionId, link: &str, name: &str) -> Result<(), String> {
        let opened = self.opener.join(link, name)?;
        let board = JoinBoard::new(opened.session, opened.ceilings, &opened.host_node);
        self.boards.insert(session, Board::Join(Box::new(board)));
        Ok(())
    }

    fn remove(&mut self, session: SessionId, client: u32) -> Result<(), String> {
        self.hosted(session)?.remove(client)
    }

    fn leave(&mut self, session: SessionId) {
        if let Some(Board::Host(mut h)) = self.boards.remove(&session) {
            h.end();
        }
    }

    fn presence(&mut self, session: SessionId, p: &Presence) {
        match self.boards.get_mut(&session) {
            Some(Board::Host(h)) => h.session.share_cursor(presence::encode(p)),
            Some(Board::Join(j)) => j.presence(p),
            None => {}
        }
    }

    fn send(&mut self, session: SessionId, msg: ClientMsg) {
        match self.boards.get_mut(&session) {
            Some(Board::Host(h)) => h.send_own(msg),
            Some(Board::Join(j)) => {
                if let Err(why) = j.send(&msg) {
                    log::warn!("live board: {why}");
                }
            }
            None => {}
        }
    }

    fn poll(&mut self) -> Vec<(SessionId, Event)> {
        let mut out = std::mem::take(&mut self.waiting);
        for (id, board) in &mut self.boards {
            let events = match board {
                Board::Host(h) => h.tick(),
                Board::Join(j) => j.tick(),
            };
            out.extend(events.into_iter().map(|e| (*id, e)));
        }
        out
    }
}
