use std::collections::{BTreeMap, BTreeSet};

use super::{DocState, Document, EngineError, NodeKind, Result, Value, json};

/// The recovery copy at `path` as a document with unsaved changes and no path.
pub(crate) fn recovered(path: &str) -> Result<DocState> {
    recovered_from(&read(path)?)
}

/// A recovery copy's bytes as a document with unsaved changes and no path (the browser keeps its
/// copies in the page's storage, not in files).
pub(crate) fn recovered_from(bytes: &[u8]) -> Result<DocState> {
    let loaded = figcraft_format::load_bytes_with_versions(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(DocState::new(loaded.doc, None).with_versions(loaded.versions).with_branches(loaded.branches).unsaved())
}

/// The largest file opened or imported as a document. A `.figcraft` file keeps its images as
/// they are, so a real one can be far larger than it unpacks to (#1313's took 7.4 GB); twice that
/// opens, while a device, a pipe or an absurd file is refused before it is read (#1114).
pub const MAX_DOCUMENT_FILE: u64 = 16 << 30;

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn read(path: &str) -> Result<Vec<u8>> {
    crate::file_access::read_capped(path, MAX_DOCUMENT_FILE)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn read(path: &str) -> Result<Vec<u8>> {
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

/// The bytes of file `path`, refused when it's larger than `max`.
pub(crate) fn read_capped(path: &str, max: u64) -> Result<Vec<u8>> {
    crate::file_access::read_capped(path, max)
}

/// Write `bytes` to `path` through a uniquely named, exclusively created staging file and one
/// atomic rename (see [`figcraft_format::write_file_atomic`]): a failed or interrupted write never
/// truncates `path`, and no file or symlink at a staging name is ever written through.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write(path: &str, bytes: &[u8]) -> Result<()> {
    crate::file_access::write(path, bytes)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn write(path: &str, _bytes: &[u8]) -> Result<()> {
    Err(EngineError::Other(format!("{path}: no file system in the browser")))
}

pub(super) fn ext(path: &str) -> String {
    std::path::Path::new(path).extension().and_then(std::ffi::OsStr::to_str).unwrap_or("").to_ascii_lowercase()
}

pub(super) fn file_stem(path: &str) -> String {
    std::path::Path::new(path).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Untitled").to_string()
}

/// The counts the import report shows for a freshly loaded document.
pub(super) fn import_summary(doc: &Document) -> Value {
    json!({"name": doc.name, "pages": doc.pages.len(), "layers": doc.nodes().filter(|n| !n.is_page()).count(), "images": doc.images.len()})
}

/// Load a document from a native file, an SVG or an image.
pub fn load(path: &str) -> Result<(Document, Vec<String>)> {
    load_path(path).map(|l| (l.doc, l.issues))
}

pub fn load_bytes(bytes: &[u8], name_hint: &str) -> Result<(Document, Vec<String>)> {
    load_with(bytes, name_hint).map(|l| (l.doc, l.issues))
}

/// [`load`] with the version history.
pub fn load_path(path: &str) -> Result<figcraft_format::Loaded> {
    load_with(&read(path)?, path)
}

/// Load from bytes: the document with its notes (repairs of a native file, or import warnings)
/// and version history. Instances are drawn from the layers they hold, never expanded from their
/// main component, so components that hold each other's instances load as they are.
pub(super) fn load_with(bytes: &[u8], name_hint: &str) -> Result<figcraft_format::Loaded> {
    load_any(bytes, name_hint)
}

fn load_any(bytes: &[u8], name_hint: &str) -> Result<figcraft_format::Loaded> {
    match ext(name_hint).as_str() {
        "svg" => {
            let mut doc = Document::new(file_stem(name_hint));
            let page = doc.first_page().ok_or(EngineError::NoDocument)?;
            let notes = figcraft_svg::import_svg_with_notes(&mut doc, bytes, page, 0.0, 0.0, &file_stem(name_hint))
                .map_err(|e| EngineError::Other(e.to_string()))?
                .notes;
            Ok(figcraft_format::Loaded { issues: notes, ..doc.into() })
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" => {
            let mut doc = Document::new(file_stem(name_hint));
            let page = doc.first_page().ok_or(EngineError::NoDocument)?;
            let (hash, w, h) = crate::cmd::create::add_readable_image(&mut doc, bytes.to_vec()).map_err(EngineError::Other)?;
            let id = doc.create(page, NodeKind::Rectangle, &file_stem(name_hint), 0.0, 0.0, f64::from(w), f64::from(h))?;
            doc.get_mut(id)?.fills = vec![figcraft_color::Paint::image(hash, figcraft_color::ImageScaleMode::Fill)];
            Ok(doc.into())
        }
        _ => {
            let mut loaded = figcraft_format::load_bytes_with_versions(bytes).map_err(|e| EngineError::Other(e.to_string()))?;
            // Older files saved an untouched default title even after the user chose a filename.
            if loaded.doc.name.is_empty() || loaded.doc.name == "Untitled" {
                loaded.doc.name = file_stem(name_hint);
            }
            Ok(loaded)
        }
    }
}

/// Whether some of the first `len` characters are in none of `runs`. Where runs start and stop
/// covering is summed once, so runs overlapping any number of times cost no more than their count.
fn has_uncovered(len: usize, runs: &[figcraft_doc::StyleRun]) -> bool {
    let mut change = vec![0i64; len + 1];
    for run in runs {
        let (start, end) = (run.start.min(len), run.end.min(len));
        if start < end {
            if let Some(c) = change.get_mut(start) {
                *c += 1;
            }
            if let Some(c) = change.get_mut(end) {
                *c -= 1;
            }
        }
    }
    let mut open = 0i64;
    change.iter().take(len).any(|c| {
        open += c;
        open <= 0
    })
}

/// Non-destructive font diagnostic for the action palette, CLI and control API.
/// Includes styled runs, not just the default typeface of each text layer.
pub(super) fn font_report(doc: &Document) -> Value {
    figcraft_text::scan_system_fonts();
    font_report_with_families(doc, &figcraft_text::family_names())
}

fn font_report_with_families(doc: &Document, installed: &[String]) -> Value {
    let known: BTreeSet<String> = installed.iter().map(|s| s.trim().to_lowercase()).collect();
    let mut families: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in doc.nodes() {
        let NodeKind::Text(text) = &node.kind else { continue };
        if text.characters.is_empty() {
            continue;
        }
        let len = text.characters.chars().count().min(100_000);
        let mut used = BTreeSet::new();
        for run in &text.runs {
            if run.start.min(len) < run.end.min(len) && !run.style.font_family.trim().is_empty() {
                used.insert(run.style.font_family.clone());
            }
        }
        let uncovered = has_uncovered(len, &text.runs);
        if uncovered {
            used.insert(text.style.font_family.clone());
        }
        for family in used {
            let family = family.trim();
            if family.is_empty() {
                continue;
            }
            if families.len() >= 256 && !families.contains_key(family) {
                continue;
            }
            let layer_name = node.name.chars().filter(|c| !c.is_control()).take(120).collect::<String>();
            let layers = families.entry(family.chars().filter(|c| !c.is_control()).take(160).collect()).or_default();
            if layers.len() < 25 {
                layers.insert(layer_name);
            }
        }
    }
    let entries = families
        .into_iter()
        .map(|(family, layers)| {
            let lower = family.to_lowercase();
            let generic = matches!(lower.as_str(), "sans-serif" | "serif" | "monospace" | "system-ui");
            // No registered families can mean an uninitialised/no-font runtime; it is
            // not evidence that all names are absent from the user's computer.
            let available = if generic {
                Some(true)
            } else if known.is_empty() {
                None
            } else {
                // Also under the other name its fonts give the family ("Economia Medium").
                Some(known.contains(&lower) || figcraft_text::has_family(&family))
            };
            json!({"family":family,"available":available,"layers":layers.into_iter().collect::<Vec<_>>()})
        })
        .collect::<Vec<_>>();
    let missing = entries.iter().filter(|v| v.get("available") == Some(&Value::Bool(false))).count();
    json!({"total": entries.len(), "missing": missing, "embeddedFontFiles": 0,
        "fontLicenseNotices": 0, "fonts": entries})
}

#[cfg(test)]
mod font_report_tests {
    use super::*;
    use figcraft_doc::TextData;

    #[test]
    fn many_overlapping_style_runs_are_summed_once() {
        use figcraft_doc::StyleRun;
        let mut doc = Document::new("Runs");
        let page = doc.first_page().unwrap();
        let mut t = TextData::new("x".repeat(100_000));
        t.style.font_family = "Unused Base".into();
        let mut run = t.style.clone();
        run.font_family = "Run Font".into();
        t.runs = (0..50_000).map(|_| StyleRun { start: 0, end: 100_000, style: run.clone() }).collect();
        doc.create(page, NodeKind::Text(t), "Long", 0.0, 0.0, 100.0, 30.0).unwrap();
        let report = font_report_with_families(&doc, &[]);
        assert_eq!(report["fonts"].as_array().map(Vec::len), Some(1), "{report}");
        assert_eq!(report["fonts"][0]["family"], "Run Font");
    }

    #[test]
    fn font_inventory_reports_missing_families_without_rewriting_text() {
        let mut doc = Document::new("Font inventory");
        let page = doc.first_page().unwrap();
        let mut text = TextData::new("Example");
        text.style.font_family = "Definitely Never Installed V10 Font Q7".into();
        doc.create(page, NodeKind::Text(text), "Caption", 0.0, 0.0, 100.0, 24.0).unwrap();
        let report = font_report_with_families(&doc, &["Inter".into()]);
        assert_eq!(report["missing"], 1);
        assert_eq!(report["fonts"][0]["family"], "Definitely Never Installed V10 Font Q7");
        assert_eq!(report["fonts"][0]["layers"][0], "Caption");
    }
}
