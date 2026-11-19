//! Brushes through the session: `stroke.brush` and rendering brush strokes, and the marker and
//! highlighter keeping their ink where it was drawn.

use serde_json::{Value, json};

use crate::Session;
use crate::params::id_of;
use crate::tools::{PointerEvent, PointerKind};
use figcraft_doc::{NodeId, NodeKind};
use figcraft_geom::Point;
use figcraft_geom::kurbo::PathEl;

fn id(v: Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

/// A 200-long horizontal vector with a 10 px black stroke.
fn line(s: &mut Session) -> NodeId {
    let v = id(s.execute("node.create", &json!({"type": "vector", "path": "M0 0 L200 0", "x": 0, "y": 0})).unwrap());
    s.execute("node.set", &json!({"id": v, "props": {"strokes": ["#000000"], "strokeWeight": 10, "strokeAlign": "center"}})).unwrap();
    v
}

/// The alpha of the page's layers at page point (`x`, `y`), rendered at 1x without the page colour.
fn alpha(s: &Session, x: f64, y: f64) -> u8 {
    let doc = s.doc().unwrap();
    let view = figcraft_render::View { origin: (x - 1.0, y - 1.0), scale: 1.0, width: 3, height: 3 };
    let mut pix = figcraft_render::Pixmap::new(3, 3).unwrap();
    figcraft_render::render_nodes_into(doc, &doc.node(s.page().unwrap()).unwrap().children, &view, &mut pix);
    pix.data()[(3 + 1) * 4 + 3]
}

#[test]
fn brushes_draw_along_the_stroke() {
    let mut s = Session::new();
    let v = line(&mut s);
    assert_eq!(alpha(&s, 15.0, 0.0), 255, "a plain stroke covers the line");
    assert!(s.execute("stroke.brush", &json!({"ids": [v], "preset": "glitter"})).is_err());
    // Dots every 2 weights: centres at 10, 30, 50 … with gaps between.
    s.execute("stroke.brush", &json!({"ids": [v], "preset": "dots", "spacing": 2})).unwrap();
    assert_eq!(alpha(&s, 10.0, 0.0), 255);
    assert_eq!(alpha(&s, 20.0, 0.0), 0, "the gap between two dots");
    let b = s.execute("node.get", &json!({"id": v})).unwrap()["brush"].clone();
    assert_eq!((b["preset"].as_str(), b["spacing"].as_f64()), (Some("dots"), Some(2.0)), "{b}");
    // A stretch brush tapers to a point at both ends.
    s.execute("stroke.brush", &json!({"ids": [v], "preset": "taper"})).unwrap();
    assert_eq!(alpha(&s, 100.0, 2.0), 255);
    assert_eq!(alpha(&s, 2.0, 3.0), 0);
    // Removing the brush: a plain stroke again.
    let mut s = Session::new();
    let v = line(&mut s);
    s.execute("stroke.brush", &json!({"ids": [v], "preset": "dots", "spacing": 2})).unwrap();
    s.execute("stroke.brush", &json!({"ids": [v], "preset": null})).unwrap();
    assert_eq!(alpha(&s, 20.0, 0.0), 255);
    // Exports draw it too.
    s.execute("stroke.brush", &json!({"ids": [v], "preset": "stars"})).unwrap();
    let doc = s.doc().unwrap();
    assert!(figcraft_svg::export_svg(doc, &[v], &figcraft_svg::SvgOptions::default()).unwrap().contains("<path"));
    assert!(figcraft_pdf::export_nodes_pdf(doc, &[v]).is_ok());
}

/// How far `path` strays from `from`: the largest distance from a point sampled along `path` to
/// the nearest segment of `from`.
fn strays(path: &figcraft_geom::BezPath, from: &figcraft_geom::BezPath) -> f64 {
    use figcraft_geom::kurbo::{ParamCurve, ParamCurveNearest};
    let segs: Vec<_> = from.segments().collect();
    path.segments()
        .flat_map(|seg| (0..=32).map(move |i| seg.eval(f64::from(i) / 32.0)))
        .map(|p| segs.iter().map(|s| s.nearest(p, 1e-6).distance_sq).fold(f64::INFINITY, f64::min).sqrt())
        .fold(0.0, f64::max)
}

/// A marker or highlighter stroke stays where it was drawn: the fitted curve shot far off the
/// stroke, a spike (#1741, its second point), where the stroke went out and back to the very point
/// it left and where samples ran on nearly straight. Pointer pixels become fractional page points,
/// so going back to a pixel lands on exactly the same point. Both sample runs are ones kurbo's fit
/// took hundreds of units off.
///
/// Each stroke is drawn in a fresh board session. Every point along the stroke drawn must lie
/// within 1.5 (twice the fit's accuracy) of the polyline through the samples.
#[test]
fn ink_keeps_its_stroke_where_it_was_drawn() {
    let (p, q) = ((86.043_565_719_086_97, -173.744_402_349_100_94), (87.851_850_739_335_35, -179.202_846_445_954_52));
    let back = [(80.0, -160.0), p, q, p, (95.0, -185.0)];
    let straight = [
        (204.0, -198.0),
        (209.598_690_794_015_5, -204.909_328_119_739_88),
        (211.277_914_263_870_97, -207.190_974_020_753_4),
        (214.246_506_828_720_84, -211.228_827_590_623_24),
        (218.0, -214.0),
    ];
    for tool in ["marker", "highlighter"] {
        for (what, samples) in [("out and back", &back[..]), ("nearly straight", &straight[..])] {
            let what = format!("{tool}, {what}");
            let mut s = crate::board_tests::board();
            s.execute("tool.set", &json!({"tool": tool})).unwrap();
            let before: Vec<NodeId> = s.doc().unwrap().nodes().map(|n| n.id).collect();
            let (first, last) = (samples[0], samples[samples.len() - 1]);
            s.pointer(&PointerEvent::at(PointerKind::Down, first.0, first.1, 1.0)).unwrap();
            for (x, y) in &samples[1..] {
                s.pointer(&PointerEvent::at(PointerKind::Move, *x, *y, 1.0)).unwrap();
            }
            s.pointer(&PointerEvent::at(PointerKind::Up, last.0, last.1, 1.0)).unwrap();
            // The layer the stroke made, and only it.
            let doc = s.doc().unwrap();
            let made: Vec<&figcraft_doc::Node> =
                doc.nodes().filter(|n| !before.contains(&n.id) && matches!(n.kind, NodeKind::Vector { .. })).collect();
            let [n] = made[..] else { panic!("{what}: {} vectors made", made.len()) };
            let NodeKind::Vector { network } = &n.kind else { panic!("{what}: not a vector") };
            let drawn = doc.world_transform(n.id) * network.stroke_path();
            let polyline = figcraft_geom::BezPath::from_vec(
                samples
                    .iter()
                    .enumerate()
                    .map(|(k, (x, y))| if k == 0 { PathEl::MoveTo(Point::new(*x, *y)) } else { PathEl::LineTo(Point::new(*x, *y)) })
                    .collect(),
            );
            let d = strays(&drawn, &polyline);
            assert!(d <= 1.5, "{what}: the stroke strays {d} from the samples: {}", drawn.to_svg());
            // The turn is a point of the stroke, where it was drawn.
            if samples == back {
                let xf = doc.world_transform(n.id);
                assert!(
                    network.vertices.iter().any(|v| (xf * Point::new(v.x, v.y) - Point::new(q.0, q.1)).hypot() < 1e-6),
                    "{what}: no point at the turn"
                );
            }
        }
    }
}
