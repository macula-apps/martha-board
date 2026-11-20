//! The home screen, drawn headless: its one section (Recents: no Online, no Templates), the
//! search, the first run (no recent files, and nothing offered in their place), no frames once
//! settled and the Getting started card (closed, kept closed, and brought back from the Help menu).

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Shape};
use figcraft_engine::Session;
use serde_json::json;

use crate::home::Section;
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text, text_center, texts};

/// The app as a first run finds it: no documents, no recent files.
fn first_run() -> Ui {
    Ui::new(Session::empty())
}

fn shapes(u: &mut Ui) -> Vec<Shape> {
    flat(&u.frame_output(vec![]).shapes)
}

/// Enough frames for what follows a click, then until every recent file has been looked at: the
/// reader is a thread of its own, and until it
/// answers a file's card is a skeleton without its name (on a loaded machine, 12 frames weren't
/// always enough, #351).
fn settle(u: &mut Ui) -> Vec<Shape> {
    for _ in 0..12 {
        u.frame(vec![]);
    }
    for _ in 0..400 {
        let files = u.app.inspect()["homeRecents"]["files"].as_array().cloned().unwrap_or_default();
        if files.iter().all(|f| !f["exists"].is_null()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        u.frame(vec![]);
    }
    shapes(u)
}

fn click(u: &mut Ui, p: Pos2) {
    for pressed in [true, false] {
        u.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    }
    u.frame(vec![]);
}

fn press(u: &mut Ui, key: Key) {
    for pressed in [true, false] {
        u.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }]);
    }
}

/// The sidebar's tabs are exactly Recents: Home ▸ Online left with online files
/// (martha-board#41), and there are no Templates or placeholders.
#[test]
fn the_sidebar_has_recents_only_and_no_online_templates_or_placeholders() {
    let mut u = first_run();
    let s = shapes(&mut u);
    assert!(has_text(&s, "Recents"));
    for gone in ["Online", "Templates", "Drafts", "Community"] {
        assert!(!has_text(&s, gone), "{gone}");
    }
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let nodes: Vec<_> = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect();
    let tab = |label: &str| nodes.iter().find(|n| n.role() == Role::Tab && n.label() == Some(label)).unwrap_or_else(|| panic!("no tab {label}"));
    // The sidebar's tabs: those in Recents' column from it down, top to bottom (the window's own
    // tab bar, with its Home tab, is above).
    let first = tab("Recents").bounds().unwrap();
    let mut sidebar: Vec<_> =
        nodes.iter().filter(|n| n.role() == Role::Tab && n.bounds().is_some_and(|b| b.x0 == first.x0 && b.y0 >= first.y0)).collect();
    sidebar.sort_by(|a, b| a.bounds().unwrap().y0.total_cmp(&b.bounds().unwrap().y0));
    assert_eq!(sidebar.iter().filter_map(|n| n.label()).collect::<Vec<_>>(), ["Recents"], "the sidebar's tabs");
    for gone in ["Online", "Templates"] {
        assert!(!nodes.iter().any(|n| n.role() == Role::Tab && n.label() == Some(gone)), "no {gone} tab anywhere");
    }
    assert_eq!(tab("Recents").is_selected(), Some(true));
    // Clicked into, and `ui.inspect` and `ui.set {homeSection}` name it the same way.
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Recents"));
    assert_eq!(u.app.ui.home_view.section, Section::Recents);
    let inspect = u.app.inspect();
    assert_eq!(inspect["homeSection"], Section::Recents.id());
    assert!(inspect.get("online").is_none(), "no Online section to inspect: {inspect}");
    assert_eq!(Section::parse(Section::Recents.id()), Some(Section::Recents));
    assert_eq!(Section::parse("drafts"), None);
    assert_eq!(Section::parse("templates"), None, "the starter templates' section is gone");
    assert_eq!(Section::parse("online"), None, "the online files' section is gone");
}

#[test]
fn home_search_finds_known_files_and_opens_an_existing_tab() {
    let mut session = Session::new();
    session.execute("file.rename", &json!({"name": "North Trail"})).unwrap();
    session.docs[0].path = Some("C:/designs/North Trail.figcraft".into());
    session.prefs.recent_files =
        vec!["C:/designs/North Trail.figcraft".into(), "C:/designs/South Trail.figcraft".into(), "C:/designs/Other.figcraft".into()];
    let mut u = Ui::new(session);
    u.app.ui.home = true;
    u.app.ui.home_view.file_query = "  TRAIL  ".into();
    let s = settle(&mut u);
    assert!(has_text(&s, "Search results"));
    assert!(has_text(&s, "North Trail") && has_text(&s, "South Trail"));
    assert!(!has_text(&s, "Other") && !has_text(&s, "Online files"));
    assert_eq!(texts(&s).iter().filter(|(name, pos, _)| name == "North Trail" && pos.y > 90.0).count(), 1, "an open file is not repeated in recents");
    assert_eq!(u.app.inspect()["homeSearch"], "  TRAIL  ");

    u.app.ui.home_view.file_query = "nothing matches".into();
    let s = shapes(&mut u);
    assert!(has_text(&s, "No files match your search"));
    assert!(!has_text(&s, "South Trail"));
    assert!(!texts(&s).iter().any(|(name, pos, _)| name == "North Trail" && pos.y > 90.0));

    u.app.ui.home_view.file_query = "north".into();
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "North Trail"));
    assert!(!u.app.ui.home, "choosing an open result shows its editor tab");
    assert_eq!(u.app.session.docs.len(), 1);
}

#[test]
fn home_search_shows_a_saved_files_preview() {
    let dir = std::env::temp_dir().join(format!("figcraft-home-search-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Search preview.figcraft");
    let image = image::RgbaImage::from_pixel(64, 32, image::Rgba([30, 140, 210, 255]));
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let doc = figcraft_doc::Document::new("Search preview");
    std::fs::write(&path, figcraft_format::save_bytes(&doc, Some(&png)).unwrap()).unwrap();

    let mut session = Session::empty();
    session.prefs.recent_files.push(path.to_string_lossy().into_owned());
    let mut u = Ui::new(session);
    u.app.ui.home_view.file_query = "Search preview".into();
    for _ in 0..400 {
        u.frame(vec![]);
        if u.app.inspect()["homeRecents"]["files"][0]["picture"] == "picture" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let s = shapes(&mut u);
    assert!(has_text(&s, "Search preview"));
    assert!(s.iter().any(|s| matches!(s, Shape::Mesh(m) if m.texture_id != egui::TextureId::default())), "the saved preview is drawn");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(dir);
}

#[test]
fn home_search_field_filters_as_typed_and_a_section_click_clears_it() {
    let mut u = first_run();
    u.app.session.prefs.recent_files = vec!["C:/designs/Glacier.figcraft".into(), "C:/designs/Forest.figcraft".into()];
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Search files"));
    u.frame(vec![Event::Text("glacier".into())]);
    let s = settle(&mut u);
    assert_eq!(u.app.ui.home_view.file_query, "glacier");
    assert!(has_text(&s, "Glacier") && !has_text(&s, "Forest"));
    click(&mut u, text_center(&s, "Recents"));
    assert!(u.app.ui.home_view.file_query.is_empty());
    let s = shapes(&mut u);
    assert!(has_text(&s, "Forest"));
    // Escape in the field clears the search too.
    click(&mut u, text_center(&s, "Search files"));
    u.frame(vec![Event::Text("forest".into())]);
    assert_eq!(u.app.ui.home_view.file_query, "forest");
    u.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(u.app.ui.home_view.file_query.is_empty(), "Escape clears the search");
}

/// A search keeps what the recent files it doesn't match know: back from it, their pictures and
/// the selection are still there (nothing is read again).
#[test]
fn a_search_keeps_the_other_recent_files_pictures_and_the_selection() {
    let dir = std::env::temp_dir().join(format!("figcraft-home-search-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([30, 140, 210, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let paths: Vec<String> = ["Alpine", "Basin"]
        .into_iter()
        .map(|name| {
            let path = dir.join(format!("{name}.figcraft"));
            std::fs::write(&path, figcraft_format::save_bytes(&figcraft_doc::Document::new(name), Some(&png)).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        })
        .collect();
    let mut session = Session::empty();
    session.prefs.recent_files = paths.clone();
    let mut u = Ui::new(session);
    let pictures = |u: &mut Ui| -> Vec<serde_json::Value> {
        u.app.inspect()["homeRecents"]["files"].as_array().unwrap().iter().map(|f| f["picture"].clone()).collect()
    };
    for _ in 0..400 {
        u.frame(vec![]);
        if pictures(&mut u).iter().all(|p| p == "picture") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(pictures(&mut u).iter().all(|p| p == "picture"), "both pictures are in");
    u.app.ui.home_view.recents.selected = Some(paths[1].clone());
    u.app.ui.home_view.file_query = "alpine".into();
    let s = shapes(&mut u);
    assert!(has_text(&s, "Alpine") && !has_text(&s, "Basin"));
    u.app.ui.home_view.file_query.clear();
    u.frame(vec![]);
    assert!(pictures(&mut u).iter().all(|p| p == "picture"), "no picture is read again");
    assert_eq!(u.app.ui.home_view.recents.selected.as_deref(), Some(paths[1].as_str()), "the selection stays");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_home_screen_draws_no_frames_once_settled() {
    let (mut h, _) = crate::live_tests::Harness::new();
    h.app.ui.home = true;
    h.frames(60);
    let (_, delay) = h.frame(vec![]);
    assert!(delay >= std::time::Duration::from_secs(1), "Recents asks for a frame in {delay:?}: {:?}", h.ctx.repaint_causes());
}

/// What Home shows, pinned by the headings and labels on screen: Recents with its Getting started
/// card; no Online section or online files (martha-board#41), and no starter templates (no
/// Templates section, no first-run offer of them).
#[test]
fn home_shows_recents_and_getting_started_and_no_templates() {
    let mut u = first_run();
    let s = settle(&mut u);
    for shown in ["Recents", "New design file", "New board", "Open file", "Join with an invite link", "Getting started"] {
        assert!(has_text(&s, shown), "{shown}");
    }
    for gone in ["Online", "Online files", "Templates", "Start from a template", "See all templates", "Search templates", "Show all templates"] {
        assert!(!has_text(&s, gone), "{gone}");
    }
    assert!(!has_text(&s, "Recent files") && !has_text(&s, "Open files"), "a first run has nothing to list");
    // With a recent file and an open document, their lists show; still no templates.
    u.app.run("file.new", json!({}));
    u.app.ui.home = true;
    u.app.session.prefs.recent_files = vec!["/tmp/ridgeline.figcraft".into()];
    let s = settle(&mut u);
    assert!(has_text(&s, "Recent files") && has_text(&s, "ridgeline") && has_text(&s, "Open files"));
    assert!(has_text(&s, "Getting started"));
    assert!(!has_text(&s, "Online") && !has_text(&s, "Online files"));
    assert!(!has_text(&s, "Templates") && !has_text(&s, "Start from a template"));
}

#[test]
fn an_open_files_long_name_and_path_share_their_row_without_overlapping() {
    let name = "Quarterly planning: onboarding flow review with annotations and handoff notes";
    let long = "/Users/alex/Documents/Clients/Northwind Traders/2026 Website relaunch/Design reviews/Week 42/onboarding-flow-review.figcraft";
    let short = "/tmp/wireframes.figcraft";
    let mut s = Session::new();
    s.execute("file.rename", &json!({"name": name})).unwrap();
    s.docs[0].path = Some(long.into());
    s.execute("file.new", &json!({"name": "Wireframes"})).unwrap();
    s.docs[1].path = Some(short.into());
    let mut u = Ui::new(s);
    u.app.ui.home = true;
    u.app.session.prefs.getting_started_dismissed = true;
    let s = settle(&mut u);
    assert!(has_text(&s, "Open files"));
    // A row that fits shows its name and path in full.
    assert!(has_text(&s, "Wireframes") && has_text(&s, short));
    // One that doesn't: the name, then the end of its path, side by side.
    let all = texts(&s);
    let rect = |(_, p, size): &(String, Pos2, egui::Vec2)| Rect::from_min_size(*p, *size);
    let detail = all.iter().find(|(t, _, _)| t.ends_with("/onboarding-flow-review.figcraft")).expect("the path");
    let detail_rect = rect(detail);
    let shown_name =
        all.iter().find(|e| e.0.starts_with('Q') && (rect(e).center().y - detail_rect.center().y).abs() < 2.0).expect("the name on the path's row");
    let name_rect = rect(shown_name);
    assert!(shown_name.0.starts_with("Quarterly planning: onboarding") && shown_name.0.ends_with('…'), "{:?}", shown_name.0);
    assert!(name_rect.max.x + 12.0 <= detail_rect.min.x, "{:?} {name_rect:?} overlaps {:?} {detail_rect:?}", shown_name.0, detail.0);
    assert!(detail.0.starts_with('…') && long.ends_with(detail.0.trim_start_matches('…')), "{:?}", detail.0);
    // Hovered, the row shows them in full.
    u.frame(vec![Event::PointerMoved(name_rect.center())]);
    for _ in 0..60 {
        u.frame(vec![]);
    }
    assert!(texts(&shapes(&mut u)).iter().any(|(t, _, _)| t.contains(long)), "no tooltip with the full path");
}

#[test]
fn the_getting_started_card_closes_stays_closed_and_comes_back_from_help() {
    let mut u = first_run();
    let s = settle(&mut u);
    assert!(has_text(&s, "Getting started"));
    for tip in ["Draw a frame", "Add auto layout", "Share a board", "Export"] {
        assert!(has_text(&s, tip), "{tip}");
    }
    // Shortcuts come from the keymap.
    let key = crate::home::shortcut("layout.add").unwrap();
    assert!(has_text(&s, &key), "the auto layout shortcut {key:?}");
    // The close button is named, and closes the card.
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let nodes = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes;
    let close = nodes.iter().map(|(_, n)| n).find(|n| n.label().is_some_and(|l| l.starts_with("Hide these tips"))).expect("a named close button");
    let b = close.bounds().unwrap();
    click(&mut u, Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32));
    assert!(u.app.session.prefs.getting_started_dismissed);
    let s = settle(&mut u);
    assert!(!has_text(&s, "Getting started") && !has_text(&s, "Draw a frame"));
    assert!(has_text(&s, "Join with an invite link"), "the rest of the screen stays");
    // The preference is kept with the others, so it stays closed next time.
    let kept: figcraft_engine::Prefs = serde_json::from_value(serde_json::to_value(&u.app.session.prefs).unwrap()).unwrap();
    assert!(kept.getting_started_dismissed);
    // Help ▸ Getting started: back on the home screen, from wherever.
    u.app.run("file.new", json!({}));
    u.app.ui.home = false;
    crate::menus::invoke(&mut u.app, "help.gettingStarted");
    assert!(u.app.ui.home && u.app.ui.home_view.section == Section::Recents && !u.app.session.prefs.getting_started_dismissed);
    assert!(has_text(&settle(&mut u), "Getting started"));
}

#[test]
fn tab_walks_the_sidebar_in_order_before_a_sections_controls() {
    let mut u = first_run();
    let mut names = Vec::new();
    for _ in 0..40 {
        press(&mut u, Key::Tab);
        let name = crate::a11y::focused_name(&u.ctx);
        if !name.is_empty() && names.last() != Some(&name) {
            names.push(name);
        }
    }
    let at = |name: &str| names.iter().position(|n| n == name).unwrap_or_else(|| panic!("Tab never reached {name:?}: {names:?}"));
    assert!(at("Recents") < at("Open file"), "{names:?}");
    assert!(!names.iter().any(|n| n == "Online"), "no Online tab to reach: {names:?}");
}

/// A temporary directory of its own for a test's files, removed with them when dropped.
struct Temp(std::path::PathBuf);

impl Temp {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("figcraft-home-drop-{}-{stamp}-{n}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if let Ok(files) = std::fs::read_dir(&self.0) {
            for file in files.flatten() {
                let _ = std::fs::remove_file(file.path());
            }
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// A file dropped on the window, as the windowing backend hands it over (read from its path).
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Drag `path` over the window (one frame: what it shows), then drop it (one frame).
fn drop_file(u: &mut Ui, path: &std::path::Path) -> Vec<Shape> {
    let hovered = egui::HoveredFile { path: Some(path.to_path_buf()), mime: String::new() };
    let hovering = flat(&u.frame_raw(egui::RawInput { hovered_files: vec![hovered], ..Default::default() }).shapes);
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path.to_path_buf()));
    u.frame_raw(egui::RawInput { dropped_files: vec![file], ..Default::default() });
    hovering
}

/// A file dropped with its contents but no file on disk (a virtual file from another app).
#[derive(Debug)]
struct DroppedBytes(std::path::PathBuf, Vec<u8>);

impl egui::DroppedFile for DroppedBytes {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.1.clone())
    }
}

#[test]
fn dropping_a_file_without_one_on_disk_opens_it_as_a_new_unsaved_document() {
    let dir = Temp::new();
    let path = dir.path("virtual.svg");
    assert!(!path.exists());
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#.to_vec();
    let mut u = first_run();
    u.app.ui.home = true;
    let file: egui::DroppedFileHandle = std::sync::Arc::new(DroppedBytes(path.clone(), svg));
    u.frame_raw(egui::RawInput { dropped_files: vec![file], ..Default::default() });
    assert_eq!(u.app.ui.pending_opens.len(), 1);
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.ui.last_error, None);
    assert_eq!(u.app.session.docs.len(), 1, "the dropped contents open");
    assert_eq!(u.app.session.docs[0].path, None, "with no save path made up for them");
    assert!(u.app.session.docs[0].doc.nodes().any(|n| matches!(n.kind, figcraft_doc::NodeKind::Rectangle)), "the SVG's content");
    assert!(!u.app.ui.home, "and the editor shows it");
    assert!(!path.exists(), "nothing is written");
}

/// What a drop's read gives, once it has arrived.
type Arrival = std::rc::Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>>;

/// A drop whose contents arrive later, as every drop in the browser does: pending until `arrived`
/// holds them.
fn later(arrived: &Arrival) -> crate::ReadFuture {
    let arrived = arrived.clone();
    Box::pin(std::future::poll_fn(move |_| match arrived.borrow_mut().take() {
        Some(bytes) => std::task::Poll::Ready(bytes),
        None => std::task::Poll::Pending,
    }))
}

#[test]
fn a_dropped_file_whose_contents_arrive_later_opens_once_they_do() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#.to_vec();
    let arrived = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut u = first_run();
    u.app.ui.home = true;
    u.app.ui.pending_opens.push(crate::pending_open::PendingOpen {
        label: "Browser drop.svg".into(),
        params: json!({"name": "Browser drop.svg"}),
        command: "file.open",
        target: None,
        read: Some(later(&arrived)),
    });
    // egui lays a new card out invisibly on its first frame.
    u.frame(vec![]);
    for _ in 0..5 {
        let s = flat(&u.frame_output(vec![]).shapes);
        assert!(has_text(&s, "Browser drop.svg"), "the card shows while the contents are read");
    }
    assert!(u.app.session.docs.is_empty(), "nothing opens before the contents arrive");
    *arrived.borrow_mut() = Some(Ok(svg));
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.ui.last_error, None);
    assert_eq!(u.app.session.docs.len(), 1, "the contents open");
    assert_eq!(u.app.session.docs[0].path, None);
    assert!(u.app.session.docs[0].doc.nodes().any(|n| matches!(n.kind, figcraft_doc::NodeKind::Rectangle)), "the SVG's content");
    assert!(!u.app.ui.home);
}

#[test]
fn a_dropped_file_that_cannot_be_read_says_so_and_opens_nothing() {
    let arrived = std::rc::Rc::new(std::cell::RefCell::new(Some(Err(crate::unreadable("Unreadable.figcraft")))));
    let mut u = first_run();
    u.app.ui.home = true;
    u.app.ui.pending_opens.push(crate::pending_open::PendingOpen {
        label: "Unreadable.figcraft".into(),
        params: json!({"name": "Unreadable.figcraft"}),
        command: "file.open",
        target: None,
        read: Some(later(&arrived)),
    });
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert!(u.app.session.docs.is_empty());
    assert_eq!(u.app.ui.last_error.as_deref(), Some("Couldn't read Unreadable.figcraft."));
}

#[test]
fn dropping_a_native_file_on_the_home_screen_opens_it() {
    let dir = Temp::new();
    let path = dir.path("Dropped.figcraft");
    let mut saved = Session::new();
    saved.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(path.exists());
    let mut u = first_run();
    u.app.ui.home = true;
    let s = drop_file(&mut u, &path);
    assert!(has_text(&s, "Drop to open"), "dragging over the home screen says what a drop does");
    // The file waits behind its card, which is on screen before it opens.
    assert_eq!(u.app.ui.pending_opens.len(), 1);
    assert!(u.app.session.docs.is_empty(), "nothing opens in the frame of the drop");
    let s = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&s, "Opening as a new file") && has_text(&s, "Opening the file…") && has_text(&s, "Dropped.figcraft"), "the card shows");
    assert_eq!(u.app.ui.pending_opens.len(), 1);
    assert!(u.app.session.docs.is_empty(), "the card is drawn first");
    assert_eq!(u.app.inspect()["pendingOpens"], 1);
    u.frame(vec![]);
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.session.docs.len(), 1);
    assert_eq!(u.app.session.docs[0].path.as_deref(), Some(path.to_string_lossy().as_ref()));
    assert!(!u.app.ui.home, "and the editor shows it");
    assert!(u.app.ui.toast.as_ref().is_none_or(|t| !t.0.starts_with("Drop a")), "no turning away");
}

#[test]
fn dropping_a_native_file_on_canvas_keeps_its_receiving_document_after_tab_switch() {
    let dir = Temp::new();
    let path = dir.path("Canvas import.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#).unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    let target_page = session.page().unwrap();
    let target_name = session.doc().unwrap().name.clone();
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path.clone()));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    let pending = u.app.ui.pending_opens.first().unwrap();
    assert_eq!(pending.target.as_ref().map(|target| (target.uid, target.page)), Some((target_uid, target_page)));
    assert_eq!(pending.target.as_ref().map(|target| target.name.as_str()), Some(target_name.as_str()));

    u.frame(vec![]);
    u.app.session.active = 1;
    assert_eq!(u.app.session.ds().unwrap().uid, other_uid, "the UI switched tabs while the card was displayed");
    assert_eq!(u.app.ui.pending_opens.len(), 1, "the switch happens before dispatch");
    u.frame(vec![]);

    assert_eq!(u.app.session.ds().unwrap().uid, other_uid, "the user's active tab stays in front");
    let layers = |uid: u64| {
        let doc = &u.app.session.docs.iter().find(|doc| doc.uid == uid).unwrap().doc;
        doc.node(doc.pages[0]).unwrap().children.len()
    };
    assert_eq!((layers(target_uid), layers(other_uid)), (1, 0), "the drop lands in the tab that received it");
}

#[test]
fn dropping_a_document_on_canvas_opens_it_as_a_new_file() {
    let dir = Temp::new();
    let path = dir.path("Dropped.figcraft");
    let mut saved = Session::new();
    saved.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    u.frame(vec![]);
    let before = u.app.session.doc().unwrap().clone();
    let s = drop_file(&mut u, &path);
    assert!(has_text(&s, "Drop to open"), "dragging a document over the canvas says it opens");
    let s = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&s, "Opening as a new file"), "the card says so too");
    u.frame(vec![]);
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.session.docs.len(), 2, "a tab of its own");
    assert_eq!(u.app.session.ds().unwrap().path.as_deref(), Some(path.to_string_lossy().as_ref()), "in front");
    assert_eq!(u.app.session.docs[0].doc.as_ref(), &before, "the file that was open gets no pages added");
}

#[test]
fn a_document_dropped_on_canvas_without_a_file_on_disk_opens_as_a_new_file() {
    // Every drop in the browser (#916).
    let dir = Temp::new();
    let saved = dir.path("Saved.figcraft");
    Session::new().execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    let bytes = std::fs::read(&saved).unwrap();
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(DroppedBytes("Shared.figcraft".into(), bytes));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.ui.last_error, None);
    assert_eq!(u.app.session.docs.len(), 2, "a tab of its own");
    assert_eq!(u.app.session.docs[0].doc.pages.len(), 1, "no pages added to the open file");
    assert_eq!(u.app.session.docs[1].path, None, "with no save path made up for it");
}

#[test]
fn dropping_several_files_on_canvas_places_them_side_by_side() {
    let dir = Temp::new();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#f00"/></svg>"##;
    let paths: Vec<_> = ["a.svg", "b.svg", "c.svg"].iter().map(|n| dir.path(n)).collect();
    for path in &paths {
        std::fs::write(path, svg).unwrap();
    }
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let files: Vec<egui::DroppedFileHandle> = paths.iter().map(|p| std::sync::Arc::new(Dropped(p.clone())) as egui::DroppedFileHandle).collect();
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: files, ..Default::default() });
    for _ in 0..4 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    let doc = u.app.session.doc().unwrap();
    let page = u.app.session.page().unwrap();
    let rects: Vec<_> = doc.node(page).unwrap().children.iter().map(|c| doc.world_bounds(*c).unwrap()).collect();
    assert_eq!(rects.len(), 3);
    for pair in rects.windows(2) {
        assert!(pair[1].x0 > pair[0].x1, "left to right with a gap: {rects:?}");
    }
}

#[test]
fn canvas_import_uses_queued_page_and_restores_the_page_selected_while_waiting() {
    let dir = Temp::new();
    let path = dir.path("Queued.svg");
    std::fs::write(&path, r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#f00"/></svg>"##)
        .unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    let first_page = session.page().unwrap();
    session.execute("page.add", &json!({"name": "Second page"})).unwrap();
    let second_page = session.page().unwrap();
    let second_page_shape = session.execute("node.create", &json!({"type": "rectangle", "x": 30, "y": 40, "width": 12, "height": 14})).unwrap()["id"]
        .as_u64()
        .map(figcraft_doc::NodeId)
        .unwrap();
    session.execute("page.select", &json!({"id": first_page})).unwrap();
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    let queued_params = u.app.ui.pending_opens[0].params.clone();
    assert_eq!(u.app.ui.pending_opens[0].target.as_ref().map(|target| (target.uid, target.page)), Some((target_uid, first_page)));

    u.frame(vec![]);
    u.app.session.execute("page.select", &json!({"id": second_page})).unwrap();
    u.app.session.execute("select.set", &json!({"ids": [second_page_shape]})).unwrap();
    u.app.session.active = 1;
    u.frame(vec![]);
    let target_index = u.app.session.docs.iter().position(|doc| doc.uid == target_uid).unwrap();
    let imported = u.app.session.docs[target_index].doc.node(first_page).unwrap().children[0];
    let doc = &u.app.session.docs[target_index].doc;
    assert_eq!(doc.page_of(imported), Some(first_page));
    let bounds = doc.world_bounds(imported).unwrap();
    assert!((bounds.x0 - queued_params["x"].as_f64().unwrap()).abs() < 1.0);
    assert!((bounds.y0 - queued_params["y"].as_f64().unwrap()).abs() < 1.0);
    assert_eq!(doc.node(second_page).unwrap().children, [second_page_shape]);
    assert_eq!(u.app.session.ds().unwrap().uid, other_uid);
    assert_eq!(u.app.session.docs[target_index].page, second_page, "the page selected during the wait is restored");
    assert_eq!(u.app.session.docs[target_index].selection, [second_page_shape], "the restored page keeps its selection");

    u.app.session.active = target_index;
    u.app.session.execute("edit.delete", &json!({})).unwrap();
    let doc = &u.app.session.docs[target_index].doc;
    assert!(doc.node(second_page).unwrap().children.is_empty(), "the next edit acts on the restored page selection");
    assert_eq!(doc.node(first_page).unwrap().children.len(), 1, "the imported object on the queued page remains");
}

#[test]
fn queued_canvas_import_is_cancelled_if_its_page_is_deleted() {
    let dir = Temp::new();
    let path = dir.path("Removed target.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#).unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    session.execute("page.add", &json!({"name": "Keep this page"})).unwrap();
    let target_page = session.page().unwrap();
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    u.app.session.execute("page.delete", &json!({"id": target_page})).unwrap();
    let other_before = u.app.session.docs.iter().find(|doc| doc.uid == other_uid).unwrap().doc.clone();
    u.app.ui.pending_open_frames = 2;
    crate::pending_open::pending_opens(&mut u.app, &u.ctx);
    assert_eq!(u.app.session.docs.iter().find(|doc| doc.uid == other_uid).unwrap().doc, other_before);
    let target = u.app.session.docs.iter().find(|doc| doc.uid == target_uid).unwrap();
    assert_eq!(target.doc.pages.len(), 1);
    assert!(!target.doc.nodes().any(|node| node.name == "Removed target"));
    assert!(u.app.ui.last_error.as_deref().is_some_and(|error| error.contains("destination page was removed")));
}

#[test]
fn queued_canvas_import_is_cancelled_if_its_document_is_closed() {
    let dir = Temp::new();
    let path = dir.path("Closed target.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#).unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    u.frame(vec![]);
    let other_before = u.app.session.docs.iter().find(|doc| doc.uid == other_uid).unwrap().doc.clone();
    let target_index = u.app.session.docs.iter().position(|doc| doc.uid == target_uid).unwrap();
    u.app.session.docs.remove(target_index);
    u.app.session.active = 0;

    while !u.app.ui.pending_opens.is_empty() {
        u.frame(vec![]);
    }
    assert_eq!(u.app.session.docs.len(), 1);
    assert_eq!(u.app.session.ds().unwrap().uid, other_uid);
    assert_eq!(u.app.session.doc().unwrap(), other_before.as_ref());
    assert!(u.app.ui.last_error.as_deref().is_some_and(|error| error.contains("destination document was closed")));
}

#[test]
fn dropping_an_unsupported_file_on_the_home_screen_opens_nothing() {
    // The formats of other apps are not read: they are as unsupported as a text file.
    for name in ["unsupported.txt", "other.fig", "board.jam", "other.sketch"] {
        let dir = Temp::new();
        let path = dir.path(name);
        std::fs::write(&path, "not a design").unwrap();
        let mut u = first_run();
        u.app.ui.home = true;
        drop_file(&mut u, &path);
        assert!(u.app.session.docs.is_empty(), "{name}");
        assert!(u.app.ui.home, "{name}");
        assert_eq!(u.app.ui.toast.as_ref().map(|t| t.0.as_str()), Some("Drop a .figcraft, .json, .svg or image file."), "{name}");
    }
}

/// The Preferences dialog can be opened and toggled from the home screen via
/// the `Cmd/Ctrl + ,` shortcut as well as by clicking Preferences in the sidebar.
#[test]
fn home_screen_preferences_shortcut_and_sidebar_access() {
    let mut u = first_run();
    u.app.ui.home = true;
    u.frame(vec![]);
    assert!(!u.app.ui.preferences);

    // Ctrl/Cmd + Comma opens Preferences on the Home screen.
    u.frame(vec![
        Event::Key { key: Key::Comma, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
        Event::Key { key: Key::Comma, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::COMMAND },
    ]);
    assert!(u.app.ui.preferences, "Ctrl/Cmd+, should open Preferences from the Home screen");

    // Escape closes Preferences.
    press(&mut u, Key::Escape);
    assert!(!u.app.ui.preferences, "Escape should close Preferences");

    // Clicking Preferences in the sidebar opens Preferences.
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Preferences"));
    assert!(u.app.ui.preferences, "Clicking Preferences in sidebar should open Preferences");

    // Clicking it again toggles it closed.
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Preferences"));
    assert!(!u.app.ui.preferences, "Clicking Preferences in sidebar again should close it");
}
