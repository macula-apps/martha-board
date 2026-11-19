//! Variables behaviour suite, written from the public documentation of variables in design
//! tools, for what a file carrying variables still does (the variables editor and the `variable.*`
//! commands left, #16): aliases resolve through chains, modes inherit down the tree, bound fields
//! take their variables' values by field and type, auto layout bindings go with the auto layout
//! and explicit edits detach a binding. Each case gives the rule's source in one line; "docs
//! silent" marks behaviour the documentation doesn't describe (pinned as it is).

use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::test_variables as tv;
use figcraft_doc::{LineHeight, Node, NodeId, VariableValue};

fn id(v: Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn node(s: &Session, n: NodeId) -> Node {
    s.doc().unwrap().node(n).unwrap().clone()
}

fn fill(s: &Session, n: NodeId) -> String {
    node(s, n).fills.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()).unwrap_or_default()
}

fn frame(s: &mut Session, parent: Option<NodeId>) -> NodeId {
    id(run(s, "frame.create", json!({"parent": parent, "x": 0, "y": 0, "width": 100, "height": 100})))
}

fn rect(s: &mut Session, parent: Option<NodeId>) -> NodeId {
    id(run(s, "shape.rectangle", json!({"parent": parent, "x": 0, "y": 0, "width": 10, "height": 10})))
}

/// A "Theme" collection with Light and Dark modes and a colour variable `bg` (white / black).
fn theme(s: &mut Session) {
    tv::collection(s, "Theme", &["Light", "Dark"]);
    tv::variable(s, "Theme", "bg", "color", &[json!("#FFFFFF"), json!("#000000")]);
}

#[test]
fn aliases_resolve_through_chains() {
    // Documented: an alias points at a variable of the same type, and resolves to its value.
    let mut s = Session::new();
    theme(&mut s);
    tv::collection(&mut s, "Brand", &["Base"]);
    let blue = tv::variable(&mut s, "Brand", "blue", "color", &[json!("#0000FF")]);
    let accent = tv::variable(&mut s, "Theme", "accent", "color", &[json!("#000000")]);
    let link = tv::variable(&mut s, "Theme", "link", "color", &[json!("#000000")]);
    crate::variable_data::edit(&mut s, |doc| {
        let modes: Vec<String> = doc.variables.collection("Theme").unwrap().modes.iter().map(|m| m.id.clone()).collect();
        for m in &modes {
            tv::set_value(doc, &accent, m, VariableValue::Alias(blue.clone()));
            tv::set_value(doc, &link, m, VariableValue::Alias(accent.clone()));
        }
        Ok(())
    })
    .unwrap();
    let r = rect(&mut s, None);
    tv::bind(&mut s, &[r], "fill", "link");
    assert_eq!(fill(&s, r), "0000FF", "a chain link → accent → blue resolves");
    tv::set(&mut s, "blue", "Base", json!("#00FF00"));
    assert_eq!(fill(&s, r), "00FF00", "the chain follows the end of it");
}

#[test]
fn modes_inherit_down_the_tree() {
    // Documented: layers are on Auto by default and take the nearest container's mode (frames,
    // groups, sections, components, pages); with none set, the collection's default mode.
    let mut s = Session::new();
    theme(&mut s);
    let page = s.page().unwrap();
    let outer = frame(&mut s, None);
    let inner = frame(&mut s, Some(outer));
    let r = rect(&mut s, Some(inner));
    let free = rect(&mut s, None);
    tv::bind(&mut s, &[r, free], "fill", "bg");
    // Auto again: the explicit mode is gone (as a file can hold it).
    fn auto(s: &mut Session, id: NodeId) {
        crate::variable_data::edit(s, |doc| {
            doc.get_mut(id)?.var_modes.clear();
            Ok(())
        })
        .unwrap();
    }
    // A step: what it does to (page, outer, inner).
    type Step = fn(&mut Session, NodeId, NodeId, NodeId);
    // (step, expected nested, expected loose)
    let steps: [(&str, Step, &str, &str); 5] = [
        ("nothing set: default mode", |_, _, _, _| {}, "FFFFFF", "FFFFFF"),
        ("page Dark: everything on it", |s, p, _, _| tv::mode(s, p, "Theme", "Dark"), "000000", "000000"),
        ("outer Light: its subtree", |s, _, o, _| tv::mode(s, o, "Theme", "Light"), "FFFFFF", "000000"),
        ("inner Dark: the nearest wins", |s, _, _, i| tv::mode(s, i, "Theme", "Dark"), "000000", "000000"),
        ("inner back to Auto: outer's again", |s, _, _, i| auto(s, i), "FFFFFF", "000000"),
    ];
    for (what, step, nested, loose) in steps {
        step(&mut s, page, outer, inner);
        assert_eq!((fill(&s, r).as_str(), fill(&s, free).as_str()), (nested, loose), "{what}");
    }
    // Moving a layer into another container: it takes that one's mode.
    run(&mut s, "node.reparent", json!({"ids": [free], "parent": outer}));
    assert_eq!(fill(&s, free), "FFFFFF");
}

#[test]
fn bindings_by_field_and_type() {
    // Documented: which properties each type applies to (number: sizes, gaps, padding, radius,
    // opacity, font properties, effects, strokes, text content; colour: fills, strokes, effects;
    // string: text content, font family/style; boolean: visibility). Opacity numbers are percent
    // (as the field shows); other numbers are pixels.
    let mut s = Session::new();
    tv::collection(&mut s, "C", &["Mode 1"]);
    for (name, ty, value) in
        [("n", "number", json!(12)), ("c", "color", json!("#336699")), ("t", "string", json!("Hi")), ("b", "boolean", json!(false))]
    {
        tv::variable(&mut s, "C", name, ty, &[value]);
    }
    let f = frame(&mut s, None);
    run(&mut s, "layout.add", json!({"ids": [f]}));
    let r = rect(&mut s, None);
    run(&mut s, "node.set", json!({"id": r, "props": {"strokes": ["#000000"], "strokeWeight": 1}}));
    let t = id(run(&mut s, "text.create", json!({"text": "Hello", "x": 0, "y": 200})));
    type Check = fn(&Node) -> bool;
    let ok: [(NodeId, &str, &str, Check); 14] = [
        (r, "width", "n", |n| n.width == 12.0),
        (r, "height", "n", |n| n.height == 12.0),
        (r, "cornerRadius", "n", |n| n.corners.uniform() == Some(12.0)),
        (r, "opacity", "n", |n| (n.opacity - 0.12).abs() < 1e-5),
        (r, "strokeWeight", "n", |n| n.stroke.weight == 12.0),
        (f, "gap", "n", |n| n.layout.as_ref().is_some_and(|l| l.gap == 12.0)),
        (f, "paddingTop", "n", |n| n.layout.as_ref().is_some_and(|l| l.padding[0] == 12.0)),
        (t, "fontSize", "n", |n| n.kind.text().is_some_and(|x| x.style.font_size == 12.0)),
        (t, "lineHeight", "n", |n| n.kind.text().is_some_and(|x| x.style.line_height == LineHeight::Pixels(12.0))),
        (t, "paragraphSpacing", "n", |n| n.kind.text().is_some_and(|x| x.paragraph_spacing == 12.0)),
        (r, "fill", "c", |n| n.fills.first().and_then(|p| p.solid_color()).is_some_and(|c| c.to_hex() == "336699")),
        (r, "strokes/0/color", "c", |n| n.strokes.first().and_then(|p| p.solid_color()).is_some_and(|c| c.to_hex() == "336699")),
        (t, "characters", "t", |n| n.kind.text().is_some_and(|x| x.characters == "Hi")),
        (t, "visible", "b", |n| !n.visible),
    ];
    for (layer, field, var, check) in ok {
        tv::bind(&mut s, &[layer], field, var);
        assert!(check(&node(&s, layer)), "{field} ← {var}");
    }
    // Number variables in text content show as text (documented: number → text content).
    let t2 = id(run(&mut s, "text.create", json!({"text": "x", "x": 0, "y": 300})));
    tv::bind(&mut s, &[t2], "characters", "n");
    assert_eq!(node(&s, t2).kind.text().map(|x| x.characters.clone()).as_deref(), Some("12"));
}

#[test]
fn auto_layout_bindings_go_with_the_auto_layout() {
    // Docs silent; pinned (#810): removing a frame's auto layout drops its gap and padding bindings,
    // and a binding never brings an auto layout back. One made without auto layout waits for it.
    let mut s = Session::new();
    tv::collection(&mut s, "C", &["Mode 1"]);
    tv::variable(&mut s, "C", "g", "number", &[json!(12)]);
    let removals =
        [("layout.remove", json!({})), ("node.set", json!({"props": {"layout": null}})), ("node.set", json!({"props": {"layout": "none"}}))];
    for (cmd, mut p) in removals {
        let f = frame(&mut s, None);
        run(&mut s, "layout.add", json!({"ids": [f]}));
        for field in ["gap", "crossGap", "paddingTop"] {
            tv::bind(&mut s, &[f], field, "g");
        }
        p["ids"] = json!([f]);
        run(&mut s, cmd, p.clone());
        let n = node(&s, f);
        assert!(n.layout.is_none(), "{cmd} {p}: auto layout stays removed");
        assert!(n.bound.is_empty(), "{cmd} {p}: layout bindings dropped");
    }
    let f = frame(&mut s, None);
    tv::bind(&mut s, &[f], "gap", "g");
    assert!(node(&s, f).layout.is_none(), "binding gap adds no auto layout");
    run(&mut s, "layout.add", json!({"ids": [f]}));
    assert_eq!(node(&s, f).layout.map(|l| l.gap), Some(12.0), "the waiting binding applies to the new auto layout");
}

#[test]
fn explicit_edits_of_bound_fields_stick_and_detach_that_binding() {
    // #809: an edit of a bound field detaches its binding (as a paint edit does) instead of
    // the variable's value coming back; bindings whose value the edit kept stay.
    let mut s = Session::new();
    tv::collection(&mut s, "C", &["Mode 1"]);
    let w = tv::variable(&mut s, "C", "w", "number", &[json!(48)]);
    let o = tv::variable(&mut s, "C", "o", "number", &[json!(40)]);
    let r = rect(&mut s, None);
    for (field, v) in [("width", &w), ("height", &w), ("opacity", &o)] {
        tv::bind(&mut s, &[r], field, v);
    }
    assert_eq!((node(&s, r).width, node(&s, r).opacity), (48.0, 0.4));

    run(&mut s, "node.set", json!({"ids": [r], "width": 100, "opacity": 0.4}));
    let n = node(&s, r);
    assert_eq!(n.width, 100.0);
    assert!(!n.bound.contains_key("width"));
    assert!(n.bound.contains_key("height") && n.bound.contains_key("opacity"), "unchanged values keep their bindings");

    run(&mut s, "transform.resize", json!({"ids": [r], "height": 30}));
    assert_eq!(node(&s, r).height, 30.0);
    assert!(!node(&s, r).bound.contains_key("height"));

    run(&mut s, "object.opacityKeys", json!({"ids": [r], "digits": "9"}));
    assert_eq!(node(&s, r).opacity, 0.9);
    assert!(node(&s, r).bound.is_empty());
}

#[test]
fn a_gesture_or_a_text_style_on_a_bound_field_detaches_it() {
    let mut s = Session::new();
    tv::collection(&mut s, "C", &["Mode 1"]);
    let v = tv::variable(&mut s, "C", "r", "number", &[json!(8)]);
    let r = rect(&mut s, None);
    tv::bind(&mut s, &[r], "cornerRadius", &v);
    s.begin_interaction("Edit").unwrap();
    let props = serde_json::Map::from_iter([("cornerRadius".to_string(), json!(2))]);
    s.preview(|d, _| crate::cmd::props::apply("node.set", d, r, &props)).unwrap();
    s.commit_interaction().unwrap();
    assert_eq!(node(&s, r).corners.radii, [2.0; 4]);
    assert!(node(&s, r).bound.is_empty());

    let t = id(run(&mut s, "text.create", json!({"x": 0, "y": 0, "text": "Hi"})));
    tv::bind(&mut s, &[t], "fontSize", &v);
    run(&mut s, "text.style", json!({"id": t, "fontSize": 20}));
    assert!(node(&s, t).bound.is_empty());
    assert_eq!(node(&s, t).kind.text().unwrap().style.font_size, 20.0);

    // The font size/weight/tracking shortcuts and text content edits detach too.
    tv::bind(&mut s, &[t], "fontSize", &v);
    assert_eq!(node(&s, t).kind.text().unwrap().style.font_size, 8.0);
    run(&mut s, "text.sizeUp", json!({"ids": [t]}));
    assert_eq!(node(&s, t).kind.text().unwrap().style.font_size, 9.0);
    assert!(node(&s, t).bound.is_empty());
    let c = tv::variable(&mut s, "C", "c", "string", &[json!("Hello")]);
    tv::bind(&mut s, &[t], "characters", &c);
    run(&mut s, "text.set", json!({"id": t, "characters": "Bye"}));
    assert_eq!(node(&s, t).kind.text().unwrap().characters, "Bye");
    assert!(node(&s, t).bound.is_empty());
}

#[test]
fn a_translucent_variable_bound_to_an_empty_fill_applies_its_alpha_once() {
    // #1514: the first paint added by a binding must not take the colour's alpha as opacity too.
    let mut s = Session::new();
    tv::collection(&mut s, "Theme", &["Light", "Dark"]);
    tv::variable(&mut s, "Theme", "tint", "color", &[json!("#FF000080"), json!("#00FF0040")]);
    let r = id(run(&mut s, "node.create", json!({"type": "rectangle", "props": {"fills": []}})));
    tv::bind(&mut s, &[r], "fill", "tint");
    assert_eq!(node(&s, r).fills[0].opacity, 1.0);
    tv::mode(&mut s, r, "Theme", "Dark");
    assert_eq!(node(&s, r).fills[0].solid_color().unwrap().to_css_hex(), "#00FF0040");
    assert_eq!(node(&s, r).fills[0].opacity, 1.0);
}
