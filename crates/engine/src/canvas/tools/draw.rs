use super::handles::SNAP_PX;
use super::{
    Affine, Cursor, Document, Gesture, Node, NodeId, NodeKind, Point, PointerEvent, Rect, Result, Session, StrokeCap, Targets, ToolId, Vec2,
    frame_at, textedit,
};

/// A frame or section drawn around layers takes those of its siblings that lie fully inside it
/// (visible and unlocked ones, in their order; a frame never takes a section). Not in auto layout,
/// where the new frame is one more item of the flow.
fn take_enclosed(doc: &mut Document, drawn: NodeId) -> Result<()> {
    let Some(n) = doc.node(drawn) else { return Ok(()) };
    let section = matches!(n.kind, NodeKind::Section { .. });
    let (Some(parent), Some(area)) = (n.parent.and_then(|p| doc.node(p)), doc.world_bounds(drawn)) else { return Ok(()) };
    if parent.layout.is_some() {
        return Ok(());
    }
    let inside: Vec<NodeId> = parent
        .children
        .iter()
        .copied()
        .filter(|c| *c != drawn && doc.node(*c).is_some_and(|k| k.visible && !k.locked && (section || !matches!(k.kind, NodeKind::Section { .. }))))
        .filter(|c| doc.world_bounds(*c).is_some_and(|b| area.contains(b.origin()) && area.contains(Point::new(b.x1, b.y1))))
        .collect();
    for c in inside {
        doc.reparent(c, drawn, None)?;
    }
    Ok(())
}

impl Session {
    pub(super) fn draw_down(&mut self, ev: &PointerEvent) -> Result<()> {
        let ds = self.ds()?;
        let doc = ds.doc.clone();
        let page = ds.page;
        if ds.text_edit.is_some() {
            textedit::end(self)?;
        }
        // Frames drawn inside frames nest; sections only at the top or in sections.
        let parent = match self.tool.tool {
            ToolId::Section => {
                frame_at(&doc, page, ev.p(), &[]).filter(|f| doc.node(*f).is_some_and(|n| matches!(n.kind, NodeKind::Section { .. }))).unwrap_or(page)
            }
            _ => frame_at(&doc, page, ev.p(), &[]).unwrap_or(page),
        };
        let targets = Targets::gather(&doc, page, &[]);
        self.begin_interaction(self.tool.tool.label())?;
        self.tool.gesture = Gesture::Draw { start: ev.p(), id: None, parent, targets };
        Ok(())
    }

    pub(super) fn draw_drag(&mut self, ev: &PointerEvent, start: Point, id: Option<NodeId>, parent: NodeId, targets: &Targets) -> Result<()> {
        let tool = self.tool.tool;
        let mut p = ev.p();
        let mut guides = Vec::new();
        let pixel = self.ds().map(|d| d.view.snap_pixel).unwrap_or(true);
        // The pixel grid first: snapping to layers comes after, so it wins on the axes it snaps.
        if pixel {
            p = Point::new(p.x.round(), p.y.round());
        }
        if self.ds().map(|d| d.view.snap_objects).unwrap_or(true) && !ev.cmd && !ev.ctrl {
            let (dx, dy, g) = targets.snap_point(p.x, p.y, SNAP_PX / ev.zoom.max(1e-6));
            p.x += dx;
            p.y += dy;
            guides = g;
        }
        let mut created = id;
        let page = self.page()?;
        // Edit ▸ Set default properties: what new shapes take (not frames, sections, slices, lines
        // or arrows, which keep their own look).
        let defaults = if matches!(tool, ToolId::Rectangle | ToolId::Ellipse | ToolId::Polygon | ToolId::Star) {
            self.default_props.shape.clone()
        } else {
            None
        };
        self.preview(|doc, sel| {
            let (mut a, b) = (start, p);
            if pixel {
                a = Point::new(a.x.round(), a.y.round());
            }
            let pw = doc.world_transform(parent);
            let (kind, base) = match tool {
                ToolId::Frame => (NodeKind::Frame, "Frame"),
                ToolId::Section => (NodeKind::Section { contents_hidden: false }, "Section"),
                ToolId::Slice => (NodeKind::Slice, "Slice"),
                ToolId::Rectangle => (NodeKind::Rectangle, "Rectangle"),
                ToolId::Ellipse => (NodeKind::Ellipse { start: 0.0, sweep: std::f64::consts::TAU, inner: 0.0 }, "Ellipse"),
                ToolId::Polygon => (NodeKind::Polygon { count: 3 }, "Polygon"),
                ToolId::Star => (NodeKind::Star { count: 5, inner_ratio: 0.38 }, "Star"),
                ToolId::Line => (NodeKind::Line, "Line"),
                ToolId::Arrow => (NodeKind::Line, "Arrow"),
                _ => return Ok(()),
            };
            let name = match tool {
                ToolId::Frame | ToolId::Section | ToolId::Slice => doc.next_name(base),
                _ => base.to_string(),
            };
            let nid = doc.alloc_id();
            let mut n = if tool == ToolId::Section { Node::new_section(nid, name, 0.0, 0.0) } else { Node::new(nid, kind, name, 0.0, 0.0) };
            if tool == ToolId::Arrow {
                n.stroke.cap_end = StrokeCap::LineArrow;
            }
            if matches!(tool, ToolId::Line | ToolId::Arrow) {
                let mut v = b - a;
                if ev.shift {
                    let ang = (v.atan2() / (std::f64::consts::PI / 4.0)).round() * std::f64::consts::PI / 4.0;
                    v = Vec2::from_angle(ang) * v.hypot();
                }
                // Alt draws from the centre, as for the other shapes.
                let (a, v) = if ev.alt { (a - v, v * 2.0) } else { (a, v) };
                n.width = v.hypot();
                n.transform = pw.inverse() * Affine::translate(a.to_vec2()) * Affine::rotate(v.atan2());
            } else {
                let mut d = b - a;
                if ev.shift {
                    let m = d.x.abs().max(d.y.abs());
                    d = Vec2::new(m * d.x.signum(), m * d.y.signum());
                }
                let (x0, y0, x1, y1) = if ev.alt { (a.x - d.x, a.y - d.y, a.x + d.x, a.y + d.y) } else { (a.x, a.y, a.x + d.x, a.y + d.y) };
                let r = Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
                n.width = r.width();
                n.height = r.height();
                n.transform = pw.inverse() * Affine::translate((r.x0, r.y0));
            }
            doc.insert(n, parent, None)?;
            if let Some(d) = &defaults {
                // Best effort: a default the shape can't take leaves it as drawn.
                let _ = crate::cmd::edit::apply_copied_props("edit.setDefaultProperties", doc, nid, d);
            }
            *sel = vec![nid];
            created = Some(nid);
            let _ = page;
            Ok(())
        })?;
        self.tool.overlay.guides = guides;
        self.tool.gesture = Gesture::Draw { start, id: created, parent, targets: targets.clone() };
        Ok(())
    }

    pub(super) fn up(&mut self, ev: &PointerEvent) -> Result<()> {
        // Alt went down or up since the last move: the drop is a copy or a move as the release has it.
        if matches!(&self.tool.gesture, Gesture::Move { moved: true, duplicate, .. } if *duplicate != ev.alt) {
            self.drag(ev)?;
        }
        let g = std::mem::take(&mut self.tool.gesture);
        self.tool.overlay.marquee = None;
        self.tool.overlay.guides.clear();
        self.tool.overlay.scale = None;
        self.tool.overlay.arc = None;
        self.tool.overlay.shape = None;
        self.tool.overlay.measure_box = None;
        self.tool.overlay.insert_line = None;
        self.tool.overlay.drop_cell = None;
        self.tool.overlay.stamp_target = None;
        let drop_target = self.tool.overlay.drop_target.take();
        match g {
            Gesture::Move { ids, moved, duplicate, deselect, keep, .. } => {
                if moved {
                    // A free move lands in the frame under the pointer (or the page) where it was
                    // dropped; layers in an auto layout frame already sit in their slot.
                    let page = self.page()?;
                    let sel = self.selection();
                    let sources = if duplicate { ids.clone() } else { Vec::new() };
                    let moving = if duplicate { sel } else { ids };
                    let doc = self.doc()?.clone();
                    let in_al =
                        moving.iter().any(|m| doc.node(*m).and_then(|n| n.parent).and_then(|p| doc.node(p)).is_some_and(|p| p.layout.is_some()));
                    let target = drop_target.unwrap_or(page);
                    let current = moving.first().and_then(|m| doc.node(*m)).and_then(|n| n.parent);
                    if !in_al && current != Some(target) {
                        let ds = self.ds_mut()?;
                        let before = ds.doc.clone();
                        let d = ds.doc_mut();
                        for m in &moving {
                            // Group, boolean group and instance layers stay put: they only move.
                            let from = d.node(*m).and_then(|n| n.parent);
                            if from.is_some_and(|f| crate::drop::keeps_layers(d, f)) {
                                continue;
                            }
                            // A layer can't go into itself; it stays where it is.
                            let _ = d.reparent(*m, target, None);
                        }
                        crate::normalize(ds.doc_mut(), &before);
                    }
                    self.commit_interaction()?;
                    // Duplicate repeats a free Alt-drag's step (an auto layout places its own).
                    if duplicate && !in_al && moving.len() == sources.len() {
                        self.ds_mut()?.last_duplicate = Some(crate::cmd::edit::Duplicated { sources, copies: moving });
                    }
                } else {
                    self.cancel_interaction()?;
                    if let Some(d) = deselect {
                        let sel = self.selection().into_iter().filter(|x| *x != d).collect();
                        self.set_selection(sel);
                    } else if let Some(k) = keep {
                        // A click (not a drag) on one layer of a multi-selection, or on a layer
                        // inside a selected one, selects just it.
                        let doc = self.doc()?.clone();
                        let sel = self.selection();
                        if sel.as_slice() != [k] && (sel.contains(&k) || sel.iter().any(|s| doc.is_ancestor(*s, k))) {
                            self.set_selection(vec![k]);
                        }
                    }
                }
            }
            Gesture::Resize { .. } | Gesture::Rotate { .. } | Gesture::Arc { .. } | Gesture::Shape { .. } => {
                self.commit_interaction()?;
            }
            Gesture::Draw { start, id, parent, .. } => {
                let tiny = (ev.p() - start).hypot() * ev.zoom < 3.0;
                if tiny || id.is_none() {
                    // Click without drag: Figma makes a 100×100 shape (frames 100×100 too).
                    self.cancel_interaction()?;
                    self.begin_interaction(self.tool.tool.label())?;
                    let (w, h) = self.tool.tool.click_size();
                    let target = start + Vec2::new(w, h);
                    let mut fake = ev.clone();
                    fake.x = target.x;
                    fake.y = target.y;
                    fake.shift = false;
                    fake.alt = false;
                    self.tool.gesture = Gesture::None;
                    let targets = Targets::default();
                    self.draw_drag(&fake, start, None, parent, &targets)?;
                    self.tool.gesture = Gesture::None;
                }
                if matches!(self.tool.tool, ToolId::Frame | ToolId::Section)
                    && let Some(drawn) = self.selection().first().copied()
                {
                    let ds = self.ds_mut()?;
                    let before = ds.doc.clone();
                    take_enclosed(ds.doc_mut(), drawn)?;
                    crate::normalize(ds.doc_mut(), &before);
                }
                self.commit_interaction()?;
                // A placement by the pointer takes the placement crosshair with it, as Enter does.
                self.tool.place_at = None;
                if !self.prefs.keep_tool {
                    self.set_tool(ToolId::Move);
                }
            }
            Gesture::Pan { .. } => {
                self.tool.overlay.cursor = Cursor::Grab;
                if let Some(t) = self.tool.temp_from.take() {
                    self.tool.tool = t;
                }
            }
            Gesture::Pencil { mut points } => {
                self.tool.overlay.pen.clear();
                // The stroke ends where the button came up, even without a move to get there.
                if points.last().is_none_or(|l| (*l - ev.p()).hypot() > 0.5 / ev.zoom.max(1e-6)) {
                    points.push(ev.p());
                }
                self.ink_up(&points)?;
            }
            Gesture::Connect { from, start } => self.connect_up(ev, from, start)?,
            Gesture::Erase { .. } => self.erase_up()?,
            Gesture::Reattach { id, from, start } => self.reattach_up(ev, id, from, start)?,
            Gesture::Reshape { id, portion, start } => self.reshape_up(ev, id, portion, start)?,
            Gesture::TextSelect { .. } | Gesture::TextBox { .. } | Gesture::Marquee { .. } | Gesture::None => {}
            Gesture::Origin => {}
        }
        if let Ok(ds) = self.ds_mut() {
            ds.revision += 1;
        }
        Ok(())
    }

    /// A finished marker or highlighter stroke (page points): a curve fitted through the samples,
    /// kept from shooting past them where the stroke turns back or runs nearly straight (#1741).
    fn ink_up(&mut self, points: &[Point]) -> Result<()> {
        if points.len() < 2 {
            return Ok(());
        }
        let mut poly = figcraft_geom::BezPath::new();
        for (k, p) in points.iter().enumerate() {
            if k == 0 {
                poly.move_to(*p);
            } else {
                poly.line_to(*p);
            }
        }
        self.ink_finish(&figcraft_geom::simplify_path(&poly, 0.75).to_svg())
    }
}
