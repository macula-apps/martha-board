//! The stroke's position / weight row in the Design panel, driven headless at the panel's default
//! width: the weight field keeps room for its value beside its icon, and the row's buttons stay
//! inside the panel (#1452: the weight field was so narrow that the value was hidden).

use egui::accesskit::Role;
use egui::{Id, Pos2, Rect};
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;

/// A stroked rectangle (`rect`) or ellipse, selected.
fn scene(kind: &str) -> Ui {
    let mut s = Session::new();
    let made = s.execute(&format!("shape.{kind}"), &json!({"x": 0, "y": 0, "width": 200, "height": 120})).unwrap();
    let id = made["id"].as_u64().unwrap();
    s.execute("stroke.add", &json!({"ids": [id]})).unwrap();
    s.execute("select.set", &json!({"ids": [id]})).unwrap();
    let mut u = Ui::new(s);
    u.frame(vec![]);
    u
}

/// The area of part `part` ("lead" or "field") of the stroke weight field.
fn weight(u: &Ui, part: &str) -> Rect {
    u.ctx.read_response(Id::unique("stroke_w").with(part)).unwrap_or_else(|| panic!("the weight field's {part} shows")).rect
}

/// The bounds of the button named `label`, found in the AccessKit tree.
fn named_button(u: &mut Ui, label: &str) -> Rect {
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update.expect("an AccessKit update").nodes;
    let n = nodes.iter().map(|(_, n)| n).find(|n| n.role() == Role::Button && n.label() == Some(label)).expect(label);
    let b = n.bounds().expect("bounds");
    Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32))
}

/// The weight field leaves the value room for a few digits beside its icon, for layers with
/// per-side strokes (one more button in the row) and without.
#[test]
fn weight_field_keeps_room_for_its_value() {
    for kind in ["rectangle", "ellipse"] {
        let u = scene(kind);
        let (lead, field) = (weight(&u, "lead"), weight(&u, "field"));
        assert!(field.max.x - lead.max.x >= 20.0, "{kind}: no room for the value right of the icon {lead:?} in {field:?}");
    }
}

/// The row's last button (Advanced) stays inside the panel's 8 pt right margin (#858's grid), in
/// line with the section headers' buttons, also with the per-side button beside the weight.
#[test]
fn stroke_row_buttons_stay_inside_the_panel() {
    for kind in ["rectangle", "ellipse"] {
        let mut u = scene(kind);
        let advanced = named_button(&mut u, "Advanced stroke");
        let panel_right = u.ctx.content_rect().max.x - crate::widgets::PAD_RIGHT;
        assert!(advanced.max.x <= panel_right + 0.5, "{kind}: Advanced at {advanced:?} runs past the panel's edge {panel_right}");
        if kind == "rectangle" {
            let sides = named_button(&mut u, "Individual strokes");
            assert!(sides.min.x >= weight(&u, "field").max.x, "{kind}: the per-side button {sides:?} overlaps the weight");
            assert!(sides.max.x <= advanced.min.x, "{kind}: the per-side button {sides:?} overlaps Advanced {advanced:?}");
        }
    }
}
