//! The seam link, end to end on the seam's in-memory bus: whole FigCraft sessions sharing a board
//! with no server (martha-board#2 P2).

use figcraft_collab::{Change, ClientMsg, DocProp, Entry, Json, Limits, Replica, Server, ServerMsg};
use figcraft_doc::{NodeId, NodeKind};
use macula_craft_seam::{Inbound, Kind, short};
use serde_json::json;

use crate::seam_link::wire::{self, CHANGES, FromHost, SERVER, To, Welcome};
use crate::seam_link_harness::{Rogue, Room, board, id_of, node, real_ceilings};

fn room(holders: usize, spares: &[String]) -> Room {
    Room::with(board("Storm"), holders, spares, real_ceilings(&node('a')))
}

/// Board acts from every side end the same on every laptop (as the engine's own
/// `boards_sync_in_live_sessions`, with the seam between them instead of a server).
fn acts_converge(holders: usize) {
    let mut room = room(holders, &[]);
    let a = id_of(&room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap());
    room.settle();
    let b = id_of(&room.s(1).execute("board.sticky", &json!({"x": 400, "y": 0, "text": "B"})).unwrap());
    room.settle();
    room.s(0).execute("board.connector", &json!({"from": a, "to": b})).unwrap();
    room.s(holders).execute("board.stamp", &json!({"x": 50, "y": 50, "stamp": "star"})).unwrap();
    room.settle();
    room.check();
    room.s(1).execute("transform.move", &json!({"ids": [b], "dx": 0, "dy": 250})).unwrap();
    room.s(0).execute("board.setColor", &json!({"ids": [a], "color": "violet"})).unwrap();
    room.settle();
    room.check();
}

#[test]
fn two_laptops_converge() {
    acts_converge(1);
}

#[test]
fn three_laptops_converge() {
    acts_converge(2);
}

#[test]
fn undo_reaches_every_laptop() {
    let mut room = room(1, &[]);
    room.s(1).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap();
    room.settle();
    room.check();
    let count = |room: &mut Room| room.s(0).doc().unwrap().nodes().count();
    let with = count(&mut room);
    room.s(1).execute("edit.undo", &json!({})).unwrap();
    room.settle();
    room.check();
    assert!(count(&mut room) < with, "the undone sticky is gone on the host too");
}

/// R2: board text travels inside the board's own changes; no laptop ever sends the seam's text edits.
#[test]
fn no_laptop_sends_seam_text_edits() {
    let mut room = room(1, &[]);
    room.s(1).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "typed"})).unwrap();
    room.settle();
    room.check();
    for i in 0..2 {
        assert!(room.sent(i, |k| matches!(k, Kind::TextEdit { .. })).is_empty(), "person {i} sent a text edit");
    }
    assert!(!room.sent(1, |k| matches!(k, Kind::Command { .. })).is_empty(), "the guest's acts did travel as commands");
}


/// Every `board.server` the host sent, read back.
fn from_host(room: &Room) -> Vec<FromHost> {
    room.sent(0, |k| matches!(k, Kind::Command { id, .. } if id == SERVER))
        .into_iter()
        .map(|m| match m.kind {
            Kind::Command { params, .. } => wire::decode(&params, room.ceilings.server).unwrap(),
            _ => unreachable!(),
        })
        .collect()
}

/// The client id the host seated `rogue` as.
fn seated(room: &mut Room, rogue: &mut Rogue) -> u32 {
    room.churn(rogue, 10);
    from_host(room)
        .into_iter()
        .find_map(|m| match m {
            FromHost::Seated { node, client_id } if node == rogue.node => Some(client_id),
            _ => None,
        })
        .expect("the rogue got a seat")
}

/// The rogue sends `changes` to the host as its own batch.
fn rogue_sends(rogue: &mut Rogue, changes: Vec<Change>) {
    let msg = ClientMsg::Changes { base_seq: 0, batch_id: 41, changes };
    rogue.session.send_command(CHANGES.into(), wire::encode(&msg).unwrap(), None);
}

/// A rogue's batch is refused whole: the rogue alone is told why, and no laptop's board moves.
fn refused_from_rogue(changes: Vec<Change>, why: &str) {
    let mut room = room(1, &[node('s')]);
    room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap();
    room.settle();
    let mut rogue = room.rogue("Mallory");
    let client = seated(&mut room, &mut rogue);
    let before = room.s(0).doc().unwrap().clone();
    rogue_sends(&mut rogue, changes);
    room.churn(&mut rogue, 10);
    let rejects: Vec<(To, String)> = from_host(&room)
        .into_iter()
        .filter_map(|m| match m {
            FromHost::Msg { to, msg: ServerMsg::Reject { reason, .. } } => Some((to, reason)),
            _ => None,
        })
        .collect();
    assert_eq!(rejects.len(), 1, "one refusal: {rejects:?}");
    assert_eq!(rejects[0].0, To::One(client), "told to the rogue only");
    assert!(rejects[0].1.contains(why), "named: {}", rejects[0].1);
    assert!(*room.s(0).doc().unwrap() == before, "the host's board did not move");
    room.check();
}

/// B1: a kind of layer a board never has.
#[test]
fn a_rogue_layer_kind_is_refused() {
    let page = NodeId(1);
    let kind = Json(serde_json::to_value(NodeKind::Slice).unwrap());
    refused_from_rogue(vec![Change::Create { id: NodeId(9_999), kind, parent: Some(page), position: "a0".into(), props: Vec::new() }], "a slice layer");
}

/// B1: a document property a board never shares.
#[test]
fn a_rogue_document_property_is_refused() {
    refused_from_rogue(vec![Change::SetDoc { prop: DocProp::ColorProfile, value: Json(json!("displayP3")) }], "the document's ColorProfile");
}

/// B1: a board stays a board.
#[test]
fn a_rogue_cannot_turn_the_board_into_a_design_file() {
    refused_from_rogue(vec![Change::SetDoc { prop: DocProp::Kind, value: Json(json!("design")) }], "a design file");
}

/// B2: a ballot under someone else's key.
#[test]
fn a_rogue_cannot_vote_for_another() {
    refused_from_rogue(vec![Change::Put { entry: Entry::Ballot("peer 1".into()), value: Json(json!({"choices": []})) }], "another voter's key");
}

/// B4: the name a node calls itself is never shown; the host's name for it is.
#[test]
fn a_rogue_cannot_pass_as_the_host() {
    let mut room = room(1, &[node('s')]);
    let mut rogue = room.rogue("Host");
    let client = seated(&mut room, &mut rogue);
    rogue.session.share_cursor(Some(serde_json::to_string(&figcraft_collab::Presence::default()).unwrap()));
    room.churn(&mut rogue, 10);
    for i in 0..2 {
        let peer = room.s(i).live().peers.get(&client).cloned().unwrap_or_else(|| panic!("person {i} sees the rogue"));
        assert_eq!(peer.name, short(&rogue.node), "person {i} shows the seated name");
    }
}

/// R4: a holder never builds a `Hello` (the host builds every one), and no seat's server welcome,
/// with its resume secret, ever leaves the host.
#[test]
fn no_server_welcome_leaves_the_host() {
    let mut room = room(2, &[node('s')]);
    let mut rogue = room.rogue("Mallory");
    seated(&mut room, &mut rogue);
    let hello = figcraft_collab::Hello {
        protocol: figcraft_collab::message::PROTOCOL,
        file: String::new(),
        token: String::new(),
        name: "Host".into(),
        color: String::new(),
        client_version: figcraft_collab::message::client_version(),
        resume: None,
    };
    rogue.session.send_command(CHANGES.into(), wire::encode(&ClientMsg::Hello(hello)).unwrap(), None);
    room.churn(&mut rogue, 10);
    let all = from_host(&room);
    assert!(all.iter().any(|m| matches!(m, FromHost::Seated { .. })), "seats did go out");
    assert!(!all.iter().any(|m| matches!(m, FromHost::Msg { msg: ServerMsg::Welcome { .. }, .. })), "a server welcome went out");
    assert_eq!(room.s(0).live().peers.len(), 3, "the rogue's Hello seated nobody more");
    room.check();
}

/// B6: a holder loads the host's board only, whoever answers its snapshot request first.
#[test]
fn a_late_joiner_takes_the_hosts_board_only() {
    let mut room = room(1, &[node('r'), node('t')]);
    room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "the host's"})).unwrap();
    room.settle();
    let mut rogue = room.rogue("Mallory");
    seated(&mut room, &mut rogue);
    let host = room.host.clone();
    let tap = room.next_tap();
    *tap.hold.lock().unwrap() = Some(Box::new(move |m| m.author == host && matches!(m.kind, Kind::Snapshot { .. })));
    let joiner = room.join();
    // The rogue answers the joiner's request with a board of its own, while the host's is held.
    let mut forged = board("Forged");
    forged.execute("board.sticky", &json!({"x": 9, "y": 9, "text": "the rogue's"})).unwrap();
    let state = Server::new(forged.doc().unwrap().clone(), "", Limits::default()).state().unwrap();
    let seat = wire::Seat { node: node('t'), client_id: 9, name: "You".into(), color: String::new(), access: figcraft_collab::Access::Edit, processed: Default::default() };
    for _ in 0..20 {
        for got in rogue.session.tick() {
            if matches!(got, Inbound::SnapshotWanted) {
                let w = Welcome { session: state.session, seq: state.seq, snapshot: Replica::new(forged.doc().unwrap().clone()).snapshot().unwrap(), seats: vec![seat.clone()] };
                rogue.session.answer_snapshot(wire::encode(&w).unwrap());
            }
        }
        room.settle();
    }
    assert!(tap.holding() > 0, "the host's snapshot was held back");
    tap.release();
    *tap.hold.lock().unwrap() = None;
    room.churn(&mut rogue, 10);
    assert!(room.s(joiner).live().client().is_some(), "the joiner is in");
    room.check();
}

/// B6: a holder applies an ordered stream from the host only; a rogue's `board.server` moves nothing.
#[test]
fn a_rogue_cannot_order_changes() {
    let mut room = room(1, &[node('s')]);
    room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap();
    room.settle();
    let mut rogue = room.rogue("Mallory");
    seated(&mut room, &mut rogue);
    // A real ordered batch, copied from what the host sent, with the next position and a new name.
    let copied = from_host(&room)
        .into_iter()
        .rev()
        .find_map(|m| match m {
            FromHost::Msg { msg: ServerMsg::Changes(a), .. } => Some(a),
            _ => None,
        })
        .expect("the host ordered a batch");
    let target = copied.changes.iter().find_map(Change::node).expect("a node in it");
    let forged = figcraft_collab::Accepted {
        seq: copied.seq + 1_000,
        author: copied.author,
        batch_id: copied.batch_id + 1,
        changes: vec![Change::Set { id: target, prop: figcraft_collab::Prop::Name, value: Json(json!("forged")) }],
    };
    let mut forged_next = forged.clone();
    forged_next.seq = room.s(1).live().client().unwrap().seq() + 1;
    for a in [forged, forged_next] {
        let m = FromHost::Msg { to: To::All, msg: ServerMsg::Changes(a) };
        rogue.session.send_command(SERVER.into(), wire::encode(&m).unwrap(), None);
    }
    room.churn(&mut rogue, 10);
    let guest = room.s(1).doc().unwrap();
    assert_ne!(guest.node(target).unwrap().name, "forged", "the rogue's order was applied");
    room.check();
}
