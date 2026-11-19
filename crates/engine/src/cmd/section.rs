//! Sections: hide a section's contents.
//! (Wrapping layers in a new section is `object.wrapInSection`, with the other wrappers.)

use figcraft_doc::{NodeId, NodeKind};
use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::{boolean, req_targets};
use crate::{Result, Session, bad, spec};

pub fn specs() -> Vec<CommandSpec> {
    vec![spec!(
        "section.toggleContents",
        "Hide/Show section contents",
        "{ids?, hidden? (default: hide unless all are hidden)} (a hidden section shows only its name and outline; its layers aren't drawn or hit)",
        true,
        toggle_contents
    )]
}

fn toggle_contents(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section.toggleContents";
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc()?;
    let sections: Vec<NodeId> = ids.iter().copied().filter(|i| doc.node(*i).is_some_and(|n| matches!(n.kind, NodeKind::Section { .. }))).collect();
    if sections.is_empty() {
        return Err(bad(cmd, "select a section"));
    }
    let hide = boolean(p, "hidden").unwrap_or_else(|| !sections.iter().all(|i| doc.node(*i).is_some_and(|n| n.hides_contents())));
    // Selected layers inside a section being hidden give way to the section.
    let sel: Vec<NodeId> = s.selection();
    let mut new_sel: Vec<NodeId> = Vec::new();
    for i in sel {
        let inside = if hide { sections.iter().copied().find(|sec| doc.is_ancestor(*sec, i)) } else { None };
        let keep = inside.unwrap_or(i);
        if !new_sel.contains(&keep) {
            new_sel.push(keep);
        }
    }
    let doc = s.doc_mut()?;
    for sec in &sections {
        if let NodeKind::Section { contents_hidden } = &mut doc.get_mut(*sec)?.kind {
            *contents_hidden = hide;
        }
    }
    s.set_selection(new_sel);
    Ok(json!({"hidden": hide, "ids": sections}))
}
