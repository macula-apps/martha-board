use super::{
    Affine, BTreeMap, Document, EngineError, ImageData, Node, NodeId, NodeKind, Result, Session, Value, bad, delete, get, id, json, num, ok,
    req_targets, roots, targets, vector_clipboard, z_sorted,
};

/// The copied form of the selection.
pub fn copy_to_clipboard(s: &mut Session, cmd: &str, ids: &[NodeId]) -> Result<()> {
    let ds = s.ds()?;
    // Pages are not layers: they can't be pasted anywhere.
    let layers: Vec<NodeId> = ids.iter().copied().filter(|i| !ds.doc.node(*i).is_some_and(Node::is_page)).collect();
    if layers.is_empty() && !ids.is_empty() {
        return Err(EngineError::Disabled(cmd.into(), "pages can't be copied; use page.duplicate".into()));
    }
    let (nodes, images, bounds) = copied(&ds.doc, &layers);
    store_clipboard(s, cmd, nodes, images, bounds)
}

/// Make `nodes` (the copied layers, each root's transform in page space) the clipboard's layers.
pub(super) fn store_clipboard(
    s: &mut Session,
    cmd: &str,
    nodes: Vec<(NodeId, Vec<Node>)>,
    images: BTreeMap<String, ImageData>,
    bounds: Option<figcraft_geom::Rect>,
) -> Result<()> {
    let ds = s.ds()?;
    let (source, source_path, uid) = (ds.doc.clone(), ds.path.clone(), ds.uid);
    s.clipboard.source = Some(source);
    s.clipboard.source_path = source_path;
    s.clipboard.source_doc = uid;
    s.clipboard.copied_components = nodes
        .iter()
        .filter(|(_, sub)| {
            cmd == "edit.copy" && sub.first().is_some_and(|n| matches!(n.kind, NodeKind::Component { .. } | NodeKind::ComponentSet { .. }))
        })
        .map(|(root, _)| *root)
        .collect();
    s.clipboard.nodes = nodes;
    s.clipboard.images = images;
    s.clipboard.bounds = bounds;
    s.clipboard.layers_generation = s.clipboard.layers_generation.wrapping_add(1);
    Ok(())
}

type Copied = (Vec<(NodeId, Vec<Node>)>, BTreeMap<String, ImageData>, Option<figcraft_geom::Rect>);

fn copied(doc: &Document, ids: &[NodeId]) -> Copied {
    let ids = z_sorted(doc, &roots(doc, ids));
    let mut nodes = Vec::new();
    let mut images = BTreeMap::new();
    for i in &ids {
        let world = doc.world_transform(*i);
        let mut sub: Vec<Node> = doc.subtree(*i).into_iter().filter_map(|x| doc.node(x).cloned()).collect();
        // Store the root's transform in page space so pastes land where it was.
        if let Some(r) = sub.first_mut() {
            r.transform = world;
        }
        collect_images(doc, &sub, &mut images);
        nodes.push((*i, sub));
    }
    let bounds = doc.bounds_of(&ids);
    (nodes, images, bounds)
}

/// The images the paints of `nodes` use, taken from `doc` into `images`.
pub(super) fn collect_images(doc: &Document, nodes: &[Node], images: &mut BTreeMap<String, ImageData>) {
    for n in nodes {
        for p in n.fills.iter().chain(n.strokes.iter()) {
            if let figcraft_color::PaintKind::Image(img) = &p.kind
                && let Some(d) = doc.images.get(&img.image)
            {
                images.insert(img.image.clone(), d.clone());
            }
        }
    }
}

/// Whether the command names its layers (`ids` / `id`): it then acts on them even in vector edit mode.
fn names_layers(p: &Value) -> bool {
    get(p, "ids").is_some() || get(p, "id").is_some()
}

pub(super) fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    // In vector edit mode with points or segments selected, Copy takes just those (#1875).
    if !names_layers(p)
        && let Some(sel) = vector_clipboard::edit_selection(s)
        && sel.any
    {
        // Only lone points selected: nothing to copy, and not the whole layer either.
        return vector_clipboard::copy_selection(s, "edit.copy", sel);
    }
    let ids = req_targets("edit.copy", s, p)?;
    copy_to_clipboard(s, "edit.copy", &ids)?;
    Ok(json!({"copied": s.clipboard.nodes.len()}))
}

pub(super) fn cut(s: &mut Session, p: &Value) -> Result<Value> {
    // In vector edit mode with points or segments selected, Cut takes just those and stays in edit mode (#1875).
    if !names_layers(p)
        && let Some(sel) = vector_clipboard::edit_selection(s)
        && sel.any
    {
        // Lone points with nothing to copy: `cut_selection` refuses, as Copy does.
        return vector_clipboard::cut_selection(s, sel);
    }
    let ids = req_targets("edit.cut", s, p)?;
    copy_to_clipboard(s, "edit.cut", &ids)?;
    let doc = s.doc()?;
    let layers: Vec<NodeId> = ids.into_iter().filter(|i| !doc.node(*i).is_some_and(Node::is_page)).collect();
    delete(s, &json!({"ids": layers}))
}

/// Paste the clipboard under `parent` with the copied top-left moved to `at` (page coords).
pub fn paste_into(s: &mut Session, parent: NodeId, at: Option<(f64, f64)>) -> Result<Vec<NodeId>> {
    let clip = s.clipboard.clone();
    if clip.nodes.is_empty() {
        return Err(EngineError::Disabled("edit.paste".into(), "the clipboard is empty".into()));
    }
    // From another file: its instances follow the mains this file can have.
    let foreign = clip.source.as_ref().filter(|_| s.ds().is_ok_and(|d| d.uid != clip.source_doc));
    let copy_local = s.ds()?.uid == clip.source_doc;
    let doc = s.doc_mut()?;
    for (k, v) in &clip.images {
        doc.images.entry(k.clone()).or_insert_with(|| v.clone());
    }
    // Sections go on the page (or in sections), never in the frame being pasted into.
    let page = doc.page_of(parent).unwrap_or(parent);
    let shift = match (at, clip.bounds) {
        (Some((x, y)), Some(b)) => Affine::translate((x - b.x0, y - b.y0)),
        _ => Affine::IDENTITY,
    };
    let mut out = Vec::new();
    let mut all = BTreeMap::new();
    for (root, nodes) in &clip.nodes {
        if copy_local
            && clip.copied_components.contains(root)
            && let Some(n) = nodes.first().filter(|n| matches!(n.kind, NodeKind::Component { .. }))
            && doc.node(*root).is_some_and(|n| matches!(n.kind, NodeKind::Component { .. }))
        {
            let instance = crate::components::create_instance(doc, *root, parent, 0.0, 0.0)?;
            doc.get_mut(instance)?.transform = doc.world_transform(parent).inverse() * shift * n.transform;
            out.push(instance);
            continue;
        }
        let mut map = BTreeMap::new();
        for n in nodes {
            map.insert(n.id, doc.alloc_id());
        }
        let mut nodes = nodes.clone();
        let section = nodes.first().is_some_and(|r| matches!(r.kind, NodeKind::Section { .. }));
        let into = if section && !doc.takes_sections(parent) { page } else { parent };
        if let Some(r) = nodes.first_mut() {
            r.transform = doc.world_transform(into).inverse() * shift * r.transform;
        }
        out.push(doc.paste_nodes(&nodes, &map, *root, into, None)?);
        all.extend(map);
    }
    // Board connectors pasted with what they join join the pasted copies.
    let originals: Vec<(NodeId, figcraft_doc::BoardItem)> =
        clip.nodes.iter().flat_map(|(_, nodes)| nodes).filter_map(|n| Some((*all.get(&n.id)?, n.board.clone()?))).collect();
    crate::board::rejoin(doc, &originals, &all, !copy_local);
    if let Some(src) = foreign {
        crate::library::adopt_pasted(doc, src, clip.source_path.as_deref(), &all)?;
    }
    crate::library::count_inserted(doc, &out);
    Ok(out)
}

pub(super) fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    // While editing a vector, the copied layers join its network (#1875); `parent` asks for layers.
    if id(p, "parent").is_none() && s.ds()?.vector_edit.is_some() {
        return vector_clipboard::paste_into_vector(s, p);
    }
    let page = s.page()?;
    // Figma pastes into the selected frame (if one is selected), at the same position when it fits,
    // else in the frame's centre; otherwise in place on the page (or the viewport centre).
    let sel = s.selection();
    let doc = s.doc()?;
    let copy_local = s.ds()?.uid == s.clipboard.source_doc;
    let target = id(p, "parent").or_else(|| {
        sel.first()
            .copied()
            .filter(|i| {
                sel.len() == 1
                    && doc.node(*i).is_some_and(|n| n.kind.is_frame_like() && !matches!(n.kind, NodeKind::Instance { .. }) && !n.is_board_atom())
            })
            .filter(|i| !copy_local || !s.clipboard.copied_components.iter().any(|root| doc.is_ancestor(*root, *i)))
    });
    // One layer selected that isn't a frame target: the copy joins its parent, just above it (#2355).
    let sibling_of = if target.is_none() && id(p, "parent").is_none() { sibling_anchor(s, &sel, copy_local) } else { None };
    let target = target.or_else(|| sibling_of.and_then(|i| doc.node(i).and_then(|n| n.parent)));
    let parent = target.unwrap_or(page);
    let at = match (num(p, "x"), num(p, "y")) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => {
            let b = s.clipboard.bounds;
            match (target, b) {
                (Some(t), Some(b)) => {
                    let tb = doc.world_bounds(t);
                    match tb {
                        Some(tb) if tb.contains(b.origin()) && tb.contains(figcraft_geom::Point::new(b.x1, b.y1)) => None,
                        Some(tb) => Some((tb.center().x - b.width() / 2.0, tb.center().y - b.height() / 2.0)),
                        None => None,
                    }
                }
                (None, Some(b)) => {
                    // Off-screen originals paste at the viewport centre.
                    let v = &s.ds()?.view;
                    let (ox, oy) = v.origin();
                    let vr = figcraft_geom::Rect::new(ox, oy, ox + v.vw / v.zoom.max(1e-9), oy + v.vh / v.zoom.max(1e-9));
                    if vr.intersect(b).is_zero_area() { Some((v.cx - b.width() / 2.0, v.cy - b.height() / 2.0)) } else { None }
                }
                _ => None,
            }
        }
    };
    let ids = paste_into(s, parent, at)?;
    if let Some(sel) = sibling_of {
        let above = s.doc()?.index_in_parent(sel).map(|ix| ix + 1);
        move_above(s, &ids, parent, above)?;
    }
    s.set_selection(ids.clone());
    Ok(json!({"ids": ids}))
}

/// The one selected layer a plain paste goes next to: not a page-level layer, not inside an instance
/// (its children are fixed), not a board atom, a component set (a variant's parent) or inside a copied
/// component. Else the page, as before.
fn sibling_anchor(s: &Session, sel: &[NodeId], copy_local: bool) -> Option<NodeId> {
    let [only] = sel else { return None };
    let doc = s.doc().ok()?;
    let parent = doc.node(*only)?.parent?;
    let pn = doc.node(parent)?;
    let ok = !pn.is_page()
        && pn.kind.is_frame_like()
        && !pn.is_board_atom()
        && !crate::components::inside_instance(doc, *only)
        && !matches!(pn.kind, NodeKind::Instance { .. } | NodeKind::ComponentSet { .. })
        && (!copy_local || !s.clipboard.copied_components.iter().any(|root| doc.is_ancestor(*root, parent)));
    ok.then_some(*only)
}

/// Move the pasted copies that landed in `parent` to `above` and after, in the copied order.
fn move_above(s: &mut Session, ids: &[NodeId], parent: NodeId, above: Option<usize>) -> Result<()> {
    let Some(above) = above else { return Ok(()) };
    let doc = s.doc_mut()?;
    for (k, i) in ids.iter().filter(|i| doc.node(**i).is_some_and(|n| n.parent == Some(parent))).copied().collect::<Vec<_>>().into_iter().enumerate()
    {
        doc.reorder(i, above + k)?;
    }
    Ok(())
}

pub(super) fn paste_over(s: &mut Session, _p: &Value) -> Result<Value> {
    let sel = s.selection();
    let page = s.page()?;
    let doc = s.doc()?;
    let (parent, at, above) = match sel.first() {
        Some(i) => {
            let parent = doc.node(*i).and_then(|n| n.parent).unwrap_or(page);
            (parent, doc.world_bounds(*i).map(|b| (b.x0, b.y0)), doc.index_in_parent(*i).map(|ix| ix + 1))
        }
        None => (page, None, None),
    };
    let ids = paste_into(s, parent, at)?;
    move_above(s, &ids, parent, above)?;
    s.set_selection(ids.clone());
    Ok(json!({"ids": ids}))
}

pub(super) fn paste_replace(s: &mut Session, p: &Value) -> Result<Value> {
    let targets = req_targets("edit.pasteReplace", s, p)?;
    let mut out = Vec::new();
    for t in targets {
        let doc = s.doc()?;
        let Some(n) = doc.node(t) else { continue };
        let parent = n.parent.ok_or_else(|| bad("edit.pasteReplace", "can't replace a page"))?;
        let idx = doc.index_in_parent(t);
        let at = doc.world_bounds(t).map(|b| (b.x0, b.y0));
        let ids = paste_into(s, parent, at)?;
        let doc = s.doc_mut()?;
        for (k, i) in ids.iter().enumerate() {
            if let Some(ix) = idx {
                let _ = doc.reorder(*i, ix + k);
            }
        }
        doc.remove(t)?;
        out.extend(ids);
    }
    s.set_selection(out.clone());
    Ok(json!({"ids": out}))
}

pub(super) fn copy_props_of(doc: &Document, id: NodeId) -> Value {
    let Some(n) = doc.node(id) else { return Value::Null };
    let mut v = json!({
        "fills": n.fills, "strokes": n.strokes, "strokeWeight": n.stroke.weight, "strokeAlign": n.stroke.align,
        "strokeSides": n.stroke.sides, "startCap": n.stroke.cap_start, "endCap": n.stroke.cap_end,
        "strokeJoin": n.stroke.join, "miterLimit": n.stroke.miter_limit, "dashes": n.stroke.dashes,
        "dashCap": n.stroke.dash_cap,
        "widthProfile": super::super::props::width_points_json(n.stroke.width_profile.as_ref()), "brush": n.stroke.brush,
        "effects": n.effects, "opacity": n.opacity, "blend": n.blend,
    });
    // A vector rounds each anchor on its own (`vector.edit.cornerRadius` keeps the radii in the
    // network's vertices, its layer corners stay empty): there is no style radius to carry, and
    // pasting the empty one would flatten every anchor's radius.
    if !matches!(n.kind, NodeKind::Vector { .. }) {
        v["cornerRadius"] = json!(n.corners.radii);
        v["cornerSmoothing"] = json!(n.corners.smoothing);
    }
    if let NodeKind::Text(t) = &n.kind {
        v["fontFamily"] = json!(t.style.font_family);
        v["fontStyle"] = json!(t.style.font_style);
        v["fontSize"] = json!(t.style.font_size);
        v["fontVariations"] = json!(t.style.variations);
        v["lineHeight"] = serde_json::to_value(t.style.line_height).unwrap_or(Value::Null);
        v["letterSpacing"] = serde_json::to_value(t.style.letter_spacing).unwrap_or(Value::Null);
    }
    v
}

pub(super) fn copy_props(s: &mut Session, p: &Value) -> Result<Value> {
    let id = targets(s, p).first().copied().ok_or_else(|| bad("edit.copyProperties", "select one layer"))?;
    let v = copy_props_of(s.doc()?, id);
    s.clipboard.props = Some(v.clone());
    Ok(v)
}

/// Give layer `id` the properties [`copy_props_of`] copied (`props`), those it can take: the text
/// properties only to text, the corners only to a layer with corners.
pub(crate) fn apply_copied_props(cmd: &str, doc: &mut Document, id: NodeId, props: &Value) -> Result<()> {
    let mut m = props.as_object().cloned().unwrap_or_default();
    let is_text = matches!(doc.get(id)?.kind, NodeKind::Text(_));
    if !is_text {
        m.retain(|k, _| !k.starts_with("font") && k != "lineHeight" && k != "letterSpacing");
    }
    if !doc.get(id)?.kind.has_corner_radius() {
        m.remove("cornerRadius");
        m.remove("cornerSmoothing");
    }
    crate::cmd::props::apply(cmd, doc, id, &m)
}

pub(super) fn paste_props(s: &mut Session, p: &Value) -> Result<Value> {
    let props = s.clipboard.props.clone().ok_or_else(|| EngineError::Disabled("edit.pasteProperties".into(), "no copied properties".into()))?;
    let ids = req_targets("edit.pasteProperties", s, p)?;
    let doc = s.doc_mut()?;
    for i in ids {
        apply_copied_props("edit.pasteProperties", doc, i, &props)?;
    }
    ok()
}
