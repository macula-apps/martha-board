//! Engine sessions sharing a board over the seam's in-memory bus, for the seam link's tests: one
//! host and its holders, each a whole FigCraft session with a [`SeamLink`] installed. A tap on
//! each transport records what that node sent and can lose what it receives.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use figcraft_engine::Session;
use macula_craft_seam::{
    Access, CollabSession, GRANT_VERSION, Grant, GrantIssuer, GrantVerifier, Kind, MemoryTransport, Message, Revocation, Role, SignedGrant,
    SignedRevocation, Transport, mint_nonce, now_ms,
};
use serde_json::json;

use crate::seam_link::{Ceilings, Opened, Opener, SeamLink};

/// The share's document id on the seam.
pub const DOC: &str = "board";

/// A node id: 64 hex digits of `c`.
pub fn node(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

/// Accepts every grant as it is (the mesh signs them; the seam's own tests do the same).
struct TestVerifier;

impl GrantVerifier for TestVerifier {
    fn verify(&self, signed: &SignedGrant, _: u64) -> Result<Grant, String> {
        Ok(signed.grant.clone())
    }
    fn verify_revocation(&self, signed: &SignedRevocation, _: u64) -> Result<Revocation, String> {
        Ok(signed.revocation.clone())
    }
}

/// Mints real, unsigned grant structures as the host.
struct TestIssuer(String);

impl GrantIssuer for TestIssuer {
    fn node_id(&self) -> String {
        self.0.clone()
    }
    fn carried_key(&self) -> Vec<u8> {
        vec![7; 32]
    }
    fn issue(&self, doc: &str, holder: &str, role: Role, ttl_ms: u64, now_ms: u64) -> Result<SignedGrant, String> {
        let grant = Grant {
            v: GRANT_VERSION,
            doc: doc.to_string(),
            host: self.0.clone(),
            holder: holder.to_string(),
            role,
            issued_ms: now_ms,
            expires_ms: now_ms.saturating_add(ttl_ms),
            nonce: mint_nonce(now_ms, 1),
        };
        Ok(SignedGrant { grant, signature: Vec::new() })
    }
    fn revoke(&self, doc: &str, holder: &str, now_ms: u64) -> Result<SignedRevocation, String> {
        let revocation =
            Revocation { v: GRANT_VERSION, doc: doc.to_string(), host: self.0.clone(), holder: holder.to_string(), issued_ms: now_ms, nonce: mint_nonce(now_ms, 1) };
        Ok(SignedRevocation { revocation, signature: Vec::new() })
    }
    fn open_token(&self, doc: &str, role: Role, ttl_ms: u64, now_ms: u64) -> Result<SignedGrant, String> {
        self.issue(doc, "", role, ttl_ms, now_ms)
    }
    fn sign(&self, _: &[u8]) -> Result<Vec<u8>, String> {
        Ok(b"sig".to_vec())
    }
}

fn table(host: &str) -> Access {
    Access::new(DOC, host, Arc::new(TestVerifier))
}

/// What a tap drops from what its node receives: the `n`th next message that `matches`.
pub type DropRule = Box<dyn FnMut(&Message) -> bool + Send>;

/// One node's transport, tapped.
#[derive(Clone, Default)]
pub struct Tap {
    /// Every message this node sent, in order.
    pub sent: Arc<Mutex<Vec<Message>>>,
    /// What this node loses on its way in, while set.
    pub drop: Arc<Mutex<Option<DropRule>>>,
    /// What it lost.
    pub dropped: Arc<Mutex<Vec<Message>>>,
    /// What this node holds back on its way in, while set, until [`Tap::release`].
    pub hold: Arc<Mutex<Option<DropRule>>>,
    held: Arc<Mutex<Vec<Message>>>,
    released: Arc<Mutex<bool>>,
}

impl Tap {
    /// What was held back arrives with the next poll, first.
    pub fn release(&self) {
        *self.released.lock().unwrap() = true;
    }

    /// How many messages are held back now.
    pub fn holding(&self) -> usize {
        self.held.lock().unwrap().len()
    }
}

struct Tapped(MemoryTransport, Tap);

impl Transport for Tapped {
    fn author(&self) -> String {
        self.0.author()
    }
    fn send(&mut self, msg: Message) -> Result<(), String> {
        self.1.sent.lock().map_err(|e| e.to_string())?.push(msg.clone());
        self.0.send(msg)
    }
    fn poll(&mut self) -> Vec<Message> {
        let mut kept = Vec::new();
        if std::mem::take(&mut *self.1.released.lock().unwrap()) {
            kept.append(&mut self.1.held.lock().unwrap());
        }
        for msg in self.0.poll() {
            let hold = self.1.hold.lock().ok().and_then(|mut d| d.as_mut().map(|rule| rule(&msg))).unwrap_or(false);
            if hold {
                self.1.held.lock().unwrap().push(msg);
                continue;
            }
            let lose = self.1.drop.lock().ok().and_then(|mut d| d.as_mut().map(|rule| rule(&msg))).unwrap_or(false);
            if lose {
                if let Ok(mut d) = self.1.dropped.lock() {
                    d.push(msg);
                }
            } else {
                kept.push(msg);
            }
        }
        kept
    }
}

/// Opens the share it was handed, once.
struct Handed(Arc<Mutex<VecDeque<Opened>>>);

impl Opener for Handed {
    fn host(&mut self, _: &str, _: &str) -> Result<Opened, String> {
        self.0.lock().map_err(|e| e.to_string())?.pop_front().ok_or_else(|| "no share to open".into())
    }
    fn join(&mut self, _: &str, _: &str) -> Result<Opened, String> {
        self.0.lock().map_err(|e| e.to_string())?.pop_front().ok_or_else(|| "no share to open".into())
    }
}

/// The ceilings of a real share of `host`'s, measured as the app measures them: once per test
/// run per host (sealing to measure takes seconds in a debug build).
pub fn real_ceilings(host: &str) -> Ceilings {
    static MEASURED: Mutex<Vec<(String, Ceilings)>> = Mutex::new(Vec::new());
    let mut measured = MEASURED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, c)) = measured.iter().find(|(h, _)| h == host) {
        return *c;
    }
    let c = measure(host);
    measured.push((host.to_string(), c));
    c
}

fn measure(host: &str) -> Ceilings {
    let prefix = macula_craft_collab::mint_share_prefix("io.macula", host).unwrap();
    let profile = macula_rust::profile::Profile::parse("pq_hybrid").unwrap();
    let crdt = CollabSession::host(DOC, "x", Box::new(MemoryTransport::bus(&[host]).remove(0)), &[]).texts().full_update();
    crate::seam_link::ceiling_measure(&prefix, profile, DOC, host, &crdt).unwrap()
}

/// One person: their FigCraft session, their node and their tap.
pub struct Person {
    pub s: Session,
    pub node: String,
    pub tap: Tap,
}

/// A host and its holders on one bus, and the bus's spare endpoints for nodes that come later.
pub struct Room {
    pub people: Vec<Person>,
    pub host: String,
    pub ceilings: Ceilings,
    spares: VecDeque<(String, Tapped)>,
}

/// A link whose host share is `host`'s own, unshared yet, with `ceilings`.
pub fn opened_link(host: &str, ceilings: Ceilings) -> Box<SeamLink> {
    let t = Tapped(MemoryTransport::bus(&[host]).remove(0), Tap::default());
    let mut session = CollabSession::host(DOC, "Host", Box::new(t), &[]);
    session.set_access(table(host));
    session.set_issuer(Box::new(TestIssuer(host.to_string())));
    Box::new(SeamLink::new(Box::new(Handed(Arc::new(Mutex::new(VecDeque::from([Opened { session, ceilings, host_node: host.to_string() }])))))))
}

/// A link that opens `opened`, installed in a fresh session.
fn session_with(opened: Opened) -> Session {
    let mut s = Session::new();
    s.live.install(Box::new(SeamLink::new(Box::new(Handed(Arc::new(Mutex::new(VecDeque::from([opened]))))))));
    s
}

/// A node the test drives by hand: a granted seam session with no board behind it.
pub struct Rogue {
    pub session: CollabSession,
    pub node: String,
    pub tap: Tap,
}

impl Room {
    /// The host shares `board` (a session with a board open) with `holders` people, each joining
    /// with their own token; every share has `ceilings`. `spares` are more nodes on the bus, for
    /// [`Room::join`] and [`Room::rogue`] (a node may be named twice: a rejoin from the same node).
    pub fn with(mut board: Session, holders: usize, spares: &[String], ceilings: Ceilings) -> Room {
        let mut nodes: Vec<String> = "abcdefgh".chars().take(holders + 1).map(node).collect();
        nodes.extend(spares.iter().cloned());
        let names: Vec<&str> = nodes.iter().map(String::as_str).collect();
        let mut bus: VecDeque<(String, Tapped)> =
            MemoryTransport::bus(&names).into_iter().zip(nodes.iter()).map(|(t, n)| (n.clone(), Tapped(t, Tap::default()))).collect();
        let host = nodes[0].clone();
        let (_, first) = bus.pop_front().unwrap();
        let tap = first.1.clone();
        let mut session = CollabSession::host(DOC, "Host", Box::new(first), &[]);
        session.set_access(table(&host));
        session.set_issuer(Box::new(TestIssuer(host.clone())));
        session.set_auto_approve(true);
        board.live.install(Box::new(SeamLink::new(Box::new(Handed(Arc::new(Mutex::new(VecDeque::from([Opened {
            session,
            ceilings,
            host_node: host.clone(),
        }]))))))));
        board.execute("collab.host", &json!({})).unwrap();
        let rest = bus.split_off(holders);
        let mut room = Room { people: vec![Person { s: board, node: host.clone(), tap }], host, ceilings, spares: rest };
        for spare in bus {
            room.join_on(spare);
        }
        room.settle();
        room
    }

    /// A granted seam session for `node` on transport `t`, joining with its own token.
    fn granted(&self, node: &str, t: Tapped, name: &str) -> CollabSession {
        let token = TestIssuer(self.host.clone()).issue(DOC, node, Role::Editor, 3_600_000, now_ms()).unwrap();
        let mut session = CollabSession::join_requesting(DOC, name, Box::new(t), Some(token), None);
        session.set_access(table(&self.host));
        session
    }

    fn join_on(&mut self, (me, t): (String, Tapped)) -> usize {
        let tap = t.1.clone();
        let session = self.granted(&me, t, &format!("Person {}", self.people.len()));
        let mut s = session_with(Opened { session, ceilings: self.ceilings, host_node: self.host.clone() });
        s.execute("collab.join", &json!({"link": "seam://invite"})).unwrap();
        self.people.push(Person { s, node: me, tap });
        self.people.len() - 1
    }

    /// The next spare node joins with a board of its own; its person number.
    pub fn join(&mut self) -> usize {
        let spare = self.spares.pop_front().expect("a spare node");
        self.join_on(spare)
    }

    /// The next spare node, admitted by hand with no board behind it, named `name`.
    pub fn rogue(&mut self, name: &str) -> Rogue {
        let (me, t) = self.spares.pop_front().expect("a spare node");
        let tap = t.1.clone();
        let session = self.granted(&me, t, name);
        Rogue { session, node: me, tap }
    }

    /// The tap of the next spare node, to set before it joins.
    pub fn next_tap(&self) -> Tap {
        self.spares.front().expect("a spare node").1.1.clone()
    }

    /// `rounds` rounds of the room settling and `rogue` ticking; what the rogue received.
    pub fn churn(&mut self, rogue: &mut Rogue, rounds: usize) -> Vec<macula_craft_seam::Inbound> {
        let mut got = Vec::new();
        for _ in 0..rounds {
            for event in rogue.session.tick() {
                // A modded holder: it takes the board it is given, so it may answer for one.
                if let macula_craft_seam::Inbound::Snapshot { crdt, .. } = &event {
                    rogue.session.snapshot_loaded(crdt);
                }
                got.push(event);
            }
            self.settle();
        }
        got
    }

    /// The session of person `i` (0: the host).
    pub fn s(&mut self, i: usize) -> &mut Session {
        &mut self.people[i].s
    }

    /// Every session polls until a few rounds pass with nothing new.
    pub fn settle(&mut self) {
        let mut quiet = 0;
        for _ in 0..500 {
            let mut any = false;
            for p in &mut self.people {
                any |= p.s.live_poll();
            }
            quiet = if any { 0 } else { quiet + 1 };
            if quiet >= 5 {
                return;
            }
        }
    }

    /// Polls until `done` holds or `ms` milliseconds pass (the seam's own deadlines run on the clock).
    pub fn settle_until(&mut self, ms: u64, mut done: impl FnMut(&mut Room) -> bool) -> bool {
        let end = now_ms() + ms;
        while now_ms() < end {
            for p in &mut self.people {
                p.s.live_poll();
            }
            if done(self) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    /// Everyone holds the host's board, with nothing of theirs waiting.
    pub fn check(&self) {
        let host = self.people[0].s.live().client().expect("the host's client");
        for (i, p) in self.people.iter().enumerate() {
            let c = p.s.live().client().unwrap_or_else(|| panic!("person {i} has no client"));
            assert_eq!(c.pending(), 0, "person {i} has changes waiting");
            assert!(c.view() == host.view(), "person {i}: {}", figcraft_collab::sim::describe(host.view(), c.view()));
            assert!(*p.s.doc().unwrap() == *c.document(), "person {i}: the canvas shows the shared board");
        }
    }

    /// The seam messages node `i` sent of `kind`.
    pub fn sent(&self, i: usize, kind: impl Fn(&Kind) -> bool) -> Vec<Message> {
        self.people[i].tap.sent.lock().unwrap().iter().filter(|m| kind(&m.kind)).cloned().collect()
    }
}

/// A new board session named `name`.
pub fn board(name: &str) -> Session {
    let mut s = Session::new();
    s.execute("file.newBoard", &json!({"name": name})).unwrap();
    s
}

/// The `id` a command answered with.
pub fn id_of(v: &serde_json::Value) -> u64 {
    v["id"].as_u64().unwrap()
}
