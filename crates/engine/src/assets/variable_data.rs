//! Variable data a document keeps after the variables editor and the `variable.*` commands left
//! the app (#16): files still carry collections, modes, values and bindings, draw their bound
//! values and save them. Normalising keeps extended collections in step with what they extend
//! ([`sync_extensions`]); tests write the data straight into the document
//! ([`crate::test_variables`]).

use std::collections::BTreeSet;

use figcraft_doc::{Collection, CollectionId, Document, ModeId, Variable, Variables};

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
