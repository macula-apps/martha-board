//! Every 24 pt icon button paints its glyph at 16 pt, the size `icons::button` draws (#2238, after
//! the Design panel's in #2227). The panels' own buttons: the Layers panel's header. The tests of the other panels sit beside
//! them (the `icon_size_tests` modules of `versions`, `online` and the style picker).

use figcraft_engine::Session;

use crate::header_icon_size_tests::{painted_rects, paints_icon_at_button_size};
use crate::icons::Icon;
use crate::{FigApp, NoServices};

fn app() -> FigApp {
    FigApp::new(Session::new(), Box::new(NoServices))
}

#[test]
fn layers_icon_buttons_paint_16pt() {
    let mut a = app();
    let rail = painted_rects(None, |ui| crate::layers::left_panel(&mut a, ui));
    for icon in [Icon::Panels, Icon::Search, Icon::Plus] {
        assert!(paints_icon_at_button_size(&rail, icon), "Layers panel: {icon:?} is not 16 pt");
    }
}
