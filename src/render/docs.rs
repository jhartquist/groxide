use std::fmt::Write;

use crate::types::{DisplayLimits, IndexItem};

use super::{feature_gate_suffix, strip_markdown, trim_trailing_newlines, truncate_doc};

/// Renders a recursive listing with full docs per item.
///
/// Each item gets its kind, name, signature, and full rendered documentation,
/// separated by blank lines. Items are grouped by parent module path.
pub(crate) fn render_docs_recursive(items: &[&IndexItem], root_path: &str) -> String {
    use std::collections::BTreeMap;

    if items.is_empty() {
        return String::new();
    }

    // Group items by parent module path
    let mut by_module: BTreeMap<&str, Vec<&IndexItem>> = BTreeMap::new();
    for &item in items {
        let parent = item
            .path
            .rsplit_once("::")
            .map_or(root_path, |(parent, _)| parent);
        by_module.entry(parent).or_default().push(item);
    }

    let limits = DisplayLimits::default();
    let mut out = String::new();
    let mut first_module = true;

    for (module_path, module_items) in &by_module {
        if !first_module {
            out.push('\n');
        }
        first_module = false;
        let _ = writeln!(out, "{module_path}:");
        let _ = writeln!(out);

        for (i, item) in module_items.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            render_item_with_docs(&mut out, item, &limits);
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders a single item with full docs for the recursive docs view.
///
/// Format:
/// ```text
/// {kind}  {path}
///     {signature}
///
///     {full docs}
/// ```
fn render_item_with_docs(out: &mut String, item: &IndexItem, limits: &DisplayLimits) {
    let kind = item.kind.short_name();
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "{kind}  {}{gate}", item.path);

    if !item.signature.is_empty() {
        let _ = writeln!(out, "    {}", item.signature);
    }

    if !item.docs.is_empty() {
        out.push('\n');
        let stripped = strip_markdown(&item.docs);
        let truncated = truncate_doc(&stripped, limits);
        for line in truncated.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "    {line}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item_full;
    use crate::types::ItemKind;

    #[test]
    fn render_docs_recursive_shows_full_docs() {
        let items = [
            make_item_full(
                "add",
                "mycrate::add",
                ItemKind::Function,
                "pub fn add(a: i32, b: i32) -> i32",
                "Adds two numbers together.\n\nReturns the sum of a and b.",
                "Adds two numbers together.",
            ),
            make_item_full(
                "Foo",
                "mycrate::Foo",
                ItemKind::Struct,
                "pub struct Foo",
                "A foo struct.\n\nUsed for testing.",
                "A foo struct.",
            ),
        ];
        let refs: Vec<&IndexItem> = items.iter().collect();
        let output = render_docs_recursive(&refs, "mycrate");

        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_docs_recursive_groups_by_module() {
        let items = [
            make_item_full(
                "bar",
                "mycrate::bar",
                ItemKind::Function,
                "pub fn bar()",
                "Does bar things.",
                "Does bar things.",
            ),
            make_item_full(
                "Baz",
                "mycrate::sub::Baz",
                ItemKind::Struct,
                "pub struct Baz",
                "A baz struct.",
                "A baz struct.",
            ),
        ];
        let refs: Vec<&IndexItem> = items.iter().collect();
        let output = render_docs_recursive(&refs, "mycrate");

        assert!(output.contains("mycrate:"), "root module header: {output}");
        assert!(
            output.contains("mycrate::sub:"),
            "sub module header: {output}"
        );
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_docs_recursive_empty_returns_empty() {
        let items: Vec<&IndexItem> = vec![];
        let output = render_docs_recursive(&items, "mycrate");
        assert!(output.is_empty());
    }
}
