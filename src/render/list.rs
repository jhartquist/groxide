use std::fmt::Write;

use crate::types::{DisplayItem, GroupedItems, IndexItem};

use super::{feature_gate_suffix, trim_trailing_newlines};

/// Renders a `DisplayItem` in list mode (`--list`).
///
/// Produces one line per child item with dynamically-aligned columns:
/// `{kind}  {path}  {signature}  {summary}`
pub(crate) fn render_list(display: &DisplayItem<'_>) -> String {
    let items = collect_list_items(display);
    if items.is_empty() {
        return String::new();
    }

    render_item_table(&items, "")
}

/// Collects the items to list based on the `DisplayItem` variant.
fn collect_list_items<'a>(display: &'a DisplayItem<'a>) -> Vec<&'a IndexItem> {
    match display {
        DisplayItem::Crate { children, .. } | DisplayItem::Module { children, .. } => {
            collect_grouped_items(children)
        }
        DisplayItem::Type { methods, .. } => methods.clone(),
        DisplayItem::Trait {
            required_methods,
            provided_methods,
            ..
        } => {
            let mut items: Vec<&IndexItem> = Vec::new();
            items.extend(required_methods);
            items.extend(provided_methods);
            items
        }
        DisplayItem::Leaf { item } => vec![item],
    }
}

/// Flattens grouped items into a single vec in category display order.
fn collect_grouped_items<'a>(groups: &'a GroupedItems<'a>) -> Vec<&'a IndexItem> {
    let mut items: Vec<&IndexItem> = Vec::new();
    for group_items in groups.values() {
        items.extend(group_items);
    }
    items
}

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

/// Renders a table of items with aligned columns.
///
/// Used by both `render_list` and could be used by other table-style renderers.
fn render_item_table(items: &[&IndexItem], prefix: &str) -> String {
    let widths = column_widths(items);
    let mut out = String::new();
    for item in items {
        render_item_line(&mut out, item, &widths, prefix);
    }
    trim_trailing_newlines(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::build_display_item;
    use crate::test_utils::make_item_full;
    use crate::types::{ChildRef, DocIndex, ItemKind};

    #[test]
    fn render_list_output_with_column_alignment() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let struct_item = make_item_full(
            "Mutex",
            "mycrate::sync::Mutex",
            ItemKind::Struct,
            "pub struct Mutex<T>",
            "An asynchronous Mutex-like type.",
            "An asynchronous Mutex-like type.",
        );
        let enum_item = make_item_full(
            "TryLockError",
            "mycrate::sync::TryLockError",
            ItemKind::Enum,
            "pub enum TryLockError",
            "Error returned from try_lock.",
            "Error returned from try_lock.",
        );
        let mod_item = make_item_full(
            "mpsc",
            "mycrate::sync::mpsc",
            ItemKind::Module,
            "",
            "A multi-producer, single-consumer queue.",
            "A multi-producer, single-consumer queue.",
        );
        let fn_item = make_item_full(
            "channel",
            "mycrate::sync::channel",
            ItemKind::Function,
            "pub fn channel<T>(capacity: usize) -> (Sender, Receiver)",
            "Creates a broadcast channel.",
            "Creates a broadcast channel.",
        );

        index.add_item(struct_item);
        index.add_item(enum_item);
        index.add_item(mod_item);
        index.add_item(fn_item);

        let mut parent_mod = make_item_full(
            "sync",
            "mycrate::sync",
            ItemKind::Module,
            "",
            "Synchronization primitives.",
            "Synchronization primitives.",
        );
        parent_mod.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Struct,
                name: "Mutex".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Enum,
                name: "TryLockError".to_string(),
            },
            ChildRef {
                index: 2,
                kind: ItemKind::Module,
                name: "mpsc".to_string(),
            },
            ChildRef {
                index: 3,
                kind: ItemKind::Function,
                name: "channel".to_string(),
            },
        ];
        index.add_item(parent_mod);

        let di = build_display_item(&index, 4, false);
        let output = render_list(&di);

        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_list_feature_gate_annotation() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let mut gated_mod = make_item_full(
            "fs",
            "mycrate::fs",
            ItemKind::Module,
            "",
            "Filesystem utilities.",
            "Filesystem utilities.",
        );
        gated_mod.feature_gate = Some("fs".to_string());
        let normal_mod = make_item_full(
            "io",
            "mycrate::io",
            ItemKind::Module,
            "",
            "I/O utilities.",
            "I/O utilities.",
        );

        index.add_item(gated_mod);
        index.add_item(normal_mod);

        let mut crate_item = make_item_full(
            "mycrate",
            "mycrate",
            ItemKind::Module,
            "",
            "A crate.",
            "A crate.",
        );
        crate_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Module,
                name: "fs".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Module,
                name: "io".to_string(),
            },
        ];
        index.add_item(crate_item);

        let di = build_display_item(&index, 2, false);
        let output = render_list(&di);

        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_list_type_shows_methods() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let m1 = make_item_full(
            "lock",
            "mycrate::Mutex::lock",
            ItemKind::Function,
            "pub fn lock(&self)",
            "Locks the mutex.",
            "Locks the mutex.",
        );
        let m2 = make_item_full(
            "new",
            "mycrate::Mutex::new",
            ItemKind::Function,
            "pub fn new(t: T) -> Self",
            "Creates a new lock.",
            "Creates a new lock.",
        );
        index.add_item(m1);
        index.add_item(m2);

        let mut struct_item = make_item_full(
            "Mutex",
            "mycrate::Mutex",
            ItemKind::Struct,
            "pub struct Mutex<T>",
            "A mutex.",
            "A mutex.",
        );
        struct_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Function,
                name: "lock".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "new".to_string(),
            },
        ];
        index.add_item(struct_item);

        let di = build_display_item(&index, 2, false);
        let output = render_list(&di);

        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_list_leaf_shows_single_item() {
        let item = make_item_full(
            "spawn",
            "mycrate::spawn",
            ItemKind::Function,
            "pub fn spawn<F>(future: F) -> JoinHandle<F::Output>",
            "Spawns a task.",
            "Spawns a task.",
        );
        let di = DisplayItem::Leaf { item: &item };
        let output = render_list(&di);

        insta::assert_snapshot!(output);
    }

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
