//! A text field that isn't being edited ends a value too long for it with an ellipsis, shows the
//! whole value on hover, and keeps it whole for screen readers (#2368: a value showed cut
//! mid-letter in a 62 px field). Shown on the export settings' suffix fields of a selected frame.

use egui::accesskit::Node;
use egui::epaint::{ClippedShape, Shape};
use egui::{Event, Pos2, pos2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;

fn id(v: serde_json::Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

/// A selected frame with an export setting for each suffix of `values`.
fn set_with(values: &[&str]) -> Ui {
    let mut s = Session::new();
    let f = id(s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 80, "height": 40})).unwrap());
    for (k, v) in values.iter().enumerate() {
        s.execute("export.add", &json!({"ids": [f]})).unwrap();
        s.execute("export.update", &json!({"ids": [f], "index": k, "suffix": v})).unwrap();
    }
    s.execute("select.set", &json!({"ids": [f]})).unwrap();
    let u = Ui::new(s);
    u.ctx.enable_accesskit();
    u
}

/// The accesskit nodes of the frame drawn now.
fn tree(u: &mut Ui) -> Vec<Node> {
    u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect()
}

/// How many painted texts read `text`.
fn count(shapes: &[String], text: &str) -> usize {
    shapes.iter().filter(|t| *t == text).count()
}

/// Frames with a still pointer on `at`, long enough for egui's tooltip delay; the last one's texts.
fn hover(u: &mut Ui, at: Pos2) -> Vec<String> {
    let mut shown = Vec::new();
    u.frame(vec![Event::PointerMoved(at)]);
    for _ in 0..90 {
        shown = texts(&u.frame_output(vec![]).shapes);
    }
    shown
}

fn texts(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(s: &Shape, out: &mut Vec<String>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            Shape::Text(t) => out.push(t.galley.text().to_owned()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// A suffix too long for its field at the right panel's default width.
const LONG: &str = "-for-the-press-kit";

/// The accesskit node of the field showing `value`.
fn chip(nodes: &[Node], value: &str) -> Pos2 {
    let n = nodes.iter().find(|n| n.value() == Some(value) && n.bounds().is_some()).unwrap_or_else(|| panic!("a chip for {value}"));
    let b = n.bounds().unwrap();
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

#[test]
fn a_value_too_long_for_its_field_ends_with_an_ellipsis() {
    let mut u = set_with(&["@2x", LONG]);
    let shown = texts(&u.frame_output(vec![]).shapes);
    // Where the value is cut depends on the font's widths, so any painted start of it counts.
    let cut: Vec<&String> = shown.iter().filter(|t| t.chars().count() > 2 && LONG.starts_with(t.trim_end_matches('…'))).collect();
    assert!(!cut.is_empty(), "the value is painted: {shown:?}");
    assert!(cut.iter().all(|t| t.ends_with('…')), "ends with an ellipsis: {cut:?}");
    assert!(!shown.iter().any(|t| t.len() > 5 && t.as_str() != LONG && LONG.starts_with(t.as_str())), "not cut mid-letter: {shown:?}");
}

#[test]
fn the_shortened_value_shows_whole_on_hover() {
    let mut u = set_with(&["@2x", LONG]);
    let at = chip(&tree(&mut u), LONG);
    let before = count(&texts(&u.frame_output(vec![]).shapes), LONG);
    let shown = hover(&mut u, at);
    assert_eq!(count(&shown, LONG), before + 1, "the whole value is painted again, in a tooltip: {shown:?}");
}

#[test]
fn a_value_that_fits_is_shown_whole_without_a_tooltip() {
    let mut u = set_with(&["@2x", LONG]);
    let nodes = tree(&mut u);
    let shown = texts(&u.frame_output(vec![]).shapes);
    let before = count(&shown, "@2x");
    assert!(before > 0, "@2x is painted as it is: {shown:?}");
    let after = hover(&mut u, chip(&nodes, "@2x"));
    assert_eq!(count(&after, "@2x"), before, "no tooltip repeats it: {after:?}");
}

#[test]
fn screen_readers_keep_the_whole_value() {
    let mut u = set_with(&["@2x", LONG]);
    let nodes = tree(&mut u);
    assert!(nodes.iter().any(|n| n.value() == Some(LONG)), "the field's value is the whole text");
    assert!(!nodes.iter().any(|n| n.value().is_some_and(|v| v.ends_with('…'))), "no node announces a shortened value");
}
