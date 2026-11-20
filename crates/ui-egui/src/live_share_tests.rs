//! Sharing in the window: the Share popover not sharing (one Start button, one join field),
//! hosting (the invite link to copy), joining with the one join field (the popover, the home
//! screen, the Join dialog) and its errors, and an idle popover asking for no frames. (Sharing
//! through a server left with martha-board#41; join codes with martha-board#43.)

use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Event, Key, Shape};
use figcraft_engine::figcraft_collab::message::ClientMsg;
use figcraft_engine::live::{Access, Event as LiveEvent, HostOptions, LiveLink, Presence, Role, SessionInfo, Status};

use crate::live_tests::{Harness, has_text, text_center};

/// What the link was asked to do, and the events it hands out next (as the session started last).
#[derive(Default)]
struct Script {
    hosted: Vec<HostOptions>,
    joined: Vec<String>,
    events: Vec<LiveEvent>,
    session: u64,
    copied: Vec<String>,
}

type Shared = Arc<Mutex<Script>>;

struct Link(Shared);

/// The invite line the scripted host gives, and what a guest pastes.
const INVITE: &str = "invite://board?file=f&token=AbCdEfGhIjKlMnOpQrStUv";

/// What the scripted link says about a session you are in as `role`, invited with `link`.
fn info(role: Role, link: &str) -> SessionInfo {
    SessionInfo {
        role,
        me: 1,
        name: "Ada".into(),
        color: "#14AE5C".into(),
        file_name: "Untitled".into(),
        pages: vec![],
        link: link.into(),
        url: String::new(),
        address: "192.168.1.20:47900".into(),
        network: true,
        access: Access::Edit,
    }
}

impl LiveLink for Link {
    fn host(&mut self, session: u64, opts: HostOptions) -> Result<SessionInfo, String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.hosted.push(opts);
        Ok(info(Role::Host, INVITE))
    }
    fn join(&mut self, session: u64, link: &str, _: &str) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.joined.push(link.to_string());
        Ok(())
    }
    fn leave(&mut self, _: u64) {}
    fn presence(&mut self, _: u64, _: &Presence) {}
    fn send(&mut self, _: u64, _: ClientMsg) {}
    fn poll(&mut self) -> Vec<(u64, LiveEvent)> {
        let mut s = self.0.lock().unwrap();
        let session = s.session;
        std::mem::take(&mut s.events).into_iter().map(|e| (session, e)).collect()
    }
}

/// The clipboard, kept where the test can read it.
struct Clipboard(Shared);

impl crate::Services for Clipboard {
    fn clipboard_set_text(&mut self, s: &str) {
        self.0.lock().unwrap().copied.push(s.to_string());
    }
}

/// A window on a file, with the scripted link.
fn harness() -> (Harness, Shared) {
    let (mut h, _) = Harness::new();
    let script = Shared::default();
    h.app.session.live.install(Box::new(Link(script.clone())));
    h.app.services = Box::new(Clipboard(script.clone()));
    h.frames(3);
    (h, script)
}

/// Open the Share popover.
fn open(h: &mut Harness) -> Vec<Shape> {
    let shapes = h.frames(1);
    h.click(text_center(&shapes, "Share"));
    h.frames(2)
}

fn click(h: &mut Harness, shapes: &[Shape], text: &str) -> Vec<Shape> {
    h.click(text_center(shapes, text));
    h.frames(2)
}

fn key(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() }]);
    }
}

/// Type into the field with the keyboard.
fn type_text(h: &mut Harness, text: &str) {
    h.frame(vec![Event::Text(text.into())]);
}

/// The session's document arrives (the host's welcome): it opens in a tab of its own.
fn document_arrives(h: &mut Harness, script: &Shared) {
    use figcraft_engine::figcraft_collab::message::ServerMsg;
    use figcraft_engine::figcraft_collab::{Client, Limits, Outgoing, Server};
    let mut server = Server::new(h.app.session.doc().unwrap().clone(), "", Limits::default());
    let ClientMsg::Hello(me) = Client::hello("f", "", "Ada", "") else { panic!("no hello") };
    let (_, outs) = server.join(me).unwrap();
    let w = outs.into_iter().find_map(|o| if let Outgoing::To(_, w @ ServerMsg::Welcome { .. }) = o { Some(w) } else { None }).unwrap();
    script.lock().unwrap().events.push(LiveEvent::Sync(w));
    h.frames(3);
}

/// Not sharing: one way to start and one to join; no codes, no server, nothing about where it
/// listens. Starting hosts the file, and its invite link is there to copy.
#[test]
fn not_sharing_the_popover_offers_one_way_to_start_and_one_to_join() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    for t in ["Share this file", "Start sharing", "Join with an invite link", "Join", "Export…"] {
        assert!(has_text(&shapes, t), "the popover shows {t:?}");
    }
    for t in ["Online", "Can comment", "Can view", "Advanced", "Port", "This computer only", "Join with a code", "Not in a live session"] {
        assert!(!has_text(&shapes, t), "no {t:?}");
    }
    // Nothing started, nothing asked of the link.
    {
        let s = script.lock().unwrap();
        assert!(s.hosted.is_empty() && s.joined.is_empty());
    }
    assert_eq!(h.app.session.live().status, Status::Off);
    let shapes = click(&mut h, &shapes, "Start sharing");
    assert_eq!(script.lock().unwrap().hosted.len(), 1, "the file is hosted");
    assert!(has_text(&shapes, "Invite link") && has_text(&shapes, "Stop session"));
    click(&mut h, &shapes, "Copy");
    assert_eq!(script.lock().unwrap().copied, [INVITE]);
}

#[test]
fn the_join_field_takes_an_invite_link_and_enter_joins() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    // Click into the field (its hint), paste the link (with the spaces a paste brings), press Enter.
    click(&mut h, &shapes, "Invite link");
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique("live_join_code"))));
    let pasted = format!(" {INVITE} ");
    type_text(&mut h, &pasted);
    assert_eq!(crate::live::inspect(&h.app)["joinField"], pasted.as_str());
    key(&mut h, Key::Enter);
    assert_eq!(script.lock().unwrap().joined, [INVITE], "the link goes to the link as it is");
    assert!(h.app.ui.live.code.is_empty(), "the field is cleared once the join started");
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Joining…") && has_text(&shapes, "Leave session"));
    // The share doesn't let you in: the popover says so and offers the field again.
    script.lock().unwrap().events.push(LiveEvent::Status(Status::Failed { reason: "That invite has expired".into() }));
    let shapes = h.frames(3);
    assert!(has_text(&shapes, "That invite has expired") && has_text(&shapes, "Join with an invite link") && has_text(&shapes, "Start sharing"));
}

#[test]
fn the_home_screen_joins_with_an_invite_link_and_opens_the_file() {
    let (mut h, script) = harness();
    h.app.ui.home = true;
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Join with an invite link") && has_text(&shapes, "Invite link") && has_text(&shapes, "Join"));
    click(&mut h, &shapes, "Invite link");
    type_text(&mut h, INVITE);
    key(&mut h, Key::Enter);
    assert_eq!(script.lock().unwrap().joined, [INVITE]);
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Joining live session…") && h.app.ui.home, "the Join dialog shows the join");
    // It failed: the dialog says why, and the home screen stays.
    script.lock().unwrap().events.push(LiveEvent::Status(Status::Failed { reason: "That invite has expired".into() }));
    let shapes = h.frames(3);
    assert!(has_text(&shapes, "That invite has expired") && h.app.ui.home);
    // Again, and it works: the shared file's tab comes to the front.
    h.app.ui.live.code = INVITE.into();
    let shapes = h.frames(1);
    click(&mut h, &shapes, "Join");
    assert_eq!(script.lock().unwrap().joined.len(), 2);
    script.lock().unwrap().events.push(LiveEvent::Welcome { info: info(Role::Guest, INVITE), peers: vec![] });
    h.frames(2);
    document_arrives(&mut h, &script);
    assert!(!h.app.ui.home, "the joined file shows");
    assert_eq!(h.app.session.docs.len(), 2);
}

/// Asserts the join dialog uses the same shared join field, focuses it, and shows the join once
/// it starts.
#[test]
fn the_join_dialog_is_the_same_field() {
    let (mut h, script) = harness();
    crate::menus::invoke(&mut h.app, "collab.join");
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Paste the invite link you were given.") && has_text(&shapes, "Invite link"));
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique("live_join_link"))), "the field has the keyboard");
    type_text(&mut h, INVITE);
    key(&mut h, Key::Enter);
    h.frames(1);
    assert_eq!(script.lock().unwrap().joined, [INVITE]);
    assert!(h.app.ui.live.join.is_none(), "its field goes once the join started");
    assert!(has_text(&h.frames(1), "Joining live session…"), "and it shows the join");
}

#[test]
fn an_open_popover_asks_for_no_frames() {
    let (mut h, _) = harness();
    let shapes = open(&mut h);
    assert!(h.goes_quiet(), "not sharing");
    click(&mut h, &shapes, "Start sharing");
    assert!(h.goes_quiet(), "sharing, with the invite link showing");
    let (shapes, delay) = h.frame(vec![]);
    assert!(has_text(&shapes, "Invite link") && delay >= Duration::from_secs(1));
}
