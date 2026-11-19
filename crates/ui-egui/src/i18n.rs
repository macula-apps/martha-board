//! The UI language: lookups over the string catalogs in `crates/ui-egui/locales/` (one TOML file
//! per language, baked in by build.rs; see docs/translating.md). The UI ships in English only
//! (#34): `en.toml` is the one catalog, and a preference or system language naming any other reads
//! English.
//!
//! Keys are the English text as written in the code: `tr("Fill")`,
//! `trf("Delete {name}", &[("name", &name)])`, `trn("{n} layers", count)`. A key a catalog lacks
//! shows its English text, so the UI never shows a raw key. Command labels are looked up by command
//! id ([`command`]): ids, the CLI, MCP and the control channel stay English.

use std::borrow::Cow;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One catalog entry. Two words: the plural forms sit behind a reference, so a baked-in row doesn't
/// carry two strings' worth of space.
pub enum Text {
    Plain(&'static str),
    Plural(&'static Forms),
}

/// Plural forms: `one` (optional) for a count of 1, `other` for every count and as the fallback.
pub struct Forms {
    pub one: Option<&'static str>,
    pub other: &'static str,
}

/// A language's catalog (generated from `locales/<code>.toml`).
pub struct Lang {
    pub code: &'static str,
    /// UI strings by English text, sorted.
    pub ui: &'static [(&'static str, Text)],
    /// Command labels by command id, sorted.
    pub commands: &'static [(&'static str, Text)],
    /// Main-menu labels by command id where the menu words it differently ("Fill" in Select
    /// matching for "Select same fill"), sorted.
    pub menu: &'static [(&'static str, Text)],
}

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

/// Index into [`LANGS`] of the language the UI shows (0: English).
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// The language the UI shows.
pub fn current() -> Option<&'static Lang> {
    LANGS.get(CURRENT.load(Ordering::Relaxed))
}

/// Whether the language the UI shows is written right to left, so the window mirrors.
pub fn right_to_left() -> bool {
    current().is_some_and(|l| writes_right_to_left(l.code))
}

/// Whether language `code` ("ar", "he-IL"…) is written right to left: Arabic, Hebrew, Persian,
/// Urdu, Pashto, Sindhi, Kurdish (Sorani), Uyghur, Dhivehi and Yiddish. It reads the code alone, so
/// the mirrored layout follows as soon as a right-to-left catalog is baked in.
fn writes_right_to_left(code: &str) -> bool {
    let base = code.split(['-', '_']).next().unwrap_or("");
    ["ar", "he", "fa", "ur", "ps", "sd", "ckb", "ug", "dv", "yi"].iter().any(|rtl| rtl.eq_ignore_ascii_case(base))
}

/// Show the language of `pref` (a language code, or "system" for `system`, the OS or browser
/// languages, most preferred first and comma-separated). Unknown languages show English.
pub fn set(pref: &str, system: Option<&str>) {
    CURRENT.store(resolve(pref, system), Ordering::Relaxed);
}

/// The [`LANGS`] index for a preference: the first language in it the UI has, else English.
fn resolve(pref: &str, system: Option<&str>) -> usize {
    let want = if pref.is_empty() || pref == "system" { system.unwrap_or("en") } else { pref };
    want.split(',').find_map(resolve_one).unwrap_or(0)
}

/// The [`LANGS`] index for one tag: the exact tag ("en-US"), else its base language ("en" for
/// "en_GB.UTF-8"); `None` when no catalog has it.
fn resolve_one(want: &str) -> Option<usize> {
    let tag = want.trim().split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let base = tag.split('-').next().unwrap_or("");
    let find = |t: &str| LANGS.iter().position(|l| l.code.eq_ignore_ascii_case(t));
    find(&tag).or_else(|| find(base))
}

/// Look `key` up in one of the sorted catalog tables.
fn find(table: &'static [(&'static str, Text)], key: &str) -> Option<&'static Text> {
    table.binary_search_by(|(k, _)| (*k).cmp(key)).ok().and_then(|i| table.get(i)).map(|(_, t)| t)
}

/// Language `lang`, then English.
fn chain(lang: usize) -> impl Iterator<Item = &'static Lang> {
    [lang, 0].into_iter().filter_map(|i| LANGS.get(i))
}

/// `key`'s entry in language `lang`, else in English.
fn lookup(lang: usize, key: &str) -> Option<&'static Text> {
    chain(lang).find_map(|l| find(l.ui, key))
}

/// The UI string for `key` (its English text) in the current language. One English text that needs
/// two translations gets a context after `|` ("Fill|sizing"); the English catalog maps it back.
pub fn tr(key: &str) -> &str {
    try_tr(key).unwrap_or(key)
}

/// [`tr`] for text that comes from data and may have no catalog entry (an importer's notes, say):
/// `None` when the catalog doesn't have it.
pub fn try_tr(key: &str) -> Option<&str> {
    match lookup(CURRENT.load(Ordering::Relaxed), key)? {
        Text::Plain(s) | Text::Plural(Forms { other: s, .. }) => Some(s),
    }
}

/// Mark a key for the catalog where the lookup happens later (`tr(label)` at display time), e.g. in
/// a `const` table.
pub const fn tr_noop(key: &str) -> &str {
    key
}

/// [`tr`] with `{name}` placeholders filled from `args`.
pub fn trf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(tr(key), args)
}

/// The plural form of `key` (its English "other" form) for count `n`, with `{n}` and `args` filled.
pub fn trn(key: &str, n: usize, args: &[(&str, &dyn Display)]) -> String {
    let lang = CURRENT.load(Ordering::Relaxed);
    let text = counted(lang, key, n);
    let mut all: Vec<(&str, &dyn Display)> = vec![("n", &n)];
    all.extend_from_slice(args);
    fill(text, &all)
}

/// The text for count `n`: the `one` form when English's rule takes it, else `other`.
fn counted(lang: usize, key: &str, n: usize) -> &str {
    chain(lang)
        .find_map(|l| {
            find(l.ui, key).map(|text| match text {
                Text::Plain(s) => *s,
                Text::Plural(Forms { one, other }) => one.filter(|_| takes_one(n)).unwrap_or(other),
            })
        })
        .unwrap_or(key)
}

/// English's plural rule (CLDR, integer counts): 1 takes the "one" form, every other count "other".
fn takes_one(n: usize) -> bool {
    n == 1
}

/// Replace each `{name}` in `text` with its argument, in one pass (an argument's own braces stay).
fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let (before, tail) = rest.split_at(open);
        out.push_str(before);
        let arg = tail.find('}').and_then(|close| {
            let name = tail.get(1..close)?;
            args.iter().find(|(k, _)| *k == name).map(|(_, v)| (close, v))
        });
        match arg {
            Some((close, v)) => {
                out.push_str(&v.to_string());
                rest = tail.get(close + 1..).unwrap_or("");
            }
            None => {
                out.push('{');
                rest = tail.get(1..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}

/// A command's label in the current language: the catalog's by command id, else `english` (the
/// engine's label).
pub fn command<'a>(id: &str, english: &'a str) -> Cow<'a, str> {
    label(false, id, english)
}

/// A main-menu item's label: the catalog's menu wording for the id, else the command's. An English
/// label ending in "…" (opens a dialog) keeps it.
pub fn menu_item<'a>(id: &str, english: &'a str) -> Cow<'a, str> {
    label(true, id, english)
}

/// A command's or menu item's label: the catalog wording from `chain`, keeping a dialog's trailing
/// ellipsis when the English label has one.
fn label<'a>(menu: bool, id: &str, english: &'a str) -> Cow<'a, str> {
    let text = chain(CURRENT.load(Ordering::Relaxed)).find_map(|l| menu.then(|| find(l.menu, id)).flatten().or_else(|| find(l.commands, id)));
    match text {
        Some(Text::Plain(s) | Text::Plural(Forms { other: s, .. })) if english.ends_with('…') && !s.ends_with('…') => Cow::Owned(format!("{s}…")),
        Some(Text::Plain(s) | Text::Plural(Forms { other: s, .. })) => Cow::Borrowed(s),
        None => Cow::Borrowed(english),
    }
}

#[cfg(test)]
mod tests;
