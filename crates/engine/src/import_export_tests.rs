//! Layer export settings (rows, sizes, "Export N layers"). Fixtures are synthetic.

use base64::Engine as _;
use figcraft_doc::*;
use serde_json::json;

use crate::Session;
use crate::params::id_of;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("figcraft-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- export settings ----

fn rect(s: &mut Session, name: &str, w: f64, h: f64) -> NodeId {
    let id = id_of(&s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": w, "height": h})).unwrap()["id"]).unwrap();
    s.execute("node.set", &json!({"ids": [id], "props": {"name": name}})).unwrap();
    id
}

fn exports(s: &Session, id: NodeId) -> Vec<ExportSetting> {
    s.doc().unwrap().node(id).unwrap().exports.clone()
}

/// Width of a PNG from its header.
fn png_width(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[16], b[17], b[18], b[19]])
}

#[test]
fn export_rows_add_step_up_and_update() {
    let mut s = Session::new();
    let r = rect(&mut s, "Icon", 32.0, 16.0);
    for _ in 0..3 {
        s.execute("export.add", &json!({"ids": [r]})).unwrap();
    }
    let rows = exports(&s, r);
    let sizes: Vec<String> = rows.iter().map(|e| e.size.label()).collect();
    assert_eq!(sizes, ["1x", "2x", "3x"]);
    assert_eq!(rows.iter().map(|e| e.suffix.as_str()).collect::<Vec<_>>(), ["", "@2x", "@3x"]);

    s.execute("export.update", &json!({"ids": [r], "index": 1, "size": "512w", "format": "webp", "suffix": "-wide"})).unwrap();
    s.execute("export.update", &json!({"ids": [r], "index": 2, "size": {"by": "height", "value": 64}, "format": "pdf"})).unwrap();
    let rows = exports(&s, r);
    assert_eq!(
        rows[1],
        ExportSetting { size: ExportSize::Width { value: 512.0 }, suffix: "-wide".into(), format: ExportFormat::Webp, ..Default::default() }
    );
    assert_eq!((rows[2].size, rows[2].format), (ExportSize::Height { value: 64.0 }, ExportFormat::Pdf));
    for bad in [
        json!({"size": "0x"}),
        json!({"size": "abc"}),
        json!({"size": "1e9w"}),
        json!({"format": "gif"}),
        json!({"size": {"by": "scale", "value": -1}}),
    ] {
        let mut p = bad.clone();
        p["ids"] = json!([r]);
        p["index"] = json!(0);
        assert!(s.execute("export.update", &p).is_err(), "{bad}");
    }
    assert_eq!(exports(&s, r), rows, "refused edits change nothing");
    // A given setting is used as is (sizes as text too).
    s.execute("export.add", &json!({"ids": [r], "setting": {"size": "0.5x", "format": "jpg"}})).unwrap();
    assert_eq!(exports(&s, r)[3].size, ExportSize::Scale { value: 0.5 });
    assert!(s.execute("export.add", &json!({"ids": [r], "setting": {"size": "huge"}})).is_err());
}

#[test]
fn export_layers_uses_each_layers_settings() {
    let mut s = Session::new();
    let a = rect(&mut s, "Icon", 32.0, 16.0);
    let b = rect(&mut s, "Logo/Dark", 10.0, 10.0);
    let c = rect(&mut s, "Plain", 10.0, 10.0);
    s.execute("export.add", &json!({"ids": [a]})).unwrap();
    s.execute("export.add", &json!({"ids": [a]})).unwrap();
    s.execute("export.add", &json!({"ids": [a], "setting": {"size": "512w", "format": "png"}})).unwrap();
    s.execute("export.add", &json!({"ids": [b], "setting": {"format": "svg"}})).unwrap();

    let r = s.execute("file.exportLayers", &json!({"ids": [a, b, c]})).unwrap();
    let files = r["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Icon.png", "Icon@2x.png", "Icon 2.png", "Logo/Dark.svg"], "unique names; a slash makes a folder");
    assert_eq!(r["skipped"], 1);
    let data = |i: usize| base64::engine::general_purpose::STANDARD.decode(files[i]["data"].as_str().unwrap()).unwrap();
    assert_eq!(png_width(&data(0)), 32);
    assert_eq!(png_width(&data(1)), 64);
    assert_eq!(png_width(&data(2)), 512, "512w scales to that width");
    assert!(String::from_utf8_lossy(&data(3)).contains("<svg"));

    // Without ids or a selection: every layer on the page with settings; into a folder.
    s.set_selection(Vec::new());
    let dir = temp_dir("exportlayers");
    let r = s.execute("file.exportLayers", &json!({"dir": dir.to_string_lossy()})).unwrap();
    assert_eq!(r["files"].as_array().map(Vec::len), Some(4));
    assert!(dir.join("Icon@2x.png").is_file() && dir.join("Logo").join("Dark.svg").is_file());
    let _ = std::fs::remove_dir_all(&dir);

    // Nothing to export.
    assert!(s.execute("file.exportLayers", &json!({"ids": [c]})).is_err());
    // `file.export` takes the same sizes.
    let r = s.execute("file.export", &json!({"ids": [a], "size": "128w"})).unwrap();
    assert_eq!(png_width(&base64::engine::general_purpose::STANDARD.decode(r["data"].as_str().unwrap()).unwrap()), 128);
    assert!(s.execute("file.export", &json!({"ids": [a], "size": "-2x"})).is_err());
}

#[test]
fn export_sizes_parse_and_label() {
    for (text, want) in [
        ("2x", ExportSize::Scale { value: 2.0 }),
        ("@0.5X", ExportSize::Scale { value: 0.5 }),
        (" 3 ", ExportSize::Scale { value: 3.0 }),
        ("512w", ExportSize::Width { value: 512.0 }),
        ("100 h", ExportSize::Height { value: 100.0 }),
    ] {
        assert_eq!(ExportSize::parse(text), Some(want), "{text}");
        assert_eq!(ExportSize::parse(&want.label()), Some(want));
    }
    for bad in ["", "x", "0x", "-1x", "NaNx", "infx", "65x", "20000w", "2q", "2xx", "+2x", "+512w", "2e0x", "2E0x", "+0.5x"] {
        assert_eq!(ExportSize::parse(bad), None, "{bad}");
    }
    let e = |size, suffix: &str, format| ExportSetting { size, suffix: suffix.into(), format, ..Default::default() };
    assert_eq!(e(ExportSize::Scale { value: 2.0 }, "", ExportFormat::Png).file_suffix(), "@2x");
    assert_eq!(e(ExportSize::Scale { value: 2.0 }, "", ExportFormat::Svg).file_suffix(), "");
    assert_eq!(e(ExportSize::Scale { value: 2.0 }, "-big", ExportFormat::Png).file_suffix(), "-big");
    assert_eq!(e(ExportSize::Width { value: 64.0 }, "", ExportFormat::Png).file_suffix(), "");
}

#[test]
fn export_size_label_parses_back_exactly() {
    for want in [
        ExportSize::Scale { value: 0.125 },
        ExportSize::Scale { value: 0.004 },
        ExportSize::Scale { value: 0.001 },
        ExportSize::Scale { value: 63.99 },
        ExportSize::Scale { value: 1.0 / 3.0 },
        ExportSize::Width { value: 512.125 },
        ExportSize::Height { value: 0.005 },
    ] {
        assert_eq!(ExportSize::parse(&want.label()), Some(want), "{}", want.label());
    }
    assert_eq!(ExportSize::Scale { value: 0.125 }.label(), "0.125x");
    assert_eq!(ExportSize::Scale { value: 2.0 }.label(), "2x");
    assert_eq!(ExportSize::Width { value: 512.0 }.label(), "512w");
}
