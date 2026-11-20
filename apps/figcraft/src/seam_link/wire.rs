//! What the board's two seam commands and its snapshot carry (martha-board#2 design r=2 §5).
//!
//! - `board.changes`: a holder's [`ClientMsg`] for the host, in FigCraft's own codec.
//! - `board.server`: what the host's in-process server sends, addressed by client id inside
//!   ([`FromHost`]). Every holder receives every one and keeps what is for it.
//! - The snapshot's data: the board welcome ([`Welcome`]). It holds the board, the server's
//!   position in its order and who is who, and nothing per client that is secret (R4).

use figcraft_collab::message::{self, MAX_PEERS};
use figcraft_collab::{Access, CatchUp, Change, ClientMsg, Peer, Processed, ServerMsg};
use serde::{Deserialize, Serialize};

/// The seam command a holder's changes travel as, to the host only.
pub const CHANGES: &str = "board.changes";

/// The seam command the host's ordered stream travels as, to every holder.
pub const SERVER: &str = "board.server";

/// Who a message from the host is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum To {
    All,
    One(u32),
    /// Everyone but this client.
    Others(u32),
}

impl To {
    /// Whether client `me` keeps it.
    pub fn includes(self, me: u32) -> bool {
        match self {
            To::All => true,
            To::One(id) => id == me,
            To::Others(id) => id != me,
        }
    }
}

/// One `board.server` message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FromHost {
    /// A holder took a seat: its node and its client id, sent before the server's `PeerJoined`
    /// for it, so every laptop knows whose cursor and name a client id is (B4).
    Seated { node: String, client_id: u32 },
    /// What the server sent, and to whom.
    Msg { to: To, msg: ServerMsg },
    /// The host stopped sharing: every holder keeps its copy.
    Ended,
}

/// One holder in the board welcome: its node, its client id, the name and colour the host gave
/// it, what it may do and what the server did with its batches so far. No resume secret (R4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seat {
    pub node: String,
    pub client_id: u32,
    pub name: String,
    pub color: String,
    pub access: Access,
    pub processed: Processed,
}

/// The board welcome, the data of the host's snapshot: the same for every holder (R4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    /// The host's server session.
    pub session: u64,
    /// The server's position in its order: the board holds every batch up to it.
    pub seq: u64,
    pub snapshot: Vec<Change>,
    /// Everyone seated, the host first.
    pub seats: Vec<Seat>,
}

impl Welcome {
    /// The seat of `node`, if it has one.
    pub fn seat_of(&self, node: &str) -> Option<&Seat> {
        self.seats.iter().find(|s| s.node == node)
    }

    /// The server welcome FigCraft's client starts from, for the holder at `node`: its own id,
    /// access and batches, the others as peers, and no secret. `None` when `node` has no seat.
    pub fn for_node(&self, node: &str) -> Option<ServerMsg> {
        let me = self.seat_of(node)?;
        let peers = self
            .seats
            .iter()
            .filter(|s| s.client_id != me.client_id)
            .take(MAX_PEERS)
            .map(|s| Peer { client_id: s.client_id, name: s.name.clone(), color: s.color.clone() })
            .collect();
        Some(ServerMsg::Welcome {
            session: self.session,
            client_id: me.client_id,
            seq: self.seq,
            catch_up: CatchUp::Snapshot(self.snapshot.clone()),
            processed: me.processed.clone(),
            peers,
            access: me.access,
            secret: 0,
        })
    }
}

/// The bytes of `msg` on the wire (FigCraft's binary codec).
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, String> {
    message::encode(msg).map_err(|e| e.to_string())
}

/// `bytes` read back, refused past `max` bytes (untrusted input).
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T, String> {
    message::decode(bytes, max).map_err(|e| e.to_string())
}

/// The changes a holder's message carries (none for any other message).
pub fn changes_of(msg: &ClientMsg) -> &[Change] {
    match msg {
        ClientMsg::Changes { changes, .. } => changes,
        _ => &[],
    }
}
