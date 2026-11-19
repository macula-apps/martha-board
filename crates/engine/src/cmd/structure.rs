//! Structure: groups, frames, sections, masks, auto layout, pages.

use figcraft_color::Paint;
use figcraft_doc::{AutoLayout, Corners, Document, LayoutMode, Node, NodeId, NodeKind, Sizing, proto};
use figcraft_geom::{Affine, Rect};
use serde_json::{Value, json};

use super::edit::{roots, z_sorted};
use super::{CommandSpec, ok};

mod suggest;
use crate::params::{id_of, num_or, req_id, req_targets, string, targets};
use crate::{EngineError, Result, Session, bad, spec};

/// The layer structure commands: group, frame, section, mask, auto layout add/remove, move to page
/// and rasterize.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("object.group", "Group selection", "{ids?, name?}", true, |s, p| wrap(s, p, "object.group", Wrap::Group)),
        spec!("object.ungroup", "Ungroup selection", "{ids?}", true, ungroup),
        spec!("object.frameSelection", "Frame selection", "{ids?, name?}", true, |s, p| wrap(s, p, "object.frameSelection", Wrap::Frame)),
        spec!("object.removeFrame", "Remove frame", "{ids?}", true, ungroup),
        spec!(
            "object.convertToGroup",
            "Convert to group",
            "{ids?} (frames become groups in place, their layers staying where they are; what only a frame has goes: auto layout, fills, strokes, corner radius, clipping, layout grids, guides and scrolling. Groups are left alone; instances, main components, layers inside instances and empty frames are refused)",
            true,
            |s, p| convert(s, p, "object.convertToGroup", false)
        ),
        spec!(
            "object.convertToFrame",
            "Convert to frame",
            "{ids?} (groups become frames of the same box in place, their layers staying where they are: no fill, not clipping; sections become frames that keep their fill and don't clip. Frames are left alone; instances, main components, layers inside instances and sections holding sections are refused)",
            true,
            |s, p| convert(s, p, "object.convertToFrame", true)
        ),
        spec!(
            "object.convertToSection",
            "Convert to section",
            "{ids?} (frames on the page or in a section become sections in place, keeping their fill and layers; what only a frame has goes: auto layout, clipping, layout grids, guides and scrolling. Sections are left alone; nested frames, instances and main components are refused)",
            true,
            convert_to_section
        ),
        spec!(
            "object.wrapInSection",
            "Wrap in new section",
            "{ids?, name?} (layers on the page or in a section; the section wraps them with some room)",
            true,
            |s, p| wrap(s, p, "object.wrapInSection", Wrap::Section)
        ),
        spec!(
            "object.resizeToFit",
            "Resize to fit",
            "{ids?} (frames, sections and components shrink or grow to their contents; the contents stay put)",
            true,
            resize_to_fit
        ),
        spec!("object.mask", "Use as mask", "{ids?}", true, mask),
        spec!("object.releaseMask", "Release mask", "{ids?}", true, |s, p| {
            for i in req_targets("object.releaseMask", s, p)? {
                s.doc_mut()?.get_mut(i)?.is_mask = false;
            }
            ok()
        }),
        spec!("layout.add", "Add auto layout", "{ids?, direction? (horizontal|vertical|grid)}", true, layout_add),
        spec!("layout.remove", "Remove auto layout", "{ids?}", true, |s, p| {
            for i in req_targets("layout.remove", s, p)? {
                // Hugging needs auto layout and the children that filled have nothing to fill: both
                // become fixed (the frame keeps filling its own auto layout parent).
                let doc = s.doc_mut()?;
                let n = doc.get_mut(i)?;
                n.layout = None;
                super::props::drop_layout_bindings(n);
                figcraft_layout::coherent_sizing(doc, i);
            }
            ok()
        }),
        spec!(
            "layout.removeAll",
            "Remove all auto layout",
            "{ids?}  takes the auto layout off the frames and every frame nested in them at once (More layout options ▸ Remove all auto layout)",
            true,
            |s, p| {
                let ids = req_targets("layout.removeAll", s, p)?;
                let doc = s.doc_mut()?;
                let mut removed = Vec::new();
                for root in ids {
                    for i in doc.subtree(root) {
                        let strip = doc.node(i).is_some_and(|n| n.layout.is_some());
                        if !strip {
                            continue;
                        }
                        let n = doc.get_mut(i)?;
                        n.layout = None;
                        super::props::drop_layout_bindings(n);
                        figcraft_layout::coherent_sizing(doc, i);
                        removed.push(i);
                    }
                }
                Ok(json!({"removed": removed}))
            }
        ),
        spec!(
            "layout.suggest",
            "Suggest auto layout",
            "{ids?, direction? (horizontal|vertical|grid)}  splits each frame's layers into rows or columns by the gaps between them, wraps every band of several layers in a frame without fill that hugs it and does the same inside, so the layers keep their place where the gaps are even and the bands start at the same offset across the flow; the frame keeps its size, and the result carries `warnings` for gaps that differ, bands that start at different offsets (they line up at the smallest and some move) and layers that overlap. Instances and the layers inside them are refused. `direction` sets the top level's direction (nested frames follow the gaps); grid lays the layers out in one grid. A group is converted to a frame and several layers are wrapped in one, as Add auto layout does",
            true,
            suggest::layout_suggest
        ),
        spec!("object.moveToPage", "Move to page", "{ids?, page}", true, move_to_page),
        spec!("object.setThumbnail", "Set as thumbnail", "{id?}", true, |s, p| {
            let id = targets(s, p).first().copied().ok_or_else(|| bad("object.setThumbnail", "select a frame"))?;
            let doc = s.doc_mut()?;
            for n in doc.ids().collect::<Vec<_>>() {
                if let Some(node) = doc.node(n)
                    && node.plugin_data.contains_key("figcraft.thumbnail")
                {
                    doc.get_mut(n)?.plugin_data.remove("figcraft.thumbnail");
                }
            }
            doc.get_mut(id)?.plugin_data.insert("figcraft.thumbnail".into(), "true".into());
            ok()
        }),
        spec!("object.rasterize", "Rasterize selection", "{ids?, scale? (default 2)}", true, rasterize),
    ]
}

enum Wrap {
    Group,
    Frame,
    Section,
}

/// Room a section made around layers leaves on each side.
const SECTION_PAD: f64 = 40.0;

/// Wrap the selection in a new container at the topmost selected layer's place. Errors name `cmd`,
/// the command that asked.
fn wrap(s: &mut Session, p: &Value, cmd: &str, w: Wrap) -> Result<Value> {
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc_mut()?;
    let ids = z_sorted(doc, &roots(doc, &ids));
    let top = *ids.last().ok_or_else(|| bad(cmd, "nothing selected"))?;
    let parent = doc.get(top)?.parent.ok_or_else(|| bad(cmd, "can't group a page"))?;
    // Sections sit on the page or in sections, never in frames or groups.
    let board = |doc: &Document, i: NodeId| {
        doc.node(i).and_then(|n| n.parent).and_then(|p| doc.node(p)).is_some_and(|p| p.is_page() || matches!(p.kind, NodeKind::Section { .. }))
    };
    if matches!(w, Wrap::Section) && !ids.iter().all(|i| board(doc, *i)) {
        return Err(bad(cmd, "only layers on the page or in a section can be wrapped in a section"));
    }
    if !matches!(w, Wrap::Section) && ids.iter().any(|i| doc.node(*i).is_some_and(|n| matches!(n.kind, NodeKind::Section { .. }))) {
        return Err(bad(cmd, "sections can't go in groups or frames"));
    }
    let index = doc.index_in_parent(top).map(|i| i + 1);
    let mut bounds = doc.bounds_of(&ids).unwrap_or_default();
    if matches!(w, Wrap::Section) {
        bounds = bounds.inflate(SECTION_PAD, SECTION_PAD);
    }
    let pw = doc.world_transform(parent);
    let local = figcraft_geom::transform_rect_bbox(pw.inverse(), bounds);
    let (kind, base) = match &w {
        Wrap::Group => (NodeKind::Group, "Group"),
        Wrap::Frame => (NodeKind::Frame, "Frame"),
        Wrap::Section => (NodeKind::Section { contents_hidden: false }, "Section"),
    };
    let name = string(p, "name").unwrap_or_else(|| doc.next_name(base));
    let gid = doc.alloc_id();
    let mut g = match w {
        Wrap::Section => Node::new_section(gid, name, local.width(), local.height()),
        _ => Node::new(gid, kind, name, local.width(), local.height()),
    };
    g.transform = Affine::translate((local.x0, local.y0));
    if let Wrap::Frame = w {
        g.clip = true;
    }
    // Insert the container where the topmost layer is.
    doc.insert(g, parent, index)?;
    for i in &ids {
        doc.reparent(*i, gid, None)?;
    }
    if !matches!(w, Wrap::Frame | Wrap::Section) {
        let _ = doc.fit_to_children(gid);
    }
    s.set_selection(vec![gid]);
    Ok(json!({"id": gid}))
}

/// Turn frames into groups or groups into frames in place (same id, name, layers, effects,
/// opacity, exports and interactions), the layers inside staying where they are on the page.
/// Nothing changes unless every layer can convert.
fn convert(s: &mut Session, p: &Value, cmd: &'static str, to_frame: bool) -> Result<Value> {
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc_mut()?;
    let mut todo = Vec::new();
    for i in ids {
        let n = doc.get(i)?;
        let refuse = |why: &str| Err(bad(cmd, format!("\"{}\" {why}", n.name)));
        match (&n.kind, to_frame) {
            // A frame can't hold sections.
            (NodeKind::Section { .. }, true)
                if n.children.iter().any(|c| doc.node(*c).is_some_and(|c| matches!(c.kind, NodeKind::Section { .. }))) =>
            {
                return refuse("holds sections, which can't go inside a frame: convert those first");
            }
            (NodeKind::Section { .. }, true) => todo.push(i),
            (NodeKind::Instance { .. }, _) => return refuse("is an instance, which stays as it is"),
            (NodeKind::Component { .. }, _) => return refuse("is a main component, which stays a frame"),
            (NodeKind::Frame, false) if n.children.is_empty() => return refuse("is empty: a group needs layers"),
            (NodeKind::Frame, false) | (NodeKind::Group, true) => todo.push(i),
            (NodeKind::Frame | NodeKind::Group, _) => {}
            _ => return refuse("is neither a frame nor a group"),
        }
    }
    for i in &todo {
        if to_frame {
            // The box as it is and nothing clipped, so the look stays: a group has nothing painted, a
            // section keeps its fill.
            let n = doc.get_mut(*i)?;
            if matches!(n.kind, NodeKind::Group) {
                n.fills.clear();
                n.strokes.clear();
            }
            n.kind = NodeKind::Frame;
            n.clip = false;
            continue;
        }
        let n = doc.get_mut(*i)?;
        n.kind = NodeKind::Group;
        (n.layout, n.clip, n.overflow) = (None, false, proto::Overflow::None);
        n.fills.clear();
        n.strokes.clear();
        n.grids.clear();
        n.guides.clear();
        n.corners = Corners::default();
        (n.styles.fill, n.styles.stroke, n.styles.grid) = (None, None, None);
        // Variables bound to what went would bring it back.
        n.bound.retain(|k, _| matches!(k.as_str(), "opacity" | "visible"));
        let kids = n.children.clone();
        for k in kids {
            // Out of the flow there is nothing to fill or to be absolute against.
            doc.get_mut(k)?.child_layout.absolute = false;
        }
        figcraft_layout::coherent_sizing(doc, *i);
        doc.fit_to_children(*i)?;
    }
    Ok(json!({"ids": todo}))
}

/// Frames on the page or in a section become sections in place (sections only sit there).
fn convert_to_section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "object.convertToSection";
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc_mut()?;
    let mut todo = Vec::new();
    for i in ids {
        let n = doc.get(i)?;
        let refuse = |why: &str| Err(bad(cmd, format!("\"{}\" {why}", n.name)));
        let on_board = n.parent.and_then(|p| doc.node(p)).is_some_and(|p| p.is_page() || matches!(p.kind, NodeKind::Section { .. }));
        match &n.kind {
            NodeKind::Section { .. } => {}
            NodeKind::Instance { .. } => return refuse("is an instance: its layers come from its main component (detach it first)"),
            NodeKind::Component { .. } | NodeKind::ComponentSet { .. } => return refuse("is a main component, which stays a frame"),
            NodeKind::Frame if !on_board => return refuse("is inside a frame or group: only frames on the page or in a section become sections"),
            NodeKind::Frame => todo.push(i),
            _ => return refuse("is not a frame"),
        }
    }
    for i in &todo {
        let n = doc.get_mut(*i)?;
        n.kind = NodeKind::Section { contents_hidden: false };
        (n.layout, n.clip, n.overflow) = (None, false, proto::Overflow::None);
        n.grids.clear();
        n.guides.clear();
        n.styles.grid = None;
        super::props::drop_layout_bindings(n);
        let kids = n.children.clone();
        for k in kids {
            // Out of the flow there is nothing to fill or to be absolute against.
            doc.get_mut(k)?.child_layout.absolute = false;
        }
        figcraft_layout::coherent_sizing(doc, *i);
    }
    Ok(json!({"ids": todo}))
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("object.ungroup", s, p)?;
    let doc = s.doc_mut()?;
    let mut out = Vec::new();
    for g in ids {
        let n = doc.get(g)?;
        if !(matches!(n.kind, NodeKind::Group | NodeKind::BooleanOp { .. } | NodeKind::Frame | NodeKind::Section { .. })) {
            continue;
        }
        let Some(parent) = n.parent else { continue };
        let idx = doc.index_in_parent(g).unwrap_or(0);
        let kids = n.children.clone();
        // Ungrouping a group keeps its opacity on the children (multiplied).
        let op = n.opacity;
        for (k, c) in kids.iter().enumerate() {
            doc.reparent(*c, parent, Some(idx + k))?;
            // Released operands no longer combine with anything.
            if doc.get(*c)?.bool_op.is_some() {
                doc.get_mut(*c)?.bool_op = None;
            }
            if op < 1.0 {
                let cn = doc.get_mut(*c)?;
                cn.opacity *= op;
            }
            out.push(*c);
        }
        doc.remove(g)?;
    }
    s.set_selection(out.clone());
    Ok(json!({"ids": out}))
}

fn mask(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("object.mask", s, p)?;
    if ids.len() == 1 {
        let doc = s.doc_mut()?;
        let id = ids.first().copied().unwrap_or(NodeId(0));
        let n = doc.get_mut(id)?;
        n.is_mask = !n.is_mask;
        return ok();
    }
    // Group, then the bottom-most child is the mask.
    let r = wrap(s, &json!({"ids": ids, "name": "Mask group"}), "object.mask", Wrap::Group)?;
    let gid = id_of(&r["id"]).ok_or_else(|| EngineError::Other("group failed".into()))?;
    let doc = s.doc_mut()?;
    if let Some(first) = doc.get(gid)?.children.first().copied() {
        doc.get_mut(first)?.is_mask = true;
    }
    Ok(r)
}

/// Infer direction, gap and padding from visible, non-absolute children, matching the layout flow.
fn infer_layout(doc: &Document, frame: NodeId) -> AutoLayout {
    let Some(f) = doc.node(frame) else { return AutoLayout::default() };
    let boxes: Vec<Rect> = figcraft_layout::flow_children(doc, frame)
        .iter()
        .filter_map(|c| doc.node(*c))
        .map(|c| figcraft_geom::transform_rect_bbox(c.transform, c.local_rect()))
        .collect();
    if boxes.is_empty() {
        return AutoLayout { padding: [10.0; 4], gap: 10.0, ..Default::default() };
    }
    let span_x = boxes.iter().map(|b| b.x1).fold(f64::MIN, f64::max) - boxes.iter().map(|b| b.x0).fold(f64::MAX, f64::min);
    let span_y = boxes.iter().map(|b| b.y1).fold(f64::MIN, f64::max) - boxes.iter().map(|b| b.y0).fold(f64::MAX, f64::min);
    let vertical = span_y > span_x;
    let mut sorted = boxes.clone();
    sorted.sort_by(|a, b| if vertical { a.y0.total_cmp(&b.y0) } else { a.x0.total_cmp(&b.x0) });
    let gaps: Vec<f64> =
        sorted.windows(2).filter_map(|w| Some(if vertical { w.get(1)?.y0 - w.first()?.y1 } else { w.get(1)?.x0 - w.first()?.x1 })).collect();
    let gap = if gaps.is_empty() { 10.0 } else { (gaps.iter().sum::<f64>() / gaps.len() as f64).max(0.0).round() };
    let all = boxes.iter().copied().reduce(|a, b| a.union(b)).unwrap_or_default();
    let pad = [all.y0.max(0.0), (f.width - all.x1).max(0.0), (f.height - all.y1).max(0.0), all.x0.max(0.0)].map(f64::round);
    AutoLayout { mode: if vertical { LayoutMode::Vertical } else { LayoutMode::Horizontal }, gap, padding: pad, ..Default::default() }
}

/// Sort unmasked spans by position along the layout axis. Masks stay at their original indices;
/// each visible mask's following content keeps its paint order until the next mask, hidden or not.
fn sort_layout_children(doc: &Document, kids: &mut [NodeId], vertical: bool) {
    let mut start = 0;
    let mut masked = false;
    for end in 0..=kids.len() {
        let mask = kids.get(end).and_then(|id| doc.node(*id)).filter(|n| n.is_mask);
        if end != kids.len() && mask.is_none() {
            continue;
        }
        if !masked && let Some(span) = kids.get_mut(start..end) {
            span.sort_by(|a, b| {
                let pa = doc.node(*a).map(|n| figcraft_geom::transform_rect_bbox(n.transform, n.local_rect())).unwrap_or_default();
                let pb = doc.node(*b).map(|n| figcraft_geom::transform_rect_bbox(n.transform, n.local_rect())).unwrap_or_default();
                if vertical { pa.y0.total_cmp(&pb.y0) } else { pa.x0.total_cmp(&pb.x0) }
            });
        }
        start = end + 1;
        masked = mask.is_some_and(|n| n.visible);
    }
}

/// Add inferred auto layout to existing frames or one group converted in place; wrap other
/// selections. Preserve mask boundaries and a converted group's Fill sizing in its parent.
fn layout_add(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("layout.add", s, p)?;
    let dir = string(p, "direction");
    let doc = s.doc()?;
    let all_frames = ids.iter().all(|i| doc.node(*i).is_some_and(|n| n.kind.is_frame_like() && n.layout.is_none()));
    let single_group = ids.len() == 1 && ids.first().and_then(|i| doc.node(*i)).is_some_and(|n| matches!(n.kind, NodeKind::Group));
    let in_place = all_frames || single_group;
    let targets = if in_place {
        if single_group {
            // Layout must arrange the group's children directly. Wrapping the group would
            // leave them in a single fixed-size layout item and add an unnecessary layer.
            convert(s, &json!({"ids": ids}), "layout.add", true)?;
        }
        ids
    } else {
        // Wrap non-frames in a new frame without a fill, then add auto layout to it.
        let r = wrap(s, &json!({"ids": ids}), "layout.add", Wrap::Frame)?;
        let fid = id_of(&r["id"]).ok_or_else(|| EngineError::Other("wrap failed".into()))?;
        let f = s.doc_mut()?.get_mut(fid)?;
        f.fills.clear();
        f.clip = false;
        vec![fid]
    };
    let doc = s.doc_mut()?;
    for f in &targets {
        let mut al = infer_layout(doc, *f);
        if let Some(d) = &dir {
            al.mode = match d.to_ascii_lowercase().as_str() {
                "vertical" | "column" => LayoutMode::Vertical,
                "grid" => LayoutMode::Grid,
                _ => LayoutMode::Horizontal,
            };
        }
        let mut kids = doc.get(*f)?.children.clone();
        sort_layout_children(doc, &mut kids, al.mode == LayoutMode::Vertical);
        let empty = doc.node(*f).is_some_and(|n| n.children.iter().filter_map(|c| doc.node(*c)).all(|c| !c.visible || c.child_layout.absolute));
        let n = doc.get_mut(*f)?;
        n.children = kids;
        n.layout = Some(al);
        if !in_place || !empty {
            // A frame wrapped around the selection hugs it; an existing frame with no flow
            // children keeps its size instead of collapsing to its padding.
            if !single_group || n.child_layout.sizing_h != Sizing::Fill {
                n.child_layout.sizing_h = Sizing::Hug;
            }
            if !single_group || n.child_layout.sizing_v != Sizing::Fill {
                n.child_layout.sizing_v = Sizing::Hug;
            }
        }
    }
    s.set_selection(targets.clone());
    Ok(json!({"ids": targets}))
}

fn move_to_page(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("object.moveToPage", s, p)?;
    let page = req_id("object.moveToPage", p, "page")?;
    let doc = s.doc_mut()?;
    if !doc.node(page).is_some_and(Node::is_page) {
        return Err(bad("object.moveToPage", format!("{page} is not a page")));
    }
    if doc.is_divider(page) {
        return Err(bad("object.moveToPage", "a divider holds no layers"));
    }
    for i in z_sorted(doc, &roots(doc, &ids)) {
        doc.reparent(i, page, None)?;
    }
    s.set_selection(vec![]);
    ok()
}

fn rasterize(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("object.rasterize", s, p)?;
    let scale = num_or(p, "scale", 2.0);
    let mut out = Vec::new();
    for i in ids {
        let doc = s.doc()?;
        let Some(pix) = figcraft_render::render_node(doc, i, scale) else { continue };
        let png = figcraft_render::encode_png(&pix).map_err(EngineError::Other)?;
        let b = figcraft_render::export_bounds(doc, i).unwrap_or_default();
        let parent = doc.get(i)?.parent.ok_or_else(|| bad("object.rasterize", "page"))?;
        let idx = doc.index_in_parent(i);
        let pw = doc.world_transform(parent);
        let name = doc.get(i)?.name.clone();
        let docm = s.doc_mut()?;
        let (hash, _, _) = figcraft_format::add_image(docm, png).map_err(|e| EngineError::Other(e.to_string()))?;
        let rid = docm.alloc_id();
        let mut r = Node::new(rid, NodeKind::Rectangle, name, b.width(), b.height());
        r.transform = pw.inverse() * Affine::translate((b.x0, b.y0));
        r.fills = vec![Paint::image(hash, figcraft_color::ImageScaleMode::Fill)];
        docm.insert(r, parent, idx.map(|x| x + 1))?;
        docm.remove(i)?;
        out.push(rid);
    }
    s.set_selection(out.clone());
    Ok(json!({"ids": out}))
}

/// Fit containers to their visible children's bounds, keeping the children where they are on the
/// page (auto layout frames hug instead); rotated containers fit in their own axes.
fn resize_to_fit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "object.resizeToFit";
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc_mut()?;
    let mut fitted = Vec::new();
    for id in ids {
        let Some(n) = doc.node(id).cloned() else { continue };
        let container = n.kind.is_frame_like() || matches!(n.kind, NodeKind::Section { .. });
        if !container || n.children.is_empty() || matches!(n.kind, NodeKind::Instance { .. }) {
            continue;
        }
        // Auto layout frames fit by hugging their contents.
        if n.layout.is_some() {
            let node = doc.get_mut(id)?;
            node.child_layout.sizing_h = Sizing::Hug;
            node.child_layout.sizing_v = Sizing::Hug;
            fitted.push(id);
            continue;
        }
        // Children's bounds in the container's own coordinates.
        let mut b: Option<figcraft_geom::Rect> = None;
        for c in n.children.iter().filter_map(|c| doc.node(*c)).filter(|c| c.visible) {
            let r = figcraft_geom::transform_rect_bbox(c.transform, c.local_rect());
            if r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite() {
                b = Some(b.map_or(r, |u| u.union(r)));
            }
        }
        let Some(b) = b else { continue };
        let (w, h) = (b.width().max(0.01), b.height().max(0.01));
        // Move the container by the offset, and its children back by it.
        let shift = Affine::translate((b.x0, b.y0));
        let node = doc.get_mut(id)?;
        node.transform *= shift;
        node.width = w;
        node.height = h;
        for c in &n.children {
            if let Ok(cn) = doc.get_mut(*c) {
                cn.transform = shift.inverse() * cn.transform;
            }
        }
        fitted.push(id);
    }
    Ok(json!({"ids": fitted}))
}
