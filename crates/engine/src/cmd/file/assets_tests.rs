//! The assets `file.exportLayers {assets: true}` writes.

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::AssetKind;
use crate::Session;
use crate::params::id_of;

fn id(v: Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn pid(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn set(s: &mut Session, ids: &[NodeId], props: Value) {
    ex(s, "node.set", json!({"ids": ids, "props": props}));
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64, parent: Option<NodeId>) -> NodeId {
    let mut p = json!({"x": x, "y": y, "width": w, "height": h});
    if let Some(parent) = parent {
        p["parent"] = json!(parent);
    }
    let r = pid(&ex(s, "shape.rectangle", p));
    set(s, &[r], json!({"fills": ["#FF0000"]}));
    r
}

#[test]
fn assets_list_layers_with_export_settings_under_the_selection() {
    let mut s = Session::new();
    let f = id(s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 100, "height": 100})).unwrap());
    let icon = id(s.execute("shape.ellipse", &json!({"x": 10, "y": 10, "width": 20, "height": 20, "parent": f})).unwrap());
    s.execute("shape.rectangle", &json!({"x": 40, "y": 10, "width": 20, "height": 20, "parent": f})).unwrap();
    s.execute("export.add", &json!({"ids": [icon]})).unwrap();
    s.execute("export.add", &json!({"ids": [icon]})).unwrap();
    let doc = s.doc().unwrap();
    let a = super::assets(doc, &[f]);
    assert_eq!(a.len(), 1, "only the layer with settings");
    assert_eq!(a[0].id, icon);
    assert_eq!(a[0].settings.len(), 2);
    assert!(a[0].settings[0].size.label().ends_with("1x"), "{:?}", a[0].settings);
    assert!(super::assets(s.doc().unwrap(), &[]).is_empty());
}

// Moved from the inspect behaviour suite when Dev Mode left (#11).
#[test]
fn asset_list_holds_exports_images_and_icons() {
    let mut s = Session::new();
    let f = pid(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 400, "height": 400})));
    let marked = rect(&mut s, 10.0, 10.0, 50.0, 50.0, Some(f));
    ex(&mut s, "export.add", json!({"ids": [marked]}));
    let png = figcraft_render::encode_png(&figcraft_render::Pixmap::new(2, 2).unwrap()).unwrap();
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    let photo = pid(&ex(&mut s, "file.placeImage", json!({"data": data, "x": 500, "y": 0})));
    let v1 = pid(&ex(&mut s, "node.create", json!({"type": "star", "x": 200, "y": 10, "width": 12, "height": 12, "parent": f})));
    let v2 = pid(&ex(&mut s, "node.create", json!({"type": "polygon", "x": 214, "y": 10, "width": 12, "height": 12, "parent": f})));
    let icon = pid(&ex(&mut s, "object.group", json!({"ids": [v1, v2]})));
    set(&mut s, &[icon], json!({"name": "icon/close"}));
    let card = rect(&mut s, 10.0, 200.0, 300.0, 100.0, Some(f));
    let assets = super::assets(s.doc().unwrap(), &[f, photo]);
    let kind = |id: NodeId| assets.iter().find(|a| a.id == id).map(|a| a.kind);
    // (layer, kind, derivation)
    let rows: [(NodeId, Option<AssetKind>, &str); 4] = [
        (marked, Some(AssetKind::Export), "layers with export settings are assets"),
        (photo, Some(AssetKind::Image), "layers filled with an image are assets"),
        (icon, Some(AssetKind::Icon), "small vector-only layers are icons (docs silent on the exact rule: vectors only, at most 128 a side)"),
        (card, None, "a plain rectangle is not an asset"),
    ];
    for (layer, want, why) in rows {
        assert_eq!(kind(layer), want, "{why}");
    }
    assert!(!assets.iter().any(|a| a.id == v1), "an icon's own vectors aren't listed again");
}
