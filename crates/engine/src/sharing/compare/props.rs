//! A layer's properties grouped the way the compare reports them (layout, auto layout,
//! typography, fills, strokes, effects), each with the style or variable behind it when there is
//! one. [`super::layer_changes`] diffs two of these row by row.

use figcraft_color::{Color, GradientKind, PaintKind};
use figcraft_doc::{
    AutoLayout, ContentAlign, CrossAlign, Document, EffectKind, LayoutMode, MainAlign, Node, NodeId, NoiseMode, StrokeAlign, StyleId, TextAlignH,
    TextData, TextDecoration, Variable,
};

/// One group of properties ("Layout", "Typography"…).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub title: &'static str,
    pub rows: Vec<Prop>,
}

/// One property: its label and value as shown and the style or variable name it comes from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Prop {
    pub label: String,
    pub value: String,
    pub token: Option<String>,
}

/// `c` as hex; the opacity follows as " · 50%".
fn color_text(c: Color) -> String {
    let body = format!("#{}", c.to_hex());
    if c.a < 0.999 { format!("{body} · {}%", (c.a * 100.0).round()) } else { body }
}

/// A number as a designer reads it: at most two decimals, no trailing zeros.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{:.2}", (v * 100.0).round() / 100.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn prop(label: impl Into<String>, value: impl Into<String>) -> Prop {
    Prop { label: label.into(), value: value.into(), token: None }
}

impl Prop {
    fn token(mut self, t: Option<String>) -> Self {
        self.token = t;
        self
    }
}

/// The variable bound to property `field` of `n` ("fill" and "stroke" also find bindings stored
/// under their list forms, like "fills/0/color").
fn bound_variable<'d>(doc: &'d Document, n: &Node, field: &str) -> Option<&'d Variable> {
    let forms: &[&str] = match field {
        "fill" => &["fill", "fills", "fills/0", "fills/0/color"],
        "stroke" => &["stroke", "strokes/0/color"],
        _ => &[],
    };
    let found = if forms.is_empty() { n.bound.get(field) } else { forms.iter().find_map(|k| n.bound.get(*k)) };
    found.and_then(|v| doc.variables.get_by_id(v))
}

/// The variable bound to `field` on `n`, by name.
fn variable(doc: &Document, n: &Node, field: &str) -> Option<String> {
    bound_variable(doc, n, field).map(|v| v.name.clone())
}

fn style_name(doc: &Document, id: Option<&StyleId>) -> Option<String> {
    id.and_then(|s| doc.style(s)).map(|s| s.name.clone())
}

/// The properties of `id`, grouped, colours in hex; empty when it doesn't exist.
pub(crate) fn properties(doc: &Document, id: NodeId) -> Vec<Section> {
    let Some(n) = doc.node(id) else { return Vec::new() };
    let mut out = vec![layout(doc, id, n)];
    if let Some(l) = &n.layout {
        out.push(auto_layout(doc, n, l));
    }
    if let Some(t) = n.kind.text() {
        out.push(typography(doc, n, t));
    }
    // Each paint's row names the variable bound to its colour; a style names the first row.
    let paint_rows = |fills: bool, style: Option<&StyleId>| -> Vec<Prop> {
        let paints = if fills { &n.fills } else { &n.strokes };
        let rows: Vec<Prop> = paints
            .iter()
            .enumerate()
            .filter(|(_, p)| p.visible)
            .map(|(i, p)| {
                let token = crate::cmd::lists::paint_binding_key(n, fills, i).and_then(|k| variable(doc, n, k));
                paint_row(p).token(token)
            })
            .collect();
        match style_name(doc, style) {
            Some(name) => tag_first(rows, Some(name)),
            None => rows,
        }
    };
    let fills = paint_rows(true, n.styles.fill.as_ref());
    if !fills.is_empty() {
        out.push(Section { title: "Fills", rows: fills });
    }
    let mut strokes = paint_rows(false, n.styles.stroke.as_ref());
    if !strokes.is_empty() {
        let align = match n.stroke.align {
            StrokeAlign::Inside => "inside",
            StrokeAlign::Center => "center",
            StrokeAlign::Outside => "outside",
        };
        strokes.push(prop("Weight", format!("{} {align}", num(n.stroke.weight))).token(variable(doc, n, "strokeWeight")));
        out.push(Section { title: "Strokes", rows: strokes });
    }
    // A grain size: one number, or across × down.
    let grain = |(sx, sy): (f64, f64)| if sx == sy { num(sx) } else { format!("{}×{}", num(sx), num(sy)) };
    let pct = |v: f64| (v * 100.0).round();
    let effects: Vec<Prop> = n
        .effects
        .iter()
        .filter(|e| e.visible)
        .map(|e| match &e.kind {
            EffectKind::DropShadow(s) | EffectKind::InnerShadow(s) => {
                let label = if matches!(e.kind, EffectKind::DropShadow(_)) { "Drop shadow" } else { "Inner shadow" };
                let v = format!("{} {} {} {} {}", num(s.offset.0), num(s.offset.1), num(s.radius), num(s.spread), color_text(s.color));
                prop(label, v)
            }
            EffectKind::LayerBlur(b) | EffectKind::BackgroundBlur(b) => {
                let label = if matches!(e.kind, EffectKind::LayerBlur(_)) { "Layer blur" } else { "Background blur" };
                let at = |(x, y): (f64, f64)| format!("{}% {}%", pct(x), pct(y));
                let v = if b.is_progressive() {
                    format!("Progressive {} → {} from {} to {}", num(b.start_radius), num(b.radius), at(b.start), at(b.end))
                } else {
                    num(b.radius)
                };
                prop(label, v)
            }
            EffectKind::Noise(nz) => {
                let colors = match nz.mode {
                    NoiseMode::Mono => color_text(nz.color),
                    NoiseMode::Duo => format!("{} {}", color_text(nz.color), color_text(nz.secondary)),
                    NoiseMode::Multi => format!("{}%", pct(f64::from(nz.opacity))),
                };
                prop("Noise", format!("{} {} {}% {colors}", nz.mode.label(), grain(nz.grain()), pct(nz.density)))
            }
            EffectKind::Texture(t) => {
                prop("Texture", format!("{} {}{}", grain(t.grain()), num(t.radius), if t.clip_to_shape { " clipped" } else { "" }))
            }
            EffectKind::Glass(g) => prop(
                "Glass",
                format!(
                    "{}° {}% refraction {}% depth {} dispersion {}% frost {} splay {}%",
                    num(g.light_angle),
                    pct(g.light_intensity),
                    pct(g.refraction),
                    num(g.depth),
                    pct(g.dispersion),
                    num(g.sanitized().frost),
                    pct(g.splay)
                ),
            ),
        })
        .collect();
    if !effects.is_empty() {
        out.push(Section { title: "Effects", rows: tag_first(effects, style_name(doc, n.styles.effect.as_ref())) });
    }
    out
}

/// One paint's row: its kind and colour (or stops, or opacity for images).
fn paint_row(p: &figcraft_color::Paint) -> Prop {
    match &p.kind {
        PaintKind::Solid { color } => {
            let c = color.with_alpha(color.a * p.opacity);
            prop("Solid", color_text(c))
        }
        PaintKind::Gradient(g) => {
            let kind = match g.kind {
                GradientKind::Linear => "Linear",
                GradientKind::Radial => "Radial",
                GradientKind::Angular => "Angular",
                GradientKind::Diamond => "Diamond",
            };
            let stops: Vec<String> = g.stops.iter().map(|s| format!("{} {}%", color_text(s.color), (s.position * 100.0).round())).collect();
            prop(kind, stops.join(", "))
        }
        PaintKind::Image(_) => prop("Image", format!("{}%", (p.opacity * 100.0).round())),
        PaintKind::Pattern(_) => prop("Pattern", format!("{}%", (p.opacity * 100.0).round())),
    }
}

/// A style or variable applies to the whole list: show it on the first row.
fn tag_first(mut rows: Vec<Prop>, token: Option<String>) -> Vec<Prop> {
    if let Some(first) = rows.first_mut() {
        first.token = token;
    }
    rows
}

fn layout(doc: &Document, id: NodeId, n: &Node) -> Section {
    let mut rows =
        vec![prop("Width", num(n.width)).token(variable(doc, n, "width")), prop("Height", num(n.height)).token(variable(doc, n, "height"))];
    // Position from the top-left of the frame around it (groups aren't frames; the canvas
    // origin at the top level).
    if let Some(b) = doc.world_bounds(id) {
        let origin = doc.containing_frame(id).and_then(|f| doc.world_bounds(f)).map_or((0.0, 0.0), |pb| (pb.x0, pb.y0));
        rows.push(prop("Top", num(b.y0 - origin.1)));
        rows.push(prop("Left", num(b.x0 - origin.0)));
    }
    let rot = figcraft_geom::rotation_degrees(n.transform);
    if rot.abs() > 0.01 {
        rows.push(prop("Rotation", format!("{}°", num(rot))));
    }
    if n.opacity < 0.999 {
        rows.push(prop("Opacity", format!("{}%", (f64::from(n.opacity) * 100.0).round())).token(variable(doc, n, "opacity")));
    }
    if !matches!(n.blend, figcraft_color::BlendMode::Normal | figcraft_color::BlendMode::PassThrough) {
        rows.push(prop("Blend", n.blend.label()));
    }
    if n.kind.has_corner_radius() {
        let r = match n.corners.uniform() {
            Some(r) if r > 0.0 => Some(num(r)),
            Some(_) => None,
            None => Some(n.corners.radii.iter().map(|r| num(*r)).collect::<Vec<_>>().join(" ")),
        };
        if let Some(r) = r {
            rows.push(prop("Radius", r).token(variable(doc, n, "cornerRadius")));
        }
    }
    Section { title: "Layout", rows }
}

fn auto_layout(doc: &Document, n: &Node, l: &AutoLayout) -> Section {
    let dir = match (l.mode, l.wrap) {
        (LayoutMode::Horizontal, true) => "Horizontal, wrap",
        (LayoutMode::Horizontal, false) => "Horizontal",
        (LayoutMode::Vertical, true) => "Vertical, wrap",
        (LayoutMode::Vertical, false) => "Vertical",
        (LayoutMode::Grid, _) => "Grid",
    };
    let mut rows = vec![prop("Direction", dir)];
    if l.mode == LayoutMode::Grid {
        rows.push(prop("Grid", format!("{} × {}", l.grid_columns, l.grid_rows)));
        rows.push(prop("Gap", format!("{} {}", num(l.gap), num(l.cross_gap))));
    } else {
        let gap = match l.main_align {
            MainAlign::SpaceBetween => "Auto (space between)".to_string(),
            MainAlign::SpaceAround => "Auto (space around)".to_string(),
            MainAlign::SpaceEvenly => "Auto (space evenly)".to_string(),
            _ => num(l.gap),
        };
        rows.push(prop("Gap", gap).token(variable(doc, n, "gap")));
        if l.wrap {
            let label = if l.mode == LayoutMode::Vertical { "Column gap" } else { "Row gap" };
            let spread = l.content_align == ContentAlign::SpaceBetween;
            rows.push(if spread { prop(label, "Auto (space between)") } else { prop(label, num(l.cross_gap)).token(variable(doc, n, "crossGap")) });
        }
    }
    let [t, r, b, lft] = l.padding;
    let padding = if t == r && r == b && b == lft {
        num(t)
    } else if t == b && r == lft {
        format!("{} {}", num(t), num(r))
    } else {
        format!("{} {} {} {}", num(t), num(r), num(b), num(lft))
    };
    let pad_token = variable(doc, n, "padding").or_else(|| variable(doc, n, "paddingHorizontal")).or_else(|| variable(doc, n, "paddingVertical"));
    rows.push(prop("Padding", padding).token(pad_token));
    let main = match l.main_align {
        MainAlign::Min => "start",
        MainAlign::Center => "center",
        MainAlign::Max => "end",
        MainAlign::SpaceBetween => "space between",
        MainAlign::SpaceAround => "space around",
        MainAlign::SpaceEvenly => "space evenly",
    };
    let cross = match l.cross_align {
        CrossAlign::Min => "start",
        CrossAlign::Center => "center",
        CrossAlign::Max => "end",
        CrossAlign::Baseline => "baseline",
    };
    rows.push(prop("Align", format!("{main}, {cross}")));
    Section { title: "Auto layout", rows }
}

fn typography(doc: &Document, n: &Node, t: &TextData) -> Section {
    let s = &t.style;
    let token = style_name(doc, n.styles.text.as_ref());
    let mut rows = vec![
        prop("Font", s.font_family.clone()).token(token),
        prop("Style", s.font_style.clone()),
        prop("Size", num(s.font_size)).token(variable(doc, n, "fontSize")),
        prop("Line height", s.line_height.label()),
        prop("Letter spacing", s.letter_spacing.label()),
    ];
    if t.paragraph_spacing > 0.0 {
        rows.push(prop("Paragraph spacing", num(t.paragraph_spacing)));
    }
    if t.leading_trim == figcraft_doc::LeadingTrim::CapHeight {
        rows.push(prop("Vertical trim", "cap height to baseline"));
    }
    let align = match t.align_h {
        TextAlignH::Left => "left",
        TextAlignH::Center => "center",
        TextAlignH::Right => "right",
        TextAlignH::Justified => "justified",
    };
    rows.push(prop("Align", align));
    if s.decoration != TextDecoration::None {
        let l = s.look();
        let style = match l.style {
            figcraft_doc::DecorationStyle::Solid => "",
            figcraft_doc::DecorationStyle::Dotted => ", dotted",
            figcraft_doc::DecorationStyle::Wavy => ", wavy",
        };
        let kind = if s.decoration == TextDecoration::Underline { "underline" } else { "strikethrough" };
        rows.push(prop("Decoration", format!("{kind}{style}")));
    }
    if t.list_spacing > 0.0 {
        rows.push(prop("List spacing", num(t.list_spacing)));
    }
    if variable(doc, n, "characters").is_some() {
        rows.push(prop("Content", t.characters.chars().take(80).collect::<String>()).token(variable(doc, n, "characters")));
    }
    Section { title: "Typography", rows }
}
