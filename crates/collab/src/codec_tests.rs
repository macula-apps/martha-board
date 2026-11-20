//! Message encodings: round trips, hostile input, and the sizes of typical messages.

use figcraft_doc::{Document, NodeId, NodeKind};

use crate::change::{Change, Json, Prop};
use crate::message::{
    Accepted, Access, CatchUp, ClientMsg, EditingText, Hello, PROTOCOL, Peer, Presence, Processed, Resume, ServerMsg, Signal, WIRE_VERSION, decode,
    decode_json, encode, encode_json,
};
use crate::sim::Rng;
use crate::{Error, Replica, diff};

#[test]
fn encoding_into_a_prefix_keeps_the_wire_body() {
    let msg = moved_layer();
    let prefix = b"container header".to_vec();
    let bytes = crate::message::encode_into(&msg, prefix.clone()).unwrap();
    assert_eq!(&bytes[..prefix.len()], prefix);
    assert_eq!(&bytes[prefix.len()..], encode(&msg).unwrap());
    assert_eq!(decode::<ClientMsg>(&bytes[prefix.len()..], bytes.len()).unwrap(), msg);
}

/// A typical edit: one layer dragged (its transform).
fn moved_layer() -> ClientMsg {
    let mut doc = Document::new("f");
    let page = doc.first_page().unwrap();
    doc.set_client_id(3);
    let r = doc.create(page, NodeKind::Rectangle, "Rectangle 1", 120.0, 80.0, 100.0, 60.0).unwrap();
    let a = Replica::new(doc);
    let mut b = a.document().clone();
    b.get_mut(r).unwrap().transform = figcraft_geom::Affine::translate((164.5, 92.25));
    ClientMsg::Changes { base_seq: 1234, batch_id: 56, changes: diff(&a, &b).unwrap().changes }
}

fn cursor() -> ClientMsg {
    ClientMsg::Presence(Presence { cursor: Some((NodeId(1), 512.5, 384.25)), ..Default::default() })
}

fn samples() -> (Vec<ClientMsg>, Vec<ServerMsg>) {
    let peer = Peer { client_id: 4, name: "Ada".into(), color: "#FF8800".into() };
    let set = Change::Set { id: NodeId((4 << 32) | 9), prop: Prop::Name, value: Json::of("Café ☕").unwrap() };
    let client = vec![
        ClientMsg::Hello(Hello {
            protocol: PROTOCOL,
            file: "f".into(),
            token: "t".into(),
            name: "Ada".into(),
            color: "#FF8800".into(),
            client_version: "0.2.0".into(),
            resume: Some(Resume { session: 77, client_id: 4, seq: 10, secret: 0x5EC2E7 }),
        }),
        moved_layer(),
        cursor(),
        ClientMsg::Presence(Presence {
            cursor: None,
            selection: Some(vec![NodeId(2), NodeId(3)]),
            viewport: Some((NodeId(1), 0.0, 0.0, 1200.0, 800.0)),
            editing_text: Some(Box::new(EditingText { node: NodeId(7), caret: 3, selection: Some((1, 3)) })),
        }),
        ClientMsg::Ack { seq: 9 },
        ClientMsg::Ping,
        ClientMsg::Signal(Signal::chat("Hi ☕")),
        ClientMsg::Signal(Signal::spotlight(true)),
    ];
    let snapshot = Replica::new(Document::new("Snap")).snapshot().unwrap();
    let server = vec![
        ServerMsg::Welcome {
            session: 77,
            client_id: 4,
            seq: 10,
            catch_up: CatchUp::Snapshot(snapshot),
            processed: Processed::default(),
            peers: vec![peer.clone()],
            access: Access::Edit,
            secret: 0x5EC2E7,
        },
        ServerMsg::Welcome {
            session: 77,
            client_id: 4,
            seq: 11,
            catch_up: CatchUp::Delta(vec![Accepted { seq: 11, author: 5, batch_id: 1, changes: vec![set.clone()] }]),
            processed: Processed { last_batch: 7, refused: vec![3, 6] },
            peers: vec![],
            access: Access::View,
            secret: 1,
        },
        ServerMsg::Changes(Accepted { seq: 11, author: 4, batch_id: 2, changes: vec![set.clone(), Change::Delete { id: NodeId(5) }] }),
        ServerMsg::Reject { batch_id: 3, reason: "no".into(), fixes: vec![set] },
        ServerMsg::Presence { client_id: 4, presence: Presence::default() },
        ServerMsg::PeerJoined(peer),
        ServerMsg::PeerLeft { client_id: 4 },
        ServerMsg::Refused { reason: "full".into() },
        ServerMsg::Pong,
        ServerMsg::Signal { client_id: 4, signal: Signal::chat("Hi ☕") },
    ];
    (client, server)
}

#[test]
fn every_message_round_trips_in_both_forms() {
    let (client, server) = samples();
    for m in &client {
        assert_eq!(&decode::<ClientMsg>(&encode(m).unwrap(), usize::MAX).unwrap(), m);
        assert_eq!(&decode_json::<ClientMsg>(&encode_json(m).unwrap(), usize::MAX).unwrap(), m);
    }
    for m in &server {
        assert_eq!(&decode::<ServerMsg>(&encode(m).unwrap(), usize::MAX).unwrap(), m);
        assert_eq!(&decode_json::<ServerMsg>(&encode_json(m).unwrap(), usize::MAX).unwrap(), m);
    }
    // The JSON form is readable: the type, then its fields; values as JSON.
    let json = encode_json(&moved_layer()).unwrap();
    assert!(json.starts_with(r#"{"changes":{"base_seq":1234,"batch_id":56,"changes":[{"set":"#), "{json}");
    assert!(json.contains(r#""prop":"transform","value":[1.0,0.0,0.0,1.0,164.5,92.25]"#), "{json}");
    let chat = encode_json(&ClientMsg::Signal(Signal::chat("hi"))).unwrap();
    assert_eq!(chat, r#"{"signal":{"kind":"chat","text":"hi"}}"#);
    // The binary form starts with the wire version and carries values as JSON text.
    let bin = encode(&moved_layer()).unwrap();
    assert_eq!(bin[0], WIRE_VERSION);
    assert!(bin.windows(5).any(|w| w == b"164.5"));
}

#[test]
fn hostile_bytes_are_refused_without_panicking() {
    assert!(matches!(decode::<ClientMsg>(&[], 100), Err(Error::Decode(_))));
    assert!(matches!(decode::<ClientMsg>(&[WIRE_VERSION + 1, 4], 100), Err(Error::Decode(_))));
    let ping = encode(&ClientMsg::Ping).unwrap();
    let mut trailing = ping.clone();
    trailing.push(0);
    assert!(decode::<ClientMsg>(&trailing, 100).is_err(), "trailing bytes");
    assert!(decode::<ClientMsg>(&ping, 1).is_err(), "over the size limit");
    assert!(decode_json::<ClientMsg>("{\"ping\":null}", 3).is_err(), "over the size limit");
    // A huge length prefix doesn't allocate it.
    let mut huge = vec![WIRE_VERSION, 1];
    huge.extend([0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    assert!(decode::<ClientMsg>(&huge, 1 << 20).is_err());
    // Deeply nested JSON inside a value is refused by the JSON parser's depth limit.
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let evil = format!(r#"{{"changes":{{"base_seq":0,"batch_id":0,"changes":[{{"set":{{"id":1,"prop":"name","value":{deep}}}}}]}}}}"#);
    assert!(decode_json::<ClientMsg>(&evil, usize::MAX).is_err());
    // The same value in a binary message (JSON text inside).
    let mut bin = encode(&ClientMsg::Changes {
        base_seq: 0,
        batch_id: 0,
        changes: vec![Change::Set { id: NodeId(1), prop: Prop::Name, value: Json::of("x").unwrap() }],
    })
    .unwrap();
    let at = bin.windows(3).position(|w| w == b"\"x\"").unwrap();
    bin.truncate(at - 1);
    let mut len = deep.len();
    while len >= 0x80 {
        bin.push((len as u8 & 0x7F) | 0x80);
        len >>= 7;
    }
    bin.push(len as u8);
    bin.extend(deep.as_bytes());
    assert!(matches!(decode::<ClientMsg>(&bin, usize::MAX), Err(Error::Decode(_))));
}

#[test]
fn fuzzed_messages_never_panic() {
    let (client, server) = samples();
    let valid: Vec<Vec<u8>> = client.iter().map(|m| encode(m).unwrap()).chain(server.iter().map(|m| encode(m).unwrap())).collect();
    let mut rng = Rng::new(42);
    for round in 0..20_000 {
        let mut bytes =
            if round % 4 == 0 { (0..rng.below(64)).map(|_| rng.next_u64() as u8).collect() } else { rng.pick(&valid).cloned().unwrap_or_default() };
        // Flip, insert or cut a few bytes.
        for _ in 0..1 + rng.below(4) {
            if bytes.is_empty() {
                break;
            }
            let i = rng.below(bytes.len());
            match rng.below(3) {
                0 => bytes[i] = rng.next_u64() as u8,
                1 => bytes.insert(i, rng.next_u64() as u8),
                _ => bytes.truncate(i.max(1)),
            }
        }
        let _ = decode::<ClientMsg>(&bytes, 1 << 20);
        let _ = decode::<ServerMsg>(&bytes, 1 << 20);
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let _ = decode_json::<ClientMsg>(text, 1 << 20);
        }
    }
}

#[test]
fn message_sizes() {
    // Recorded in docs/collab-protocol.md (Encoding). Binary stays well under JSON.
    let edit = moved_layer();
    let (eb, ej) = (encode(&edit).unwrap().len(), encode_json(&edit).unwrap().len());
    let cur = cursor();
    let (cb, cj) = (encode(&cur).unwrap().len(), encode_json(&cur).unwrap().len());
    println!("typical edit (move a layer): binary {eb} B, JSON {ej} B; cursor update: binary {cb} B, JSON {cj} B");
    assert!(eb < ej && cb < cj);
    assert!(cb <= 32, "a cursor update is tiny: {cb}");
    assert!(eb <= 96, "a move is small: {eb}");
}

/// The tag (postcard's variant index, the byte after the wire version) of every message kept when
/// the server-only ones left (martha-board#41) and join codes did (martha-board#43): each keeps
/// the number it had, so a kept message's bytes are what they always were. The gaps are the retired tags.
#[test]
fn every_kept_message_keeps_its_tag() {
    let (client, server) = samples();
    let tag = |bytes: Vec<u8>| bytes[1];
    let client_tag = |m: &ClientMsg| match m {
        ClientMsg::Hello(_) => 0,
        ClientMsg::Changes { .. } => 1,
        ClientMsg::Presence(_) => 2,
        ClientMsg::Ack { .. } => 3,
        ClientMsg::Ping => 4,
        ClientMsg::Signal(_) => 9,
        _ => panic!("a retired message can't be made"),
    };
    let server_tag = |m: &ServerMsg| match m {
        ServerMsg::Welcome { .. } => 0,
        ServerMsg::Changes(_) => 1,
        ServerMsg::Reject { .. } => 2,
        ServerMsg::Presence { .. } => 3,
        ServerMsg::PeerJoined(_) => 4,
        ServerMsg::PeerLeft { .. } => 5,
        ServerMsg::Refused { .. } => 6,
        ServerMsg::Pong => 7,
        ServerMsg::Signal { .. } => 10,
        _ => panic!("a retired message can't be made"),
    };
    let mut kept = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    for m in &client {
        assert_eq!(tag(encode(m).unwrap()), client_tag(m), "{m:?}");
        kept.0.insert(client_tag(m));
    }
    for m in &server {
        assert_eq!(tag(encode(m).unwrap()), server_tag(m), "{m:?}");
        kept.1.insert(server_tag(m));
    }
    // Every kept message is in the samples (and round-trips there).
    assert_eq!(kept.0.into_iter().collect::<Vec<_>>(), [0, 1, 2, 3, 4, 9]);
    assert_eq!(kept.1.into_iter().collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6, 7, 10]);
}

/// A frame carrying a tag that left with martha-board#41 or #43 reads as [`Error::Retired`] with
/// that tag: not a panic, and never another message. The tags are the variant indexes as they were
/// on main at 4f2f3956 (`crates/collab/src/message.rs`, counted from 0 in declaration order):
/// `ClientMsg` 5 `Manage`, 6 `Admin`, 7 `OpenLibrary`, 8 `Library`, 10 `OpenPlugins`, 11 `Plugins`,
/// 12 `OpenHosting`, 13 `Report`, 14 `Resolve` (#43), 15 `ResolveTeam`; `ServerMsg` 8 `Admin`,
/// 9 `Library`, 11 `Plugins`, 12 `Hosting`, 13 `Reported`, 14 `Resolved` (#43). Each frame is
/// the wire version, the tag, then a payload as its old message would carry one (a string's
/// length and bytes; nothing for some).
#[test]
fn a_retired_tag_reads_as_a_named_error() {
    let frame = |tag: u8, payload: &[u8]| [&[WIRE_VERSION, tag][..], payload].concat();
    let reason = [1, b'x'];
    for tag in [5, 6, 7, 8, 10, 11, 12, 13, 14, 15] {
        for payload in [&[][..], &reason[..]] {
            assert_eq!(decode::<ClientMsg>(&frame(tag, payload), 1 << 20), Err(Error::Retired(u32::from(tag))), "ClientMsg tag {tag}");
        }
    }
    for tag in [8, 9, 11, 12, 13, 14] {
        for payload in [&[][..], &reason[..]] {
            assert_eq!(decode::<ServerMsg>(&frame(tag, payload), 1 << 20), Err(Error::Retired(u32::from(tag))), "ServerMsg tag {tag}");
        }
    }
    // A tag past the last one is unknown, not retired; and a kept message after a retired one
    // reads as itself.
    assert!(matches!(decode::<ClientMsg>(&frame(16, &[]), 1 << 20), Err(Error::Decode(_))));
    assert!(matches!(decode::<ServerMsg>(&frame(15, &[]), 1 << 20), Err(Error::Decode(_))));
    assert_eq!(decode::<ClientMsg>(&frame(4, &[]), 1 << 20), Ok(ClientMsg::Ping));
    assert_eq!(decode::<ServerMsg>(&frame(7, &[]), 1 << 20), Ok(ServerMsg::Pong));
}
