//! Layer thumbnails: a layer rendered to fit a square of a given side, for the export preview and
//! the compare view.

use figcraft_doc::{Document, NodeId, NodeKind};

pub(crate) fn thumbnail(ctx: &egui::Context, doc: &Document, id: NodeId, px: f32) -> Option<egui::TextureHandle> {
    let pix = thumbnail_pixmap(doc, id, px)?;
    let img = crate::render_worker::screen_image(&pix, doc.color_profile);
    Some(ctx.load_texture(format!("asset-thumb-{}", id.0), img, egui::TextureOptions::LINEAR))
}

/// The pixels a thumbnail shows. A slice paints nothing itself, so it shows the content under it
/// that exporting it with its first export setting writes (`figcraft_engine::cmd::file::drawn_layers`):
/// without this a slice's preview is a blank checkerboard next to a correct export.
pub(crate) fn thumbnail_pixmap(doc: &Document, id: NodeId, px: f32) -> Option<figcraft_render::Pixmap> {
    let slice = doc.node(id).filter(|n| matches!(n.kind, NodeKind::Slice));
    let b = if slice.is_some() { doc.world_bounds(id)? } else { figcraft_render::export_bounds(doc, id)? };
    let side = b.width().max(b.height());
    if !(side.is_finite() && side > 0.0) {
        return None;
    }
    let scale = (f64::from(px) / side).min(2.0);
    if let Some(n) = slice {
        let setting = n.exports.first().cloned().unwrap_or_default();
        let layers = figcraft_engine::cmd::file::drawn_layers(doc, &[id], &setting);
        return figcraft_render::render_area(doc, &layers, b, scale);
    }
    figcraft_render::render_node(doc, id, scale)
}
