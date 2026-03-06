use std::fmt::Write;

use crate::types::{DisplayItem, DisplayLimits, GroupedItems, IndexItem, TraitImplInfo};

use super::{feature_gate_suffix, strip_markdown, trim_trailing_newlines, truncate_doc};

/// Renders a `DisplayItem` as plain text.
///
/// Returns the complete plain text output string ready for stdout.
pub(crate) fn render_text(display: &DisplayItem<'_>, limits: &DisplayLimits) -> String {
    match display {
        DisplayItem::Crate { item, children } => render_crate(item, children, limits),
        DisplayItem::Module { item, children } => render_module(item, children, limits),
        DisplayItem::Type {
            item,
            methods,
            variants,
            trait_impls,
        } => render_type(item, methods, variants, trait_impls, limits),
        DisplayItem::Trait {
            item,
            required_methods,
            provided_methods,
        } => render_trait(item, required_methods, provided_methods, limits),
        DisplayItem::Leaf { item } => render_leaf(item, limits),
    }
}

/// Renders crate root display.
fn render_crate(item: &IndexItem, children: &GroupedItems<'_>, limits: &DisplayLimits) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "crate {}{gate}", item.name);

    // Doc text
    if !item.docs.is_empty() {
        let stripped = strip_markdown(&item.docs);
        let truncated = truncate_doc(&stripped, limits);
        out.push('\n');
        out.push_str(&truncated);
        out.push('\n');
    }

    // Children grouped by category
    render_grouped_children(&mut out, children);

    trim_trailing_newlines(&mut out);
    out
}

/// Renders module display.
fn render_module(item: &IndexItem, children: &GroupedItems<'_>, limits: &DisplayLimits) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "mod {}{gate}", item.path);

    // Doc text
    if !item.docs.is_empty() {
        let stripped = strip_markdown(&item.docs);
        let truncated = truncate_doc(&stripped, limits);
        out.push('\n');
        out.push_str(&truncated);
        out.push('\n');
    }

    // Children grouped by category
    render_grouped_children(&mut out, children);

    trim_trailing_newlines(&mut out);
    out
}

/// Renders type (struct/enum/union) display.
fn render_type(
    item: &IndexItem,
    methods: &[&IndexItem],
    variants: &[&IndexItem],
    trait_impls: &[TraitImplInfo],
    limits: &DisplayLimits,
) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "{} {}{gate}", item.kind.short_name(), item.path);

    // Signature
    out.push('\n');
    out.push_str(&item.signature);
    out.push('\n');

    // Doc text
    if !item.docs.is_empty() {
        let stripped = strip_markdown(&item.docs);
        let truncated = truncate_doc(&stripped, limits);
        out.push('\n');
        out.push_str(&truncated);
        out.push('\n');
    }

    // Variants (enums only)
    if !variants.is_empty() {
        out.push('\n');
        out.push_str("Variants:\n");
        for v in variants {
            render_signature_line(&mut out, v);
        }
    }

    // Methods
    if !methods.is_empty() {
        out.push('\n');
        out.push_str("Methods:\n");
        for m in methods {
            render_signature_line(&mut out, m);
        }
    }

    // Trait implementations
    if !trait_impls.is_empty() {
        // Sort: non-synthetic first (alphabetically), then synthetic (alphabetically)
        let mut sorted_impls: Vec<&TraitImplInfo> = trait_impls.iter().collect();
        sorted_impls.sort_by(|a, b| {
            a.is_synthetic
                .cmp(&b.is_synthetic)
                .then_with(|| a.trait_path.cmp(&b.trait_path))
        });

        out.push('\n');
        out.push_str("Trait Implementations:\n");
        for ti in &sorted_impls {
            let _ = writeln!(out, "  impl {}", ti.trait_path);
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders trait display.
fn render_trait(
    item: &IndexItem,
    required_methods: &[&IndexItem],
    provided_methods: &[&IndexItem],
    limits: &DisplayLimits,
) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "trait {}{gate}", item.path);

    // Signature
    out.push('\n');
    out.push_str(&item.signature);
    out.push('\n');

    // Doc text
    if !item.docs.is_empty() {
        let stripped = strip_markdown(&item.docs);
        let truncated = truncate_doc(&stripped, limits);
        out.push('\n');
        out.push_str(&truncated);
        out.push('\n');
    }

    let has_required = !required_methods.is_empty();
    let has_provided = !provided_methods.is_empty();

    if !has_required && !has_provided {
        // Marker trait
        out.push('\n');
        out.push_str("(no methods)");
    } else if has_required && has_provided {
        // Both: use distinct headers
        out.push('\n');
        out.push_str("Required Methods:\n");
        for m in required_methods {
            render_signature_line(&mut out, m);
        }

        out.push('\n');
        out.push_str("Provided Methods:\n");
        for m in provided_methods {
            render_signature_line(&mut out, m);
        }
    } else {
        // Only one kind: use generic "Methods:" header
        let methods = if has_required {
            required_methods
        } else {
            provided_methods
        };

        out.push('\n');
        out.push_str("Methods:\n");
        for m in methods {
            render_signature_line(&mut out, m);
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders leaf item display.
fn render_leaf(item: &IndexItem, limits: &DisplayLimits) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "{} {}{gate}", item.kind.short_name(), item.path);

    // Signature
    out.push('\n');
    out.push_str(&item.signature);
    out.push('\n');

    // Doc text
    if !item.docs.is_empty() {
        let stripped = strip_markdown(&item.docs);
        let truncated = truncate_doc(&stripped, limits);
        out.push('\n');
        out.push_str(&truncated);
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders grouped children sections for crate root and module views.
fn render_grouped_children(out: &mut String, children: &GroupedItems<'_>) {
    for (category, items) in children {
        out.push('\n');
        let _ = writeln!(out, "{}", category.header());
        if category.uses_signature_display() {
            for item in items {
                render_signature_line(out, item);
            }
        } else {
            for item in items {
                render_name_line(out, item);
            }
        }
    }
}

/// Renders a name + summary line (for container-like items in listings).
///
/// Format: `  {name:<30}  {summary}`
fn render_name_line(out: &mut String, item: &IndexItem) {
    let name = &item.name;
    let summary = &item.summary;
    let gate_suffix = feature_gate_suffix(item.feature_gate.as_ref());
    if summary.is_empty() && gate_suffix.is_empty() {
        let _ = writeln!(out, "  {name}");
    } else {
        let display_summary = if gate_suffix.is_empty() {
            summary.clone()
        } else if summary.is_empty() {
            gate_suffix
        } else {
            format!("{summary}{gate_suffix}")
        };
        let _ = writeln!(out, "  {name:<30}  {display_summary}");
    }
}

/// Renders a signature + summary line (for value-like items and methods/variants).
///
/// Format: `  {signature:<58}  {summary}`
fn render_signature_line(out: &mut String, item: &IndexItem) {
    let sig = &item.signature;
    let summary = &item.summary;
    let gate_suffix = feature_gate_suffix(item.feature_gate.as_ref());
    if summary.is_empty() && gate_suffix.is_empty() {
        let _ = writeln!(out, "  {sig}");
    } else {
        let display_summary = if gate_suffix.is_empty() {
            summary.clone()
        } else if summary.is_empty() {
            gate_suffix
        } else {
            format!("{summary}{gate_suffix}")
        };
        let _ = writeln!(out, "  {sig:<58}  {display_summary}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item_full;
    use crate::types::{ChildRef, DisplayLimits, DocIndex, ItemKind, TraitImplInfo};

    // ---- Crate root output format ----

    #[test]
    fn render_crate_root_output_format() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut crate_item = make_item_full(
            "mycrate",
            "mycrate",
            ItemKind::Module,
            "",
            "A framework for doing things.",
            "A framework for doing things.",
        );
        let struct_item = make_item_full(
            "Widget",
            "mycrate::Widget",
            ItemKind::Struct,
            "pub struct Widget",
            "A widget.",
            "A widget.",
        );
        let fn_item = make_item_full(
            "process",
            "mycrate::process",
            ItemKind::Function,
            "pub fn process(x: u32) -> u32",
            "Processes a value.",
            "Processes a value.",
        );
        let mod_item = make_item_full(
            "utils",
            "mycrate::utils",
            ItemKind::Module,
            "",
            "Utility helpers.",
            "Utility helpers.",
        );

        index.add_item(struct_item);
        index.add_item(fn_item);
        index.add_item(mod_item);
        crate_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Struct,
                name: "Widget".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "process".to_string(),
            },
            ChildRef {
                index: 2,
                kind: ItemKind::Module,
                name: "utils".to_string(),
            },
        ];
        index.add_item(crate_item);

        let di = crate::render::build_display_item(&index, 3, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Module output format ----

    #[test]
    fn render_module_output_format() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut mod_item = make_item_full(
            "sync",
            "mycrate::sync",
            ItemKind::Module,
            "",
            "Synchronization primitives.",
            "Synchronization primitives.",
        );
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

        index.add_item(struct_item);
        index.add_item(enum_item);
        mod_item.children = vec![
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
        ];
        index.add_item(mod_item);

        let di = crate::render::build_display_item(&index, 2, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Struct with methods (truncated at 15) ----

    #[test]
    fn render_struct_with_methods_truncated_at_15() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        // Create 20 method items
        let mut children = Vec::new();
        for i in 0..20 {
            let name = format!("method_{i:02}");
            let path = format!("mycrate::Big::method_{i:02}");
            let sig = format!("pub fn method_{i:02}(&self)");
            let summary = format!("Method {i}.");
            let method = make_item_full(&name, &path, ItemKind::Function, &sig, "", &summary);
            let idx = index.items.len();
            index.add_item(method);
            children.push(ChildRef {
                index: idx,
                kind: ItemKind::Function,
                name,
            });
        }

        let mut struct_item = make_item_full(
            "Big",
            "mycrate::Big",
            ItemKind::Struct,
            "pub struct Big",
            "A struct with many methods.",
            "A struct with many methods.",
        );
        struct_item.children = children;
        index.add_item(struct_item);

        let di = crate::render::build_display_item(&index, 20, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Enum with variants ----

    #[test]
    fn render_enum_with_variants() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let v1 = make_item_full(
            "Red",
            "mycrate::Color::Red",
            ItemKind::Variant,
            "Red",
            "",
            "The color red.",
        );
        let v2 = make_item_full(
            "Green",
            "mycrate::Color::Green",
            ItemKind::Variant,
            "Green",
            "",
            "The color green.",
        );
        let v3 = make_item_full(
            "Blue",
            "mycrate::Color::Blue",
            ItemKind::Variant,
            "Blue(u8)",
            "",
            "The color blue with intensity.",
        );
        let method = make_item_full(
            "name",
            "mycrate::Color::name",
            ItemKind::Function,
            "pub fn name(&self) -> &str",
            "Returns the color name.",
            "Returns the color name.",
        );

        index.add_item(v1);
        index.add_item(v2);
        index.add_item(v3);
        index.add_item(method);

        let mut enum_item = make_item_full(
            "Color",
            "mycrate::Color",
            ItemKind::Enum,
            "pub enum Color",
            "Represents a color.",
            "Represents a color.",
        );
        enum_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Variant,
                name: "Red".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Variant,
                name: "Green".to_string(),
            },
            ChildRef {
                index: 2,
                kind: ItemKind::Variant,
                name: "Blue".to_string(),
            },
            ChildRef {
                index: 3,
                kind: ItemKind::Function,
                name: "name".to_string(),
            },
        ];
        index.add_item(enum_item);

        let di = crate::render::build_display_item(&index, 4, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Trait with required + provided methods ----

    #[test]
    fn render_trait_with_required_and_provided_methods() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let mut req_method = make_item_full(
            "poll_read",
            "mycrate::AsyncRead::poll_read",
            ItemKind::Function,
            "fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>>",
            "Attempts to read from the source.",
            "Attempts to read from the source.",
        );
        req_method.has_body = false;

        let mut prov_method1 = make_item_full(
            "chain",
            "mycrate::AsyncRead::chain",
            ItemKind::Function,
            "fn chain<R>(self, next: R) -> Chain<Self, R>",
            "Creates an adaptor which chains this stream.",
            "Creates an adaptor which chains this stream.",
        );
        prov_method1.has_body = true;

        let mut prov_method2 = make_item_full(
            "take",
            "mycrate::AsyncRead::take",
            ItemKind::Function,
            "fn take(self, limit: u64) -> Take<Self>",
            "Creates an adaptor which reads at most limit bytes.",
            "Creates an adaptor which reads at most limit bytes.",
        );
        prov_method2.has_body = true;

        index.add_item(req_method);
        index.add_item(prov_method1);
        index.add_item(prov_method2);

        let mut trait_item = make_item_full(
            "AsyncRead",
            "mycrate::AsyncRead",
            ItemKind::Trait,
            "pub trait AsyncRead",
            "Read bytes from a source asynchronously.",
            "Read bytes from a source asynchronously.",
        );
        trait_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Function,
                name: "poll_read".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "chain".to_string(),
            },
            ChildRef {
                index: 2,
                kind: ItemKind::Function,
                name: "take".to_string(),
            },
        ];
        index.add_item(trait_item);

        let di = crate::render::build_display_item(&index, 3, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Function (leaf) with full docs ----

    #[test]
    fn render_function_leaf_with_full_docs() {
        let item = make_item_full(
            "spawn",
            "mycrate::spawn",
            ItemKind::Function,
            "pub fn spawn<F>(future: F) -> JoinHandle<F::Output>",
            "Spawns a new asynchronous task, returning a JoinHandle for it.\n\nThe provided future will start running in the background immediately\nwhen spawn is called, even if you don't await the returned JoinHandle.",
            "Spawns a new asynchronous task, returning a JoinHandle for it.",
        );
        let di = DisplayItem::Leaf { item: &item };
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Constant (leaf) ----

    #[test]
    fn render_constant_leaf() {
        let item = make_item_full(
            "PI",
            "std::f64::consts::PI",
            ItemKind::Constant,
            "pub const PI: f64",
            "Archimedes' constant (pi = 3.14159...).",
            "Archimedes' constant (pi = 3.14159...).",
        );
        let di = DisplayItem::Leaf { item: &item };
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Feature gate annotation in listings ----

    #[test]
    fn render_feature_gate_annotation_in_listings() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut mod_item = make_item_full(
            "mycrate",
            "mycrate",
            ItemKind::Module,
            "",
            "A crate.",
            "A crate.",
        );
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
        let mut gated_fn = make_item_full(
            "read",
            "mycrate::read",
            ItemKind::Function,
            "pub fn read(path: &str) -> Vec<u8>",
            "Reads a file.",
            "Reads a file.",
        );
        gated_fn.feature_gate = Some("fs".to_string());

        index.add_item(gated_mod);
        index.add_item(normal_mod);
        index.add_item(gated_fn);
        mod_item.children = vec![
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
            ChildRef {
                index: 2,
                kind: ItemKind::Function,
                name: "read".to_string(),
            },
        ];
        index.add_item(mod_item);

        let di = crate::render::build_display_item(&index, 3, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Truncation at ~1500 chars (with expand_all disabled) ----

    #[test]
    fn render_truncation_at_1500_chars() {
        let long_docs = format!(
            "First paragraph of documentation.\n\n{}",
            "This is a very long sentence that repeats. ".repeat(50)
        );
        let item = make_item_full(
            "big_fn",
            "mycrate::big_fn",
            ItemKind::Function,
            "pub fn big_fn()",
            &long_docs,
            "First paragraph of documentation.",
        );
        let di = DisplayItem::Leaf { item: &item };
        let limits = DisplayLimits {
            expand_all: false,
            ..DisplayLimits::default()
        };
        let output = render_text(&di, &limits);

        // Verify truncation happened
        assert!(
            output.contains("..."),
            "output should contain ... for truncation"
        );
        // Verify output is reasonable size
        assert!(
            output.len() < 2000,
            "output should be truncated, got {} bytes",
            output.len()
        );

        insta::assert_snapshot!(output);
    }

    // ---- Truncation respects UTF-8 boundaries ----

    #[test]
    fn render_truncation_respects_utf8_boundaries() {
        // Create text with multi-byte characters that exceeds 1500 chars
        let long_docs = "日本語テスト。".repeat(300);
        let item = make_item_full(
            "unicode_fn",
            "mycrate::unicode_fn",
            ItemKind::Function,
            "pub fn unicode_fn()",
            &long_docs,
            "Japanese test text.",
        );
        let di = DisplayItem::Leaf { item: &item };
        let limits = DisplayLimits {
            expand_all: false,
            ..DisplayLimits::default()
        };
        let output = render_text(&di, &limits);

        // Should not panic (proving UTF-8 safety) and should contain truncation marker
        assert!(output.contains("..."), "output should be truncated");
        // Verify output is valid UTF-8
        assert!(std::str::from_utf8(output.as_bytes()).is_ok());
    }

    // ---- Trait with only required methods uses generic "Methods:" header ----

    #[test]
    fn render_trait_only_required_uses_methods_header() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let mut method = make_item_full(
            "next",
            "mycrate::Iterator::next",
            ItemKind::Function,
            "fn next(&mut self) -> Option<Self::Item>",
            "Advances the iterator.",
            "Advances the iterator.",
        );
        method.has_body = false;
        index.add_item(method);

        let mut trait_item = make_item_full(
            "Iterator",
            "mycrate::Iterator",
            ItemKind::Trait,
            "pub trait Iterator",
            "An interface for dealing with iterators.",
            "An interface for dealing with iterators.",
        );
        trait_item.children = vec![ChildRef {
            index: 0,
            kind: ItemKind::Function,
            name: "next".to_string(),
        }];
        index.add_item(trait_item);

        let di = crate::render::build_display_item(&index, 1, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        assert!(output.contains("Methods:"));
        assert!(!output.contains("Required Methods:"));
        assert!(!output.contains("Provided Methods:"));
    }

    // ---- Marker trait shows (no methods) ----

    #[test]
    fn render_marker_trait_shows_no_methods() {
        let item = make_item_full(
            "Send",
            "mycrate::Send",
            ItemKind::Trait,
            "pub unsafe auto trait Send",
            "Types that can be transferred across thread boundaries.",
            "Types that can be transferred across thread boundaries.",
        );
        let di = DisplayItem::Trait {
            item: &item,
            required_methods: vec![],
            provided_methods: vec![],
        };
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Leaf with no docs ----

    #[test]
    fn render_leaf_no_docs() {
        let item = make_item_full(
            "internal_helper",
            "mycrate::internal_helper",
            ItemKind::Function,
            "pub fn internal_helper(x: u32) -> u32",
            "",
            "",
        );
        let di = DisplayItem::Leaf { item: &item };
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Struct with trait implementations ----

    #[test]
    fn render_struct_with_trait_impls() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let struct_item = make_item_full(
            "Foo",
            "mycrate::Foo",
            ItemKind::Struct,
            "pub struct Foo",
            "A foo struct.",
            "A foo struct.",
        );
        index.add_item(struct_item);
        index.trait_impls.insert(
            0,
            vec![
                TraitImplInfo {
                    trait_path: "Debug".to_string(),
                    is_synthetic: false,
                },
                TraitImplInfo {
                    trait_path: "Clone".to_string(),
                    is_synthetic: false,
                },
                TraitImplInfo {
                    trait_path: "Send".to_string(),
                    is_synthetic: true,
                },
                TraitImplInfo {
                    trait_path: "Sync".to_string(),
                    is_synthetic: true,
                },
            ],
        );

        let di = crate::render::build_display_item(&index, 0, false);
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }

    // ---- Feature gate on header line ----

    #[test]
    fn render_feature_gate_on_leaf_header() {
        let mut item = make_item_full(
            "read",
            "mycrate::fs::read",
            ItemKind::Function,
            "pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>>",
            "Reads the entire contents of a file.",
            "Reads the entire contents of a file.",
        );
        item.feature_gate = Some("fs".to_string());
        let di = DisplayItem::Leaf { item: &item };
        let limits = DisplayLimits::default();
        let output = render_text(&di, &limits);

        insta::assert_snapshot!(output);
    }
}
