//! #1029, through the whole app: the Type settings popover keeps the width it asks for on every
//! frame while it stays open. The report came from a French UI, whose labels ran past the label
//! column; the UI ships in English only since #34, so this drives the English UI, and a label that
//! runs past its column is covered where it is cut (`field_ellipsis_tests`, `field_tests`).
#![allow(clippy::unwrap_used)]

use egui::{Id, Popup, Pos2, RawInput, Rect, vec2};
use figcraft_engine::Session;
use figcraft_ui_egui::{FigApp, NoServices};
use serde_json::json;

/// The Type settings popover's width over the frames it is open.
fn widths() -> Vec<f32> {
    let mut s = Session::new();
    // Truncating text also shows Max lines.
    let text = s.execute("text.create", &json!({"text": "Type", "props": {"textAutoResize": "truncate"}})).unwrap()["id"].clone();
    s.execute("select.set", &json!({"ids": [text]})).unwrap();
    let ctx = egui::Context::default();
    let mut app = FigApp::new(s, Box::new(NoServices));
    let open = Id::unique("ty_settings_open");
    let mut widths = Vec::new();
    for i in 0..40 {
        if i == 3 {
            Popup::open_id(&ctx, open);
        }
        let raw =
            RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), time: Some(i as f64 / 60.0), ..Default::default() };
        ctx.run_ui(raw, |ui| app.ui(ui)).textures_delta.clear();
        // From the frame after the one it opened on (egui's sizing pass).
        if i > 3 {
            assert!(Popup::is_id_open(&ctx, open), "Type settings closed on frame {i}");
            widths.push(ctx.memory(|m| m.area_rect(open)).unwrap().width());
        }
    }
    widths
}

#[test]
fn type_settings_keeps_its_width_while_open() {
    let w = widths();
    // The width it asks for (`Popup::width`), every frame.
    assert!(w.iter().all(|x| (x - 264.0).abs() < 0.01), "Type settings widens: {} → {}", w[0], w[w.len() - 1]);
}
