//! Sharing with a join code against a scripted link: `collab.share` on the local network, the
//! codes in `collab.peers`, switching what the code lets people do, stopping, and `collab.join`
//! with a code of either kind or a link (what typing tolerates, and what it says when it can't be
//! a code). Sharing through a server left with martha-board#41.

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use figcraft_collab::code::{self, Local};
use figcraft_collab::message::ClientMsg;
use serde_json::{Value, json};

use crate::live::{Access, Bind, DEFAULT_SERVER, Event, HostOptions, LiveLink, Presence, Role, SessionInfo, Status, WayIn, join_target};
use crate::{EngineError, Session};

/// What the link was asked to do, and the events it hands out next (as the session started last).
#[derive(Default)]
struct Script {
    hosted: Vec<HostOptions>,
    joined: Vec<String>,
    left: Vec<u64>,
    events: Vec<Event>,
    session: u64,
}

struct Link(Arc<Mutex<Script>>);

/// Where the scripted host listens, and its session's secret.
const HOST: Local = Local { ip: Ipv4Addr::new(192, 168, 1, 20), port: 47_900, secret: 0x123_4567_89AB };
/// The same session's view-only secret.
const VIEW: Local = Local { secret: 0xBA9_8765_4321, ..HOST };

/// The WebSocket URL the scripted link gives with an invite `link` (the same secret).
fn mock_url(link: &str) -> String {
    link.replace("figcraft://join?host=", "ws://")
}

impl LiveLink for Link {
    /// Host as the network link does: a code's short secret when shared with a code, and the
    /// view-only link when asked for one.
    fn host(&mut self, session: u64, opts: HostOptions) -> Result<SessionInfo, String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        // As the network link does: a short secret when the session is shared with a code.
        let link = if opts.short { HOST.link() } else { "figcraft://join?host=192.168.1.20:47900&file=f&token=AbCdEfGhIjKlMnOpQrStUv".to_string() };
        let view = opts.view.then(|| WayIn { link: VIEW.link(), url: mock_url(&VIEW.link()) });
        s.hosted.push(opts);
        Ok(SessionInfo {
            role: Role::Host,
            me: 1,
            name: "Me".into(),
            color: "#0D99FF".into(),
            file_name: "Shared".into(),
            pages: vec![],
            url: mock_url(&link),
            link,
            address: "192.168.1.20:47900".into(),
            network: true,
            access: Access::Edit,
            view,
        })
    }
    fn join(&mut self, session: u64, link: &str, _: &str) -> Result<(), String> {
        if !link.starts_with("figcraft://join?") {
            return Err("that's not a live session invite".into());
        }
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.joined.push(link.to_string());
        Ok(())
    }
    fn leave(&mut self, session: u64) {
        self.0.lock().unwrap().left.push(session);
    }
    fn presence(&mut self, _: u64, _: &Presence) {}
    fn send(&mut self, _: u64, _: ClientMsg) {}
    fn poll(&mut self) -> Vec<(u64, Event)> {
        let mut s = self.0.lock().unwrap();
        let session = s.session;
        std::mem::take(&mut s.events).into_iter().map(|e| (session, e)).collect()
    }
}

fn session() -> (Session, Arc<Mutex<Script>>) {
    let mut s = Session::new();
    let script = Arc::new(Mutex::new(Script::default()));
    s.live.install(Box::new(Link(script.clone())));
    (s, script)
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn err(s: &mut Session, cmd: &str, p: Value) -> String {
    s.execute(cmd, &p).unwrap_err().to_string()
}

#[test]
fn the_built_in_server_is_the_default_and_can_be_changed_or_turned_off() {
    let (mut s, _) = session();
    assert_eq!(s.prefs.publish_server, DEFAULT_SERVER);
    // A build may ship without one (`FIGCRAFT_DEFAULT_SERVER=`); any it ships is TLS.
    assert!(
        DEFAULT_SERVER.is_empty() || (DEFAULT_SERVER.starts_with("wss://") && DEFAULT_SERVER.ends_with('/')),
        "TLS, and an address the preference itself accepts"
    );
    assert_eq!(ex(&mut s, "pref.publishServer", json!({"url": DEFAULT_SERVER}))["url"], DEFAULT_SERVER);
    // Another server; back to the built-in one; off.
    assert_eq!(ex(&mut s, "pref.publishServer", json!({"url": "wss://share.example/"}))["url"], "wss://share.example/");
    let reset = ex(&mut s, "pref.publishServer", json!({}));
    assert_eq!((&reset["url"], &reset["default"]), (&json!(DEFAULT_SERVER), &json!(DEFAULT_SERVER)));
    assert_eq!(ex(&mut s, "pref.publishServer", json!({"url": ""}))["url"], "");
    // Off is saved as off (an empty address isn't left out, or the default would come back).
    let saved = serde_json::to_value(&s.prefs).unwrap();
    assert_eq!(saved["publish_server"], "");
    let loaded: crate::Prefs = serde_json::from_value(saved).unwrap();
    assert!(loaded.publish_server.is_empty());
    // Preferences saved before there was a default get it.
    let old: crate::Prefs = serde_json::from_value(json!({"theme": "dark"})).unwrap();
    assert_eq!(old.publish_server, DEFAULT_SERVER);
}

/// Without a server in Preferences a server's join code can't be used; sharing needs none (it is
/// on the local network), and sharing through a server is no more.
#[test]
fn online_sharing_is_off_without_a_server() {
    let (mut s, script) = session();
    ex(&mut s, "pref.publishServer", json!({"url": ""}));
    assert!(err(&mut s, "collab.join", json!({"code": "K7M-2QX-9TD"})).contains("online sharing is off"));
    // The local network needs no server.
    assert_eq!(ex(&mut s, "collab.share", json!({}))["share"]["via"], "local");
    ex(&mut s, "collab.stop", json!({}));
    assert_eq!(ex(&mut s, "collab.share", json!({"via": "local"}))["status"], "live");
    ex(&mut s, "collab.stop", json!({}));
    // Sharing through a server left: asking for it hosts nothing.
    let hosted = script.lock().unwrap().hosted.len();
    assert!(matches!(s.execute("collab.share", &json!({"via": "online"})), Err(EngineError::BadParams { .. })));
    assert_eq!(script.lock().unwrap().hosted.len(), hosted);
    // Without a link (the one-shot CLI) there is nothing to share with.
    let mut bare = Session::new();
    assert!(matches!(bare.execute("collab.share", &json!({"via": "local"})), Err(EngineError::Disabled(..))));
    assert!(matches!(bare.execute("collab.join", &json!({"code": HOST.code()})), Err(EngineError::Disabled(..))));
}

/// What happens in a file that isn't in front names the file. (It was set up with a file shared
/// online until that left with martha-board#41; a file shared on the local network is the same
/// session.)
#[test]
fn what_happens_in_a_file_that_isnt_in_front_names_the_file() {
    let (mut s, script) = session();
    ex(&mut s, "file.rename", json!({"name": "Homepage"}));
    ex(&mut s, "collab.share", json!({"via": "local"}));
    let peer = |id, name: &str| crate::live::Peer { client_id: id, name: name.into(), color: "#9747FF".into() };
    // In front: as before.
    script.lock().unwrap().events.push(Event::PeerJoined(peer(2, "Ada")));
    s.live_poll();
    assert_eq!(s.message.take().as_deref(), Some("Ada joined"));
    // Another file in front.
    ex(&mut s, "file.new", json!({}));
    script.lock().unwrap().events.push(Event::PeerJoined(peer(3, "Hana")));
    s.live_poll();
    assert_eq!(s.message.take().as_deref(), Some("Hana joined Homepage"));
    script.lock().unwrap().events.push(Event::PeerLeft(2));
    s.live_poll();
    assert_eq!(s.message.take().as_deref(), Some("Ada left Homepage"));
    script.lock().unwrap().events.push(Event::Ended { reason: "The server stopped".into() });
    s.live_poll();
    assert_eq!(s.message.take().as_deref(), Some("Homepage: The server stopped"));
    assert_eq!(s.live.iter().filter(|l| l.status == Status::Live).count(), 0);
}

/// A local network share shows the code that carries the host's address, has a view-only code
/// for the same session, switches which shows without hosting again, and listens where told.
#[test]
fn sharing_on_the_local_network_shows_a_code_that_carries_the_address() {
    let (mut s, script) = session();
    let r = ex(&mut s, "collab.share", json!({"via": "local"}));
    assert_eq!((&r["status"], &r["role"], &r["share"]["via"]), (&json!("live"), &json!("host"), &json!("local")));
    let shown = r["code"].as_str().unwrap();
    assert_eq!(code::parse(shown), Ok(code::Code::Local(HOST)), "{shown}");
    assert_eq!(shown, code::grouped(&HOST.code()));
    assert_eq!(r["link"], json!(HOST.link()));
    // A view-only code too, for the same session with its own secret; no comment code.
    let view = code::grouped(&VIEW.code());
    assert_eq!((&r["share"]["viewCode"], &r["share"]["viewLink"]), (&json!(view), &json!(VIEW.link())));
    assert_eq!((&r["share"]["editLink"], &r["share"]["commentCode"]), (&json!(HOST.link()), &Value::Null));
    {
        let sc = script.lock().unwrap();
        let [h] = sc.hosted.as_slice() else { panic!("hosted {}", sc.hosted.len()) };
        assert!(h.short && h.view && h.bind == Bind::Lan && h.port.is_none(), "the local network unless told otherwise");
    }
    // Which code shows: the view code, then the edit code again (nothing is hosted twice).
    // The link goes with the code: a view-only code's link doesn't edit.
    let r = ex(&mut s, "collab.share", json!({"access": "view"}));
    assert_eq!((&r["code"], &r["share"]["access"]), (&json!(view), &json!("view")));
    assert_eq!((&r["link"], &r["url"], &r["sessions"][0]["link"]), (&json!(VIEW.link()), &json!(mock_url(&VIEW.link())), &json!(VIEW.link())));
    let r = ex(&mut s, "collab.share", json!({"access": "edit"}));
    assert_eq!((&r["code"], &r["link"], &r["url"]), (&json!(code::grouped(&HOST.code())), &json!(HOST.link()), &json!(mock_url(&HOST.link()))));
    assert!(err(&mut s, "collab.share", json!({"access": "comment"})).contains("not a comment code"));
    assert_eq!(script.lock().unwrap().hosted.len(), 1);
    assert_eq!(ex(&mut s, "collab.stop", json!({}))["ended"], true);
    // Shared for looking only from the start: the edit code isn't the one shown.
    let r = ex(&mut s, "collab.share", json!({"via": "local", "access": "view"}));
    assert_eq!((&r["code"], &r["share"]["access"]), (&json!(view), &json!("view")));
    assert_eq!((&r["link"], &r["url"]), (&json!(VIEW.link()), &json!(mock_url(&VIEW.link()))), "the edit link and URL aren't handed out");
    assert_eq!(r["sessions"][0]["link"], json!(VIEW.link()));
    ex(&mut s, "collab.stop", json!({}));
    assert!(err(&mut s, "collab.share", json!({"via": "local", "access": "comment"})).contains("not a comment code"));
    // Where to listen is the advanced part.
    ex(&mut s, "collab.share", json!({"via": "local", "bind": "local", "port": 5000}));
    let sc = script.lock().unwrap();
    assert!(sc.hosted[2].bind == Bind::Local && sc.hosted[2].port == Some(5000));
    drop(sc);
    ex(&mut s, "collab.stop", json!({}));
    for bad in [json!({"via": "post"}), json!({"via": "local", "port": 70000}), json!({"via": "local", "bind": "everywhere"})] {
        assert!(matches!(s.execute("collab.share", &bad), Err(EngineError::BadParams { .. })), "{bad}");
    }
    // A plain `collab.host` session has an invite link, not a code, and everyone in it edits.
    let r = ex(&mut s, "collab.host", json!({}));
    assert_eq!((&r["code"], &r["share"]), (&Value::Null, &Value::Null));
    let h = &script.lock().unwrap().hosted[3];
    assert!(!h.short && !h.view);
}

#[test]
fn joining_takes_a_code_of_either_kind_or_a_link() {
    let (mut s, script) = session();
    ex(&mut s, "pref.publishServer", json!({"url": "wss://share.example/"}));
    let asks = "figcraft://join?server=wss://share.example/&code=K7M2QX9TD";
    let local = HOST.link();
    let link = "figcraft://join?host=10.0.0.5:47900&file=abc&token=AbCdEfGhIjKlMnOpQrStUv";
    let shown = code::grouped(&HOST.code());
    let table: [(Value, &str); 8] = [
        (json!({"code": "K7M-2QX-9TD"}), asks),
        (json!({"code": "  k7m 2qx 9td \n"}), asks),
        (json!({"code": "k7m2qx9td"}), asks),
        (json!({"link": "K7M2QX9TD"}), asks),
        (json!({"code": shown}), &local),
        (json!({"code": shown.to_lowercase().replace('-', "")}), &local),
        (json!({"link": link}), link),
        // The one field takes either: a link where a code is expected.
        (json!({"code": format!("  {link} ")}), link),
    ];
    for (params, want) in table {
        let r = ex(&mut s, "collab.join", params.clone());
        assert_eq!(r["status"], "connecting", "{params}");
        assert_eq!(script.lock().unwrap().joined.last().map(String::as_str), Some(want), "{params}");
    }
    // O for 0 and I or L for 1 are the same code.
    assert_eq!(join_target("wss://s.example/", "O1l-iLo-0oO"), join_target("wss://s.example/", "011-110-000"));
    // What can't be a code says why, and nothing is asked of the network.
    let before = script.lock().unwrap().joined.len();
    for (typed, why) in [
        ("", "type the join code"),
        ("K7M-2QX", "9 characters"),
        ("K7M-2QX-9TDX", "that one has 10"),
        ("K7M-2QU-9TD", "no U"),
        ("hello world!", "only letters and digits"),
        (&shown.replace(&shown[..1], if shown.starts_with('Z') { "Y" } else { "Z" }), "typing mistake"),
    ] {
        let e = err(&mut s, "collab.join", json!({"code": typed}));
        assert!(e.contains(why), "{typed:?}: {e}");
    }
    assert!(err(&mut s, "collab.join", json!({})).contains("give the join `code`"));
    assert!(err(&mut s, "collab.join", json!({"code": 7})).contains("9 characters"));
    assert!(s.execute("collab.join", &json!({"code": {"x": 1}})).is_err());
    // Something that looks like a link goes to the link's reader.
    assert!(err(&mut s, "collab.join", json!({"code": "https://example.com/x"})).contains("not a live session invite"));
    assert_eq!(script.lock().unwrap().joined.len(), before);
    // A failed lookup shows on the tab it was started from (the link's event).
    ex(&mut s, "collab.join", json!({"code": "K7M-2QX-9TD"}));
    script.lock().unwrap().events.push(Event::Status(Status::Failed { reason: "No session with that code".into() }));
    s.live_poll();
    let r = ex(&mut s, "collab.peers", json!({}));
    assert_eq!((&r["status"], &r["reason"]), (&json!("failed"), &json!("No session with that code")));
    assert_eq!(s.message.as_deref(), Some("No session with that code"));
}
