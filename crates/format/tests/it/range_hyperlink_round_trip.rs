//! A hyperlink cleared on part of a text layer stays cleared through a native save and load.

use figcraft_doc::{Document, NodeKind, TextData};

fn links(doc: &Document, name: &str) -> Vec<Option<String>> {
    let Some(NodeKind::Text(text)) = doc.nodes().find(|n| n.name == name).map(|n| &n.kind) else { return Vec::new() };
    text.spans().into_iter().flat_map(|(start, end, style)| std::iter::repeat_n(style.link.clone(), end - start)).collect()
}

#[test]
fn cleared_range_hyperlink_round_trip() {
    let mut document = Document::new("File");
    let page = document.first_page().unwrap();
    let url = "https://example.com/".to_string();

    let mut text = TextData { characters: "AB".into(), ..Default::default() };
    text.style.link = Some(url.clone());
    text.restyle(1, 2, |style| style.link = None);
    document.create(page, NodeKind::Text(text), "Cleared", 0.0, 0.0, 100.0, 30.0).unwrap();

    let mut control = TextData { characters: "AB".into(), ..Default::default() };
    control.restyle(1, 2, |style| style.link = Some(url.clone()));
    document.create(page, NodeKind::Text(control), "Explicit", 0.0, 40.0, 100.0, 30.0).unwrap();

    let expected = vec![Some(url.clone()), None];
    assert_eq!(links(&document, "Cleared"), expected);
    let native_bytes = figcraft_format::save_bytes(&document, None).unwrap();
    let native = figcraft_format::load_bytes(&native_bytes).unwrap().0;
    assert_eq!(links(&native, "Cleared"), expected);
    assert_eq!(links(&native, "Explicit"), vec![None, Some(url)]);
}
