//! What only FigCraft's server served left the board (martha-board#41): publishing a file to a
//! server, your online files, a server's hosting and its admin, and reports to its operator. None
//! of their commands is registered, found, run, listed in a menu (and so in the Actions palette)
//! or bound to a key. Live sessions (host, join, leave, stop, the people in them) and comparing
//! with the saved file stay.

use serde_json::json;

use crate::cmd::find_command;
use crate::{EngineError, Session};

/// The commands that only talked to a figcraft-server.
const REMOVED: [&str; 8] = [
    "collab.publish",
    "collab.files",
    "collab.openFile",
    "collab.forgetFile",
    "collab.manageFile",
    "collab.recoverFile",
    "collab.serverInfo",
    "collab.report",
];

/// Commands of live sessions that stay.
const KEPT: [&str; 7] = ["collab.host", "collab.join", "collab.leave", "collab.stop", "collab.peers", "collab.diff", "collab.share"];

#[test]
fn no_server_only_command_is_registered_listed_or_bound() {
    let ids: Vec<&str> = crate::all_commands().iter().map(|c| c.id).collect();
    // The list is read: commands that stay are in it.
    for kept in KEPT {
        assert!(ids.contains(&kept), "{kept} is registered");
        assert!(find_command(kept).is_some(), "{kept} is found");
    }
    let mut s = Session::new();
    for id in REMOVED {
        assert!(!ids.contains(&id), "{id} is registered");
        assert!(find_command(id).is_none(), "{id} is found");
        assert!(matches!(s.execute(id, &json!({})), Err(EngineError::UnknownCommand(_))), "{id} runs");
    }
    // Neither the menus (and the Actions palette, which lists them) nor the keymap name one.
    assert!(!crate::catalog::MENU.is_empty() && !crate::keymap::KEYS.is_empty());
    let menus: Vec<&str> = crate::catalog::MENU.iter().map(|m| m.id).filter(|id| REMOVED.contains(id)).collect();
    assert!(menus.is_empty(), "in the menus: {menus:?}");
    let keys: Vec<&str> = crate::keymap::KEYS.iter().map(|b| b.id).filter(|id| REMOVED.contains(id)).collect();
    assert!(keys.is_empty(), "in the keymap: {keys:?}");
    assert!(crate::catalog::MENU.iter().any(|m| m.id == "collab.join"), "the menus are read");
    assert!(crate::keymap::KEYS.iter().any(|b| b.id == "collab.cursorChat"), "the keymap is read");
}
