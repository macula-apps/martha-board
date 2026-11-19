//! A cut command (Windows sends Shift+Delete as one, and never as a key) cuts the selected layer:
//! with vector edit mode gone (#32) there are no points for it to take instead.

use egui::{Event, Modifiers};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;

fn id(v: serde_json::Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

#[test]
fn a_cut_command_cuts_the_selected_layer() {
    let mut s = Session::new();
    let path = id(s.execute("node.create", &json!({"type": "vector", "points": [[100, 100], [200, 100], [300, 100]], "closed": false})).unwrap());
    s.execute("select.set", &json!({"ids": [path]})).unwrap();
    let mut u = Ui::new(s);
    u.frame_with(vec![Event::Cut], Modifiers::SHIFT);
    assert!(u.app.session.doc().unwrap().node(path).is_none(), "the selected layer is cut");
}
