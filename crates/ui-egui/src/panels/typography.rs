//! Typography section of the Design panel: font family (searchable, previewed, recent first),
//! style, size (with common sizes), line height, letter spacing, paragraph spacing, resizing,
//! alignment, and the Type settings popover (decoration, case, lists, indent, truncation, OpenType
//! features and variable font axes). While text is being edited with a range selected, character
//! edits apply to the range (`text.style`, `text.setVariations`); otherwise they apply to whole
//! layers. Differing values show "Mixed".

use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_doc::{
    DecorationLength, Document, LeadingTrim, LetterSpacing, LineHeight, ListStyle, NodeId, NodeKind, TextAlignH, TextAlignV, TextAutoResize,
    TextCase, TextData, TextDecoration, TextDirection, TextStyle, trim_num,
};
use serde_json::{Value, json};

use crate::FigApp;
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
use crate::right::{common, menu_item, menu_popup, row};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{self, FIELD_H, FieldEdit, Lead};

mod axes;
mod fields;
mod glyphs;
mod picker;
mod spacing;

use fields::{font_rows, paragraph_rows};
pub(crate) use picker::font_picker;
pub(crate) use spacing::{letter_spacing_of, line_height_of, parse_letter_spacing, parse_line_height};

/// Font sizes offered by the size dropdown.
const SIZES: [f64; 15] = [10.0, 11.0, 12.0, 13.0, 14.0, 16.0, 20.0, 24.0, 32.0, 36.0, 40.0, 48.0, 64.0, 96.0, 128.0];

/// OpenType features offered in Type settings ▸ Details: (tag, label, on by default in fonts).
const FEATURES: [(&str, &str, bool); 12] = [
    ("liga", tr_noop("Standard ligatures"), true),
    ("calt", tr_noop("Contextual alternates"), true),
    ("tnum", tr_noop("Tabular numbers"), false),
    ("frac", tr_noop("Fractions"), false),
    ("case", tr_noop("Case-sensitive forms"), false),
    ("zero", tr_noop("Slashed zero"), false),
    ("ss01", tr_noop("Stylistic set 1"), false),
    ("ss02", tr_noop("Stylistic set 2"), false),
    ("ss03", tr_noop("Stylistic set 3"), false),
    ("ss04", tr_noop("Stylistic set 4"), false),
    ("ss05", tr_noop("Stylistic set 5"), false),
    ("smcp", tr_noop("Small capitals"), false),
];

/// Shown for differing values (translated where shown: `tr(MIXED)`).
const MIXED: &str = tr_noop("Mixed");

/// The Type settings tabs (Variable only for variable fonts).
const TABS: [&str; 3] = [tr_noop("Basics"), tr_noop("Details"), tr_noop("Variable")];

/// The value shared by every style, or None when they differ.
fn uniform<T: PartialEq + Clone>(styles: &[TextStyle], f: impl Fn(&TextStyle) -> T) -> Option<T> {
    let mut it = styles.iter();
    let first = f(it.next()?);
    if it.all(|s| f(s) == first) { Some(first) } else { None }
}

/// The character styles the panel shows: the edited range's spans, else every span of every
/// selected text layer.
pub(crate) fn target_styles(doc: &Document, ids: &[NodeId], range: Option<(NodeId, usize, usize)>) -> Vec<TextStyle> {
    let mut out: Vec<TextStyle> = Vec::new();
    let mut push = |s: &TextStyle| {
        if !out.contains(s) {
            out.push(s.clone());
        }
    };
    for id in ids {
        let Some(t) = doc.node(*id).and_then(|n| n.kind.text()) else { continue };
        match range {
            Some((rid, a, b)) if rid == *id && a < b => {
                for (s, e, st) in t.spans() {
                    if s < b && e > a {
                        push(st);
                    }
                }
            }
            _ => {
                for (_, _, st) in t.spans() {
                    push(st);
                }
            }
        }
    }
    out
}

pub fn typography(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let ids: Vec<NodeId> = sel.iter().copied().filter(|i| matches!(doc.node(*i).map(|n| &n.kind), Some(NodeKind::Text(_)))).collect();
    if ids.is_empty() {
        return;
    }
    // While editing a range, character edits apply to it.
    let range = figcraft_engine::textedit::selected_range(&app.session);
    let editing = range.is_some();
    let styles = target_styles(doc, &ids, range);
    let apply_style = |app: &mut FigApp, key: &str, v: Value| commit_text(app, &ids, range, json!({ key: v }));

    let hdr = widgets::section_header(ui, tr("Typography"), &[(Icon::Styles, tr("Text styles"))], false);
    if hdr == Some(0) {
        crate::style_picker::open(app, ui, &ids, crate::style_picker::StyleSlot::Text);
    }
    // A text style: its chip stands for the font rows (they show again when the chars differ).
    crate::style_picker::chip(app, ui, doc, &ids, crate::style_picker::StyleSlot::Text);
    if !matches!(crate::style_picker::text_link(app, doc, &ids), Some(Some(_))) {
        font_rows(app, ui, &ids, &styles, editing, &apply_style);
    }
    paragraph_rows(app, ui, doc, &ids, &styles, &apply_style);
    path_row(app, ui, doc, sel, &ids);
}

/// Text on a path: where the text starts, which side it runs on, and taking it off the path; or,
/// with a text and a path selected, putting the text on the path.
fn path_row(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], ids: &[NodeId]) {
    let paths: Option<Vec<(f64, bool)>> =
        ids.iter().map(|i| doc.node(*i).and_then(|n| n.kind.text()).and_then(|t| t.path.as_ref()).map(|p| (p.start(), p.flip))).collect();
    let Some(paths) = paths else {
        let other = sel.iter().find(|i| !ids.contains(i)).and_then(|i| doc.node(*i));
        if ids.len() == 1 && sel.len() == 2 && other.is_some_and(|n| figcraft_doc::geometry::stroke_geometry(n).is_some()) {
            row(ui, 32.0, |ui| {
                if widgets::button(ui, tr("Put text on path"), false, true).clicked() {
                    app.run("text.onPath", json!({ "ids": sel }));
                }
            });
        }
        return;
    };
    let start = paths.first().map(|p| p.0).filter(|s| paths.iter().all(|p| p.0 == *s));
    let flip = paths.iter().all(|p| p.1);
    row(ui, 32.0, |ui| {
        let lead = Lead::Icon(Icon::Pen, tr("Start on path"));
        match widgets::number_field(ui, Id::unique("ty_path_start"), lead, start.map(|s| s * 100.0), "%", 96.0, 1.0) {
            Some(FieldEdit::Set(v)) => {
                app.run("text.pathOptions", json!({"ids": ids, "start": v / 100.0}));
            }
            Some(FieldEdit::Scrub(v, done)) => live(app, "Start on path", "text.pathOptions", json!({"ids": ids, "start": v / 100.0}), done),
            None => {}
        }
        if icons::button(ui, Icon::FlipV, 24.0, flip, tr("Flip text to the other side")).clicked() {
            app.run("text.pathOptions", json!({"ids": ids, "flip": !flip}));
        }
        if icons::button(ui, Icon::Detach, 24.0, false, tr("Remove text from path")).clicked() {
            app.run("text.removeFromPath", json!({ "ids": ids }));
        }
    });
}

/// Set character `props` on the selected chars of the text being edited, else on whole layers
/// (through `node.set`, which records overrides inside instances). Paragraph spacing is always
/// the whole layer's: `text.style` doesn't take it.
pub(crate) fn commit_text(app: &mut FigApp, ids: &[NodeId], range: Option<(NodeId, usize, usize)>, props: Value) {
    match range {
        Some((_, a, b)) if a != b && props.get("paragraphSpacing").is_none() => {
            app.run("text.style", text_style_params(ids, range, props));
        }
        _ => app.set_props(ids, props),
    }
}

/// `text.style` params for `props` on the selected chars of the text being edited, else on the
/// whole of every layer in `ids` (previews; commits go through [`commit_text`]).
pub(crate) fn text_style_params(ids: &[NodeId], range: Option<(NodeId, usize, usize)>, mut props: Value) -> Value {
    match range {
        Some((id, a, b)) if a != b => {
            props["id"] = json!(id);
            props["start"] = json!(a);
            props["end"] = json!(b);
        }
        _ => props["ids"] = json!(ids),
    }
    props
}

/// Run `cmd` as a live preview while scrubbing; the gesture becomes one undo step when `done`.
/// (While text is edited, run commands directly: the editing session already groups undo.)
pub(crate) fn live(app: &mut FigApp, label: &str, cmd: &str, params: Value, done: bool) {
    let _ = app.session.begin_interaction(label);
    if let Err(e) = app.session.execute(cmd, &params) {
        app.toast(e.to_string());
    }
    if done {
        let _ = app.session.commit_interaction();
    }
}

/// A Type settings row: a label, then `add` with the rest of the width. A label wider than its
/// column (French, say) narrows the field rather than the row outgrowing the popover, which then
/// widened every frame (#1029).
pub(crate) fn field_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui, f32) -> R) -> R {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, FIELD_H), egui::Label::new(egui::RichText::new(label).font(regular(11.0)).color(t.text)));
        add(ui, ui.available_width())
    })
    .inner
}

#[cfg(test)]
mod settings_tests {
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
    use figcraft_doc::WrapStyle;
    use figcraft_engine::Session;

    use super::*;
    use crate::NoServices;

    /// The Typography section, drawn for a selected text layer (Inter Regular).
    struct Panel {
        app: FigApp,
        ctx: egui::Context,
        time: f64,
        text: NodeId,
        /// The AccessKit nodes of the last frame.
        nodes: Vec<egui::accesskit::Node>,
    }

    impl Panel {
        fn new() -> Self {
            let mut s = Session::new();
            let text = NodeId(s.execute("text.create", &json!({"text": "Title"})).unwrap()["id"].as_u64().unwrap());
            s.execute("select.set", &json!({ "ids": [text] })).unwrap();
            let mut p = Self { app: FigApp::new(s, Box::new(NoServices)), ctx: egui::Context::default(), time: 0.0, text, nodes: Vec::new() };
            p.ctx.enable_accesskit();
            crate::theme::install_fonts(&p.ctx, false, 0);
            crate::theme::apply(&p.ctx, Tokens::light());
            for _ in 0..3 {
                p.frame(vec![]);
            }
            p
        }

        /// One frame with `events`; the texts drawn, with their centers.
        fn frame(&mut self, events: Vec<Event>) -> Vec<(String, Pos2)> {
            fn walk(shape: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
                match shape {
                    egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos + t.galley.size() / 2.0)),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(280.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut out = self.ctx.run_ui(input, |ui| {
                let ds = self.app.session.ds().unwrap();
                let doc = ds.doc.clone();
                let sel = ds.selection.clone();
                typography(&mut self.app, ui, &doc, &sel);
            });
            out.textures_delta.clear();
            if let Some(update) = out.platform_output.accesskit_update.take() {
                self.nodes = update.nodes.into_iter().map(|(_, n)| n).collect();
            }
            let mut texts = Vec::new();
            for c in out.shapes {
                walk(&c.shape, &mut texts);
            }
            texts
        }

        /// Hover `p`, then press and release there; the texts drawn after.
        fn click(&mut self, p: Pos2) -> Vec<(String, Pos2)> {
            self.frame(vec![Event::PointerMoved(p)]);
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
            }
            self.frame(vec![])
        }
    }

    /// The centre of the AccessKit node labelled `label` (a button, say).
    fn node_center(p: &Panel, label: &str) -> Pos2 {
        let n = p.nodes.iter().find(|n| n.label() == Some(label)).unwrap_or_else(|| panic!("no node {label:?}"));
        let r = n.bounds().expect("bounds");
        Pos2::new(((r.x0 + r.x1) / 2.0) as f32, ((r.y0 + r.y1) / 2.0) as f32)
    }

    const WRAP: [&str; 3] = [
        "Auto: as many words as fit on each line",
        "Balance: every line close to the same length",
        "Pretty: full lines, no one word alone on the last",
    ];

    /// The Wrap style buttons that are on, by short name.
    fn wrap_on(p: &Panel) -> Vec<&'static str> {
        let on = |label: &str| p.nodes.iter().find(|n| n.label() == Some(label)).unwrap_or_else(|| panic!("no node {label:?}")).toggled();
        ["Auto", "Balance", "Pretty"].into_iter().zip(WRAP).filter(|(_, l)| on(l) == Some(egui::accesskit::Toggled::True)).map(|(n, _)| n).collect()
    }

    /// The panel with the Type settings popover open, over two paragraphs.
    fn settings_panel() -> Panel {
        let mut p = Panel::new();
        p.app.session.execute("node.set", &json!({"id": p.text, "props": {"characters": "one\ntwo"}})).unwrap();
        p.frame(vec![]);
        let at = node_center(&p, "Type settings");
        p.click(at);
        p.frame(vec![]);
        p
    }

    #[test]
    fn wrap_style_control_follows_the_caret_paragraph() {
        // #1827: with the caret in one paragraph the control shows and sets that paragraph's style.
        let mut p = settings_panel();
        assert_eq!(wrap_on(&p), ["Auto"], "the layer's style when not editing");
        figcraft_engine::textedit::begin(&mut p.app.session, p.text, Some(5), false).unwrap();
        p.frame(vec![]);
        assert_eq!(wrap_on(&p), ["Auto"]);
        let at = node_center(&p, WRAP[1]);
        p.click(at);
        let td = |p: &Panel| p.app.session.doc().unwrap().node(p.text).unwrap().kind.text().unwrap().clone();
        assert_eq!((td(&p).para_wrap_style(0), td(&p).para_wrap_style(1)), (WrapStyle::Auto, WrapStyle::Balance), "only the caret's paragraph");
        assert_eq!(td(&p).wrap_style, WrapStyle::Auto);
        assert_eq!(wrap_on(&p), ["Balance"], "the control shows the paragraph's value");
        figcraft_engine::textedit::set_caret(&mut p.app.session, 1, false).unwrap();
        p.frame(vec![]);
        assert_eq!(wrap_on(&p), ["Auto"], "another paragraph shows its own");
        assert!(p.app.ui.last_error.is_none(), "{:?}", p.app.ui.last_error);
    }

    #[test]
    fn wrap_style_control_has_nothing_active_for_mixed_paragraphs() {
        let mut p = settings_panel();
        figcraft_engine::textedit::begin(&mut p.app.session, p.text, Some(5), false).unwrap();
        p.frame(vec![]);
        let at = node_center(&p, WRAP[2]);
        p.click(at);
        assert_eq!(wrap_on(&p), ["Pretty"]);
        // A selection across both paragraphs, which differ: none is on.
        figcraft_engine::textedit::set_caret(&mut p.app.session, 1, false).unwrap();
        figcraft_engine::textedit::set_caret(&mut p.app.session, 6, true).unwrap();
        p.frame(vec![]);
        assert!(wrap_on(&p).is_empty(), "{:?}", wrap_on(&p));
        // Choosing one sets both paragraphs.
        let at = node_center(&p, WRAP[1]);
        p.click(at);
        assert_eq!(wrap_on(&p), ["Balance"]);
        let t = p.app.session.doc().unwrap().node(p.text).unwrap().kind.text().unwrap().clone();
        assert_eq!((t.para_wrap_style(0), t.para_wrap_style(1)), (WrapStyle::Balance, WrapStyle::Balance));
    }
}
