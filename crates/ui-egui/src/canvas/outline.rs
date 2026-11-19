use super::{Arc, Color32, Document, FigApp, NodeId, NodeKind, Point, Pos2, Shape, Stroke, vec2};

pub(super) fn stroke(c: Color32, w: f32) -> Stroke {
    Stroke::new(w, c)
}

/// A text layer's layout placed on the canvas: where carets and selected characters show (yours
/// and other people's in a live session).
pub(crate) struct TextOnScreen {
    pub layout: Arc<figcraft_text::TextLayout>,
    /// How many characters the text has.
    pub chars: usize,
    /// The layout's vertical offset in the box (vertical alignment).
    dy: f64,
    world: figcraft_geom::Affine,
}

impl TextOnScreen {
    /// Text layer `id` of `doc` (`None`: not a text layer).
    pub(crate) fn of(doc: &Document, id: NodeId) -> Option<Self> {
        let n = doc.node(id)?;
        let NodeKind::Text(t) = &n.kind else { return None };
        let (layout, dy) = figcraft_text::layout_node(n)?;
        Some(Self { layout, chars: t.char_count(), dy, world: doc.world_transform(id) })
    }

    /// Layout point `(x, y)` on screen (on the path, for text on a path).
    pub(crate) fn at(&self, app: &FigApp, x: f64, y: f64) -> Pos2 {
        let q = self.layout.warp_point(x, y);
        let p = self.world * Point::new(q.x, q.y + self.dy);
        app.to_screen(p.x, p.y)
    }

    /// The highlight of characters `a..b`, one screen quad per line.
    pub(crate) fn selection(&self, app: &FigApp, a: usize, b: usize) -> Vec<[Pos2; 4]> {
        let rects = self.layout.selection_rects(a, b);
        rects.iter().map(|r| [self.at(app, r.x0, r.y0), self.at(app, r.x1, r.y0), self.at(app, r.x1, r.y1), self.at(app, r.x0, r.y1)]).collect()
    }

    /// The caret before character `index`: its top and bottom on screen (an empty layer: a line
    /// of default height at its origin).
    pub(crate) fn caret(&self, app: &FigApp, index: usize, zoom: f64) -> [Pos2; 2] {
        let (cx, li) = self.layout.caret(index);
        match self.layout.lines.get(li).or(self.layout.lines.last()) {
            Some(line) => [self.at(app, cx, line.top), self.at(app, cx, line.top + line.height)],
            None => {
                let p = self.at(app, 0.0, 0.0);
                [p, p + vec2(0.0, 14.0 * zoom as f32)]
            }
        }
    }
}

/// A node's oriented outline in screen space.
pub(crate) fn node_quad(app: &FigApp, doc: &Document, id: NodeId) -> Option<[Pos2; 4]> {
    let n = doc.node(id)?;
    let w = doc.world_transform(id);
    let corners = [Point::new(0.0, 0.0), Point::new(n.width, 0.0), Point::new(n.width, n.height), Point::new(0.0, n.height)];
    Some(corners.map(|c| {
        let p = w * c;
        app.to_screen(p.x, p.y)
    }))
}

pub(super) fn quad(painter: &egui::Painter, q: [Pos2; 4], s: Stroke) {
    painter.add(Shape::closed_line(q.to_vec(), s));
}

/// The path a layer's hover and selection outline follows, in its own space, when that isn't just
/// its box: the shape of ellipses, vectors, polygons, stars and boolean groups, and of rectangles
/// and frames with rounded corners (each corner's radius and the smoothing, as the fill draws them,
/// whatever the stroke's alignment).
pub(super) fn outline_path(doc: &Document, n: &figcraft_doc::Node) -> Option<figcraft_geom::kurbo::BezPath> {
    let shaped = match n.kind {
        NodeKind::Ellipse { .. } | NodeKind::Vector { .. } | NodeKind::Polygon { .. } | NodeKind::Star { .. } | NodeKind::BooleanOp { .. } => true,
        NodeKind::Rectangle
        | NodeKind::Frame
        | NodeKind::Component { .. }
        | NodeKind::ComponentSet { .. }
        | NodeKind::Instance { .. }
        | NodeKind::Section { .. } => n.corners.radii.iter().any(|r| *r > 0.0),
        _ => false,
    };
    if !shaped {
        return None;
    }
    figcraft_render::node_outline(doc, n).map(|(p, _)| p)
}

/// [`outline_path`] on screen, flattened into polylines.
fn outline_shape(app: &FigApp, doc: &Document, id: NodeId) -> Option<Vec<Vec<Pos2>>> {
    let n = doc.node(id)?;
    let path = outline_path(doc, n)?;
    let w = doc.world_transform(id);
    let mut out = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    figcraft_geom::kurbo::flatten(path.iter(), 0.5 / app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0), |el| {
        use figcraft_geom::kurbo::PathEl::*;
        match el {
            MoveTo(p) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                let q = w * p;
                cur.push(app.to_screen(q.x, q.y));
            }
            LineTo(p) => {
                let q = w * p;
                cur.push(app.to_screen(q.x, q.y));
            }
            ClosePath => {
                if let Some(f) = cur.first().copied() {
                    cur.push(f);
                }
            }
            _ => {}
        }
    });
    if cur.len() > 1 {
        out.push(cur);
    }
    Some(out)
}

/// A layer's outline on the canvas: its shape where [`outline_path`] has one, its box otherwise.
pub(super) fn draw_outline(app: &FigApp, painter: &egui::Painter, doc: &Document, id: NodeId, s: Stroke) {
    painter.extend(outline_shapes(app, doc, id, s));
}

/// The shapes [`draw_outline`] draws.
pub(super) fn outline_shapes(app: &FigApp, doc: &Document, id: NodeId, s: Stroke) -> Vec<Shape> {
    match outline_shape(app, doc, id) {
        Some(lines) => lines.into_iter().map(|l| Shape::line(l, s)).collect(),
        None => node_quad(app, doc, id).map(|q| Shape::closed_line(q.to_vec(), s)).into_iter().collect(),
    }
}
