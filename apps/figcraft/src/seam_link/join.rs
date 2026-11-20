//! A holder's side of a board share (design r=2 §5 "The welcome", r=3 M1).
//!
//! A holder loads only the host's snapshot (B6: by its author, the node the invite pinned) and
//! applies `board.server` only from the host. It keeps its place in the host's order itself:
//! - a batch at or below that place is already in the board it loaded, and is dropped;
//! - a batch past the next one means one was lost. It asks for the board again once, drops the
//!   stream until a snapshot loads, and asks again only after one has loaded, or after the seam
//!   gave the request up (`awaiting_snapshot` false).

use std::collections::BTreeMap;

use figcraft_collab::{ClientMsg, ServerMsg};
use figcraft_engine::live::{Event, Peer, Role, SessionInfo, Status};
use macula_craft_seam::{CollabSession, Inbound};

use super::ceiling::{self, Ceilings};
use super::guard;
use super::presence::{self, Shown};
use super::wire::{self, CHANGES, FromHost, SERVER, Welcome};

/// A holder of one board share.
pub struct JoinBoard {
    pub session: CollabSession,
    ceilings: Ceilings,
    /// The host's node, pinned by the invite.
    host_node: String,
    me_node: String,
    name: String,
    /// The client id the host seated this node as, once the board is loaded.
    me: Option<u32>,
    /// Who is who: node → client id, from the welcome and each seat since.
    clients: BTreeMap<String, u32>,
    /// The host's order this board holds: the last batch applied.
    position: u64,
    /// A gap was found: the stream is dropped until the host's snapshot loads.
    held: bool,
    shown: Shown,
    /// Host messages refused here (B6, an unreadable one, another holder posing as the host).
    pub refused: u64,
    pub last_refusal: Option<String>,
    /// Gaps found in the host's order (M1), each asked about once.
    pub gaps: u64,
    events: Vec<Event>,
}

impl JoinBoard {
    pub fn new(session: CollabSession, ceilings: Ceilings, host_node: &str, name: &str) -> JoinBoard {
        let me_node = session.author();
        JoinBoard {
            session,
            ceilings,
            host_node: host_node.to_string(),
            me_node,
            name: name.to_string(),
            me: None,
            clients: BTreeMap::new(),
            position: 0,
            held: false,
            shown: Shown::default(),
            refused: 0,
            last_refusal: None,
            gaps: 0,
            events: Vec::new(),
        }
    }

    /// The client id this holder is seated as (`None` before the board is loaded).
    pub fn me(&self) -> Option<u32> {
        self.me
    }

    /// Whether the stream is held for a gap (M1).
    pub fn held(&self) -> bool {
        self.held
    }

    fn refuse(&mut self, why: String) {
        log::warn!("live board: refused: {why}");
        self.session.note_refused();
        self.refused += 1;
        self.last_refusal = Some(why);
    }

    /// One frame: the seam's messages, the M1 retry and presence. The events since.
    pub fn tick(&mut self) -> Vec<Event> {
        for event in self.session.tick() {
            match event {
                Inbound::Snapshot { author, data, crdt } => self.snapshot(&author, &data, &crdt),
                Inbound::Command { author, id, params, .. } if id == SERVER => self.from_host(&author, &params),
                // Every holder hears the others' changes on their way to the host; it reads
                // them a moment later in the host's order.
                Inbound::Command { id, .. } if id == CHANGES => {}
                Inbound::Command { id, .. } => self.refuse(format!("a holder sent `{id}`, which a board never sends")),
                Inbound::Text { .. } => self.refuse("text outside a board change".into()),
                // Only the host answers.
                Inbound::SnapshotWanted => {}
            }
        }
        // The seam gave the request up (nobody answered in time): ask once more.
        if self.held && !self.session.awaiting_snapshot() {
            self.session.request_snapshot();
        }
        let clients = self.clients.clone();
        self.events.extend(self.shown.changes(self.session.peers(), |node| clients.get(node).copied()));
        std::mem::take(&mut self.events)
    }

    /// A snapshot: loaded only from the host (B6), only within the ceiling and the board rule.
    fn snapshot(&mut self, author: &str, data: &[u8], crdt: &[u8]) {
        if author != self.host_node {
            // Never parsed.
            return self.refuse(format!("a board from {}, who is not the host", macula_craft_seam::short(author)));
        }
        if let Err(why) = ceiling::fits(ceiling::WELCOME, data.len(), self.ceilings.welcome) {
            return self.refuse(why);
        }
        let welcome: Welcome = match wire::decode(data, self.ceilings.welcome) {
            Ok(w) => w,
            Err(e) => return self.refuse(format!("an unreadable board: {e}")),
        };
        if let Some(why) = guard::refused_any(&welcome.snapshot) {
            return self.refuse(why);
        }
        // Not seated yet: the host seats a holder before it answers, so this is an answer to an
        // earlier ask. The seam asks again.
        let Some(server_welcome) = welcome.for_node(&self.me_node) else { return };
        let Some(seat) = welcome.seat_of(&self.me_node).cloned() else { return };
        self.session.snapshot_loaded(crdt);
        let again = self.me.is_some();
        self.clients = welcome.seats.iter().map(|s| (s.node.clone(), s.client_id)).collect();
        self.position = welcome.seq;
        self.held = false;
        self.me = Some(seat.client_id);
        let peers: Vec<Peer> = welcome
            .seats
            .iter()
            .filter(|s| s.client_id != seat.client_id)
            .map(|s| Peer { client_id: s.client_id, name: s.name.clone(), color: s.color.clone() })
            .collect();
        let info = SessionInfo {
            role: Role::Guest,
            me: seat.client_id,
            name: seat.name.clone(),
            color: seat.color.clone(),
            file_name: String::new(),
            pages: Vec::new(),
            link: String::new(),
            url: String::new(),
            address: String::new(),
            network: true,
            access: seat.access,
            view: None,
        };
        if again {
            // A resync: the client goes offline and takes the welcome back as after a reconnect.
            self.events.push(Event::Status(Status::Reconnecting { attempt: 1 }));
        }
        self.events.push(Event::Welcome { info, peers });
        self.events.push(Event::Sync(server_welcome));
    }

    /// A `board.server`: applied only from the host, past this board's place in its order.
    fn from_host(&mut self, author: &str, params: &[u8]) {
        if author != self.host_node {
            return self.refuse(format!("an ordered change from {}, who is not the host", macula_craft_seam::short(author)));
        }
        let Some(me) = self.me.filter(|_| !self.held) else { return };
        let msg: FromHost = match wire::decode(params, self.ceilings.server) {
            Ok(m) => m,
            Err(e) => return self.refuse(format!("an unreadable ordered change: {e}")),
        };
        let (to, msg) = match msg {
            FromHost::Seated { node, client_id } => {
                self.clients.insert(node, client_id);
                return;
            }
            FromHost::Ended => {
                self.events.push(Event::Ended { reason: "The host stopped sharing the board; your copy stays open here".into() });
                return;
            }
            FromHost::Msg { to, msg } => (to, msg),
        };
        if !to.includes(me) {
            return;
        }
        match msg {
            ServerMsg::Changes(acc) if acc.seq <= self.position => {}
            ServerMsg::Changes(acc) if acc.seq != self.position.saturating_add(1) => {
                // A batch was lost on its way here (M1): ask for the board once.
                log::warn!("live board: sequence {} after {}: asking for the board again", acc.seq, self.position);
                self.gaps += 1;
                self.held = true;
                self.session.request_snapshot();
            }
            ServerMsg::Changes(acc) => {
                self.position = acc.seq;
                self.events.push(Event::Sync(ServerMsg::Changes(acc)));
            }
            ServerMsg::Reject { .. } => self.events.push(Event::Sync(msg)),
            ServerMsg::PeerJoined(p) if p.client_id != me => self.events.push(Event::PeerJoined(p)),
            ServerMsg::PeerLeft { client_id } if client_id == me => {
                self.events.push(Event::Ended { reason: "The host removed you from the board; your copy stays open here".into() });
            }
            ServerMsg::PeerLeft { client_id } => self.events.push(Event::PeerLeft(client_id)),
            ServerMsg::Signal { client_id, signal } => self.events.push(Event::Signal(client_id, signal)),
            _ => {}
        }
    }

    /// Whether this holder's `msg` may be applied and sent: the board rule (B1) and the ceiling
    /// of one `board.changes` (r=3 M2). The engine asks before it applies an act.
    pub fn check(&self, msg: &ClientMsg) -> Result<(), String> {
        if let Some(why) = guard::refused_any(wire::changes_of(msg)) {
            return Err(why);
        }
        let bytes = wire::encode(msg)?;
        ceiling::fits(ceiling::CHANGES, bytes.len(), self.ceilings.changes)
    }

    /// Sends `msg` to the host, refused by name and unsent over the ceiling.
    pub fn send(&mut self, msg: &ClientMsg) -> Result<(), String> {
        let bytes = wire::encode(msg)?;
        ceiling::fits(ceiling::CHANGES, bytes.len(), self.ceilings.changes)?;
        self.session.send_command(CHANGES.to_string(), bytes, None);
        Ok(())
    }

    /// This holder's cursor and selection, to everyone.
    pub fn presence(&mut self, p: &figcraft_engine::live::Presence) {
        self.session.share_cursor(presence::encode(p));
    }

    /// The name this holder joined with.
    pub fn name(&self) -> &str {
        &self.name
    }
}
