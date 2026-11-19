use figcraft_engine::Session;
use serde_json::json;

use crate::import_report::{MAX_NOTE_CHARS, MAX_NOTES, Report, note_text};
use crate::{FigApp, NoServices};

#[test]
fn import_results_with_warnings_become_reports_with_counts() {
    let result = json!({"warnings": ["Synthetic note"], "summary": {"name": "Synthetic design", "pages": 2, "layers": 12, "images": 3}});
    let report = Report::from_result("file.import", &json!({}), &result).unwrap();
    assert_eq!(report.name, "Synthetic design");
    assert_eq!((report.pages, report.layers, report.images), (Some(2), Some(12), Some(3)));
    assert_eq!(report.notes, ["Synthetic note"]);
    assert_eq!(report.note_count, 1);
}

#[test]
fn only_import_commands_with_warnings_create_reports() {
    // A warning-free open still has a summary to show.
    let report = Report::from_result("file.open", &json!({}), &json!({"warnings": [], "summary": {"name": "Local", "pages": 1}})).unwrap();
    assert!(!report.appended && report.notes.is_empty());
    // A native file's repairs have their own message; an SVG without notes has no `warnings`.
    assert!(Report::from_result("file.open", &json!({}), &json!({"repairs": ["Repaired parent"]})).is_none());
    assert!(Report::from_result("file.open", &json!({}), &json!({"index": 0})).is_none());
    assert!(Report::from_result("file.export", &json!({}), &json!({"warnings": ["Export warning"]})).is_none());
    assert!(Report::from_result("shape.rectangle", &json!({}), &json!({})).is_none());
    // File ▸ Open recent opens by path and reports like File ▸ Open.
    let recent = json!({"warnings": ["Synthetic note"], "summary": {"name": "Recent", "pages": 1}});
    assert!(Report::from_result("file.openRecent", &json!({"path": "Recent.figcraft"}), &recent).is_some());
    assert!(Report::from_result("file.openRecent", &json!({"path": "Clean.svg"}), &json!({"warnings": []})).is_none());
}

#[test]
fn svg_placements_are_not_reported_as_appended_pages() {
    assert!(Report::from_result("file.import", &json!({}), &json!({"id": 7})).is_none());
    let report = Report::from_result("file.import", &json!({}), &json!({"id": 7, "warnings": ["Filter skipped"]})).unwrap();
    assert!(!report.appended);
    assert!(Report::from_result("file.import", &json!({}), &json!({"pages": [1], "summary": {"pages": 1}, "warnings": []})).unwrap().appended);
}

#[test]
fn report_retention_is_bounded_and_truncates_only_at_character_boundaries() {
    let long_note = "🦎".repeat(MAX_NOTE_CHARS + 20);
    let notes: Vec<_> = std::iter::repeat_n(long_note, MAX_NOTES + 37).collect();
    let report = Report::from_result("file.open", &json!({}), &json!({"warnings": notes})).unwrap();
    assert_eq!(report.note_count, MAX_NOTES + 37);
    assert_eq!(report.notes.len(), MAX_NOTES);
    assert!(report.notes.iter().all(|note| note.chars().count() == MAX_NOTE_CHARS + 1 && note.ends_with('…')));
}

#[test]
fn note_text_keeps_the_repeat_suffix_and_unknown_notes() {
    // No catalog entry: the note is shown as it is, with its repeat count.
    assert_eq!(note_text("some unknown note (×122)"), "some unknown note (×122)");
    assert_eq!(note_text("some unknown note"), "some unknown note");
    assert_eq!(note_text("weird (×x)"), "weird (×x)");
    assert_eq!(note_text("weird (×12) tail"), "weird (×12) tail");
}

#[test]
fn escape_closes_the_import_report_like_other_simple_dialogs() {
    let mut u = crate::layers_drag_tests::Ui::new(Session::new());
    let mut report = Report::from_result("file.open", &json!({}), &json!({"warnings": ["Synthetic note"], "summary": {"name": "Synthetic design"}}));
    report.as_mut().unwrap().uid = u.app.session.ds().unwrap().uid;
    u.app.ui.import_report = report;
    u.frame(vec![]);
    let escape =
        vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
    u.frame(escape);
    assert!(u.app.ui.import_report.is_none(), "Escape closes the report");
}

pub(crate) fn shown_texts(shapes: Vec<egui::epaint::ClippedShape>) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn a_clean_svg_open_has_no_report_and_a_placed_svg_says_where_it_went() {
    let svg_open = Report::from_result("file.open", &json!({"path": "icons/Logo.SVG"}), &json!({"warnings": []}));
    assert!(svg_open.is_none(), "a clean SVG opened with file.open has nothing to report");
    let noted = Report::from_result("file.open", &json!({"path": "Logo.svg"}), &json!({"warnings": ["Filter skipped"]})).unwrap();
    assert_eq!(noted.name, "Logo.svg");

    let placed = Report::from_result("file.import", &json!({"path": "dir/Logo.svg"}), &json!({"id": 7, "warnings": ["Filter skipped"]})).unwrap();
    assert!(placed.placed && !placed.appended);
    assert_eq!(placed.name, "Logo.svg");
    let mut u = crate::layers_drag_tests::Ui::new(Session::new());
    let mut placed = placed;
    placed.uid = u.app.session.ds().unwrap().uid;
    u.app.ui.import_report = Some(placed);
    let texts = shown_texts({
        u.frame(vec![]);
        u.frame_output(vec![]).shapes
    });
    assert!(texts.iter().any(|t| t == "Placed on the current page"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "Opened as a new document"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Logo.svg"), "{texts:?}");
}

#[test]
fn the_report_goes_when_its_document_closes_and_counts_omitted_notes_in_the_singular() {
    let mut u = crate::layers_drag_tests::Ui::new(Session::new());
    let mut report = Report::from_result("file.open", &json!({}), &json!({"warnings": ["One note"], "summary": {"name": "Synthetic"}})).unwrap();
    report.uid = u.app.session.ds().unwrap().uid;
    report.note_count = 2;
    u.app.ui.import_report = Some(report);
    let texts = shown_texts({
        u.frame(vec![]);
        u.frame_output(vec![]).shapes
    });
    assert!(texts.iter().any(|t| t == "1 more note omitted from this report"), "{texts:?}");

    u.app.session.execute("file.new", &json!({"name": "Other"})).unwrap();
    u.frame(vec![]);
    assert!(u.app.ui.import_report.is_some(), "another tab was added; the report's document is still open");
    u.app.session.execute("file.close", &json!({"index": 0, "force": true})).unwrap();
    u.frame(vec![]);
    assert!(u.app.ui.import_report.is_none(), "the report left with its document");
}

/// An SVG whose filter is neither a shadow nor a blur: opening it leaves a note.
const NOTED_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><filter id="noise"><feTurbulence baseFrequency="0.5"/></filter><rect width="20" height="20" filter="url(#noise)"/></svg>"#;

fn has_filter_note(notes: &[String]) -> bool {
    notes.iter().any(|n| n.contains("isn't a shadow or blur"))
}

#[test]
fn a_dropped_file_shows_its_report_once_it_opens() {
    let path = std::env::temp_dir().join(format!("figcraft-import-report-drop-{}.svg", std::process::id()));
    std::fs::write(&path, NOTED_SVG).unwrap();
    // A programmatic open (control channel, MCP) never opens the window.
    let mut app = FigApp::new(Session::new(), Box::new(NoServices));
    assert!(app.run("file.open", json!({"path": path.to_string_lossy()})).is_some());
    assert!(app.ui.import_report.is_none());
    let mut u = crate::layers_drag_tests::Ui::new(Session::new());
    u.app.ui.pending_opens.push(crate::pending_open::PendingOpen {
        label: "Dropped.svg".into(),
        params: json!({"path": path.to_string_lossy()}),
        command: "file.open",
        target: None,
        read: None,
    });
    for _ in 0..6 {
        u.frame(vec![]);
    }
    std::fs::remove_file(&path).unwrap();
    let report = u.app.ui.import_report.as_ref().expect("the dropped file's report");
    assert_eq!(report.uid, u.app.session.ds().unwrap().uid);
    assert!(has_filter_note(&report.notes), "{:?}", report.notes);
    assert_eq!(u.app.inspect()["importReport"]["warningCount"], report.note_count);
}

#[test]
fn a_file_opened_from_base64_data_has_a_summary_too() {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(NOTED_SVG);
    let mut app = FigApp::new(Session::new(), Box::new(NoServices));
    let opened = app.run_user("file.open", json!({"data": data, "name": "Dropped.svg"})).unwrap();
    assert_eq!(opened["summary"]["pages"], 1);
    let report = app.ui.import_report.as_ref().expect("a report for the SVG open");
    assert_eq!(report.pages, Some(1));
    assert!(has_filter_note(&report.notes), "{:?}", report.notes);
    // An SVG note has no catalog entry: it is shown as it is, worked out once when the report opens.
    assert!(report.shown.iter().any(|n| n.starts_with("filter noise isn't a shadow or blur")), "{:?}", report.shown);
}
