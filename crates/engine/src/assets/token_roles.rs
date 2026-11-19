//! What a file's number variables measure, so exported tokens carry the right unit: a radius,
//! font size, spacing or other length is in pixels, opacity and font weight are bare numbers.
//! A number's role comes from the layer fields bound to it (and a text style's weight), else its scopes, and an alias target
//! with no role of its own takes the role of the variables that read it (the target of an opacity
//! alias stays unitless). A number nothing tells about is a length: design tokens mostly are.

use std::collections::HashMap;

use figcraft_doc::{Document, Variable, VariableId, VariableType, VariableValue};

/// What a number variable measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberRole {
    Radius,
    FontSize,
    Spacing,
    /// Another length (stroke, line height, letter spacing, effects): pixels.
    Length,
    /// Opacity or font weight: no unit.
    Unitless,
}

fn role_of_field(field: &str) -> Option<NumberRole> {
    Some(match field {
        "cornerRadius" | "topLeftRadius" | "topRightRadius" | "bottomRightRadius" | "bottomLeftRadius" => NumberRole::Radius,
        "fontSize" => NumberRole::FontSize,
        "opacity" | "fontWeight" => NumberRole::Unitless,
        f if f == "gap" || f == "counterAxisSpacing" || f.starts_with("padding") || f == "width" || f == "height" => NumberRole::Spacing,
        f if f.starts_with("min") || f.starts_with("max") => NumberRole::Spacing,
        "strokeWeight" | "lineHeight" | "letterSpacing" | "paragraphSpacing" | "paragraphIndent" => NumberRole::Length,
        _ => return None,
    })
}

fn role_of_scope(scope: &str) -> Option<NumberRole> {
    Some(match scope {
        "CORNER_RADIUS" => NumberRole::Radius,
        "FONT_SIZE" => NumberRole::FontSize,
        "GAP" | "WIDTH_HEIGHT" => NumberRole::Spacing,
        "OPACITY" | "FONT_WEIGHT" => NumberRole::Unitless,
        "STROKE_FLOAT" | "EFFECT_FLOAT" | "LINE_HEIGHT" | "LETTER_SPACING" | "PARAGRAPH_SPACING" | "PARAGRAPH_INDENT" => NumberRole::Length,
        _ => return None,
    })
}

/// The roles of a file's number variables, computed once per export.
#[derive(Default)]
pub struct Roles(HashMap<VariableId, NumberRole>);

impl Roles {
    pub fn new(doc: &Document) -> Self {
        let vars = &doc.variables.variables;
        let mut roles: HashMap<VariableId, NumberRole> = HashMap::new();
        for n in doc.nodes() {
            for (field, id) in &n.bound {
                if let Some(r) = role_of_field(field) {
                    roles.entry(id.clone()).or_insert(r);
                }
            }
        }
        for st in &doc.styles {
            for (field, id) in &st.bound {
                if field == "fontWeight" {
                    roles.entry(id.clone()).or_insert(NumberRole::Unitless);
                }
            }
        }
        for v in vars.iter().filter(|v| v.kind == VariableType::Number) {
            if let Some(r) = v.scopes.as_ref().and_then(|s| s.iter().find_map(|s| role_of_scope(s))) {
                roles.entry(v.id.clone()).or_insert(r);
            }
        }
        // Until a pass adds nothing: each pass adds a role or stops, so this ends within one pass
        // per variable, however long a chain is.
        loop {
            let mut changed = false;
            for v in vars {
                let Some(r) = roles.get(&v.id).copied() else { continue };
                for value in v.values.values() {
                    if let VariableValue::Alias(target) = value
                        && !roles.contains_key(target)
                    {
                        roles.insert(target.clone(), r);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        Roles(roles)
    }

    /// What `v` measures; a number nothing tells about is a length.
    pub fn role(&self, v: &Variable) -> NumberRole {
        self.0.get(&v.id).copied().unwrap_or(NumberRole::Length)
    }

    /// A number variable's CSS value: pixels for lengths, bare for opacity and font weight.
    pub fn number_css(&self, v: &Variable, x: f64) -> String {
        if self.role(v) == NumberRole::Unitless { num(x) } else { format!("{}px", num(x)) }
    }
}

/// `v` without trailing zeros (2 decimals).
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let scaled = v * 100.0;
    // Past the x100 overflow every value is whole anyway, so keep it as is.
    let r = if scaled.is_finite() { scaled.round() / 100.0 } else { v };
    if (r - r.round()).abs() < 1e-9 {
        // `as i64` saturates past 2^63, so larger whole values are printed from the float.
        if r.abs() < 9e18 { format!("{}", r.round() as i64) } else { format!("{:.0}", r.round()) }
    } else {
        format!("{r}")
    }
}

#[cfg(test)]
mod tests {
    use super::num;

    #[test]
    fn num_keeps_finite_values_past_the_x100_overflow() {
        for v in [1e307, -1e307, f64::MAX] {
            let s = num(v);
            assert_eq!(s, format!("{v:.0}"));
            assert_eq!(s.parse::<f64>(), Ok(v));
        }
    }

    #[test]
    fn num_keeps_huge_whole_numbers() {
        assert_eq!(num(1e20), "100000000000000000000");
        assert_eq!(num(-1e20), "-100000000000000000000");
        assert_eq!(num(1e18), "1000000000000000000");
        assert_eq!(num(-0.0), "0");
    }
}
