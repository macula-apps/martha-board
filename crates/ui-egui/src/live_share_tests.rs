//! Sharing with a join code in the window: the Share popover not sharing (the local network, one
//! Start button, the rest under Advanced), sharing on the local network (to edit, or to look only;
//! copying; what it lets people do; stop), a host without a code, joining with the one join field
//! (the popover, the home screen, the Join dialog), its errors, the server preference, and an idle
//! popover asking for no frames. (Sharing through a server left with martha-board#41.)

use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Event, Key, Shape};
use figcraft_engine::figcraft_collab::code::{self, Local};
use figcraft_engine::figcraft_collab::message::ClientMsg;
use figcraft_engine::live::{Access, DEFAULT_SERVER, Event as LiveEvent, HostOptions, LiveLink, Presence, Role, SessionInfo, Status, WayIn};
use serde_json::json;

use crate::live_tests::{Harness, has_text, text_center};

/// What the link was asked to do, and the events it hands out next (as the session started last).
#[derive(Default)]
struct Script {
    hosted: Vec<HostOptions>,
    joined: Vec<String>,
    left: usize,
    events: Vec<LiveEvent>,
    session: u64,
    copied: Vec<String>,
}

type Shared = Arc<Mutex<Script>>;

struct Link(Shared);

/// Where the scripted host listens, and its session's secret.
const HOST: Local = Local { ip: std::net::Ipv4Addr::new(192, 168, 1, 20), port: 47_900, secret: 0x123_4567_89AB };
/// The same session's view-only secret.
const VIEW: Local = Local { secret: 0xBA9_8765_4321, ..HOST };
/// A link without a code in it (an address a code can't carry).
const PLAIN: &str = "figcraft://join?host=[fd00::1]:47900&file=f&token=AbCdEfGhIjKlMnOpQrStUv";

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
        view: None,
    }
}

impl LiveLink for Link {
    /// Host as the network link does: port 6 stands for an address a code can't carry, and a
    /// share for looking only gets a view link too.
    fn host(&mut self, session: u64, opts: HostOptions) -> Result<SessionInfo, String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        let (link, view) = if opts.port == Some(6) { (PLAIN.to_string(), PLAIN.replace("AbCd", "View")) } else { (HOST.link(), VIEW.link()) };
        let view = opts.view.then(|| WayIn { url: view.replace("figcraft://join?host=", "ws://"), link: view });
        s.hosted.push(opts);
        Ok(SessionInfo { view, ..info(Role::Host, &link) })
    }
    fn join(&mut self, session: u64, link: &str, _: &str) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.joined.push(link.to_string());
        Ok(())
    }
    fn leave(&mut self, _: u64) {
        self.0.lock().unwrap().left += 1;
    }
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

/// A window on a file, with the scripted link and a server of the test's own in Preferences.
fn harness() -> (Harness, Shared) {
    let (mut h, _) = Harness::new();
    let script = Shared::default();
    h.app.session.live.install(Box::new(Link(script.clone())));
    h.app.services = Box::new(Clipboard(script.clone()));
    h.app.session.execute("pref.publishServer", &json!({"url": "wss://share.example/"})).unwrap();
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

/// Not sharing: one way to start (on the local network, offering Can edit / Can view) and one to
/// join, with where it listens under Advanced; nothing about a server.
#[test]
fn not_sharing_the_popover_offers_one_way_to_start_and_one_to_join() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    for t in ["Share this file", "Start sharing", "Join with a code", "Join", "Advanced", "Export…", "Can edit", "Can view"] {
        assert!(has_text(&shapes, t), "the popover shows {t:?}");
    }
    assert!(has_text(&shapes, "People on your network join with a code. Nothing goes through a server."));
    // Sharing through a server left: no choice of where, nothing about a server.
    for t in ["Online", "Local network", "About this server", "Can comment", "Server key", "Publish to another server…"] {
        assert!(!has_text(&shapes, t), "no {t:?}");
    }
    // Where it listens only once Advanced is opened.
    for t in ["Port", "This computer only", "WebSocket URL", "Not in a live session"] {
        assert!(!has_text(&shapes, t), "not in the main flow: {t:?}");
    }
    let shapes = click(&mut h, &shapes, "Advanced");
    assert!(has_text(&shapes, "This computer only") && has_text(&shapes, "Port"));
    let shapes = click(&mut h, &shapes, "Advanced");
    assert!(!has_text(&shapes, "This computer only"), "closed again");
    // Nothing started, nothing asked of the network.
    let s = script.lock().unwrap();
    assert!(s.hosted.is_empty() && s.joined.is_empty());
    assert_eq!(h.app.session.live().status, Status::Off);
}

#[test]
fn sharing_on_the_local_network_shows_its_longer_code() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    let shapes = click(&mut h, &shapes, "Start sharing");
    let shown = code::grouped(&HOST.code());
    for t in ["Sharing on your network · nobody else here yet", shown.as_str(), "Copy code", "Copy link", "Stop sharing"] {
        assert!(has_text(&shapes, t), "sharing shows {t:?}");
    }
    assert!(has_text(&shapes, "Anyone on your network with this code can edit the file, while this app stays open."));
    assert!(has_text(&shapes, "Can view") && !has_text(&shapes, "Can comment"), "a view-only code too, no comment code");
    {
        let s = script.lock().unwrap();
        assert!(s.hosted[0].short && s.hosted[0].view && s.hosted[0].bind == figcraft_engine::live::Bind::Lan && s.hosted[0].port.is_none());
    }
    let shapes = click(&mut h, &shapes, "Copy code");
    let shapes = click(&mut h, &shapes, "Copy link");
    assert_eq!(script.lock().unwrap().copied, [shown.clone(), HOST.link()]);
    // Can view shows the view-only code and link, and says what they let people do.
    let shapes = click(&mut h, &shapes, "Can view");
    let view = code::grouped(&VIEW.code());
    assert!(has_text(&shapes, &view) && !has_text(&shapes, &shown));
    assert!(has_text(&shapes, "Anyone on your network with this code can look at the file, not change it, while this app stays open."));
    let shapes = click(&mut h, &shapes, "Copy code");
    let shapes = click(&mut h, &shapes, "Copy link");
    assert_eq!(script.lock().unwrap().copied[2..], [view, VIEW.link()]);
    assert_eq!(script.lock().unwrap().hosted.len(), 1, "the same session");
    // Advanced: this computer only, on a port of your choosing.
    let shapes = click(&mut h, &shapes, "Stop sharing");
    let shapes = click(&mut h, &shapes, "Advanced");
    let shapes = click(&mut h, &shapes, "This computer only");
    h.app.ui.live.port = "6".into();
    click(&mut h, &shapes, "Start sharing");
    let s = script.lock().unwrap();
    assert!(s.hosted[1].bind == figcraft_engine::live::Bind::Local && s.hosted[1].port == Some(6));
}

/// Can view picked before a local network share starts: the view-only code is the one shown, so
/// the edit code isn't on the screen until Can edit is picked.
#[test]
fn a_local_share_can_start_for_looking_only() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    let shapes = click(&mut h, &shapes, "Can view");
    assert_eq!(h.app.ui.live.local_access, Access::View);
    assert_eq!(h.app.session.live().status, Status::Off, "picking starts nothing");
    let shapes = click(&mut h, &shapes, "Start sharing");
    let (view, edit) = (code::grouped(&VIEW.code()), code::grouped(&HOST.code()));
    assert!(has_text(&shapes, &view) && !has_text(&shapes, &edit));
    assert!(has_text(&shapes, "Anyone on your network with this code can look at the file, not change it, while this app stays open."));
    assert_eq!(crate::live::inspect(&h.app)["share"]["access"], "view");
    assert!(script.lock().unwrap().hosted[0].view);
    // Can edit shows the edit code.
    let shapes = click(&mut h, &shapes, "Can edit");
    assert!(has_text(&shapes, &edit) && has_text(&shapes, "Anyone on your network with this code can edit the file, while this app stays open."));
}

/// A host whose address a code can't carry shares its invite link instead.
#[test]
fn without_a_code_the_link_is_shared_instead() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    h.app.ui.live.port = "6".into();
    let shapes = click(&mut h, &shapes, "Start sharing");
    assert!(has_text(&shapes, "Invite link") && !has_text(&shapes, "Copy code"));
    assert!(has_text(&shapes, "No join code here (this address can't be put in one): share the link instead."));
    click(&mut h, &shapes, "Copy");
    assert_eq!(script.lock().unwrap().copied.last().map(String::as_str), Some(PLAIN));
}

/// Asserts the join field accepts a typed code or invite link, joins on Enter, clears after
/// joining, shows failure/retry, rejects an invalid code with an error, and accepts local
/// codes and links.
#[test]
fn the_join_field_takes_a_code_or_a_link_and_enter_joins() {
    let (mut h, script) = harness();
    let shapes = open(&mut h);
    // Click into the field (its hint), type a code the way people type it, press Enter.
    click(&mut h, &shapes, "Code or invite link");
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique("live_join_code"))));
    type_text(&mut h, " k7m 2qx-9td ");
    assert_eq!(crate::live::inspect(&h.app)["joinField"], " k7m 2qx-9td ");
    key(&mut h, Key::Enter);
    assert_eq!(script.lock().unwrap().joined, ["figcraft://join?server=wss://share.example/&code=K7M2QX9TD"]);
    assert!(h.app.ui.live.code.is_empty(), "the field is cleared once the join started");
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Joining…") && has_text(&shapes, "Leave session"));
    // The server doesn't know the code: the popover says so and offers the field again.
    script.lock().unwrap().events.push(LiveEvent::Status(Status::Failed { reason: "No session with that code".into() }));
    let shapes = h.frames(3);
    assert!(has_text(&shapes, "No session with that code") && has_text(&shapes, "Join with a code") && has_text(&shapes, "Start sharing"));
    // Something that can't be a code: it says why and stays to be fixed (the Join button).
    click(&mut h, &shapes, "Code or invite link");
    type_text(&mut h, "K7M-2QX");
    let shapes = h.frames(1);
    click(&mut h, &shapes, "Join");
    assert!(h.app.ui.last_error.as_deref().is_some_and(|e| e.contains("9 characters")), "{:?}", h.app.ui.last_error);
    assert_eq!(h.app.ui.live.code, "K7M-2QX");
    assert_eq!(script.lock().unwrap().joined.len(), 1);
    // A local network code and a pasted link go through the same field.
    for (typed, want) in [(code::grouped(&HOST.code()), HOST.link()), (PLAIN.to_string(), PLAIN.to_string())] {
        h.app.ui.live.code = typed;
        let shapes = h.frames(1);
        click(&mut h, &shapes, "Join");
        assert_eq!(script.lock().unwrap().joined.last(), Some(&want));
        h.app.session.execute("collab.leave", &json!({})).unwrap();
        h.frames(2);
    }
}

#[test]
fn the_home_screen_joins_with_a_code_and_opens_the_file() {
    let (mut h, script) = harness();
    h.app.ui.home = true;
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Join with a code") && has_text(&shapes, "Code or invite link") && has_text(&shapes, "Join"));
    click(&mut h, &shapes, "Code or invite link");
    type_text(&mut h, "K7M-2QX-9TD");
    key(&mut h, Key::Enter);
    assert_eq!(script.lock().unwrap().joined, ["figcraft://join?server=wss://share.example/&code=K7M2QX9TD"]);
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Joining live session…") && h.app.ui.home, "the Join dialog shows the join");
    // It failed: the dialog says why, and the home screen stays.
    script.lock().unwrap().events.push(LiveEvent::Status(Status::Failed { reason: "That code has expired or its sharing was stopped".into() }));
    let shapes = h.frames(3);
    assert!(has_text(&shapes, "That code has expired or its sharing was stopped") && h.app.ui.home);
    // Again, and it works: the shared file's tab comes to the front.
    h.app.ui.live.code = "K7M-2QX-9TD".into();
    let shapes = h.frames(1);
    click(&mut h, &shapes, "Join");
    assert_eq!(script.lock().unwrap().joined.len(), 2);
    script.lock().unwrap().events.push(LiveEvent::Welcome { info: info(Role::Guest, PLAIN), peers: vec![] });
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
    assert!(has_text(&shapes, "Type the join code you were given, or paste an invite link.") && has_text(&shapes, "Code or invite link"));
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::unique("live_join_link"))), "the field has the keyboard");
    type_text(&mut h, "4fh j8r 2wn");
    key(&mut h, Key::Enter);
    h.frames(1);
    assert_eq!(script.lock().unwrap().joined, ["figcraft://join?server=wss://share.example/&code=4FHJ8R2WN"]);
    assert!(h.app.ui.live.join.is_none(), "its field goes once the join started");
    assert!(has_text(&h.frames(1), "Joining live session…"), "and it shows the join");
}

/// The server in Preferences (where a server's join code is asked about) resets, and turned off
/// says what is left; sharing doesn't need it.
#[test]
fn the_server_preference_resets_and_turns_online_sharing_off() {
    let (mut h, script) = harness();
    h.app.ui.preferences = true;
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Server for online sharing") && has_text(&shapes, "Reset"));
    click(&mut h, &shapes, "Reset");
    assert_eq!(h.app.session.prefs.publish_server, DEFAULT_SERVER);
    // Off: Preferences say what that means, and the popover shares on the local network as ever.
    h.app.session.execute("pref.publishServer", &json!({"url": ""})).unwrap();
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Online sharing is off: files are shared on the local network only."));
    h.app.ui.preferences = false;
    h.frames(2);
    let shapes = open(&mut h);
    assert!(!has_text(&shapes, "Online") && has_text(&shapes, "Start sharing"));
    click(&mut h, &shapes, "Start sharing");
    assert_eq!(script.lock().unwrap().hosted.len(), 1, "shared on the local network");
}

#[test]
fn an_open_popover_asks_for_no_frames() {
    let (mut h, _) = harness();
    let shapes = open(&mut h);
    assert!(h.goes_quiet(), "not sharing");
    click(&mut h, &shapes, "Start sharing");
    assert!(h.goes_quiet(), "sharing, with the code showing");
    let (shapes, delay) = h.frame(vec![]);
    assert!(has_text(&shapes, &code::grouped(&HOST.code())) && delay >= Duration::from_secs(1));
}
