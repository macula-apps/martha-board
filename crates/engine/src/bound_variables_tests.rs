//! What a file carrying variables keeps doing (the variables editor and its export left, #16):
//! gradient stops bound to variables follow their stops.

use figcraft_color::{Color, ColorStop, Gradient, GradientKind, Paint, PaintKind};
use figcraft_doc::*;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::test_variables as tv;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    match s.execute(cmd, &p) {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {p}: {e}"),
    }
}

fn idv(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn healthy(s: &Session) {
    let doc = s.doc().unwrap();
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
}

/// A rectangle filled with a three-stop linear gradient, and colour variables `brand` and `ink`.
fn gradient_rect(s: &mut Session) -> NodeId {
    let r = idv(&ex(s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 40, "height": 40})));
    let mut g = Gradient::default_for(GradientKind::Linear, Color::BLACK);
    g.stops = vec![
        ColorStop { position: 0.0, color: Color::BLACK },
        ColorStop { position: 0.5, color: Color::WHITE },
        ColorStop { position: 1.0, color: Color::BLACK },
    ];
    ex(s, "fill.set", json!({"ids": [r], "index": 0, "paint": Paint::gradient(g)}));
    tv::collection(s, "T", &["Mode 1"]);
    tv::variable(s, "T", "brand", "color", &[json!("#ff0000")]);
    tv::variable(s, "T", "ink", "color", &[json!("#00ff00")]);
    r
}

fn stops(s: &Session, r: NodeId) -> Vec<String> {
    match &s.doc().unwrap().node(r).unwrap().fills[0].kind {
        PaintKind::Gradient(g) => g.stops.iter().map(|st| st.color.to_hex()).collect(),
        k => panic!("{k:?}"),
    }
}

fn bound_keys(s: &Session, r: NodeId) -> Vec<String> {
    s.doc().unwrap().node(r).unwrap().bound.keys().cloned().collect()
}

#[test]
fn bound_gradient_stops_follow_their_variables_and_their_stops() {
    let mut s = Session::new();
    let r = gradient_rect(&mut s);
    tv::bind(&mut s, &[r], "fills/0/stops/2/color", "brand");
    tv::bind(&mut s, &[r], "fills/0/stops/1/color", "ink");
    assert_eq!(stops(&s, r), ["000000", "00FF00", "FF0000"]);
    tv::set(&mut s, "brand", "Mode 1", json!("#0000ff"));
    assert_eq!(stops(&s, r), ["000000", "00FF00", "0000FF"]);
    // Removing the first stop moves the bindings down with their stops.
    let cur = |s: &Session| s.doc().unwrap().node(r).unwrap().fills[0].clone();
    let mut p = cur(&s);
    if let PaintKind::Gradient(g) = &mut p.kind {
        g.stops.remove(0);
    }
    ex(&mut s, "fill.set", json!({"ids": [r], "index": 0, "paint": p}));
    assert_eq!(bound_keys(&s, r), ["fills/0/stops/0/color", "fills/0/stops/1/color"]);
    assert_eq!(stops(&s, r), ["00FF00", "0000FF"]);
    // Moving a stop (or recolouring another paint) keeps them; removing a bound stop drops its binding.
    let mut p = cur(&s);
    if let PaintKind::Gradient(g) = &mut p.kind {
        g.stops[0].position = 0.9;
        g.stops.push(ColorStop { position: 0.1, color: Color::WHITE });
    }
    ex(&mut s, "fill.set", json!({"ids": [r], "index": 0, "paint": p}));
    assert_eq!(bound_keys(&s, r).len(), 2);
    let mut p = cur(&s);
    if let PaintKind::Gradient(g) = &mut p.kind {
        g.stops.remove(1);
    }
    ex(&mut s, "fill.set", json!({"ids": [r], "index": 0, "paint": p}));
    assert_eq!(bound_keys(&s, r), ["fills/0/stops/0/color"]);
    // The paint moves up the list with its binding; a solid paint has no stops to bind.
    ex(&mut s, "fill.add", json!({"ids": [r], "color": "#cccccc"}));
    ex(&mut s, "fill.move", json!({"ids": [r], "from": 0, "to": 1}));
    assert_eq!(bound_keys(&s, r), ["fills/1/stops/0/color"]);
    ex(&mut s, "fill.set", json!({"ids": [r], "index": 1, "color": "#123456"}));
    assert!(bound_keys(&s, r).is_empty());
    // Junk stop keys bind nothing.
    assert!(crate::cmd::lists::stop_binding("stops/-1/color").is_none() && crate::cmd::lists::stop_binding("stops/1/color") == Some(1));
    healthy(&s);
}

#[test]
fn paints_replaced_through_node_set_keep_unchanged_stop_bindings() {
    let mut s = Session::new();
    let r = gradient_rect(&mut s);
    tv::bind(&mut s, &[r], "fills/0/stops/2/color", "brand");
    tv::bind(&mut s, &[r], "fills/0/stops/1/color", "ink");
    let cur = |s: &Session| s.doc().unwrap().node(r).unwrap().fills[0].clone();
    // Removing the first stop moves both bindings down with their stops.
    let mut p = cur(&s);
    if let PaintKind::Gradient(g) = &mut p.kind {
        g.stops.remove(0);
    }
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"fills": [p]}}));
    assert_eq!(bound_keys(&s, r), ["fills/0/stops/0/color", "fills/0/stops/1/color"]);
    // Recolouring one bound stop detaches that stop only.
    let mut p = cur(&s);
    if let PaintKind::Gradient(g) = &mut p.kind {
        g.stops[0].color = Color::WHITE;
    }
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"fills": [p]}}));
    assert_eq!(bound_keys(&s, r), ["fills/0/stops/1/color"]);
    tv::set(&mut s, "brand", "Mode 1", json!("#0000ff"));
    assert_eq!(stops(&s, r), ["FFFFFF", "0000FF"]);
    healthy(&s);
}
