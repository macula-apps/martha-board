//! The window that makes a colour the eyedropper picked (with Cmd/Ctrl+Shift) into a colour style:
//! it opens from the eyedropper's request and Create makes the style.

use serde_json::json;

use crate::export_preview_tests::{click, labelled};
use crate::layers_drag_tests::Ui;

fn opened(s: figcraft_engine::Session) -> Ui {
    let mut u = Ui::new(s);
    u.app.session.request_ui("dialog.createFromColor", json!({"color": "#ff8800"}));
    u.app.handle_ui_requests();
    u.frame(vec![]);
    u.frame(vec![]);
    u
}

fn press(u: &mut Ui, text: &str) {
    let shown = labelled(u);
    let at = shown.iter().find(|(t, _)| t == text).map(|x| x.1).unwrap_or_else(|| panic!("{text:?} in {shown:?}"));
    click(u, at);
}

#[test]
fn the_eyedroppers_request_opens_the_window_and_create_makes_a_style() {
    let mut u = opened(figcraft_engine::Session::new());
    let s = u.app.ui.sampled_color.clone().expect("the window is open");
    assert_eq!((s.color.as_str(), s.name.as_str()), ("#FF8800", "Color 1"));
    let texts: Vec<String> = labelled(&mut u).into_iter().map(|(t, _)| t).collect();
    assert!(texts.iter().any(|t| t == "Create from color") && texts.iter().any(|t| t == "FF8800"), "{texts:?}");
    press(&mut u, "Create");
    let doc = u.app.session.doc().unwrap();
    let style = doc.styles.iter().find(|s| s.name == "Color 1").expect("a style");
    let colour = match &style.kind {
        figcraft_doc::StyleKind::Paint { paints } => paints.first().and_then(|p| p.solid_color()).map(|c| c.to_hex()),
        _ => None,
    };
    assert_eq!(colour.as_deref(), Some("FF8800"), "a paint style of the colour: {style:?}");
    assert!(u.app.ui.sampled_color.is_none(), "it closes");
}
