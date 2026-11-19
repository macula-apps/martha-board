//! Tests: a boolean group written into the document as a file holds one (the boolean commands left
//! with vector editing, #32).

use figcraft_doc::{BoolOp, Node, NodeId, NodeKind};
use figcraft_engine::Session;

/// A boolean group doing `op` over sibling layers `ids` (bottom first): it goes where the top
/// layer was, looks like that layer (a subtraction like the bottom one it cuts) and fits its
/// operands. The selection is left as it was.
pub(crate) fn bool_group(s: &mut Session, ids: &[NodeId], op: BoolOp) -> NodeId {
    let doc = s.doc_mut().unwrap();
    let top = *ids.last().unwrap();
    let parent = doc.node(top).unwrap().parent.unwrap();
    let index = doc.index_in_parent(top).map(|i| i + 1);
    let local = figcraft_geom::transform_rect_bbox(doc.world_transform(parent).inverse(), doc.bounds_of(ids).unwrap());
    let look = if op == BoolOp::Subtract { ids.first() } else { ids.last() };
    let look = doc.node(*look.unwrap()).unwrap().clone();
    let gid = doc.alloc_id();
    let mut g = Node::new(gid, NodeKind::BooleanOp { op }, op.label(), local.width(), local.height());
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
