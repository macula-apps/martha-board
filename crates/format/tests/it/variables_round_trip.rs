//! Nothing in the app edits variables any more (#16), but a file that carries them still opens,
//! saves them unchanged and draws its bound values: a native save and load keeps the collections,
//! modes, values and bindings (on layers and styles) equal and their JSON byte-identical, and a
//! bound fill still resolves to its variable's value in the mode its frame sets.

use std::collections::{BTreeMap, BTreeSet};

use figcraft_doc::color::{Color, Paint};
use figcraft_doc::{Collection, Document, Mode, NodeId, NodeKind, Style, StyleKind, Variable, VariableType, VariableValue, Variables};

/// What the round trip must keep: the variables, each layer's bindings and modes, each style's
/// bindings, and all of them as JSON text (None: a layer is missing).
type Carried = (Variables, Vec<(BTreeMap<String, String>, BTreeMap<String, String>)>, Vec<BTreeMap<String, String>>, String);

fn carried(doc: &Document, layers: &[NodeId]) -> Option<Carried> {
    let bound = layers.iter().map(|l| doc.node(*l).map(|n| (n.bound.clone(), n.var_modes.clone()))).collect::<Option<Vec<_>>>()?;
    let styles: Vec<_> = doc.styles.iter().map(|s| s.bound.clone()).collect();
    let text = serde_json::to_string(&(&doc.variables, &bound, &styles)).ok()?;
    Some((doc.variables.clone(), bound, styles, text))
}

fn variable(id: &str, name: &str, kind: VariableType, values: [(&str, VariableValue); 2]) -> Variable {
    Variable {
        id: id.into(),
        name: name.into(),
        collection: "C:1".into(),
        kind,
        values: values.into_iter().map(|(m, v)| (m.to_string(), v)).collect(),
        description: String::new(),
        scopes: None,
        hidden_from_publishing: false,
        extends: None,
        overrides: BTreeSet::new(),
        code_syntax: BTreeMap::new(),
    }
}

/// A frame on the Dark mode holding a rectangle whose fill, corner radius and opacity are bound,
/// a paint style bound to the brand colour, and a "Theme" collection (Light, Dark) of a colour, a
/// number and an alias variable, one with a description, scopes, code syntax and hidden from
/// publishing. The document, the frame and the rectangle (None: one isn't made).
fn fixture() -> Option<(Document, NodeId, NodeId)> {
    let mut doc = Document::new("Variables carrier");
    let page = doc.first_page()?;
    let frame = doc.create(page, NodeKind::Frame, "Card", 0.0, 0.0, 200.0, 120.0).ok()?;
    let rect = doc.create(frame, NodeKind::Rectangle, "Swatch", 20.0, 20.0, 40.0, 40.0).ok()?;
    let (light, dark) = ("C:1/mode1", "C:1/mode2");
    let modes = vec![Mode { id: light.into(), name: "Light".into() }, Mode { id: dark.into(), name: "Dark".into() }];
    doc.variables.collections.push(Collection { id: "C:1".into(), name: "Theme".into(), modes, extends: None });
    let brand = [(light, VariableValue::Color(Color::from_hex("#1F6F5C")?)), (dark, VariableValue::Color(Color::from_hex("#DDEFE8")?))];
    let mut brand = variable("V:1", "Brand", VariableType::Color, brand);
    brand.description = "The brand colour".into();
    brand.scopes = Some(vec!["FRAME_FILL".into(), "SHAPE_FILL".into()]);
    brand.code_syntax.insert("WEB".into(), "var(--brand)".into());
    let radius = variable("V:2", "Radius", VariableType::Number, [(light, VariableValue::Number(4.0)), (dark, VariableValue::Number(12.5))]);
    let mut accent =
        variable("V:3", "Accent", VariableType::Color, [(light, VariableValue::Alias("V:1".into())), (dark, VariableValue::Alias("V:1".into()))]);
    accent.hidden_from_publishing = true;
    doc.variables.variables = vec![brand, radius, accent];
    let n = doc.get_mut(rect).ok()?;
    n.fills = vec![Paint::solid(Color::from_hex("#1F6F5C")?)];
    n.bound = BTreeMap::from([("fill".into(), "V:3".into()), ("cornerRadius".into(), "V:2".into()), ("opacity".into(), "V:2".into())]);
    doc.get_mut(frame).ok()?.var_modes.insert("C:1".into(), dark.into());
    let paint = StyleKind::Paint { paints: vec![Paint::solid(Color::from_hex("#1F6F5C")?)] };
    let style = Style {
        id: "S:1".into(),
        name: "Brand".into(),
        description: String::new(),
        kind: paint,
        bound: BTreeMap::from([("fill".into(), "V:1".into())]),
    };
    doc.styles.push(style);
    Some((doc, frame, rect))
}

#[test]
fn variables_modes_and_bindings_survive_a_save_and_load() {
    let (document, frame, rect) = fixture().unwrap();
    let layers = [frame, rect];
    let before = carried(&document, &layers).unwrap();
    assert_eq!((before.0.collections.len(), before.0.variables.len()), (1, 3), "the fixture is built");
    assert_eq!(before.1[1].0.len(), 3, "the fixture is built");

    let bytes = figcraft_format::save_bytes(&document, None).unwrap();
    let (opened, warnings) = figcraft_format::load_bytes(&bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let after_load = carried(&opened, &layers).unwrap();
    assert_eq!(after_load.0, before.0, "collections, modes and variables");
    assert_eq!(after_load.1, before.1, "the layers' bindings and modes");
    assert_eq!(after_load.2, before.2, "the styles' bindings");
    assert_eq!(after_load.3, before.3, "the same bytes");

    // Saved again, unchanged.
    let again = figcraft_format::save_bytes(&opened, None).unwrap();
    let (reopened, _) = figcraft_format::load_bytes(&again).unwrap();
    assert_eq!(carried(&reopened, &layers).unwrap().3, before.3, "the same bytes after a second save");

    // The bound fill resolves (through the alias) to the brand colour of the mode its frame sets:
    // Dark, not the Light value the layer was saved with.
    let modes = reopened.modes_for(rect);
    assert_eq!(modes.get("C:1").map(String::as_str), Some("C:1/mode2"), "the rectangle takes its frame's mode");
    let fill = reopened.node(rect).unwrap().bound.get("fill").cloned().unwrap();
    let shown = reopened.variables.resolve(&fill, &modes);
    assert_eq!(shown, Some(VariableValue::Color(Color::from_hex("#DDEFE8").unwrap())), "the Dark value");
    let radius = reopened.variables.resolve("V:2", &modes);
    assert_eq!(radius, Some(VariableValue::Number(12.5)));
}
