//! Helpers the engine's tests share: run a command, read an id, look at a layer.

use figcraft_doc::{Node, NodeId};
use serde_json::Value;

use crate::Session;
use crate::params::id_of;

/// Run `cmd` with `p`, panicking with the command and its params when it fails.
pub(crate) fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    match s.execute(cmd, &p) {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {p}: {e}"),
    }
}

/// The `id` a command answered with.
pub(crate) fn idv(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

/// Layer `id` of the active document, as it is now.
pub(crate) fn node(s: &Session, id: NodeId) -> Node {
    s.doc().unwrap().node(id).unwrap().clone()
}

/// The characters of text layer `id` (empty for other layers).
pub(crate) fn chars(s: &Session, id: NodeId) -> String {
    node(s, id).kind.text().map(|t| t.characters.clone()).unwrap_or_default()
}

/// A boolean group doing `op` over sibling layers `ids` (bottom first), built from `figcraft_doc`
/// types the way a file holds one now that no command makes one (#32): the group goes where the
/// top layer was, looks like that layer (a subtraction like the bottom one it cuts) and fits its
/// operands. The selection is left as it was.
pub(crate) fn bool_group(s: &mut Session, ids: &[NodeId], op: figcraft_doc::BoolOp) -> NodeId {
    let doc = s.doc_mut().unwrap();
    let top = *ids.last().unwrap();
    let parent = doc.node(top).unwrap().parent.unwrap();
    let index = doc.index_in_parent(top).map(|i| i + 1);
    let local = figcraft_geom::transform_rect_bbox(doc.world_transform(parent).inverse(), doc.bounds_of(ids).unwrap());
    let look = if op == figcraft_doc::BoolOp::Subtract { ids.first() } else { ids.last() };
    let look = doc.node(*look.unwrap()).unwrap().clone();
    let gid = doc.alloc_id();
    let mut g = Node::new(gid, figcraft_doc::NodeKind::BooleanOp { op }, op.label(), local.width(), local.height());
    g.transform = figcraft_geom::Affine::translate((local.x0, local.y0));
    g.fills = look.fills;
    g.strokes = look.strokes;
    g.stroke = look.stroke;
    doc.insert(g, parent, index).unwrap();
    for i in ids {
        doc.reparent(*i, gid, None).unwrap();
    }
    doc.fit_to_children(gid).unwrap();
    gid
}
