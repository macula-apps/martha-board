//! Tests: a colour variable bound to a layer's paint, written into the document as a file holds it
//! (the variables editor and the `variable.*` commands left, #16).

use figcraft_doc::{Collection, Mode, NodeId, Variable, VariableType, VariableValue};
use figcraft_engine::Session;
use serde_json::json;

/// Paint `field` (`fill`, `fills/<i>/color`) of `ids` `hex` and bind it to a new colour variable
/// `name` of that value, in a one-mode collection "Theme" → the variable's id.
pub(crate) fn bind_color(s: &mut Session, ids: &[NodeId], field: &str, name: &str, hex: &str) -> String {
    let index = field.strip_prefix("fills/").and_then(|r| r.split('/').next()).and_then(|i| i.parse::<usize>().ok()).unwrap_or(0);
    s.execute("fill.set", &json!({"ids": ids, "index": index, "color": hex})).unwrap();
    let doc = s.doc_mut().unwrap();
    let cid = match doc.variables.collection("Theme") {
        Some(c) => c.id.clone(),
        None => {
            let cid = doc.new_collection_id();
            let modes = vec![Mode { id: format!("{cid}/mode1"), name: "Mode 1".into() }];
            doc.variables.collections.push(Collection { id: cid.clone(), name: "Theme".into(), modes, extends: None });
            cid
        }
    };
    let mode = doc.variables.collection_by_id(&cid).unwrap().modes[0].id.clone();
    let id = doc.new_variable_id();
    doc.variables.variables.push(Variable {
        id: id.clone(),
        name: name.into(),
        collection: cid,
        kind: VariableType::Color,
        values: [(mode, VariableValue::Color(figcraft_color::Color::parse_css(hex).unwrap()))].into_iter().collect(),
        description: String::new(),
        scopes: None,
        hidden_from_publishing: false,
        extends: None,
        overrides: Default::default(),
        code_syntax: Default::default(),
    });
    for n in ids {
        doc.get_mut(*n).unwrap().bound.insert(field.into(), id.clone());
    }
    id
}
