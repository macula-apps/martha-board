//! Design panel extras for frames and sections: section colours, hiding a section's contents and
//! resize to fit.

use egui::{CornerRadius, Sense, Stroke, Ui, vec2};
use figcraft_doc::{Document, NodeId, NodeKind};
use serde_json::json;

use crate::FigApp;
use crate::i18n::{tr, tr_noop};
use crate::right::{common, row};
use crate::widgets;

/// Section fill presets: light tints that keep dark text readable.
const SECTION_COLORS: [(&str, &str); 8] = [
    (tr_noop("White"), "#FFFFFF"),
    (tr_noop("Gray"), "#EDEDED"),
    (tr_noop("Red"), "#FFE0DE"),
    (tr_noop("Orange"), "#FFE8D1"),
    (tr_noop("Yellow"), "#FFF4C2"),
    (tr_noop("Green"), "#DDF5E3"),
    (tr_noop("Blue"), "#DCEBFF"),
    (tr_noop("Violet"), "#EBE3FF"),
];

fn is_section(doc: &Document, id: NodeId) -> bool {
    doc.node(id).is_some_and(|n| matches!(n.kind, NodeKind::Section { .. }))
}

/// Whether `id` gets these controls: a section, or a frame at the top of a page or section.
fn takes_controls(doc: &Document, id: NodeId) -> bool {
    doc.node(id).is_some_and(|n| {
        is_section(doc, id)
            || (n.kind.is_frame_like()
                && !matches!(n.kind, NodeKind::Instance { .. })
                && n.parent.is_some_and(|p| doc.node(p).is_some_and(|p| p.is_page()) || is_section(doc, p)))
    })
}

/// Draw section colour and hide-contents controls and Resize to fit; returns for an empty
/// selection or one that isn't all sections and top-level frames.
pub fn section(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let sections = !sel.is_empty() && sel.iter().all(|i| is_section(doc, *i));
    if sel.is_empty() || !sel.iter().all(|i| takes_controls(doc, *i)) {
        return;
    }
    let t = crate::theme::Tokens::get(ui.ctx());
    let ids = sel.to_vec();
    let single_frame = sel.len() == 1 && sel.first().and_then(|id| doc.node(*id)).is_some_and(|n| matches!(n.kind, NodeKind::Frame));
    let col = widgets::col_width(ui.available_width());
    if sections {
        widgets::sublabels(ui, tr("Section color"), None);
        row(ui, 32.0, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let current = common(doc, sel, |n| n.fills.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()));
            for (name, hex) in SECTION_COLORS {
                let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let c = figcraft_color::Color::from_hex(hex).unwrap_or(figcraft_color::Color::WHITE);
                ui.painter().rect_filled(r, CornerRadius::same(9), crate::theme::doc_c32(ui.ctx(), c));
                let on = current.as_ref().and_then(|c| c.as_ref()).is_some_and(|h| h.eq_ignore_ascii_case(hex.trim_start_matches('#')));
                let stroke = if on { Stroke::new(2.0, crate::theme::ACCENT) } else { Stroke::new(1.0, t.border) };
                ui.painter().rect_stroke(r, CornerRadius::same(9), stroke, egui::StrokeKind::Inside);
                let name = tr(name);
                crate::a11y::radio(&resp, name, on);
                if resp.on_hover_text(name).clicked() {
                    app.set_props(&ids, json!({"fills": [hex]}));
                }
            }
        });
        row(ui, 28.0, |ui| {
            let hidden = crate::selection_cache::every(doc, sel, |n| n.hides_contents());
            if widgets::checkbox(ui, hidden, tr("Hide contents")) {
                app.run("section.toggleContents", json!({"ids": ids, "hidden": !hidden}));
            }
        });
    }
    // A single frame has the same action beside its type in the selection header.
    if !single_frame {
        row(ui, 32.0, |ui| {
            let fit = ui
                .add_sized(vec2(col * 2.0 + 8.0, widgets::FIELD_H), egui::Button::new(tr("Resize to fit")))
                .on_hover_text(crate::shortcuts::tip(tr("Fit to contents"), "object.resizeToFit"));
            if fit.clicked() {
                app.run("object.resizeToFit", json!({"ids": ids}));
            }
        });
    }
    ui.add_space(4.0);
    widgets::separator(ui);
}
