//! The catalog checks: English is the one catalog (#34), every key the code looks up is in it and it
//! has no keys the code doesn't use; a language without a catalog reads English.

use std::collections::BTreeSet;

use super::{Forms, LANGS, Lang, Text, counted, fill, lookup, resolve, takes_one, writes_right_to_left};

/// Keys looked up from data rather than written as `tr("…")` in the code: the main menu's
/// submenu names (the engine's menu catalog) and labels of engine and document enums.
fn dynamic_keys() -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = figcraft_engine::catalog::MENU.iter().flat_map(|m| m.menu.split('/')).map(str::to_string).collect();
    keys.extend(dynamic_labels().into_iter().map(str::to_string));
    keys
}

/// Labels from engine and document data the UI shows through `tr(x.label())` and isn't written as
/// a literal anywhere in the UI: node kinds (and the Design panel header's plurals), blend modes,
/// stroke caps and width profiles, effects, brushes, image filters, frame preset categories, compare
/// properties, comment filters and sorts, change kinds, the live status.
// shortcut: a hand-kept list; enumerate each enum's `ALL` here when a label goes missing in review.
const DATA_LABELS: &[&str] = &[
    "Added",
    "Align",
    "All comments",
    "Alpha",
    "Background blur",
    "Black & white",
    "Blend",
    "Bristle",
    "Changed",
    "Circle",
    "Color burn",
    "Color dodge",
    "Component",
    "Component set",
    "Component sets",
    "Confetti",
    "Content",
    "Cool",
    "Darken",
    "Desktop",
    "Difference",
    "Direction",
    "Dots",
    "Drop shadow",
    "Duo",
    "Ellipses",
    "Exclude",
    "Excludes",
    "Exclusion",
    "Eye",
    "Fade",
    "Fills",
    "Frames",
    "Glass",
    "Group",
    "Groups",
    "Hard light",
    "Hue",
    "Ink",
    "Inner shadow",
    "Instances",
    "Intersect",
    "Intersects",
    "Joining…",
    "Layer blur",
    "Leaves",
    "Lighten",
    "Line arrow",
    "Lines",
    "Luminance",
    "Luminosity",
    "Mono",
    "Multi",
    "Multiply",
    "Muted",
    "No status",
    "Noise",
    "Normal",
    "Only your threads",
    "Padding",
    "Paper",
    "Pass through",
    "Phone",
    "Pinch",
    "Plus darker",
    "Plus lighter",
    "Polygons",
    "Presentation",
    "Progressive",
    "Rectangles",
    "Removed",
    "Reversed triangle",
    "Ribbon",
    "Rounded",
    "Screen",
    "Sections",
    "Slices",
    "Social media",
    "Soft light",
    "Sort by date",
    "Sort by page",
    "Square",
    "Stars",
    "Strokes",
    "Style",
    "Subtract",
    "Subtracts",
    "Tablet",
    "Taper",
    "Taper both ends",
    "Text align",
    "Texts",
    "Texture",
    "Triangle arrow",
    "Uniform",
    "Union",
    "Unions",
    "Unread",
    "Vectors",
    "Vivid",
    "Warm",
    "Watch",
    "Wedge",
    "Weight",
];

/// Labels of engine enums the UI shows through `tr(x.label())`, read from the engine, plus [`DATA_LABELS`].
fn dynamic_labels() -> Vec<&'static str> {
    use figcraft_engine::keymap::{GESTURES, Group};
    let mut v: Vec<&'static str> = figcraft_engine::ToolId::ALL.iter().map(|t| t.label()).collect();
    // Board shapes and stamps (the board tools' flyouts and the contextual bar).
    v.extend(figcraft_doc::BoardShape::ALL.iter().map(|b| b.label()));
    v.extend(figcraft_doc::Stamp::ALL.iter().map(|b| b.label()));
    v.extend(figcraft_engine::board_templates::TEMPLATES.iter().map(|t| t.name));
    v.extend(Group::ALL.iter().map(|g| g.label()));
    v.extend(GESTURES.iter().flat_map(|(_, label, mac, other)| [*label, *mac, *other]));
    v.extend(DATA_LABELS);
    v.push(figcraft_engine::live::TOO_LARGE_TO_JOIN);
    v
}

/// The string literals passed to `tr`, `trf`, `trn` and `tr_noop` in this crate's sources.
fn used_keys() -> BTreeSet<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut keys = BTreeSet::new();
    for path in sources(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with("_tests.rs") || name == "i18n.rs" {
            continue;
        }
        keys.extend(literals(&std::fs::read_to_string(&path).unwrap(), LOOKUPS));
    }
    keys
}

/// Every `.rs` file under `dir`, in subdirectories too, leaving out test code: `tests.rs` and
/// anything under a `tests` directory.
fn sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" {
                out.extend(sources(&path));
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            out.push(path);
        }
    }
    out
}

/// The lookup functions whose first argument is a catalog key.
const LOOKUPS: &[&str] = &["tr(", "trf(", "trn(", "tr_noop("];

/// The first-argument string literals of the functions `fns` (each with its `(`) in `src`.
fn literals(src: &str, fns: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for f in fns {
        let mut from = 0;
        while let Some(i) = src.get(from..).and_then(|s| s.find(f)) {
            let at = from + i;
            from = at + f.len();
            let prev = src.get(..at).and_then(|s| s.chars().next_back());
            if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let rest = src.get(from..).unwrap().trim_start();
            if let Some(lit) = rest.strip_prefix('"').and_then(string_literal) {
                out.push(lit);
            }
        }
    }
    out
}

/// A Rust string literal's value (after its opening quote), up to the closing quote.
fn string_literal(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\n' => {
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
    None
}

fn forms(t: &Text) -> Vec<&'static str> {
    match t {
        Text::Plain(s) => vec![s],
        Text::Plural(Forms { one, other }) => one.iter().copied().chain([*other]).collect(),
    }
}

/// The build bakes the catalog in: a row is a key and a two-word entry, not an entry with room for
/// every plural form.
#[test]
fn a_catalog_row_is_four_words() {
    assert_eq!(std::mem::size_of::<Text>(), 2 * std::mem::size_of::<usize>());
    assert_eq!(std::mem::size_of::<(&str, Text)>(), 4 * std::mem::size_of::<usize>());
}

fn english() -> &'static Lang {
    LANGS.first().filter(|l| l.code == "en").expect("locales/en.toml is baked in first")
}

#[test]
fn every_key_in_the_code_is_in_the_english_catalog_and_no_other() {
    let used: BTreeSet<String> = used_keys().into_iter().chain(dynamic_keys()).collect();
    let en: BTreeSet<String> = english().ui.iter().map(|(k, _)| k.to_string()).collect();
    let missing: Vec<_> = used.difference(&en).collect();
    let unused: Vec<_> = en.difference(&used).collect();
    assert!(
        missing.is_empty(),
        "keys missing from locales/en.toml:\n{}",
        missing.iter().map(|k| format!("{k:?} = {k:?}")).collect::<Vec<_>>().join("\n")
    );
    assert!(unused.is_empty(), "keys in locales/en.toml the code doesn't use: {unused:?}");
}

#[test]
fn english_text_is_its_key_except_plurals() {
    for (k, t) in english().ui {
        if let Text::Plain(s) = t {
            // "Fill|sizing": the same English as "Fill", translated apart (a context after `|`).
            assert_eq!(k.split('|').next(), Some(*s), "an English entry differs from its key (the key is the English text)");
        }
    }
    assert!(english().commands.is_empty() && english().menu.is_empty(), "English command labels come from the engine");
}

/// The UI ships in English only (#34): a catalog put back in `locales/` fails here, so bringing a
/// language back is a decision, not a stray file.
#[test]
fn english_is_the_only_catalog() {
    assert_eq!(LANGS.iter().map(|l| l.code).collect::<Vec<_>>(), [english().code]);
}

#[test]
fn lookups_fall_back_to_english_and_fill_placeholders() {
    assert_eq!(fill("Delete {name}?", &[("name", &"Frame {1}")]), "Delete Frame {1}?");
    assert_eq!(fill("{a} and {b", &[("a", &1)]), "1 and {b");
    assert!(lookup(0, "no such key").is_none());
    // English entries read English, through any language index (a stale one too).
    let (k, t) = english().ui.first().unwrap();
    assert!(matches!(lookup(0, k), Some(x) if std::ptr::eq(x, t)));
    assert!(matches!(lookup(LANGS.len(), k), Some(x) if std::ptr::eq(x, t)));
}

#[test]
fn a_live_failure_the_ui_knows_is_read_back_for_translation() {
    use figcraft_engine::live::TOO_LARGE_TO_JOIN;
    let reason = TOO_LARGE_TO_JOIN.replace("{mb}", "1024");
    assert!(matches!(crate::live::failure_text(&reason), std::borrow::Cow::Owned(t) if t == reason));
    assert!(matches!(crate::live::failure_text("Couldn't reach the server"), std::borrow::Cow::Borrowed(_)));
}

/// A system language or a saved preference naming a language whose catalog left in #34 (or never
/// had one) reads English, the system's list and regional tags included; English tags read English.
#[test]
fn a_removed_or_unknown_language_reads_english() {
    let en = 0;
    assert_eq!(LANGS[en].code, english().code);
    for removed in ["nl_BE.UTF-8", "ar", "ar_EG.UTF-8", "ja", "ja-JP", "es-419", "es_MX.UTF-8", "zh-Hant", "pt-BR", "fr-CA", "ru", "xx-YY", "", " "] {
        assert_eq!(resolve(removed, None), en, "preference {removed:?}");
        assert_eq!(resolve("system", Some(removed)), en, "system {removed:?}");
        assert_eq!(resolve("", Some(removed)), en, "no preference, system {removed:?}");
    }
    assert_eq!(resolve("system", Some("ja-JP,ar,nl-BE")), en, "a system list of removed languages");
    assert_eq!(resolve("system", None), en);
    for tag in ["en", "EN", "en-US", "en_GB.UTF-8", "en-Latn-GB"] {
        assert_eq!(resolve(tag, None), en, "{tag}");
        assert_eq!(resolve("system", Some(tag)), en, "{tag}");
    }
}

/// A preferences file saved with a language whose catalog left in #34 still loads, keeps its other
/// preferences, and shows English; `pref.language` still takes such a code without an error.
#[test]
fn a_preferences_file_naming_a_removed_language_loads_and_reads_english() {
    for (removed, saved) in [
        ("ar", r#"{"language": "ar", "user_name": "Ada", "theme": "dark"}"#),
        ("ja", r#"{"language": "ja", "user_name": "Ada", "theme": "dark"}"#),
        ("es-419", r#"{"language": "es-419", "user_name": "Ada", "theme": "dark"}"#),
        ("system", r#"{"language": "system", "user_name": "Ada", "theme": "dark"}"#),
    ] {
        let prefs: figcraft_engine::Prefs = serde_json::from_str(saved).unwrap_or_else(|e| panic!("{saved}: {e}"));
        assert_eq!((prefs.language.as_str(), prefs.user_name.as_str(), prefs.theme.as_str()), (removed, "Ada", "dark"));
        assert_eq!(LANGS[resolve(&prefs.language, Some("nl_BE.UTF-8"))].code, english().code, "{removed}");
    }
    let mut s = figcraft_engine::Session::new();
    for removed in ["ar", "ja", "es-419", "nl"] {
        s.execute("pref.language", &serde_json::json!({"language": removed})).unwrap();
        assert_eq!(s.prefs.language, removed);
        assert_eq!(LANGS[resolve(&s.prefs.language, None)].code, english().code, "{removed}");
    }
}

/// Mirroring reads the language code alone, with no catalog behind it: the window mirrors for a
/// right-to-left language as soon as one is baked in, and English reads left to right.
#[test]
fn right_to_left_languages_mirror_the_window() {
    for code in ["ar", "ar-SA", "AR", "he", "he-IL", "fa", "ur", "ckb", "yi"] {
        assert!(writes_right_to_left(code), "{code}");
    }
    for code in ["en", "", "nl", "nl_BE", "ja", "es-419", "zh-Hans", "tr", "ru", "arn", "hey"] {
        assert!(!writes_right_to_left(code), "{code}");
    }
    assert!(!writes_right_to_left(english().code), "English reads left to right");
}

/// Release builds embed craft-fonts' Noto Sans Arabic (`theme::install_fonts`' built-in faces) for
/// Arabic people type: it draws every letter of the basic Arabic alphabet.
#[test]
fn the_built_in_arabic_face_draws_arabic_letters() {
    if !figcraft_text::CRAFT_FONTS.iter().any(|f| f.family == "Noto Sans Arabic" && f.style == "Regular") {
        eprintln!("built without craft-fonts' Noto Sans Arabic; skipping");
        return;
    }
    let Some(face) = figcraft_text::face_with_char('ع', &["Noto Sans Arabic"]) else {
        panic!("the supplied Noto Sans Arabic face must draw Arabic");
    };
    // Hamza to Ghain and Feh to Yeh: the letters Arabic text is written in (tatweel and the
    // Persian letters between them left out).
    let missing: String = ('\u{0621}'..='\u{063A}').chain('\u{0641}'..='\u{064A}').filter(|c| !face.has_char(*c)).collect();
    assert!(missing.is_empty(), "Arabic letters missing from the embedded face: {missing:?}");
}

/// Release builds embed craft-fonts' Noto Sans Georgian (`theme::install_fonts`' built-in faces),
/// since Inter and egui's fonts have no Georgian: it draws every letter of the modern (Mkhedruli)
/// alphabet people type.
#[test]
fn the_built_in_georgian_face_draws_georgian_letters() {
    let Some(face) = figcraft_text::find_face("Noto Sans Georgian", "Regular").filter(|f| f.family == "Noto Sans Georgian") else {
        eprintln!("built without craft-fonts' Noto Sans Georgian; skipping");
        return;
    };
    let missing: String = ('\u{10D0}'..='\u{10F0}').filter(|c| !face.has_char(*c)).collect();
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}

#[test]
fn the_key_scanner_reads_rust_literals() {
    let src = r#"x(tr("A \"b\"")); trf(
        "C {n}", &[]); attr("no"); tr_noop("D\
          E"); tr(label);"#;
    assert_eq!(literals(src, LOOKUPS), vec!["A \"b\"", "C {n}", "DE"]);
    assert_eq!(literals(r#"self.warn("a note"); s.warn(format!("{n}")); forewarn("no")"#, &["warn("]), vec!["a note"]);
}

/// English's plural rule: 1 takes the `one` form, 0 and every other count `other`, and a key the
/// catalog lacks reads as itself.
#[test]
fn english_counts_take_one_only_for_one() {
    assert!(takes_one(1));
    for n in [0, 2, 11, 21, 1_000_000] {
        assert!(!takes_one(n), "{n}");
    }
    let Some(Text::Plural(Forms { one: Some(one), other })) = lookup(0, "{n} layers") else {
        panic!("\"{{n}} layers\" has a one form in en.toml");
    };
    assert_eq!(counted(0, "{n} layers", 1), *one);
    for n in [0, 2, 21] {
        assert_eq!(counted(0, "{n} layers", n), *other, "{n}");
    }
    assert_eq!(counted(0, "unknown {n}", 21), "unknown {n}");
    assert!(english().ui.iter().flat_map(|(_, t)| forms(t)).all(|f| !f.is_empty()));
}

#[test]
fn the_language_preference_takes_language_codes() {
    let mut s = figcraft_engine::Session::new();
    assert_eq!(s.prefs.language, "system");
    s.execute("pref.language", &serde_json::json!({"language": "en-GB"})).unwrap();
    assert_eq!(s.prefs.language, "en-GB");
    for bad in ["", "../es", "es--ES", "abcdefghi"] {
        assert!(s.execute("pref.language", &serde_json::json!({"language": bad})).is_err(), "{bad:?}");
    }
}
