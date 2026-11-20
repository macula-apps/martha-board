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
        .filter_map(|m| match m.kind {
            Kind::Command { params, .. } => Some(wire::decode(&params, room.ceilings.server).unwrap()),
            _ => None,
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
    refused_from_rogue(
        vec![Change::Create { id: NodeId(9_999), kind, parent: Some(page), position: "a0".into(), props: Vec::new() }],
        "a slice layer",
    );
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
    let mut answered = 0;
    let state = Server::new(forged.doc().unwrap().clone(), "", Limits::default()).state().unwrap();
    let seat = wire::Seat {
        node: node('t'),
        client_id: 9,
        name: "You".into(),
        color: String::new(),
        access: figcraft_collab::Access::Edit,
        processed: Default::default(),
    };
    // The joiner's first ask may come before the rogue saw its grant; the seam asks again later.
    let end = macula_craft_seam::now_ms() + 15_000;
    while answered == 0 && macula_craft_seam::now_ms() < end {
        std::thread::sleep(std::time::Duration::from_millis(20));
        for got in room.churn(&mut rogue, 1) {
            if matches!(got, Inbound::SnapshotWanted) {
                let w = Welcome {
                    session: state.session,
                    seq: state.seq,
                    snapshot: Replica::new(forged.doc().unwrap().clone()).snapshot().unwrap(),
                    seats: vec![seat.clone()],
                };
                rogue.session.answer_snapshot(wire::encode(&w).unwrap());
                answered += 1;
            }
        }
    }
    assert!(answered > 0, "the rogue answered the joiner");
    assert!(tap.holding() > 0, "the host's snapshot was held back");
    assert!(room.s(joiner).live().client().is_none(), "the joiner took the rogue's board");
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
    // Exactly the next position: a gap would make the holder ask the host for its board again.
    let forged = figcraft_collab::Accepted {
        seq: room.s(1).live().client().unwrap().seq() + 1,
        author: copied.author,
        batch_id: copied.batch_id + 1,
        changes: vec![Change::Set { id: target, prop: figcraft_collab::Prop::Name, value: Json(json!("forged")) }],
    };
    let m = FromHost::Msg { to: To::All, msg: ServerMsg::Changes(forged) };
    rogue.session.send_command(SERVER.into(), wire::encode(&m).unwrap(), None);
    room.churn(&mut rogue, 10);
    let guest = room.s(1).doc().unwrap();
    assert_ne!(guest.node(target).unwrap().name, "forged", "the rogue's order was applied");
    room.check();
}

/// Whether every laptop holds the host's board now (the soft form of [`Room::check`], for waiting).
fn all_equal(room: &Room) -> bool {
    let Some(host) = room.people[0].s.live().client() else { return false };
    room.people.iter().all(|p| p.s.live().client().is_some_and(|c| c.pending() == 0 && c.view() == host.view()))
}

/// R3: a board holding what a board never shares is refused at the start, each thing named.
#[test]
fn a_board_with_an_image_and_a_style_is_not_shared() {
    use figcraft_doc::color::{ImageScaleMode, Paint};
    let mut s = board("Storm");
    let sticky = id_of(&s.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap());
    s.execute("style.create", &json!({"type": "paint", "name": "Brand", "from": sticky})).unwrap();
    s.doc_mut().unwrap().node_mut(NodeId(sticky)).unwrap().fills = vec![Paint::image("cafe", ImageScaleMode::Fill)];
    let (link, _) = crate::seam_link_harness::opened_link(&node('a'), real_ceilings(&node('a')));
    s.live.install(link);
    let err = s.execute("collab.host", &json!({})).unwrap_err().to_string();
    assert!(err.contains("an image") && err.contains("a style"), "both named: {err}");
    assert!(s.live().client().is_none(), "nothing was shared");
}

/// The host removes a holder: the seam revokes its node, every laptop sees it go, and it keeps its copy.
#[test]
fn the_host_removes_a_holder() {
    let mut room = room(2, &[]);
    let gone = room.s(2).live().client().unwrap().id();
    room.s(0).execute("collab.removePerson", &json!({"client": gone})).unwrap();
    room.settle();
    assert!(!room.s(0).live().peers.contains_key(&gone), "the host's list");
    assert!(!room.s(1).live().peers.contains_key(&gone), "the other holder's list");
    // What the removed holder sends after is not ordered.
    let before = room.s(0).doc().unwrap().clone();
    let _ = room.s(2).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "late"}));
    room.settle();
    assert!(*room.s(0).doc().unwrap() == before, "a removed holder's act reached the host");
    assert!(room.s(2).doc().is_ok(), "the removed holder keeps its copy");
}

/// A holder that comes back from the same node (a restarted app) takes its old seat and id.
#[test]
fn a_rejoin_from_the_same_node_keeps_its_id() {
    let mut room = room(1, &[node('b')]);
    let id = room.s(1).live().client().unwrap().id();
    // The app on that node restarted: its old session is gone. A restart is a new process at a
    // later millisecond, so the seam gives it a new epoch; in one test process that takes a wait.
    room.people.remove(1);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let again = room.join();
    assert_eq!(again, 1);
    room.settle();
    assert_eq!(room.s(again).live().client().unwrap().id(), id);
    room.s(again).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "back"})).unwrap();
    room.settle();
    room.check();
}

/// R1: an ordered batch lost on its way to a holder: the seam notices the hole, and the holder
/// ends with the host's board (the seam's stall runs on the clock).
#[test]
fn a_lost_batch_is_recovered() {
    let mut room = room(1, &[]);
    let host = room.host.clone();
    let mut lost = 0;
    *room.people[1].tap.drop.lock().unwrap() = Some(Box::new(move |m| {
        let hit = lost == 0 && m.author == host && matches!(&m.kind, Kind::Command { id, .. } if id == SERVER);
        lost += usize::from(hit);
        hit
    }));
    room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "lost"})).unwrap();
    room.settle();
    room.s(0).execute("board.sticky", &json!({"x": 300, "y": 0, "text": "after"})).unwrap();
    assert_eq!(room.people[1].tap.dropped.lock().unwrap().len(), 1, "one batch was lost");
    assert!(room.settle_until(20_000, |r| all_equal(r)), "the holder caught up");
    room.check();
}

/// M1: batches ordered while a joiner's board was on its way are not in the board it got; the next
/// one shows the hole, and the joiner asks for the board again instead of applying past it.
#[test]
fn a_joiner_that_missed_batches_asks_again() {
    let mut room = room(1, &[node('t')]);
    let host = room.host.clone();
    let tap = room.next_tap();
    *tap.hold.lock().unwrap() = Some(Box::new(move |m| m.author == host && matches!(m.kind, Kind::Snapshot { .. })));
    let joiner = room.join();
    assert!(room.settle_until(10_000, |r| r.people[2].tap.holding() > 0), "the host's board was held");
    // The host's board, as it was when asked for, is on its way; two batches pass it.
    room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "one"})).unwrap();
    room.s(0).execute("board.sticky", &json!({"x": 300, "y": 0, "text": "two"})).unwrap();
    room.settle();
    *tap.hold.lock().unwrap() = None;
    tap.release();
    room.settle();
    room.s(0).execute("board.sticky", &json!({"x": 600, "y": 0, "text": "three"})).unwrap();
    assert!(room.settle_until(20_000, |r| all_equal(r)), "the joiner caught up");
    assert!(room.s(joiner).live().client().is_some());
    room.check();
}

/// A 1×1 PNG, as `image.place` takes it.
const ONE_PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// What person `i` sent on the seam so far.
fn sent_count(room: &Room, i: usize) -> usize {
    room.people[i].tap.sent.lock().unwrap().len()
}

/// R3 while shared: an act that adds what a board never shares is put back on the laptop it was
/// made on, said by name, and nothing of it reaches the stream.
fn refused_locally(i: usize, cmd: &str, params: serde_json::Value, why: &str) {
    let mut room = room(1, &[]);
    room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap();
    room.settle();
    let before = room.s(i).doc().unwrap().clone();
    let sent = sent_count(&room, i);
    let _ = room.s(i).execute(cmd, &params);
    assert!(*room.s(i).doc().unwrap() == before, "the act was put back");
    let said = room.s(i).message.clone().unwrap_or_default();
    assert!(said.contains(why), "said by name: {said}");
    room.settle();
    assert_eq!(sent_count(&room, i), sent, "nothing went out");
    room.check();
}

#[test]
fn the_host_cannot_add_an_image_while_shared() {
    refused_locally(0, "image.place", json!({"data": ONE_PIXEL, "x": 0, "y": 0}), "an image");
}

#[test]
fn a_holder_cannot_add_a_style_while_shared() {
    let mut room = room(1, &[]);
    let a = id_of(&room.s(0).execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap());
    room.settle();
    let before = room.s(1).doc().unwrap().clone();
    let sent = sent_count(&room, 1);
    let _ = room.s(1).execute("style.create", &json!({"type": "paint", "name": "Brand", "from": a}));
    assert!(*room.s(1).doc().unwrap() == before, "the style was put back");
    assert!(room.s(1).message.clone().unwrap_or_default().contains("a style"));
    room.settle();
    assert_eq!(sent_count(&room, 1), sent, "nothing went out");
    room.check();
}

/// The agent commands: an invite names one node, a revoke takes it out, the numbers count it.
#[test]
fn invite_revoke_and_stats_by_command() {
    let mut room = room(1, &[]);
    let refused = room.s(0).execute("collab.invite", &json!({})).unwrap_err().to_string();
    assert!(refused.contains("forNode"), "{refused}");
    assert!(room.s(0).execute("collab.invite", &json!({"forNode": node('e'), "role": "owner"})).is_err());
    let line = room.s(0).execute("collab.invite", &json!({"forNode": node('e'), "forName": "Eve", "role": "viewer"})).unwrap();
    assert!(!line["invite"].as_str().unwrap().is_empty());
    let stats = room.s(0).execute("collab.stats", &json!({})).unwrap();
    assert_eq!(stats["role"], "host");
    assert!(stats["holders"].as_array().unwrap().iter().any(|h| *h == json!(node('b'))), "{stats}");
    room.s(0).execute("collab.revoke", &json!({"node": node('b')})).unwrap();
    room.settle();
    assert!(room.s(0).live().peers.is_empty(), "the revoked holder left");
    assert!(room.s(1).execute("collab.revoke", &json!({"node": node('a')})).is_err(), "only the host revokes");
}

/// R2: a message exactly at its ceiling goes; one byte over is refused, naming both sizes.
#[test]
fn a_ceiling_is_exact() {
    use crate::seam_link::ceiling;
    let at = real_ceilings(&node('a')).welcome;
    assert!(ceiling::fits(ceiling::WELCOME, at, at).is_ok());
    let over = ceiling::fits(ceiling::WELCOME, at + 1, at).unwrap_err();
    assert!(over.contains(&format!("{} bytes", at + 1)) && over.contains(&format!("{at} bytes")), "{over}");
    assert!(over.starts_with("the board is"), "named: {over}");
}

/// R2 at the start: a board over its welcome ceiling is refused by name, and nothing is sent.
#[test]
fn a_board_over_its_ceiling_is_not_shared() {
    let host = node('a');
    let mut s = board("Storm");
    s.execute("board.sticky", &json!({"x": 0, "y": 0, "text": "A"})).unwrap();
    let mut small = real_ceilings(&host);
    small.welcome = 64;
    let (link, tap) = crate::seam_link_harness::opened_link(&host, small);
    s.live.install(link);
    let err = s.execute("collab.host", &json!({})).unwrap_err().to_string();
    assert!(err.contains("the board is") && err.contains("over the 64 bytes"), "{err}");
    let sent: Vec<String> = tap.sent.lock().unwrap().iter().map(|m| format!("{:?}", m.kind).chars().take(80).collect()).collect();
    assert!(sent.is_empty(), "nothing was sent: {sent:?}");
}

/// R2 while shared: every board the host sends a joiner fits the ceiling it was measured against.
#[test]
fn every_snapshot_sent_fits_its_ceiling() {
    let mut room = room(2, &[]);
    room.check();
    let sent = room.sent(0, |k| matches!(k, Kind::Snapshot { .. }));
    assert!(!sent.is_empty(), "the host answered");
    for m in sent {
        let Kind::Snapshot { data, .. } = m.kind else { continue };
        assert!(data.len() <= room.ceilings.welcome, "{} over {}", data.len(), room.ceilings.welcome);
    }
}
