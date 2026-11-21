//! The hot's side of a board share (design r=2 §5 "Ordering, at the host", r=3 M2).
//!
//! The host runs FigCraft's `collab::Server` in-process. Every holder the seam admitted gets a seat
//! in it, under a `Hello` the host builds from the node id the seam verified (R4); nothing a
//! holder sends names who it is. A holder's `board.changes` is checked (B1, B2, the ceilings)
//! before the server orders it, and everything the server sends goes out as `board.server`.

use std::collections::{BTreeMap, BTreeSet};

use figcraft_collab::message::{PROTOCOL, client_version};
use figcraft_collab::{Accepted, Access, ClientMsg, Document, Hello, Limits, Outgoing, Replica, Resume, Server, ServerMsg};
use figcraft_engine::live::{Event, PEOPLE};
use macula_craft_seam::{CollabSession, Inbound, Role};

use super::ceiling::{self, Ceilings};
use super::guard::{self, Ballots};
use super::presence::Shown;
use super::wire::{self, CHANGES, FromHost, SERVER, Seat, To, Welcome};

/// A holder's seat: its client id for the whole share (a rejoin keeps it, R4), and what the host
/// gave it.
struct Seated {
    client_id: u32,
    name: String,
    color: String,
    access: Access,
    /// The server's resume secret for this client id. It stays in this process: the host uses it
    /// to seat the node again under the same id, and it is never sent (R4).
    secret: u64,
    /// In the server now (a revoked holder keeps its seat record, out of the server).
    joined: bool,
}

/// The host of one board share.
pub struct HostBoard {
    pub session: CollabSession,
    ceilings: Ceilings,
    server: Server,
    me: u32,
    me_node: String,
    seats: BTreeMap<String, Seated>,
    /// The name and access the host gave each node it invited (B4).
    invited: BTreeMap<String, (String, Access)>,
    /// An upper bound on the board welcome's encoded size (r=3 M2): measured at share start,
    /// raised by each ordered batch, and measured again only near the ceiling.
    welcome_bound: usize,
    /// What the shared board's items were signed with, deleted ones too (B4, r=2).
    signed: guard::Signed,
    shown: Shown,
    /// Holder messages refused here (B1, B2, a ceiling, an unknown command).
    pub refused: u64,
    /// Why the last one was refused.
    pub last_refusal: Option<String>,
    events: Vec<Event>,
}

/// The server message `msg` as an engine event on the host's own side, if it is one.
fn local_event(me: u32, msg: ServerMsg) -> Option<Event> {
    match msg {
        ServerMsg::Changes(_) | ServerMsg::Reject { .. } | ServerMsg::Welcome { .. } => Some(Event::Sync(msg)),
        ServerMsg::PeerJoined(p) if p.client_id != me => Some(Event::PeerJoined(p)),
        ServerMsg::PeerLeft { client_id } => Some(Event::PeerLeft(client_id)),
        ServerMsg::Signal { client_id, signal } => Some(Event::Signal(client_id, signal)),
        _ => None,
    }
}

fn hello(name: &str, color: &str, resume: Option<Resume>) -> Hello {
    Hello {
        protocol: PROTOCOL,
        file: String::new(),
        token: String::new(),
        name: name.to_string(),
        color: color.to_string(),
        client_version: client_version(),
        resume,
    }
}

/// What the seam role an invite gives becomes in FigCraft's server.
pub fn access_of(role: Role) -> Access {
    match role {
        Role::Editor => Access::Edit,
        Role::Viewer => Access::View,
    }
}

impl HostBoard {
    /// Shares `doc` on `session`, the host seated first under `name`. Refused by name, with
    /// nothing sent, when the board holds something a board doesn't share (R3) or its welcome is
    /// over the ceiling (R2).
    pub fn start(session: CollabSession, ceilings: Ceilings, doc: Document, name: &str) -> Result<HostBoard, String> {
        let snapshot = Replica::new(doc.clone()).snapshot().map_err(|e| e.to_string())?;
        if let Some(why) = guard::refused_any(&snapshot) {
            return Err(format!("this board can't be shared: {why}"));
        }
        let me_node = session.author().to_string();
        let mut host = HostBoard {
            session,
            ceilings,
            server: Server::new(doc, "", Limits::default()),
            me: 0,
            me_node: me_node.clone(),
            seats: BTreeMap::new(),
            invited: BTreeMap::new(),
            welcome_bound: 0,
            signed: guard::Signed::default(),
            shown: Shown::default(),
            refused: 0,
            last_refusal: None,
            events: Vec::new(),
        };
        host.signed.learn(&snapshot);
        host.invited.insert(me_node.clone(), (name.to_string(), Access::Edit));
        host.me = host.seat(&me_node).ok_or("the host could not take its own seat")?;
        let size = wire::encode(&host.welcome()?)?.len();
        ceiling::fits(ceiling::WELCOME, size, ceilings.welcome).map_err(|e| format!("this board can't be shared: {e}"))?;
        host.welcome_bound = size;
        Ok(host)
    }

    /// The host's client id.
    pub fn me(&self) -> u32 {
        self.me
    }

    /// The name and colour the host goes by.
    pub fn my_name_and_color(&self) -> (String, String) {
        self.seats.get(&self.me_node).map(|s| (s.name.clone(), s.color.clone())).unwrap_or_default()
    }

    /// Records the name and access an invite gave `node` (B4: the name shown for it).
    pub fn invited(&mut self, node: &str, name: &str, role: Role) {
        self.invited.insert(node.to_string(), (name.to_string(), access_of(role)));
    }

    /// The node seated as `client`.
    pub fn node_of(&self, client: u32) -> Option<&str> {
        self.seats.iter().find(|(_, s)| s.client_id == client).map(|(n, _)| n.as_str())
    }

    /// Seats `node` in the server (again, under its old id when it had one); its client id.
    fn seat(&mut self, node: &str) -> Option<u32> {
        // A node the host never invited (its invite was minted elsewhere, or before a restart)
        // reads and does not write: only an invite gives editing (Terra, P2 note).
        let (name, access) = self.invited.get(node).cloned().unwrap_or((String::new(), Access::View));
        let name = if name.trim().is_empty() { macula_craft_seam::short(node) } else { name };
        let old = self.seats.get(node);
        let color = old.map_or_else(|| PEOPLE[self.seats.len() % PEOPLE.len()].to_string(), |s| s.color.clone());
        let resume = old.map(|s| Resume { session: self.server_session(), client_id: s.client_id, seq: self.server.seq(), secret: s.secret });
        let (id, outs) = match self.server.join_as(hello(&name, &color, resume), access) {
            Ok(joined) => joined,
            Err(refused) => {
                log::warn!("live board: couldn't seat {}: {refused:?}", macula_craft_seam::short(node));
                return None;
            }
        };
        let mut secret = old.map_or(0, |s| s.secret);
        for out in &outs {
            if let Outgoing::To(to, ServerMsg::Welcome { secret: s, .. }) = out
                && *to == id
            {
                secret = *s;
            }
        }
        self.seats.insert(node.to_string(), Seated { client_id: id, name, color, access, secret, joined: true });
        if node == self.me_node {
            // The host seats itself before it shares: nobody holds the board yet, and every holder
            // learns of the host from the welcome, so nothing of this leaves (a refused start
            // sends nothing).
            self.me = id;
            self.route_to(outs, false);
        } else {
            self.broadcast(&FromHost::Seated { node: node.to_string(), client_id: id });
            self.route(outs);
        }
        Some(id)
    }

    fn server_session(&self) -> u64 {
        self.server.state().map(|s| s.session).unwrap_or_default()
    }

    /// The board welcome: the board, the server's position and every seat (R4: no secret).
    pub fn welcome(&self) -> Result<Welcome, String> {
        welcome_of(&self.server, &self.seats)
    }

    /// Seats the holders the seam admitted since the last tick, and unseats the revoked.
    fn seat_holders(&mut self) {
        let holders: BTreeSet<String> = self.session.holders().into_iter().filter(|h| *h != self.me_node).collect();
        for node in &holders {
            if !self.seats.get(node).is_some_and(|s| s.joined) {
                self.seat(node);
            }
        }
        let gone: Vec<u32> =
            self.seats.iter().filter(|(n, s)| s.joined && **n != self.me_node && !holders.contains(*n)).map(|(_, s)| s.client_id).collect();
        for id in gone {
            self.unseat(id);
        }
    }

    /// Takes client `id` out of the server (its seat record stays, for its id).
    fn unseat(&mut self, id: u32) {
        if let Some(s) = self.seats.values_mut().find(|s| s.client_id == id) {
            s.joined = false;
        }
        let outs = self.server.leave(id);
        self.route(outs);
    }

    /// Revokes `node` (hex): the seam revokes its grant and the server drops its seat.
    pub fn revoke(&mut self, node: &str) -> Result<(), String> {
        let node = node.trim().to_ascii_lowercase();
        if node == self.me_node {
            return Err("the host cannot revoke itself: stop sharing instead".into());
        }
        self.session.revoke(&node)?;
        if let Some(id) = self.seats.get(&node).filter(|s| s.joined).map(|s| s.client_id) {
            self.unseat(id);
        }
        Ok(())
    }

    /// Removes client `client` from the share: the seam revokes its node and the server drops it.
    pub fn remove(&mut self, client: u32) -> Result<(), String> {
        let node = self.node_of(client).filter(|n| *n != self.me_node).map(str::to_string).ok_or_else(|| format!("nobody has client id {client}"))?;
        self.session.revoke(&node)?;
        self.unseat(client);
        Ok(())
    }

    /// One frame: the seam's messages, new and revoked holders, presence. The events since.
    pub fn tick(&mut self) -> Vec<Event> {
        let inbound = self.session.tick();
        self.seat_holders();
        for event in inbound {
            match event {
                Inbound::Command { author, id, params, .. } if id == CHANGES => self.on_holders_changes(&author, &params),
                Inbound::SnapshotWanted => self.answer_snapshot(),
                // The host sends the ordered stream and loads no one's board.
                Inbound::Snapshot { .. } => {}
                Inbound::Command { id, .. } => self.refuse(format!("a holder sent `{id}`, which a board never sends")),
                Inbound::Text { .. } => self.refuse("a holder sent text outside a board change".into()),
            }
        }
        let seats: BTreeMap<String, u32> = self.seats.iter().map(|(n, s)| (n.clone(), s.client_id)).collect();
        self.events.extend(self.shown.changes(self.session.peers(), |node| seats.get(node).copied()));
        std::mem::take(&mut self.events)
    }

    fn refuse(&mut self, why: String) {
        log::warn!("live board: refused: {why}");
        self.session.note_refused();
        self.refused += 1;
        self.last_refusal = Some(why);
    }

    /// A holder's `board.changes`, as the client its node is seated as.
    fn on_holders_changes(&mut self, author: &str, params: &[u8]) {
        let Some((client, name)) = self.seats.get(author).filter(|s| s.joined).map(|s| (s.client_id, s.name.clone())) else {
            return self.refuse("changes from a node with no seat".into());
        };
        let msg = match wire::decode::<ClientMsg>(params, self.ceilings.changes) {
            Ok(m) => m,
            Err(e) => return self.refuse(format!("unreadable changes: {e}")),
        };
        match msg {
            ClientMsg::Changes { batch_id, ref changes, .. } => {
                let why = guard::refused_any(changes)
                    .or_else(|| guard::refused_facilitation(changes))
                    .or_else(|| guard::refused_ballot(client, changes, Ballots::OwnOnly))
                    .or_else(|| guard::refused_author(&name, changes, &self.signed))
                    .or_else(|| self.order_fits(&msg).err());
                if let Some(why) = why {
                    self.refuse(why.clone());
                    // Answered as FigCraft's server answers a refused batch: nothing of it applies.
                    let reject = ServerMsg::Reject { batch_id, reason: why, fixes: Vec::new() };
                    return self.route(vec![Outgoing::To(client, reject)]);
                }
                self.order(client, msg);
            }
            ClientMsg::Ack { .. } | ClientMsg::Ping | ClientMsg::Signal(_) => {
                let outs = self.server.receive(client, msg);
                self.route(outs);
            }
            // The host builds every `Hello` itself (R4); presence travels as the seam's cursor.
            _ => self.refuse("a message a holder never sends to the host".into()),
        }
    }

    /// Orders `msg` from client `client` and sends what the server sends.
    fn order(&mut self, client: u32, msg: ClientMsg) {
        let outs = self.server.receive(client, msg);
        for out in &outs {
            if let Outgoing::All(m @ ServerMsg::Changes(acc)) = out {
                self.signed.learn(&acc.changes);
                self.welcome_bound = self.welcome_bound.saturating_add(server_len(&To::All, m));
            }
        }
        self.route(outs);
    }

    /// Whether the host's own `msg` may be applied and sent: the same rule as a holder's (R3; B2:
    /// its own ballot only, though it may clear others' as it runs the votes, B3), and the
    /// ceilings. The engine asks before it applies an act (r=3 M2).
    pub fn check(&mut self, msg: &ClientMsg) -> Result<(), String> {
        let changes = wire::changes_of(msg);
        let name = self.my_name_and_color().0;
        let why = guard::refused_any(changes)
            .or_else(|| guard::refused_ballot(self.me, changes, Ballots::MayClear))
            .or_else(|| guard::refused_author(&name, changes, &self.signed));
        if let Some(why) = why {
            return Err(why);
        }
        self.order_fits(msg)
    }

    /// Whether ordering `msg` keeps its `board.server` and the board welcome under their ceilings
    /// (r=3 M2): the batch as it would go out, then the welcome's bound, measured for real only
    /// when the bound comes within one batch of the ceiling.
    fn order_fits(&mut self, msg: &ClientMsg) -> Result<(), String> {
        let ClientMsg::Changes { batch_id, changes, .. } = msg else { return Ok(()) };
        let widest = ServerMsg::Changes(Accepted { seq: u64::MAX, author: u32::MAX, batch_id: *batch_id, changes: changes.clone() });
        let batch = server_len(&To::All, &widest);
        ceiling::fits(ceiling::SERVER, batch, self.ceilings.server)?;
        if self.welcome_bound.saturating_add(batch) <= self.ceilings.welcome {
            return Ok(());
        }
        let mut probe = self.server.clone();
        let _ = probe.receive(self.me, ClientMsg::Changes { base_seq: probe.seq(), batch_id: u64::MAX, changes: changes.clone() });
        let real = wire::encode(&welcome_of(&probe, &self.seats)?)?.len();
        ceiling::fits(ceiling::WELCOME, real, self.ceilings.welcome)?;
        self.welcome_bound = self.welcome_bound.max(real);
        Ok(())
    }

    /// The host's own message (its acts on the board, already checked): ordered at once.
    pub fn send_own(&mut self, msg: ClientMsg) {
        if matches!(msg, ClientMsg::Changes { .. }) {
            return self.order(self.me, msg);
        }
        let outs = self.server.receive(self.me, msg);
        self.route(outs);
    }

    /// Answers a holder's snapshot request with the board welcome, when it fits.
    fn answer_snapshot(&mut self) {
        let bytes = match self.welcome().and_then(|w| wire::encode(&w)) {
            Ok(b) => b,
            Err(e) => return self.refuse(format!("the board welcome: {e}")),
        };
        if let Err(why) = ceiling::fits(ceiling::WELCOME, bytes.len(), self.ceilings.welcome) {
            return self.refuse(why);
        }
        self.welcome_bound = bytes.len();
        self.session.answer_snapshot(bytes);
    }

    /// The share ends: every holder is told, and keeps its copy.
    pub fn end(&mut self) {
        self.broadcast(&FromHost::Ended);
    }

    /// What the server sent: to the host's own side, and out as `board.server`. A client's own
    /// server welcome never goes out: it carries the resume secret (R4), and a holder starts from
    /// the board welcome instead.
    fn route(&mut self, outs: Vec<Outgoing>) {
        self.route_to(outs, true);
    }

    /// [`route`](Self::route), sending out as `board.server` only when `out`.
    fn route_to(&mut self, outs: Vec<Outgoing>, out: bool) {
        for out_msg in outs {
            let (to, msg) = match out_msg {
                Outgoing::To(id, msg) => (To::One(id), msg),
                Outgoing::All(msg) => (To::All, msg),
                Outgoing::Others(id, msg) => (To::Others(id), msg),
            };
            if to.includes(self.me)
                && let Some(e) = local_event(self.me, msg.clone())
            {
                self.events.push(e);
            }
            if out && to != To::One(self.me) && !matches!(msg, ServerMsg::Welcome { .. }) {
                self.broadcast(&FromHost::Msg { to, msg });
            }
        }
    }

    /// Sends one `board.server`; over its ceiling it is refused by name and nothing is sent.
    fn broadcast(&mut self, msg: &FromHost) {
        let bytes = match wire::encode(msg) {
            Ok(b) => b,
            Err(e) => return self.refuse(format!("an ordered change: {e}")),
        };
        if let Err(why) = ceiling::fits(ceiling::SERVER, bytes.len(), self.ceilings.server) {
            return self.refuse(why);
        }
        self.session.send_command(SERVER.to_string(), bytes, None);
    }
}

/// The encoded size of `msg` as one `board.server` to `to`.
fn server_len(to: &To, msg: &ServerMsg) -> usize {
    wire::encode(&FromHost::Msg { to: *to, msg: msg.clone() }).map_or(usize::MAX, |b| b.len())
}

/// The board welcome of `server` with `seats`' holders in it.
fn welcome_of(server: &Server, seats: &BTreeMap<String, Seated>) -> Result<Welcome, String> {
    let state = server.state().map_err(|e| e.to_string())?;
    let mut list: Vec<Seat> = seats
        .iter()
        .filter(|(_, s)| s.joined)
        .map(|(node, s)| Seat {
            node: node.clone(),
            client_id: s.client_id,
            name: s.name.clone(),
            color: s.color.clone(),
            access: s.access,
            processed: state.issued.get(&s.client_id).cloned().unwrap_or_default(),
        })
        .collect();
    list.sort_by_key(|s| s.client_id);
    Ok(Welcome { session: state.session, seq: state.seq, snapshot: state.snapshot, seats: list })
}
