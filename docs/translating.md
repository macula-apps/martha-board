# UI strings

The interface ships in English only. Its strings live in one TOML catalog,
[`crates/ui-egui/locales/en.toml`](../crates/ui-egui/locales/en.toml), which the build bakes into
the binary (desktop and web); nothing is read at run time. Every UI string is written once, and a
change edits one catalog.

The language preference (`pref.language {language}` from the CLI, MCP and the control channel, or a
saved preferences file) still takes a language code or `system`, and a language without a catalog
reads English, so preferences saved with another language load and show English. Preferences has no
language picker: one language is no choice.

People still type names, notes and comments in any script: text shaping, bidi, the CJK, Bengali and
Thai font fallback (`crates/ui-egui/src/cjk.rs`) and the built-in Arabic, Hebrew, Georgian and Thai
faces stay. The right-to-left mirroring of the window stays too: it reads the language code alone
(`writes_right_to_left` in [`crates/ui-egui/src/i18n.rs`](../crates/ui-egui/src/i18n.rs)), so it
follows if a right-to-left catalog is ever baked in again.

## History

Until #34 the interface also shipped 25 other catalogs: Spanish (`es`, with the Latin American
overlay `es-419`), French, German, Dutch, Brazilian Portuguese, Japanese, Korean, Simplified and
Traditional Chinese, Czech, Indonesian, Finnish, Bengali, Italian, Vietnamese, Thai, Bosnian,
Polish, Georgian, Russian, Turkish, Swedish, Tamil and Arabic. Most were machine-drafted and waiting
for native review. Polish was reviewed and approved by Lucas (`lukasz-sklad`), Russian by VanTan;
Georgian, Turkish and Tamil were reviewed by native speakers; Arabic was contributed by
`hussain-hz` with OpenAI Codex. On 2026-11-19 the owner ruled "English only": the 25 catalogs and
the tests that named them left the repository in #34. They, and this document's earlier guide to
adding a language, remain in the git history.

## The catalog

A key is the English text as the code writes it, so the code stays readable: `tr("Fill")`,
`trf("Delete {name}?", &[("name", &name)])`, `trn("{n} layers", count, &[])`. Values equal their
keys except plurals, which give both forms: `"{n} layers" = { one = "{n} layer", other = "{n} layers" }`
(`one` for a count of 1, `other` for every other count). One English text that needs two meanings
gets a context after `|` (`"Fill|sizing"`), which the catalog maps back to the English text. Command
labels come from the engine; command ids, the CLI, MCP and control-channel output are English.

`cargo test -p figcraft-ui-egui i18n` fails on keys the code uses that `en.toml` lacks (it prints
the lines to paste), keys no code uses, and a catalog other than English in `locales/`.

## Adding strings in code

Wrap every string a user sees or a screen reader hears: `tr` for plain text, `trf` for `{named}`
placeholders, `trn` for counts, `tr_noop` for literals in `const` tables that are looked up later
with `tr(label)`. The key must be a string literal as the first argument (the check reads the
source). Don't wrap ids, command ids, JSON, log messages, file extensions or document content. Then
add the key to `en.toml`.

Labels from the engine and document crates (tool names, blend modes, easing curves…) are looked up
at display with `tr(x.label())`; they are listed in `dynamic_labels` in
`crates/ui-egui/src/i18n/tests.rs`.
