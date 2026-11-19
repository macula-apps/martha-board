//! FigCraft's starter templates left the board (#33): neither the command that listed them nor
//! the one that opened one is registered, found, run, listed in a menu (and so in the Actions
//! palette) or bound to a key. The board templates stay.

use serde_json::json;

use crate::cmd::find_command;
use crate::{EngineError, Session};

/// The starter templates' commands.
const REMOVED: [&str; 2] = ["file.newFromTemplate", "template.list"];

#[test]
fn no_starter_template_command_is_registered_listed_or_bound() {
    let ids: Vec<&str> = crate::all_commands().iter().map(|c| c.id).collect();
    // The list is read: commands that stay are in it.
    for kept in ["file.new", "file.newBoard", "board.template", "board.templates"] {
        assert!(ids.contains(&kept), "{kept} is registered");
        assert!(find_command(kept).is_some(), "{kept} is found");
    }
    let mut s = Session::new();
    for id in REMOVED {
        assert!(!ids.contains(&id), "{id} is registered");
        assert!(find_command(id).is_none(), "{id} is found");
        assert!(matches!(s.execute(id, &json!({})), Err(EngineError::UnknownCommand(_))), "{id} runs");
    }
    assert!(!ids.iter().any(|id| id.starts_with("template.")), "a `template.` command is registered");
    // Neither the menus (and the Actions palette, which lists them) nor the keymap name one.
    assert!(!crate::catalog::MENU.is_empty() && !crate::keymap::KEYS.is_empty());
    let menus: Vec<&str> = crate::catalog::MENU.iter().map(|m| m.id).filter(|id| REMOVED.contains(id)).collect();
    assert!(menus.is_empty(), "in the menus: {menus:?}");
    let keys: Vec<&str> = crate::keymap::KEYS.iter().map(|b| b.id).filter(|id| REMOVED.contains(id)).collect();
    assert!(keys.is_empty(), "in the keymap: {keys:?}");
    assert!(crate::catalog::MENU.iter().any(|m| m.id == "file.new"), "the menus are read");
}
