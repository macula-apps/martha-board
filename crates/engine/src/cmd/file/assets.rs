//! The assets `file.exportLayers {assets: true}` writes: the layers in and under a selection to
//! hand over as files (those with export settings, icons and images).

use figcraft_color::PaintKind;
use figcraft_doc::{Document, ExportFormat, ExportSetting, Node, NodeId, NodeKind};

/// Why a layer is listed as an asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// It has export settings.
    Export,
    /// A small layer made only of vector shapes (exported as SVG).
    Icon,
    /// A layer filled with an image (exported as PNG).
    Image,
}

/// A layer in the selection to hand over as a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub id: NodeId,
    pub kind: AssetKind,
    /// How it exports: its export settings, or SVG for an icon and PNG for an image.
    pub settings: Vec<ExportSetting>,
}

/// Most assets listed for one selection.
const MAX_ASSETS: usize = 200;
/// Largest side of a layer listed as an icon.
const ICON_MAX: f64 = 128.0;

/// Whether `id` (visible parts only) is made of vector shapes alone: paths, booleans, stars,
/// polygons and lines, with rectangles and ellipses among them, in groups or frames. `paths` is
/// set when one of the former is in it.
fn vector_only(doc: &Document, id: NodeId, depth: usize, paths: &mut bool) -> bool {
    let Some(n) = doc.node(id) else { return false };
    if !n.visible {
        return true;
    }
    if n.fills.iter().chain(&n.strokes).any(|p| matches!(p.kind, PaintKind::Image(_) | PaintKind::Pattern(_))) {
        return false;
    }
    match n.kind {
        NodeKind::Vector { .. } | NodeKind::BooleanOp { .. } | NodeKind::Star { .. } | NodeKind::Polygon { .. } | NodeKind::Line => {
            *paths = true;
            true
        }
        NodeKind::Rectangle | NodeKind::Ellipse { .. } => true,
        NodeKind::Group | NodeKind::Frame | NodeKind::Component { .. } | NodeKind::Instance { .. } => {
            depth < 16 && !n.children.is_empty() && n.children.iter().all(|c| vector_only(doc, *c, depth + 1, paths))
        }
        _ => false,
    }
}

/// Why `n` is an asset, if it is one.
fn asset_kind(doc: &Document, n: &Node) -> Option<AssetKind> {
    let mut paths = false;
    if !n.exports.is_empty() {
        Some(AssetKind::Export)
    } else if n.width <= ICON_MAX && n.height <= ICON_MAX && vector_only(doc, n.id, 0, &mut paths) && paths {
        Some(AssetKind::Icon)
    } else if n.fills.iter().any(|p| p.visible && matches!(p.kind, PaintKind::Image(_))) {
        Some(AssetKind::Image)
    } else {
        None
    }
}

/// The assets in and under `ids`, in layer order: layers with export settings, icons and images
/// (an icon's own shapes aren't listed again).
pub fn assets(doc: &Document, ids: &[NodeId]) -> Vec<Asset> {
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = ids.iter().rev().copied().collect();
    // Bounded: a page can hold very many layers.
    let mut visited = 0usize;
    while let Some(id) = stack.pop() {
        visited += 1;
        if out.len() >= MAX_ASSETS || visited > 20_000 {
            break;
        }
        let Some(n) = doc.node(id) else { continue };
        if !n.visible || n.is_page() {
            if n.is_page() {
                stack.extend(n.children.iter().rev().copied());
            }
            continue;
        }
        let kind = asset_kind(doc, n);
        if let Some(kind) = kind
            && !out.iter().any(|a: &Asset| a.id == id)
        {
            let settings = match kind {
                AssetKind::Export => n.exports.clone(),
                AssetKind::Icon => vec![ExportSetting { format: ExportFormat::Svg, ..Default::default() }],
                AssetKind::Image => vec![ExportSetting::default()],
            };
            out.push(Asset { id, kind, settings });
        }
        if kind != Some(AssetKind::Icon) {
            stack.extend(n.children.iter().rev().copied());
        }
    }
    out
}

/// The files exporting the assets in and under `ids` makes (see [`assets`]).
pub fn asset_jobs(doc: &Document, ids: &[NodeId]) -> Vec<super::ExportJob> {
    super::jobs_for(assets(doc, ids).into_iter().map(|a| (a.id, a.settings)), doc)
}
