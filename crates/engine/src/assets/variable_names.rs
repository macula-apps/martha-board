//! The names a file's variables take when exported (`variable.export`): CSS custom properties,
//! unique in the file, and Tailwind CSS v4 theme variables (`--color-*`, `--radius-*`…). Also the
//! colour literal the Dart export writes.

use std::collections::{HashMap, HashSet};

use figcraft_color::Color;
use figcraft_doc::{Document, Variable, VariableId, VariableType};

use crate::token_roles::{NumberRole, Roles};

/// A variable's CSS custom property name: "Colors/Brand 500" → `--colors-brand-500`.
pub fn css_var_name(variable: &str) -> String {
    let mut out = String::from("--");
    for c in variable.chars().flat_map(char::to_lowercase).take(80) {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.len() <= 2 { "--variable".into() } else { trimmed.to_string() }
}

/// A variable's CSS custom property: its Web code syntax (`var(--x)` or `--x`) when it has one,
/// else made from its name.
pub fn css_var_of(v: &Variable) -> String {
    let Some(web) = v.code_syntax.get("WEB").map(|w| w.trim()) else { return css_var_name(&v.name) };
    let inner = web.strip_prefix("var(").map_or(web, |r| r.split([',', ')']).next().unwrap_or("").trim());
    let valid = inner.len() > 2 && inner.starts_with("--") && inner.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if valid { inner.chars().take(80).collect() } else { css_var_name(inner) }
}

/// The file's variables' CSS custom properties, unique in the file: [`css_var_of`], and where an
/// earlier variable already has that name, the name with a numeric suffix (`--brand-primary-2`).
pub struct CssNames {
    names: HashMap<VariableId, String>,
}

impl CssNames {
    pub fn new(doc: &Document) -> Self {
        let vars = &doc.variables.variables;
        let mut names = HashMap::with_capacity(vars.len());
        // Every plain name is reserved first, so a suffixed name never takes another's plain one;
        // the next suffix per base keeps this linear.
        let plain: Vec<String> = vars.iter().map(css_var_of).collect();
        let mut taken: HashSet<String> = HashSet::with_capacity(vars.len());
        let all: HashSet<&str> = plain.iter().map(String::as_str).collect();
        let mut next: HashMap<&str, usize> = HashMap::new();
        for (v, base) in vars.iter().zip(&plain) {
            let mut name = base.clone();
            if !taken.insert(name.clone()) {
                let k = next.entry(base.as_str()).or_insert(1);
                loop {
                    *k += 1;
                    name = format!("{base}-{k}");
                    if !all.contains(name.as_str()) && taken.insert(name.clone()) {
                        break;
                    }
                }
            }
            names.insert(v.id.clone(), name);
        }
        CssNames { names }
    }

    /// The custom property for `v` (its [`css_var_of`] name for a variable not in the file).
    pub fn name(&self, v: &Variable) -> String {
        self.names.get(&v.id).cloned().unwrap_or_else(|| css_var_of(v))
    }
}

/// The roles and Tailwind theme names of a file's variables, computed once per export so they
/// are unique in the file.
#[derive(Default)]
pub struct Theme {
    roles: Roles,
    names: HashMap<VariableId, String>,
}

impl Theme {
    pub fn new(doc: &Document) -> Self {
        let vars = &doc.variables.variables;
        let roles = Roles::new(doc);
        let mut t = Theme { roles, names: HashMap::new() };
        // Short names ("accent") where they are unique, the full path ("palette-accent") where two
        // variables would share one, and a numeric suffix as a last resort.
        let short: Vec<String> = vars.iter().map(|v| t.candidate(v, false)).collect();
        // Linear in the number of variables: counts for clashes, a set of names taken and the next
        // suffix per base, so a file with very many variables can't make this quadratic.
        let mut count: HashMap<&str, usize> = HashMap::new();
        for n in &short {
            *count.entry(n.as_str()).or_default() += 1;
        }
        let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut next: HashMap<String, usize> = HashMap::new();
        for (v, s) in vars.iter().zip(&short) {
            let base = if count.get(s.as_str()).copied().unwrap_or(0) > 1 { t.candidate(v, true) } else { s.clone() };
            let mut name = base.clone();
            if taken.contains(&name) {
                let k = next.entry(base.clone()).or_insert(1);
                loop {
                    *k += 1;
                    name = format!("{base}-{k}");
                    if !taken.contains(&name) {
                        break;
                    }
                }
            }
            taken.insert(name.clone());
            t.names.insert(v.id.clone(), name);
        }
        t
    }

    /// What a number variable measures; a number nothing tells about is a length.
    pub fn role(&self, v: &Variable) -> NumberRole {
        self.roles.role(v)
    }

    /// The theme variable for `v`, unique in the file (see [`Theme::new`]).
    pub fn name(&self, v: &Variable) -> String {
        self.names.get(&v.id).cloned().unwrap_or_else(|| self.candidate(v, false))
    }

    /// `--color-*` for colours, `--radius-*`, `--text-*`, `--spacing-*` for numbers by role,
    /// `--font-*` for font families, others their CSS name; from the last path segment, or the
    /// whole path (`full`). A name already in the namespace isn't prefixed twice.
    fn candidate(&self, v: &Variable, full: bool) -> String {
        let ns = match v.kind {
            VariableType::Color => Some("color"),
            VariableType::Number => match self.role(v) {
                NumberRole::Radius => Some("radius"),
                NumberRole::FontSize => Some("text"),
                NumberRole::Spacing => Some("spacing"),
                NumberRole::Length | NumberRole::Unitless => None,
            },
            VariableType::String if v.scopes.as_ref().is_some_and(|s| s.iter().any(|s| s == "FONT_FAMILY")) => Some("font"),
            _ => None,
        };
        let Some(ns) = ns else {
            return if full { css_var_name(&v.name) } else { css_var_of(v) };
        };
        let source = if full { v.name.as_str() } else { v.name.rsplit('/').next().unwrap_or(&v.name) };
        let slug = css_var_name(source);
        let slug = slug.trim_start_matches('-');
        let slug = [format!("{ns}s-"), format!("{ns}-")].iter().find_map(|p| slug.strip_prefix(p.as_str())).unwrap_or(slug);
        // A variable named after its namespace ("radius", "colors") is the namespace's base value.
        let slug = if slug == ns || slug == format!("{ns}s") { "base" } else { slug };
        format!("--{ns}-{slug}")
    }
}

/// `Color(0xAARRGGBB)`, as Flutter writes a colour.
pub(crate) fn kotlin_color(c: Color) -> String {
    let [r, g, b, a] = c.to_rgba8();
    format!("Color(0x{a:02X}{r:02X}{g:02X}{b:02X})")
}
