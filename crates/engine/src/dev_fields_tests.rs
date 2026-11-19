//! Dev Mode left the app (#11), but its fields stay in the file format and the collab change model
//! until their own package: a file that carries them opens and saves them unchanged, collaborators
//! receive them, agents see them, and copies and component sync keep them as before. Nothing in the
//! app sets them now, so these tests set them on the document, as a file from an earlier build
//! carries them. (Moved from the annotation, measurement, dev resource, category and compare tests.)

use std::sync::Arc;

use figcraft_collab::{Change, Prop, Replica};
use figcraft_doc::*;
use serde_json::json;

use crate::Session;
use crate::test_helpers::{ex, idv, node};

fn rect(s: &mut Session, x: f64, parent: NodeId) -> NodeId {
    idv(&ex(s, "shape.rectangle", json!({"x": x, "y": 20, "width": 50, "height": 40, "parent": parent})))
}

#[test]
fn a_file_carrying_dev_fields_opens_saves_and_syncs_them() {
    let mut s = Session::new();
    let f = idv(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 100, "name": "Screen"})));
    let a = rect(&mut s, 20.0, f);
    let b = rect(&mut s, 100.0, f);
    let before = s.doc().unwrap().clone();
    {
        let doc = s.doc_mut().unwrap();
        let snap = doc.snapshot_subtree(f).unwrap();
        doc.dev_snapshots.insert(f, Arc::new(snap));
        doc.annotation_categories = Some(vec![AnnotationCategory::new("motion", "Motion", AnnotationCategory::COLORS[4])]);
        let n = doc.get_mut(f).unwrap();
        (n.dev_status, n.dev_changed, n.dev_note) = (DevStatus::ReadyForDev, true, "Checked".into());
        let n = doc.get_mut(a).unwrap();
        n.annotations.push(Annotation { label: "Fade".into(), properties: vec!["height".into(), "future".into()], category: Some("motion".into()) });
        n.dev_resources.push(DevResource { url: "https://example.com/spec".into(), name: "Spec".into() });
        n.measurements.push(Measurement { to: Some(b), side: None });
        doc.get_mut(b).unwrap().measurements.push(Measurement { to: None, side: Some(Side::Right) });
    }
    // Changes collaborators receive.
    let d = figcraft_collab::diff(&Replica::new(before), s.doc().unwrap()).unwrap();
    for prop in [Prop::Annotations, Prop::DevResources, Prop::Measurements] {
        assert!(d.changes.iter().any(|c| matches!(c, Change::Set { id, prop: p, .. } if *id == a && *p == prop)), "{prop:?} in {:?}", d.changes);
    }
    // Saved and opened again, unchanged (unknown pinned property ids too).
    let doc = s.doc().unwrap().clone();
    let bytes = figcraft_format::save_bytes(&doc, None).unwrap();
    let (back, issues) = figcraft_format::load_bytes(&bytes).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    for id in [f, a, b] {
        let (x, y) = (back.node(id).unwrap(), doc.node(id).unwrap());
        assert_eq!((x.dev_status, x.dev_changed, &x.dev_note), (y.dev_status, y.dev_changed, &y.dev_note));
        assert_eq!((&x.annotations, &x.dev_resources, &x.measurements), (&y.annotations, &y.dev_resources, &y.measurements));
    }
    assert_eq!(back.annotation_categories, doc.annotation_categories);
    assert_eq!(back.dev_snapshots, doc.dev_snapshots);
    // What agents inspect.
    assert_eq!(ex(&mut s, "node.get", json!({"id": f}))["devStatus"], json!("readyForDev"));
    let got = ex(&mut s, "node.get", json!({"id": a}));
    assert_eq!(got["annotations"], json!([{"label": "Fade", "properties": ["height", "future"], "category": "motion"}]));
    assert_eq!(crate::inspect::node_json(&doc, b, 0)["measurements"], json!([{"side": "right"}]));
    // Layers without them leave the fields out, and files without them read as none.
    let plain = serde_json::to_value(Node::new(NodeId(7), NodeKind::Rectangle, "R", 1.0, 1.0)).unwrap();
    for key in ["annotations", "dev_resources", "devResources", "measurements", "dev_status", "devStatus"] {
        assert!(plain.get(key).is_none(), "{key}");
    }
    let n: Node = serde_json::from_value(plain).unwrap();
    assert!(n.annotations.is_empty() && n.dev_resources.is_empty() && n.measurements.is_empty() && n.dev_status == DevStatus::None);
    let old: Annotation = serde_json::from_value(json!({"label": "Old"})).unwrap();
    assert_eq!(old.category, None);
    // A snapshot goes with its layer.
    ex(&mut s, "edit.delete", json!({"ids": [f]}));
    assert!(s.doc().unwrap().dev_snapshots.is_empty());
}

#[test]
fn damaged_snapshots_are_ignored() {
    let mut s = Session::new();
    let f = idv(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 100})));
    rect(&mut s, 20.0, f);
    let page = s.page().unwrap();
    let mut doc = s.doc().unwrap().clone();
    let n = doc.get_mut(f).unwrap();
    (n.dev_status, n.dev_changed) = (DevStatus::ReadyForDev, true);
    let good = doc.snapshot_subtree(f).unwrap();
    let damaged = [
        DevSnapshot { nodes: vec![] },
        DevSnapshot { nodes: good.nodes.iter().skip(1).cloned().collect() },
        DevSnapshot { nodes: good.nodes.iter().cloned().chain(doc.node_arc(page).cloned()).collect() },
    ];
    for (k, snap) in damaged.into_iter().enumerate() {
        let mut d = doc.clone();
        d.dev_snapshots.insert(f, Arc::new(snap));
        if k < 2 {
            assert!(d.with_dev_snapshot(f).is_none(), "{k}");
        }
        // A page claimed by the snapshot isn't put in the copy.
        if let Some(copy) = d.with_dev_snapshot(f) {
            assert!(copy.node(page).is_some_and(|p| p.is_page()));
        }
        // Repairing (as after loading) drops snapshots that don't start with their layer.
        d.repair();
        assert_eq!(d.dev_snapshots.contains_key(&f), k == 2, "{k}");
    }
}

#[test]
fn copies_keep_dev_fields_as_before() {
    let mut s = Session::new();
    let f = idv(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 100})));
    let a = rect(&mut s, 20.0, f);
    let b = rect(&mut s, 100.0, f);
    s.doc_mut().unwrap().get_mut(a).unwrap().measurements =
        vec![Measurement { to: Some(b), side: None }, Measurement { to: None, side: Some(Side::Bottom) }];
    // The frame copied: the copy of `a` measures to the copy of `b`.
    ex(&mut s, "edit.duplicate", json!({"ids": [f]}));
    let copy = s.selection()[0];
    let kids = node(&s, copy).children.clone();
    let ca = node(&s, kids[0]);
    assert_eq!(ca.measurements.len(), 2);
    assert_eq!(ca.measurements[0].to, Some(kids[1]));
    // `a` alone copied: only its side measurement comes along.
    ex(&mut s, "edit.duplicate", json!({"ids": [a]}));
    let lone = s.selection()[0];
    let m = &node(&s, lone).measurements;
    assert_eq!((m.len(), m[0].to, m[0].side), (1, None, Some(Side::Bottom)));
}
