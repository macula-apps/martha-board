//! A native save and load keeps all three distributed main-axis alignments distinct.

use figcraft_doc::{AutoLayout, Document, MainAlign, NodeKind};

#[test]
fn distributed_alignment_round_trip() {
    for expected in [MainAlign::SpaceBetween, MainAlign::SpaceAround, MainAlign::SpaceEvenly] {
        let mut doc = Document::new("Alignment");
        let page = doc.first_page().unwrap();
        let frame = doc.create(page, NodeKind::Frame, "Frame", 0.0, 0.0, 200.0, 40.0).unwrap();
        doc.node_mut(frame).unwrap().layout = Some(AutoLayout { main_align: expected, ..Default::default() });
        let (native, _) = figcraft_format::load_bytes(&figcraft_format::save_bytes(&doc, None).unwrap()).unwrap();
        assert_eq!(native.node(frame).unwrap().layout.as_ref().unwrap().main_align, expected);
    }
}
