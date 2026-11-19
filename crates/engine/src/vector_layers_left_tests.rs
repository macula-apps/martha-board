//! What the removal of vector editing (#32) leaves: no `vector.*`, `bool.*` or `shapeBuilder.*`
//! command and no Pen, Pencil or Shape builder tool, in the registry, the menus or the keymap; and
//! vector and boolean layers that a file already holds still open, draw, move, resize, export and
//! save.

use figcraft_color::{Color, Paint};
use figcraft_doc::{BoolOp, Document, ExportFormat, Node, NodeId, NodeKind, VectorNetwork};
use figcraft_geom::{BezPath, Rect};
use serde_json::json;

use crate::tools::ToolId;
use crate::{EngineError, Session, all_commands, find_command};

/// Every command id that left with vector editing.
const REMOVED: &[&str] = &[
    "bool.exclude",
    "bool.flatten",
    "bool.intersect",
    "bool.subtract",
    "bool.union",
    "shapeBuilder.merge",
    "shapeBuilder.mode",
    "shapeBuilder.regions",
    "shapeBuilder.remove",
    "tool.pen",
    "tool.pencil",
    "tool.pencilBrush",
    "tool.shapeBuilder",
    "vector.create",
    "vector.deleteHeal",
    "vector.edit.addVertex",
    "vector.edit.begin",
    "vector.edit.bend",
    "vector.edit.bounds",
    "vector.edit.connect",
    "vector.edit.cornerRadius",
    "vector.edit.cut",
    "vector.edit.delete",
    "vector.edit.end",
    "vector.edit.erase",
    "vector.edit.lasso",
    "vector.edit.mirroring",
    "vector.edit.moveHandles",
    "vector.edit.moveVertices",
    "vector.edit.nudge",
    "vector.edit.paint",
    "vector.edit.pen",
    "vector.edit.penEnd",
    "vector.edit.position",
    "vector.edit.resize",
    "vector.edit.rotate",
    "vector.edit.select",
    "vector.edit.selectAll",
    "vector.edit.setHandle",
    "vector.edit.smooth",
    "vector.edit.state",
    "vector.edit.strokeCap",
    "vector.edit.strokeJoin",
    "vector.edit.tool",
    "vector.edit.transform",
    "vector.edit.widthPoint",
    "vector.fillRule",
    "vector.flatten",
    "vector.join",
    "vector.offset",
    "vector.outlineStroke",
    "vector.reverse",
    "vector.reverseLoop",
    "vector.simplify",
    "vector.smoothJoin",
];

/// The tool ids that left with vector editing.
const REMOVED_TOOLS: &[&str] = &["pen", "pencil", "shapeBuilder"];

/// Whether `id` names a command of the vector editing that left.
fn vector_editing(id: &str) -> bool {
    ["vector.", "bool.", "shapeBuilder."].iter().any(|p| id.starts_with(p)) || REMOVED.contains(&id)
}

#[test]
fn no_vector_or_boolean_command_or_tool_is_left() {
    let ids: Vec<&str> = all_commands().iter().map(|c| c.id).collect();
    // What stays is there, so an empty list can't pass.
    for kept in ["node.create", "node.set", "file.open", "file.saveAs", "file.export", "board.ink", "tool.set", "tool.marker", "object.group"] {
        assert!(ids.contains(&kept), "{kept} is registered");
    }
    let left: Vec<&&str> = ids.iter().filter(|id| vector_editing(id)).collect();
    assert!(left.is_empty(), "still registered: {left:?}");
    let tools: Vec<&str> = ToolId::ALL.iter().map(|t| t.id()).collect();
    for kept in ["move", "rectangle", "ellipse", "line", "arrow", "polygon", "star", "sticky", "connector", "marker"] {
        assert!(tools.contains(&kept), "the {kept} tool stays");
    }
    for gone in REMOVED_TOOLS {
        assert!(!tools.contains(gone), "{gone} is a tool");
        assert!(ToolId::from_id(gone).is_none(), "{gone} reads as a tool");
        assert!(Session::new().execute("tool.set", &json!({"tool": gone})).is_err(), "tool.set takes {gone}");
    }
    let mut s = Session::new();
    for id in REMOVED {
        assert!(find_command(id).is_none(), "{id} is found");
        assert!(matches!(s.execute(id, &json!({})), Err(EngineError::UnknownCommand(_))), "{id} runs");
    }
    // Neither the menus (and the Actions palette, which lists them) nor the keymap name one.
    assert!(!crate::catalog::MENU.is_empty() && !crate::keymap::KEYS.is_empty());
    let menus: Vec<&str> = crate::catalog::MENU.iter().map(|m| m.id).filter(|id| vector_editing(id)).collect();
    assert!(menus.is_empty(), "in the menus: {menus:?}");
    let keys: Vec<&str> = crate::keymap::KEYS.iter().map(|b| b.id).filter(|id| vector_editing(id)).collect();
    assert!(keys.is_empty(), "in the keymap: {keys:?}");
}

/// The vector layer's place and size as the file holds it.
const VECTOR: Rect = Rect::new(10.0, 20.0, 110.0, 120.0);
/// Where `node.set` moves it, and the size it resizes it to.
const MOVED_TO: (f64, f64) = (40.0, 60.0);
const RESIZED_TO: (f64, f64) = (50.0, 25.0);
/// The vector's fill region's own fill.
const REGION_FILL: Color = Color::BLACK;
/// The boolean group's fill.
const GROUP_FILL: Color = Color::BLACK;
/// The boolean group's operands: B is subtracted from A, and they overlap in a 50 × 50 square.
const A: Rect = Rect::new(200.0, 0.0, 300.0, 100.0);
const B: Rect = Rect::new(250.0, 50.0, 350.0, 150.0);

/// A document holding a vector layer whose closed path has a fill region of its own, and a
/// subtracting boolean group of two rectangles, built from `figcraft_doc` types: (document,
/// vector, boolean group).
fn file_with_vector_layers() -> (Document, NodeId, NodeId) {
    let mut doc = Document::new("Vector layers");
    let page = doc.current_page.unwrap();
    let mut square = BezPath::new();
    square.move_to((0.0, 0.0));
    square.line_to((VECTOR.width(), 0.0));
    square.line_to((VECTOR.width(), VECTOR.height()));
    square.line_to((0.0, VECTOR.height()));
    square.close_path();
    let mut network = VectorNetwork::from_path(&square);
    assert_eq!(network.regions.len(), 1, "a closed path: one fill region");
    network.regions[0].fills = Some(vec![Paint::solid(REGION_FILL)]);
    let v = doc.create(page, NodeKind::Vector { network }, "Vector", VECTOR.x0, VECTOR.y0, VECTOR.width(), VECTOR.height()).unwrap();
    let a = doc.create(page, NodeKind::Rectangle, "A", A.x0, A.y0, A.width(), A.height()).unwrap();
    let b = doc.create(page, NodeKind::Rectangle, "B", B.x0, B.y0, B.width(), B.height()).unwrap();
    let g = doc.alloc_id();
    let mut group = Node::new(g, NodeKind::BooleanOp { op: BoolOp::Subtract }, BoolOp::Subtract.label(), 0.0, 0.0);
    group.fills = vec![Paint::solid(GROUP_FILL)];
    doc.insert(group, page, None).unwrap();
    doc.reparent(a, g, None).unwrap();
    doc.reparent(b, g, None).unwrap();
    doc.fit_to_children(g).unwrap();
    (doc, v, g)
}

/// The PNG export of each of `ids` in the session's document.
fn pngs(s: &Session, ids: &[NodeId]) -> Vec<Vec<u8>> {
    ids.iter().map(|id| crate::cmd::file::export_bytes(s.doc().unwrap(), &[*id], ExportFormat::Png, 1.0).unwrap()).collect()
}

/// The pixel at page point `at` in the PNG export of `id`. The export covers the layer's bounds at
/// scale 1, so a page point lands on the pixel at its offset from the bounds' top left.
fn pixel(s: &Session, id: NodeId, at: (f64, f64)) -> [u8; 4] {
    let png = crate::cmd::file::export_bytes(s.doc().unwrap(), &[id], ExportFormat::Png, 1.0).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    let b = bounds(s, id);
    assert_eq!((f64::from(img.width()), f64::from(img.height())), (b.width().ceil(), b.height().ceil()), "the export is the layer's bounds");
    // Whole pixels: the points asked for lie well inside their shapes.
    let (x, y) = ((at.0 - b.x0).floor(), (at.1 - b.y0).floor());
    img.get_pixel(x as u32, y as u32).0
}

/// The middle of the span from `a` to `b`, on each axis.
fn between(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1))
}

/// What the exports draw: the vector's fill region painted black, and the boolean group A minus
/// B: painted where only A is, and not where B overlaps it or where only B is.
fn assert_exports_draw(s: &Session, v: NodeId, g: NodeId) {
    assert_eq!(pixel(s, v, (VECTOR.center().x, VECTOR.center().y)), REGION_FILL.to_rgba8(), "the vector's fill region");
    assert_eq!(pixel(s, g, between((A.x0, A.y0), (B.x0, B.y0))), GROUP_FILL.to_rgba8(), "A outside B");
    assert_eq!(pixel(s, g, between((B.x0, B.y0), (A.x1, A.y1)))[3], 0, "the overlap B takes away");
}

/// The layers on the page, and the operands of boolean group `g`.
fn layer_count(s: &Session, g: NodeId) -> (usize, usize) {
    let doc = s.doc().unwrap();
    (doc.node(s.page().unwrap()).unwrap().children.len(), doc.node(g).unwrap().children.len())
}

fn bounds(s: &Session, id: NodeId) -> Rect {
    s.doc().unwrap().world_bounds(id).unwrap()
}

/// A folder of the test's own under the system temp folder, removed when dropped.
struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn vector_layers_in_a_file_open_draw_move_resize_export_and_save() {
    let dir = TempDir(std::env::temp_dir().join(format!("figcraft-vector-layers-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let path = |name: &str| dir.0.join(name).to_string_lossy().to_string();
    let (doc, v, g) = file_with_vector_layers();
    std::fs::write(path("built.figcraft"), figcraft_format::save_bytes(&doc, None).unwrap()).unwrap();

    // Opened the way the app opens a file.
    let mut s = Session::empty();
    s.execute("file.open", &json!({"path": path("built.figcraft")})).unwrap();
    assert_eq!(layer_count(&s, g), (2, 2), "the vector and the boolean group, with its two operands");
    assert_eq!(bounds(&s, v), VECTOR);
    let before = pngs(&s, &[v, g]);
    assert_exports_draw(&s, v, g);

    // Saved and opened again: the same layers draw the same.
    s.execute("file.saveAs", &json!({"path": path("saved.figcraft")})).unwrap();
    let mut t = Session::empty();
    t.execute("file.open", &json!({"path": path("saved.figcraft")})).unwrap();
    assert_eq!(layer_count(&t, g), (2, 2));
    assert_eq!(pngs(&t, &[v, g]), before, "the PNG exports after a save and reopen");
    assert_exports_draw(&t, v, g);

    // Moved and resized through `node.set`, and still there after another save.
    t.execute("node.set", &json!({"id": v, "props": {"x": MOVED_TO.0, "y": MOVED_TO.1}})).unwrap();
    let moved = Rect::new(MOVED_TO.0, MOVED_TO.1, MOVED_TO.0 + VECTOR.width(), MOVED_TO.1 + VECTOR.height());
    assert_eq!(bounds(&t, v), moved);
    t.execute("node.set", &json!({"id": v, "props": {"width": RESIZED_TO.0, "height": RESIZED_TO.1}})).unwrap();
    let resized = Rect::new(MOVED_TO.0, MOVED_TO.1, MOVED_TO.0 + RESIZED_TO.0, MOVED_TO.1 + RESIZED_TO.1);
    assert_eq!(bounds(&t, v), resized);
    let NodeKind::Vector { network } = &t.doc().unwrap().node(v).unwrap().kind else { panic!("still a vector") };
    assert_eq!(network.regions.len(), 1, "the fill region stays");
    assert_eq!(network.regions[0].fills, Some(vec![Paint::solid(REGION_FILL)]), "with its own fill");
    t.execute("file.save", &json!({})).unwrap();
    let mut u = Session::empty();
    u.execute("file.open", &json!({"path": path("saved.figcraft")})).unwrap();
    assert_eq!(layer_count(&u, g), (2, 2));
    assert_eq!(bounds(&u, v), resized);
    assert_eq!(pngs(&u, &[v, g]), pngs(&t, &[v, g]));
}
