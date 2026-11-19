//! Inspect behaviour, written from the public documentation of design tools' inspect modes: the
//! redline measurements between layers and what agents read about a layer. Each rule is a table
//! row or an assert with a one-line derivation; where the documentation is silent the row says so
//! and pins today's behaviour. (The Inspect tab's properties, colour formats and generated code
//! left with Dev Mode, #11; its asset list is `cmd/file/assets_tests`.)

// The tables read best as tuples.
#![allow(clippy::type_complexity)]

use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use figcraft_doc::NodeId;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64, parent: Option<NodeId>) -> NodeId {
    let mut p = json!({"x": x, "y": y, "width": w, "height": h});
    if let Some(parent) = parent {
        p["parent"] = json!(parent);
    }
    let r = id(&ex(s, "shape.rectangle", p));
    set(s, &[r], json!({"fills": ["#FF0000"]}));
    r
}

fn set(s: &mut Session, ids: &[NodeId], props: Value) {
    ex(s, "node.set", json!({"ids": ids, "props": props}));
}

#[test]
fn redlines_between_layers() {
    let mut s = Session::new();
    let f = id(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 200})));
    let a = rect(&mut s, 20.0, 20.0, 50.0, 50.0, Some(f));
    let b = rect(&mut s, 120.0, 20.0, 50.0, 50.0, Some(f));
    let c = rect(&mut s, 40.0, 40.0, 50.0, 50.0, Some(f));
    let labels = |s: &mut Session, from: NodeId, to: Option<NodeId>| -> Vec<String> {
        let mut p = json!({"ids": [from]});
        if let Some(to) = to {
            p["to"] = json!(to);
        }
        let r = ex(s, "measure.distances", p);
        let mut v: Vec<String> = r["lines"].as_array().unwrap().iter().filter_map(|l| l["label"].as_str().map(str::to_string)).collect();
        v.sort();
        v
    };
    // (from, to, labels, derivation)
    let rows: [(NodeId, Option<NodeId>, &[&str], &str); 3] = [
        (a, Some(b), &["50"], "side by side: the gap between them"),
        (a, None, &["130", "20", "20", "230"], "with nothing hovered: to each edge of the parent frame"),
        (a, Some(c), &["20", "20", "20", "20"], "overlapping: between matching edges"),
    ];
    for (from, to, want, why) in rows {
        let mut want: Vec<String> = want.iter().map(|w| w.to_string()).collect();
        want.sort();
        assert_eq!(labels(&mut s, from, to), want, "{why}");
    }
}

#[test]
fn inspected_json_reports_the_layer() {
    let mut s = Session::new();
    let r = rect(&mut s, 10.0, 20.0, 80.0, 40.0, None);
    set(&mut s, &[r], json!({"rotation": 15, "opacity": 0.25, "blend": "screen", "cornerRadius": 6}));
    // What `inspect_document` and the control channel return for a layer.
    let n = crate::inspect::node_json(s.doc().unwrap(), r, 0);
    // (field, value, derivation)
    let rows: [(&str, Value, &str); 6] = [
        ("width", json!(80.0), "width"),
        ("height", json!(40.0), "height"),
        ("rotation", json!(15.0), "rotation in degrees"),
        ("opacity", json!(0.25), "opacity as a fraction"),
        ("blend", json!("screen"), "the blend mode"),
        ("cornerRadius", json!(6.0), "the radius"),
    ];
    for (k, want, why) in rows {
        assert_eq!(n[k], want, "{k}: {why}");
    }
}
