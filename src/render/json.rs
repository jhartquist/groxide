use std::fmt::Write;

use serde::Serialize;

use crate::types::{DisplayItem, GroupedItems, IndexItem, TraitImplInfo};

/// JSON representation of a documented item (doc view).
#[derive(Debug, Serialize)]
pub(crate) struct JsonDocItem {
    /// Full item path.
    pub(crate) path: String,
    /// Item kind short name.
    pub(crate) kind: String,
    /// Rendered signature (empty string for modules).
    pub(crate) signature: String,
    /// Full doc comment (raw markdown, NOT stripped).
    pub(crate) doc: String,
    /// Feature gate name, or null.
    pub(crate) feature_gate: Option<String>,
    /// Methods (for Type and Trait items).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) methods: Option<Vec<JsonMethod>>,
    /// Trait implementation paths (for Type items).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) trait_impls: Option<Vec<String>>,
    /// Enum variants (for Enum items).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) variants: Option<Vec<JsonVariant>>,
}

/// JSON representation of a method.
#[derive(Debug, Serialize)]
pub(crate) struct JsonMethod {
    /// Simple method name.
    pub(crate) name: String,
    /// Rendered method signature.
    pub(crate) signature: String,
    /// First sentence of docs.
    pub(crate) summary: String,
    /// Whether the method has a body (provided). Only on trait methods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) has_body: Option<bool>,
}

/// JSON representation of an enum variant.
#[derive(Debug, Serialize)]
pub(crate) struct JsonVariant {
    /// Simple variant name.
    pub(crate) name: String,
    /// Rendered variant signature.
    pub(crate) signature: String,
    /// First sentence of docs.
    pub(crate) summary: String,
}

/// JSON representation of a list item (used in `--json --list` and container children).
#[derive(Debug, Serialize)]
pub(crate) struct JsonListItem {
    /// Full item path.
    pub(crate) path: String,
    /// Item kind short name.
    pub(crate) kind: String,
    /// Rendered signature.
    pub(crate) signature: String,
    /// First sentence of docs.
    pub(crate) summary: String,
}

/// Renders a `DisplayItem` as JSON doc view (`--json`).
///
/// For modules/crate roots, produces JSON Lines: the item itself + one line per child.
/// For other items, produces a single JSON object.
pub(crate) fn render_json(display: &DisplayItem<'_>) -> String {
    match display {
        DisplayItem::Crate { item, children } | DisplayItem::Module { item, children } => {
            render_json_container(item, children)
        }
        DisplayItem::Type {
            item,
            methods,
            variants,
            trait_impls,
        } => render_json_type(item, methods, variants, trait_impls),
        DisplayItem::Trait {
            item,
            required_methods,
            provided_methods,
        } => render_json_trait(item, required_methods, provided_methods),
        DisplayItem::Leaf { item } => render_json_leaf(item),
    }
}

/// Renders a `DisplayItem` as JSON Lines list (`--json --list`).
///
/// Each line is a `JsonListItem`.
#[allow(dead_code)]
pub(crate) fn render_json_list(display: &DisplayItem<'_>) -> String {
    let items = collect_json_list_items(display);
    let mut out = String::new();
    for item in &items {
        let list_item = JsonListItem {
            path: item.path.clone(),
            kind: item.kind.short_name().to_string(),
            signature: item.signature.clone(),
            summary: item.summary.clone(),
        };
        let json = serde_json::to_string(&list_item).expect("invariant: JsonListItem serializes");
        let _ = writeln!(out, "{json}");
    }
    trim_trailing_newlines(&mut out);
    out
}

/// Renders ambiguous matches as JSON Lines.
///
/// Each line is a `JsonListItem` with path, kind, signature, summary.
pub(crate) fn render_json_ambiguous(items: &[&IndexItem]) -> String {
    let mut out = String::new();
    for item in items {
        let list_item = JsonListItem {
            path: item.path.clone(),
            kind: item.kind.short_name().to_string(),
            signature: item.signature.clone(),
            summary: item.summary.clone(),
        };
        let json = serde_json::to_string(&list_item).expect("invariant: JsonListItem serializes");
        let _ = writeln!(out, "{json}");
    }
    trim_trailing_newlines(&mut out);
    out
}

/// Renders a module or crate root as JSON Lines.
fn render_json_container(item: &IndexItem, children: &GroupedItems<'_>) -> String {
    let mut out = String::new();

    // First line: the container item itself
    let doc_item = JsonDocItem {
        path: item.path.clone(),
        kind: item.kind.short_name().to_string(),
        signature: item.signature.clone(),
        doc: item.docs.clone(),
        feature_gate: item.feature_gate.clone(),
        methods: None,
        trait_impls: None,
        variants: None,
    };
    let json = serde_json::to_string(&doc_item).expect("invariant: JsonDocItem serializes");
    let _ = writeln!(out, "{json}");

    // Subsequent lines: children as JsonListItem
    for group_items in children.values() {
        for child in group_items {
            let list_item = JsonListItem {
                path: child.path.clone(),
                kind: child.kind.short_name().to_string(),
                signature: child.signature.clone(),
                summary: child.summary.clone(),
            };
            let json =
                serde_json::to_string(&list_item).expect("invariant: JsonListItem serializes");
            let _ = writeln!(out, "{json}");
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders a type (struct/enum/union) as a single JSON object.
fn render_json_type(
    item: &IndexItem,
    methods: &[&IndexItem],
    variants: &[&IndexItem],
    trait_impls: &[TraitImplInfo],
) -> String {
    let json_methods: Vec<JsonMethod> = methods
        .iter()
        .map(|m| JsonMethod {
            name: m.name.clone(),
            signature: m.signature.clone(),
            summary: m.summary.clone(),
            has_body: None,
        })
        .collect();

    let json_variants: Vec<JsonVariant> = variants
        .iter()
        .map(|v| JsonVariant {
            name: v.name.clone(),
            signature: v.signature.clone(),
            summary: v.summary.clone(),
        })
        .collect();

    let json_trait_impls: Vec<String> =
        trait_impls.iter().map(|ti| ti.trait_path.clone()).collect();

    let doc_item = JsonDocItem {
        path: item.path.clone(),
        kind: item.kind.short_name().to_string(),
        signature: item.signature.clone(),
        doc: item.docs.clone(),
        feature_gate: item.feature_gate.clone(),
        methods: if json_methods.is_empty() {
            None
        } else {
            Some(json_methods)
        },
        trait_impls: if json_trait_impls.is_empty() {
            None
        } else {
            Some(json_trait_impls)
        },
        variants: if json_variants.is_empty() {
            None
        } else {
            Some(json_variants)
        },
    };

    serde_json::to_string(&doc_item).expect("invariant: JsonDocItem serializes")
}

/// Renders a trait as a single JSON object.
fn render_json_trait(
    item: &IndexItem,
    required_methods: &[&IndexItem],
    provided_methods: &[&IndexItem],
) -> String {
    let mut json_methods: Vec<JsonMethod> = Vec::new();

    for m in required_methods {
        json_methods.push(JsonMethod {
            name: m.name.clone(),
            signature: m.signature.clone(),
            summary: m.summary.clone(),
            has_body: Some(false),
        });
    }
    for m in provided_methods {
        json_methods.push(JsonMethod {
            name: m.name.clone(),
            signature: m.signature.clone(),
            summary: m.summary.clone(),
            has_body: Some(true),
        });
    }

    let doc_item = JsonDocItem {
        path: item.path.clone(),
        kind: item.kind.short_name().to_string(),
        signature: item.signature.clone(),
        doc: item.docs.clone(),
        feature_gate: item.feature_gate.clone(),
        methods: if json_methods.is_empty() {
            None
        } else {
            Some(json_methods)
        },
        trait_impls: None,
        variants: None,
    };

    serde_json::to_string(&doc_item).expect("invariant: JsonDocItem serializes")
}

/// Renders a leaf item as a single JSON object.
fn render_json_leaf(item: &IndexItem) -> String {
    let doc_item = JsonDocItem {
        path: item.path.clone(),
        kind: item.kind.short_name().to_string(),
        signature: item.signature.clone(),
        doc: item.docs.clone(),
        feature_gate: item.feature_gate.clone(),
        methods: None,
        trait_impls: None,
        variants: None,
    };

    serde_json::to_string(&doc_item).expect("invariant: JsonDocItem serializes")
}

/// Collects items for JSON list mode.
#[allow(dead_code)]
fn collect_json_list_items<'a>(display: &'a DisplayItem<'a>) -> Vec<&'a IndexItem> {
    match display {
        DisplayItem::Crate { children, .. } | DisplayItem::Module { children, .. } => {
            let mut items = Vec::new();
            for group_items in children.values() {
                items.extend(group_items);
            }
            items
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

/// Removes trailing newlines from output.
fn trim_trailing_newlines(s: &mut String) {
    while s.ends_with('\n') {
        s.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::build_display_item;
    use crate::types::{ChildRef, DocIndex, IndexItem, ItemKind, SourceSpan, TraitImplInfo};

    fn make_source_span() -> SourceSpan {
        SourceSpan {
            file: String::new(),
            line_start: 0,
            line_end: 0,
        }
    }

    fn make_item_full(
        name: &str,
        path: &str,
        kind: ItemKind,
        signature: &str,
        docs: &str,
        summary: &str,
    ) -> IndexItem {
        IndexItem {
            path: path.to_string(),
            name: name.to_string(),
            kind,
            signature: signature.to_string(),
            docs: docs.to_string(),
            summary: summary.to_string(),
            span: make_source_span(),
            children: Vec::new(),
            is_public: true,
            has_body: false,
            feature_gate: None,
        }
    }

    // ---- JSON doc view for struct ----

    #[test]
    fn render_json_struct() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let m1 = make_item_full(
            "lock",
            "mycrate::Mutex::lock",
            ItemKind::Function,
            "pub fn lock(&self) -> MutexGuard<'_, T>",
            "Locks this mutex.",
            "Locks this mutex.",
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
            "pub struct Mutex<T: ?Sized>",
            "An asynchronous Mutex-like type.",
            "An asynchronous Mutex-like type.",
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

        index.trait_impls.insert(
            2,
            vec![
                TraitImplInfo {
                    trait_path: "Debug".to_string(),
                    is_synthetic: false,
                },
                TraitImplInfo {
                    trait_path: "Send".to_string(),
                    is_synthetic: true,
                },
            ],
        );

        let di = build_display_item(&index, 2, false);
        let output = render_json(&di);

        // Parse and verify it's valid JSON
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("should be valid JSON");
        assert_eq!(parsed["path"], "mycrate::Mutex");
        assert_eq!(parsed["kind"], "struct");
        assert_eq!(parsed["signature"], "pub struct Mutex<T: ?Sized>");
        assert!(parsed["methods"].is_array());
        assert_eq!(parsed["methods"].as_array().unwrap().len(), 2);
        assert!(parsed["trait_impls"].is_array());
        assert_eq!(parsed["trait_impls"].as_array().unwrap().len(), 2);
        assert!(parsed["variants"].is_null() || parsed.get("variants").is_none());

        insta::assert_snapshot!(output);
    }

    // ---- JSON doc view for trait ----

    #[test]
    fn render_json_trait_view() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let mut req = make_item_full(
            "next",
            "mycrate::Iterator::next",
            ItemKind::Function,
            "fn next(&mut self) -> Option<Self::Item>",
            "Advances the iterator.",
            "Advances the iterator.",
        );
        req.has_body = false;

        let mut prov = make_item_full(
            "count",
            "mycrate::Iterator::count",
            ItemKind::Function,
            "fn count(self) -> usize",
            "Consumes the iterator.",
            "Consumes the iterator.",
        );
        prov.has_body = true;

        index.add_item(req);
        index.add_item(prov);

        let mut trait_item = make_item_full(
            "Iterator",
            "mycrate::Iterator",
            ItemKind::Trait,
            "pub trait Iterator",
            "An interface for dealing with iterators.",
            "An interface for dealing with iterators.",
        );
        trait_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Function,
                name: "next".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "count".to_string(),
            },
        ];
        index.add_item(trait_item);

        let di = build_display_item(&index, 2, false);
        let output = render_json(&di);

        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("should be valid JSON");
        assert_eq!(parsed["kind"], "trait");
        let methods = parsed["methods"].as_array().unwrap();
        assert_eq!(methods.len(), 2);
        // Required method has has_body: false
        assert_eq!(methods[0]["has_body"], false);
        // Provided method has has_body: true
        assert_eq!(methods[1]["has_body"], true);

        insta::assert_snapshot!(output);
    }

    // ---- JSON doc view for crate root (with top_level_items) ----

    #[test]
    fn render_json_crate_root_includes_top_level_items() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let struct_item = make_item_full(
            "Widget",
            "mycrate::Widget",
            ItemKind::Struct,
            "pub struct Widget",
            "A widget.",
            "A widget.",
        );
        let mod_item = make_item_full(
            "utils",
            "mycrate::utils",
            ItemKind::Module,
            "",
            "Utility helpers.",
            "Utility helpers.",
        );
        let fn_item = make_item_full(
            "process",
            "mycrate::process",
            ItemKind::Function,
            "pub fn process() -> u32",
            "Processes data.",
            "Processes data.",
        );

        index.add_item(struct_item);
        index.add_item(mod_item);
        index.add_item(fn_item);

        let mut crate_item = make_item_full(
            "mycrate",
            "mycrate",
            ItemKind::Module,
            "",
            "A test crate.",
            "A test crate.",
        );
        crate_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Struct,
                name: "Widget".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Module,
                name: "utils".to_string(),
            },
            ChildRef {
                index: 2,
                kind: ItemKind::Function,
                name: "process".to_string(),
            },
        ];
        index.add_item(crate_item);

        let di = build_display_item(&index, 3, false);
        let output = render_json(&di);

        // Should be JSON Lines
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 4); // 1 crate + 3 children

        // First line is the crate itself
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["path"], "mycrate");
        assert_eq!(first["kind"], "mod");
        assert!(first.get("doc").is_some()); // Has full doc field

        // Subsequent lines are children with summary
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert!(second.get("summary").is_some());
        assert!(second.get("doc").is_none()); // No doc field on list items

        insta::assert_snapshot!(output);
    }

    // ---- JSON Lines list output ----

    #[test]
    fn render_json_lines_list() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let s1 = make_item_full(
            "Mutex",
            "mycrate::sync::Mutex",
            ItemKind::Struct,
            "pub struct Mutex<T: ?Sized>",
            "An asynchronous Mutex-like type.",
            "An asynchronous Mutex-like type.",
        );
        let s2 = make_item_full(
            "RwLock",
            "mycrate::sync::RwLock",
            ItemKind::Struct,
            "pub struct RwLock<T: ?Sized>",
            "An asynchronous reader-writer lock.",
            "An asynchronous reader-writer lock.",
        );

        index.add_item(s1);
        index.add_item(s2);

        let mut mod_item = make_item_full(
            "sync",
            "mycrate::sync",
            ItemKind::Module,
            "",
            "Sync primitives.",
            "Sync primitives.",
        );
        mod_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Struct,
                name: "Mutex".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Struct,
                name: "RwLock".to_string(),
            },
        ];
        index.add_item(mod_item);

        let di = build_display_item(&index, 2, false);
        let output = render_json_list(&di);

        // Each line should be valid JSON
        for line in output.lines() {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("each line should be valid JSON");
            assert!(parsed.get("path").is_some());
            assert!(parsed.get("kind").is_some());
            assert!(parsed.get("signature").is_some());
            assert!(parsed.get("summary").is_some());
            // No doc field in list items
            assert!(parsed.get("doc").is_none());
        }

        insta::assert_snapshot!(output);
    }

    // ---- JSON ambiguous output (array as JSON Lines) ----

    #[test]
    fn render_json_ambiguous_output() {
        let item1 = make_item_full(
            "Error",
            "mycrate::de::Error",
            ItemKind::Trait,
            "pub trait Error: Sized",
            "When deserialization encounters an error.",
            "When deserialization encounters an error.",
        );
        let item2 = make_item_full(
            "Error",
            "mycrate::ser::Error",
            ItemKind::Trait,
            "pub trait Error: Sized",
            "When serialization encounters an error.",
            "When serialization encounters an error.",
        );

        let items: Vec<&IndexItem> = vec![&item1, &item2];
        let output = render_json_ambiguous(&items);

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["path"], "mycrate::de::Error");
        assert_eq!(first["kind"], "trait");

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["path"], "mycrate::ser::Error");

        insta::assert_snapshot!(output);
    }
}
