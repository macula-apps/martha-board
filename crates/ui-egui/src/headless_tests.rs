//! A FigApp driven headless through egui, for the tests: frames of raw input, commands run through
//! the session with the app's UI requests handled, pointer presses and key presses.

use egui::{Event, Key, Pos2, RawInput, Rect, vec2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::Value;

use crate::{FigApp, NoServices};

pub(crate) struct Ui {
    pub(crate) ctx: egui::Context,
    pub(crate) app: FigApp,
    pub(crate) time: f64,
    /// A display's pixels per point and the biggest texture side its GPU takes (None: egui's defaults).
    pub(crate) hidpi: Option<(f32, usize)>,
}

impl Ui {
    pub(crate) fn new(s: Session) -> Self {
        let mut u = Self { ctx: egui::Context::default(), app: FigApp::new(s, Box::new(NoServices)), time: 0.0, hidpi: None };
        u.frames(3, Vec::new);
        u
    }

    pub(crate) fn frame(&mut self, events: Vec<Event>) {
        self.frame_output(events);
    }

    /// [`Self::frame`], returning what egui output (the AccessKit tree when it's enabled).
    pub(crate) fn frame_output(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        let mut input =
            RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), time: Some(self.time), events, ..Default::default() };
        if let Some((ppp, side)) = self.hidpi {
            input.max_texture_side = Some(side);
            input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        }
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(input, |ui| app.ui(ui));
        out.textures_delta.clear();
        out
    }

    pub(crate) fn frames(&mut self, n: usize, events: impl Fn() -> Vec<Event>) {
        for _ in 0..n {
            self.frame(events());
        }
    }

    pub(crate) fn run(&mut self, cmd: &str, params: Value) -> Value {
        let r = self.app.session.execute(cmd, &params).unwrap();
        self.app.handle_ui_requests();
        self.frame(vec![]);
        r
    }

    pub(crate) fn button(&mut self, p: Pos2, pressed: bool) {
        self.frame(vec![
            Event::PointerMoved(p),
            Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() },
        ]);
    }

    pub(crate) fn key(&mut self, at: Pos2, key: Key) {
        let k = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        self.frame(vec![Event::PointerMoved(at), k(true), k(false)]);
    }
}

pub(crate) fn id(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}
