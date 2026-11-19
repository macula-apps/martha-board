//! A large board for benchmarks (`figcraft-cli perf`, the UI frame benchmarks): a grid of sections,
//! each holding an auto layout header (a title and two icons), a group (a card), three sticky
//! notes and a shape with text joined in a row by connectors, and a grid of rectangles, ellipses
//! and text, so every layer kind the hot paths care about is on it, board objects included.

use super::connectors::{CAPS, make};
use super::items::{STICKY_TEXT_SIZE, make_shape, make_sticky};
use super::{
    AutoLayout, BOARD_COLORS, BoardShape, Color, Connector, Document, End, FileKind, NodeId, NodeKind, Paint, Result, Routing, SHAPE, STICKY, Side,
    TextData,
};

/// A section's size and the gap between sections (page units).
pub const SECTION_W: f64 = 1000.0;
pub const SECTION_H: f64 = 460.0;
pub const SECTION_GAP: f64 = 120.0;
/// Layers per section, the section included (the last section may hold fewer).
pub const PER_SECTION: usize = 50;

fn text(content: &str) -> NodeKind {
    NodeKind::Text(TextData::new(content))
}

fn ellipse() -> NodeKind {
    NodeKind::Ellipse { start: 0.0, sweep: std::f64::consts::TAU, inner: 0.0 }
}

fn fill(doc: &mut Document, id: NodeId, c: Color) {
    if let Some(n) = doc.node_mut(id) {
        n.fills = vec![Paint::solid(c)];
    }
}

/// The header: an auto layout frame of a title and two icons (4 layers).
fn header(doc: &mut Document, section: NodeId, s: usize) -> Result<()> {
    let header = doc.create(section, NodeKind::Frame, "Header", 16.0, 16.0, 220.0, 40.0)?;
    if let Some(n) = doc.node_mut(header) {
        n.layout = Some(AutoLayout { gap: 8.0, padding: [8.0; 4], ..AutoLayout::default() });
        n.fills = vec![Paint::solid(Color::from_rgb8_const(0xF5, 0xF5, 0xF5))];
    }
    doc.create(header, text(&format!("Section {}", s + 1)), "Title", 0.0, 0.0, 120.0, 16.0)?;
    for k in 0..2 {
        let icon = doc.create(header, ellipse(), &format!("Icon {}", k + 1), 0.0, 0.0, 20.0, 20.0)?;
        fill(doc, icon, Color::from_rgb8_const(0x99, 0x99, 0x99));
    }
    Ok(())
}

/// A card: a group of a background, an avatar and two lines of text (5 layers).
fn card(doc: &mut Document, section: NodeId) -> Result<()> {
    let card = doc.create(section, NodeKind::Group, "Card", 16.0, 72.0, 160.0, 64.0)?;
    let bg = doc.create(card, NodeKind::Rectangle, "Card background", 0.0, 0.0, 160.0, 64.0)?;
    fill(doc, bg, Color::from_rgb8_const(0xEE, 0xF4, 0xFF));
    doc.create(card, ellipse(), "Avatar", 8.0, 8.0, 32.0, 32.0)?;
    doc.create(card, text("Ada Lovelace"), "Name", 48.0, 8.0, 100.0, 16.0)?;
    doc.create(card, text("Analyst"), "Role", 48.0, 28.0, 100.0, 16.0)?;
    Ok(())
}

/// One cell of the grid of shapes and text (one layer); the grid starts with a rectangle.
fn cell(doc: &mut Document, section: NodeId, s: usize, k: usize) -> Result<()> {
    let (gx, gy) = (260.0 + (k % 16) as f64 * 24.0, 16.0 + (k / 16) as f64 * 34.0);
    let hue = ((s * PER_SECTION + k) % 360) as f32 / 360.0;
    let kind = (k + 2) % 5;
    let id = match kind {
        0 => doc.create(section, text(&format!("Item {k}")), "Text", gx, gy, 20.0, 14.0)?,
        1 | 3 => doc.create(section, ellipse(), "Ellipse", gx, gy, 18.0, 18.0)?,
        _ => doc.create(section, NodeKind::Rectangle, "Rectangle", gx, gy, 18.0, 18.0)?,
    };
    if kind != 0 {
        fill(doc, id, Color::new(hue, 0.5, 1.0 - hue, 1.0));
        if let Some(n) = doc.node_mut(id) {
            n.corners.radii = [3.0; 4];
        }
    }
    Ok(())
}

/// A connector from the right side of `from` to the left side of `to`, on the page (one layer).
fn join(doc: &mut Document, page: NodeId, from: NodeId, to: NodeId) -> Result<()> {
    let end = |node, side| End::Node { node, side, at: None };
    let c = Connector { from: end(from, Side::Right), to: end(to, Side::Left), routing: Routing::Straight, path: None, branch: false };
    make(doc, page, c, CAPS).map(drop)
}

/// The sticky notes and the shape of section `s`, joined in a row (2 layers a sticky, 3 the shape,
/// one a connector), as far as `budget` goes while leaving one layer for the grid.
fn row(doc: &mut Document, page: NodeId, section: NodeId, s: usize, budget: &mut usize) -> Result<()> {
    let color = BOARD_COLORS.get(s % BOARD_COLORS.len()).map_or(Color::WHITE, |c| c.1);
    let mut last: Option<NodeId> = None;
    for k in 0..4 {
        let at = (16.0 + k as f64 * (STICKY + 40.0), 220.0);
        let cost = if k < 3 { 2 } else { 3 } + usize::from(last.is_some());
        if *budget < cost + 1 {
            break;
        }
        let made = if k < 3 {
            make_sticky(doc, section, at, &format!("Idea {}", k + 1), color, "", STICKY_TEXT_SIZE)?.0
        } else {
            make_shape(doc, section, at, (SHAPE, SHAPE), BoardShape::Diamond, "Decide", color)?.0
        };
        if let Some(prev) = last {
            join(doc, page, prev, made)?;
        }
        *budget -= cost;
        last = Some(made);
    }
    Ok(())
}

/// A board of about `layers` layers (pages not counted), normalised like an opened file. Returns
/// the document and its page.
pub fn large_board(layers: usize) -> Result<(Document, NodeId)> {
    let mut doc = Document::new("Large board");
    doc.kind = FileKind::Board;
    let page = doc.first_page().ok_or_else(|| figcraft_doc::DocError::Invalid("a new document has no page".into()))?;
    let layers = layers.clamp(10, 2_000_000);
    let sections = layers.div_ceil(PER_SECTION).max(1);
    let cols = ((sections as f64).sqrt().ceil() as usize).max(1);
    let mut budget = layers;
    for s in 0..sections {
        let (x, y) = ((s % cols) as f64 * (SECTION_W + SECTION_GAP), (s / cols) as f64 * (SECTION_H + SECTION_GAP));
        let section = doc.create(page, NodeKind::Section { contents_hidden: false }, &format!("Section {}", s + 1), x, y, SECTION_W, SECTION_H)?;
        budget -= 1;
        let start = budget;
        if budget > 4 {
            header(&mut doc, section, s)?;
            budget -= 4;
        }
        if budget > 5 {
            card(&mut doc, section)?;
            budget -= 5;
        }
        row(&mut doc, page, section, s, &mut budget)?;
        // The rest of the section's layers: the grid.
        let grid = (PER_SECTION - 1).saturating_sub(start - budget).min(budget);
        for k in 0..grid {
            cell(&mut doc, section, s, k)?;
        }
        budget -= grid;
        if budget == 0 {
            break;
        }
    }
    // Settle derived state (text sizes, group bounds, auto layout, connector routes) as after
    // opening a file.
    let empty = Document::new("");
    crate::normalize(&mut doc, &empty);
    Ok((doc, page))
}

#[cfg(test)]
mod tests {
    use figcraft_doc::{BoardItem, Node, NodeKind};

    #[test]
    fn builds_about_the_requested_size() {
        for n in [10, 100, 1000, 2500] {
            let (doc, page) = super::large_board(n).unwrap();
            let layers = doc.node_count() - 1;
            assert!(layers >= n && layers <= n + 3, "{n} → {layers}");
            assert!(doc.validate().is_empty(), "{n}: {:?}", doc.validate());
            assert!(doc.page_content_bounds(page).is_some());
        }
    }

    #[test]
    fn documents_hold_the_benchmark_targets() {
        let has = |doc: &figcraft_doc::Document, f: &dyn Fn(&Node) -> bool| doc.nodes().any(f);
        for n in [10, 11, 12, 13, 14, 15, 100] {
            let (doc, _) = super::large_board(n).unwrap();
            assert!(has(&doc, &|x| matches!(x.kind, NodeKind::Rectangle) && x.name == "Rectangle"), "{n}: no rectangle");
            assert!(has(&doc, &|x| x.name == "Title"), "{n}: no title");
        }
        let (doc, _) = super::large_board(100).unwrap();
        assert!(has(&doc, &|x| matches!(x.board, Some(BoardItem::Sticky { .. }))), "no sticky");
        assert!(has(&doc, &|x| matches!(x.board, Some(BoardItem::Connector(_)))), "no connector");
        assert!(has(&doc, &|x| matches!(x.board, Some(BoardItem::Shape { .. }))), "no shape");
    }
}
