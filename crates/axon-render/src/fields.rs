//! Field reference extraction from Mustache templates.
//!
//! Parses `{{field}}`, `{{{field}}}`, `{{#field}}`, `{{^field}}`, and
//! `{{nested.field}}` expressions.  Closing tags (`{{/field}}`), comments
//! (`{{! ... }}`), and partials (`{{> name}}`) are ignored — they carry no
//! field-reference semantics for validation purposes.

use std::collections::HashSet;

use regex::Regex;

/// Tags that carry no field reference (closer, comment, partial).
fn is_skipped_modifier(modifier: &str) -> bool {
    matches!(modifier, "/" | "!" | ">")
}

/// Extract every unique field path referenced in `template`.
///
/// The returned set contains dot-paths exactly as written, e.g.
/// `"amount.value"`.  Section openers (`#`, `^`) are included because they
/// reference a field even though they also control rendering flow.
///
/// # Examples
///
/// ```
/// # use axon_render::fields::extract_field_refs;
/// let refs = extract_field_refs("Hello {{name}}, total: {{amount.value}}");
/// assert!(refs.contains("name"));
/// assert!(refs.contains("amount.value"));
/// ```
pub fn extract_field_refs(template: &str) -> HashSet<String> {
    // Match triple-mustache first (unescaped), then double-mustache tags.
    // Pattern breakdown:
    //   \{\{\{([^}]+?)\}\}\}  — {{{field}}} unescaped
    //   \{\{([#^/!>]?)\s*([^}]+?)\s*\}\}  — {{[modifier]field}}
    let re = Regex::new(r"\{\{\{([^}]+?)\}\}\}|\{\{([#^/!>]?)\s*([^}]+?)\s*\}\}")
        .expect("compile-time regex");

    let mut refs = HashSet::new();

    for cap in re.captures_iter(template) {
        if let Some(triple) = cap.get(1) {
            // {{{field}}} — unescaped variable
            let field = triple.as_str().trim().to_string();
            if !field.is_empty() {
                refs.insert(field);
            }
        } else if let (Some(modifier), Some(name)) = (cap.get(2), cap.get(3)) {
            if !is_skipped_modifier(modifier.as_str()) {
                let field = name.as_str().trim().to_string();
                if !field.is_empty() {
                    refs.insert(field);
                }
            }
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_field() {
        let refs = extract_field_refs("Hello {{name}}!");
        assert!(refs.contains("name"));
    }

    #[test]
    fn unescaped_field() {
        let refs = extract_field_refs("{{{body}}}");
        assert!(refs.contains("body"));
    }

    #[test]
    fn nested_field() {
        let refs = extract_field_refs("{{amount.value}} {{amount.currency}}");
        assert!(refs.contains("amount.value"));
        assert!(refs.contains("amount.currency"));
    }

    #[test]
    fn section_open() {
        let refs = extract_field_refs("{{#items}}x{{/items}}");
        assert!(refs.contains("items"), "section open should be captured");
        assert!(!refs.contains("/items"), "section close should be ignored");
    }

    #[test]
    fn inverted_section() {
        let refs = extract_field_refs("{{^empty}}non-empty{{/empty}}");
        assert!(refs.contains("empty"));
    }

    #[test]
    fn comment_ignored() {
        let refs = extract_field_refs("{{! this is a comment }}{{name}}");
        assert!(!refs.iter().any(|r| r.contains("comment")));
        assert!(refs.contains("name"));
    }

    #[test]
    fn partial_ignored() {
        let refs = extract_field_refs("{{> header}}{{name}}");
        assert!(!refs.iter().any(|r| r.contains("header")));
        assert!(refs.contains("name"));
    }

    #[test]
    fn whitespace_trimmed() {
        let refs = extract_field_refs("{{ name }}");
        assert!(refs.contains("name"));
    }

    #[test]
    fn no_refs_static_template() {
        let refs = extract_field_refs("# Static Heading\n\nNo fields here.");
        assert!(refs.is_empty());
    }

    #[test]
    fn mixed_template() {
        let refs = extract_field_refs(
            "# Invoice {{invoice_number}}\n\
             {{#line_items}}- {{description}}{{/line_items}}\n\
             {{{notes}}}",
        );
        assert!(refs.contains("invoice_number"));
        assert!(refs.contains("line_items"));
        assert!(refs.contains("description"));
        assert!(refs.contains("notes"));
    }
}
