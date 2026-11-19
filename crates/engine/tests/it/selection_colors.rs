use figcraft_color::Paint;
use figcraft_doc::NodeId;
use figcraft_engine::{
    Session,
    selection_colors::{self, SelectionColor, collect},
};
use serde_json::{Value, json};

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap()
}
fn rectangle(s: &mut Session, color: &str) -> NodeId {
    NodeId(run(s, "shape.rectangle", json!({"width":40,"height":40,"props":{"fill":color}}))["id"].as_u64().unwrap())
}
fn group(s: &Session, ids: &[NodeId], hex: &str) -> SelectionColor {
    collect(s.doc().unwrap(), ids).into_iter().find(|g| g.paint.solid_color().is_some_and(|c| c.to_hex() == hex)).unwrap()
}
fn fill(s: &Session, id: NodeId) -> String {
    s.doc().unwrap().node(id).unwrap().fills[0].solid_color().unwrap().to_hex()
}

#[test]
fn aggregate_descendants_deduplicate_roots_and_ignore_images_and_hidden_paints() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#00ff00");
    run(&mut s, "stroke.add", json!({"id":b,"color":"#ff0000"}));
    let parent = NodeId(run(&mut s, "object.group", json!({"ids":[a,b]}))["id"].as_u64().unwrap());
    let colors = collect(s.doc().unwrap(), &[parent, a]);
    assert_eq!(colors.len(), 2);
    assert_eq!(group(&s, &[parent, a], "FF0000").targets.len(), 2);
    run(&mut s, "fill.add", json!({"id":a,"paint":Paint::image("synthetic",figcraft_color::ImageScaleMode::Fill)}));
    run(&mut s, "fill.add", json!({"id":a,"paint":{"type":"solid","color":"#0000ff","visible":false}}));
    assert_eq!(collect(s.doc().unwrap(), &[parent]).len(), 2);
    run(&mut s, "select.set", json!({"ids":[parent]}));
    assert_eq!(run(&mut s, "selection.colors", json!({}))["colors"].as_array().unwrap().len(), 2);
}

#[test]
fn editing_only_matching_slots_is_one_undo_step_and_keeps_selection() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    let c = rectangle(&mut s, "#00ff00");
    run(&mut s, "stroke.add", json!({"id":b,"color":"#ff0000"}));
    run(&mut s, "select.set", json!({"ids":[a,b]}));
    let red = group(&s, &[a, b], "FF0000");
    let before = s.undo_depth();
    run(&mut s, "selection.setPaint", json!({"targets":red.targets,"paint":"#ffff00"}));
    assert_eq!(fill(&s, a), "FFFF00");
    assert_eq!(fill(&s, b), "0000FF");
    assert_eq!(fill(&s, c), "00FF00");
    assert_eq!(s.doc().unwrap().node(b).unwrap().strokes[0].solid_color().unwrap().to_hex(), "FFFF00");
    assert_eq!(s.selection(), vec![a, b]);
    assert_eq!(s.undo_depth().0, before.0 + 1);
    s.undo().unwrap();
    assert_eq!(fill(&s, a), "FF0000");
    s.redo().unwrap();
    assert_eq!(fill(&s, a), "FFFF00");
}

#[test]
fn live_picker_targets_do_not_expand_when_colors_merge() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    let red = group(&s, &[a, b], "FF0000");
    let before = s.undo_depth();
    s.begin_interaction("Color").unwrap();
    for hex in ["#0000ff", "#00ff00"] {
        run(&mut s, "selection.setPaint", json!({"targets":red.targets,"paint":hex}));
    }
    s.commit_interaction().unwrap();
    assert_eq!(fill(&s, a), "00FF00");
    assert_eq!(fill(&s, b), "0000FF");
    assert_eq!(s.undo_depth().0, before.0 + 1);
    s.undo().unwrap();
    assert_eq!(fill(&s, a), "FF0000");
}

#[test]
fn text_range_colors_use_character_offsets_and_preserve_typography() {
    let mut s = Session::new();
    let id = NodeId(run(&mut s, "text.create", json!({"text":"Яab","props":{"fill":"#ff0000"}}))["id"].as_u64().unwrap());
    run(&mut s, "text.style", json!({"id":id,"start":1,"end":2,"fontSize":30,"textFills":["#0000ff"]}));
    let blue = group(&s, &[id], "0000FF");
    assert_eq!(blue.targets[0].range, Some((1, 2)));
    run(&mut s, "selection.setPaint", json!({"targets":blue.targets,"paint":"#00ff00"}));
    let n = s.doc().unwrap().node(id).unwrap();
    let t = n.kind.text().unwrap();
    assert_eq!(t.characters, "Яab");
    assert_eq!(t.style_at(1).font_size, 30.0);
    assert_eq!(t.style_at(1).fills.as_ref().unwrap()[0].solid_color().unwrap().to_hex(), "00FF00");
    assert_eq!(t.style_at(0).fills.as_deref().unwrap_or(&n.fills)[0].solid_color().unwrap().to_hex(), "FF0000");
    assert_eq!(t.style_at(2).fills.as_deref().unwrap_or(&n.fills)[0].solid_color().unwrap().to_hex(), "FF0000");
}

#[test]
fn selecting_color_selects_its_unique_owners_without_document_history() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    run(&mut s, "stroke.add", json!({"id":a,"color":"#ff0000"}));
    let red = group(&s, &[a, b], "FF0000");
    let before = s.undo_depth();
    run(&mut s, "selection.selectColor", json!({"targets":red.targets}));
    assert_eq!(s.selection(), vec![a]);
    assert_eq!(s.undo_depth(), before);
}

#[test]
fn applying_color_styles_links_matching_fill_and_stroke_lists_only() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    run(&mut s, "stroke.add", json!({"id":b,"color":"#ff0000"}));
    let red = group(&s, &[a, b], "FF0000");
    let style = run(&mut s, "style.create", json!({"type":"paint","name":"Brand","value":"#00ff00"}))["id"].clone();
    let before = s.undo_depth();
    run(&mut s, "selection.applyColorStyle", json!({"targets":red.targets,"style":style}));
    assert_eq!(fill(&s, a), "00FF00");
    assert_eq!(fill(&s, b), "0000FF");
    assert_eq!(s.doc().unwrap().node(a).unwrap().styles.fill.as_deref(), style.as_str());
    assert_eq!(s.doc().unwrap().node(b).unwrap().styles.stroke.as_deref(), style.as_str());
    assert_eq!(s.undo_depth().0, before.0 + 1);
    run(&mut s, "style.update", json!({"style":style,"value":"#ffff00"}));
    assert_eq!(fill(&s, a), "FFFF00");
    assert_eq!(fill(&s, b), "0000FF");
}

#[test]
fn explicit_color_detaches_its_binding_but_keeps_other_bindings() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let v = theme_variable(&mut s, "Brand", "#ff0000");
    bind(&mut s, a, "fill", &v);
    let red = group(&s, &[a], "FF0000");
    run(&mut s, "selection.setPaint", json!({"targets":red.targets,"paint":"#0000ff"}));
    assert_eq!(fill(&s, a), "0000FF");
    assert!(s.doc().unwrap().node(a).unwrap().bound.is_empty());
    s.undo().unwrap();
    assert_eq!(fill(&s, a), "FF0000");
    assert!(!s.doc().unwrap().node(a).unwrap().bound.is_empty());
}

#[test]
fn invalid_or_stale_targets_roll_back_atomically() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let mut red = group(&s, &[a], "FF0000");
    let mut bad = red.targets[0].clone();
    bad.index = 999;
    red.targets.push(bad);
    let before = s.undo_depth();
    assert!(s.execute("selection.setPaint", &json!({"targets":red.targets,"paint":"#0000ff"})).is_err());
    assert_eq!(fill(&s, a), "FF0000");
    assert_eq!(s.undo_depth(), before);
    for command in ["selection.setPaint", "selection.selectColor", "selection.applyColorStyle", "selection.detachColor", "selection.createColorStyle"]
    {
        for params in [json!({}), json!({"targets":[]}), json!({"targets":[{"id":a,"fills":true,"index":0,"range":[0,1]}]})] {
            assert!(s.execute(command, &params).is_err());
        }
    }
}

#[test]
fn equal_resolved_colors_from_different_styles_or_variables_stay_separate() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#ff0000");
    let c = rectangle(&mut s, "#ff0000");
    let style = run(&mut s, "style.create", json!({"type":"paint","name":"Brand","value":"#ff0000"}))["id"].clone();
    run(&mut s, "style.apply", json!({"id":a,"style":style,"target":"fill"}));
    let variable = theme_variable(&mut s, "Accent", "#ff0000");
    bind(&mut s, b, "fill", &variable);
    let colors = collect(s.doc().unwrap(), &[a, b, c]);
    assert_eq!(colors.len(), 3);
    assert_eq!(colors[0].style.as_deref(), style.as_str());
    assert_eq!(colors[1].variable.as_deref(), Some(variable.as_str()));
    assert!(colors[2].style.is_none() && colors[2].variable.is_none());
}

#[test]
fn gradients_and_multiple_paints_are_edited_without_replacing_other_slots() {
    use figcraft_color::{Color, Gradient, GradientKind};
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let paint = Paint::gradient(Gradient::default_for(GradientKind::Linear, Color::BLACK));
    run(&mut s, "fill.add", json!({"id":a,"paint":paint}));
    let colors = collect(s.doc().unwrap(), &[a]);
    assert_eq!(colors.len(), 2);
    let gradient = colors.iter().find(|g| g.paint == paint).unwrap();
    run(&mut s, "selection.setPaint", json!({"targets":gradient.targets,"paint":"#00ff00"}));
    let paints = &s.doc().unwrap().node(a).unwrap().fills;
    assert_eq!(paints.len(), 2);
    assert_eq!(paints[0].solid_color().unwrap().to_hex(), "FF0000");
    assert_eq!(paints[1].solid_color().unwrap().to_hex(), "00FF00");
}

#[test]
fn masks_and_boolean_operands_are_not_selection_colors() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#00ff00");
    let boolean = NodeId(run(&mut s, "bool.union", json!({"ids":[a,b]}))["id"].as_u64().unwrap());
    run(&mut s, "fill.set", json!({"id":boolean,"color":"#0000ff"}));
    let colors = collect(s.doc().unwrap(), &[boolean]);
    assert_eq!(colors.len(), 1);
    assert_eq!(colors[0].paint.solid_color().unwrap().to_hex(), "0000FF");
    s.doc_mut().unwrap().get_mut(boolean).unwrap().is_mask = true;
    assert!(collect(s.doc().unwrap(), &[boolean]).is_empty());
}

#[test]
fn range_edit_keeps_a_node_binding_that_other_characters_inherit() {
    let mut s = Session::new();
    let id = NodeId(run(&mut s, "text.create", json!({"text":"abc","props":{"fill":"#ff0000"}}))["id"].as_u64().unwrap());
    let v = theme_variable(&mut s, "Brand", "#ff0000");
    bind(&mut s, id, "fill", &v);
    run(&mut s, "text.style", json!({"id":id,"start":1,"end":2,"textFills":["#0000ff"]}));
    let blue = group(&s, &[id], "0000FF");
    run(&mut s, "selection.setPaint", json!({"targets":blue.targets,"paint":"#00ff00"}));
    assert!(!s.doc().unwrap().node(id).unwrap().bound.is_empty());
    assert_eq!(fill(&s, id), "FF0000");
    assert_eq!(
        s.doc().unwrap().node(id).unwrap().kind.text().unwrap().style_at(1).fills.as_ref().unwrap()[0].solid_color().unwrap().to_hex(),
        "00FF00"
    );
}

#[test]
fn selection_color_edits_respect_read_only_version_previews() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    run(&mut s, "file.saveVersion", json!({"name":"Before"}));
    run(&mut s, "version.preview", json!({"index":0}));
    let red = group(&s, &[a], "FF0000");
    let before = s.undo_depth();
    assert!(s.execute("selection.setPaint", &json!({"targets":red.targets,"paint":"#0000ff"})).is_err());
    assert_eq!(fill(&s, a), "FF0000");
    assert_eq!(s.undo_depth(), before);
}

#[test]
fn opacity_edits_keep_the_color_variable_binding() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let v = theme_variable(&mut s, "Brand", "#ff0000");
    bind(&mut s, a, "fill", &v);
    let mut red = group(&s, &[a], "FF0000");
    red.paint.opacity = 0.5;
    run(&mut s, "selection.setPaint", json!({"targets":red.targets,"paint":red.paint}));
    let n = s.doc().unwrap().node(a).unwrap();
    assert_eq!(n.fills[0].opacity, 0.5);
    assert!(!n.bound.is_empty());
}

#[test]
fn whole_text_fill_keeps_a_color_style_link() {
    let mut s = Session::new();
    let id = NodeId(run(&mut s, "text.create", json!({"text":"Text","props":{"fill":"#ff0000"}}))["id"].as_u64().unwrap());
    let red = group(&s, &[id], "FF0000");
    assert_eq!(red.targets[0].range, None);
    let style = run(&mut s, "style.create", json!({"type":"paint","name":"Brand","value":"#0000ff"}))["id"].clone();
    run(&mut s, "selection.applyColorStyle", json!({"targets":red.targets,"style":style}));
    assert_eq!(s.doc().unwrap().node(id).unwrap().styles.fill.as_deref(), style.as_str());
    run(&mut s, "style.update", json!({"style":style,"value":"#00ff00"}));
    assert_eq!(fill(&s, id), "00FF00");
}

/// Edit the document as a file or a collaborator brings it (one gesture: normalised like an edit,
/// bound layers take their variables' values). The variables editor left (#16).
fn edit_doc(s: &mut Session, f: impl FnOnce(&mut figcraft_doc::Document)) {
    s.begin_interaction("Edit").unwrap();
    s.preview(|d, _| {
        f(d);
        Ok(())
    })
    .unwrap();
    s.commit_interaction().unwrap();
}

/// A colour variable `name` holding `hex` in a one-mode "Theme" collection → its id.
fn theme_variable(s: &mut Session, name: &str, hex: &str) -> String {
    let mut id = String::new();
    edit_doc(s, |doc| {
        let cid = match doc.variables.collection("Theme") {
            Some(c) => c.id.clone(),
            None => {
                let cid = doc.new_collection_id();
                let mode = figcraft_doc::Mode { id: format!("{cid}/mode1"), name: "Mode 1".into() };
                doc.variables.collections.push(figcraft_doc::Collection { id: cid.clone(), name: "Theme".into(), modes: vec![mode], extends: None });
                cid
            }
        };
        let modes: Vec<String> = doc.variables.collection_by_id(&cid).unwrap().modes.iter().map(|m| m.id.clone()).collect();
        let value = figcraft_doc::VariableValue::Color(figcraft_color::Color::parse_css(hex).unwrap());
        id = doc.new_variable_id();
        doc.variables.variables.push(figcraft_doc::Variable {
            id: id.clone(),
            name: name.into(),
            collection: cid,
            kind: figcraft_doc::VariableType::Color,
            values: modes.into_iter().map(|m| (m, value.clone())).collect(),
            description: String::new(),
            scopes: None,
            hidden_from_publishing: false,
            extends: None,
            overrides: Default::default(),
            code_syntax: Default::default(),
        });
    });
    id
}

/// Bind `field` of layer `id` to `variable` (an id), as a file holds the binding.
fn bind(s: &mut Session, id: NodeId, field: &str, variable: &str) {
    edit_doc(s, |doc| {
        doc.get_mut(id).unwrap().bound.insert(field.into(), variable.into());
    });
}

/// Give `variable` (an id) the colour `hex` in every mode.
fn set_variable(s: &mut Session, variable: &str, hex: &str) {
    edit_doc(s, |doc| {
        let value = figcraft_doc::VariableValue::Color(figcraft_color::Color::parse_css(hex).unwrap());
        let v = doc.variables.variables.iter_mut().find(|v| v.id == variable).unwrap();
        v.values.values_mut().for_each(|x| *x = value.clone());
    });
}

#[test]
fn detaching_a_selection_color_keeps_its_value() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#ff0000");
    let c = rectangle(&mut s, "#0000ff");
    let style = run(&mut s, "style.create", json!({"type":"paint","name":"Brand","value":"#00ff00"}))["id"].clone();
    run(&mut s, "style.apply", json!({"id":a,"style":style,"target":"fill"}));
    let v = theme_variable(&mut s, "Accent", "#ffff0080");
    bind(&mut s, b, "fill", &v);
    bind(&mut s, c, "fill", &v);
    let colors = collect(s.doc().unwrap(), &[a, b]);
    assert_eq!(colors.len(), 2);
    let before = s.undo_depth();
    for g in &colors {
        run(&mut s, "selection.detachColor", json!({"targets":g.targets}));
    }
    assert_eq!(s.undo_depth().0, before.0 + 2);
    assert!(s.doc().unwrap().node(a).unwrap().styles.fill.is_none());
    assert!(s.doc().unwrap().node(b).unwrap().bound.is_empty());
    assert!(!s.doc().unwrap().node(c).unwrap().bound.is_empty());
    assert_eq!(fill(&s, a), "00FF00");
    assert_eq!(s.doc().unwrap().node(b).unwrap().fills[0].solid_color().unwrap().to_css_hex(), "#FFFF0080");
    set_variable(&mut s, &v, "#000000");
    assert_eq!(fill(&s, b), "FFFF00");
    assert!(collect(s.doc().unwrap(), &[a, b]).iter().all(|g| g.style.is_none() && g.variable.is_none()));
}

#[test]
fn a_style_created_from_a_row_links_its_fills_and_strokes_in_one_undo_step() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    run(&mut s, "stroke.add", json!({"id":b,"color":"#ff0000"}));
    let red = group(&s, &[a, b], "FF0000");
    let before = s.undo_depth();
    let id = run(&mut s, "selection.createColorStyle", json!({"targets":red.targets,"name":"Brand/Red"}))["id"].clone();
    let doc = s.doc().unwrap();
    let style = doc.style(id.as_str().unwrap()).unwrap();
    assert_eq!(style.name, "Brand/Red");
    assert!(matches!(&style.kind, figcraft_doc::StyleKind::Paint { paints } if paints.len() == 1 && paints[0] == red.paint));
    assert_eq!(doc.node(a).unwrap().styles.fill.as_deref(), id.as_str());
    assert_eq!(doc.node(b).unwrap().styles.stroke.as_deref(), id.as_str());
    assert_eq!(doc.node(b).unwrap().styles.fill, None);
    assert_eq!(fill(&s, b), "0000FF");
    assert_eq!(s.undo_depth().0, before.0 + 1);
    run(&mut s, "style.update", json!({"style":id,"value":"#00ff00"}));
    assert_eq!(fill(&s, a), "00FF00");
    assert_eq!(s.doc().unwrap().node(b).unwrap().strokes[0].solid_color().unwrap().to_hex(), "00FF00");
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(s.doc().unwrap().style("Brand/Red").is_none());
    assert_eq!(s.doc().unwrap().node(a).unwrap().styles.fill, None);
    assert_eq!(fill(&s, a), "FF0000");
    s.redo().unwrap();
    assert_eq!(s.doc().unwrap().node(a).unwrap().styles.fill.as_deref(), id.as_str());
}

#[test]
fn a_created_style_keeps_the_rows_gradient_opacity_and_blend_mode() {
    use figcraft_color::{BlendMode, Color, Gradient, GradientKind};
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    let mut paint = Paint::gradient(Gradient::default_for(GradientKind::Radial, Color::BLACK));
    paint.opacity = 0.5;
    paint.blend = BlendMode::Multiply;
    for id in [a, b] {
        run(&mut s, "fill.set", json!({"id":id,"paint":paint}));
    }
    let row = collect(s.doc().unwrap(), &[a, b]).into_iter().find(|g| g.paint == paint).unwrap();
    assert_eq!(row.targets.len(), 2);
    let id = run(&mut s, "selection.createColorStyle", json!({"targets":row.targets,"name":"Shade"}))["id"].clone();
    let doc = s.doc().unwrap();
    assert!(matches!(&doc.style("Shade").unwrap().kind, figcraft_doc::StyleKind::Paint { paints } if paints == &vec![paint.clone()]));
    for n in [a, b] {
        assert_eq!(doc.node(n).unwrap().fills, vec![paint.clone()]);
        assert_eq!(doc.node(n).unwrap().styles.fill.as_deref(), id.as_str());
    }
}

#[test]
fn creating_a_style_refuses_taken_or_empty_names_and_stale_rows_and_changes_nothing() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#ff0000");
    run(&mut s, "style.create", json!({"type":"paint","name":"Brand","value":"#00ff00"}));
    let red = group(&s, &[a, b], "FF0000");
    let before = s.undo_depth();
    let styles = s.doc().unwrap().styles.len();
    for name in ["Brand", " Brand ", "", "   "] {
        assert!(s.execute("selection.createColorStyle", &json!({"targets":red.targets,"name":name})).is_err(), "{name:?}");
    }
    assert!(s.execute("selection.createColorStyle", &json!({"targets":red.targets})).is_err());
    // One slot of the row changed since it was listed: the row no longer stands for one paint.
    run(&mut s, "fill.set", json!({"id":b,"color":"#0000ff"}));
    let before_stale = s.undo_depth();
    assert!(s.execute("selection.createColorStyle", &json!({"targets":red.targets,"name":"Red"})).is_err());
    assert_eq!(s.undo_depth(), before_stale);
    assert_eq!(s.undo_depth().0, before.0 + 1);
    let doc = s.doc().unwrap();
    assert_eq!(doc.styles.len(), styles);
    assert_eq!(doc.node(a).unwrap().styles.fill, None);
    assert_eq!(fill(&s, a), "FF0000");
}

#[test]
fn creating_a_style_refuses_a_row_whose_list_holds_other_paints() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#ff0000");
    run(&mut s, "fill.add", json!({"id":a,"paint":"#0000ff"}));
    let red = group(&s, &[a, b], "FF0000");
    let before = s.undo_depth();
    let styles = s.doc().unwrap().styles.len();
    let err = s.execute("selection.createColorStyle", &json!({"targets":red.targets,"name":"Red"})).unwrap_err();
    assert!(err.to_string().contains("holds this one paint"), "{err}");
    assert!(selection_colors::create_refusal(s.doc().unwrap(), &red.targets).is_some());
    assert_eq!(s.undo_depth(), before);
    let doc = s.doc().unwrap();
    assert_eq!(doc.styles.len(), styles);
    assert_eq!(doc.node(a).unwrap().fills.len(), 2, "the layer keeps its other fill");
    assert!(doc.node(b).unwrap().styles.fill.is_none());
    // The single-paint layer alone still makes a style.
    let b_only: Vec<_> = red.targets.iter().filter(|t| t.id == b).cloned().collect();
    assert!(selection_colors::create_refusal(s.doc().unwrap(), &b_only).is_none());
    run(&mut s, "selection.createColorStyle", json!({"targets":b_only,"name":"Red"}));
    assert!(s.doc().unwrap().node(b).unwrap().styles.fill.is_some());
}

#[test]
fn a_style_created_from_a_variable_bound_row_keeps_following_the_variable() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#ff0000");
    let v = theme_variable(&mut s, "Brand", "#ff0000");
    for id in [a, b] {
        bind(&mut s, id, "fill", &v);
    }
    let red = group(&s, &[a, b], "FF0000");
    assert_eq!(red.targets.len(), 2);
    run(&mut s, "selection.createColorStyle", json!({"targets":red.targets,"name":"Brand"}));
    let style = s.doc().unwrap().style("Brand").unwrap();
    assert_eq!(style.bound.get("fills/0/color").map(String::as_str), Some(v.as_str()));
    set_variable(&mut s, &v, "#00ff00");
    assert_eq!(fill(&s, a), "00FF00");
    assert_eq!(fill(&s, b), "00FF00");
}

#[test]
fn a_style_created_from_a_gradient_keeps_its_stop_variables() {
    use figcraft_color::{Color, Gradient, GradientKind, PaintKind};
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let paint = Paint::gradient(Gradient::default_for(GradientKind::Linear, Color::BLACK));
    run(&mut s, "fill.set", json!({"id":a,"paint":paint}));
    let v = theme_variable(&mut s, "Brand", "#ff0000");
    bind(&mut s, a, "fills/0/stops/0/color", &v);
    let row = collect(s.doc().unwrap(), &[a]).into_iter().next().unwrap();
    run(&mut s, "selection.createColorStyle", json!({"targets":row.targets,"name":"Shade"}));
    assert!(s.doc().unwrap().style("Shade").unwrap().bound.contains_key("fills/0/stops/0/color"));
    set_variable(&mut s, &v, "#00ff00");
    let PaintKind::Gradient(g) = &s.doc().unwrap().node(a).unwrap().fills[0].kind else { panic!("not a gradient") };
    assert_eq!(g.stops[0].color.to_hex(), "00FF00");
}

#[test]
fn creating_a_style_refuses_targets_bound_to_different_variables() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#ff0000");
    let v = theme_variable(&mut s, "Brand", "#ff0000");
    bind(&mut s, a, "fill", &v);
    let targets: Vec<_> = collect(s.doc().unwrap(), &[a, b]).into_iter().flat_map(|g| g.targets).collect();
    assert_eq!(targets.len(), 2);
    assert!(selection_colors::create_refusal(s.doc().unwrap(), &targets).is_some());
    let before = s.undo_depth();
    let styles = s.doc().unwrap().styles.len();
    let err = s.execute("selection.createColorStyle", &json!({"targets":targets,"name":"Red"})).unwrap_err();
    assert!(err.to_string().contains("different variables"), "{err}");
    assert_eq!(s.undo_depth(), before);
    let doc = s.doc().unwrap();
    assert_eq!(doc.styles.len(), styles);
    assert!(!doc.node(a).unwrap().bound.is_empty());
    assert!(doc.node(b).unwrap().styles.fill.is_none());
}

#[test]
fn a_linked_row_switches_to_another_style() {
    let mut s = Session::new();
    let a = rectangle(&mut s, "#ff0000");
    let b = rectangle(&mut s, "#0000ff");
    run(&mut s, "style.create", json!({"type":"paint","name":"Brand","value":"#ff0000"}));
    let accent = run(&mut s, "style.create", json!({"type":"paint","name":"Accent","value":"#00ff00"}))["id"].clone();
    run(&mut s, "style.apply", json!({"id":a,"style":"Brand","target":"fill"}));
    let linked = collect(s.doc().unwrap(), &[a, b]).into_iter().find(|g| g.style.is_some()).unwrap();
    run(&mut s, "selection.applyColorStyle", json!({"targets":linked.targets,"style":"Accent"}));
    assert_eq!(s.doc().unwrap().node(a).unwrap().styles.fill.as_deref(), accent.as_str());
    assert_eq!(fill(&s, a), "00FF00");
    assert_eq!(fill(&s, b), "0000FF");
}
