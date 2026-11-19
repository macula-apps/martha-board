//! Commands run without waiting on a server, against a scripted link that holds each background
//! request until the test answers it: nothing is waited for, scripts still wait, answers resume the
//! command (twice for a command with two requests), a cancelled command ignores its answer, errors
//! are its outcome, and a link that can't send in the background is waited on as before.

use std::sync::{Arc, Mutex};

use figcraft_collab::message::{ClientMsg, PluginEntry, PluginReply, PluginReq};
use figcraft_plugin::bundle::{Bundle, sha256_hex};
use serde_json::json;

use crate::live::{Event, HostOptions, LiveLink, NetReply, NetRequest, Presence, SessionInfo};
use crate::{EngineError, Session};

const ID: &str = "org.example.tiny";
const SERVER: &str = "ws://127.0.0.1:9/";
const LINK: &str = "ws://127.0.0.1:9/?file=f&token=t";

/// What the scripted link saw: background requests out, answers to hand back, waited-for calls.
#[derive(Default)]
struct Wire {
    background: bool,
    out: Vec<(u64, NetRequest)>,
    answers: Vec<Event>,
    waited: usize,
}

struct Link(Arc<Mutex<Wire>>);

impl LiveLink for Link {
    fn host(&mut self, _: u64, _: HostOptions) -> Result<SessionInfo, String> {
        Err("no".into())
    }
    fn join(&mut self, _: u64, _: &str, _: &str) -> Result<(), String> {
        Err("no".into())
    }
    fn leave(&mut self, _: u64) {}
    fn presence(&mut self, _: u64, _: &Presence) {}
    fn send(&mut self, _: u64, _: ClientMsg) {}
    fn poll(&mut self) -> Vec<(u64, Event)> {
        std::mem::take(&mut self.0.lock().unwrap().answers).into_iter().map(|e| (0, e)).collect()
    }
    fn plugins(&mut self, _: &str, req: PluginReq) -> Result<PluginReply, String> {
        self.0.lock().unwrap().waited += 1;
        Ok(directory(req))
    }
    fn background(&mut self, job: u64, req: NetRequest) -> bool {
        let mut w = self.0.lock().unwrap();
        if w.background {
            w.out.push((job, req));
        }
        w.background
    }
}

fn wasm() -> Vec<u8> {
    wat::parse_str(r#"(module (memory (export "memory") 1) (func (export "figcraft_run") (param i32) (result i32) i32.const 0))"#).unwrap()
}

fn bundle() -> Vec<u8> {
    let manifest = json!({"id": ID, "name": "Tiny tool", "version": "1.0.0", "main": "t.wasm", "permissions": {"access": "write", "commands": ["shape.rectangle"]}});
    Bundle { manifest: manifest.to_string(), wasm: wasm(), assets: Vec::new() }.encode()
}

fn directory(req: PluginReq) -> PluginReply {
    let bytes = bundle();
    let entry = PluginEntry {
        id: ID.into(),
        name: "Tiny tool".into(),
        version: "1.0.0".into(),
        access: "write".into(),
        commands: vec!["shape.rectangle".into()],
        sha256: sha256_hex(&bytes),
        ..PluginEntry::default()
    };
    match req {
        PluginReq::List { .. } => PluginReply::List(vec![entry]),
        PluginReq::Fetch { .. } => PluginReply::Bundle { entry, bundle: bytes },
        _ => PluginReply::Failed { reason: "no".into() },
    }
}

fn session(background: bool) -> (Session, Arc<Mutex<Wire>>) {
    let wire = Arc::new(Mutex::new(Wire { background, ..Wire::default() }));
    let mut s = Session::new();
    s.live.install(Box::new(Link(wire.clone())));
    (s, wire)
}

/// Answer the requests out (as the scripted server would) and let the session take the answers;
/// how many there were.
fn answer(s: &mut Session, wire: &Arc<Mutex<Wire>>) -> usize {
    let out = std::mem::take(&mut wire.lock().unwrap().out);
    for (job, req) in &out {
        let mut w = wire.lock().unwrap();
        let reply = match req.clone() {
            NetRequest::Plugins { req, .. } => NetReply::Plugins(Ok(directory(req))),
            NetRequest::Library { .. } => NetReply::Library(Err("no".into())),
            NetRequest::Upload(_) => NetReply::Upload(Err("no".into())),
            NetRequest::Hosting { .. } => NetReply::Hosting(Err("no".into())),
            NetRequest::Share { .. } => NetReply::Share(Err("no".into())),
        };
        w.answers.push(Event::Net { job: *job, reply: Box::new(reply) });
    }
    s.live_poll();
    out.len()
}

#[test]
fn a_continue_batch_waits_instead_of_skipping_background_work() {
    for nested in [false, true] {
        let (mut s, wire) = session(true);
        let before = s.doc().unwrap().clone();
        let request = json!({"command": "plugin.directory.list", "params": {"server": SERVER}});
        let step = if nested { json!({"command": "command.batch", "params": {"onError": "continue", "commands": [request]}}) } else { request };
        let params = json!({"onError": "continue", "commands": [
            {"command": "page.rename", "params": {"name": "Before request"}},
            step,
            {"command": "page.rename", "params": {"name": "After request"}}
        ]});
        assert_eq!(s.start_background("command.batch", params).unwrap(), None);
        assert!(s.waiting_for("command.batch"));
        assert_eq!(s.doc().unwrap(), &before, "waiting rolls back the whole batch");
        assert_eq!(s.undo_depth(), (0, 0));
        assert_eq!(answer(&mut s, &wire), 1);
        let (command, result) = s.take_background_done().unwrap();
        assert_eq!(command, "command.batch");
        let result = result.unwrap();
        assert!(result.as_array().unwrap().iter().all(|r| r["ok"] == true));
        let directory = if nested { &result[1]["result"][0]["result"] } else { &result[1]["result"] };
        assert_eq!(directory["plugins"][0]["id"], ID);
        assert_eq!(s.doc().unwrap().node(s.page().unwrap()).unwrap().name, "After request");
        assert_eq!(s.undo_depth(), (1, 0));
        assert!(!s.waiting_for("command.batch"));
    }
}

#[test]
fn browsing_and_installing_wait_for_nothing_and_scripts_still_wait() {
    let (mut s, wire) = session(true);
    assert_eq!(s.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap(), None);
    assert!(s.waiting_for("plugin.directory.list") && s.plugins.directory.is_none());
    assert_eq!(wire.lock().unwrap().waited, 0, "nothing was waited for");
    // A script (the MCP server, the control channel) still gets its answer at once.
    let r = s.execute("plugin.directory.list", &json!({"server": SERVER})).unwrap();
    assert_eq!(r["plugins"][0]["id"], ID);
    assert_eq!(wire.lock().unwrap().waited, 1);
    assert!(s.take_background_done().is_none(), "no answer yet");
    assert_eq!(answer(&mut s, &wire), 1);
    let (cmd, r) = s.take_background_done().unwrap();
    assert_eq!((cmd.as_str(), r.unwrap()["plugins"][0]["id"].as_str()), ("plugin.directory.list", Some(ID)));
    assert!(!s.waiting_for("plugin.directory.list") && s.take_background_done().is_none());

    let install = json!({"id": ID, "confirm": true, "server": SERVER});
    assert_eq!(s.start_background("plugin.directory.install", install).unwrap(), None);
    assert!(s.prefs.plugins.is_empty() && !s.plugins.unsaved);
    answer(&mut s, &wire);
    let (_, r) = s.take_background_done().unwrap();
    assert_eq!(r.unwrap()["installed"], ID);
    assert_eq!(s.prefs.plugins.len(), 1);
    assert!(s.plugins.unsaved, "the app is told to save the plugin list at once");
    assert_eq!(wire.lock().unwrap().waited, 1, "the background path never waited");
}

#[test]
fn cancelled_commands_drop_their_answer_and_errors_are_the_outcome() {
    let (mut s, wire) = session(true);
    s.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap();
    s.cancel_background("plugin.directory.list");
    assert!(!s.waiting_for("plugin.directory.list"));
    answer(&mut s, &wire);
    assert!(s.take_background_done().is_none() && s.plugins.directory.is_none(), "the late answer is ignored");

    // Starting it again replaces the one waiting: only the newer answer counts.
    s.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap();
    s.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap();
    assert_eq!(answer(&mut s, &wire), 2);
    assert!(s.take_background_done().is_some() && s.take_background_done().is_none());

    s.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap();
    let (job, _) = wire.lock().unwrap().out.pop().unwrap();
    wire.lock().unwrap().answers.push(Event::Net { job, reply: Box::new(NetReply::Plugins(Err("the server can't be reached".into()))) });
    s.live_poll();
    let (_, r) = s.take_background_done().unwrap();
    assert!(r.unwrap_err().to_string().contains("can't be reached"));
    // Bad parameters fail at once, without a request.
    assert!(s.start_background("plugin.directory.install", json!({"id": ID})).is_err());
    assert!(wire.lock().unwrap().out.is_empty());
}

#[test]
fn a_link_without_background_requests_is_waited_on() {
    let (mut s, wire) = session(false);
    let r = s.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap().unwrap();
    assert_eq!(r["plugins"][0]["id"], ID);
    assert!(!s.waiting_for("plugin.directory.list"));
    assert_eq!(wire.lock().unwrap().waited, 1);
    // Without any link, the usual error.
    let mut bare = Session::new();
    assert!(bare.start_background("plugin.directory.list", json!({"server": SERVER})).unwrap_err().to_string().contains("desktop app"));
}

/// Confined to automation roots, commands reach no server (#1095): browsing a plugin directory,
/// hosting, joining and publishing are refused before the link is asked. Unconfined, the same
/// request reaches the server as before.
#[test]
fn commands_confined_to_automation_roots_reach_no_server() {
    let dir = std::env::temp_dir().join(format!("figcraft-roots-network-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let roots = Arc::new(crate::file_access::Roots::new(&[], &[dir.to_string_lossy().into_owned()]).unwrap());
    let (mut s, wire) = session(false);
    let refused = |r: Result<serde_json::Value, EngineError>| {
        let e = r.unwrap_err().to_string();
        assert!(e.contains(crate::file_access::NETWORK_REFUSED), "{e}");
    };
    crate::file_access::confined(Some(&roots), || {
        refused(s.execute("plugin.directory.list", &json!({"server": SERVER})));
        refused(s.execute("collab.host", &json!({})));
        refused(s.execute("collab.join", &json!({"link": LINK})));
        refused(s.execute("collab.publish", &json!({"server": SERVER})));
    });
    assert_eq!(wire.lock().unwrap().waited, 0, "the server was never asked");
    assert!(s.live.iter().next().is_none(), "no session was started");
    let _ = std::fs::remove_dir_all(&dir);

    let r = s.execute("plugin.directory.list", &json!({"server": SERVER})).unwrap();
    assert_eq!(r["plugins"][0]["id"], ID);
    assert_eq!(wire.lock().unwrap().waited, 1);
    let e = s.execute("collab.host", &json!({})).unwrap_err().to_string();
    assert!(!e.contains(crate::file_access::NETWORK_REFUSED), "unconfined, the link is asked: {e}");
}

/// A command that sends two requests (`collab.files {refresh}` asks each of two servers what became
/// of the files recorded on it) resumes after the first answer and is done after the second.
#[test]
fn a_command_with_two_requests_resumes_twice() {
    let (mut s, wire) = session(true);
    let record = |server: &str| {
        let edit = format!("figcraft://join?server={server}&file=f&token=EditSecretEditSecret00");
        json!({"server": server, "file": "f", "name": "File f", "owner": "OwnerSecretOwnerSecret", "edit": edit, "view": "", "created": 1, "opened": 1})
    };
    s.prefs.online_files = serde_json::from_value(json!([record(SERVER), record("ws://127.0.0.2:9/")])).unwrap();
    assert_eq!(s.start_background("collab.files", json!({"refresh": true})).unwrap(), None);
    assert_eq!(answer(&mut s, &wire), 1);
    assert!(s.take_background_done().is_none() && s.waiting_for("collab.files"));
    assert_eq!(answer(&mut s, &wire), 1);
    let (_, r) = s.take_background_done().unwrap();
    assert_eq!(r.unwrap()["problems"].as_array().map(Vec::len), Some(2), "both servers answered");
    assert_eq!(wire.lock().unwrap().waited, 0);
}
