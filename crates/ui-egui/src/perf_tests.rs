//! UI frame benchmarks on large documents (ignored by default; `cargo xtask bench` runs them in
//! release): the whole UI drawn headless through egui, timing idle frames, hovering, panning and a
//! layers panel with every screen expanded. Also the per-document-version caches that keep those
//! frames cheap: they must follow edits, undo, page switches and the layers panel's state.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, vec2};
use figcraft_doc::{Document, NodeId};
use figcraft_engine::Session;
use serde_json::json;

use crate::{FigApp, NoServices};

const SCREEN: (f32, f32) = (1920.0, 1080.0);

struct Bench {
    ctx: egui::Context,
    app: FigApp,
    time: f64,
}

impl Bench {
    fn new(layers: usize) -> Self {
        let (doc, _) = figcraft_engine::board::large_board(layers).unwrap();
        Self::with_document(doc)
    }

    fn with_document(doc: Document) -> Self {
        figcraft_text::scan_system_fonts();
        let mut b = Self { ctx: egui::Context::default(), app: FigApp::new(Session::with_document(doc), Box::new(NoServices)), time: 0.0 };
        // Fonts install on the first frame; the next ones lay out and settle.
        for _ in 0..4 {
            b.frame(vec![]);
        }
        b
    }

    /// One UI frame with `events`; returns its wall time in ms.
    fn frame(&mut self, events: Vec<Event>) -> f64 {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(SCREEN.0, SCREEN.1))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let t = Instant::now();
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(input, |ui| app.ui(ui));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        // No GPU here to take the texture uploads.
        out.textures_delta.clear();
        ms
    }

    /// Mean frame ms over `n` frames, events from `ev(i)`.
    fn mean(&mut self, n: usize, ev: impl Fn(usize) -> Vec<Event>) -> f64 {
        (0..n).map(|i| self.frame(ev(i))).sum::<f64>() / n as f64
    }

    fn canvas_point(&self, i: usize) -> Pos2 {
        let r = self.app.canvas.rect;
        let (fx, fy) = ((i * 7919 % 1000) as f32 / 1000.0, (i * 104_729 % 1000) as f32 / 1000.0);
        Pos2::new(r.min.x + 20.0 + (r.width() - 40.0) * fx, r.min.y + 20.0 + (r.height() - 40.0) * fy)
    }
}

/// Build and print a synthetic UI benchmark report for the given layer count; it makes no
/// assertions.
fn report(layers: usize) {
    let mut b = Bench::new(layers);
    let mut rows: Vec<(&str, f64)> = Vec::new();
    rows.push(("idle frame, nothing selected", b.mean(30, |_| vec![])));
    let hover = |b: &Bench, i: usize| vec![Event::PointerMoved(b.canvas_point(i))];
    let pts: Vec<Vec<Event>> = (0..60).map(|i| hover(&b, i)).collect();
    rows.push(("hover over the canvas", b.mean(60, |i| pts[i].clone())));
    let wheel = |d: f32| Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        source: egui::MouseWheelSource::Unknown,
        delta: vec2(0.0, d),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    };
    let centre = b.app.canvas.rect.center();
    rows.push(("pan (wheel) over the canvas", b.mean(60, |i| vec![Event::PointerMoved(centre), wheel(if i % 20 < 10 { -40.0 } else { 40.0 })])));
    // One layer selected: the design panel shows its properties.
    let screen = b.app.session.doc().ok().and_then(|d| d.node(b.app.session.page().ok()?)?.children.first().copied());
    let first = screen.and_then(|s| b.app.session.doc().ok()?.node(s)?.children.last().copied());
    b.app.session.set_selection(first.into_iter().collect());
    rows.push(("idle frame, one layer selected", b.mean(30, |_| vec![])));
    b.app.session.set_selection(vec![]);
    // Every screen expanded in the layers panel.
    let page = b.app.session.page().unwrap();
    let screens = b.app.session.doc().unwrap().node(page).unwrap().children.clone();
    b.app.ui.expanded.extend(screens);
    b.frame(vec![]);
    rows.push(("idle frame, every screen expanded in layers", b.mean(30, |_| vec![])));
    let panel = Pos2::new(150.0, 600.0);
    rows.push(("scroll the expanded layers panel", b.mean(60, |i| vec![Event::PointerMoved(panel), wheel(if i % 30 < 15 { -60.0 } else { 60.0 })])));
    b.app.ui.layer_search = true;
    b.app.ui.layer_filter = "item".into();
    b.frame(vec![]);
    rows.push(("idle frame, layers filtered by name", b.mean(30, |_| vec![])));
    b.app.ui.layer_search = false;
    b.app.ui.layer_filter.clear();
    // Long lists from the keyboard: every layer expanded, the arrows walking the rows, a layer at
    // the bottom of the list revealed, everything collapsed.
    let open: Vec<NodeId> = b.app.session.doc().unwrap().nodes().filter(|n| !n.children.is_empty()).map(|n| n.id).collect();
    b.app.ui.expanded.extend(open);
    rows.push(("first frame, every layer expanded", b.frame(vec![])));
    b.app.ui.layers_focus = true;
    let top = b.app.session.doc().ok().and_then(|d| d.node(page)?.children.last().copied());
    b.app.session.set_selection(top.into_iter().collect());
    b.frame(vec![]);
    rows.push(("arrow key down the expanded layers", b.mean(30, |_| key(Key::ArrowDown, Modifiers::NONE))));
    b.app.ui.expanded.clear();
    b.frame(vec![]);
    b.app.session.set_selection(bottom_layer(&b.app.session).into_iter().collect());
    rows.push(("reveal a layer at the bottom of the list", b.frame(vec![])));
    b.app.run("layers.collapseAll", json!({}));
    rows.push(("collapse all layers", b.frame(vec![])));
    println!("UI frame ms — {layers} layers ({})", if cfg!(debug_assertions) { "debug" } else { "release" });
    for (name, v) in rows {
        println!("| {name:<44} | {v:>8.2} |");
    }
}

/// A key press and release.
fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

/// The first screen's header title: the layers panel lists the top of the stack first, so it is
/// near the bottom of the list.
fn bottom_layer(s: &Session) -> Option<NodeId> {
    let doc = s.doc().ok()?;
    let screen = *doc.node(s.page().ok()?)?.children.first()?;
    let header = *doc.node(screen)?.children.first()?;
    doc.node(header)?.children.first().copied()
}

#[test]
fn a_layer_revealed_at_the_bottom_of_10k_layers_gets_the_keyboard_on_its_row() {
    let mut b = Bench::new(10_000);
    let title = bottom_layer(&b.app.session).unwrap();
    b.app.ui.expanded.clear();
    b.app.session.set_selection(vec![title]);
    // The list scrolls there (animated over a few frames).
    for _ in 0..40 {
        b.frame(vec![]);
    }
    let doc = b.app.session.doc().unwrap();
    assert!(doc.ancestors(title).iter().filter(|a| doc.node(**a).is_some_and(|n| !n.is_page())).all(|a| b.app.ui.expanded.contains(a)));
    // F6 to the layers panel lands on the row, which is only drawn when it was scrolled into view.
    b.frame(key(Key::F6, Modifiers::SHIFT));
    b.frame(vec![]);
    assert_eq!(crate::a11y::focused_name(&b.ctx), "Title");
    // The arrows walk on from it and the keyboard follows.
    b.frame(key(Key::ArrowUp, Modifiers::NONE));
    for _ in 0..3 {
        b.frame(vec![]);
    }
    let sel = b.app.session.selection();
    assert!(sel.len() == 1 && sel != vec![title], "the row above is selected");
    let above = b.app.session.doc().unwrap().node(sel[0]).unwrap().name.clone();
    assert_eq!(crate::a11y::focused_name(&b.ctx), above);
}

#[test]
#[ignore = "benchmark: cargo xtask bench"]
fn perf_ui_frames() {
    let sizes: Vec<usize> = std::env::var("FIGCRAFT_BENCH_LAYERS")
        .ok()
        .map(|s| s.split(',').filter_map(|v| v.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1000, 10_000]);
    for n in sizes {
        report(n);
    }
}

#[test]
fn bench_harness_draws_frames() {
    let mut b = Bench::new(200);
    assert!(b.frame(vec![]) >= 0.0);
    assert!(b.app.canvas.rect.width() > 100.0, "the canvas was laid out");
}

/// Layer rows through the panel's cache, in a frame of `ctx` (the cache lives in its memory).
fn rows(ctx: &egui::Context, doc: &Arc<Document>, page: NodeId, expanded: &BTreeSet<NodeId>, filter: &str) -> Arc<Vec<crate::layers::Row>> {
    let mut out = Arc::new(Vec::new());
    ctx.run_ui(RawInput::default(), |ui| out = crate::layers::rows_for(ui, doc, page, expanded, filter)).textures_delta.clear();
    out
}

fn ids(rows: &[crate::layers::Row]) -> Vec<NodeId> {
    rows.iter().map(|r| r.id).collect()
}

#[test]
fn layer_rows_follow_edits_undo_pages_expansion_and_filter() {
    let ctx = egui::Context::default();
    let mut s = Session::new();
    let page = s.page().unwrap();
    let frame = s.execute("node.create", &json!({"type": "frame", "x": 0, "y": 0, "width": 100, "height": 100})).unwrap()["id"].as_u64().unwrap();
    let frame = NodeId(frame);
    let rect = NodeId(
        s.execute("node.create", &json!({"type": "rectangle", "parent": frame, "x": 0, "y": 0, "width": 10, "height": 10})).unwrap()["id"]
            .as_u64()
            .unwrap(),
    );
    let mut expanded = BTreeSet::new();
    let doc1 = s.ds().unwrap().doc.clone();
    let first = rows(&ctx, &doc1, page, &expanded, "");
    assert_eq!(ids(&first), vec![frame]);
    assert!(Arc::ptr_eq(&first, &rows(&ctx, &doc1, page, &expanded, "")), "same inputs: the cached rows");
    // Expanding shows the children.
    expanded.insert(frame);
    assert_eq!(ids(&rows(&ctx, &doc1, page, &expanded, "")), vec![frame, rect]);
    // An edit adds a row; undo takes it away again.
    let ell =
        NodeId(s.execute("node.create", &json!({"type": "ellipse", "x": 200, "y": 0, "width": 10, "height": 10})).unwrap()["id"].as_u64().unwrap());
    let doc2 = s.ds().unwrap().doc.clone();
    assert_eq!(ids(&rows(&ctx, &doc2, page, &expanded, "")), vec![ell, frame, rect]);
    s.undo().unwrap();
    let undone = s.ds().unwrap().doc.clone();
    assert_eq!(ids(&rows(&ctx, &undone, page, &expanded, "")), vec![frame, rect]);
    // The filter lists matches only.
    assert_eq!(ids(&rows(&ctx, &doc2, page, &expanded, "ellipse")), vec![ell]);
    assert_eq!(ids(&rows(&ctx, &doc2, page, &expanded, "")), vec![ell, frame, rect]);
    // Another page (added after the undo: no ellipse).
    s.execute("page.add", &json!({})).unwrap();
    let p2 = s.page().unwrap();
    assert_ne!(p2, page);
    let doc3 = s.ds().unwrap().doc.clone();
    assert!(rows(&ctx, &doc3, p2, &expanded, "").is_empty());
    assert_eq!(ids(&rows(&ctx, &doc3, page, &expanded, "")), vec![frame, rect]);
    // Collapsing hides the children again.
    expanded.clear();
    assert_eq!(ids(&rows(&ctx, &doc3, page, &expanded, "")), vec![frame]);
}
