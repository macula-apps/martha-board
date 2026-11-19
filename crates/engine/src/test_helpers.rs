//! Helpers the engine's tests share: run a command, read an id, look at a layer.

use figcraft_doc::{Node, NodeId};
use serde_json::Value;

use crate::Session;
use crate::params::id_of;

/// Run `cmd` with `p`, panicking with the command and its params when it fails.
pub(crate) fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    match s.execute(cmd, &p) {
        Ok(v) => v,
        Err(e) => panic!("{cmd} {p}: {e}"),
    }
}

/// The `id` a command answered with.
pub(crate) fn idv(v: &Value) -> NodeId {
    id_of(&v["id"]).unwrap()
}

/// Layer `id` of the active document, as it is now.
pub(crate) fn node(s: &Session, id: NodeId) -> Node {
    s.doc().unwrap().node(id).unwrap().clone()
}

/// The characters of text layer `id` (empty for other layers).
pub(crate) fn chars(s: &Session, id: NodeId) -> String {
    node(s, id).kind.text().map(|t| t.characters.clone()).unwrap_or_default()
}
