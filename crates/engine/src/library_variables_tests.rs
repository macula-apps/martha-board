//! Library variables: publishing collections (hidden variables, versions), copies of library
//! variables (linked, read-only) and the layers bound to them following updates, components that
//! bind variables, and files that bring copies along. Using a library variable from a picker left
//! with the variables editor (#16): the fixtures copy them in as it did.

use figcraft_doc::*;
use serde_json::{Value, json};

use crate::Session;
use crate::library::*;
use crate::params::id_of;
use crate::test_variables as tv;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    match s.execute(cmd, &p) {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {p}: {e}"),
    }
}

fn idv(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn fill(s: &Session, id: NodeId) -> String {
    s.doc().unwrap().node(id).unwrap().fills.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()).unwrap_or_default()
}

fn healthy(s: &Session) {
    let doc = s.doc().unwrap();
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
}

/// A scratch directory for one test.
fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("figcraft-libvars-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Edit variable `name` (name or id) as a file can hold it.
fn edit_variable(s: &mut Session, name: &str, f: impl FnOnce(&mut Variable)) {
    crate::variable_data::edit(s, |doc| {
        let id = doc.variables.get(name).unwrap().id.clone();
        f(doc.variables.variables.iter_mut().find(|v| v.id == id).unwrap());
        Ok(())
    })
    .unwrap();
}

/// A library file's variables: "Theme" (Light/Dark) with `primary` (blue/white), `radius` (4),
/// `accent` aliasing `base` (hidden from publishing), and a private "_Scratch" collection.
fn theme(s: &mut Session) {
    tv::collection(s, "Theme", &["Light", "Dark"]);
    tv::variable(s, "Theme", "primary", "color", &[json!("#0000ff"), json!("#ffffff")]);
    tv::variable(s, "Theme", "radius", "number", &[json!(4)]);
    let base = tv::variable(s, "Theme", "base", "color", &[json!("#00ff00")]);
    edit_variable(s, "base", |v| v.hidden_from_publishing = true);
    tv::variable(s, "Theme", "accent", "color", &[json!("#000000")]);
    tv::set_all(s, "accent", VariableValue::Alias(base));
    tv::collection(s, "_Scratch", &["Mode 1"]);
    tv::variable(s, "_Scratch", "tmp", "number", &[json!(1)]);
}

/// Publish the active document and save it to `path`; returns the library id.
fn publish_to(s: &mut Session, path: &str) -> String {
    let r = ex(s, "library.publish", json!({}));
    ex(s, "file.saveAs", json!({"path": path}));
    r["id"].as_str().unwrap().to_string()
}

fn republish(lib: &mut Session) {
    ex(lib, "library.publish", json!({}));
    ex(lib, "file.save", json!({}));
}

/// The local copy of library variable `name`.
fn copy_of(s: &Session, name: &str) -> Variable {
    let doc = s.doc().unwrap();
    doc.variables.variables.iter().find(|v| v.name == name && is_remote_variable(doc, &v.id)).cloned().unwrap()
}

/// Bind `field` of `ids` to a copy of library `lid`'s variable `name` (copied in first) → the copy.
fn bind_library(s: &mut Session, lid: &str, name: &str, field: &str, ids: &[NodeId]) -> String {
    let local = tv::import_variable(s, lid, name);
    tv::bind(s, ids, field, &local);
    local
}

/// Show `frame` in `mode` of a copy of library `lid`'s collection `name` (copied in first).
fn library_mode(s: &mut Session, lid: &str, name: &str, mode: &str, frame: NodeId) {
    let local = tv::import_collection(s, lid, name);
    tv::mode(s, frame, &local, mode);
}

#[test]
fn publishing_collections_hides_and_versions_variables() {
    let mut s = Session::new();
    theme(&mut s);
    let r = ex(&mut s, "library.publish", json!({}));
    assert_eq!((r["collections"].as_u64(), r["variables"].as_u64()), (Some(1), Some(3)), "{r}");
    let lib = s.doc().unwrap().published.clone().unwrap();
    let m = &lib.manifest;
    let var = |name: &str| m.variables.iter().find(|v| v.name == name).cloned();
    assert!(m.collections.iter().all(|c| c.name != "_Scratch"), "names starting with _ stay private");
    assert!(var("primary").is_some_and(|v| v.published && v.version == 1 && v.kind == "color"));
    // A variable hidden from publishing stays out of the offer, but comes along for the alias.
    assert!(var("base").is_some_and(|v| !v.published));
    assert!(var("tmp").is_none());
    assert_eq!(lib.doc.variables.variables.len(), 4);
    // Saved and read back, the manifest and snapshot are the same.
    let bytes = figcraft_format::save_bytes(s.doc().unwrap(), None).unwrap();
    let read = figcraft_format::read_library(std::io::Cursor::new(&bytes)).unwrap().unwrap();
    assert_eq!(read.manifest, *m);
    assert_eq!(read.doc.variables, lib.doc.variables);

    // Nothing changed: refused. A change bumps only the variable it touches.
    assert!(s.execute("library.publish", &json!({})).unwrap_err().to_string().contains("no changes"));
    tv::set(&mut s, "radius", "Light", json!(8));
    assert_eq!(ex(&mut s, "library.publish", json!({}))["version"], 2);
    let m = s.doc().unwrap().published.clone().unwrap().manifest.clone();
    let version = |name: &str| m.variables.iter().find(|v| v.name == name).map(|v| v.version);
    assert_eq!((version("radius"), version("primary")), (Some(2), Some(1)));
    assert_eq!(m.collections.first().map(|c| c.version), Some(1), "the collection itself didn't change");
    // A new mode changes the collection.
    crate::variable_data::edit(&mut s, |doc| {
        let c = doc.variables.collections.iter_mut().find(|c| c.name == "Theme").unwrap();
        c.modes.push(Mode { id: format!("{}/dim", c.id), name: "Dim".into() });
        Ok(())
    })
    .unwrap();
    ex(&mut s, "library.publish", json!({}));
    let m = s.doc().unwrap().published.clone().unwrap().manifest.clone();
    assert_eq!(m.collections.first().map(|c| c.version), Some(3));
    // Hiding a published variable is a change; collections can be included and excluded by name.
    edit_variable(&mut s, "radius", |v| v.hidden_from_publishing = true);
    let r = ex(&mut s, "library.publish", json!({}));
    assert_eq!(r["variables"], 2);
    let r = ex(&mut s, "library.publish", json!({"include": ["_Scratch"]}));
    assert_eq!((r["collections"].as_u64(), r["variables"].as_u64()), (Some(1), Some(1)));
    ex(&mut s, "library.publish", json!({"include": ["Theme", "_Scratch"]}));
    let r = ex(&mut s, "library.publish", json!({"exclude": ["_Scratch"]}));
    assert_eq!(r["collections"], 1, "the default follows the last publish");
    let list = ex(&mut s, "library.list", json!({"assets": true}));
    assert_eq!(list["publishable"]["collections"].as_array().map(Vec::len), Some(2), "{list}");
    // Variables alone can be published; nothing at all can't.
    let mut empty = Session::new();
    assert!(empty.execute("library.publish", &json!({})).is_err());
}

/// `copies_follow_updates_and_survive_without_the_library`: saved and reopened, the file renders
/// without the library.
fn reopened_without_library(s: &mut Session, d: &std::path::Path, lib_path: &str, outer: NodeId) {
    // Saved and reopened, the copies, records and bindings stay; the file renders without the library.
    let user_path = path_str(&d.join("app.figcraft"));
    ex(s, "file.saveAs", json!({"path": user_path}));
    std::fs::remove_file(lib_path).unwrap();
    let mut t = Session::empty();
    ex(&mut t, "file.open", json!({"path": user_path}));
    assert_eq!(t.doc().unwrap().libraries, s.doc().unwrap().libraries);
    assert_eq!(fill(&t, outer), "FF0000");
}

/// `copies_follow_updates_and_survive_without_the_library`: the library changes its variables;
/// the copies, bindings and alias follow.
fn library_update_follows(s: &mut Session, lib: &mut Session, local: &str, [inner, outer]: [NodeId; 2]) {
    let modes = Default::default();
    // The library changes its variables and publishes again.
    tv::set(lib, "primary", "Light", json!("#ff0000"));
    tv::set(lib, "primary", "Dark", json!("#222222"));
    tv::set(lib, "base", "Light", json!("#00ffff"));
    crate::variable_data::edit(lib, |doc| {
        doc.variables.collections.iter_mut().find(|c| c.name == "Theme").unwrap().modes[1].name = "Night".into();
        Ok(())
    })
    .unwrap();
    republish(lib);
    let u = ex(s, "library.updates", json!({}));
    assert_eq!(u["count"], 3, "primary, base and the collection: {u}");
    assert_eq!(u["libraries"][0]["variables"].as_array().map(Vec::len), Some(2));
    let r = ex(s, "library.acceptUpdates", json!({}));
    assert_eq!((r["variables"].as_u64(), r["collections"].as_u64()), (Some(2), Some(1)), "{r}");
    // Same copies (same ids), new values: the bound layers and the alias follow.
    assert_eq!(copy_of(s, "primary").id, local);
    assert_eq!((fill(s, inner), fill(s, outer)), ("222222".into(), "FF0000".into()));
    assert!(
        matches!(s.doc().unwrap().variables.resolve(&s.doc().unwrap().variables.get("brand").unwrap().id, &modes), Some(VariableValue::Color(c)) if c.to_hex() == "00FFFF")
    );
    assert_eq!(s.doc().unwrap().variables.collections[0].modes[1].name, "Night");
    assert_eq!(ex(s, "library.updates", json!({}))["count"], 0);
    // Undo takes the update back.
    ex(s, "edit.undo", json!({}));
    assert_eq!(fill(s, outer), "0000FF");
    ex(s, "edit.redo", json!({}));
    healthy(s);
}

#[test]
fn copies_follow_updates_and_survive_without_the_library() {
    let d = dir("flow");
    let lib_path = path_str(&d.join("theme.figcraft"));
    let mut lib = Session::new();
    theme(&mut lib);
    let lid = publish_to(&mut lib, &lib_path);

    let mut s = Session::new();
    let added = ex(&mut s, "library.add", json!({"path": lib_path}));
    assert_eq!(added["variables"].as_array().map(Vec::len), Some(3), "{added}");
    assert_eq!(added["collections"][0]["modes"].as_array().map(Vec::len), Some(2));
    let frame = idv(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 200, "height": 200})));
    let inner = idv(&ex(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20, "parent": frame})));
    let outer = idv(&ex(&mut s, "shape.rectangle", json!({"x": 300, "y": 0, "width": 20, "height": 20})));

    // Layers bound to a copy (linked, read-only) take its value.
    let local = bind_library(&mut s, &lid, "primary", "fill", &[inner, outer]);
    assert_eq!((fill(&s, inner), fill(&s, outer)), ("0000FF".into(), "0000FF".into()));
    bind_library(&mut s, &lid, "radius", "cornerRadius", &[outer]);
    {
        let doc = s.doc().unwrap();
        assert!(is_remote_variable(doc, &local));
        assert_eq!(doc.variables.collections.len(), 1, "one copy of the collection for both");
        assert!(local_collections(doc).next().is_none(), "library copies aren't local collections");
        assert!(publishable(doc).collections.is_empty());
    }
    // Copying again reuses the copy.
    bind_library(&mut s, &lid, "primary", "fill", &[outer]);
    assert_eq!(s.doc().unwrap().variables.variables.len(), 2);

    // Explicit mode: the frame shows the library collection's Dark mode.
    library_mode(&mut s, &lid, "Theme", "Dark", frame);
    assert_eq!((fill(&s, inner), fill(&s, outer)), ("FFFFFF".into(), "0000FF".into()));

    // A local variable aliases a library one (copied in with the hidden variable it aliases).
    tv::collection(&mut s, "App", &["Mode 1"]);
    tv::variable(&mut s, "App", "brand", "color", &[json!("#123456")]);
    let accent = tv::import_variable(&mut s, &lid, "accent");
    tv::set_all(&mut s, "brand", VariableValue::Alias(accent));
    let modes = Default::default();
    assert!(
        matches!(s.doc().unwrap().variables.resolve(&s.doc().unwrap().variables.get("brand").unwrap().id, &modes), Some(VariableValue::Color(c)) if c.to_hex() == "00FF00")
    );
    healthy(&s);

    library_update_follows(&mut s, &mut lib, &local, [inner, outer]);

    reopened_without_library(&mut s, &d, &lib_path, outer);

    // Removing the library keeps the copies in use and drops the rest.
    tv::unbind(&mut s, outer, "cornerRadius");
    ex(&mut s, "library.remove", json!({"id": lid}));
    let doc = s.doc().unwrap();
    let names: Vec<&str> = doc.variables.variables.iter().filter(|v| is_remote_variable(doc, &v.id)).map(|v| v.name.as_str()).collect();
    assert_eq!(names.len(), 3, "primary (bound), accent and base (aliased): {names:?}");
    assert!(!names.contains(&"radius"));
    assert_eq!(doc.libraries.collections.len(), 1);
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}

/// A library's colour calculations come in reading this file's copies of their operands (the
/// library's ids mean nothing here), keep following the library's updates, and keep the copies they
/// read when the library is removed.
#[test]
fn library_colour_calculations_read_the_copies() {
    let d = dir("calc");
    let lib_path = path_str(&d.join("theme.figcraft"));
    let mut lib = Session::new();
    theme(&mut lib);
    let (base_id, primary_id) = {
        let vars = &lib.doc().unwrap().variables;
        (vars.get("base").unwrap().id.clone(), vars.get("primary").unwrap().id.clone())
    };
    tv::variable(&mut lib, "Theme", "hover", "color", &[json!("#000000")]);
    tv::set_all(&mut lib, "hover", VariableValue::ColorAlpha { variable: base_id, opacity: 0.5 });
    tv::variable(&mut lib, "Theme", "tint", "color", &[json!("#000000")]);
    let white = ColorOperand::Literal { literal: figcraft_color::Color::WHITE };
    let tint = VariableValue::ColorMix { from: ColorOperand::Variable { variable: primary_id }, to: white.clone(), t: 0.5 };
    tv::set_all(&mut lib, "tint", tint);
    let lid = publish_to(&mut lib, &lib_path);
    let lib_ids: Vec<String> = lib.doc().unwrap().variables.variables.iter().map(|v| v.id.clone()).collect();

    let mut s = Session::new();
    // Local variables first, so the copies get ids the library's don't share.
    tv::collection(&mut s, "App", &["Mode 1"]);
    for name in ["one", "two", "three", "four", "five", "six", "seven"] {
        tv::variable(&mut s, "App", name, "number", &[json!(1)]);
    }
    ex(&mut s, "library.add", json!({"path": lib_path}));
    let a = idv(&ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})));
    let b = idv(&ex(&mut s, "shape.rectangle", json!({"x": 20, "y": 0, "width": 10, "height": 10})));
    bind_library(&mut s, &lid, "hover", "fill", &[a]);
    bind_library(&mut s, &lid, "tint", "fill", &[b]);
    let shown = |s: &Session, id: NodeId| Some(crate::paint_tests::fill(s, id).to_css_hex());
    assert_eq!((shown(&s, a), shown(&s, b)), (Some("#00FF0080".into()), Some("#8080FF".into())));
    let (base, primary) = (copy_of(&s, "base").id, copy_of(&s, "primary").id);
    assert!(!lib_ids.contains(&base) && !lib_ids.contains(&primary), "copies have ids of their own");
    let reads = |name: &str| copy_of(&s, name).values.values().flat_map(|v| v.references().into_iter().cloned()).collect::<Vec<_>>();
    assert_eq!(reads("hover"), [base.clone(), base]);
    assert_eq!(reads("tint"), [primary.clone(), primary.clone()]);

    tv::set(&mut lib, "base", "Light", json!("#ff0000"));
    tv::set(&mut lib, "primary", "Light", json!("#000000"));
    republish(&mut lib);
    ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!((shown(&s, a), shown(&s, b)), (Some("#FF000080".into()), Some("#808080".into())));

    // A local calculation is the only reader of `primary` once the layers let go of the library.
    tv::variable(&mut s, "App", "wash", "color", &[json!("#000000")]);
    tv::set_all(&mut s, "wash", VariableValue::ColorMix { from: ColorOperand::Variable { variable: primary.clone() }, to: white, t: 0.5 });
    tv::unbind(&mut s, a, "fill");
    tv::unbind(&mut s, b, "fill");
    ex(&mut s, "library.remove", json!({"id": lid}));
    let doc = s.doc().unwrap();
    assert!(doc.variables.get_by_id(&primary).is_some(), "the copy a local calculation reads stays");
    let wash = doc.variables.get("wash").unwrap();
    let Some(VariableValue::Color(c)) = doc.variables.resolve(&wash.id, &Default::default()) else { panic!("wash resolves to a colour") };
    assert_eq!(c.to_rgba8(), [128, 128, 128, 255]);
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn library_components_keep_their_variable_bindings() {
    let d = dir("components");
    let lib_path = path_str(&d.join("kit.figcraft"));
    let mut lib = Session::new();
    theme(&mut lib);
    let f = idv(&ex(&mut lib, "frame.create", json!({"x": 0, "y": 0, "width": 100, "height": 40, "name": "Chip"})));
    tv::bind(&mut lib, &[f], "fill", "primary");
    tv::bind(&mut lib, &[f], "cornerRadius", "tmp");
    tv::mode(&mut lib, f, "Theme", "Dark");
    let chip = idv(&ex(&mut lib, "component.create", json!({"ids": [f]})));
    let lid = publish_to(&mut lib, &lib_path);
    // The private collection's variable comes along (not offered) for the component.
    let m = lib.doc().unwrap().published.clone().unwrap().manifest.clone();
    assert!(m.variables.iter().any(|v| v.name == "tmp" && !v.published));

    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": lib_path}));
    let inst = idv(&ex(&mut s, "library.place", json!({"library": lid, "component": "Chip", "x": 0, "y": 0})));
    assert_eq!(fill(&s, inst), "FFFFFF", "the component's own mode comes along");
    {
        let doc = s.doc().unwrap();
        let node = doc.node(inst).unwrap();
        let bound = node.bound.get("fill").unwrap();
        assert!(is_remote_variable(doc, bound), "bound to the copy, not to a variable of the file");
        assert!(node.var_modes.keys().all(|c| is_remote_collection(doc, c)));
        assert_eq!(doc.libraries.variables.len(), 2);
    }
    // A value of a mode the component doesn't show: only the variable changes.
    tv::set(&mut lib, "primary", "Light", json!("#444444"));
    republish(&mut lib);
    let version = |lib: &Session| lib.doc().unwrap().published.as_ref().and_then(|p| p.manifest.component(chip)).map(|a| a.version);
    assert_eq!(version(&lib), Some(1), "the component didn't change");
    assert_eq!(ex(&mut s, "library.updates", json!({}))["count"], 1);
    ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(fill(&s, inst), "FFFFFF");
    let light = copy_of(&s, "primary").values.values().next().cloned();
    assert!(matches!(light, Some(VariableValue::Color(c)) if c.to_hex() == "444444"));
    // The value it shows: the component (its resolved fill) and the variable update, the instance follows.
    tv::set(&mut lib, "primary", "Dark", json!("#333333"));
    republish(&mut lib);
    assert_eq!(version(&lib), Some(3));
    assert_eq!(ex(&mut s, "library.updates", json!({}))["count"], 2);
    ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(fill(&s, inst), "333333");
    healthy(&s);

    // Another file importing these pages brings the copies and their records along.
    let user_path = path_str(&d.join("user.figcraft"));
    ex(&mut s, "file.saveAs", json!({"path": user_path}));
    let mut t = Session::new();
    ex(&mut t, "file.import", json!({"path": user_path}));
    let doc = t.doc().unwrap();
    assert_eq!(doc.libraries.variables.len(), 2);
    assert_eq!(doc.libraries.collections.len(), 2);
    assert!(local_collections(doc).next().is_none());
    healthy(&t);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_file_bringing_older_copies_of_the_same_library_variables_shares_them() {
    let d = dir("older-copies");
    let lib_path = path_str(&d.join("kit.figcraft"));
    let mut lib = Session::new();
    theme(&mut lib);
    let lid = publish_to(&mut lib, &lib_path);
    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": lib_path}));
    let r = idv(&ex(&mut s, "shape.rectangle", json!({"width": 10, "height": 10, "name": "Swatch"})));
    bind_library(&mut s, &lid, "primary", "fill", &[r]);
    let old_path = path_str(&d.join("old.figcraft"));
    ex(&mut s, "file.saveAs", json!({"path": old_path}));
    // The copies take an update; the saved file keeps the older ones under the same ids.
    tv::set(&mut lib, "primary", "Light", json!("#444444"));
    republish(&mut lib);
    ex(&mut s, "library.acceptUpdates", json!({}));
    let before = s.doc().unwrap().clone();
    ex(&mut s, "file.import", json!({"path": old_path}));
    let doc = s.doc().unwrap();
    assert_eq!((&doc.variables, &doc.libraries.variables), (&before.variables, &before.libraries.variables), "shared by id, as before");
    let swatches: Vec<&Node> = doc.nodes().filter(|n| n.name == "Swatch").collect();
    assert_eq!(swatches.len(), 2);
    assert!(swatches.iter().all(|n| n.bound.get("fill") == Some(&copy_of(&s, "primary").id)));
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn library_style_bindings_ignore_unrelated_local_variables() {
    let d = dir("style-bound");
    let path = path_str(&d.join("lib.figcraft"));
    let mut lib = Session::new();
    tv::collection(&mut lib, "Tokens", &["Mode 1"]);
    let var = tv::variable(&mut lib, "Tokens", "Brand", "color", &[json!("#0000ff")]);
    let style = ex(&mut lib, "style.create", json!({"type": "paint", "name": "Brand", "value": "#0000ff"}))["id"].clone();
    tv::bind_style(&mut lib, style.as_str().unwrap(), "fill", &var);
    let lid = publish_to(&mut lib, &path);

    let mut s = Session::new();
    tv::collection(&mut s, "Local", &["Mode 1"]);
    let local = tv::variable(&mut s, "Local", "Unrelated", "color", &[json!("#ff0000")]);
    assert_eq!(local, var, "the ids must collide for this test");
    let r = idv(&ex(&mut s, "shape.rectangle", json!({"width": 40, "height": 40})));
    ex(&mut s, "library.add", json!({"path": path}));
    ex(&mut s, "library.applyStyle", json!({"library": lid, "style": "Brand", "id": r}));
    assert_eq!(fill(&s, r), "0000FF");
    healthy(&s);

    // An updated style follows the library's variable, not the local one.
    tv::set(&mut lib, "Brand", "Mode 1", json!("#00ffff"));
    republish(&mut lib);
    ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(fill(&s, r), "00FFFF");
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}

/// A library "Modal" holding a Button instance whose icon copy is on mode Dark: (the Button main,
/// the icon copy in Modal).
fn nested_mode_kit(lib: &mut Session) -> (NodeId, NodeId) {
    theme(lib);
    let f = idv(&ex(lib, "frame.create", json!({"x": 0, "y": 0, "width": 20, "height": 20, "name": "Icon"})));
    tv::bind(lib, &[f], "fill", "primary");
    let icon = idv(&ex(lib, "component.create", json!({"ids": [f]})));
    let b = idv(&ex(lib, "frame.create", json!({"x": 100, "y": 0, "width": 60, "height": 40, "name": "Button"})));
    ex(lib, "instance.create", json!({"component": icon, "x": 10, "y": 10, "parent": b}));
    let button = idv(&ex(lib, "component.create", json!({"ids": [b]})));
    let m = idv(&ex(lib, "frame.create", json!({"x": 300, "y": 0, "width": 200, "height": 100, "name": "Modal"})));
    let close = idv(&ex(lib, "instance.create", json!({"component": button, "x": 10, "y": 10, "parent": m})));
    let nested = lib.doc().unwrap().node(close).unwrap().children[0];
    tv::mode(lib, nested, "Theme", "Dark");
    ex(lib, "component.create", json!({"ids": [m]}));
    (button, nested)
}

/// The icon inside the Button inside library Modal instance `inst`.
fn modal_icon(s: &Session, inst: NodeId) -> NodeId {
    let doc = s.doc().unwrap();
    doc.node(doc.node(inst).unwrap().children[0]).unwrap().children[0]
}

#[test]
fn modes_set_inside_nested_instances_come_along() {
    let d = dir("nested-modes");
    let lib_path = path_str(&d.join("kit.figcraft"));
    let mut lib = Session::new();
    let (button, _) = nested_mode_kit(&mut lib);
    let lid = publish_to(&mut lib, &lib_path);

    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": lib_path}));
    let inst = idv(&ex(&mut s, "library.place", json!({"library": lid, "component": "Modal", "x": 0, "y": 0})));
    let copy = |s: &Session| modal_icon(s, inst);
    assert_eq!(fill(&s, copy(&s)), "FFFFFF", "the Dark mode set on the nested icon comes along");
    // An update that rebuilds the Button copies keeps it too.
    ex(&mut lib, "node.set", json!({"ids": [button], "layout": "horizontal"}));
    republish(&mut lib);
    ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(fill(&s, copy(&s)), "FFFFFF");
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_library_published_before_modes_were_overrides_keeps_the_modes_set_inside_nested_instances() {
    let d = dir("nested-modes-old");
    let lib_path = path_str(&d.join("kit.figcraft"));
    let mut lib = Session::new();
    let (_, nested) = nested_mode_kit(&mut lib);
    // Published before `modes` was an override group: the copy has its mode but no marker.
    lib.doc_mut().unwrap().node_mut(nested).unwrap().overrides.remove("modes");
    let lid = publish_to(&mut lib, &lib_path);

    let mut s = Session::new();
    ex(&mut s, "library.add", json!({"path": lib_path}));
    let inst = idv(&ex(&mut s, "library.place", json!({"library": lid, "component": "Modal", "x": 0, "y": 0})));
    assert_eq!(fill(&s, modal_icon(&s, inst)), "FFFFFF", "the Dark mode comes along");
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn stored_variable_ids_keep_library_alias_dependencies() {
    let mut s = Session::new();
    for name in ["Shadow", "Base", "Theme"] {
        tv::collection(&mut s, name, &["Mode 1"]);
    }
    let shadow = tv::variable(&mut s, "Shadow", "shadow", "color", &[json!("#FF0000")]);
    let surface = tv::variable(&mut s, "Base", "surface", "color", &[json!("#0D99FF")]);
    let alias = tv::variable(&mut s, "Theme", "alias", "color", &[json!("#000000")]);
    tv::set_all(&mut s, &alias, VariableValue::Alias(surface.clone()));
    let f = idv(&ex(&mut s, "frame.create", json!({"name": "Chip", "x": 0, "y": 0, "width": 100, "height": 40})));
    tv::bind(&mut s, &[f], "fill", &alias);
    let chip = idv(&ex(&mut s, "component.create", json!({"ids": [f]})));
    // A variable named like another's id: publishing and importing go by ids.
    edit_variable(&mut s, &shadow, |v| v.name.clone_from(&surface));
    let doc = s.doc().unwrap();
    let selection = Selection { components: vec![chip], ..Default::default() };
    let lib = publish(doc, None, &selection, None, 0).unwrap();
    let target = surface.as_str();
    assert!(lib.doc.variables.get_by_id(target).is_some());
    assert!(lib.doc.variables.get_by_id(&shadow).is_none());
    assert!(lib.doc.variables.collections.iter().any(|c| c.name == "Base"));

    // Import from a snapshot that retains both the dependency and its namesake.
    let lib = publish(doc, None, &publishable(doc), None, 1).unwrap();
    let mut dest = Document::new("consumer");
    let key = alias.clone();
    let map = import_variables(&mut dest, &lib, &std::collections::BTreeSet::from([key.clone()]), &Default::default(), false);
    assert!(map.variables.contains_key(target));
    let copy = dest.variables.get_by_id(&map.variables[&key]).unwrap();
    assert!(!copy.values.is_empty());
    assert!(copy.values.values().all(|v| *v == VariableValue::Alias(map.variables[target].clone())));
    assert!(dest.variables.collections.iter().any(|c| c.name == "Base"));
}

/// A library with collection `coll` (Main/Alt), a colour variable `var` and a paint style `var`
/// bound to it, published to `path`; returns (the library session, its id).
fn styled_library(path: &str, coll: &str, var: &str, main: &str, alt: &str) -> (Session, String) {
    let mut lib = Session::new();
    tv::collection(&mut lib, coll, &["Main", "Alt"]);
    let v = tv::variable(&mut lib, coll, var, "color", &[json!(main), json!(alt)]);
    let style = ex(&mut lib, "style.create", json!({"type": "paint", "name": var, "value": main}))["id"].clone();
    tv::bind_style(&mut lib, style.as_str().unwrap(), "fill", &v);
    let lid = publish_to(&mut lib, path);
    (lib, lid)
}

#[test]
fn accepting_updates_keeps_variables_only_library_styles_use() {
    let d = dir("accept-style-vars");
    let (pa, pb) = (path_str(&d.join("a.figcraft")), path_str(&d.join("b.figcraft")));
    let (mut a, la) = styled_library(&pa, "TokensA", "accent", "#1565c0", "#2e7d32");
    let (_b, lb) = styled_library(&pb, "TokensB", "primary", "#7b1fa2", "#00838f");

    let mut s = Session::new();
    let frame = idv(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 200, "height": 200})));
    let ra = idv(&ex(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20, "parent": frame})));
    let rb = idv(&ex(&mut s, "shape.rectangle", json!({"x": 50, "y": 10, "width": 20, "height": 20, "parent": frame})));
    ex(&mut s, "library.add", json!({"path": pa}));
    ex(&mut s, "library.add", json!({"path": pb}));
    ex(&mut s, "library.applyStyle", json!({"library": la, "style": "accent", "id": ra}));
    ex(&mut s, "library.applyStyle", json!({"library": lb, "style": "primary", "id": rb}));
    library_mode(&mut s, &la, "TokensA", "Alt", frame);
    library_mode(&mut s, &lb, "TokensB", "Alt", frame);
    assert_eq!(fill(&s, ra), "2E7D32");
    assert_eq!(fill(&s, rb), "00838F");
    healthy(&s);

    tv::set(&mut a, "accent", "Alt", json!("#ef6c00"));
    republish(&mut a);
    let r = ex(&mut s, "library.acceptUpdates", json!({}));
    assert_eq!(r["variables"], 1, "{r}");

    let doc = s.doc().unwrap();
    for name in ["accent", "primary"] {
        let v = doc.variables.variables.iter().find(|v| v.name == name && is_remote_variable(doc, &v.id));
        assert!(v.is_some(), "library variable copy `{name}` was deleted");
        let id = &v.unwrap().id;
        assert!(doc.libraries.variables.iter().any(|r| &r.local == id), "record of `{name}` was deleted");
    }
    assert_eq!(fill(&s, ra), "EF6C00");
    assert_eq!(fill(&s, rb), "00838F");
    healthy(&s);
    let _ = std::fs::remove_dir_all(&d);
}
