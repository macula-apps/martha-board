//! A colour variable's alpha applies once to the paint it is bound to (#802): a `#RRGGBBAA` fill keeps
//! its alpha in the paint opacity, and binding a variable with alpha hands the variable the whole
//! colour, so the two alphas don't multiply (canvas, SVG, mode switches, unbinding, styles).

use figcraft_doc::NodeId;
use serde_json::json;

use crate::Session;
use crate::test_variables as tv;

/// 0x80 / 255 and 0x40 / 255.
const HALF: f32 = 128.0 / 255.0;
const QUARTER: f32 = 64.0 / 255.0;

fn ex(s: &mut Session, cmd: &str, p: serde_json::Value) {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
}

/// A rectangle filled `#0D6EFD80` and a colour variable `tint` (Light `light`, Dark `dark`).
fn setup(light: &str, dark: &str) -> (Session, NodeId) {
    let mut s = Session::new();
    let r = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10, "props": {"fill": "#0D6EFD80"}})).unwrap();
    let r = NodeId(r["id"].as_u64().unwrap());
    tv::collection(&mut s, "Tokens", &["Light", "Dark"]);
    tv::variable(&mut s, "Tokens", "tint", "color", &[json!(light)]);
    tv::set(&mut s, "tint", "Dark", json!(dark));
    (s, r)
}

/// The fill's effective alpha (colour alpha times paint opacity).
fn alpha(s: &Session, r: NodeId) -> f32 {
    s.doc().unwrap().node(r).unwrap().fills[0].solid_color().unwrap().a
}

/// The alpha byte of the rectangle rendered on its own (transparent backdrop).
fn pixel_alpha(s: &Session, r: NodeId) -> u8 {
    let pix = figcraft_render::render_node(s.doc().unwrap(), r, 1.0).unwrap();
    let i = ((5 * pix.width() + 5) * 4 + 3) as usize;
    pix.data()[i]
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn a_hex_with_alpha_keeps_its_alpha_in_the_opacity() {
    let (s, r) = setup("#0D6EFD80", "#0D6EFD40");
    let p = &s.doc().unwrap().node(r).unwrap().fills[0];
    assert_eq!(p.kind, figcraft_color::PaintKind::Solid { color: figcraft_color::Color::from_rgb8_const(0x0D, 0x6E, 0xFD) });
    assert!(near(p.opacity, HALF));
    assert_eq!(pixel_alpha(&s, r), 128);
}

#[test]
fn a_bound_variable_alpha_applies_once() {
    // The fill and the variable are both #0D6EFD80: the result is 50%, not 25%.
    let (mut s, r) = setup("#0D6EFD80", "#0D6EFD40");
    tv::bind(&mut s, &[r], "fill", "tint");
    assert!(near(alpha(&s, r), HALF), "{}", alpha(&s, r));
    assert_eq!(pixel_alpha(&s, r), 128);
    let svg = figcraft_svg::export_svg(s.doc().unwrap(), &[r], &figcraft_svg::SvgOptions::default()).unwrap();
    assert!(svg.contains("fill=\"#0D6EFD\" fill-opacity=\"0.502\""), "{svg}");
    // Binding the same variable again changes nothing.
    tv::bind(&mut s, &[r], "fill", "tint");
    assert!(near(alpha(&s, r), HALF));
}

#[test]
fn unbinding_keeps_the_colour_with_its_alpha_in_the_opacity() {
    let (mut s, r) = setup("#0D6EFD40", "#0D6EFD");
    tv::bind(&mut s, &[r], "fill", "tint");
    assert_eq!(pixel_alpha(&s, r), 64);
    tv::unbind(&mut s, r, "fill");
    let p = &s.doc().unwrap().node(r).unwrap().fills[0];
    assert_eq!(p.kind, figcraft_color::PaintKind::Solid { color: figcraft_color::Color::from_rgb8_const(0x0D, 0x6E, 0xFD) });
    assert!(near(p.opacity, QUARTER), "{}", p.opacity);
    assert_eq!(pixel_alpha(&s, r), 64);
    assert_eq!(pixel_alpha(&s, r), 64);
}

#[test]
fn modes_switch_the_alpha_without_accumulating() {
    let (mut s, r) = setup("#0D6EFD40", "#0D6EFD80");
    tv::bind(&mut s, &[r], "fill", "tint");
    let page = s.page().unwrap();
    for (mode, want) in [("Dark", HALF), ("Light", QUARTER), ("Dark", HALF), ("Light", QUARTER)] {
        tv::mode(&mut s, page, "Tokens", mode);
        assert!(near(alpha(&s, r), want), "{mode}: {}", alpha(&s, r));
    }
    // An opacity set while bound multiplies on top and survives mode switches.
    ex(&mut s, "fill.set", json!({"ids": [r.0], "index": 0, "opacity": 0.5}));
    tv::mode(&mut s, page, "Tokens", "Dark");
    assert!(near(alpha(&s, r), HALF * 0.5));
}

#[test]
fn style_bindings_apply_the_alpha_once() {
    let (mut s, r) = setup("#0D6EFD80", "#0D6EFD40");
    // A paint style made from a `#RRGGBBAA` colour, bound to the variable.
    let st = s.execute("style.create", &json!({"type": "paint", "name": "Tint", "value": "#0D6EFD80"})).unwrap()["id"].clone();
    tv::bind_style(&mut s, st.as_str().unwrap(), "fills/0/color", "tint");
    ex(&mut s, "style.apply", json!({"ids": [r.0], "style": st}));
    assert!(near(alpha(&s, r), HALF), "{}", alpha(&s, r));
}

#[test]
fn binding_keeps_an_opacity_set_on_the_paint() {
    // A 15% tint whose colour comes from a variable (#923): the bind replaces the colour only.
    let mut s = Session::new();
    tv::collection(&mut s, "Tokens", &["Light"]);
    tv::variable(&mut s, "Tokens", "red", "color", &[json!("#FF0000")]);
    let props = json!({"fills": [{"type": "solid", "color": "#000000", "opacity": 0.15}]});
    let r = s.execute("node.create", &json!({"type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10, "props": props})).unwrap();
    let r = NodeId(r["id"].as_u64().unwrap());
    tv::bind(&mut s, &[r], "fill", "red");
    let p = s.doc().unwrap().node(r).unwrap().fills[0].clone();
    assert_eq!(p.kind, figcraft_color::PaintKind::Solid { color: figcraft_color::Color::from_rgb8_const(0xFF, 0, 0) });
    assert!(near(p.opacity, 0.15), "{}", p.opacity);
    tv::unbind(&mut s, r, "fill");
    assert!(near(s.doc().unwrap().node(r).unwrap().fills[0].opacity, 0.15));
    // An opacity set on a `#RRGGBBAA` fill is the user's too, and stays.
    let (mut s, r) = setup("#FF0000", "#FF0000");
    ex(&mut s, "fill.set", json!({"ids": [r.0], "index": 0, "opacity": 0.25}));
    tv::bind(&mut s, &[r], "fill", "tint");
    assert!(near(alpha(&s, r), 0.25), "{}", alpha(&s, r));
}

#[test]
fn an_unbind_and_rebind_keeps_the_opacity_set_on_the_paint() {
    // A paint at 50% bound to a half-transparent colour shows a quarter, unbound or bound again.
    let (mut s, r) = setup("#FF000080", "#FF000080");
    ex(&mut s, "fill.set", json!({"ids": [r.0], "index": 0, "opacity": 0.5}));
    for bound in [true, false, true, false] {
        if bound {
            tv::bind(&mut s, &[r], "fill", "tint");
        } else {
            tv::unbind(&mut s, r, "fill");
        }
        assert!(near(alpha(&s, r), HALF / 2.0), "bound {bound}: {}", alpha(&s, r));
    }
}
