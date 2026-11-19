//! A colour picked with the eyedropper while holding Cmd/Ctrl+Shift becomes a colour style, as in
//! the reference app: the eyedropper asks for this window (`dialog.createFromColor`), which asks
//! what to call it, then runs `style.create`.

use egui::{Context, RichText};
use figcraft_color::{Color, Paint};
use serde_json::{Value, json};

use super::window;
use crate::FigApp;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::widgets;

/// The window's state: the colour and the style's name.
#[derive(Clone, Debug, PartialEq)]
pub struct SampledColor {
    /// The document it was picked in: the window goes when another is in front.
    pub uid: u64,
    /// `#rrggbb`.
    pub color: String,
    pub name: String,
}

/// `dialog.createFromColor {color}` from the eyedropper: the window for the document in front, with
/// the next free style name ("Color 1"…).
pub fn request(app: &mut FigApp, data: &Value) {
    let Some(color) = data.get("color").and_then(Value::as_str).and_then(Color::from_hex) else { return };
    let Ok(ds) = app.session.ds() else { return };
    app.ui.sampled_color = Some(SampledColor { uid: ds.uid, color: color.to_css_hex(), name: crate::style_picker::next_name(&ds.doc, "Color") });
}

/// The `style.create` params that make what `s` asks for: a paint style of its colour.
pub fn create_params(s: &SampledColor) -> Value {
    json!({"type": "paint", "name": s.name.trim(), "value": s.color})
}

/// The window: a swatch of the colour, the style's name, Cancel and Create (Enter creates).
pub fn show(app: &mut FigApp, ctx: &Context) {
    let Some(mut s) = app.ui.sampled_color.clone() else { return };
    if app.session.ds().map(|d| d.uid).ok() != Some(s.uid) {
        app.ui.sampled_color = None;
        return;
    }
    let t = Tokens::get(ctx);
    let (mut open, mut cancel, mut create) = (true, false, false);
    window(tr("Create from color"), ctx).id(egui::Id::unique("Create from color")).open(&mut open).show(ctx, |ui| {
        ui.set_width(280.0);
        ui.horizontal(|ui| {
            let paint = Color::from_hex(&s.color).map(Paint::solid);
            if let Some(p) = &paint {
                widgets::swatch(ui, p, 24.0);
            }
            ui.label(RichText::new(s.color.trim_start_matches('#').to_uppercase()).color(t.text));
        });
        ui.add_space(8.0);
        let name = ui.add(egui::TextEdit::singleline(&mut s.name).hint_text(tr("Name")).desired_width(260.0));
        widgets::select_all_on_focus(ui, &name, &s.name);
        if ui.memory(|m| m.focused().is_none()) {
            name.request_focus();
        }
        ui.add_space(10.0);
        let ready = !s.name.trim().is_empty();
        ui.horizontal(|ui| {
            cancel = widgets::button(ui, tr("Cancel"), false, true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            let enter = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            create = (widgets::button(ui, tr("Create"), true, ready).clicked() || enter) && ready;
        });
    });
    let made = create && { app.run("style.create", create_params(&s)).is_some() };
    // A name already taken leaves the window open to change it (the error says why).
    app.ui.sampled_color = (open && !cancel && !made).then_some(s);
}
