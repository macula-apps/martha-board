//! Extended collections a file holds: variables a library adds to a collection this file extends
//! are an update of their own, and a library collection that is itself an extended collection can
//! be extended again, values flowing down the chain as each file accepts updates. A collection of
//! this file that extends another follows its edits at once (and chains of them). Extending a
//! collection left with the variables editor (#16): the fixtures write the extensions.

use figcraft_doc::*;
use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
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
    let d = std::env::temp_dir().join(format!("figcraft-extchain-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A library file with "Theme" (Light/Dark: `primary`, `radius`) and "Other" (`size`), published
/// and saved at `path`; returns (session, library id).
fn core(path: &str) -> (Session, String) {
    let mut lib = Session::new();
    tv::collection(&mut lib, "Theme", &["Light", "Dark"]);
    tv::variable(&mut lib, "Theme", "primary", "color", &[json!("#0000ff"), json!("#ffffff")]);
    tv::variable(&mut lib, "Theme", "radius", "number", &[json!(4)]);
    tv::collection(&mut lib, "Other", &["Mode 1"]);
    tv::variable(&mut lib, "Other", "size", "number", &[json!(1)]);
    let id = ex(&mut lib, "library.publish", json!({}))["id"].as_str().unwrap().to_string();
    ex(&mut lib, "file.saveAs", json!({"path": path}));
    (lib, id)
}

fn republish(lib: &mut Session) {
    ex(lib, "library.publish", json!({}));
    ex(lib, "file.save", json!({}));
}

/// The library collection `name` of `library`, copied in and extended as `extension` → the
/// extension's id (what extending a library collection made).
fn extend_library(s: &mut Session, library: &str, name: &str, extension: &str) -> String {
    let copy = tv::import_collection(s, library, name);
    tv::extend(s, &copy, extension)
}

#[test]
fn variables_new_to_an_extended_collection_are_an_update_of_their_own() {
    let d = dir("new");
    let path = d.join("core.figcraft").to_string_lossy().into_owned();
    let (mut lib, lid) = core(&path);
    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": path}));
    let ext = extend_library(&mut s, &lid, "Theme", "Theme (extended)");
    assert_eq!(ex(&mut s, "library.updates", json!({}))["count"], 0);

    // The library only adds a variable to the extended collection (and one to a collection this
    // file doesn't use): that alone is an update, listed by name.
    tv::variable(&mut lib, "Theme", "gap", "number", &[json!(8)]);
    tv::variable(&mut lib, "Other", "unused", "number", &[json!(2)]);
    republish(&mut lib);
    let u = ex(&mut s, "library.updates", json!({}));
    assert_eq!(u["count"], 1, "{u}");
    let row = &u["libraries"][0]["variables"][0];
    assert_eq!((&row["name"], &row["id"]), (&json!("gap"), &Value::Null), "no copy yet: no id");
    // Accepting brings it in, inheriting; then nothing is pending.
    let done = ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(done["variables"], 1);
    let gap = var(&s, &ext, "gap");
    assert!(gap.extends.is_some() && gap.overrides.is_empty());
    assert_eq!(value(&s, &ext, "gap", "Dark"), "8");
    assert_eq!(ex(&mut s, "library.updates", json!({}))["count"], 0);
    // Docs silent: the variable added to a collection nobody here uses stays out.
    assert!(!s.doc().unwrap().variables.variables.iter().any(|v| v.name == "unused"));
    healthy(&s);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn an_extended_collection_can_be_extended_again() {
    let d = dir("chain");
    let core_path = d.join("core.figcraft").to_string_lossy().into_owned();
    let brand_path = d.join("brand.figcraft").to_string_lossy().into_owned();
    let (mut core_lib, core_id) = core(&core_path);

    // A brand library extends the core theme, overrides a value, adds a variable and publishes.
    let mut brand = Session::new();
    ex(&mut brand, "library.add", json!({"path": core_path}));
    let bext = extend_library(&mut brand, &core_id, "Theme", "Brand");
    let bprimary = var(&brand, &bext, "primary").id.clone();
    tv::set(&mut brand, &bprimary, "Light", json!("#ff0000"));
    tv::variable(&mut brand, &bext, "logo", "number", &[json!(3)]);
    let brand_id = ex(&mut brand, "library.publish", json!({}))["id"].as_str().unwrap().to_string();
    ex(&mut brand, "file.saveAs", json!({"path": brand_path}));

    // A product file extends the brand's extended collection: the brand's values, modes and
    // variables come along, and it overrides one of its own.
    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": brand_path}));
    let pext = extend_library(&mut s, &brand_id, "Brand", "Brand (extended)");
    let modes: Vec<String> = s.doc().unwrap().variables.collection(&pext).unwrap().modes.iter().map(|m| m.name.clone()).collect();
    assert_eq!(modes, ["Light", "Dark"]);
    assert_eq!(value(&s, &pext, "primary", "Light"), "FF0000", "the brand's override is inherited");
    assert_eq!(value(&s, &pext, "primary", "Dark"), "FFFFFF", "the core value is inherited");
    assert_eq!(value(&s, &pext, "logo", "Light"), "3", "the brand's own variable is inherited");
    let radius = var(&s, &pext, "radius").id.clone();
    tv::set(&mut s, &radius, "Dark", json!(12));
    let r = id_of(&ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10}))["id"]).unwrap();
    let primary = var(&s, &pext, "primary").id.clone();
    tv::bind(&mut s, &[r], "fill", &primary);

    // The core changes: the brand accepts and republishes, then the product accepts. Values
    // follow down the chain; each level's overrides stay.
    tv::set(&mut core_lib, "primary", "Dark", json!("#222222"));
    tv::set(&mut core_lib, "radius", "Light", json!(6));
    republish(&mut core_lib);
    ex(&mut brand, "library.acceptUpdates", json!({}));
    assert_eq!(value(&brand, &bext, "primary", "Dark"), "222222");
    assert_eq!(value(&brand, &bext, "primary", "Light"), "FF0000");
    republish(&mut brand);
    assert!(ex(&mut s, "library.updates", json!({}))["count"].as_u64().unwrap() > 0);
    ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(value(&s, &pext, "primary", "Dark"), "222222");
    assert_eq!(value(&s, &pext, "primary", "Light"), "FF0000");
    assert_eq!(value(&s, &pext, "radius", "Light"), "6");
    assert_eq!(value(&s, &pext, "radius", "Dark"), "12", "the product's override stays");
    healthy(&s);

    // Saved and reopened, the chain's end is the same.
    let user = d.join("product.figcraft").to_string_lossy().into_owned();
    ex(&mut s, "file.saveAs", json!({"path": user}));
    let mut t = Session::empty();
    ex(&mut t, "file.open", json!({"path": user}));
    assert_eq!(t.doc().unwrap().variables, s.doc().unwrap().variables);
    std::fs::remove_dir_all(&d).ok();
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
    crate::variable_data::edit(&mut s, |doc| {
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
    crate::variable_data::edit(&mut s, |doc| {
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
