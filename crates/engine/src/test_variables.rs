//! Tests: variables, bindings and modes written into a session's document from literal values, as
//! a file carrying them holds them (the `variable.*` commands left the app, #16). Each call
//! normalises like an edit (bound layers take their values) but is no undo step.

use std::collections::{BTreeMap, BTreeSet};

use figcraft_doc::{Collection, CollectionId, Document, Mode, ModeId, NodeId, Variable, VariableId, VariableType, VariableValue};
use serde_json::Value;

use crate::{Result, Session};

/// A collection `name` with `modes` (the first is the default) → its id.
pub(crate) fn collection(s: &mut Session, name: &str, modes: &[&str]) -> String {
    edit(s, |doc| Ok(add_collection(doc, name, modes))).unwrap()
}

/// A variable `name` of `kind` (color, number, string or boolean) in `collection` (name or id), with
/// one value per mode in order (a single value: every mode) → its id.
pub(crate) fn variable(s: &mut Session, collection: &str, name: &str, kind: &str, values: &[Value]) -> String {
    let kind = kind_of(kind).unwrap_or_else(|| panic!("no variable type `{kind}`"));
    edit(s, |doc| {
        let coll = doc.variables.collection(collection).unwrap_or_else(|| panic!("no collection `{collection}`")).clone();
        let mut by_mode = BTreeMap::new();
        for (i, m) in coll.modes.iter().enumerate() {
            let v = if values.len() == 1 { &values[0] } else { &values[i] };
            by_mode.insert(m.id.clone(), value_of(kind, v).unwrap_or_else(|| panic!("`{name}`: not a {} value: {v}", kind.id())));
        }
        Ok(add_variable(doc, &coll.id, name, kind, by_mode))
    })
    .unwrap()
}

/// Set `variable`'s (name or id) value in `mode` (name or id) to the plain `value`, as a
/// collaborator's edit or a library update brings it.
pub(crate) fn set(s: &mut Session, variable: &str, mode: &str, value: Value) {
    edit(s, |doc| {
        let var = doc.variables.get(variable).unwrap_or_else(|| panic!("no variable `{variable}`")).clone();
        let mid = doc.variables.collection_by_id(&var.collection).and_then(|c| c.find_mode(mode)).map(|m| m.id.clone());
        let mid = mid.unwrap_or_else(|| panic!("no mode `{mode}`"));
        let val = value_of(var.kind, &value).unwrap_or_else(|| panic!("`{variable}`: not a {} value: {value}", var.kind.id()));
        set_value(doc, &var.id, &mid, val);
        Ok(())
    })
    .unwrap();
}

/// Set variable `id`'s value in mode `mode` (ids) to `value` (any [`VariableValue`]). On a variable
/// an extended collection inherits, the value is that mode's override, as a file holds it.
pub(crate) fn set_value(doc: &mut figcraft_doc::Document, id: &str, mode: &str, value: VariableValue) {
    if let Some(slot) = doc.variables.variables.iter_mut().find(|v| v.id == id) {
        if slot.extends.is_some() {
            slot.overrides.insert(mode.to_string());
        }
        slot.values.insert(mode.to_string(), value);
    }
}

/// Bind `field` of `ids` to `variable` (name or id).
pub(crate) fn bind(s: &mut Session, ids: &[NodeId], field: &str, variable: &str) {
    edit(s, |doc| {
        let vid = doc.variables.get(variable).map(|v| v.id.clone()).unwrap_or_else(|| panic!("no variable `{variable}`"));
        for id in ids {
            bind_field(doc, *id, field, &vid)?;
        }
        Ok(())
    })
    .unwrap();
}

/// Show layer `id` (a frame or the page) in `mode` of `collection` (names or ids).
pub(crate) fn mode(s: &mut Session, id: NodeId, collection: &str, mode: &str) {
    edit(s, |doc| {
        let coll = doc.variables.collection(collection).unwrap_or_else(|| panic!("no collection `{collection}`"));
        let mid = coll.find_mode(mode).map(|m| m.id.clone()).unwrap_or_else(|| panic!("no mode `{mode}`"));
        let cid = coll.id.clone();
        set_mode(doc, id, &cid, &mid)
    })
    .unwrap();
}

/// Set every mode's value of `variable` (name or id) to `value` (any [`VariableValue`]: an alias
/// or a colour calculation too, reading variables by id).
pub(crate) fn set_all(s: &mut Session, variable: &str, value: VariableValue) {
    edit(s, |doc| {
        let var = doc.variables.get(variable).unwrap_or_else(|| panic!("no variable `{variable}`")).clone();
        for m in var.values.keys() {
            set_value(doc, &var.id, m, value.clone());
        }
        Ok(())
    })
    .unwrap();
}

/// A collection extending `parent` (an id), named `name` → its id: normalising gives it the
/// parent's modes and one inheriting variable per variable of the parent.
pub(crate) fn extend(s: &mut Session, parent: &str, name: &str) -> String {
    edit(s, |doc| {
        let id = doc.new_collection_id();
        let c = figcraft_doc::Collection { id: id.clone(), name: name.to_string(), modes: Vec::new(), extends: Some(parent.to_string()) };
        doc.variables.collections.push(c);
        Ok(id)
    })
    .unwrap()
}

/// Bind `field` of the shared style `style` (id or name) to `variable` (name or id): every layer
/// using the style shows the field resolved in its own modes.
pub(crate) fn bind_style(s: &mut Session, style: &str, field: &str, variable: &str) {
    edit(s, |doc| {
        let vid = doc.variables.get(variable).map(|v| v.id.clone()).unwrap_or_else(|| panic!("no variable `{variable}`"));
        let st = doc.styles.iter_mut().find(|x| x.id == style || x.name == style).unwrap_or_else(|| panic!("no style `{style}`"));
        let key = crate::style_vars::canonical(field);
        if let figcraft_doc::StyleKind::Paint { paints } = &mut st.kind {
            crate::cmd::styles::rebind_paint_alpha(paints, &mut [], &key, true);
        }
        // A style binds its weight as a number or as a face name, not both.
        match key.as_str() {
            "fontWeight" => st.bound.remove("fontStyle"),
            "fontStyle" => st.bound.remove("fontWeight"),
            _ => None,
        };
        st.bound.insert(key, vid);
        Ok(())
    })
    .unwrap();
}

/// Unbind `field` of layer `id`, keeping the value it shows (as a file can hold it unbound).
pub(crate) fn unbind(s: &mut Session, id: NodeId, field: &str) {
    edit(s, |doc| {
        let n = doc.get_mut(id)?;
        if n.bound.remove(field).is_some() {
            crate::cmd::styles::rebind_paint_alpha(&mut n.fills, &mut n.strokes, field, false);
        }
        Ok(())
    })
    .unwrap();
}

/// Set `variable`'s (name or id) value in its collection's default (first) mode to the plain `value`.
pub(crate) fn set_default(s: &mut Session, variable: &str, value: Value) {
    let mode = {
        let doc = s.doc().unwrap();
        let var = doc.variables.get(variable).unwrap_or_else(|| panic!("no variable `{variable}`"));
        doc.variables.collection_by_id(&var.collection).and_then(|c| c.modes.first()).map(|m| m.id.clone()).unwrap()
    };
    set(s, variable, &mode, value);
}

/// Edit variable `variable` (name or id) with `f`, as a file can hold it (a name, a code syntax).
pub(crate) fn edit_variable(s: &mut Session, variable: &str, f: impl FnOnce(&mut figcraft_doc::Variable)) {
    edit(s, |doc| {
        let id = doc.variables.get(variable).unwrap_or_else(|| panic!("no variable `{variable}`")).id.clone();
        if let Some(v) = doc.variables.variables.iter_mut().find(|v| v.id == id) {
            f(v);
        }
        Ok(())
    })
    .unwrap();
}

/// Edit the active document with `f`, then normalise it as a command's edit is: bound layers take
/// their variables' values, text resizes, auto layout re-flows. Not an undo step.
pub(crate) fn edit<T>(s: &mut Session, f: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
    let ds = s.ds_mut()?;
    let before = ds.doc.clone();
    let out = f(ds.doc_mut())?;
    crate::derived::normalize(ds.doc_mut(), &before);
    Ok(out)
}

/// A new collection `name` with `modes` (in order; the first is the default) → its id.
pub(crate) fn add_collection(doc: &mut Document, name: &str, modes: &[&str]) -> CollectionId {
    let id = doc.new_collection_id();
    let modes = modes.iter().enumerate().map(|(i, m)| Mode { id: format!("{id}/mode{}", i + 1), name: (*m).to_string() }).collect();
    doc.variables.collections.push(Collection { id: id.clone(), name: name.to_string(), modes, extends: None });
    id
}

/// A new variable `name` of `kind` in collection `collection` (an id) with `values` by mode id;
/// a mode without one takes the type's default → its id.
pub(crate) fn add_variable(
    doc: &mut Document,
    collection: &str,
    name: &str,
    kind: VariableType,
    mut values: BTreeMap<ModeId, VariableValue>,
) -> VariableId {
    let modes: Vec<ModeId> = doc.variables.collection_by_id(collection).map(|c| c.modes.iter().map(|m| m.id.clone()).collect()).unwrap_or_default();
    for m in modes {
        values.entry(m).or_insert_with(|| VariableValue::default_for(kind));
    }
    let id = doc.new_variable_id();
    doc.variables.variables.push(Variable {
        id: id.clone(),
        name: name.to_string(),
        collection: collection.to_string(),
        kind,
        values,
        description: String::new(),
        scopes: None,
        hidden_from_publishing: false,
        extends: None,
        overrides: BTreeSet::new(),
        code_syntax: Default::default(),
    });
    id
}

/// Bind `field` of layer `id` to `variable` (an id). A colour variable bound to a solid paint takes
/// over the paint's colour alpha (see `rebind_paint_alpha`); normalising writes the value.
fn bind_field(doc: &mut Document, id: NodeId, field: &str, variable: &str) -> Result<()> {
    let n = doc.get_mut(id)?;
    if n.bound.get(field).map(String::as_str) != Some(variable) {
        crate::cmd::styles::rebind_paint_alpha(&mut n.fills, &mut n.strokes, field, true);
    }
    n.bound.insert(field.to_string(), variable.to_string());
    Ok(())
}

/// Show layer `id` (a frame or page) and what it holds in `mode` of `collection` (ids).
pub(crate) fn set_mode(doc: &mut Document, id: NodeId, collection: &str, mode: &str) -> Result<()> {
    doc.get_mut(id)?.var_modes.insert(collection.to_string(), mode.to_string());
    Ok(())
}

/// The variable type a test names: color, number, string or boolean.
pub(crate) fn kind_of(name: &str) -> Option<VariableType> {
    match name {
        "color" => Some(VariableType::Color),
        "number" => Some(VariableType::Number),
        "string" => Some(VariableType::String),
        "boolean" => Some(VariableType::Boolean),
        _ => None,
    }
}

/// A plain value of `kind` from JSON: a colour as "#RRGGBB[AA]", a number, a string (anything
/// else as its JSON text) or a boolean; None when it isn't one.
pub(crate) fn value_of(kind: VariableType, v: &serde_json::Value) -> Option<VariableValue> {
    Some(match kind {
        VariableType::Color => VariableValue::Color(crate::params::color_of(v)?),
        VariableType::Number => VariableValue::Number(crate::params::f64_of(v)?),
        VariableType::String => VariableValue::String(v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())),
        VariableType::Boolean => VariableValue::Boolean(v.as_bool()?),
    })
}
