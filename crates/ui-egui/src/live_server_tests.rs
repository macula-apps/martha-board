//! Joined files in the window: a view link's "View only" badge with only the moving tools, a
//! comment link's badge with the comment tool, and the Share popover's address and access for
//! each. (Publishing to a server, with its dialog, hosting and reports, left with
//! martha-board#41.)

use egui::{Event, Key};
use figcraft_engine::figcraft_collab::message::{ClientMsg, ServerMsg};
use figcraft_engine::figcraft_collab::{Client, Limits, Outgoing, Server};
use figcraft_engine::live::{Access, COMMENT_ONLY, Event as LiveEvent, Role, SessionInfo, VIEW_ONLY};
use figcraft_engine::tools::ToolId;
use serde_json::json;

use crate::live_tests::{Harness, has_text, text_center};

fn key(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() }]);
    }
}

/// Joined through a server's link with `access`: the welcome and the document (a new tab).
pub(crate) fn joined(h: &mut Harness, access: Access) {
    h.app.session.execute("collab.join", &json!({"link": "invite://board?host=127.0.0.1:47900&file=f&token=AbCdEfGhIjKlMnOpQrStUv"})).unwrap();
    let mut server = Server::new(h.app.session.doc().unwrap().clone(), "", Limits::default());
    let ClientMsg::Hello(me) = Client::hello("f", "", "Ada", "") else { panic!("no hello") };
    let (id, outs) = server.join_as(me, access).unwrap();
    let w = outs.into_iter().find_map(|o| if let Outgoing::To(_, w @ ServerMsg::Welcome { .. }) = o { Some(w) } else { None }).unwrap();
    let info = SessionInfo {
        role: Role::Guest,
        me: id,
        name: "Ada".into(),
        color: "#14AE5C".into(),
        file_name: String::new(),
        pages: vec![],
        link: String::new(),
        url: String::new(),
        address: "127.0.0.1:47900".into(),
        network: false,
        access,
    };
    h.send(vec![LiveEvent::Welcome { info, peers: vec![] }, LiveEvent::Sync(w)]);
    h.frames(2);
    assert!(h.app.session.live_shared());
}

#[test]
fn a_view_link_shows_the_badge_and_only_the_moving_tools() {
    let (mut h, _) = Harness::new();
    h.app.session.execute("tool.set", &json!({"tool": "rectangle"})).unwrap();
    joined(&mut h, Access::View);
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "View only"), "the badge");
    assert_eq!(h.app.session.tool.tool, ToolId::Move);
    assert_eq!(crate::live::inspect(&h.app)["readOnly"], VIEW_ONLY);
    // A tool's shortcut changes nothing, and says why.
    key(&mut h, Key::R);
    assert_eq!(h.app.session.tool.tool, ToolId::Move);
    assert!(h.app.ui.last_error.as_deref().is_some_and(|e| e.contains("View only")));
    // The Share popover says what the link allows.
    let shapes = h.frames(1);
    h.click(text_center(&shapes, "Share"));
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "127.0.0.1:47900 · View only"));
    assert!(has_text(&shapes, "View only: you see every change as it happens but can't edit. Leave to keep a copy you can edit."));
    // Leaving: the copy is yours, the full toolbar is back.
    h.click(text_center(&shapes, "Leave session"));
    let shapes = h.frames(2);
    assert!(!has_text(&shapes, "View only"));
    assert!(h.app.session.execute("tool.set", &json!({"tool": "rectangle"})).is_ok());
}

#[test]
fn a_comment_link_shows_its_badge_and_leaves_the_comment_tool() {
    let (mut h, _) = Harness::new();
    h.app.session.execute("tool.set", &json!({"tool": "rectangle"})).unwrap();
    joined(&mut h, Access::Comment);
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "Can comment"), "the badge");
    assert_eq!(h.app.session.tool.tool, ToolId::Move);
    assert_eq!(crate::live::inspect(&h.app)["readOnly"], COMMENT_ONLY);
    // The comment tool is in the pill and its shortcut works; an editing tool's says why not.
    h.app.session.execute("tool.set", &json!({"tool": "comment"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.tool.tool, ToolId::Comment, "commenting stays");
    key(&mut h, Key::R);
    assert_eq!(h.app.session.tool.tool, ToolId::Comment);
    assert!(h.app.ui.last_error.as_deref().is_some_and(|e| e.contains("Can comment")));
    // The Share popover says what the link allows.
    let shapes = h.frames(1);
    h.click(text_center(&shapes, "Share"));
    let shapes = h.frames(2);
    assert!(has_text(&shapes, "127.0.0.1:47900 · Can comment"));
    assert!(has_text(&shapes, "Can comment: you see every change as it happens and can comment, but can't edit. Leave to keep a copy you can edit."));
}
