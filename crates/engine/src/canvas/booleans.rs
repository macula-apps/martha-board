//! Boolean groups in the board: existing boolean groups (`NodeKind::BooleanOp`) still open,
//! draw, select and move; nothing makes new ones or changes their operands' points (#32).
//!
//! - **Selecting operands.** A click selects the boolean group; a double click (or Cmd/Ctrl-click,
//!   or a click while a sibling operand is selected) selects the operand under the pointer. Clicks
//!   hit a boolean group where its result is (a click in a subtracted hole goes through to what's
//!   behind), except while it or a layer in it is selected: then its operands are hit, so each can
//!   be picked ([`result_outline`]).
//! - **Live results.** Moving or resizing an operand changes the document on every pointer move, so
//!   the group's result is redrawn during the drag (one undo step per gesture); a dragged operand
//!   stays in its group. While an operand is selected, the canvas outlines every operand of its
//!   group ([`outlined_operands`]).
//! - **Per-operand operations.** Each operand above the bottom one combines with the result of the
//!   operands below it by its own operation (`Node::bool_op`), or by the group's; the layers panel
//!   marks one that differs ([`distinct_op`]).

use figcraft_doc::{BoolOp, Document, Node, NodeId, NodeKind};
use figcraft_geom::BezPath;

/// Whether `id` is a boolean group.
pub fn is_group(doc: &Document, id: NodeId) -> bool {
    doc.node(id).is_some_and(|n| matches!(n.kind, NodeKind::BooleanOp { .. }))
}

/// The boolean group `id` is an operand of.
pub fn group_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    let parent = doc.node(id)?.parent?;
    is_group(doc, parent).then_some(parent)
}

/// An operand above the bottom one whose operation differs from its group's (what the layers
/// panel marks with the operation's icon).
pub fn distinct_op(doc: &Document, id: NodeId) -> Option<BoolOp> {
    let g = doc.node(group_of(doc, id)?)?;
    let NodeKind::BooleanOp { op } = g.kind else { return None };
    let own = doc.node(id)?.bool_op.filter(|o| *o != op)?;
    (g.children.first() != Some(&id)).then_some(own)
}

/// Boolean group results for hit testing ([`figcraft_doc::geometry::BoolResult`]): none for a
/// group that is selected or holds a selected layer, whose operands are hit instead.
pub fn result_outline<'a>(doc: &'a Document, selection: &'a [NodeId]) -> impl Fn(NodeId, &Node) -> Option<BezPath> + 'a {
    move |id, n| {
        if selection.iter().any(|s| doc.is_ancestor(id, *s)) {
            return None;
        }
        figcraft_render::node_outline(doc, n).map(|(p, _)| p)
    }
}

/// The operands to outline on the canvas while layers inside a boolean group are selected: every
/// operand of the nearest boolean group holding each of them, in layer order, without repeats.
pub fn outlined_operands(doc: &Document, selection: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for id in selection.iter().copied() {
        let Some(g) = doc.ancestors(id).into_iter().find(|a| is_group(doc, *a)) else { continue };
        for c in doc.node(g).map(|n| n.children.clone()).unwrap_or_default() {
            if doc.node(c).is_some_and(|n| n.visible) && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}
