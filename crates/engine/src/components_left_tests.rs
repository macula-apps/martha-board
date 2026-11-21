//! Components, instances and libraries left the board (#28): no command for them is registered,
//! listed in a menu or bound to a key, and a file that holds components and instances still opens,
//! draws and saves without losing a layer, a file whose components hold each other's instances
//! included. The files are built here from `figcraft_doc` types, as such a file holds them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, ExportFormat, NodeId, NodeKind, TextData};
use serde_json::json;

use crate::Session;
use crate::test_helpers::ex;

/// Command ids that went with components, instances and libraries besides the `component.`,
/// `instance.`, `library.` and `libraries.` ones: the component playground, filling copies of a
/// frame with data, and the Assets and Libraries panels.
const OTHER_REMOVED: &[&str] = &["data.fill", "view.assets", "libraries.open", "panel.libraries"];

fn removed(id: &str) -> bool {
    ["component.", "instance.", "library.", "libraries."].iter().any(|p| id.starts_with(p)) || OTHER_REMOVED.contains(&id)
}

#[test]
fn no_component_instance_or_library_command_is_registered_listed_or_bound() {
    let ids: Vec<&str> = crate::all_commands().iter().map(|c| c.id).collect();
    assert!(ids.len() > 300, "the registry is read: {} commands", ids.len());
    let registered: Vec<&&str> = ids.iter().filter(|id| removed(id)).collect();
    assert!(registered.is_empty(), "registered: {registered:?}");
    let menus: Vec<&str> = crate::catalog::MENU.iter().map(|m| m.id).filter(|id| removed(id)).collect();
    assert!(menus.is_empty(), "in the menus: {menus:?}");
    let keys: Vec<&str> = crate::keymap::KEYS.iter().map(|k| k.id).filter(|id| removed(id)).collect();
    assert!(keys.is_empty(), "bound to keys: {keys:?}");
    // The check itself sees such an id.
    assert!(removed("component.create") && removed("instance.swap") && removed("library.publish") && removed("data.fill"));
    assert!(!removed("board.sticky") && !removed("node.create"));
}

/// A folder of the test's own under the temporary folder, removed when it goes out of scope.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("figcraft-components-left-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Self(d)
    }

    fn file(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn component() -> NodeKind {
    NodeKind::Component { description: String::new(), props: BTreeMap::new(), prop_order: Vec::new() }
}

/// An instance of `main` at `(x, y)` in `parent`: a copy of the main's layers, each pointing at the
/// layer it copies, as an instance holds them.
fn instance_of(doc: &mut Document, main: NodeId, parent: NodeId, x: f64, y: f64) -> NodeId {
    let id = doc.duplicate_into(main, parent, None).unwrap();
    let pairs: Vec<(NodeId, NodeId)> = doc.subtree(main).into_iter().zip(doc.subtree(id)).collect();
    for (from, to) in pairs.into_iter().skip(1) {
        doc.get_mut(to).unwrap().source = Some(from);
    }
    let n = doc.get_mut(id).unwrap();
    n.kind = NodeKind::Instance { component: main, props: BTreeMap::new() };
    n.transform = figcraft_geom::Affine::translate((x, y));
    id
}

/// A file with a main component (a background and a title) and two instances of it, the second
/// with its background's fill and its title overridden: (document, main, plain instance,
/// overridden instance).
fn card_file() -> (Document, NodeId, NodeId, NodeId) {
    let mut doc = Document::new("Cards");
    let page = doc.first_page().unwrap();
    let main = doc.create(page, component(), "Card", 0.0, 0.0, 160.0, 80.0).unwrap();
    let bg = doc.create(main, NodeKind::Rectangle, "Background", 0.0, 0.0, 160.0, 80.0).unwrap();
    doc.get_mut(bg).unwrap().fills = vec![Paint::solid(Color::from_rgb8_const(0x1F, 0x6F, 0x5C))];
    doc.create(main, NodeKind::Text(TextData::new("Hello")), "Title", 12.0, 12.0, 120.0, 20.0).unwrap();
    let plain = instance_of(&mut doc, main, page, 200.0, 0.0);
    let overridden = instance_of(&mut doc, main, page, 400.0, 0.0);
    let bg = doc.node(overridden).unwrap().children[0];
    let n = doc.get_mut(bg).unwrap();
    n.fills = vec![Paint::solid(Color::from_rgb8_const(0xF2, 0xA5, 0x41))];
    n.overrides.insert("fills".into());
    let title = doc.node(overridden).unwrap().children[1];
    let n = doc.get_mut(title).unwrap();
    if let NodeKind::Text(t) = &mut n.kind {
        t.characters = "Overridden".into();
    }
    n.overrides.insert("characters".into());
    (doc, main, plain, overridden)
}

fn layers(doc: &Document) -> usize {
    doc.nodes().filter(|n| !n.is_page()).count()
}

fn png(s: &Session, id: NodeId) -> Vec<u8> {
    crate::cmd::file::export_bytes(s.doc().unwrap(), &[id], ExportFormat::Png, 1.0).unwrap()
}

fn opened(path: &str) -> Session {
    let mut s = Session::empty();
    ex(&mut s, "file.open", json!({"path": path}));
    s
}

fn write(path: &str, doc: &Document) {
    std::fs::write(Path::new(path), figcraft_format::save_bytes(doc, None).unwrap()).unwrap();
}

#[test]
fn a_file_with_components_and_instances_opens_draws_and_saves_without_losing_a_layer() {
    let dir = Dir::new("cards");
    let path = dir.file("cards.figcraft");
    let (doc, main, plain, overridden) = card_file();
    assert_eq!(doc.validate(), Vec::<String>::new());
    let count = layers(&doc);
    assert_eq!(count, 9, "a main of three layers and two instances of three");
    write(&path, &doc);

    let mut s = opened(&path);
    assert_eq!(layers(s.doc().unwrap()), count, "opening keeps every layer");
    let bytes = png(&s, overridden);
    assert!(bytes.starts_with(b"\x89PNG"), "the instance draws");
    assert_ne!(bytes, png(&s, plain), "the override shows: the instances differ");
    ex(&mut s, "file.save", json!({}));

    let again = opened(&path);
    let doc = again.doc().unwrap();
    assert_eq!(layers(doc), count, "saving again keeps every layer");
    assert_eq!(png(&again, overridden), bytes, "the instance draws the same after saving again");
    assert!(matches!(doc.node(overridden).unwrap().kind, NodeKind::Instance { component, .. } if component == main), "still an instance of its main");
    let title = doc.node(overridden).unwrap().children[1];
    assert_eq!(doc.node(title).unwrap().kind.text().map(|t| t.characters.as_str()), Some("Overridden"), "the override is kept");
}

#[test]
fn a_file_whose_components_hold_each_others_instances_opens() {
    let dir = Dir::new("cycle");
    let path = dir.file("cycle.figcraft");
    let mut doc = Document::new("Cycle");
    let page = doc.first_page().unwrap();
    let a = doc.create(page, component(), "A", 0.0, 0.0, 200.0, 200.0).unwrap();
    doc.create(a, NodeKind::Rectangle, "A body", 0.0, 0.0, 40.0, 40.0).unwrap();
    let b = doc.create(page, component(), "B", 300.0, 0.0, 200.0, 200.0).unwrap();
    doc.create(b, NodeKind::Rectangle, "B body", 0.0, 0.0, 40.0, 40.0).unwrap();
    // A holds an instance of B, and B one of A.
    let b_in_a = instance_of(&mut doc, b, a, 50.0, 50.0);
    // A's copy inside B holds a copy of A's instance of B: the loop closes, the tree stays finite.
    instance_of(&mut doc, a, b, 50.0, 50.0);
    let count = layers(&doc);
    write(&path, &doc);
    let (tx, rx) = std::sync::mpsc::channel();
    let p = path.clone();
    std::thread::spawn(move || {
        let s = opened(&p);
        let out = (layers(s.doc().unwrap()), png(&s, a).starts_with(b"\x89PNG"), png(&s, b_in_a).starts_with(b"\x89PNG"));
        let _ = tx.send(out);
    });
    let got = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the file opens and draws without hanging or panicking");
    assert_eq!(got, (count, true, true), "every layer is there and both draw");
}

/// A caller still naming a library for a colour style is refused by name, not given this file's
/// style of the same name.
#[test]
fn a_colour_style_from_a_library_is_refused_by_name() {
    let mut s = Session::new();
    ex(&mut s, "style.create", json!({"type": "paint", "name": "Brand", "value": "#0000ff"}));
    let e = s.execute("selection.applyColorStyle", &json!({"targets": [], "style": "Brand", "library": "kit"})).unwrap_err().to_string();
    assert!(e.contains("unknown parameter `library`"), "{e}");
}

/// The layers a paste answered with, with everything they hold.
fn pasted(s: &Session, answer: &serde_json::Value) -> Vec<NodeId> {
    let doc = s.doc().unwrap();
    let roots: Vec<NodeId> = serde_json::from_value(answer["ids"].clone()).unwrap();
    roots.into_iter().flat_map(|r| doc.subtree(r)).collect()
}

/// The main component and the main layer pasted layer `id` names.
fn mains(s: &Session, id: NodeId) -> (Option<NodeId>, Option<NodeId>) {
    let n = s.doc().unwrap().node(id).unwrap();
    let component = match n.kind {
        NodeKind::Instance { component, .. } => Some(component),
        _ => None,
    };
    (component, n.source)
}

/// A paste from another file never leaves an instance or an instance's layer naming an unrelated
/// layer of this file: a main left behind is named by an id no layer here has, and a main pasted
/// with it is named by its pasted copy, even when the two were copied as separate layers.
#[test]
fn a_paste_from_another_file_names_a_pasted_main_or_none_of_this_files_layers() {
    let mut s = Session::new();
    let page = s.doc().unwrap().first_page().unwrap();
    let doc = s.doc_mut().unwrap();
    let main = doc.create(page, component(), "Card", 0.0, 0.0, 160.0, 80.0).unwrap();
    doc.create(main, NodeKind::Rectangle, "Background", 0.0, 0.0, 160.0, 80.0).unwrap();
    let instance = instance_of(doc, main, page, 200.0, 0.0);

    // The instance alone: its main stays behind in the first file.
    ex(&mut s, "edit.copy", json!({"ids": [instance]}));
    ex(&mut s, "file.new", json!({}));
    let answer = ex(&mut s, "edit.paste", json!({}));
    let ids = pasted(&s, &answer);
    assert_eq!(ids.len(), 2, "the instance and its layer");
    for id in &ids {
        let (component, source) = mains(&s, *id);
        for named in component.into_iter().chain(source) {
            assert!(s.doc().unwrap().node(named).is_none(), "pasted layer {id:?} names {named:?}, a layer of this file");
        }
    }

    // The main and the instance, copied as two layers: the copy follows the pasted main.
    ex(&mut s, "file.new", json!({}));
    let first = s.docs.iter().position(|d| d.doc.node(main).is_some_and(|n| n.name == "Card")).unwrap();
    s.active = first;
    ex(&mut s, "edit.copy", json!({"ids": [main, instance]}));
    s.active = s.docs.len() - 1;
    let answer = ex(&mut s, "edit.paste", json!({}));
    let ids = pasted(&s, &answer);
    let doc = s.doc().unwrap();
    let kind = |id: &NodeId| doc.node(*id).map(|n| n.kind.clone());
    let new_main = *ids.iter().find(|id| matches!(kind(id), Some(NodeKind::Component { .. }))).unwrap();
    let new_instance = *ids.iter().find(|id| matches!(kind(id), Some(NodeKind::Instance { .. }))).unwrap();
    assert_eq!(mains(&s, new_instance).0, Some(new_main), "the pasted instance names the pasted main");
    let main_layers = doc.node(new_main).unwrap().children.clone();
    let instance_layers = doc.node(new_instance).unwrap().children.clone();
    assert_eq!(main_layers.len(), instance_layers.len());
    for (m, i) in main_layers.iter().zip(&instance_layers) {
        assert_eq!(mains(&s, *i).1, Some(*m), "the pasted instance's layer names the pasted main's layer");
    }
}

/// martha-board#46: `text.set` records a characters override only inside an instance (as the
/// window's text editor does); a plain text layer, such as a sticky's note, gets none.
#[test]
fn text_set_overrides_characters_only_inside_an_instance() {
    let dir = Dir::new("text-set-overrides");
    let path = dir.file("cards.figcraft");
    let (doc, main, plain, _) = card_file();
    write(&path, &doc);
    let mut s = opened(&path);
    let in_main = s.doc().unwrap().node(main).unwrap().children[1];
    let in_instance = s.doc().unwrap().node(plain).unwrap().children[1];
    for (id, what) in [(in_main, "the main component's own title"), (in_instance, "an instance's title")] {
        ex(&mut s, "text.set", json!({"id": id.0, "characters": "Typed"}));
        let n = s.doc().unwrap().node(id).unwrap();
        assert_eq!(n.kind.text().map(|t| t.characters.as_str()), Some("Typed"), "{what} took the text");
        let inside = n.source.is_some();
        assert_eq!(n.overrides.contains("characters"), inside, "{what}: overridden only inside an instance");
    }
}
