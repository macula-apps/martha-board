//! What a file carrying variables keeps doing (the variables editor and its export left, #16):
//! extended collections keep their overrides through library updates, gradient stops bound to
//! variables follow their stops, and variant properties follow string variables.

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

fn var<'a>(s: &'a Session, collection: &str, name: &str) -> &'a Variable {
    let doc = s.doc().unwrap();
    doc.variables.variables.iter().find(|v| v.collection == collection && v.name == name).unwrap()
}

fn hex(v: Option<&VariableValue>) -> String {
    match v {
        Some(VariableValue::Color(c)) => c.to_hex(),
        other => format!("{other:?}"),
    }
}

fn fill_hex(s: &Session, id: NodeId) -> String {
    s.doc().unwrap().node(id).unwrap().fills.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()).unwrap_or_default()
}

fn healthy(s: &Session) {
    let doc = s.doc().unwrap();
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
}

#[test]
fn extended_collections_keep_their_overrides_through_library_updates() {
    let d = std::env::temp_dir().join(format!("figcraft-extvars-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let lib_path = d.join("theme.figcraft").to_string_lossy().into_owned();
    let mut lib = Session::new();
    tv::collection(&mut lib, "Theme", &["Light", "Dark"]);
    tv::variable(&mut lib, "Theme", "primary", "color", &[json!("#0000ff"), json!("#ffffff")]);
    tv::variable(&mut lib, "Theme", "radius", "number", &[json!(4)]);
    let lid = ex(&mut lib, "library.publish", json!({}))["id"].as_str().unwrap().to_string();
    ex(&mut lib, "file.saveAs", json!({"path": lib_path}));

    // This file holds an extension of the library's Theme, overriding one value in Dark.
    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": lib_path}));
    let parent = tv::import_collection(&mut s, &lid, "Theme");
    let ext = tv::extend(&mut s, &parent, "Theme (extended)");
    let coll = s.doc().unwrap().variables.collection_by_id(&ext).unwrap().clone();
    assert!(crate::library::is_remote_collection(s.doc().unwrap(), &parent), "it extends the library's copy");
    let (light, dark) = (coll.modes[0].id.clone(), coll.modes[1].id.clone());
    let primary = var(&s, &ext, "primary").clone();
    assert!(primary.extends.is_some() && primary.overrides.is_empty());
    assert_eq!(hex(primary.values.get(&dark)), "FFFFFF");
    assert_eq!(var(&s, &ext, "radius").values.get(&light), Some(&VariableValue::Number(4.0)));
    tv::set(&mut s, &primary.id, "Dark", json!("#ff0000"));
    assert_eq!(var(&s, &ext, "primary").overrides.iter().collect::<Vec<_>>(), [&dark]);
    let r = idv(&ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})));
    tv::bind(&mut s, &[r], "fill", &primary.id);
    tv::mode(&mut s, r, &ext, "Dark");
    assert_eq!(fill_hex(&s, r), "FF0000");

    // The library changes a value and gains a variable: inherited values follow, overrides stay.
    tv::set(&mut lib, "primary", "Light", json!("#00ff00"));
    tv::set(&mut lib, "primary", "Dark", json!("#222222"));
    tv::variable(&mut lib, "Theme", "gap", "number", &[json!(8)]);
    ex(&mut lib, "library.publish", json!({}));
    ex(&mut lib, "file.save", json!({}));
    ex(&mut s, "library.acceptUpdates", json!({}));
    let p = var(&s, &ext, "primary");
    assert_eq!((hex(p.values.get(&light)), hex(p.values.get(&dark))), ("00FF00".into(), "FF0000".into()));
    assert!(var(&s, &ext, "gap").extends.is_some(), "the new library variable is inherited");
    assert_eq!(fill_hex(&s, r), "FF0000");
    healthy(&s);

    // Saved and reopened, it is the same; removing the library keeps what it extends.
    let user = d.join("app.figcraft").to_string_lossy().into_owned();
    ex(&mut s, "file.saveAs", json!({"path": user}));
    let mut t = Session::empty();
    ex(&mut t, "file.open", json!({"path": user}));
    assert_eq!(t.doc().unwrap().variables, s.doc().unwrap().variables);
    ex(&mut s, "library.remove", json!({"id": lid}));
    assert!(s.doc().unwrap().variables.collections.iter().any(|c| c.id == parent));
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
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
fn variant_properties_follow_string_variables() {
    let mut s = Session::new();
    let mut variants = Vec::new();
    for (k, (state, color)) in [("Default", "#ff0000"), ("Hover", "#0000ff")].iter().enumerate() {
        let f =
            idv(&ex(&mut s, "frame.create", json!({"x": k as f64 * 150.0, "y": 0, "width": 100, "height": 40, "name": format!("State={state}")})));
        ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 100, "height": 40, "parent": f, "props": {"name": "Bg", "fill": color}}));
        variants.push(idv(&ex(&mut s, "component.create", json!({"ids": [f]}))));
    }
    let set = idv(&ex(&mut s, "component.combineVariants", json!({"ids": variants.clone()})));
    let i = idv(&ex(&mut s, "instance.create", json!({"component": set, "x": 0, "y": 200})));
    tv::collection(&mut s, "UI", &["Calm", "Busy"]);
    tv::variable(&mut s, "UI", "state", "string", &[json!("Default"), json!("Hover")]);
    let main = |s: &Session| match s.doc().unwrap().node(i).unwrap().kind {
        NodeKind::Instance { component, .. } => component,
        _ => panic!(),
    };
    tv::bind(&mut s, &[i], "componentProperties/State", "state");
    assert_eq!(main(&s), variants[0]);
    tv::mode(&mut s, i, "UI", "Busy");
    assert_eq!(main(&s), variants[1]);
    let bg = s.doc().unwrap().node(i).unwrap().children[0];
    assert_eq!(fill_hex(&s, bg), "0000FF", "the instance shows the variant");
    // A value naming no variant changes nothing.
    tv::set(&mut s, "state", "Busy", json!("Pressed"));
    assert_eq!(main(&s), variants[1]);
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
