use std::fmt::Write;

use crate::types::IndexItem;

use super::{feature_gate_suffix, trim_trailing_newlines};

/// Renders a recursive listing of items grouped by parent module.
///
/// Items are grouped by their parent module path, with each module shown as a
/// section header. Within each module, items are rendered with
/// kind/path/signature/summary columns.
pub(crate) fn render_list_recursive(items: &[&IndexItem], root_path: &str) -> String {
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

    // Compute global column widths across all items
    let widths = column_widths(items);

    let mut out = String::new();
    let mut first = true;
    for (module_path, module_items) in &by_module {
        if !first {
            out.push('\n');
        }
        first = false;
        let _ = writeln!(out, "{module_path}:");
        for item in module_items {
            render_item_line(&mut out, item, &widths, "  ");
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Column widths for aligned table output.
struct ColumnWidths {
    kind: usize,
    path: usize,
    signature: usize,
}

/// Computes column widths from a slice of items.
fn column_widths(items: &[&IndexItem]) -> ColumnWidths {
    ColumnWidths {
        kind: items
            .iter()
            .map(|i| i.kind.short_name().len())
            .max()
            .unwrap_or(0),
        path: items.iter().map(|i| i.path.len()).max().unwrap_or(0),
        signature: items.iter().map(|i| i.signature.len()).max().unwrap_or(0),
    }
}

/// Renders one item as a table row: `{prefix}{kind}  {path}  {signature}  {summary}`.
fn render_item_line(out: &mut String, item: &IndexItem, widths: &ColumnWidths, prefix: &str) {
    let kind = item.kind.short_name();
    let path = &item.path;
    let sig = &item.signature;
    let summary = &item.summary;
    let gate_suffix = feature_gate_suffix(item.feature_gate.as_ref());
    let max_kind = widths.kind;
    let max_path = widths.path;
    let max_sig = widths.signature;

    // Build the trailing part: signature + summary + gate suffix
    let has_sig = !sig.is_empty();
    let has_summary = !summary.is_empty() || !gate_suffix.is_empty();

    if !has_sig && !has_summary {
        let _ = writeln!(out, "{prefix}{kind:<max_kind$}  {path:<max_path$}");
    } else if !has_sig {
        // No signature (e.g., modules) — skip sig column, show summary
        let display_summary = build_display_summary(summary, &gate_suffix);
        let _ = writeln!(
            out,
            "{prefix}{kind:<max_kind$}  {path:<max_path$}  {:<max_sig$}  {display_summary}",
            ""
        );
    } else if !has_summary {
        let _ = writeln!(
            out,
            "{prefix}{kind:<max_kind$}  {path:<max_path$}  {sig:<max_sig$}"
        );
    } else {
        let display_summary = build_display_summary(summary, &gate_suffix);
        let _ = writeln!(
            out,
            "{prefix}{kind:<max_kind$}  {path:<max_path$}  {sig:<max_sig$}  {display_summary}"
        );
    }
}

/// Combines summary and feature gate suffix into one display string.
fn build_display_summary(summary: &str, gate_suffix: &str) -> String {
    if gate_suffix.is_empty() {
        summary.to_string()
    } else if summary.is_empty() {
        gate_suffix.to_string()
    } else {
        format!("{summary}{gate_suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item_full;
    use crate::types::ItemKind;

    #[test]
    fn render_list_recursive_groups_by_module() {
        let items = [
            make_item_full(
                "bar",
                "mycrate::bar",
                ItemKind::Function,
                "pub fn bar()",
                "Does bar.",
                "Does bar.",
            ),
            make_item_full(
                "sub",
                "mycrate::sub",
                ItemKind::Module,
                "",
                "A submodule.",
                "A submodule.",
            ),
            make_item_full(
                "Foo",
                "mycrate::sub::Foo",
                ItemKind::Struct,
                "pub struct Foo",
                "A struct.",
                "A struct.",
            ),
        ];
        let refs: Vec<&IndexItem> = items.iter().collect();
        let output = render_list_recursive(&refs, "mycrate");

        insta::assert_snapshot!(output);
    }
}
