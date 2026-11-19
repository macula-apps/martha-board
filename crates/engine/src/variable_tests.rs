//! Variable bindings in a document: bound layers take their variables' values in their modes and follow
//! new values (a file holds them; a collaborator or a library update changes them).

use serde_json::json;

use crate::Session;
use crate::params::id_of;
use crate::test_variables as tv;

fn stale_bound_color_document() -> (figcraft_doc::Document, figcraft_doc::NodeId) {
    let mut s = Session::new();
    let id = id_of(&s.execute("shape.rectangle", &json!({"x": 4000, "y": 4000, "width": 23, "height": 17})).unwrap()["id"]).unwrap();
    let page = s.doc().unwrap().first_page().unwrap();
    tv::collection(&mut s, "Theme", &["Light", "Dark"]);
    tv::variable(&mut s, "Theme", "surface", "color", &[json!("#FFFFFF"), json!("#202124")]);
    tv::variable(&mut s, "Theme", "size", "number", &[json!(23), json!(99)]);
    tv::bind(&mut s, &[id], "fill", "surface");
    tv::bind(&mut s, &[id], "width", "size");
    tv::mode(&mut s, page, "Theme", "Dark");
    let mut doc = s.doc().unwrap().clone();
    let node = doc.get_mut(id).unwrap();
    node.name = "load-binding-sentinel".into();
    node.fills = vec![figcraft_color::Paint::solid(figcraft_color::Color::WHITE)];
    node.width = 5.0;
    node.source = Some(page);
    node.overrides.insert("sentinel".into());
    doc.take_dirty();
    (doc, id)
}

fn assert_dark_bound_color(doc: &figcraft_doc::Document, transform: figcraft_doc::geom::Affine) -> figcraft_doc::NodeId {
    let node = doc.nodes().find(|n| n.name == "load-binding-sentinel").unwrap();
    assert_eq!(node.fills.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()), Some("202124".into()));
    assert_eq!((node.transform, node.width, node.height), (transform, 5.0, 17.0));
    assert!(node.bound.contains_key("fills/0/color") || node.bound.contains_key("fill"));
    node.id
}

#[test]
fn initial_color_bindings_settle_in_constructor_and_loaders() {
    let (stale, _id) = stale_bound_color_document();
    let name_transform = stale.nodes().find(|n| n.name == "load-binding-sentinel").unwrap().transform;

    let mut state = Session::with_document(stale.clone());
    let id = assert_dark_bound_color(state.doc().unwrap(), name_transform);
    assert_eq!(state.doc().unwrap().node(id).unwrap().source, stale.first_page());
    assert!(state.doc().unwrap().node(id).unwrap().overrides.contains("sentinel"));
    assert!(!state.ds().unwrap().is_dirty());
    assert!(state.doc().unwrap().dirty().is_empty());
    assert_eq!(state.undo_depth(), (0, 0));
    state.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 2, "height": 2})).unwrap();
    assert_dark_bound_color(state.doc().unwrap(), name_transform);

    let native = figcraft_format::save_bytes(&stale, None).unwrap();
    let (loaded, _) = crate::cmd::file::load_bytes(&native, "bindings.figcraft").unwrap();
    // A native file settles when it opens as a document, not in the loader.
    let opened = Session::with_document(loaded);
    let loaded = opened.doc().unwrap();
    let loaded_id = assert_dark_bound_color(loaded, name_transform);
    assert_eq!(loaded_id, id);
    assert_eq!(loaded.node(id).unwrap().source, stale.first_page());
    assert!(loaded.node(id).unwrap().overrides.contains("sentinel"));
    assert!(loaded.dirty().is_empty());
}

#[test]
fn initial_paint_settlement_preserves_existing_dirty_nodes() {
    let (mut doc, _) = stale_bound_color_document();
    let page = doc.first_page().unwrap();
    doc.mark_dirty([page]);

    crate::cmd::styles::apply_paint_bindings_on_load(&mut doc);

    assert_eq!(doc.dirty().iter().copied().collect::<Vec<_>>(), [page]);
}

#[test]
fn bound_layers_follow_variable_values_and_modes() {
    let mut s = Session::new();
    let r = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap()["id"]).unwrap();
    tv::collection(&mut s, "Tokens", &["Light", "Dark"]);
    tv::variable(&mut s, "Tokens", "size/lg", "number", &[json!(48)]);
    tv::variable(&mut s, "Tokens", "brand", "color", &[json!("#0D99FF")]);
    tv::bind(&mut s, &[r], "width", "size/lg");
    tv::bind(&mut s, &[r], "fill", "brand");
    let node = |s: &Session| s.doc().unwrap().node(r).unwrap().clone();
    assert_eq!(node(&s).width, 48.0);
    assert_eq!(node(&s).fills.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()), Some("0D99FF".into()));
    // A new value of the variable updates the bound layer.
    tv::set(&mut s, "size/lg", "Light", json!(64));
    assert_eq!(node(&s).width, 64.0);
    // A mode with another value applies through the page.
    tv::set(&mut s, "size/lg", "Dark", json!(80));
    let page = s.page().unwrap();
    tv::mode(&mut s, page, "Tokens", "Dark");
    assert_eq!(node(&s).width, 80.0);
}

#[test]
fn opacity_padding_text_and_visibility_follow_their_variables() {
    let mut s = Session::new();
    let f = id_of(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 100, "height": 100})).unwrap()["id"]).unwrap();
    s.execute("layout.add", &json!({"ids": [f]})).unwrap();
    let t = id_of(&s.execute("text.create", &json!({"text": "Hello", "x": 200, "y": 0})).unwrap()["id"]).unwrap();
    tv::collection(&mut s, "Tokens", &["Mode 1"]);
    for (name, kind, value) in
        [("fade", "number", json!(40)), ("pad", "number", json!(12)), ("label", "string", json!("Sign in")), ("shown", "boolean", json!(false))]
    {
        tv::variable(&mut s, "Tokens", name, kind, &[value]);
    }
    tv::bind(&mut s, &[f], "opacity", "fade");
    tv::bind(&mut s, &[f], "paddingHorizontal", "pad");
    tv::bind(&mut s, &[t], "characters", "label");
    tv::bind(&mut s, &[t], "visible", "shown");
    let doc = s.doc().unwrap();
    let frame = doc.node(f).unwrap();
    // Opacity variables are percentages.
    assert!((frame.opacity - 0.4).abs() < 1e-6, "{}", frame.opacity);
    let pad = frame.layout.as_ref().unwrap().padding;
    assert_eq!((pad[1], pad[3]), (12.0, 12.0));
    let text = doc.node(t).unwrap();
    assert_eq!(text.kind.text().map(|x| x.characters.as_str()), Some("Sign in"));
    assert!(!text.visible);
    // Out-of-range percentages clamp; booleans flip visibility back.
    tv::set(&mut s, "fade", "Mode 1", json!(250));
    tv::set(&mut s, "shown", "Mode 1", json!(true));
    let doc = s.doc().unwrap();
    assert_eq!(doc.node(f).unwrap().opacity, 1.0);
    assert!(doc.node(t).unwrap().visible);
}

#[test]
fn line_height_and_letter_spacing_follow_number_variables_in_pixels() {
    let mut s = Session::new();
    let t = id_of(&s.execute("text.create", &json!({"text": "Hello", "x": 0, "y": 0})).unwrap()["id"]).unwrap();
    tv::collection(&mut s, "Type", &["Mode 1"]);
    tv::variable(&mut s, "Type", "leading", "number", &[json!(24)]);
    tv::variable(&mut s, "Type", "tracking", "number", &[json!(2)]);
    tv::bind(&mut s, &[t], "lineHeight", "leading");
    tv::bind(&mut s, &[t], "letterSpacing", "tracking");
    let style = |s: &Session| s.doc().unwrap().node(t).unwrap().kind.text().unwrap().style.clone();
    assert_eq!(style(&s).line_height, figcraft_doc::LineHeight::Pixels(24.0));
    assert_eq!(style(&s).letter_spacing, figcraft_doc::LetterSpacing::Pixels(2.0));
    tv::set(&mut s, "leading", "Mode 1", json!(30));
    assert_eq!(style(&s).line_height, figcraft_doc::LineHeight::Pixels(30.0));
}

#[test]
fn effects_follow_their_variables() {
    let mut s = Session::new();
    let r = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap()["id"]).unwrap();
    s.execute("effect.add", &json!({"ids": [r], "type": "dropShadow"})).unwrap();
    s.execute("effect.add", &json!({"ids": [r], "type": "layerBlur"})).unwrap();
    tv::collection(&mut s, "Fx", &["Mode 1"]);
    for (name, kind, value) in [("y", "number", json!(6)), ("blur", "number", json!(12)), ("shadow", "color", json!("#FF000080"))] {
        tv::variable(&mut s, "Fx", name, kind, &[value]);
    }
    tv::bind(&mut s, &[r], "effects/0/offsetY", "y");
    tv::bind(&mut s, &[r], "effects/0/color", "shadow");
    tv::bind(&mut s, &[r], "effects/1/radius", "blur");
    let fx = |s: &Session| s.doc().unwrap().node(r).unwrap().effects.clone();
    match &fx(&s)[0].kind {
        figcraft_doc::EffectKind::DropShadow(sh) => {
            assert_eq!(sh.offset.1, 6.0);
            assert_eq!(sh.color.to_hex(), "FF0000");
            assert!((sh.color.a - 0.5).abs() < 0.01);
        }
        k => panic!("{k:?}"),
    }
    assert!(matches!(fx(&s)[1].kind, figcraft_doc::EffectKind::LayerBlur(ref b) if b.radius == 12.0));
    tv::set(&mut s, "blur", "Mode 1", json!(-3));
    assert!(matches!(fx(&s)[1].kind, figcraft_doc::EffectKind::LayerBlur(ref b) if b.radius == 0.0), "clamped");
}
