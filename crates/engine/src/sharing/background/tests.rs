//! Commands run without waiting on a server, against a scripted link that holds each background
//! request until the test answers it: nothing is waited for, scripts still wait, answers resume the
//! command (twice for a command with two requests), a cancelled command ignores its answer, errors
//! are its outcome, and a link that can't send in the background is waited on as before.

use std::sync::{Arc, Mutex};

use figcraft_collab::message::{Admin, AdminReply, ClientMsg, HostInfo};
use serde_json::json;

use crate::live::{Event, HostOptions, LiveLink, NetReply, NetRequest, Presence, SessionInfo};
use crate::{EngineError, Session};

/// Who runs the scripted server, as its hosting answer says.
const OPERATOR: &str = "ops@example.org";
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
    fn hosting(&mut self, _: &str, _: &str, _: &str, _: Option<Admin>) -> Result<(HostInfo, Option<AdminReply>), String> {
        self.0.lock().unwrap().waited += 1;
        Ok((hosting(), None))
    }
    fn background(&mut self, job: u64, req: NetRequest) -> bool {
        let mut w = self.0.lock().unwrap();
        if w.background {
            w.out.push((job, req));
        }
        w.background
    }
}

/// The scripted server's hosting.
fn hosting() -> HostInfo {
    HostInfo { operator: OPERATOR.into(), ..HostInfo::default() }
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
            NetRequest::Plugins { .. } => NetReply::Plugins(Err("no".into())),
            NetRequest::Library { .. } => NetReply::Library(Err("no".into())),
            NetRequest::Upload(_) => NetReply::Upload(Err("no".into())),
            // `collab.serverInfo` asks about the hosting alone; `collab.files` about owned files.
            NetRequest::Hosting { req: None, .. } => NetReply::Hosting(Ok((hosting(), None))),
            NetRequest::Hosting { .. } => NetReply::Hosting(Err("no".into())),
            NetRequest::Share { .. } => NetReply::Share(Err("no".into())),
        };
        w.answers.push(Event::Net { job: *job, reply: Box::new(reply) });
    }
    s.live_poll();
    out.len()
}

/// Asking the scripted server about its hosting.
const INFO: &str = "collab.serverInfo";

#[test]
fn a_continue_batch_waits_instead_of_skipping_background_work() {
    for nested in [false, true] {
        let (mut s, wire) = session(true);
        let before = s.doc().unwrap().clone();
        let request = json!({"command": INFO, "params": {"server": SERVER}});
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
        let info = if nested { &result[1]["result"][0]["result"] } else { &result[1]["result"] };
        assert_eq!(info["operator"], OPERATOR);
        assert_eq!(s.doc().unwrap().node(s.page().unwrap()).unwrap().name, "After request");
        assert_eq!(s.undo_depth(), (1, 0));
        assert!(!s.waiting_for("command.batch"));
    }
}

#[test]
fn asking_waits_for_nothing_and_scripts_still_wait() {
    let (mut s, wire) = session(true);
    assert_eq!(s.start_background(INFO, json!({"server": SERVER})).unwrap(), None);
    assert!(s.waiting_for(INFO));
    assert_eq!(wire.lock().unwrap().waited, 0, "nothing was waited for");
    // A script (the MCP server, the control channel) still gets its answer at once.
    let r = s.execute(INFO, &json!({"server": SERVER})).unwrap();
    assert_eq!(r["operator"], OPERATOR);
    assert_eq!(wire.lock().unwrap().waited, 1);
    assert!(s.take_background_done().is_none(), "no answer yet");
    assert_eq!(answer(&mut s, &wire), 1);
    let (cmd, r) = s.take_background_done().unwrap();
    assert_eq!((cmd.as_str(), r.unwrap()["operator"].as_str()), (INFO, Some(OPERATOR)));
    assert!(!s.waiting_for(INFO) && s.take_background_done().is_none());
    assert_eq!(wire.lock().unwrap().waited, 1, "the background path never waited");
}

#[test]
fn cancelled_commands_drop_their_answer_and_errors_are_the_outcome() {
    let (mut s, wire) = session(true);
    s.start_background(INFO, json!({"server": SERVER})).unwrap();
    s.cancel_background(INFO);
    assert!(!s.waiting_for(INFO));
    answer(&mut s, &wire);
    assert!(s.take_background_done().is_none(), "the late answer is ignored");

    // Starting it again replaces the one waiting: only the newer answer counts.
    s.start_background(INFO, json!({"server": SERVER})).unwrap();
    s.start_background(INFO, json!({"server": SERVER})).unwrap();
    assert_eq!(answer(&mut s, &wire), 2);
    assert!(s.take_background_done().is_some() && s.take_background_done().is_none());

    s.start_background(INFO, json!({"server": SERVER})).unwrap();
    let (job, _) = wire.lock().unwrap().out.pop().unwrap();
    wire.lock().unwrap().answers.push(Event::Net { job, reply: Box::new(NetReply::Hosting(Err("the server can't be reached".into()))) });
    s.live_poll();
    let (_, r) = s.take_background_done().unwrap();
    assert!(r.unwrap_err().to_string().contains("can't be reached"));
    // Bad parameters fail at once, without a request.
    assert!(s.start_background(INFO, json!({})).is_err());
    assert!(wire.lock().unwrap().out.is_empty());
}

#[test]
fn a_link_without_background_requests_is_waited_on() {
    let (mut s, wire) = session(false);
    let r = s.start_background(INFO, json!({"server": SERVER})).unwrap().unwrap();
    assert_eq!(r["operator"], OPERATOR);
    assert!(!s.waiting_for(INFO));
    assert_eq!(wire.lock().unwrap().waited, 1);
    // Without any link, the usual error.
    let mut bare = Session::new();
    assert!(bare.start_background(INFO, json!({"server": SERVER})).unwrap_err().to_string().contains(crate::live::NO_HOSTING));
}

/// Confined to automation roots, commands reach no server (#1095): asking a server about its
/// hosting, hosting, joining and publishing are refused before the link is asked. Unconfined, the
/// same request reaches the server as before.
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
        refused(s.execute(INFO, &json!({"server": SERVER})));
        refused(s.execute("collab.host", &json!({})));
        refused(s.execute("collab.join", &json!({"link": LINK})));
        refused(s.execute("collab.publish", &json!({"server": SERVER})));
    });
    assert_eq!(wire.lock().unwrap().waited, 0, "the server was never asked");
    assert!(s.live.iter().next().is_none(), "no session was started");
    let _ = std::fs::remove_dir_all(&dir);

    let r = s.execute(INFO, &json!({"server": SERVER})).unwrap();
    assert_eq!(r["operator"], OPERATOR);
    assert_eq!(wire.lock().unwrap().waited, 1);
    let e = s.execute("collab.host", &json!({})).unwrap_err().to_string();
    assert!(!e.contains(crate::file_access::NETWORK_REFUSED), "unconfined, the link is asked: {e}");
}

/// A plugin directory request (still on the wire, martha-board#31) is refused by name on a session
/// without a link, and a link that doesn't answer it refuses it by the same name.
#[test]
fn plugin_directory_requests_are_refused_by_name() {
    use figcraft_collab::message::PluginReq;
    let req = NetRequest::Plugins { server: SERVER.into(), req: PluginReq::List { query: String::new() } };
    let (_, wire) = session(false);
    let mut link = Link(wire);
    assert!(matches!(link.request(req), NetReply::Plugins(Err(why)) if why == crate::live::NO_PLUGIN_DIRECTORY));
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
