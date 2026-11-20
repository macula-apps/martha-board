//! The marks on tabs of files in a live session, and toasts that name a file that isn't in front.
//! (These lived with Home ▸ Online's tests until online files left with martha-board#41.)

use std::sync::{Arc, Mutex};

use figcraft_engine::figcraft_collab::message::ClientMsg;
use figcraft_engine::live::{Access, Event as LiveEvent, HostOptions, LiveLink, Peer, Presence, Role, SessionInfo};
use serde_json::json;

use crate::live_tests::Harness;

/// What the scripted link was asked, and the events it hands out next.
#[derive(Default)]
struct Script {
    hosted: usize,
    events: Vec<LiveEvent>,
    session: u64,
}

type Shared = Arc<Mutex<Script>>;

struct Link(Shared);

impl LiveLink for Link {
    fn host(&mut self, session: u64, _: HostOptions) -> Result<SessionInfo, String> {
        let mut s = self.0.lock().unwrap();
        s.session = session;
        s.hosted += 1;
        Ok(SessionInfo {
            role: Role::Host,
            me: 1,
            name: "Ada".into(),
            color: "#14AE5C".into(),
            file_name: "Untitled".into(),
            pages: vec![],
            link: "invite://board?host=192.168.1.20:47900&file=f&token=AbCdEfGhIjKlMnOpQrStUv".into(),
            url: String::new(),
            address: "192.168.1.20:47900".into(),
            network: true,
            access: Access::Edit,
        })
    }
    fn join(&mut self, _: u64, _: &str, _: &str) -> Result<(), String> {
        Err("not in this test".into())
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

fn toast(h: &Harness) -> String {
    h.app.ui.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

#[test]
fn tabs_mark_files_in_a_live_session_and_toasts_name_a_file_that_isnt_in_front() {
    let (mut h, _) = Harness::new();
    let script = Shared::default();
    h.app.session.live.install(Box::new(Link(script.clone())));
    h.app.ui.home = false;
    h.frames(3);
    h.app.session.execute("file.rename", &json!({"name": "Homepage"})).unwrap();
    assert_eq!(h.app.inspect()["tabs"], json!([{"name": "Homepage", "live": null}]));
    // Hosting: others get in.
    h.app.session.execute("collab.host", &json!({})).unwrap();
    h.frames(2);
    assert_eq!(script.lock().unwrap().hosted, 1);
    assert_eq!(crate::tabs::live_mark(&h.app, h.app.session.docs[0].uid), Some(true));
    // Someone joins while the file is in front, then while another is.
    let peer = |id, name: &str| Peer { client_id: id, name: name.into(), color: "#9747FF".into() };
    script.lock().unwrap().events.push(LiveEvent::PeerJoined(peer(2, "Hana")));
    h.frames(2);
    assert_eq!(toast(&h), "Hana joined");
    h.app.session.execute("file.new", &json!({})).unwrap();
    script.lock().unwrap().events.push(LiveEvent::PeerJoined(peer(3, "Ada")));
    h.frames(2);
    assert_eq!(toast(&h), "Ada joined Homepage");
    assert_eq!(h.app.inspect()["tabs"], json!([{"name": "Homepage", "live": "shared"}, {"name": "Untitled", "live": null}]));
    script.lock().unwrap().events.push(LiveEvent::PeerLeft(2));
    h.frames(2);
    assert_eq!(toast(&h), "Hana left Homepage");
}
