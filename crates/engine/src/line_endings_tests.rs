//! Text set by a command stores every line ending as LF, like editor insertion does, so layout
//! sees two lines and no stray glyph for CRLF, bare CR, U+2028 or U+2029.

use serde_json::json;

use crate::Session;
use crate::params::id_of;
use crate::test_helpers::{chars, ex, idv};

const INPUTS: [&str; 5] = ["a\nb", "a\r\nb", "a\rb", "a\u{2028}b", "a\u{2029}b"];

#[test]
fn text_set_normalizes_line_endings() {
    let mut s = Session::new();
    let created = s.execute("text.create", &json!({"text": "", "x": 0, "y": 0})).unwrap();
    let id = id_of(&created["id"]).unwrap();
    for input in INPUTS {
        s.execute("text.set", &json!({"id": id, "characters": input})).unwrap();
        let t = s.doc().unwrap().node(id).unwrap().kind.text().unwrap().clone();
        assert_eq!(t.characters, "a\nb", "text.set {input:?}");
    }
}

#[test]
fn text_create_normalizes_line_endings() {
    let mut s = Session::new();
    for input in INPUTS {
        let created = s.execute("text.create", &json!({"text": input, "x": 0, "y": 0})).unwrap();
        let id = id_of(&created["id"]).unwrap();
        let t = s.doc().unwrap().node(id).unwrap().kind.text().unwrap().clone();
        assert_eq!(t.characters, "a\nb", "text.create {input:?}");
    }
}

#[test]
fn find_replace_normalizes_line_endings() {
    for input in INPUTS {
        let mut s = Session::new();
        let id = idv(&ex(&mut s, "text.create", json!({"text": "x", "x": 0, "y": 0})));
        ex(&mut s, "edit.findReplace", json!({"find": "x", "replace": input}));
        assert_eq!(chars(&s, id), "a\nb", "edit.findReplace {input:?}");
    }
}
