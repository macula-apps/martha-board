//! Collections of this file that extend another collection of this file: an extension follows its
//! parent's edits at once (and chains of them), and a loop of extensions comes apart into plain
//! collections. Extending a collection left with the variables editor (#16): the fixtures write the
//! extensions. (The library cases left with libraries, #28.)

use figcraft_doc::*;
use serde_json::{Value, json};

use crate::Session;
use crate::test_variables as tv;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn var<'a>(s: &'a Session, collection: &str, name: &str) -> &'a Variable {
    let doc = s.doc().unwrap();
    doc.variables.variables.iter().find(|v| v.collection == collection && v.name == name).unwrap()
}

/// `name`'s value in mode `mode` (by name) of `collection`, as text.
fn value(s: &Session, collection: &str, name: &str, mode: &str) -> String {
    let doc = s.doc().unwrap();
    let m = doc.variables.collection(collection).and_then(|c| c.find_mode(mode)).unwrap().id.clone();
    match var(s, collection, name).values.get(&m) {
        Some(VariableValue::Color(c)) => c.to_hex(),
        Some(VariableValue::Number(n)) => n.to_string(),
        other => format!("{other:?}"),
    }
}

fn healthy(s: &Session) {
    let doc = s.doc().unwrap();
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("figcraft-extcoll-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// "Theme" (Light/Dark: `primary`, `radius`) in this file, extended; returns the extension's id.
fn local_theme(s: &mut Session) -> String {
    let theme = tv::collection(s, "Theme", &["Light", "Dark"]);
    tv::variable(s, "Theme", "primary", "color", &[json!("#0000ff"), json!("#ffffff")]);
    tv::variable(s, "Theme", "radius", "number", &[json!(4)]);
    tv::extend(s, &theme, "Theme (extended)")
}

fn theme_id(s: &Session) -> String {
    s.doc().unwrap().variables.collection("Theme").unwrap().id.clone()
}

#[test]
fn a_collection_of_this_file_extends_and_follows_its_edits() {
    let mut s = Session::new();
    let ext = local_theme(&mut s);
    let theme = theme_id(&s);
    let doc = s.doc().unwrap();
    let c = doc.variables.collection_by_id(&ext).unwrap();
    assert_eq!(c.extends.as_deref(), Some(theme.as_str()));
    assert_eq!(c.modes, doc.variables.collection_by_id(&theme).unwrap().modes, "the same modes, ids included");
    assert_eq!(value(&s, &ext, "primary", "Dark"), "FFFFFF");
    assert!(var(&s, &ext, "radius").extends.is_some());

    // Overridden in Light; then the parent changes both modes: Dark follows, Light stays.
    let radius = var(&s, &ext, "radius").id.clone();
    tv::set(&mut s, &radius, "Light", json!(8));
    let parent_radius = var(&s, &theme, "radius").id.clone();
    tv::set(&mut s, &parent_radius, "Light", json!(6));
    tv::set(&mut s, &parent_radius, "Dark", json!(10));
    assert_eq!((value(&s, &ext, "radius", "Light"), value(&s, &ext, "radius", "Dark")), ("8".into(), "10".into()));
    assert_eq!(value(&s, &theme, "radius", "Light"), "6", "the parent keeps its own value");

    // New variables and modes, renames and mode moves in the parent arrive at once.
    tv::variable(&mut s, &theme, "gap", "number", &[json!(2)]);
    crate::test_variables::edit(&mut s, |doc| {
        let c = doc.variables.collections.iter_mut().find(|c| c.id == theme).unwrap();
        c.modes.push(figcraft_doc::Mode { id: format!("{theme}/contrast"), name: "Contrast".into() });
        c.modes.swap(0, 1);
        let v = doc.variables.variables.iter_mut().find(|v| v.id == parent_radius).unwrap();
        v.name = "corner".into();
        v.values.insert(format!("{theme}/contrast"), VariableValue::Number(6.0));
        let gap = doc.variables.variables.iter_mut().find(|v| v.name == "gap" && v.collection == theme).unwrap();
        gap.values.insert(format!("{theme}/contrast"), VariableValue::Number(2.0));
        Ok(())
    })
    .unwrap();
    let doc = s.doc().unwrap();
    assert_eq!(doc.variables.collection_by_id(&ext).unwrap().modes, doc.variables.collection_by_id(&theme).unwrap().modes);
    assert_eq!(value(&s, &ext, "gap", "Contrast"), "2");
    assert_eq!(var(&s, &ext, "corner").id, radius, "renamed, the same variable");
    healthy(&s);

    // Saved and reopened, still extending.
    let d = dir("local");
    let path = d.join("local.figcraft").to_string_lossy().into_owned();
    ex(&mut s, "file.saveAs", json!({"path": path}));
    let mut t = Session::empty();
    ex(&mut t, "file.open", json!({"path": path}));
    assert_eq!(t.doc().unwrap().variables, s.doc().unwrap().variables);
    std::fs::remove_dir_all(&d).ok();

    // The parent gone, the extension becomes a plain collection keeping its values.
    crate::test_variables::edit(&mut s, |doc| {
        doc.variables.collections.retain(|c| c.id != theme);
        doc.variables.variables.retain(|v| v.collection != theme);
        Ok(())
    })
    .unwrap();
    let doc = s.doc().unwrap();
    assert!(doc.variables.collection_by_id(&ext).unwrap().extends.is_none());
    assert!(doc.variables.variables.iter().filter(|v| v.collection == ext).all(|v| v.extends.is_none() && v.overrides.is_empty()));
    assert_eq!(value(&s, &ext, "corner", "Dark"), "10");
    healthy(&s);
}

#[test]
fn an_extended_collection_of_this_file_extends_again() {
    let mut s = Session::new();
    let brand = local_theme(&mut s);
    let theme = theme_id(&s);
    let primary = var(&s, &brand, "primary").id.clone();
    tv::set(&mut s, &primary, "Light", json!("#ff0000"));
    tv::variable(&mut s, &brand, "logo", "number", &[json!(3)]);
    let product = tv::extend(&mut s, &brand, "Brand (extended)");
    assert_eq!(value(&s, &product, "primary", "Light"), "FF0000", "the brand's override is inherited");
    assert!(var(&s, &product, "primary").overrides.is_empty(), "but isn't the product's own override");
    assert_eq!(value(&s, &product, "logo", "Dark"), "3", "the brand's own variable is inherited");
    // The theme changes: values flow down through the brand where nobody overrode them.
    let core = var(&s, &theme, "primary").id.clone();
    tv::set(&mut s, &core, "Dark", json!("#222222"));
    tv::set(&mut s, &core, "Light", json!("#00ff00"));
    assert_eq!(value(&s, &product, "primary", "Dark"), "222222");
    assert_eq!(value(&s, &product, "primary", "Light"), "FF0000");
    healthy(&s);
}

#[test]
fn a_loop_of_extensions_comes_apart_into_plain_collections() {
    let mut s = Session::new();
    let ext = local_theme(&mut s);
    let theme = theme_id(&s);
    // A hostile file: the theme extends its own extension.
    let doc = s.doc_mut().unwrap();
    doc.variables.collections.iter_mut().find(|c| c.id == theme).unwrap().extends = Some(ext.clone());
    crate::variable_data::sync_extensions(doc);
    assert!(doc.variables.collections.iter().all(|c| c.extends.is_none()));
    assert!(doc.variables.variables.iter().all(|v| v.extends.is_none()));
    assert_eq!(value(&s, &ext, "primary", "Light"), "0000FF", "values kept");
}
