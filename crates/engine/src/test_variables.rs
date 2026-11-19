//! Tests: variables, bindings and modes written into a session's document from literal values, as
//! a file carrying them holds them (the `variable.*` commands left the app, #16). Each call
//! normalises like an edit (bound layers take their values) but is no undo step.

use std::collections::BTreeMap;

use figcraft_doc::{NodeId, VariableValue};
use serde_json::Value;

use crate::{Session, variable_data};

/// A collection `name` with `modes` (the first is the default) → its id.
pub(crate) fn collection(s: &mut Session, name: &str, modes: &[&str]) -> String {
    variable_data::edit(s, |doc| Ok(variable_data::add_collection(doc, name, modes))).unwrap()
}

/// A variable `name` of `kind` (color, number, string or boolean) in `collection` (name or id), with
/// one value per mode in order (a single value: every mode) → its id.
pub(crate) fn variable(s: &mut Session, collection: &str, name: &str, kind: &str, values: &[Value]) -> String {
    let kind = variable_data::kind_of(kind).unwrap_or_else(|| panic!("no variable type `{kind}`"));
    variable_data::edit(s, |doc| {
        let coll = doc.variables.collection(collection).unwrap_or_else(|| panic!("no collection `{collection}`")).clone();
        let mut by_mode = BTreeMap::new();
        for (i, m) in coll.modes.iter().enumerate() {
            let v = if values.len() == 1 { &values[0] } else { &values[i] };
            by_mode.insert(m.id.clone(), variable_data::value_of(kind, v).unwrap_or_else(|| panic!("`{name}`: not a {} value: {v}", kind.id())));
        }
        Ok(variable_data::add_variable(doc, &coll.id, name, kind, by_mode))
    })
    .unwrap()
}

/// Set `variable`'s (name or id) value in `mode` (name or id) to the plain `value`, as a
/// collaborator's edit or a library update brings it.
pub(crate) fn set(s: &mut Session, variable: &str, mode: &str, value: Value) {
    variable_data::edit(s, |doc| {
        let var = doc.variables.get(variable).unwrap_or_else(|| panic!("no variable `{variable}`")).clone();
        let mid = doc.variables.collection_by_id(&var.collection).and_then(|c| c.find_mode(mode)).map(|m| m.id.clone());
        let mid = mid.unwrap_or_else(|| panic!("no mode `{mode}`"));
        let val = variable_data::value_of(var.kind, &value).unwrap_or_else(|| panic!("`{variable}`: not a {} value: {value}", var.kind.id()));
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
    variable_data::edit(s, |doc| {
        let vid = doc.variables.get(variable).map(|v| v.id.clone()).unwrap_or_else(|| panic!("no variable `{variable}`"));
        for id in ids {
            variable_data::bind(doc, *id, field, &vid)?;
        }
        Ok(())
    })
    .unwrap();
}

/// Show layer `id` (a frame or the page) in `mode` of `collection` (names or ids).
pub(crate) fn mode(s: &mut Session, id: NodeId, collection: &str, mode: &str) {
    variable_data::edit(s, |doc| {
        let coll = doc.variables.collection(collection).unwrap_or_else(|| panic!("no collection `{collection}`"));
        let mid = coll.find_mode(mode).map(|m| m.id.clone()).unwrap_or_else(|| panic!("no mode `{mode}`"));
        let cid = coll.id.clone();
        variable_data::set_mode(doc, id, &cid, &mid)
    })
    .unwrap();
}

/// Set every mode's value of `variable` (name or id) to `value` (any [`VariableValue`]: an alias
/// or a colour calculation too, reading variables by id).
pub(crate) fn set_all(s: &mut Session, variable: &str, value: VariableValue) {
    variable_data::edit(s, |doc| {
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
    variable_data::edit(s, |doc| {
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
    variable_data::edit(s, |doc| {
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
    variable_data::edit(s, |doc| {
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
    variable_data::edit(s, |doc| {
        let id = doc.variables.get(variable).unwrap_or_else(|| panic!("no variable `{variable}`")).id.clone();
        if let Some(v) = doc.variables.variables.iter_mut().find(|v| v.id == id) {
            f(v);
        }
        Ok(())
    })
    .unwrap();
}
