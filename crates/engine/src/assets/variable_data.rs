//! Variable data a document keeps after the variables editor and the `variable.*` commands left
//! the app (#16): files still carry collections, modes, values and bindings, draw their bound
//! values and save them. Normalising keeps extended collections in step with what they extend
//! ([`sync_extensions`]); what builds documents in code (the starter templates, tests) writes the
//! data straight into the document ([`edit`], [`add_collection`], [`add_variable`], [`bind`],
//! [`set_mode`]).

use std::collections::{BTreeMap, BTreeSet};

use figcraft_doc::{Collection, CollectionId, Document, Mode, ModeId, NodeId, Variable, VariableId, VariableType, VariableValue, Variables};

use crate::{Result, Session};

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
pub(crate) fn bind(doc: &mut Document, id: NodeId, field: &str, variable: &str) -> Result<()> {
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

/// Keep extended collections in step with the collections they extend (after updating library
/// copies, or when a file holding them is edited): the same modes, one inheriting variable per
/// variable of the parent (named like it, with its values in every mode not overridden) and values
/// in every mode for their own variables. When a parent collection or variable is gone, what
/// extended it becomes a plain collection or variable, keeping its values (so does one in a loop
/// of extensions).
pub fn sync_extensions(doc: &mut Document) {
    // Parents first: an extension of an extended collection follows what its parent just took.
    let mut exts: Vec<(Option<usize>, CollectionId)> =
        doc.variables.collections.iter().filter(|c| c.extends.is_some()).map(|c| (extension_depth(&doc.variables, c), c.id.clone())).collect();
    exts.sort_by_key(|(depth, _)| depth.unwrap_or(usize::MAX));
    for (depth, cid) in exts {
        let parent =
            depth.and_then(|_| doc.variables.collection_by_id(&cid)?.extends.as_ref()).and_then(|p| doc.variables.collection_by_id(p)).cloned();
        let vars = &mut doc.variables.variables;
        let Some(parent) = parent else {
            for v in vars.iter_mut().filter(|v| v.collection == cid) {
                v.extends = None;
                v.overrides.clear();
            }
            if let Some(c) = doc.variables.collections.iter_mut().find(|c| c.id == cid) {
                c.extends = None;
            }
            continue;
        };
        let parents: Vec<Variable> = vars.iter().filter(|v| v.collection == parent.id).cloned().collect();
        let modes: Vec<ModeId> = parent.modes.iter().map(|m| m.id.clone()).collect();
        for v in vars.iter_mut().filter(|v| v.collection == cid) {
            match v.extends.as_ref().map(|e| parents.iter().find(|p| p.id == *e)) {
                Some(Some(p)) => {
                    if v.kind != p.kind {
                        v.kind = p.kind;
                        v.overrides.clear();
                    }
                    v.name.clone_from(&p.name);
                    v.overrides.retain(|m| modes.contains(m));
                    let own = std::mem::take(&mut v.values);
                    v.values =
                        p.values.iter().map(|(m, x)| (m.clone(), own.get(m).filter(|_| v.overrides.contains(m)).unwrap_or(x).clone())).collect();
                }
                Some(None) => {
                    v.extends = None;
                    v.overrides.clear();
                }
                None => {}
            }
            // Its own variables (and orphans) keep a value in every mode: new modes start with the
            // default mode's.
            if v.extends.is_none() {
                let first = modes.iter().find_map(|m| v.values.get(m)).or_else(|| v.values.values().next()).cloned();
                v.values.retain(|m, _| modes.contains(m));
                if let Some(f) = first {
                    for m in &modes {
                        v.values.entry(m.clone()).or_insert_with(|| f.clone());
                    }
                }
            }
        }
        let missing: Vec<Variable> =
            parents.into_iter().filter(|p| !vars.iter().any(|v| v.collection == cid && v.extends.as_ref() == Some(&p.id))).collect();
        if let Some(c) = doc.variables.collections.iter_mut().find(|c| c.id == cid)
            && c.modes != parent.modes
        {
            c.modes = parent.modes.clone();
        }
        for p in missing {
            let id = doc.new_variable_id();
            let extends = Some(p.id.clone());
            // The parent's own overrides (it may be an extension too) aren't this one's.
            doc.variables.variables.push(Variable {
                id,
                collection: cid.clone(),
                extends,
                hidden_from_publishing: false,
                overrides: BTreeSet::new(),
                ..p
            });
        }
    }
}

/// How many extensions deep `c` is (1 for one extending a plain collection; a gone parent ends the
/// chain); None in a loop of extensions.
fn extension_depth(vars: &Variables, c: &Collection) -> Option<usize> {
    let mut at = c;
    for depth in 0..=vars.collections.len() {
        let Some(next) = at.extends.as_ref().and_then(|p| vars.collection_by_id(p)) else { return Some(depth) };
        at = next;
    }
    None
}

/// The variable type a template or test names: color, number, string or boolean.
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
