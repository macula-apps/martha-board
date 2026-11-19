//! Reordering paints, effects and layout grids (`fill.move`, `stroke.move`, `effect.move`,
//! `grid.move`): one undo step each, items keep everything, variable bindings and style links
//! follow; layout grid fields bound to variables.

use figcraft_color::{Color, Gradient, GradientKind, ImageScaleMode, Paint};
use figcraft_doc::{GridKind, LayoutGrid, Node, NodeId};
use serde_json::{Value, json};

use crate::Session;
use crate::test_variables as tv;

fn id(v: Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn node(s: &Session, id: NodeId) -> Node {
    s.doc().unwrap().node(id).unwrap().clone()
}

fn rect(s: &mut Session) -> NodeId {
    id(ex(s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})))
}

fn hexes(paints: &[Paint]) -> Vec<String> {
    paints.iter().map(|p| p.solid_color().map(|c| c.to_hex()).unwrap_or_else(|| "-".into())).collect()
}

fn vars(s: &mut Session) {
    tv::collection(s, "T", &["Light", "Dark"]);
    for (name, kind, value) in [("brand", "color", json!("#FF0000")), ("gap", "number", json!(12)), ("cols", "number", json!(6))] {
        tv::variable(s, "T", name, kind, &[json!(value)]);
    }
}

#[test]
fn fill_move_reorders_as_one_undo_step_and_keeps_every_paint() {
    let mut s = Session::new();
    let r = rect(&mut s);
    let gradient = Paint::gradient(Gradient::default_for(GradientKind::Radial, Color::from_hex("#00FF00").unwrap()));
    let mut image = Paint::image("synthetic-hash", ImageScaleMode::Fit);
    image.opacity = 0.4;
    let mut red = Paint::solid(Color::from_hex("#FF0000").unwrap());
    red.visible = false;
    let fills = vec![red.clone(), gradient.clone(), image.clone()];
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"fills": fills}}));
    let undo = s.ds().unwrap().history.undo.len();
    // Bottom (0) to top (2).
    ex(&mut s, "fill.move", json!({"ids": [r], "from": 0, "to": 2}));
    assert_eq!(node(&s, r).fills, vec![gradient.clone(), image.clone(), red.clone()], "image, gradient and hidden paint survive whole");
    assert_eq!(s.ds().unwrap().history.undo.len(), undo + 1, "one undo step");
    ex(&mut s, "fill.move", json!({"ids": [r], "from": 2, "to": 1}));
    assert_eq!(node(&s, r).fills, vec![gradient.clone(), red.clone(), image.clone()]);
    ex(&mut s, "edit.undo", json!({}));
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(node(&s, r).fills, fills);
    ex(&mut s, "edit.redo", json!({}));
    assert_eq!(node(&s, r).fills, vec![gradient, image, red]);
}

#[test]
fn moves_out_of_range_change_nothing_and_junk_is_refused() {
    let mut s = Session::new();
    let r = rect(&mut s);
    ex(&mut s, "fill.add", json!({"ids": [r], "color": "#00FF00"}));
    let before = node(&s, r).fills;
    let undo = s.ds().unwrap().history.undo.len();
    for cmd in ["fill.move", "stroke.move", "effect.move", "grid.move"] {
        // Nothing to move: no change.
        ex(&mut s, cmd, json!({"ids": [r], "from": 0, "to": 9}));
        ex(&mut s, cmd, json!({"ids": [r], "from": 1, "to": 1}));
        for bad in [json!({"ids": [r], "from": -1, "to": 0}), json!({"ids": [r], "from": 0.5, "to": 1}), json!({"ids": [r], "from": 0})] {
            assert!(s.execute(cmd, &bad).is_err(), "{cmd} {bad}");
        }
    }
    assert_eq!(node(&s, r).fills, before);
    assert_eq!(s.ds().unwrap().history.undo.len(), undo, "no-op moves add no undo steps");
    // A negative index never removes the first paint or grid.
    assert!(s.execute("fill.remove", &json!({"ids": [r], "index": -1})).is_err());
    ex(&mut s, "node.set", json!({"ids": [r], "props": {"grids": [LayoutGrid::default()]}}));
    assert!(s.execute("grid.remove", &json!({"ids": [r], "index": -1})).is_err());
    assert_eq!(node(&s, r).fills, before);
    assert_eq!(node(&s, r).grids.len(), 1);
}

#[test]
fn a_bound_paint_colour_follows_its_paint() {
    let mut s = Session::new();
    let r = rect(&mut s);
    vars(&mut s);
    ex(&mut s, "fill.set", json!({"ids": [r], "index": 0, "color": "#0000FF"}));
    ex(&mut s, "fill.add", json!({"ids": [r], "color": "#00FF00"}));
    ex(&mut s, "fill.add", json!({"ids": [r], "color": "#FFFFFF"}));
    tv::bind(&mut s, &[r], "fill", "brand");
    assert_eq!(hexes(&node(&s, r).fills), ["FF0000", "00FF00", "FFFFFF"]);
    // The bound bottom paint goes to the top: the binding goes with it.
    ex(&mut s, "fill.move", json!({"ids": [r], "from": 0, "to": 2}));
    assert_eq!(node(&s, r).bound.keys().collect::<Vec<_>>(), ["fills/2/color"]);
    tv::set_default(&mut s, "brand", json!("#123456"));
    assert_eq!(hexes(&node(&s, r).fills), ["00FF00", "FFFFFF", "123456"], "the neighbours aren't recoloured");
    // Modes recolour the moved paint too.
    tv::set(&mut s, "brand", "Dark", json!("#ABCDEF"));
    tv::mode(&mut s, r, "T", "Dark");
    assert_eq!(hexes(&node(&s, r).fills), ["00FF00", "FFFFFF", "ABCDEF"]);
    // Removing a paint below shifts the binding down with its paint.
    ex(&mut s, "fill.remove", json!({"ids": [r], "index": 0}));
    assert_eq!(node(&s, r).bound.keys().collect::<Vec<_>>(), ["fills/1/color"]);
    tv::set(&mut s, "brand", "Dark", json!("#010203"));
    assert_eq!(hexes(&node(&s, r).fills), ["FFFFFF", "010203"]);
    // Undo (of the remove: the value and mode edits are no undo steps) brings back the binding.
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(node(&s, r).bound.keys().collect::<Vec<_>>(), ["fills/2/color"]);
    // Removing the bound paint drops its binding.
    ex(&mut s, "fill.remove", json!({"ids": [r], "index": 2}));
    assert!(node(&s, r).bound.is_empty());
}

#[test]
fn strokes_move_with_their_bindings_and_moves_unlink_styles() {
    let mut s = Session::new();
    let (a, b) = (rect(&mut s), rect(&mut s));
    vars(&mut s);
    for r in [a, b] {
        ex(&mut s, "stroke.add", json!({"ids": [r], "color": "#000000"}));
        ex(&mut s, "stroke.add", json!({"ids": [r], "color": "#00FF00"}));
    }
    tv::bind(&mut s, &[a, b], "stroke", "brand");
    // (Adding a fill to a styled layer would unlink its style: add it first.)
    ex(&mut s, "fill.add", json!({"ids": [a, b], "color": "#FFFFFF"}));
    ex(&mut s, "style.create", json!({"type": "paint", "name": "Two", "from": a}));
    ex(&mut s, "style.apply", json!({"ids": [a, b], "style": "Two"}));
    assert!(node(&s, a).styles.fill.is_some());
    // One command moves the strokes of every selected layer.
    ex(&mut s, "stroke.move", json!({"ids": [a, b], "from": 0, "to": 1}));
    for r in [a, b] {
        assert_eq!(hexes(&node(&s, r).strokes), ["00FF00", "FF0000"]);
        assert_eq!(node(&s, r).bound.keys().collect::<Vec<_>>(), ["strokes/1/color"]);
    }
    tv::set_default(&mut s, "brand", json!("#0000FF"));
    assert_eq!(hexes(&node(&s, b).strokes), ["00FF00", "0000FF"]);
    // A paint style defined the whole fill list: reordering it unlinks the style.
    let fills = node(&s, a).fills.len();
    assert!(fills >= 2);
    ex(&mut s, "fill.move", json!({"ids": [a], "from": 0, "to": fills - 1}));
    assert!(node(&s, a).styles.fill.is_none());
    assert!(node(&s, b).styles.fill.is_some(), "only the moved layer unlinks");
}

#[test]
fn bound_paint_keys_resolve_by_index() {
    use crate::cmd::lists::{BoundList, list_binding};
    assert_eq!(list_binding("fill"), Some((BoundList::Fills, 0, "color")));
    assert_eq!(list_binding("stroke"), Some((BoundList::Strokes, 0, "color")));
    assert_eq!(list_binding("fills/3/color"), Some((BoundList::Fills, 3, "color")));
    assert_eq!(list_binding("layoutGrids/1/gutter"), Some((BoundList::Grids, 1, "gutter")));
    for junk in
        ["fills/-1/color", "fills/+1/color", "fills//color", "fills/1/", "effects/x/radius", "grids/0/size", "fills/99999999999999999999999/color"]
    {
        assert_eq!(list_binding(junk), None, "{junk}");
    }
}

#[test]
fn a_paint_binding_follows_the_moved_paint() {
    let mut s = Session::new();
    let r = rect(&mut s);
    vars(&mut s);
    ex(&mut s, "fill.add", json!({"ids": [r], "color": "#00FF00"}));
    tv::bind(&mut s, &[r], "fill", "brand");
    ex(&mut s, "fill.move", json!({"ids": [r], "from": 0, "to": 1}));
    let doc = s.doc().unwrap();
    let n = doc.node(r).unwrap();
    let names: Vec<Option<&str>> = (0..n.fills.len())
        .map(|i| crate::cmd::lists::paint_binding_key(n, true, i).and_then(|k| doc.variables.get_by_id(&n.bound[k])).map(|v| v.name.as_str()))
        .collect();
    assert_eq!(names, [None, Some("brand")], "the binding follows the moved paint");
}

#[test]
fn layout_grid_fields_bind_to_number_variables_and_follow_moves() {
    let mut s = Session::new();
    let f = id(ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 400, "height": 300})));
    vars(&mut s);
    ex(&mut s, "grid.add", json!({"ids": [f], "grid": {"kind": "columns", "count": 4, "gutter": 20}}));
    ex(&mut s, "grid.add", json!({"ids": [f], "grid": {"kind": "rows", "count": 3}}));
    tv::bind(&mut s, &[f], "layoutGrids/0/gutter", "gap");
    tv::bind(&mut s, &[f], "layoutGrids/0/count", "cols");
    let g = node(&s, f).grids[0].clone();
    assert_eq!((g.kind, g.count, g.gutter), (GridKind::Columns, 6, 12.0));
    // Values and modes flow in.
    tv::set(&mut s, "gap", "Dark", json!(32));
    tv::mode(&mut s, f, "T", "Dark");
    assert_eq!(node(&s, f).grids[0].gutter, 32.0);
    // Junk numbers are kept in range.
    tv::set(&mut s, "cols", "Dark", json!(-5));
    assert_eq!(node(&s, f).grids[0].count, 1);
    tv::set(&mut s, "cols", "Dark", json!(1e12));
    assert_eq!(node(&s, f).grids[0].count, 1000);
    tv::set(&mut s, "cols", "Dark", json!(5));
    // Move the bound grid on top: its bindings go with it, the rows grid isn't touched.
    ex(&mut s, "grid.move", json!({"ids": [f], "from": 0, "to": 1}));
    let keys: Vec<String> = node(&s, f).bound.keys().cloned().collect();
    assert_eq!(keys, ["layoutGrids/1/count", "layoutGrids/1/gutter"]);
    tv::set(&mut s, "gap", "Dark", json!(8));
    let gs = node(&s, f).grids;
    assert_eq!((gs[0].kind, gs[0].count), (GridKind::Rows, 3));
    assert_eq!((gs[1].kind, gs[1].count, gs[1].gutter), (GridKind::Columns, 5, 8.0));
    // Removing the grid below shifts them down; removing the bound grid drops them.
    ex(&mut s, "grid.remove", json!({"ids": [f], "index": 0}));
    assert!(node(&s, f).bound.keys().all(|k| k.starts_with("layoutGrids/0/")));
    ex(&mut s, "grid.remove", json!({"ids": [f], "index": 0}));
    assert!(node(&s, f).bound.is_empty());
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(node(&s, f).bound.len(), 2);
}
