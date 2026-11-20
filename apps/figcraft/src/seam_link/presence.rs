//! Presence over the seam's cursor (design r=2 §5 "Presence"). The position is FigCraft's own
//! `Presence` as JSON. A cursor is shown for the client id the host seated that node as; the name
//! in the seam's frame is never shown (B4): the engine shows the seated name.

use std::collections::BTreeMap;

use figcraft_engine::live::{Event, Presence};
use macula_craft_seam::Peer;

/// The positions already passed on, by node.
#[derive(Default)]
pub struct Shown(BTreeMap<String, Option<String>>);

impl Shown {
    /// The presence events for the cursors that moved since the last call: by the client id
    /// `client_of` gives the node, and only for a seated node.
    pub fn changes<'a>(&mut self, peers: impl Iterator<Item = &'a Peer>, client_of: impl Fn(&str) -> Option<u32>) -> Vec<Event> {
        let mut out = Vec::new();
        for peer in peers {
            if self.0.get(&peer.author) == Some(&peer.position) {
                continue;
            }
            let Some(client) = client_of(&peer.author) else { continue };
            self.0.insert(peer.author.clone(), peer.position.clone());
            let presence = peer.position.as_deref().and_then(|p| serde_json::from_str::<Presence>(p).ok()).unwrap_or_default();
            out.push(Event::Presence(client, presence));
        }
        out
    }
}

/// What the seam's cursor carries for `presence`.
pub fn encode(presence: &Presence) -> Option<String> {
    serde_json::to_string(presence).ok()
}
