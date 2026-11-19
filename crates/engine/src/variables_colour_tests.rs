//! Colour calculations a file holds: an alpha calculation (a referenced colour with its alpha
//! replaced) and a mix (a blend of two colour operands) as per-mode variable values — propagation
//! to bound layers and mode switches, composition over aliases and other calculations, the
//! save/load round trip and colour profile conversion. Setting them left with the variables editor
//! (#16); a file, a collaborator or a library update brings them.

use serde_json::json;

use crate::Session;
use crate::components::tests::{ex, idv};
use crate::paint_tests::fill;
use crate::plugins_tests::rect;
use crate::test_variables as tv;
use figcraft_color::Color;
use figcraft_doc::{ColorOperand, VariableValue};

/// A "C" collection (mode M) with `ink` #FF0000, `edge` #00FF00 and the number `size`; the ids of
/// ink and edge.
fn seeds() -> (Session, String, String) {
    let mut s = Session::new();
    tv::collection(&mut s, "C", &["M"]);
    let ink = tv::variable(&mut s, "C", "ink", "color", &[json!("#FF0000")]);
    let edge = tv::variable(&mut s, "C", "edge", "color", &[json!("#00FF00")]);
    tv::variable(&mut s, "C", "size", "number", &[json!(5)]);
    (s, ink, edge)
}

/// `variable`'s colour (an id) with its alpha replaced by `opacity`.
fn alpha(variable: &str, opacity: f32) -> VariableValue {
    VariableValue::ColorAlpha { variable: variable.to_string(), opacity }
}

/// A blend of `from` and `to` at `t`.
fn mix(from: ColorOperand, to: ColorOperand, t: f32) -> VariableValue {
    VariableValue::ColorMix { from, to, t }
}

fn var(id: &str) -> ColorOperand {
    ColorOperand::Variable { variable: id.to_string() }
}

fn lit(hex: &str) -> ColorOperand {
    ColorOperand::Literal { literal: Color::parse_css(hex).unwrap() }
}

/// A colour variable `name` in "C" holding `value` in every mode → its id.
fn calc(s: &mut Session, name: &str, value: VariableValue) -> String {
    let id = tv::variable(s, "C", name, "color", &[json!("#000000")]);
    tv::set_all(s, &id, value);
    id
}

#[test]
fn alpha_and_mix_propagate_to_bound_layers() {
    let (mut s, ink, edge) = seeds();
    let hover = calc(&mut s, "hover", alpha(&ink, 0.5));
    let box_id = rect(&mut s, 0.0);
    tv::bind(&mut s, &[box_id], "fill", &hover);
    assert_eq!(fill(&s, box_id).to_rgba8(), [255, 0, 0, 128], "alpha replaces the source's alpha");
    tv::set(&mut s, "ink", "M", json!("#00AA00"));
    assert_eq!(fill(&s, box_id).to_rgba8(), [0, 170, 0, 128], "the bound layer follows the source, no re-set needed");
    // A mix of two variables, one literal, and the t edges.
    tv::set_all(&mut s, &hover, mix(var(&ink), var(&edge), 0.0));
    assert_eq!(fill(&s, box_id).to_rgba8(), [0, 170, 0, 255], "t=0 gives from exactly");
    tv::set_all(&mut s, &hover, mix(lit("#000000"), lit("#FFFFFF"), 0.4));
    assert_eq!(fill(&s, box_id).to_rgba8(), [102, 102, 102, 255], "literal operands blend (0.4 -> 0x66)");
    // Compose: a mix reading the calculation, and alpha over an alias.
    let mirror = calc(&mut s, "mirror", VariableValue::Alias(ink.clone()));
    let faded = calc(&mut s, "faded-mirror", alpha(&mirror, 0.5));
    let b2 = rect(&mut s, 0.0);
    tv::bind(&mut s, &[b2], "fill", &faded);
    assert_eq!(fill(&s, b2).to_rgba8(), [0, 170, 0, 128], "alpha over an alias reads the aliased value");
    let toast = calc(&mut s, "toast", alpha(&hover, 0.25));
    let b3 = rect(&mut s, 0.0);
    tv::bind(&mut s, &[b3], "fill", &toast);
    assert_eq!(fill(&s, b3).to_rgba8(), [102, 102, 102, 64], "a calculation may read a calculation");
}

#[test]
fn per_mode_operands_follow_each_collection_chosen_mode() {
    let mut s = Session::new();
    tv::collection(&mut s, "Core", &["Light", "Dark"]);
    tv::collection(&mut s, "Extra", &["Light", "Dark"]);
    let p = tv::variable(&mut s, "Core", "p", "color", &[json!("#FF0000"), json!("#0000FF")]);
    let x = tv::variable(&mut s, "Extra", "s", "color", &[json!("#000000"), json!("#FFFFFF")]);
    let panel = tv::variable(&mut s, "Core", "panel", "color", &[json!("#000000")]);
    tv::set_all(&mut s, &panel, mix(var(&p), var(&x), 0.5));
    let b = rect(&mut s, 0.0);
    tv::bind(&mut s, &[b], "fill", &panel);
    assert_eq!(fill(&s, b).to_rgba8(), [128, 0, 0, 255], "Light mixes Light values of BOTH collections");
    tv::mode(&mut s, b, "Core", "Dark");
    assert_eq!(fill(&s, b).to_rgba8(), [0, 0, 128, 255], "a Core switch moves only the Core operand (Extra stays Light)");
    tv::mode(&mut s, b, "Extra", "Dark");
    assert_eq!(fill(&s, b).to_rgba8(), [128, 128, 255, 255], "the Extra switch then moves its operand, independently");
}

#[test]
fn calculations_survive_save_and_load() {
    let (mut s, ink, edge) = seeds();
    calc(&mut s, "mixv", mix(var(&ink), var(&edge), 0.4));
    let dir = std::env::temp_dir().join(format!("figcraft-colour-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("d.figcraft");
    ex(&mut s, "file.save", json!({"path": path.to_string_lossy()}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let mixv = t.doc().unwrap().variables.get("mixv").unwrap().clone();
    assert!(matches!(mixv.values.values().next(), Some(VariableValue::ColorMix { .. })), "still a calculation after a round trip");
    // And it still propagates in the reloaded document.
    let b = idv(&ex(&mut t, "shape.rectangle", json!({"x": 0, "y": 0, "width": 4, "height": 4})));
    tv::bind(&mut t, &[b], "fill", "mixv");
    assert_eq!(fill(&t, b).to_rgba8(), [153, 102, 0, 255], "ink + edge at 0.4: r=255*(1-0.4), g=255*0.4");
}

#[test]
fn profile_conversion_reaches_the_colours_stored_in_a_calculation() {
    // A mix of identical literals shows that colour; after a converting profile change the
    // mix-bound layer must match a layer bound to the plain colour (the literal converted too).
    let (mut s, _, _) = seeds();
    tv::variable(&mut s, "C", "p", "color", &[json!("#804020")]);
    calc(&mut s, "dl", mix(lit("#804020"), lit("#804020"), 0.5));
    let a = rect(&mut s, 0.0);
    let b = rect(&mut s, 0.0);
    tv::bind(&mut s, &[a], "fill", "p");
    tv::bind(&mut s, &[b], "fill", "dl");
    ex(&mut s, "file.colorProfile", json!({"profile": "displayP3"}));
    let (fa, fb) = (fill(&s, a), fill(&s, b));
    assert_ne!(fa.to_rgba8(), [128, 64, 32, 255], "the profile change converted the plain colour");
    assert_eq!(fa.to_rgba8(), fb.to_rgba8(), "the colour stored inside the calculation converted the same way");
}
