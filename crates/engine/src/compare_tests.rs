//! The compare behind branch review: what changed between two versions of a document, layer by
//! layer, and each layer's properties as the compare formats them.

use crate::Session;
use crate::compare::props::{Prop, Section, properties};
use crate::compare::{ChangeKind, Comparison, compare_docs};
use crate::components::tests::{card, copy_of, ex, idv};
use crate::test_variables as tv;
use figcraft_doc::*;
use serde_json::{Value, json};

/// A "Screen" frame holding a "Title" text, a "Button" rectangle and a "Badge" group.
struct Screen {
    frame: NodeId,
    title: NodeId,
    button: NodeId,
    badge: NodeId,
    dot: NodeId,
}

fn screen(s: &mut Session) -> Screen {
    let frame = idv(&ex(s, "frame.create", json!({"x": 0, "y": 0, "width": 400, "height": 300, "name": "Screen"})));
    let title = idv(&ex(s, "text.create", json!({"text": "Welcome", "x": 20, "y": 20, "parent": frame, "props": {"name": "Title"}})));
    let button = idv(&ex(
        s,
        "shape.rectangle",
        json!({"x": 20, "y": 200, "width": 120, "height": 40, "parent": frame, "props": {"name": "Button", "fill": "#FF0000"}}),
    ));
    let dot = idv(&ex(s, "shape.ellipse", json!({"x": 300, "y": 20, "width": 20, "height": 20, "parent": frame, "props": {"name": "Dot"}})));
    let badge = idv(&ex(s, "object.group", json!({"ids": [dot], "name": "Badge"})));
    Screen { frame, title, button, badge, dot }
}

/// What changed in `s`'s document since `before`.
fn since(before: &Document, s: &Session) -> Comparison {
    compare_docs(before, s.doc().unwrap())
}

fn texts(c: &Comparison, id: NodeId) -> Vec<String> {
    c.changes.iter().filter(|l| l.id == id).flat_map(|l| l.properties.iter().map(|p| p.text.clone())).collect()
}

fn kind(c: &Comparison, id: NodeId) -> Option<ChangeKind> {
    c.changes.iter().find(|l| l.id == id).map(|l| l.change)
}

#[test]
fn property_changes_read_like_inspect() {
    let mut s = Session::new();
    let sc = screen(&mut s);
    let before = s.doc().unwrap().clone();
    ex(&mut s, "node.set", json!({"id": sc.button, "props": {"width": 136, "fill": "#E03030", "name": "Primary", "cornerRadius": 8}}));
    ex(&mut s, "node.set", json!({"id": sc.button, "props": {"opacity": 0.5, "x": 30}}));
    ex(&mut s, "text.set", json!({"id": sc.title, "characters": "Hello there"}));
    ex(&mut s, "node.set", json!({"id": sc.title, "props": {"fontSize": 24}}));
    ex(&mut s, "node.set", json!({"id": sc.frame, "props": {"fills": ["#F0F0F0"]}}));
    let c = since(&before, &s);
    assert!(!c.truncated);
    let button = texts(&c, sc.button);
    for want in ["name Button → Primary", "width 120 → 136", "left 20 → 30", "opacity 100% → 50%", "corner radius 0 → 8", "fill #FF0000 → #E03030"]
    {
        assert!(button.contains(&want.to_string()), "{want} in {button:?}");
    }
    let title = texts(&c, sc.title);
    assert!(title.contains(&"text “Welcome” → “Hello there”".to_string()), "{title:?}");
    assert!(title.iter().any(|t| t.starts_with("font size") && t.ends_with("→ 24")), "{title:?}");
    assert!(texts(&c, sc.frame).contains(&"fill #FFFFFF → #F0F0F0".to_string()));
    assert_eq!(kind(&c, sc.badge), None, "untouched layers aren't listed");
    assert_eq!((c.added, c.removed, c.modified), (0, 0, 3));
    // Sections and levels are reported with each change.
    let fill = c.changes.iter().find(|l| l.id == sc.button).and_then(|l| l.properties.iter().find(|p| p.property == "fill")).unwrap();
    assert_eq!((fill.section, fill.before.as_str(), fill.after.as_str()), ("Fills", "#FF0000", "#E03030"));
    assert_eq!(c.changes.iter().find(|l| l.id == sc.button).map(|l| l.depth), Some(2), "page › Screen › Button");
    // Auto layout and effects.
    ex(&mut s, "layout.add", json!({"ids": [sc.frame]}));
    ex(&mut s, "node.set", json!({"id": sc.dot, "props": {"effects": [{"type": "dropShadow"}]}}));
    let c = since(&before, &s);
    assert!(texts(&c, sc.frame).iter().any(|t| t.starts_with("auto layout none → ")), "{:?}", texts(&c, sc.frame));
    assert!(texts(&c, sc.dot).iter().any(|t| t.starts_with("drop shadow none → ")), "{:?}", texts(&c, sc.dot));
}

#[test]
fn added_removed_moved_and_reordered_layers() {
    let mut s = Session::new();
    let sc = screen(&mut s);
    let before = s.doc().unwrap().clone();
    let extra = idv(&ex(&mut s, "shape.rectangle", json!({"x": 200, "y": 100, "width": 10, "height": 10, "parent": sc.frame})));
    let wrap = idv(&ex(&mut s, "object.group", json!({"ids": [extra], "name": "New group"})));
    ex(&mut s, "edit.delete", json!({"ids": [sc.title]}));
    // The dot leaves its group (the group, emptied, disappears) for the frame.
    ex(&mut s, "node.reparent", json!({"ids": [sc.dot], "parent": sc.frame}));
    let c = since(&before, &s);
    assert_eq!(kind(&c, wrap), Some(ChangeKind::Added));
    assert_eq!(c.changes.iter().find(|l| l.id == wrap).map(|l| l.layers), Some(2), "the group and its rectangle");
    assert_eq!(kind(&c, extra), None, "listed with its group");
    assert_eq!(kind(&c, sc.title), Some(ChangeKind::Removed));
    assert_eq!(kind(&c, sc.badge), Some(ChangeKind::Removed));
    assert!(texts(&c, sc.dot).contains(&"parent Badge → Screen".to_string()), "{:?}", texts(&c, sc.dot));
    // Removed layers come from the before version, with their names.
    let removed = c.changes.iter().find(|l| l.id == sc.title).unwrap();
    assert_eq!((removed.name.as_str(), removed.type_name), ("Title", "TEXT"));
    // Reordering among the layers both versions have.
    let mut s = Session::new();
    let sc = screen(&mut s);
    let before = s.doc().unwrap().clone();
    ex(&mut s, "arrange.front", json!({"ids": [sc.title]}));
    let c = since(&before, &s);
    assert!(texts(&c, sc.title).iter().any(|t| t.starts_with("order ") && t.ends_with("→ 1")), "{:?}", texts(&c, sc.title));
}

#[test]
fn nested_and_instance_changes_are_found() {
    let mut s = Session::new();
    let (c, bg, title, _dot) = card(&mut s);
    let frame = idv(&ex(&mut s, "frame.create", json!({"x": 500, "y": 0, "width": 400, "height": 300, "name": "Screen"})));
    let inst = idv(&ex(&mut s, "instance.create", json!({"component": c, "x": 10, "y": 10, "parent": frame})));
    let group_child = idv(&ex(&mut s, "shape.rectangle", json!({"x": 10, "y": 200, "width": 10, "height": 10, "parent": frame})));
    let outer = idv(&ex(&mut s, "object.group", json!({"ids": [group_child], "name": "Outer"})));
    let _inner = idv(&ex(&mut s, "object.group", json!({"ids": [group_child], "name": "Inner"})));
    let before = s.doc().unwrap().clone();
    // A main component edit reaches the instance in the frame.
    ex(&mut s, "node.set", json!({"id": bg, "props": {"fill": "#0000FF"}}));
    let ibg = copy_of(&s, inst, bg);
    let cmp = since(&before, &s);
    assert!(texts(&cmp, ibg).contains(&"fill #EEEEEE → #0000FF".to_string()), "{:?}", cmp.changes);
    // An override inside the instance, and a change deep in nested groups.
    let ititle = copy_of(&s, inst, title);
    ex(&mut s, "text.set", json!({"id": ititle, "characters": "Inbox"}));
    ex(&mut s, "node.set", json!({"id": group_child, "props": {"fill": "#00FF00"}}));
    let cmp = since(&before, &s);
    assert!(texts(&cmp, ititle).contains(&"text “Hello” → “Inbox”".to_string()), "{:?}", cmp.changes);
    let deep = cmp.changes.iter().find(|l| l.id == group_child).expect("nested change");
    assert_eq!(deep.depth, 4, "page › frame › Outer › Inner › layer");
    assert!(cmp.changes.iter().all(|l| l.id != outer), "the groups themselves didn't change");
    // Swapping the instance's component is reported by name.
    let (c2, ..) = card(&mut s);
    ex(&mut s, "node.set", json!({"id": c2, "props": {"name": "Card 2"}}));
    ex(&mut s, "instance.swap", json!({"ids": [inst], "component": c2}));
    let cmp = since(&before, &s);
    assert!(texts(&cmp, inst).contains(&"component Card → Card 2".to_string()), "{:?}", texts(&cmp, inst));
}

// Moved from the Inspect tab's tests when Dev Mode left (#11): the properties the compare formats.

fn pid(v: Value) -> NodeId {
    crate::params::id_of(&v["id"]).unwrap()
}

fn row<'a>(sections: &'a [Section], title: &str, label: &str) -> &'a Prop {
    sections.iter().find(|s| s.title == title).and_then(|s| s.rows.iter().find(|r| r.label == label)).unwrap_or_else(|| panic!("{title}/{label}"))
}

#[test]
fn properties_group_layout_auto_layout_paints_and_tokens() {
    let mut s = Session::new();
    let f = pid(s.execute("frame.create", &json!({"x": 100, "y": 50, "width": 300, "height": 200})).unwrap());
    let r = pid(s.execute("shape.rectangle", &json!({"x": 130, "y": 90, "width": 80, "height": 40, "parent": f})).unwrap());
    s.execute("node.set", &json!({"id": r, "props": {"fill": "#0D99FF80", "cornerRadius": 6, "opacity": 0.5}})).unwrap();
    tv::collection(&mut s, "T", &["Mode 1"]);
    tv::variable(&mut s, "T", "radius/sm", "number", &[json!(6)]);
    tv::bind(&mut s, &[r], "cornerRadius", "radius/sm");
    s.execute("select.set", &json!({"ids": [r]})).unwrap();
    s.execute("style.create", &json!({"type": "paint", "name": "Brand/Blue"})).unwrap();
    s.execute("style.apply", &json!({"ids": [r], "style": "Brand/Blue"})).unwrap();
    let doc = s.doc().unwrap();
    let p = properties(doc, r);
    assert_eq!(row(&p, "Layout", "Width").value, "80");
    // Top/left from the parent frame's corner (the rectangle was placed in the frame's coordinates).
    assert_eq!((row(&p, "Layout", "Top").value.as_str(), row(&p, "Layout", "Left").value.as_str()), ("90", "130"));
    assert_eq!(row(&p, "Layout", "Opacity").value, "50%");
    let radius = row(&p, "Layout", "Radius");
    assert_eq!((radius.value.as_str(), radius.token.as_deref()), ("6", Some("radius/sm")));
    let fill = &p.iter().find(|s| s.title == "Fills").unwrap().rows[0];
    assert_eq!(fill.value, "#0D99FF · 50%");
    assert_eq!(fill.token.as_deref(), Some("Brand/Blue"));
    assert!(p.iter().all(|s| s.title != "Auto layout" && s.title != "Typography"));

    // Auto layout: direction, gap, padding shorthand, alignment.
    s.execute("layout.add", &json!({"ids": [f]})).unwrap();
    s.execute("node.set", &json!({"id": f, "props": {"layoutMode": "vertical", "gap": 12, "padding": [8, 16, 8, 16]}})).unwrap();
    let p = properties(s.doc().unwrap(), f);
    assert_eq!(row(&p, "Auto layout", "Direction").value, "Vertical");
    assert_eq!(row(&p, "Auto layout", "Gap").value, "12");
    assert_eq!(row(&p, "Auto layout", "Padding").value, "8 16");
    // At the top level, positions are from the page origin.
    assert_eq!(row(&p, "Layout", "Left").value, "100");
    assert!(properties(s.doc().unwrap(), NodeId(999_999)).is_empty(), "no such layer");
}

#[test]
fn stored_variable_ids_keep_their_property_token() {
    let mut s = Session::new();
    tv::collection(&mut s, "Theme", &["Mode 1"]);
    let shadow = tv::variable(&mut s, "Theme", "shadow", "color", &[json!("#FF0000")]);
    let surface = tv::variable(&mut s, "Theme", "surface", "color", &[json!("#0D99FF")]);
    let r = pid(s.execute("node.create", &json!({"type": "rectangle"})).unwrap());
    tv::bind(&mut s, &[r], "fill", &surface);
    tv::edit_variable(&mut s, "surface", |v| {
        v.code_syntax.insert("WEB".into(), "--real-surface".into());
    });
    // A variable named like another's id: the token comes from the id the layer is bound to.
    tv::edit_variable(&mut s, &shadow, |v| {
        v.name.clone_from(&surface);
        v.code_syntax.insert("WEB".into(), "--shadow".into());
    });

    let p = properties(s.doc().unwrap(), r);
    let fill = row(&p, "Fills", "Solid");
    assert_eq!((fill.value.as_str(), fill.token.as_deref()), ("#0D99FF", Some("surface")));
}

#[test]
fn typography_section_for_text() {
    let mut s = Session::new();
    let t = pid(s.execute("text.create", &json!({"text": "Hello", "x": 0, "y": 0})).unwrap());
    s.execute("node.set", &json!({"id": t, "props": {"fontSize": 18, "textAlignHorizontal": "center"}})).unwrap();
    let p = properties(s.doc().unwrap(), t);
    assert_eq!(row(&p, "Typography", "Size").value, "18");
    assert!(p.iter().any(|s| s.title == "Typography"));
}
