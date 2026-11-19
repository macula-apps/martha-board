//! The right panel: the Design panel.

use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use figcraft_color::{BlendMode, Paint, PaintKind};
use figcraft_doc::{
    Blur, BlurType, Brush, Document, Effect, EffectKind, Glass, GridAlign, GridKind, LayoutGrid, MAX_BLUR_RADIUS, MaskType, Node, NodeId, NodeKind,
    Noise, NoiseMode, Shadow, StrokeAlign, StrokeCap, StrokeJoin, StrokeProps, Texture, WidthPoint, WidthPreset, WidthProfile,
};
use figcraft_engine::cmd::lists::{moved_index, removed_index};
use serde_json::{Value, json};

use crate::FigApp;
use crate::a11y::{self, Tip};
use crate::i18n::{tr, trf, trn};
use crate::icons::{self, Icon};
use crate::list_drag::{ListDrag, move_flags, remove_flags};
use crate::style_picker::{self, StyleSlot};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{self, Edit, FIELD_H, FieldEdit, Lead, PAD};

mod effects;
mod menu;
mod paints;
mod stroke;

pub(crate) use effects::effect_open;
use effects::{effects, grids};
#[cfg(test)]
pub(crate) use menu::menu_row_width;
#[cfg(test)]
pub(crate) use menu::preview_blend;
pub use menu::{blend_menu, context_popup, menu_check, menu_item, menu_item_beside_checks, menu_item_with, menu_popup, menu_row};
pub(crate) use menu::{block, menu_style, row};
use paints::{appearance, paints};
pub(crate) use stroke::{CAP_ART_W, cap_art};

/// Common value of `f` across `ids` (None when mixed or empty).
pub(crate) fn common<T: PartialEq + Clone>(doc: &Document, ids: &[NodeId], f: impl Fn(&Node) -> T) -> Option<T> {
    crate::selection_cache::with_nodes(doc, ids, |it| {
        let mut it = it.flatten();
        let first = f(it.next()?);
        if it.all(|n| f(n) == first) { Some(first) } else { None }
    })
}

impl FigApp {
    /// Set properties on `ids` (one undo step).
    pub fn set_props(&mut self, ids: &[NodeId], props: Value) {
        if ids.is_empty() {
            return;
        }
        self.run("node.set", json!({"ids": ids, "props": props}));
    }

    /// Apply a field edit: scrubs are live previews committed as one step.
    pub fn field(&mut self, ids: &[NodeId], key: &str, edit: Option<FieldEdit>, map: impl Fn(f64) -> Value) {
        if ids.is_empty() {
            return;
        }
        self.field_run(edit, "node.set", |v| json!({"ids": ids, "props": { key: map(v) }}));
    }

    /// [`Self::field`] for a field showing `shown(layer)`: a relative edit of mixed values (`+10`,
    /// `*2`, an arrow key's step) goes to each layer from its own value, in one undo step.
    pub fn field_of(&mut self, ids: &[NodeId], key: &str, edit: Option<Edit>, shown: impl Fn(&Node) -> f64, map: impl Fn(f64) -> Value) {
        let rel = match edit {
            Some(Edit::Each(rel)) => rel,
            Some(Edit::All(e)) => return self.field(ids, key, Some(e), map),
            None => return,
        };
        let Ok(doc) = self.session.doc() else { return };
        let mut values: Vec<(NodeId, f64)> = ids.iter().filter_map(|i| Some((*i, rel.apply(shown(doc.node(*i)?))?))).collect();
        if key == "absoluteRotation" {
            // A page angle depends on the parents': turn a selected parent before the layers in it
            // (as `node.set` orders a multi-id call), whatever order the selection is in.
            values.sort_by_key(|(i, _)| doc.ancestors(*i).len());
        }
        if values.is_empty() || self.session.begin_interaction("Edit").is_err() {
            return;
        }
        for (id, v) in values {
            if let Err(e) = self.session.execute("node.set", &json!({"ids": [id], "props": { key: map(v) }})) {
                self.failed("node.set", &e);
            }
        }
        let _ = self.session.commit_interaction();
    }

    /// Apply a field edit through any command: `params` builds the command's params from the value.
    pub fn field_run(&mut self, edit: Option<FieldEdit>, cmd: &str, params: impl Fn(f64) -> Value) {
        self.field_run_named(edit, "Edit", cmd, params);
    }

    /// Apply a field edit with a specific undo label for scrubs; commit only on release.
    /// Typed edits use `run`; scrub errors are discarded as in `field_run`.
    pub(crate) fn field_run_named(&mut self, edit: Option<FieldEdit>, label: &str, cmd: &str, params: impl Fn(f64) -> Value) {
        match edit {
            Some(FieldEdit::Set(v)) => {
                self.run(cmd, params(v));
            }
            Some(FieldEdit::Scrub(v, done)) => {
                let _ = self.session.begin_interaction(label);
                let _ = self.session.execute(cmd, &params(v));
                if done {
                    let _ = self.session.commit_interaction();
                }
            }
            None => {}
        }
    }
}

/// Draw the right panel: people and share controls, the Design heading and zoom menu, then
/// route the scroll area to comments, frame presets or design.
pub fn right_panel(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    // The panel's edge, drawn here: the frame has no stroke (a stroke counts as margin and would
    // take 1 pt off each side of the panel (240 pt by default)).
    let edge = ui.max_rect();
    app.ui.right_panel_rect = Some(edge);
    // On the side facing the canvas (its right in a mirrored window).
    let x = if app.ui.mirrored { edge.max.x } else { edge.min.x };
    ui.painter().vline(x, edge.y_range(), Stroke::new(1.0, t.border));
    let w = ui.available_width();
    // Top bar: people (you and the others in a live session), share.
    let (top, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
    // As wide as its label needs (longer in some languages).
    let label = ui.painter().layout_no_wrap(tr("Share").to_string(), semibold(11.5), t.accent_on);
    let sw = (label.size().x + 24.0).max(56.0);
    let share = Rect::from_min_size(pos2(top.max.x - 14.0 - sw, top.center().y - 16.0), vec2(sw, 32.0));
    let sresp = ui.interact(share, ui.scope_id().with("share"), Sense::click()).tip(tr("Share: live session, export"));
    ui.painter().rect_filled(share, CornerRadius::same(6), if sresp.hovered() { t.accent_hover } else { t.accent });
    ui.painter().galley(share.center() - label.size() / 2.0, label, t.accent_on);
    crate::live::share_popover(app, &sresp);
    crate::live::people_bar(app, ui, Rect::from_min_max(pos2(top.min.x + 12.0, top.min.y), pos2(share.min.x - 6.0, top.max.y)));
    // Heading row: the panel's name, then the zoom menu.
    let (tabs, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    ui.painter().line_segment([tabs.left_top(), tabs.right_top()], Stroke::new(1.0, t.border));
    let galley = ui.painter().layout_no_wrap(tr("Design").into(), semibold(11.0), t.text);
    let r = Rect::from_min_size(pos2(tabs.min.x + 8.0, tabs.center().y - 12.0), vec2(galley.size().x + 16.0, 24.0));
    ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    ui.painter().galley(pos2(r.min.x + 8.0, r.center().y - galley.size().y / 2.0), galley, t.text);
    // Zoom menu.
    let zoom = app.session.ds().map(|d| d.view.zoom).unwrap_or(1.0);
    let zr = Rect::from_min_size(pos2(tabs.max.x - 64.0, tabs.center().y - 12.0), vec2(56.0, 24.0));
    let zresp = ui.interact(zr, ui.scope_id().with("zoommenu"), Sense::click());
    // The value is only spelled out while a screen reader reads the tree.
    let value = if a11y::active(&zresp.ctx) { format!("{}%", (zoom * 100.0).round()) } else { String::new() };
    a11y::combo(&zresp, tr("Zoom"), &value, a11y::menu_open(&zresp));
    if zresp.hovered() {
        ui.painter().rect_filled(zr, CornerRadius::same(5), t.hover);
    }
    ui.painter().text(pos2(zr.min.x + 6.0, zr.center().y), Align2::LEFT_CENTER, format!("{}%", (zoom * 100.0).round()), regular(11.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(zr.max.x - 9.0, zr.center().y), vec2(10.0, 10.0)), Icon::Chevron, t.icon_secondary);
    menu_popup(&zresp).width(200.0).show(|ui| {
        for (label, cmd) in [
            (tr("Zoom in"), "view.zoomIn"),
            (tr("Zoom out"), "view.zoomOut"),
            (tr("Zoom to fit"), "view.zoomFit"),
            (tr("Zoom to 50%"), "view.zoom50"),
            (tr("Zoom to 100%"), "view.zoom100"),
            (tr("Zoom to 200%"), "view.zoom200"),
            (tr("Zoom to selection"), "view.zoomSelection"),
        ] {
            if menu_item(ui, label, crate::shortcuts::shortcut(cmd).unwrap_or("")) {
                app.run(cmd, json!({}));
                ui.close();
            }
        }
        ui.separator();
        let Ok(v) = app.session.ds().map(|d| d.view.clone()) else { return };
        for (label, scale) in [(tr("Pixel preview 1x"), 1u8), (tr("Pixel preview 2x"), 2)] {
            let on = v.pixel_preview && v.pixel_scale == scale;
            if menu_check(ui, on, label, "") {
                app.run("view.pixelPreview", json!({"on": !on, "scale": scale}));
                ui.close();
            }
        }
        let toggles: [(&str, &str, bool); 5] = [
            (tr("Pixel grid"), "view.pixelGrid", v.pixel_grid),
            (tr("Snap to pixel grid"), "view.snapPixel", v.snap_pixel),
            (tr("Layout grids"), "view.layoutGrids", v.layout_grids),
            (tr("Rulers"), "view.rulers", v.rulers),
            (tr("Outlines"), "view.outlines", v.outlines),
        ];
        for (label, cmd, on) in toggles {
            if menu_check(ui, on, label, crate::shortcuts::shortcut(cmd).unwrap_or("")) {
                app.run(cmd, json!({}));
                ui.close();
            }
        }
    });
    ui.painter().line_segment([tabs.left_bottom(), tabs.right_bottom()], Stroke::new(1.0, t.border));
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
        if app.session.tool.tool == figcraft_engine::ToolId::Comment {
            crate::comments::panel(app, ui);
            ui.add_space(80.0);
            return;
        }
        // The Frame tool shows the frame presets.
        if app.session.tool.tool == figcraft_engine::ToolId::Frame {
            crate::frame_presets::panel(app, ui);
        } else {
            design(app, ui);
        }
        ui.add_space(80.0);
        a11y::scroll_focus(&mut app.ui.nav, ui);
    });
}

fn design(app: &mut FigApp, ui: &mut Ui) {
    let Ok(ds) = app.session.ds() else { return };
    // The panel shows the document itself, not a live text preview over it.
    let doc = ds.text_preview.clone().unwrap_or_else(|| ds.doc.clone());
    // While a gesture only moves the selection, nothing the sections show changes but where it
    // is: they show the document it started from, so what they keep stays kept (#1571).
    let shown = crate::selection_cache::moved(ds).filter(|_| ds.text_preview.is_none()).map_or_else(|| doc.clone(), |(b, _)| b);
    let sel = ds.selection.clone();
    if sel.is_empty() {
        if app.session.tool.tool == figcraft_engine::ToolId::Scale {
            crate::scale_panel::note_no_selection(ui);
        }
        page_section(app, ui, &doc);
        return;
    }
    crate::selection_cache::show(&shown, &sel);
    header(app, ui, &shown, &sel);
    widgets::separator(ui);
    crate::section_panel::section(app, ui, &shown, &sel);
    if app.session.tool.tool == figcraft_engine::ToolId::Scale {
        crate::scale_panel::section(app, ui, &shown, &sel);
        widgets::separator(ui);
    }
    position(app, ui, &doc, &sel);
    widgets::separator(ui);
    crate::layout_panel::layout_section(app, ui, &shown, &sel);
    widgets::separator(ui);
    if arc_section(app, ui, &shown, &sel) || points_section(app, ui, &shown, &sel) {
        widgets::separator(ui);
    }
    appearance(app, ui, &shown, &sel);
    widgets::separator(ui);
    if crate::selection_cache::with_nodes(&shown, &sel, |it| it.flatten().any(|n| matches!(n.kind, NodeKind::Text(_)))) {
        crate::typography::typography(app, ui, &shown, &sel);
        widgets::separator(ui);
    }
    paints(app, ui, &shown, &sel, true);
    widgets::separator(ui);
    paints(app, ui, &shown, &sel, false);
    widgets::separator(ui);
    effects(app, ui, &shown, &sel);
    widgets::separator(ui);
    crate::selection_colors::section(app, ui, &shown, &sel);
    if crate::selection_cache::every(&shown, &sel, |n| n.kind.is_frame_like()) {
        grids(app, ui, &shown, &sel);
        widgets::separator(ui);
    }
    crate::export_panel::section(app, ui, &shown, &sel);
}

/// Draw the page section: background and colour profile, local styles, and the
/// page export section.
fn page_section(app: &mut FigApp, ui: &mut Ui, doc: &Document) {
    let Ok(page) = app.session.page() else { return };
    let bg = match doc.node(page).map(|n| &n.kind) {
        Some(NodeKind::Page { background, .. }) => *background,
        _ => figcraft_color::Color::DEFAULT_PAGE,
    };
    widgets::section_header(ui, tr("Page"), &[], false);
    // The new colour and, from a scrub, whether the scrub is over (live edits stay one undo step).
    let mut new_color: Option<(figcraft_color::Color, Option<bool>)> = None;
    row(ui, 32.0, |ui| {
        let p = Paint::solid(bg);
        let (sw, opacity) =
            widgets::paint_field(ui, Id::unique("pagebg_swatch"), Id::unique("pagebg_a"), &p, ui.available_width(), |child, text_r| {
                // A typed colour keeps the page's opacity unless it carries one: 4 or 8 hex digits
                // (`FF000080`; `FF0000FF` asks for opaque).
                if let Some(h) = widgets::text_field_inline(child, Id::unique("pagebg"), &bg.to_hex(), text_r.shrink2(vec2(0.0, 4.0)), regular(11.0))
                    && let Some(c) = figcraft_color::Color::from_hex_or_name(&h)
                {
                    let digits = h.trim().trim_start_matches('#');
                    let carries_alpha = matches!(digits.len(), 4 | 8) && digits.chars().all(|ch| ch.is_ascii_hexdigit());
                    new_color = Some((if carries_alpha { c } else { c.with_alpha(bg.a) }, None));
                }
            });
        app.picker_control(crate::colorpicker::PickerTarget::PageBackground, &sw);
        // The opacity: a field like every other opacity field, not a label (#1748).
        let alpha = |v: f64| bg.with_alpha((v / 100.0).clamp(0.0, 1.0) as f32);
        match opacity {
            Some(FieldEdit::Set(v)) => new_color = Some((alpha(v), None)),
            Some(FieldEdit::Scrub(v, done)) => new_color = Some((alpha(v), Some(done))),
            None => {}
        }
    });
    if let Some((c, scrub)) = new_color {
        let p = json!({"color": c.to_css_hex()});
        match scrub {
            None => {
                app.run("page.setBackground", p);
            }
            Some(done) => {
                let _ = app.session.begin_interaction("Page background");
                let _ = app.session.execute("page.setBackground", &p);
                if done {
                    let _ = app.session.commit_interaction();
                }
            }
        }
    }
    widgets::separator(ui);
    widgets::section_header(ui, tr("Color profile"), &[], false);
    row(ui, 32.0, |ui| {
        let resp = widgets::dropdown(ui, doc.color_profile.label(), 140.0, None);
        menu_popup(&resp).show(|ui| {
            for p in figcraft_color::ColorProfile::ALL {
                if menu_item(ui, p.label(), "") {
                    if p != doc.color_profile {
                        app.ui.color_profile = Some(p);
                    }
                    ui.close();
                }
            }
        });
    });
    widgets::separator(ui);
    widgets::section_header(ui, tr("Local styles"), &[], doc.styles.is_empty());
    crate::style_picker::local_styles(app, ui, doc);
    widgets::separator(ui);
    crate::export_panel::page_section(app, ui, doc, page);
}

/// Draw the selection header: type name (or Mixed), frame/group conversion, orientation
/// swap and frame presets, component/mask/boolean quick actions, and instance details.
fn header(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let t = Tokens::get(ui.ctx());
    let label = match common(doc, sel, |n| n.kind.label()) {
        Some(l) if sel.len() == 1 => tr(l).to_string(),
        // The plural ("Frames") is a key of its own.
        Some(l) => tr(&format!("{l}s")).to_string(),
        None => tr("Mixed").into(),
    };
    let is_frame = sel.len() == 1 && sel.first().and_then(|i| doc.node(*i)).is_some_and(|n| matches!(n.kind, NodeKind::Frame));
    // Frames and groups convert into each other from the type menu.
    let is_group = !sel.is_empty() && crate::selection_cache::every(doc, sel, |n| matches!(n.kind, NodeKind::Group));
    let kind_items = |app: &mut FigApp, ui: &mut Ui, frame: bool| {
        for (to_frame, label, cmd) in [(true, tr("Frame"), "object.convertToFrame"), (false, tr("Group"), "object.convertToGroup")] {
            if menu_check(ui, to_frame == frame, label, "") {
                if to_frame != frame {
                    app.run(cmd, json!({"ids": sel}));
                }
                ui.close();
            }
        }
    };
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    // Keep the type menu and frame actions together. A short label must not leave
    // the orientation icon floating in the middle of the header.
    let max_label_width = if is_frame { (w - 202.0).clamp(40.0, 120.0) } else { 92.0 };
    let shown = if is_frame || is_group { widgets::elide_font(ui, &label, max_label_width, semibold(13.0)) } else { label.clone() };
    let label_width = if is_frame || is_group { ui.painter().layout_no_wrap(shown.clone(), semibold(13.0), t.text).size().x + 28.0 } else { 120.0 };
    let lr = Rect::from_min_size(pos2(r.min.x + 8.0, r.center().y - 12.0), vec2(label_width, 24.0));
    let resp = ui.interact(lr, ui.scope_id().with("typedd"), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(lr, CornerRadius::same(5), t.hover);
    }
    let resp = if shown == label { resp } else { resp.on_hover_text(&label) };
    let label_rect = ui.painter().text(pos2(lr.min.x + 8.0, lr.center().y), Align2::LEFT_CENTER, &shown, semibold(13.0), t.text);
    if is_frame || is_group {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(label_rect.right() + 8.0, lr.center().y), vec2(10.0, 10.0)),
            Icon::Chevron,
            t.icon_secondary,
        );
    }
    if is_group {
        menu_popup(&resp).width(180.0).show(|ui| kind_items(app, ui, false));
    }
    if is_frame {
        let sr = Rect::from_min_size(pos2(lr.max.x + 2.0, r.center().y - 12.0), vec2(24.0, 24.0));
        let sresp = ui.interact(sr, ui.scope_id().with("swap_orient"), Sense::click()).tip(tr("Swap orientation"));
        if sresp.hovered() {
            ui.painter().rect_filled(sr, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), crate::widgets::icon_glyph_rect(sr), Icon::Rotate, t.icon);
        if sresp.clicked() {
            app.run("frame.swapOrientation", json!({"ids": sel}));
        }
        let fit_r = sr.translate(vec2(26.0, 0.0));
        let fit_tip = crate::shortcuts::tip(tr("Fit to contents"), "object.resizeToFit");
        if widgets::icon_button_at(ui, ui.scope_id().with("frame_fit_contents"), fit_r, Icon::FitContents, fit_tip) {
            app.run("object.resizeToFit", json!({"ids": sel}));
        }
        let ids: Vec<NodeId> = sel.to_vec();
        menu_popup(&resp).width(260.0).show(|ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                kind_items(app, ui, true);
                ui.separator();
                let mut cat = "";
                for (c, name, pw, ph) in figcraft_engine::cmd::create::FRAME_PRESETS {
                    if *c != cat {
                        cat = c;
                        ui.label(egui::RichText::new(tr(c)).font(semibold(11.0)).color(t.text_secondary));
                    }
                    if menu_item(ui, name, &format!("{}×{}", pw, ph)) {
                        app.set_props(&ids, json!({"width": pw, "height": ph}));
                        ui.close();
                    }
                }
            });
        });
    }
    // The quick action on the right: Use as mask.
    let (bw, cmd) = (24.0, "object.mask");
    let br = Rect::from_min_size(pos2(r.max.x - 8.0 - bw, r.center().y - 12.0), vec2(bw, 24.0));
    let bresp = ui.interact(br, ui.scope_id().with(("qa", cmd)), Sense::click()).tip(crate::shortcuts::tip(tr("Use as mask"), cmd));
    if bresp.hovered() {
        ui.painter().rect_filled(br, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), crate::widgets::icon_glyph_rect(br), Icon::Mask, t.icon);
    if bresp.clicked() {
        app.run(cmd, json!({}));
    }
}

/// The Position section: its values from `doc`.
fn position(app: &mut FigApp, ui: &mut Ui, doc: &Arc<Document>, sel: &[NodeId]) {
    widgets::section_header(ui, tr("Position"), &[], false);
    position_fields(app, ui, doc, sel);
    ui.add_space(8.0);
}

/// What the position fields show of the selection (`None`: mixed), kept while the document and the
/// selection stay the same ([`crate::selection_cache`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PositionValues {
    pub can_align: bool,
    /// Align is off because the one selected layer is placed by its auto layout frame.
    pub align_blocked: bool,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub rot: Option<f64>,
    /// Every selected layer is in an auto layout frame, and whether they ignore its layout.
    pub parent_al: bool,
    pub absolute: Option<bool>,
    /// Every selected layer is in a frame that places it by constraints, and those constraints.
    pub in_frame: bool,
    pub constraints: Option<figcraft_doc::Constraints>,
}

/// What the X field shows of a layer (in its frame's space: groups have none of their own); a
/// relative edit of mixed values starts from it.
fn x_of(doc: &Document, n: &Node) -> f64 {
    (doc.frame_relative(n.id).as_coeffs()[4] * 100.0).round() / 100.0
}

fn y_of(doc: &Document, n: &Node) -> f64 {
    (doc.frame_relative(n.id).as_coeffs()[5] * 100.0).round() / 100.0
}

/// The layer's rotation on the page (its own angle plus its parents'), what the field shows.
fn rot_of(doc: &Document, n: &Node) -> f64 {
    (figcraft_geom::rotation_degrees(doc.world_transform(n.id)) * 100.0).round() / 100.0
}

pub(crate) fn position_values(ctx: &egui::Context, doc: &Arc<Document>, sel: &[NodeId]) -> PositionValues {
    let parent = |n: &Node| n.parent.and_then(|p| doc.node(p));
    crate::selection_cache::cached(ctx, "position", doc, sel, &[], || PositionValues {
        can_align: figcraft_engine::cmd::arrange::can_align(doc, sel),
        align_blocked: figcraft_engine::cmd::arrange::align_blocked_by_auto_layout(doc, sel),
        x: common(doc, sel, |n| x_of(doc, n)),
        y: common(doc, sel, |n| y_of(doc, n)),
        rot: common(doc, sel, |n| rot_of(doc, n)),
        parent_al: sel.iter().all(|i| doc.node(*i).and_then(parent).is_some_and(|p| p.layout.is_some())),
        absolute: common(doc, sel, |n| n.child_layout.absolute),
        // Constraints: children of frames without auto layout, or ignoring it.
        in_frame: sel.iter().all(|i| {
            doc.node(*i).is_some_and(|n| parent(n).is_some_and(|p| p.kind.is_frame_like() && (p.layout.is_none() || n.child_layout.absolute)))
        }),
        constraints: common(doc, sel, |n| n.constraints),
    })
}

/// Whether the Design panel offers Tidy up for `sel`: three or more layers forming a row, a column
/// or a full grid, none in an auto layout flow (`can_tidy`).
pub(crate) fn tidy_available(ctx: &egui::Context, doc: &Arc<Document>, sel: &[NodeId]) -> bool {
    crate::selection_cache::cached(ctx, "tidy", doc, sel, &[], || figcraft_engine::tidy::can_tidy(doc, sel))
}

/// Draw alignment and distribution, X/Y and rotation with flip/rotate controls, the
/// absolute-position toggle and frame constraints.
fn position_fields(app: &mut FigApp, ui: &mut Ui, doc: &Arc<Document>, sel: &[NodeId]) {
    let pv = position_values(ui.ctx(), doc, sel);
    let col = widgets::col_width(ui.available_width());
    align_row(app, ui, sel, pv.can_align, pv.align_blocked, col);
    position_row(app, ui, doc, sel, &pv, col);
    rotation_row(app, ui, doc, sel, pv.rot, col);
    if pv.in_frame {
        constraints_row(app, ui, sel, pv.constraints, col);
    }
}

/// Alignment, and distribution and tidy up for three or more layers.
pub fn align_row(app: &mut FigApp, ui: &mut Ui, sel: &[NodeId], can_align: bool, blocked: bool, col: f32) {
    let ids = sel.to_vec();
    let params = || json!({"ids": ids});
    widgets::sublabels(ui, tr("Alignment"), None);
    row(ui, 32.0, |ui| {
        let cmds = ["align.left", "align.hcenter", "align.right", "align.top", "align.vcenter", "align.bottom"];
        let tip = crate::shortcuts::tip;
        let items = [
            (Icon::AlignLeft, tip(tr("Align left"), cmds[0])),
            (Icon::AlignHCenter, tip(tr("Align horizontal centers"), cmds[1])),
            (Icon::AlignRight, tip(tr("Align right"), cmds[2])),
            (Icon::AlignTop, tip(tr("Align top"), cmds[3])),
            (Icon::AlignVCenter, tip(tr("Align vertical centers"), cmds[4])),
            (Icon::AlignBottom, tip(tr("Align bottom"), cmds[5])),
        ];
        let w = col / 3.0;
        let picked = ui.add_enabled_ui(can_align, |ui| {
            let h = widgets::segmented(ui, &items[..3], None, w);
            let v = widgets::segmented(ui, &items[3..], None, w).map(|i| i + 3);
            h.or(v)
        });
        let why = if blocked {
            tr("A layer in an auto layout frame is placed by its frame")
        } else {
            tr("A layer on its own aligns to its frame: select more layers, or one in a frame")
        };
        picked.response.on_disabled_hover_text(why);
        if let Some(i) = picked.inner {
            app.run(cmds[i], params());
        }
        if ids.len() > 2 {
            let more = icons::button(ui, Icon::DistributeH, 24.0, false, tr("Distribute and tidy"));
            menu_popup(&more).show(|ui| {
                let can_tidy = app.session.ds().is_ok_and(|ds| tidy_available(ui.ctx(), &ds.doc, &ids));
                let tidy = (tr("Tidy up"), "arrange.tidy", can_tidy);
                let entries = [
                    (tr("Distribute horizontal spacing"), "distribute.horizontal", true),
                    (tr("Distribute vertical spacing"), "distribute.vertical", true),
                    tidy,
                ];
                for (l, c, on) in entries {
                    let item = ui.add_enabled_ui(on, |ui| menu_item(ui, l, crate::shortcuts::shortcut(c).unwrap_or("")));
                    if !on {
                        item.response
                            .on_disabled_hover_text(tr("Select a row, a column or a full grid of three or more layers, outside an auto layout flow"));
                    }
                    if item.inner {
                        app.run(c, params());
                        ui.close();
                    }
                }
            });
        }
    });
}

/// X and Y, and the absolute-position toggle for children of auto layout frames.
fn position_row(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], pv: &PositionValues, col: f32) {
    let ids = sel.to_vec();
    widgets::sublabels(ui, tr("Position"), None);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field_mixed(ui, Id::unique("pos_x"), Lead::Text("X"), pv.x, "", col, 1.0);
        app.field_of(&ids, "x", e, |n| x_of(doc, n), |v| json!(v));
        let e = widgets::number_field_mixed(ui, Id::unique("pos_y"), Lead::Text("Y"), pv.y, "", col, 1.0);
        app.field_of(&ids, "y", e, |n| y_of(doc, n), |v| json!(v));
        // Children of auto layout frames can ignore the layout (absolute position).
        if pv.parent_al {
            let on = pv.absolute.unwrap_or(false);
            if icons::button(
                ui,
                Icon::AbsolutePosition,
                24.0,
                on,
                if on { tr("Use auto layout") } else { tr("Ignore auto layout (absolute position)") },
            )
            .clicked()
            {
                app.set_props(&ids, json!({"absolute": !on}));
            }
        }
    });
}

/// Rotation, with the rotate and flip buttons.
fn rotation_row(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], rot: Option<f64>, col: f32) {
    let ids = sel.to_vec();
    widgets::sublabels(ui, tr("Rotation"), None);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field_mixed(ui, Id::unique("pos_rot"), Lead::Icon(Icon::Angle, tr("Rotation")), rot, "°", col, 1.0);
        app.field_of(&ids, "absoluteRotation", e, |n| rot_of(doc, n), |v| json!(v));
        let items = [
            (Icon::Rotate, tr("Rotate 90° right")),
            (Icon::FlipH, crate::shortcuts::tip(tr("Flip horizontal"), "object.flipH")),
            (Icon::FlipV, crate::shortcuts::tip(tr("Flip vertical"), "object.flipV")),
        ];
        if let Some(i) = widgets::segmented(ui, &items, None, col / 3.0) {
            app.run(["object.rotateRight", "object.flipH", "object.flipV"][i], json!({"ids": ids}));
        }
    });
}

/// The horizontal and vertical constraints of children of frames without auto layout.
fn constraints_row(app: &mut FigApp, ui: &mut Ui, sel: &[NodeId], c: Option<figcraft_doc::Constraints>, col: f32) {
    let ids = sel.to_vec();
    widgets::sublabels(ui, tr("Constraints"), None);
    row(ui, 32.0, |ui| {
        let h = c.map(|c| format!("{:?}", c.horizontal)).unwrap_or_else(|| "Mixed".into());
        let resp = widgets::dropdown(ui, tr(&pretty(&h)), col, None);
        menu_popup(&resp).show(|ui| {
            for (l, v) in
                [(tr("Left"), "left"), (tr("Right"), "right"), (tr("Left & right"), "leftRight"), (tr("Center"), "center"), (tr("Scale"), "scale")]
            {
                if menu_item(ui, l, "") {
                    app.set_props(&ids, json!({"constraints": {"horizontal": v}}));
                    ui.close();
                }
            }
        });
        let v = c.map(|c| format!("{:?}", c.vertical)).unwrap_or_else(|| "Mixed".into());
        let resp = widgets::dropdown(ui, tr(&pretty(&v)), col, None);
        menu_popup(&resp).show(|ui| {
            for (l, val) in
                [(tr("Top"), "top"), (tr("Bottom"), "bottom"), (tr("Top & bottom"), "topBottom"), (tr("Center"), "center"), (tr("Scale"), "scale")]
            {
                if menu_item(ui, l, "") {
                    app.set_props(&ids, json!({"constraints": {"vertical": val}}));
                    ui.close();
                }
            }
        });
    });
}

/// The selected ellipses' arc: Start (degrees), Sweep (a share of a full turn, negative running
/// the other way) and Ratio (the inner radius), the same values the canvas arc handles drag
/// (`node.set`'s `arcStart` / `arcSweep` / `arcInner`). Returns whether it showed.
fn arc_section(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) -> bool {
    let arcs: Vec<(f64, f64, f64)> = crate::selection_cache::with_nodes(doc, sel, |it| {
        it.flatten()
            .filter_map(|n| match n.kind {
                NodeKind::Ellipse { start, sweep, inner } => Some((start, sweep, inner)),
                _ => None,
            })
            .collect()
    });
    if arcs.is_empty() || arcs.len() != sel.len() {
        return false;
    }
    let ids = sel.to_vec();
    let same = |f: &dyn Fn(&(f64, f64, f64)) -> f64| -> Option<f64> {
        let v = (f(arcs.first()?) * 100.0).round() / 100.0;
        arcs.iter().all(|a| ((f(a) * 100.0).round() / 100.0 - v).abs() < 1e-9).then_some(v)
    };
    let start = same(&|a| a.0.to_degrees().rem_euclid(360.0));
    let sweep = same(&|a| a.1 / std::f64::consts::TAU * 100.0);
    let ratio = same(&|a| a.2 * 100.0);
    widgets::section_header(ui, tr("Arc"), &[], false);
    let col = widgets::col_width(ui.available_width());
    let field = |ui: &mut Ui, key: &str, icon: Icon, name: &str, value: Option<f64>, suffix: &str| {
        widgets::number_field(ui, Id::unique(("arc", key)), Lead::Icon(icon, name), value, suffix, col, 1.0)
    };
    widgets::sublabels(ui, tr("Start"), Some(tr("Sweep")));
    row(ui, 32.0, |ui| {
        let e = field(ui, "arcStart", Icon::ArcStart, tr("Arc start"), start, "°");
        app.field(&ids, "arcStart", e, |v| json!(v));
        let e = field(ui, "arcSweep", Icon::ArcSweep, tr("Arc sweep"), sweep, "%");
        app.field(&ids, "arcSweep", e, |v| json!(v.clamp(-100.0, 100.0) * 3.6));
    });
    widgets::sublabels(ui, tr("Ratio"), None);
    row(ui, 32.0, |ui| {
        let e = field(ui, "arcInner", Icon::ArcRatio, tr("Arc ratio"), ratio, "%");
        app.field(&ids, "arcInner", e, |v| json!((v / 100.0).clamp(0.0, 0.99)));
    });
    ui.add_space(8.0);
    true
}

/// The selected polygons' and stars' point count (3 to 60 on stars), and the stars' ratio (the
/// inner radius as a share of the outer one), as `node.set`'s `pointCount` / `innerRadius`.
/// Returns whether it showed.
fn points_section(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) -> bool {
    let shapes: Vec<(u32, Option<f64>)> = crate::selection_cache::with_nodes(doc, sel, |it| {
        it.flatten()
            .filter_map(|n| match n.kind {
                NodeKind::Polygon { count } => Some((count, None)),
                NodeKind::Star { count, inner_ratio } => Some((count, Some(inner_ratio))),
                _ => None,
            })
            .collect()
    });
    if shapes.is_empty() || shapes.len() != sel.len() {
        return false;
    }
    let ids = sel.to_vec();
    let count = shapes.first().map(|s| s.0).filter(|c| shapes.iter().all(|s| s.0 == *c)).map(f64::from);
    let stars: Vec<f64> = shapes.iter().filter_map(|s| s.1).collect();
    let ratio = stars.first().map(|r| (r * 1000.0).round() / 10.0).filter(|r| stars.iter().all(|x| ((x * 1000.0).round() / 10.0 - r).abs() < 1e-9));
    let all_stars = stars.len() == shapes.len();
    widgets::section_header(ui, tr(if all_stars { "Star" } else { "Polygon" }), &[], false);
    let col = widgets::col_width(ui.available_width());
    widgets::sublabels(ui, tr("Count"), all_stars.then_some(tr("Ratio")));
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::unique("pts_count"), Lead::Icon(Icon::Polygon, tr("Count")), count, "", col, 1.0);
        app.field(&ids, "pointCount", e, |v| json!(v.round().clamp(3.0, 120.0)));
        if all_stars {
            let e = widgets::number_field(ui, Id::unique("pts_ratio"), Lead::Icon(Icon::Star, tr("Ratio")), ratio, "%", col, 1.0);
            app.field(&ids, "innerRadius", e, |v| json!((v / 100.0).clamp(0.001, 1.0)));
        }
    });
    ui.add_space(8.0);
    true
}

/// Constraint names as shown (English keys, translated at display).
fn pretty(s: &str) -> String {
    match s {
        "LeftRight" => "Left & right".into(),
        "TopBottom" => "Top & bottom".into(),
        other => other.to_string(),
    }
}
