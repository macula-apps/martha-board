//! The cursor each tool asks for (`tool.overlay.cursor`), by tool, modifier and what the pointer
//! is over: tables of tool × modifiers × hover target → cursor, the cursors of gestures under way
//! (panning, duplicating, rotating).

use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::tools::{Cursor, PointerEvent, PointerKind, ToolId};
use figcraft_doc::NodeId;

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn id(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

#[derive(Clone, Copy, Default)]
struct Mods {
    shift: bool,
    alt: bool,
    cmd: bool,
}

const NONE: Mods = Mods { shift: false, alt: false, cmd: false };
const ALT: Mods = Mods { shift: false, alt: true, cmd: false };

fn ev(kind: PointerKind, (x, y): (f64, f64), m: Mods) -> PointerEvent {
    PointerEvent { shift: m.shift, alt: m.alt, cmd: m.cmd, ..PointerEvent::at(kind, x, y, 1.0) }
}

fn hover(s: &mut Session, at: (f64, f64), m: Mods) -> Cursor {
    s.pointer(&ev(PointerKind::Hover, at, m)).unwrap();
    s.tool.overlay.cursor
}

/// A resize cursor's direction, 0–180° (it has two heads).
fn resize(c: Cursor) -> f64 {
    match c {
        Cursor::Resize(a) => a.rem_euclid(180.0),
        other => panic!("not a resize cursor: {other:?}"),
    }
}

fn rotate(c: Cursor) -> f64 {
    match c {
        Cursor::Rotate(a) => a.rem_euclid(360.0),
        other => panic!("not a rotate cursor: {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A square (100,100)–(200,200), selected, and the text "Hello" at (400, 100), not selected.
fn scene() -> (Session, NodeId, NodeId) {
    // The text is measured with a font: without craft-fonts that is a system one, scanned here
    // rather than by whichever test happens to run first (#333).
    figcraft_text::scan_system_fonts();
    let mut s = Session::new();
    let r = id(&ex(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 100, "height": 100})));
    let t = id(&ex(&mut s, "text.create", json!({"text": "Hello", "x": 400, "y": 100, "props": {"fontSize": 40}})));
    ex(&mut s, "select.set", json!({"ids": [r]}));
    (s, r, t)
}

const EMPTY: (f64, f64) = (700.0, 600.0);
const ON_SQUARE: (f64, f64) = (150.0, 150.0);
const ON_SQUARE_EDGE: (f64, f64) = (101.0, 150.0);
const ON_TEXT: (f64, f64) = (420.0, 120.0);

#[test]
fn each_tool_has_its_cursor_by_modifier_and_what_is_under_the_pointer() {
    use Cursor::*;
    // (tool, modifiers, where, cursor, why)
    type Row = (ToolId, Mods, (f64, f64), Cursor, &'static str);
    let rows: Vec<Row> = vec![
        (ToolId::Move, NONE, EMPTY, Default, "the arrow over empty canvas"),
        (ToolId::Move, NONE, ON_SQUARE, Default, "the arrow over a layer"),
        (ToolId::Move, NONE, ON_TEXT, Default, "the arrow over a text that isn't being edited"),
        (ToolId::Move, ALT, ON_SQUARE, Copy, "Alt over a selected layer: a drag makes a copy"),
        (ToolId::Move, ALT, ON_TEXT, Default, "Alt over another layer measures to it"),
        (ToolId::Move, ALT, EMPTY, Default, "Alt over empty canvas"),
        (ToolId::Scale, NONE, ON_SQUARE, Default, "the Scale tool away from the handles"),
        (ToolId::Scale, ALT, ON_SQUARE, Default, "the Scale tool doesn't duplicate"),
        (ToolId::Hand, NONE, EMPTY, Grab, "the open hand"),
        (ToolId::Hand, NONE, ON_SQUARE, Grab, "the open hand over a layer too"),
        (ToolId::Frame, NONE, EMPTY, Crosshair, "frame"),
        (ToolId::Section, NONE, EMPTY, Crosshair, "section"),
        (ToolId::Slice, NONE, EMPTY, Crosshair, "slice"),
        (ToolId::Rectangle, NONE, ON_SQUARE, Crosshair, "shapes are drawn over layers too"),
        (ToolId::Line, NONE, EMPTY, Crosshair, "line"),
        (ToolId::Arrow, NONE, EMPTY, Crosshair, "arrow"),
        (ToolId::Ellipse, NONE, EMPTY, Crosshair, "ellipse"),
        (ToolId::Polygon, NONE, EMPTY, Crosshair, "polygon"),
        (ToolId::Star, NONE, EMPTY, Crosshair, "star"),
        (ToolId::Text, NONE, EMPTY, Crosshair, "the Text tool makes a text where it is pressed"),
        (ToolId::Text, NONE, ON_SQUARE, Crosshair, "over a shape too"),
        (ToolId::Text, NONE, ON_TEXT, Text, "the I-beam over a text: a click edits it there"),
        (ToolId::TextOnPath, NONE, EMPTY, Default, "text on a path: a click on empty canvas does nothing"),
        (ToolId::TextOnPath, NONE, ON_SQUARE, Default, "nor inside a shape, away from its path"),
        (ToolId::TextOnPath, NONE, ON_SQUARE_EDGE, Text, "the I-beam over a shape's path: a click puts text on it"),
        (ToolId::Comment, NONE, EMPTY, Comment, "the comment pin"),
        (ToolId::Eyedropper, NONE, ON_SQUARE, Eyedropper, "the eyedropper"),
    ];
    for (tool, m, at, want, why) in rows {
        let (mut s, ..) = scene();
        s.set_tool(tool);
        assert_eq!(s.tool.overlay.cursor, tool.cursor(), "{tool:?}: its cursor shows as soon as it is chosen");
        assert_eq!(hover(&mut s, at, m), want, "{tool:?} at {at:?}: {why}");
    }
    // Every tool has a row above.
    for t in ToolId::ALL {
        assert!(matches!(t.cursor(), Default | Crosshair | Grab | Comment | Eyedropper), "{t:?}");
    }
}

#[test]
fn handles_show_arrows_that_follow_the_handle_and_the_boxs_rotation() {
    let (mut s, r, _) = scene();
    // (where, the arrow's direction in degrees: 0 = left-right, 45 = top left to bottom right)
    let handles = [((200.0, 150.0), 0.0), ((150.0, 100.0), 90.0), ((200.0, 200.0), 45.0), ((100.0, 200.0), 135.0), ((100.0, 100.0), 45.0)];
    for (at, want) in handles {
        let got = resize(hover(&mut s, at, NONE));
        assert!(close(got, want), "handle at {at:?}: {got}° (wanted {want}°)");
    }
    // Just outside each corner: the rotate cursor, turned to that corner (from the centre).
    for (at, want) in [((208.0, 208.0), 45.0), ((92.0, 208.0), 135.0), ((92.0, 92.0), 225.0), ((208.0, 92.0), 315.0)] {
        let got = rotate(hover(&mut s, at, NONE));
        assert!(close(got, want), "outside the corner at {at:?}: {got}° (wanted {want}°)");
    }
    // The Scale tool's handles too.
    s.set_tool(ToolId::Scale);
    assert!(close(resize(hover(&mut s, (200.0, 150.0), NONE)), 0.0));
    s.set_tool(ToolId::Move);
    // A box turned by 30° turns its cursors by 30°: the handle that was at the right edge.
    ex(&mut s, "node.set", json!({"id": r, "rotation": 30}));
    let corners = crate::tools::selection_box(s.doc().unwrap(), &[r]).unwrap();
    let mid = |a: usize, b: usize| ((corners[a].x + corners[b].x) / 2.0, (corners[a].y + corners[b].y) / 2.0);
    let edge = resize(hover(&mut s, mid(1, 2), NONE));
    assert!(close(edge, 30.0) || close(edge, 150.0), "the right edge's arrow turned with the box: {edge}°");
    let centre = mid(0, 2);
    let c = corners[2];
    let out = (c.x + (c.x - centre.0) * 0.15, c.y + (c.y - centre.1) * 0.15);
    let want = (c.y - centre.1).atan2(c.x - centre.0).to_degrees().rem_euclid(360.0);
    let got = rotate(hover(&mut s, out, NONE));
    assert!(close(got, want) && !close(got, 45.0), "the rotate cursor points away from the turned box's centre: {got}° (wanted {want}°)");
    // A locked layer has no handles: the arrow.
    ex(&mut s, "node.set", json!({"id": r, "rotation": 0, "locked": true}));
    assert_eq!(hover(&mut s, (200.0, 150.0), NONE), Cursor::Default);
}

#[test]
fn gestures_show_what_they_are_doing() {
    // Panning: the hand closes.
    let (mut s, r, _) = scene();
    s.set_tool(ToolId::Hand);
    s.pointer(&ev(PointerKind::Down, EMPTY, NONE)).unwrap();
    assert_eq!(s.tool.overlay.cursor, Cursor::Grabbing);
    s.pointer(&ev(PointerKind::Up, EMPTY, NONE)).unwrap();
    assert_eq!(hover(&mut s, EMPTY, NONE), Cursor::Grab, "and opens again");
    // Alt-drag: the copy cursor from the press to the release; a plain drag: the arrow.
    for (m, want) in [(ALT, Cursor::Copy), (NONE, Cursor::Default)] {
        let (mut s, ..) = scene();
        s.pointer(&ev(PointerKind::Down, ON_SQUARE, m)).unwrap();
        assert_eq!(s.tool.overlay.cursor, want, "on the press");
        s.pointer(&ev(PointerKind::Move, (180.0, 170.0), m)).unwrap();
        s.pointer(&ev(PointerKind::Move, (260.0, 190.0), m)).unwrap();
        assert_eq!(s.tool.overlay.cursor, want, "while dragging");
        s.pointer(&ev(PointerKind::Up, (260.0, 190.0), m)).unwrap();
    }
    // Rotating: the cursor turns with the pointer about the centre.
    s.set_tool(ToolId::Move);
    ex(&mut s, "select.set", json!({"ids": [r]}));
    s.pointer(&ev(PointerKind::Down, (208.0, 208.0), NONE)).unwrap();
    for (at, want) in [((150.0, 260.0), 90.0), ((40.0, 150.0), 180.0), ((150.0, 40.0), 270.0)] {
        s.pointer(&ev(PointerKind::Move, at, NONE)).unwrap();
        let got = rotate(s.tool.overlay.cursor);
        assert!(close(got, want), "rotating with the pointer at {at:?}: {got}° (wanted {want}°)");
    }
    s.pointer(&ev(PointerKind::Up, (150.0, 40.0), NONE)).unwrap();
    // While a text is edited, the I-beam over it (with the Move tool) and the arrow elsewhere.
    let (mut s, _, t) = scene();
    ex(&mut s, "text.edit.begin", json!({"id": t}));
    assert_eq!(hover(&mut s, ON_TEXT, NONE), Cursor::Text);
    assert_eq!(hover(&mut s, EMPTY, NONE), Cursor::Default);
    ex(&mut s, "text.edit.end", json!({}));
    assert_eq!(hover(&mut s, ON_TEXT, NONE), Cursor::Default, "editing over: the arrow");
}
