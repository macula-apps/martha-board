//! The toolbar's mode switcher with a pen path open (#307): Design, the mode already shown, leaves
//! the path going on, and a press on the switcher never reaches the canvas under it.

use egui::accesskit::Role;
use egui::{Pos2, pos2};
use figcraft_engine::Session;
use figcraft_engine::tools::ToolId;
use serde_json::json;

use crate::layers_drag_tests::Ui;

fn click(u: &mut Ui, p: Pos2) {
    u.button(p, true);
    u.button(p, false);
    u.frame(vec![]);
}

fn layers(u: &Ui) -> usize {
    let doc = u.app.session.doc().unwrap();
    doc.node(u.app.session.page().unwrap()).unwrap().children.len()
}

/// The middle of the toolbar's mode button named `label`, found in the AccessKit tree.
fn mode_button(u: &mut Ui, label: &str) -> Pos2 {
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update.expect("an AccessKit update").nodes;
    let n = nodes.iter().map(|(_, n)| n).find(|n| n.role() == Role::Button && n.label() == Some(label)).expect(label);
    let b = n.bounds().expect("bounds");
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// A pen path with two anchors still open on an empty page.
fn drawing() -> Ui {
    let mut u = Ui::new(Session::new());
    u.app.run("tool.set", json!({"tool": "pen"}));
    click(&mut u, pos2(600.0, 400.0));
    click(&mut u, pos2(700.0, 450.0));
    assert_eq!(u.app.session.tool.pen.as_ref().map(|p| p.points.len()), Some(2), "the path is open");
    u
}

#[test]
fn the_mode_switchers_design_button_keeps_the_pen_path() {
    // Design is already the mode while the pen draws: pressing it again leaves nothing, so the
    // path goes on.
    let mut u = drawing();
    let at = mode_button(&mut u, "Design");
    click(&mut u, at);
    assert!(u.app.session.tool.pen.is_some(), "Design: the path goes on");
    assert_eq!(u.app.session.tool.tool, ToolId::Pen, "Design: still the pen");
}

#[test]
fn a_press_on_the_mode_switcher_does_not_reach_the_canvas() {
    // The switcher floats over the canvas: the pen must not place an anchor under it.
    for label in ["Draw", "Design"] {
        let mut u = Ui::new(Session::new());
        u.app.run("tool.set", json!({"tool": "pen"}));
        let at = mode_button(&mut u, label);
        u.button(at, true);
        assert!(u.app.session.tool.pen.is_none(), "{label}: the press starts no path");
        u.button(at, false);
        u.frame(vec![]);
        assert!(u.app.session.tool.pen.is_none(), "{label}: no path after the click");
        assert_eq!(layers(&u), 0, "{label}: the click adds nothing");
    }
}
