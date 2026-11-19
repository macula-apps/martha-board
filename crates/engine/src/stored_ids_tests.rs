//! A variable renamed to another one's id never takes its place (#1613), beside #1618's tests: a
//! binding and an expression read the stored id, not a variable named like it.

use figcraft_doc::VariableValue;
use figcraft_doc::proto::Expr;
use serde_json::json;

use crate::Session;
use crate::test_helpers::ex;
use crate::test_variables as tv;

/// The issue's document: Theme with `shadow`, `surface` and `alias` (→ surface), then `shadow`
/// renamed to surface's id with Web code syntax `--shadow`, as a file can hold it. Ids of shadow,
/// surface and alias.
fn renamed_like_an_id() -> (Session, String, String, String) {
    let mut s = Session::new();
    tv::collection(&mut s, "Theme", &["Mode 1"]);
    let shadow = tv::variable(&mut s, "Theme", "shadow", "color", &[json!("#FF0000")]);
    let surface = tv::variable(&mut s, "Theme", "surface", "color", &[json!("#0D99FF")]);
    let alias = tv::variable(&mut s, "Theme", "alias", "color", &[json!("#000000")]);
    tv::set_all(&mut s, &alias, VariableValue::Alias(surface.clone()));
    tv::edit_variable(&mut s, &shadow, |v| {
        v.name.clone_from(&surface);
        v.code_syntax.insert("WEB".into(), "--shadow".into());
    });
    (s, shadow, surface, alias)
}

#[test]
fn a_fill_binding_names_the_stored_variable() {
    let (mut s, _, surface, _) = renamed_like_an_id();
    let r = ex(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10}));
    let r = crate::params::id_of(&r["id"]).unwrap();
    tv::bind(&mut s, &[r], "fill", &surface);
    let doc = s.doc().unwrap();
    let n = doc.node(r).unwrap();
    let key = crate::cmd::lists::paint_binding_key(n, true, 0).unwrap();
    assert_eq!(doc.variables.get_by_id(&n.bound[key]).unwrap().name, "surface");
}

#[test]
fn expressions_read_and_show_the_stored_variable() {
    let (mut s, _, surface, _) = renamed_like_an_id();
    crate::variable_data::edit(&mut s, |doc| {
        let c = doc.variables.collections.iter_mut().find(|c| c.name == "Theme").unwrap();
        c.modes.push(figcraft_doc::Mode { id: format!("{}/mode2", c.id), name: "Dark".into() });
        Ok(())
    })
    .unwrap();
    tv::set(&mut s, &surface, "Dark", json!("#00FF00"));
    let vars = &s.doc().unwrap().variables;
    let theme = vars.collection("Theme").unwrap().clone();
    let dark = theme.find_mode("Dark").unwrap().id.clone();
    let e = Expr::InMode { variable: surface.clone(), mode: dark };
    assert!(matches!(e.eval(vars, &Default::default()), Some(VariableValue::Color(c)) if c.to_hex() == "00FF00"));
    assert_eq!(e.display(vars), "surface[Dark]");
    assert_eq!(Expr::var(surface).display(vars), "surface");
}
