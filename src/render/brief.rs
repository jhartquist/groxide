use std::fmt::Write;

use crate::types::{DisplayItem, IndexItem};

use super::trim_trailing_newlines;

/// Renders a brief view showing only kind + name for a single item.
///
/// For container items (crate, module), lists children with kind + name.
/// For types, shows the type header plus method/variant names.
/// For traits, shows the trait header plus method names.
/// For leaf items, shows just kind + name.
pub(crate) fn render_brief(display: &DisplayItem<'_>) -> String {
    let mut out = String::new();

    match display {
        DisplayItem::Crate { item, children } => {
            let _ = writeln!(out, "crate {}", item.path);
            let max_kind = children
                .values()
                .flat_map(|items| items.iter())
                .map(|i| i.kind.short_name().len())
                .max()
                .unwrap_or(0);
            for items in children.values() {
                for child in items {
                    let kind = child.kind.short_name();
                    let _ = writeln!(out, "  {kind:<max_kind$}  {}", child.name);
                }
            }
        }
        DisplayItem::Module { item, children } => {
            let _ = writeln!(out, "{} {}", item.kind.short_name(), item.path);
            let max_kind = children
                .values()
                .flat_map(|items| items.iter())
                .map(|i| i.kind.short_name().len())
                .max()
                .unwrap_or(0);
            for items in children.values() {
                for child in items {
                    let kind = child.kind.short_name();
                    let _ = writeln!(out, "  {kind:<max_kind$}  {}", child.name);
                }
            }
        }
        DisplayItem::Type {
            item,
            methods,
            variants,
            ..
        } => {
            let _ = writeln!(out, "{} {}", item.kind.short_name(), item.path);
            for v in variants {
                let _ = writeln!(out, "  variant  {}", v.name);
            }
            for m in methods {
                let _ = writeln!(out, "  fn       {}", m.name);
            }
        }
        DisplayItem::Trait {
            item,
            required_methods,
            provided_methods,
        } => {
            let _ = writeln!(out, "trait {}", item.path);
            for m in required_methods {
                let _ = writeln!(out, "  fn  {}", m.name);
            }
            for m in provided_methods {
                let _ = writeln!(out, "  fn  {}", m.name);
            }
        }
        DisplayItem::Leaf { item } => {
            let _ = writeln!(out, "{} {}", item.kind.short_name(), item.path);
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders a brief recursive listing of items grouped by parent module.
///
/// Shows only kind + name columns, grouped under module path headers.
pub(crate) fn render_brief_recursive(items: &[&IndexItem], root_path: &str) -> String {
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

    // Compute max kind width across all items
    let max_kind = items
        .iter()
        .map(|i| i.kind.short_name().len())
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    let mut first = true;
    for (module_path, module_items) in &by_module {
        if !first {
            out.push('\n');
        }
        first = false;
        let _ = writeln!(out, "{module_path}:");
        for item in module_items {
            let kind = item.kind.short_name();
            let _ = writeln!(out, "  {kind:<max_kind$}  {}", item.name);
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item_full;
    use crate::types::{ChildRef, DocIndex, ItemKind};

    use crate::render::build_display_item;
    use crate::test_utils::make_item;

    #[test]
    fn render_brief_module_shows_kind_and_name_only() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut root = make_item("mycrate", "mycrate", ItemKind::Module);
        let child_fn = make_item("bar", "mycrate::bar", ItemKind::Function);
        let child_mod = make_item("sub", "mycrate::sub", ItemKind::Module);
        index.add_item(root.clone());
        index.add_item(child_fn);
        index.add_item(child_mod);
        root.children = vec![
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "bar".to_string(),
            },
            ChildRef {
                index: 2,
                kind: ItemKind::Module,
                name: "sub".to_string(),
            },
        ];
        index.items[0] = root;

        let display = build_display_item(&index, 0, false);
        let output = render_brief(&display);
        assert!(output.contains("crate mycrate"), "header: {output}");
        assert!(
            output.contains("fn   bar") || output.contains("fn  bar"),
            "child fn: {output}"
        );
        assert!(output.contains("mod  sub"), "child mod: {output}");
        assert!(!output.contains("pub"), "no signatures: {output}");
    }

    #[test]
    fn render_brief_type_shows_method_names() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut st = make_item_full(
            "Foo",
            "mycrate::Foo",
            ItemKind::Struct,
            "pub struct Foo",
            "",
            "",
        );
        let method = make_item("do_thing", "mycrate::Foo::do_thing", ItemKind::Function);
        index.add_item(st.clone());
        index.add_item(method);
        st.children = vec![ChildRef {
            index: 1,
            kind: ItemKind::Function,
            name: "do_thing".to_string(),
        }];
        index.items[0] = st;

        let display = build_display_item(&index, 0, false);
        let output = render_brief(&display);
        assert!(output.contains("struct mycrate::Foo"), "header: {output}");
        assert!(output.contains("fn       do_thing"), "method: {output}");
        assert!(
            !output.contains("pub struct"),
            "no full signature: {output}"
        );
    }

    #[test]
    fn render_brief_recursive_groups_by_module() {
        let items = [
            make_item_full(
                "bar",
                "mycrate::bar",
                ItemKind::Function,
                "pub fn bar()",
                "",
                "",
            ),
            make_item_full("sub", "mycrate::sub", ItemKind::Module, "", "", ""),
            make_item_full(
                "Foo",
                "mycrate::sub::Foo",
                ItemKind::Struct,
                "pub struct Foo",
                "",
                "",
            ),
        ];
        let refs: Vec<&IndexItem> = items.iter().collect();
        let output = render_brief_recursive(&refs, "mycrate");

        assert!(output.contains("mycrate:"), "root module header: {output}");
        assert!(
            output.contains("mycrate::sub:"),
            "sub module header: {output}"
        );
        assert!(output.contains("fn      bar"), "fn name: {output}");
        assert!(output.contains("struct  Foo"), "struct name: {output}");
        assert!(!output.contains("pub fn"), "no signatures: {output}");
        assert!(!output.contains("pub struct"), "no signatures: {output}");
    }
}
