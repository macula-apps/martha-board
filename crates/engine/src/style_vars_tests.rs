//! Variables bound inside shared styles, as a file holds them (`style.bind` left with the variables
//! editor, #16): a layer using the style showing it resolved in its own modes (and staying linked),
//! the style's own value in the default modes, variables and modes changing afterwards, styles made
//! from bound layers, and weights.

use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::test_variables as tv;
use figcraft_doc::{Node, NodeId, StyleKind};

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
    node(s, n).fills[0].solid_color().unwrap().to_css_hex().to_uppercase()
}

fn style_fill(s: &Session, sid: &str) -> String {
    match &s.doc().unwrap().style(sid).unwrap().kind {
        StyleKind::Paint { paints } => paints[0].solid_color().unwrap().to_css_hex().to_uppercase(),
        k => panic!("{k:?}"),
    }
}

/// A Theme collection (Light, Dark) with a colour `bg` (white, black) and numbers `size` (16, 20)
/// and `blur` (4, 12).
fn theme(s: &mut Session) {
    tv::collection(s, "Theme", &["Light", "Dark"]);
    tv::variable(s, "Theme", "bg", "color", &[json!("#FFFFFF"), json!("#000000")]);
    tv::variable(s, "Theme", "size", "number", &[json!(16), json!(20)]);
    tv::variable(s, "Theme", "blur", "number", &[json!(4), json!(12)]);
}

/// Two frames (the second set to Dark), a red rectangle in each, and a paint style "Surface"
/// (red) both rectangles use.
fn two_modes(s: &mut Session) -> (NodeId, NodeId, NodeId, NodeId, String) {
    theme(s);
    let frame = |s: &mut Session, x: f64| id(run(s, "frame.create", json!({"x": x, "y": 0, "width": 100, "height": 100})));
    let (light, dark) = (frame(s, 0.0), frame(s, 200.0));
    tv::mode(s, dark, "Theme", "Dark");
    let rect = |s: &mut Session, f: NodeId| {
        id(run(s, "shape.rectangle", json!({"parent": f, "x": 10, "y": 10, "width": 40, "height": 40, "props": {"fill": "#FF0000"}})))
    };
    let (a, b) = (rect(s, light), rect(s, dark));
    let sid = run(s, "style.create", json!({"type": "paint", "name": "Surface", "from": a}))["id"].as_str().unwrap().to_string();
    run(s, "style.apply", json!({"ids": [b], "style": sid}));
    (light, dark, a, b, sid)
}

#[test]
fn a_bound_paint_style_shows_in_each_layers_mode() {
    let mut s = Session::new();
    let (_, dark, a, b, sid) = two_modes(&mut s);
    tv::bind_style(&mut s, &sid, "fill", "bg");
    // The layer in the Light frame is white, the one in the Dark frame black; both stay linked.
    assert_eq!((fill(&s, a), fill(&s, b)), ("#FFFFFF".to_string(), "#000000".to_string()));
    assert_eq!((node(&s, a).styles.fill, node(&s, b).styles.fill), (Some(sid.clone()), Some(sid.clone())));
    // The style itself holds the default mode's value, under the one spelling of the key.
    assert_eq!(style_fill(&s, &sid), "#FFFFFF");
    let st = s.doc().unwrap().style(&sid).unwrap().clone();
    assert_eq!(st.bound.keys().collect::<Vec<_>>(), ["fills/0/color"]);
    // The variable's value changes: every user follows, in its mode.
    tv::set(&mut s, "bg", "Dark", json!("#112233"));
    assert_eq!((fill(&s, a), fill(&s, b)), ("#FFFFFF".to_string(), "#112233".to_string()));
    // A frame's mode changes: the layers in it follow, still linked.
    tv::mode(&mut s, dark, "Theme", "Light");
    assert_eq!((fill(&s, b), node(&s, b).styles.fill), ("#FFFFFF".to_string(), Some(sid.clone())));
    tv::mode(&mut s, dark, "Theme", "Dark");
    assert_eq!(fill(&s, b), "#112233");
    // A layer moved into the other frame takes that frame's mode.
    run(&mut s, "node.reparent", json!({"id": a, "parent": dark}));
    assert_eq!((fill(&s, a), node(&s, a).styles.fill), ("#112233".to_string(), Some(sid.clone())));
}

#[test]
fn editing_a_bound_style_or_its_users() {
    let mut s = Session::new();
    let (_, _, a, b, sid) = two_modes(&mut s);
    tv::bind_style(&mut s, &sid, "fill", "bg");
    // An edit of the layer's own fill still unlinks it (the documented rule for styled properties).
    run(&mut s, "node.set", json!({"id": b, "props": {"fill": "#00FF00"}}));
    assert_eq!((fill(&s, b), node(&s, b).styles.fill), ("#00FF00".to_string(), None));
    // Applying the style again gives the layer's mode's value, not the stored one.
    run(&mut s, "style.apply", json!({"ids": [b], "style": sid}));
    assert_eq!(fill(&s, b), "#000000");
    // A new value without the bound paint drops the binding with it.
    run(&mut s, "style.update", json!({"style": sid, "value": []}));
    assert!(s.doc().unwrap().style(&sid).unwrap().bound.is_empty());
    assert!(node(&s, a).fills.is_empty() && node(&s, a).styles.fill == Some(sid));
}

#[test]
fn effect_grid_and_text_styles_take_variables_too() {
    let mut s = Session::new();
    let (_, dark, a, b, _) = two_modes(&mut s);
    // Effect: a layer blur's radius.
    run(&mut s, "effect.add", json!({"ids": [a], "type": "layerBlur"}));
    let fx = run(&mut s, "style.create", json!({"type": "effect", "name": "Soft", "from": a}))["id"].as_str().unwrap().to_string();
    run(&mut s, "style.apply", json!({"ids": [b], "style": fx}));
    tv::bind_style(&mut s, &fx, "effects/0/radius", "blur");
    let radius = |s: &Session, n: NodeId| node(s, n).effects[0].blur().unwrap().radius;
    assert_eq!((radius(&s, a), radius(&s, b)), (4.0, 12.0));
    assert_eq!((node(&s, a).styles.effect.is_some(), node(&s, b).styles.effect.is_some()), (true, true));
    // Grid: the frames' layout grid size.
    let light = node(&s, a).parent.unwrap();
    run(&mut s, "grid.add", json!({"ids": [light]}));
    let grid = run(&mut s, "style.create", json!({"type": "grid", "name": "Base", "from": light}))["id"].as_str().unwrap().to_string();
    run(&mut s, "style.apply", json!({"ids": [dark], "style": grid}));
    tv::bind_style(&mut s, &grid, "layoutGrids/0/size", "size");
    assert_eq!((node(&s, light).grids[0].size, node(&s, dark).grids[0].size), (16.0, 20.0));
    // Text: the font size, for chars linked to the style.
    let text = |s: &mut Session, f: NodeId| id(run(s, "text.create", json!({"text": "Hello", "x": 0, "y": 60, "parent": f})));
    let (t1, t2) = (text(&mut s, light), text(&mut s, dark));
    let body = run(&mut s, "style.create", json!({"type": "text", "name": "Body", "from": t1}))["id"].as_str().unwrap().to_string();
    run(&mut s, "style.apply", json!({"ids": [t2], "style": body}));
    tv::bind_style(&mut s, &body, "fontSize", "size");
    let size = |s: &Session, n: NodeId| node(s, n).kind.text().unwrap().style.font_size;
    assert_eq!((size(&s, t1), size(&s, t2)), (16.0, 20.0));
    assert_eq!((node(&s, t1).styles.text.clone(), node(&s, t2).styles.text.clone()), (Some(body.clone()), Some(body.clone())));
    tv::set(&mut s, "size", "Dark", json!(28));
    assert_eq!((size(&s, t2), node(&s, t2).styles.text.clone(), node(&s, dark).grids[0].size), (28.0, Some(body), 28.0));
}

#[test]
fn a_style_made_from_a_bound_layer_keeps_the_binding() {
    let mut s = Session::new();
    theme(&mut s);
    let r =
        id(run(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 40, "height": 40, "props": {"fill": "#FF0000", "strokes": ["#00FF00"]}})));
    tv::bind(&mut s, &[r], "stroke", "bg");
    let sid = run(&mut s, "style.create", json!({"type": "stroke", "name": "Edge", "from": r}))["id"].as_str().unwrap().to_string();
    let st = s.doc().unwrap().style(&sid).unwrap().clone();
    assert_eq!(st.bound.keys().collect::<Vec<_>>(), ["fills/0/color"], "a paint style's paints are its fills");
    // The layer's own binding gave way to the style's.
    let n = node(&s, r);
    assert!(n.bound.is_empty() && n.styles.stroke == Some(sid) && n.strokes[0].solid_color().unwrap().to_css_hex().to_uppercase() == "#FFFFFF");
}

/// A text style "Body" (made from a layer, or from `value`) used by a layer in the Light frame
/// and one in the Dark one; returns (light layer, dark layer, style id).
fn body_style(s: &mut Session, value: Option<Value>) -> (NodeId, NodeId, String) {
    let (_, dark, a, _, _) = two_modes(s);
    let light = node(s, a).parent.unwrap();
    let text = |s: &mut Session, f: NodeId| id(run(s, "text.create", json!({"text": "Hello", "x": 0, "y": 60, "parent": f})));
    let (t1, t2) = (text(s, light), text(s, dark));
    let from = match &value {
        Some(v) => json!({"type": "text", "name": "Body", "value": v}),
        None => json!({"type": "text", "name": "Body", "from": t1}),
    };
    let body = run(s, "style.create", from)["id"].as_str().unwrap().to_string();
    run(s, "style.apply", json!({"ids": [t1, t2], "style": body}));
    (t1, t2, body)
}

fn text_of(s: &Session, n: NodeId) -> figcraft_doc::TextStyle {
    node(s, n).kind.text().unwrap().style.clone()
}

#[test]
fn a_number_weight_picks_the_face_and_the_wght_axis() {
    let mut s = Session::new();
    let (t1, t2, body) = body_style(&mut s, None);
    tv::variable(&mut s, "Theme", "weight", "number", &[json!(700), json!(400)]);
    tv::bind_style(&mut s, &body, "fontWeight", "weight");
    let face = |s: &Session, n: NodeId| {
        let t = text_of(s, n);
        (t.font_style, t.variations.get("wght").copied())
    };
    assert_eq!(face(&s, t1), ("Bold".to_string(), Some(700.0)));
    assert_eq!(face(&s, t2), ("Regular".to_string(), Some(400.0)));
    tv::set(&mut s, "weight", "Dark", json!(650));
    assert_eq!(face(&s, t2), ("Bold".to_string(), Some(650.0)));
    // A style name bound instead of the number (a style binds one of the two).
    tv::collection(&mut s, "Names", &["Only"]);
    tv::variable(&mut s, "Names", "face", "string", &[json!("Light")]);
    tv::bind_style(&mut s, &body, "fontStyle", "face");
    let bound = s.doc().unwrap().style(&body).unwrap().bound.clone();
    assert!(bound.contains_key("fontStyle") && !bound.contains_key("fontWeight"), "{bound:?}");
    // The name replaces the number's weight: no stale `wght` axis keeps drawing 700.
    assert_eq!(face(&s, t1), ("Light".to_string(), None));
    assert_eq!(text_of(&s, t1).weight(), 300);
}

#[test]
fn junk_weight_values_clamp_to_the_lightest_and_heaviest_face() {
    let mut s = Session::new();
    let (t1, _, body) = body_style(&mut s, None);
    tv::variable(&mut s, "Theme", "weight", "number", &[json!(-50), json!(5000)]);
    tv::bind_style(&mut s, &body, "fontWeight", "weight");
    let face = |s: &Session| {
        let t = text_of(s, t1);
        (t.font_style, t.variations.get("wght").copied())
    };
    assert_eq!(face(&s), ("Thin".to_string(), Some(1.0)));
    tv::set(&mut s, "weight", "Light", json!(5000));
    assert_eq!(face(&s), ("Black".to_string(), Some(1000.0)));
}

#[test]
fn font_weight_wins_over_font_style_when_a_file_carries_both() {
    let mut s = Session::new();
    let (t1, _, body) = body_style(&mut s, None);
    tv::variable(&mut s, "Theme", "weight", "number", &[json!(700), json!(400)]);
    tv::collection(&mut s, "Names", &["Only"]);
    tv::variable(&mut s, "Names", "face", "string", &[json!("Light")]);
    let doc = s.doc_mut().unwrap();
    let (w, f) = (doc.variables.get("weight").unwrap().id.clone(), doc.variables.get("face").unwrap().id.clone());
    let st = doc.styles.iter_mut().find(|x| x.id == body).unwrap();
    st.bound.insert("fontStyle".into(), f);
    st.bound.insert("fontWeight".into(), w);
    crate::style_vars::settle(doc);
    crate::style_vars::refresh(doc, None, None);
    assert_eq!(text_of(&s, t1).font_style, "Bold");
}

#[test]
fn a_number_bound_to_a_percent_line_height_applies_as_pixels() {
    let mut s = Session::new();
    let (t1, _, body) = body_style(&mut s, Some(json!({"fontSize": 24, "lineHeight": "150%"})));
    tv::variable(&mut s, "Theme", "lh", "number", &[json!(150), json!(36)]);
    tv::bind_style(&mut s, &body, "lineHeight", "lh");
    let lh = |s: &Session| match &s.doc().unwrap().style(&body).unwrap().kind {
        StyleKind::Text { style, .. } => style.line_height,
        k => panic!("{k:?}"),
    };
    assert_eq!(lh(&s), figcraft_doc::LineHeight::Pixels(150.0));
    assert_eq!(text_of(&s, t1).line_height, figcraft_doc::LineHeight::Pixels(150.0));
    assert_eq!(node(&s, t1).height, 150.0);
    tv::set(&mut s, "lh", "Light", json!(36));
    assert_eq!((lh(&s), node(&s, t1).height), (figcraft_doc::LineHeight::Pixels(36.0), 36.0));
}
