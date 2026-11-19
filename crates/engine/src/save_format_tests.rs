//! Saves write the native format: a `.fig` path is refused and leaves the save state alone.

use std::path::PathBuf;
use std::sync::Arc;

use figcraft_format::Version;
use serde_json::json;

use crate::{DocState, EngineError, Session};

struct SaveFiles {
    dir: PathBuf,
    files: Vec<PathBuf>,
}

impl SaveFiles {
    fn new() -> Self {
        // Tests run in parallel and the clock can read the same for two of them (macOS counts
        // microseconds), so a counter keeps their folders apart.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("figcraft-save-formats-{}-{stamp}-{n}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        Self { dir, files: Vec::new() }
    }

    fn path(&mut self, name: &str) -> PathBuf {
        let path = self.dir.join(name);
        self.files.push(path.clone());
        path
    }
}

impl Drop for SaveFiles {
    fn drop(&mut self) {
        for path in &self.files {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_dir(path);
        }
        let _ = std::fs::remove_dir(&self.dir);
    }
}

fn rect(s: &mut Session, name: &str) {
    s.execute("shape.rectangle", &json!({"name": name, "x": 10, "y": 20, "width": 30, "height": 40})).unwrap();
}

fn assert_versions_unchanged(after: &[Version], before: &[Version]) {
    assert_eq!(after.len(), before.len());
    for (after, before) in after.iter().zip(before) {
        assert_eq!(
            (&after.id, &after.name, &after.description, after.time, after.auto),
            (&before.id, &before.name, &before.description, before.time, before.auto)
        );
        assert_eq!(*after.document().unwrap(), *before.document().unwrap());
    }
}

fn assert_save_state_unchanged(s: &Session, before: &DocState, recent: &[String]) {
    let after = s.ds().unwrap();
    assert_eq!(after.path, before.path);
    assert_eq!(after.is_dirty(), before.is_dirty());
    assert_eq!(after.versions_changed, before.versions_changed);
    assert!(Arc::ptr_eq(&after.doc, &before.doc));
    assert!(Arc::ptr_eq(&after.saved_doc, &before.saved_doc));
    assert_eq!(
        after.preview.as_ref().map(|p| (&p.id, &p.name, p.time, &p.selection, p.page)),
        before.preview.as_ref().map(|p| (&p.id, &p.name, p.time, &p.selection, p.page))
    );
    if let (Some(after), Some(before)) = (&after.preview, &before.preview) {
        assert!(Arc::ptr_eq(&after.doc, &before.doc));
    }
    assert_eq!((&after.selection, after.page), (&before.selection, before.page));
    assert_versions_unchanged(&after.versions, &before.versions);
    assert_eq!(after.branches.len(), before.branches.len());
    for (after, before) in after.branches.iter().zip(&before.branches) {
        assert_eq!(
            (&after.id, &after.name, after.time, after.client, after.state),
            (&before.id, &before.name, before.time, before.client, before.state)
        );
        assert_eq!(*after.base, *before.base);
        assert_eq!(*after.head, *before.head);
        assert_versions_unchanged(&after.versions, &before.versions);
    }
    for (after, before) in [(&after.history.undo, &before.history.undo), (&after.history.redo, &before.history.redo)] {
        assert_eq!(after.len(), before.len());
        for (after, before) in after.iter().zip(before) {
            assert_eq!((&after.label, &after.selection, after.page), (&before.label, &before.selection, before.page));
            assert!(Arc::ptr_eq(&after.doc, &before.doc));
        }
    }
    assert_eq!(s.prefs.recent_files, recent);
}

#[test]
fn first_native_save_uses_chosen_filename_for_an_untitled_document() {
    let mut files = SaveFiles::new();
    let path = files.path("Redis.figcraft");
    let copy = files.path("Redis copy.figcraft");
    let manual_copy = files.path("Other.figcraft");
    let mut s = Session::new();
    s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(s.ds().unwrap().doc.name, "Redis");
    assert!(!s.ds().unwrap().is_dirty());
    assert_eq!(figcraft_format::load_bytes(&std::fs::read(&path).unwrap()).unwrap().0.name, "Redis");

    let mut reopened = Session::empty();
    reopened.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(reopened.ds().unwrap().doc.name, "Redis");

    s.execute("file.saveAs", &json!({"path": copy.to_string_lossy()})).unwrap();
    assert_eq!(s.ds().unwrap().doc.name, "Redis copy");
    s.execute("file.rename", &json!({"name": "Chosen title"})).unwrap();
    s.execute("file.saveAs", &json!({"path": manual_copy.to_string_lossy()})).unwrap();
    assert_eq!(s.ds().unwrap().doc.name, "Chosen title");
}

#[test]
fn opening_an_older_file_uses_its_filename_only_for_the_default_title() {
    let mut files = SaveFiles::new();
    let old_path = files.path("Redis.figcraft");
    let named_path = files.path("Other.figcraft");
    let mut original = Session::new();
    std::fs::write(&old_path, crate::cmd::file::file_bytes(original.ds().unwrap()).unwrap()).unwrap();
    original.execute("file.rename", &json!({"name": "Chosen title"})).unwrap();
    std::fs::write(&named_path, crate::cmd::file::file_bytes(original.ds().unwrap()).unwrap()).unwrap();

    let mut reopened = Session::empty();
    reopened.execute("file.open", &json!({"path": old_path.to_string_lossy()})).unwrap();
    assert_eq!(reopened.ds().unwrap().doc.name, "Redis");
    reopened.execute("file.open", &json!({"path": named_path.to_string_lossy()})).unwrap();
    assert_eq!(reopened.ds().unwrap().doc.name, "Chosen title");
}

#[test]
fn a_fig_path_is_refused_and_keeps_save_state() {
    let mut files = SaveFiles::new();
    let lower = files.path("refused.fig");
    let upper = files.path("REFUSED.FIG");
    let mut s = Session::new();
    rect(&mut s, "Kept rectangle");
    s.execute("file.saveVersion", &json!({"name": "Kept version"})).unwrap();
    rect(&mut s, "Later rectangle");
    s.execute("version.preview", &json!({"index": 0})).unwrap();
    let before = s.ds().unwrap().clone();
    for command in ["file.save", "file.saveAs"] {
        for path in [&lower, &upper] {
            assert!(matches!(s.execute(command, &json!({"path": path.to_string_lossy()})), Err(EngineError::Disabled(..))), "{command} {path:?}");
            assert_save_state_unchanged(&s, &before, &[]);
            assert!(!path.exists(), "nothing is written under a .fig name");
        }
    }
}

#[test]
fn save_as_without_extension_stays_native() {
    let mut files = SaveFiles::new();
    let requested = files.dir.join("native");
    let native = files.path("native.figcraft");
    let mut s = Session::new();
    rect(&mut s, "Native rectangle");
    let result = s.execute("file.saveAs", &json!({"path": requested.to_string_lossy()})).unwrap();
    assert_eq!(result["path"], native.to_string_lossy().as_ref());
    assert_eq!(s.ds().unwrap().path.as_deref(), native.to_str());
    assert!(!s.ds().unwrap().is_dirty());
    assert!(!requested.exists());
    let loaded = figcraft_format::load_bytes(&std::fs::read(native).unwrap()).unwrap();
    assert!(loaded.0.nodes().any(|n| n.name == "Native rectangle"));
}

#[test]
fn pending_save_choice_keeps_version_preview() {
    let mut files = SaveFiles::new();
    for (index, (command, native_path)) in [("file.saveAs", false), ("file.saveAs", true), ("file.save", false)].into_iter().enumerate() {
        let mut s = Session::new();
        rect(&mut s, "A");
        s.execute("file.saveVersion", &json!({"name": "A only"})).unwrap();
        if native_path {
            let native = files.path(&format!("saved-{index}.figcraft"));
            s.execute("file.save", &json!({"path": native.to_string_lossy()})).unwrap();
        }
        rect(&mut s, "B");
        s.execute("version.preview", &json!({"index": 0})).unwrap();
        let before = s.ds().unwrap().clone();
        let recent = s.prefs.recent_files.clone();
        assert!(matches!(s.execute(command, &json!({})), Err(EngineError::Disabled(..))));
        assert_eq!(s.ui_requests.last().unwrap().kind, "dialog.saveAs");
        assert_save_state_unchanged(&s, &before, &recent);
    }
}

#[test]
fn native_save_from_version_preview_writes_current_document() {
    let mut files = SaveFiles::new();
    for (index, (command, explicit)) in [("file.save", false), ("file.save", true), ("file.saveAs", true)].into_iter().enumerate() {
        let native = files.path(&format!("native-version-{index}.figcraft"));
        let mut s = Session::new();
        rect(&mut s, "A");
        s.execute("file.saveVersion", &json!({"name": "A only"})).unwrap();
        s.execute("file.save", &json!({"path": native.to_string_lossy()})).unwrap();
        rect(&mut s, "B");
        s.execute("version.preview", &json!({"index": 0})).unwrap();
        let undo = s.ds().unwrap().history.undo.len();
        let params = if explicit { json!({"path": native.to_string_lossy()}) } else { json!({}) };
        s.execute(command, &params).unwrap();
        assert!(s.ds().unwrap().preview.is_none());
        assert!(!s.ds().unwrap().is_dirty());
        assert_eq!(s.ds().unwrap().history.undo.len(), undo);
        assert_eq!(s.ds().unwrap().path.as_deref(), native.to_str());
        let loaded = figcraft_format::load_bytes_with_versions(&std::fs::read(&native).unwrap()).unwrap();
        assert!(loaded.doc.nodes().any(|n| n.name == "A"));
        assert!(loaded.doc.nodes().any(|n| n.name == "B"));
        assert!(!loaded.versions[0].document().unwrap().nodes().any(|n| n.name == "B"));
    }
}

#[test]
fn implicit_save_protects_legacy_fig_destination() {
    let mut files = SaveFiles::new();
    for (index, command) in ["file.save", "file.saveAs"].into_iter().enumerate() {
        let legacy = files.path(&format!("legacy-{index}.FIG"));
        let native = files.path(&format!("corrected-{index}.figcraft"));
        let mut s = Session::new();
        rect(&mut s, "A");
        s.execute("file.saveVersion", &json!({"name": "A only"})).unwrap();
        // Earlier builds could write native data under .fig; keep those bytes safe on plain Save.
        let sentinel = crate::cmd::file::file_bytes(s.ds().unwrap()).unwrap();
        std::fs::write(&legacy, &sentinel).unwrap();
        s.ds_mut().unwrap().path = Some(legacy.to_string_lossy().into_owned());
        s.ds_mut().unwrap().mark_saved();
        rect(&mut s, "B");
        s.execute("version.preview", &json!({"index": 0})).unwrap();
        let before = s.ds().unwrap().clone();
        let recent = s.prefs.recent_files.clone();

        assert!(matches!(s.execute("file.save", &json!({})), Err(EngineError::Disabled(..))));
        assert_eq!(s.ui_requests.last().unwrap().kind, "dialog.saveAs");
        assert_save_state_unchanged(&s, &before, &recent);
        assert_eq!(std::fs::read(&legacy).unwrap(), sentinel);

        s.execute(command, &json!({"path": native.to_string_lossy()})).unwrap();
        assert!(s.ds().unwrap().preview.is_none());
        assert!(!s.ds().unwrap().is_dirty());
        assert_eq!(s.ds().unwrap().path.as_deref(), native.to_str());
        let loaded = figcraft_format::load_bytes_with_versions(&std::fs::read(&native).unwrap()).unwrap();
        assert!(loaded.doc.nodes().any(|n| n.name == "A"));
        assert!(loaded.doc.nodes().any(|n| n.name == "B"));
        assert_eq!(loaded.versions[0].name, "A only");
        rect(&mut s, "C");
        s.execute("file.save", &json!({})).unwrap();
        let loaded = figcraft_format::load_bytes(&std::fs::read(&native).unwrap()).unwrap();
        assert!(loaded.0.nodes().any(|n| n.name == "C"));
        assert_eq!(std::fs::read(&legacy).unwrap(), sentinel);
    }
}
