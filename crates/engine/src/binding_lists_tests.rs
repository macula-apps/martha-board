//! Variable bindings on paint and effect lists: a bound fill colour changes only the first fill,
//! effect bindings follow their effects when the list is reordered or shrinks, and removing the
//! first fill takes its binding with it.

use figcraft_color::{Color, PaintKind};
use figcraft_doc::{EffectKind, NodeId};
use serde_json::json;

use crate::Session;
use crate::test_variables as tv;

fn id(v: serde_json::Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn ex(s: &mut Session, cmd: &str, p: serde_json::Value) {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
}

fn setup() -> (Session, NodeId) {
    let mut s = Session::new();
    let r = id(s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap());
    tv::collection(&mut s, "T", &["Mode 1"]);
    for (name, kind, value) in [("brand", "color", json!("#FF0000")), ("small", "number", json!(2)), ("large", "number", json!(30))] {
        tv::variable(&mut s, "T", name, kind, &[json!(value)]);
    }
    (s, r)
}

fn blur(s: &Session, r: NodeId, i: usize) -> f64 {
    match s.doc().unwrap().node(r).unwrap().effects.get(i).map(|e| &e.kind) {
        Some(EffectKind::LayerBlur(b) | EffectKind::BackgroundBlur(b)) => b.radius,
        k => panic!("{k:?}"),
    }
}

fn bound(s: &Session, r: NodeId) -> Vec<(String, String)> {
    s.doc().unwrap().node(r).unwrap().bound.iter().map(|(k, v)| (k.clone(), v.to_string())).collect()
}

#[test]
fn a_bound_fill_colour_keeps_the_other_fills() {
    let (mut s, r) = setup();
    ex(&mut s, "fill.add", json!({"ids": [r], "color": "#00FF00"}));
    ex(&mut s, "fill.set", json!({"ids": [r], "index": 0, "color": "#0000FF", "opacity": 0.5}));
    tv::bind(&mut s, &[r], "fill", "brand");
    let fills = s.doc().unwrap().node(r).unwrap().fills.clone();
    assert_eq!(fills.len(), 2, "the second fill stays");
    assert_eq!(fills[0].kind, PaintKind::Solid { color: Color::from_rgb8_const(0xFF, 0, 0) });
    assert_eq!(fills[0].opacity, 0.5, "the bound paint keeps its opacity");
    assert_eq!(fills[1].kind, PaintKind::Solid { color: Color::from_rgb8_const(0, 0xFF, 0) });
    // Removing the bound (first) fill takes the binding with it.
    ex(&mut s, "fill.remove", json!({"ids": [r], "index": 0}));
    assert!(bound(&s, r).is_empty());
    tv::set_default(&mut s, "brand", json!("#123456"));
    let fills = s.doc().unwrap().node(r).unwrap().fills.clone();
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].kind, PaintKind::Solid { color: Color::from_rgb8_const(0, 0xFF, 0) }, "the remaining fill isn't recoloured");
}

#[test]
fn effect_bindings_follow_their_effects() {
    let (mut s, r) = setup();
    for _ in 0..3 {
        ex(&mut s, "effect.add", json!({"ids": [r], "type": "layerBlur"}));
    }
    tv::bind(&mut s, &[r], "effects/0/radius", "small");
    tv::bind(&mut s, &[r], "effects/2/radius", "large");
    assert_eq!((blur(&s, r, 0), blur(&s, r, 2)), (2.0, 30.0));
    // Move the "small" effect to the top: its binding goes with it.
    ex(&mut s, "effect.move", json!({"ids": [r], "from": 0, "to": 2}));
    assert_eq!((blur(&s, r, 1), blur(&s, r, 2)), (30.0, 2.0));
    tv::set_default(&mut s, "small", json!(5));
    assert_eq!((blur(&s, r, 1), blur(&s, r, 2)), (30.0, 5.0));
    // Removing the effect under the "large" one shifts its binding down.
    ex(&mut s, "effect.remove", json!({"ids": [r], "index": 0}));
    tv::set_default(&mut s, "large", json!(40));
    assert_eq!((blur(&s, r, 0), blur(&s, r, 1)), (40.0, 5.0));
    // Removing a bound effect drops its binding.
    ex(&mut s, "effect.remove", json!({"ids": [r], "index": 1}));
    let keys: Vec<String> = bound(&s, r).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, vec!["effects/0/radius".to_string()]);
    // Undo restores the list and its bindings together.
    s.undo().unwrap();
    assert_eq!(bound(&s, r).len(), 2);
}

#[test]
fn effect_moves_and_removals_reject_junk() {
    let (mut s, r) = setup();
    ex(&mut s, "effect.add", json!({"ids": [r], "type": "layerBlur"}));
    for bad in [json!({"ids": [r], "from": -1, "to": 0}), json!({"ids": [r], "from": 0}), json!({"ids": [r], "from": "x", "to": 0})] {
        assert!(s.execute("effect.move", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("effect.remove", &json!({"ids": [r], "index": -1})).is_err());
    // Out-of-range indices change nothing.
    ex(&mut s, "effect.move", json!({"ids": [r], "from": 0, "to": 5}));
    ex(&mut s, "effect.remove", json!({"ids": [r], "index": 7}));
    assert_eq!(s.doc().unwrap().node(r).unwrap().effects.len(), 1);
}

fn glass(s: &Session, r: NodeId) -> figcraft_doc::Glass {
    match s.doc().unwrap().node(r).unwrap().effects.first().map(|e| &e.kind) {
        Some(EffectKind::Glass(g)) => g.clone(),
        k => panic!("{k:?}"),
    }
}

#[test]
fn glass_fields_take_number_variables() {
    let (mut s, r) = setup();
    ex(&mut s, "effect.add", json!({"ids": [r], "type": "glass"}));
    // Percent fields read the variable as the Design panel shows them; `radius` is the frost.
    tv::bind(&mut s, &[r], "effects/0/splay", "large");
    tv::bind(&mut s, &[r], "effects/0/radius", "small");
    tv::bind(&mut s, &[r], "effects/0/depth", "large");
    let g = glass(&s, r);
    assert_eq!((g.splay, g.frost, g.depth), (0.3, 2.0, 30.0));
    // A new value follows, clamped to the field's range (frost stops at 100).
    tv::set_default(&mut s, "small", json!(150));
    tv::set_default(&mut s, "large", json!(75));
    let g = glass(&s, r);
    assert_eq!((g.splay, g.frost, g.depth), (0.75, figcraft_doc::Glass::MAX_FROST, 75.0));
    // An effect style binds them too, and every layer using it follows.
    let fx = ex_value(&mut s, "style.create", json!({"type": "effect", "name": "Pane", "from": r}))["id"].as_str().unwrap().to_string();
    tv::bind_style(&mut s, &fx, "effects/0/refraction", "large");
    assert_eq!(glass(&s, r).refraction, 0.75);
}

#[test]
fn a_binding_to_a_field_its_effect_lacks_does_nothing() {
    let (mut s, r) = setup();
    ex(&mut s, "effect.add", json!({"ids": [r], "type": "dropShadow"}));
    ex(&mut s, "effect.add", json!({"ids": [r], "type": "glass"}));
    // The fields each effect has bind.
    tv::bind(&mut s, &[r], "effects/0/spread", "small");
    tv::bind(&mut s, &[r], "effects/1/refraction", "large");
    assert_eq!(s.doc().unwrap().node(r).unwrap().bound.len(), 2);
    // A binding a file has on a field its effect lacks does nothing.
    let large = s.doc().unwrap().variables.get("large").unwrap().id.clone();
    s.doc_mut().unwrap().get_mut(r).unwrap().bound.insert("effects/0/refraction".into(), large);
    tv::set_default(&mut s, "large", json!(40));
    assert!(matches!(&s.doc().unwrap().node(r).unwrap().effects[0].kind, EffectKind::DropShadow(_)));
}

fn ex_value(s: &mut Session, cmd: &str, p: serde_json::Value) -> serde_json::Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn fill_hex(s: &Session, r: NodeId) -> Vec<String> {
    s.doc().unwrap().node(r).unwrap().fills.iter().filter_map(|p| p.solid_color()).map(|c| c.to_hex()).collect()
}
#[test]
fn explicit_paint_edits_detach_the_colour_binding() {
    // An edit that reported success keeps its colour: the binding it overwrote lets go (as
    // selection colours already do), instead of putting the variable's colour back.
    let edits = [
        ("fill.set", json!({"color": "#00FF00"})),
        ("node.set", json!({"props": {"fill": "#00FF00"}})),
        ("node.set", json!({"props": {"fills": ["#00FF00"]}})),
    ];
    for (cmd, mut p) in edits {
        let (mut s, r) = setup();
        tv::bind(&mut s, &[r], "fill", "brand");
        p["ids"] = json!([r]);
        ex(&mut s, cmd, p.clone());
        assert_eq!(fill_hex(&s, r), ["00FF00"], "{cmd} {p}");
        assert!(bound(&s, r).is_empty(), "{cmd} {p}");
        tv::set_default(&mut s, "brand", json!("#123456"));
        assert_eq!(fill_hex(&s, r), ["00FF00"], "{cmd} {p}: no longer follows the variable");
    }
    for (cmd, mut p) in [("stroke.set", json!({"color": "#00FF00"})), ("node.set", json!({"props": {"stroke": "#00FF00"}}))] {
        let (mut s, r) = setup();
        ex(&mut s, "node.set", json!({"ids": [r], "props": {"strokes": ["#000000"]}}));
        tv::bind(&mut s, &[r], "stroke", "brand");
        p["ids"] = json!([r]);
        ex(&mut s, cmd, p);
        let n = s.doc().unwrap().node(r).unwrap();
        assert_eq!(n.strokes[0].solid_color().map(|c| c.to_hex()).as_deref(), Some("00FF00"), "{cmd}");
        assert!(n.bound.is_empty(), "{cmd}");
    }
}

#[test]
fn clearing_bound_fills_adds_no_paint_back() {
    let (mut s, r) = setup();
    tv::bind(&mut s, &[r], "fill", "brand");
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"fills": []}}));
    assert!(s.doc().unwrap().node(r).unwrap().fills.is_empty());
    assert!(bound(&s, r).is_empty());
    tv::set_default(&mut s, "brand", json!("#123456"));
    assert!(s.doc().unwrap().node(r).unwrap().fills.is_empty(), "the variable doesn't bring the fill back");
}

#[test]
fn paint_opacity_edits_keep_the_colour_binding() {
    let (mut s, r) = setup();
    tv::bind(&mut s, &[r], "fill", "brand");
    ex(&mut s, "fill.set", json!({"ids": [r], "opacity": 0.5}));
    tv::set_default(&mut s, "brand", json!("#0000FF"));
    assert_eq!(fill_hex(&s, r), ["0000FF"], "the colour still follows the variable");
    assert_eq!(s.doc().unwrap().node(r).unwrap().fills[0].opacity, 0.5);
    // The same colour with another opacity, through node.set, keeps it too.
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"fills": [{"type": "solid", "color": "#0000FF", "opacity": 0.25}]}}));
    assert_eq!(bound(&s, r).len(), 1);
    assert_eq!(s.doc().unwrap().node(r).unwrap().fills[0].opacity, 0.25);
}
