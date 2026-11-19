//! Nothing in the app sets prototype interactions, flows or a page's prototype settings any more
//! (#14), but a file that carries them still opens and saves them unchanged: a native save and load
//! keeps those fields equal, and they serialize to the same bytes.

use figcraft_doc::color::Color;
use figcraft_doc::proto::{
    Action, DeviceRotation, Direction, Easing, Flow, Interaction, OverlayAnchor, OverlayPosition, PrototypeSettings, Transition, Trigger,
};
use figcraft_doc::{Document, NodeId, NodeKind};

/// What the round trip must keep: each frame's interactions and the first page's flows and
/// prototype settings, and all of them as JSON text (None: a frame or the page is missing).
type Carried = (Vec<Vec<Interaction>>, Vec<Flow>, PrototypeSettings, String);

fn carried(doc: &Document, frames: &[NodeId]) -> Option<Carried> {
    let interactions = frames.iter().map(|f| doc.node(*f).map(|n| n.interactions.clone())).collect::<Option<Vec<_>>>()?;
    let NodeKind::Page { flows, prototype, .. } = &doc.node(doc.first_page()?)?.kind else { return None };
    let text = serde_json::to_string(&(&interactions, flows, prototype)).ok()?;
    Some((interactions, flows.clone(), prototype.clone(), text))
}

#[test]
fn interactions_flows_and_prototype_settings_survive_a_save_and_load() {
    let mut document = Document::new("Prototype carrier");
    let page = document.first_page().unwrap();
    let a = document.create(page, NodeKind::Frame, "Start", 0.0, 0.0, 390.0, 844.0).unwrap();
    let b = document.create(page, NodeKind::Frame, "Details", 500.0, 0.0, 390.0, 844.0).unwrap();
    let overlay = document.create(page, NodeKind::Frame, "Sheet", 1000.0, 0.0, 390.0, 300.0).unwrap();
    let click = Interaction {
        trigger: Trigger::OnClick,
        actions: vec![Action::Navigate { dest: b, preserve_scroll: true }],
        transition: Transition::Push { direction: Direction::Right },
        duration_ms: 450,
        easing: Easing::CustomBezier { x1: 0.25, y1: 0.1, x2: 0.3, y2: 0.95 },
        delay_ms: 0,
    };
    let key = Interaction {
        trigger: Trigger::KeyPress { key: "Shift+K".into() },
        actions: vec![
            Action::OpenOverlay {
                dest: overlay,
                position: OverlayPosition::Manual,
                close_on_outside: false,
                background: Color::from_hex("#10203080"),
                offset: Some([12.0, 34.0]),
                anchor: OverlayAnchor::TriggerOutside,
            },
            Action::OpenUrl { url: "https://example.com/help".into(), new_tab: false },
        ],
        transition: Transition::Dissolve,
        duration_ms: 120,
        easing: Easing::Linear,
        delay_ms: 0,
    };
    let leave = Interaction {
        trigger: Trigger::MouseLeave,
        actions: vec![Action::Back, Action::CloseOverlay],
        transition: Transition::SmartAnimate,
        duration_ms: 800,
        easing: Easing::Bouncy,
        delay_ms: 250,
    };
    let after = Interaction {
        trigger: Trigger::AfterDelay { ms: 1500 },
        actions: vec![Action::SwapOverlay { dest: overlay }],
        transition: Transition::SlideIn { direction: Direction::Bottom },
        duration_ms: 200,
        easing: Easing::EaseInOut,
        delay_ms: 0,
    };
    document.get_mut(a).unwrap().interactions = vec![click, key];
    document.get_mut(b).unwrap().interactions = vec![leave, after];
    let NodeKind::Page { flows, prototype, .. } = &mut document.get_mut(page).unwrap().kind else { panic!("the first page is a page") };
    *flows = vec![
        Flow { name: "Onboarding".into(), start: a, description: "First run, from the welcome screen".into() },
        Flow { name: "Details".into(), start: b, description: String::new() },
    ];
    *prototype = PrototypeSettings {
        device: "iPhone 16".into(),
        background: Color::from_hex("#F0E0D0").unwrap(),
        rotation: DeviceRotation::Landscape,
        color: "Ultramarine".into(),
        custom_size: [800.0, 600.0],
        responsive: true,
    };

    let frames = [a, b, overlay];
    let before = carried(&document, &frames).unwrap();
    assert_eq!(before.0[0].len(), 2, "the fixture is built");
    assert_eq!(before.1.len(), 2, "the fixture is built");

    let bytes = figcraft_format::save_bytes(&document, None).unwrap();
    let (opened, warnings) = figcraft_format::load_bytes(&bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let after_load = carried(&opened, &frames).unwrap();
    assert_eq!(after_load.0, before.0, "interactions");
    assert_eq!(after_load.1, before.1, "flows");
    assert_eq!(after_load.2, before.2, "prototype settings");
    assert_eq!(after_load.3, before.3, "the same bytes");

    // Saved again, unchanged.
    let again = figcraft_format::save_bytes(&opened, None).unwrap();
    let (reopened, _) = figcraft_format::load_bytes(&again).unwrap();
    assert_eq!(carried(&reopened, &frames).unwrap().3, before.3, "the same bytes after a second save");
}
