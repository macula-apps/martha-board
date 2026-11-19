//! Layers that only moved skip the steps of [`normalize`] that follow a layer's own content
//! (#1571): the document must come out exactly as a full normalisation leaves it, for any set of
//! layers moved, on a board holding every built-in template and during a drag.

use figcraft_doc::{Document, NodeId};
use figcraft_geom::kurbo::Affine;
use serde_json::json;

use crate::Session;
use crate::derived::{NO_MOVE_SHORTCUT, only_moved_layers};
use crate::normalize;
use crate::params::id_of;
use crate::test_variables as tv;
use crate::tools::{PointerEvent, PointerKind};

/// Normalise `doc` against `before`, with the shortcut or without it.
fn normalized(mut doc: Document, before: &Document, shortcut: bool) -> Document {
    NO_MOVE_SHORTCUT.with(|c| c.set(!shortcut));
    normalize(&mut doc, before);
    NO_MOVE_SHORTCUT.with(|c| c.set(false));
    doc
}

/// `doc` settled: everything normalised until nothing changes.
fn settled(mut doc: Document) -> Document {
    for _ in 0..3 {
        let before = doc.clone();
        let all: Vec<NodeId> = doc.ids().collect();
        doc.mark_dirty(all);
        normalize(&mut doc, &before);
    }
    doc.take_dirty();
    doc
}

/// `base` with each of `ids` moved by (13, -7) in its parent.
fn moved(base: &Document, ids: &[NodeId]) -> Document {
    let mut doc = base.clone();
    for id in ids {
        if let Some(n) = doc.node_mut(*id) {
            n.transform = Affine::translate((13.0, -7.0)) * n.transform;
        }
    }
    doc
}

/// A board holding each built-in template once (sections, stickies, shapes with text, connectors,
/// tables and text), side by side.
fn templates_board() -> Document {
    let mut s = crate::board_tests::board();
    let list = s.execute("board.templates", &json!({})).unwrap();
    let names: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert!(!names.is_empty());
    for (i, name) in names.iter().enumerate() {
        s.execute("board.template", &json!({"name": name, "x": i as f64 * 2000.0, "y": 0})).unwrap();
    }
    s.doc().unwrap().clone()
}

#[test]
fn moving_layers_of_a_templates_board_matches_a_full_normalise() {
    let base = settled(templates_board());
    let layers: Vec<NodeId> = base.ids().filter(|i| !base.pages.contains(i)).collect();
    let top: Vec<NodeId> = base.pages.iter().flat_map(|pg| base.get(*pg).unwrap().children.clone()).collect();
    let every_third: Vec<NodeId> = layers.iter().copied().step_by(3).collect();
    for (case, ids) in [("top-level layers", &top), ("every layer", &layers), ("every third layer", &every_third)] {
        let doc = moved(&base, ids);
        let mut probe = doc.clone();
        let touched = probe.take_dirty();
        assert!(!only_moved_layers(&mut probe, &base, &touched).is_empty(), "{case}: some layers take the shortcut");
        assert!(normalized(doc.clone(), &base, true) == normalized(doc, &base, false), "{case}: as a full normalisation");
    }
}

#[test]
fn layers_that_did_more_than_move_go_through_every_step() {
    let mut s = Session::new();
    let r = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 40, "height": 40})).unwrap()["id"]).unwrap();
    let base = s.doc().unwrap().clone();
    let mut doc = moved(&base, &[r]);
    doc.node_mut(r).unwrap().width = 90.0;
    let touched = doc.take_dirty();
    assert!(only_moved_layers(&mut doc, &base, &touched).is_empty(), "resized too");
    let mut doc = base.clone();
    doc.mark_dirty([r]);
    let touched = doc.take_dirty();
    assert!(only_moved_layers(&mut doc, &base, &touched).is_empty(), "touched, but unchanged");
}

fn rect(s: &mut Session, parent: Option<NodeId>, x: f64, y: f64) -> NodeId {
    let mut p = json!({"x": x, "y": y, "width": 40, "height": 40});
    if let Some(parent) = parent {
        p["parent"] = json!(parent);
    }
    id_of(&s.execute("shape.rectangle", &p).unwrap()["id"]).unwrap()
}

/// Plain layers (one bound to variables), a layer in a group, one ignoring its frame's auto layout
/// and one in a frame with a copy beside it, all selected; then pressed at (20, 20) and
/// dragged through `to`.
fn drag_preview(shortcut: bool, to: &[(f64, f64)]) -> (Document, Document) {
    NO_MOVE_SHORTCUT.with(|c| c.set(!shortcut));
    let mut s = Session::new();
    let mut sel = vec![rect(&mut s, None, 0.0, 0.0), rect(&mut s, None, 100.0, 0.0)];
    tv::collection(&mut s, "T", &["Mode 1"]);
    tv::variable(&mut s, "T", "brand", "color", &[json!("#FF0000")]);
    tv::variable(&mut s, "T", "wide", "number", &[json!(64)]);
    for (field, variable) in [("fill", "brand"), ("width", "wide")] {
        tv::bind(&mut s, &[sel[1]], field, variable);
    }
    assert_eq!(s.doc().unwrap().node(sel[1]).unwrap().bound.len(), 2);
    let g1 = rect(&mut s, None, 0.0, 200.0);
    let g2 = rect(&mut s, None, 80.0, 200.0);
    s.execute("object.group", &json!({"ids": [g1, g2]})).unwrap();
    sel.push(g1);
    let f = id_of(&s.execute("frame.create", &json!({"x": 300, "y": 0, "width": 300, "height": 300})).unwrap()["id"]).unwrap();
    rect(&mut s, Some(f), 310.0, 10.0);
    let free = rect(&mut s, Some(f), 400.0, 100.0);
    s.execute("layout.add", &json!({"ids": [f]})).unwrap();
    s.execute("node.set", &json!({"id": free, "props": {"absolute": true}})).unwrap();
    sel.push(free);
    let c = id_of(&s.execute("frame.create", &json!({"x": 0, "y": 400, "width": 200, "height": 200})).unwrap()["id"]).unwrap();
    sel.push(rect(&mut s, Some(c), 10.0, 410.0));
    s.execute("edit.duplicate", &json!({"ids": [c], "dx": 300})).unwrap();
    s.execute("select.set", &json!({"ids": sel})).unwrap();
    assert_eq!(s.selection().len(), sel.len());
    let ev = |kind, (x, y): (f64, f64)| PointerEvent::at(kind, x, y, 1.0);
    s.pointer(&ev(PointerKind::Down, (20.0, 20.0))).unwrap();
    for p in to {
        s.pointer(&ev(PointerKind::Move, *p)).unwrap();
    }
    let preview = s.doc().unwrap().clone();
    s.pointer(&ev(PointerKind::Up, *to.last().unwrap())).unwrap();
    NO_MOVE_SHORTCUT.with(|c| c.set(false));
    (preview, s.doc().unwrap().clone())
}

#[test]
fn a_drag_previews_and_drops_as_with_a_full_normalise() {
    let path = [(30.0, 25.0), (61.0, 47.0), (44.0, 90.0)];
    let (preview, dropped) = drag_preview(true, &path);
    let (full_preview, full_dropped) = drag_preview(false, &path);
    assert!(preview == full_preview, "the preview");
    assert!(dropped == full_dropped, "the drop");
    assert!(preview.ids().count() > 8);
}
