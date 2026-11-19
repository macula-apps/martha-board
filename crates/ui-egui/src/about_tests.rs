//! Help ▸ About shows MarthaBoard's line, as the window paints it.

use figcraft_engine::Session;
use serde_json::json;

use crate::export_preview_tests::labelled;
use crate::layers_drag_tests::Ui;

#[test]
fn the_about_tab_says_what_marthaboard_is() {
    let mut u = Ui::new(Session::new());
    u.app.run("help.about", json!({}));
    assert!(u.app.ui.about, "help.about opens the About window");
    let lines: Vec<String> = labelled(&mut u).into_iter().map(|(t, _)| t).collect();
    assert!(lines.iter().any(|t| t == "MarthaBoard: the board for the Martha methodology."), "the About tab paints {lines:?}");
}
