//! The three ceilings (R2, M2): the most bytes a `board.changes`, a `board.server` and the
//! board welcome may carry in one sealed frame. They come from craft-collab's
//! `MeshTransport::largest_payload_that_fits`, measured once per share; no size is written here.

/// The most app bytes each of the board's three messages carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ceilings {
    /// A holder's `board.changes` params.
    pub changes: usize,
    /// One `board.server` params.
    pub server: usize,
    /// The board welcome, the host's snapshot data.
    pub welcome: usize,
}

/// What each ceiling is called when a send is refused.
pub const CHANGES: &str = "change";
pub const SERVER: &str = "ordered change";
pub const WELCOME: &str = "board";

/// Refuses `len` bytes of `what` past `ceiling`, by name: nothing is sent.
pub fn fits(what: &str, len: usize, ceiling: usize) -> Result<(), String> {
    if len <= ceiling {
        return Ok(());
    }
    Err(format!("the {what} is {len} bytes, over the {ceiling} bytes one sealed message carries: nothing was sent"))
}

/// The three ceilings of the share whose group is `prefix`, for a publisher of `profile`: each
/// measured through craft-collab's frame path with the message the board will send (`doc` and
/// `author` as the share has them, `crdt` the snapshot's text history).
pub fn measure(prefix: &str, profile: macula_rust::profile::Profile, doc: &str, author: &str, crdt: &[u8]) -> Result<Ceilings, String> {
    use macula_craft_collab::MeshTransport;
    use macula_craft_seam::{Kind, Message};
    let envelope = |kind: Kind| Message { doc: doc.to_string(), author: author.to_string(), epoch: 0, seq: 0, sent_at_ms: 0, kind };
    let command = |id: &str| envelope(Kind::Command { id: id.to_string(), params: Vec::new(), anchor: None });
    Ok(Ceilings {
        changes: MeshTransport::largest_payload_that_fits(prefix, profile, &command(super::wire::CHANGES))?,
        server: MeshTransport::largest_payload_that_fits(prefix, profile, &command(super::wire::SERVER))?,
        welcome: MeshTransport::largest_payload_that_fits(prefix, profile, &envelope(Kind::Snapshot { data: Vec::new(), crdt: crdt.to_vec() }))?,
    })
}
