//! The toolbar has no Resources button (#1210).

use egui::accesskit::Role;
use figcraft_engine::Session;

use crate::layers_drag_tests::Ui;

/// The toolbar's buttons, as screen readers get them.
fn toolbar_labels(u: &mut Ui) -> Vec<String> {
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let out = u.frame_output(vec![]);
    out.platform_output
        .accesskit_update
        .expect("an AccessKit update")
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::Button)
        .filter_map(|(_, n)| n.label().map(str::to_string))
        .collect()
}

#[test]
fn the_toolbar_has_no_resources_button() {
    let mut u = Ui::new(Session::new());
    let labels = toolbar_labels(&mut u);
    // Actions stays (the palette's button); Resources went (#1210).
    assert!(labels.iter().any(|l| l == "Actions"), "Actions is in the toolbar: {labels:?}");
    assert!(!labels.iter().any(|l| l == "Resources"), "no Resources button: {labels:?}");
}
