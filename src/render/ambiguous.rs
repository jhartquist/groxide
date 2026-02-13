use std::fmt::Write;

use crate::types::{DocIndex, IndexItem, ItemKind, TraitImplInfo};

use super::{feature_gate_suffix, strip_markdown, trim_trailing_newlines};

/// Maximum items shown in condensed (6+) mode.
const MAX_SHOWN: usize = 10;

/// Renders ambiguous matches for default text output.
///
/// Selects the appropriate format based on count and item kinds.
pub(crate) fn render_ambiguous(index: &DocIndex, indices: &[usize], query: &str) -> String {
    let items: Vec<&IndexItem> = indices.iter().map(|&i| index.get(i)).collect();
    let count = items.len();

    if count == 0 {
        return String::new();
    }

    // Check for macro/trait special case: exactly 2 matches, one trait, one macro
    if count == 2 {
        let kinds: Vec<ItemKind> = items.iter().map(|i| i.kind).collect();
        let has_trait = kinds
            .iter()
            .any(|k| matches!(k, ItemKind::Trait | ItemKind::TraitAlias));
        let has_macro = kinds
            .iter()
            .any(|k| matches!(k, ItemKind::Macro | ItemKind::ProcMacro));
        if has_trait && has_macro {
            return render_macro_trait_ambiguity(&items, query);
        }
    }

    if count <= 5 {
        render_few_matches(&items, query)
    } else {
        render_many_matches(&items, query)
    }
}

/// Renders ambiguous matches in list mode (`--list` with ambiguous).
///
/// One path per line, no formatting.
pub(crate) fn render_ambiguous_list(index: &DocIndex, indices: &[usize]) -> String {
    let mut out = String::new();
    for &idx in indices {
        let item = index.get(idx);
        let _ = writeln!(out, "{}", item.path);
    }
    trim_trailing_newlines(&mut out);
    out
}

/// Renders the impls view (`--impls`) for a type (struct/enum/union).
///
/// Shows all trait implementations with no truncation.
pub(crate) fn render_impls_type(item: &IndexItem, trait_impls: &[TraitImplInfo]) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "{} {}{gate}", item.kind.short_name(), item.path);

    // Signature
    out.push('\n');
    out.push_str(&item.signature);
    out.push('\n');

    if trait_impls.is_empty() {
        out.push('\n');
        out.push_str("No trait implementations.");
    } else {
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
            if ti.is_synthetic {
                let _ = writeln!(out, "  impl {} (synthetic)", ti.trait_path);
            } else {
                let _ = writeln!(out, "  impl {}", ti.trait_path);
            }
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders the impls view (`--impls`) for a trait.
///
/// Shows implementors.
pub(crate) fn render_impls_trait(item: &IndexItem, implementors: &[String]) -> String {
    let mut out = String::new();

    // Header
    let gate = feature_gate_suffix(item.feature_gate.as_ref());
    let _ = writeln!(out, "trait {}{gate}", item.path);

    // Signature
    out.push('\n');
    out.push_str(&item.signature);
    out.push('\n');

    if implementors.is_empty() {
        out.push('\n');
        out.push_str("No known implementors.");
    } else {
        out.push('\n');
        out.push_str("Implementors:\n");
        for imp in implementors {
            let _ = writeln!(out, "  {imp}");
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders the impls view for non-type/trait items.
pub(crate) fn render_impls_other(item: &IndexItem) -> String {
    format!(
        "{} {} has no trait implementations.",
        item.kind.short_name(),
        item.path
    )
}

/// Renders the source view (`--source`) for a single item.
pub(crate) fn render_source(item: &IndexItem, source_content: Option<&str>) -> String {
    let span = &item.span;

    // Check if source is available
    if span.file.is_empty() || (span.line_start == 0 && span.line_end == 0) {
        return "// source not available (macro-generated or built-in)".to_string();
    }

    match source_content {
        Some(content) => {
            let mut out = String::new();
            if span.line_start == span.line_end {
                let _ = writeln!(out, "// {}:{}", span.file, span.line_start);
            } else {
                let _ = writeln!(
                    out,
                    "// {}:{}-{}",
                    span.file, span.line_start, span.line_end
                );
            }
            out.push_str(content);
            trim_trailing_newlines(&mut out);
            out
        }
        None => {
            format!("// source not available (Could not read {})", span.file)
        }
    }
}

/// Renders the source view for multiple ambiguous matches.
pub(crate) fn render_source_ambiguous(items_with_source: &[(&IndexItem, Option<&str>)]) -> String {
    let mut out = String::new();
    for (i, (item, source)) in items_with_source.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = writeln!(out, "--- {} ---", item.path);
        out.push_str(&render_source(item, *source));
        out.push('\n');
    }
    trim_trailing_newlines(&mut out);
    out
}

// ---- Private helpers ----

/// Renders the macro/trait special format for exactly 2 ambiguous matches.
fn render_macro_trait_ambiguity(items: &[&IndexItem], query: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\"{query}\" matches 2 items:");
    out.push('\n');
    for item in items {
        let kind = item.kind.short_name();
        let name = &item.name;
        let summary = &item.summary;
        if summary.is_empty() {
            let _ = writeln!(out, "  {kind:<7}  {name:<20}");
        } else {
            let _ = writeln!(out, "  {kind:<7}  {name:<20}  {summary}");
        }
    }
    out.push('\n');
    out.push_str("Use --kind trait or --kind macro to select.");
    out
}

/// Renders the few matches (2-5) detailed format.
fn render_few_matches(items: &[&IndexItem], query: &str) -> String {
    let mut out = String::new();
    let count = items.len();
    let _ = writeln!(out, "Found {count} items matching \"{query}\":");

    // Classify items as primary (2-4 path segments, not a variant) or deeply nested
    let mut primary_items: Vec<&IndexItem> = Vec::new();
    let mut nested_items: Vec<&IndexItem> = Vec::new();

    for &item in items {
        let segment_count = item.path.split("::").count();
        if segment_count >= 5 || item.kind == ItemKind::Variant {
            nested_items.push(item);
        } else {
            primary_items.push(item);
        }
    }

    // Render primary items with full detail
    for item in &primary_items {
        out.push('\n');
        let _ = writeln!(out, "--- {} ---", item.path);
        if !item.signature.is_empty() {
            let _ = writeln!(out, "{}", item.signature);
        }
        let first_para = extract_first_paragraph(&item.docs);
        if !first_para.is_empty() {
            let stripped = strip_markdown(&first_para);
            let _ = writeln!(out, "{stripped}");
        }
    }

    // Render deeply nested items in compact format
    if !nested_items.is_empty() && !primary_items.is_empty() {
        out.push('\n');
        out.push_str("Also found in nested contexts:\n");
        for item in &nested_items {
            let kind = item.kind.short_name();
            let path = &item.path;
            let summary = &item.summary;
            if summary.is_empty() {
                let _ = writeln!(out, "  {kind:<7}  {path:<38}");
            } else {
                let _ = writeln!(out, "  {kind:<7}  {path:<38}  {summary}");
            }
        }
    } else if nested_items.is_empty() {
        // All items are primary — already rendered above
    } else {
        // All items are deeply nested — render as compact list
        for item in &nested_items {
            out.push('\n');
            let _ = writeln!(out, "--- {} ---", item.path);
            if !item.signature.is_empty() {
                let _ = writeln!(out, "{}", item.signature);
            }
            let first_para = extract_first_paragraph(&item.docs);
            if !first_para.is_empty() {
                let stripped = strip_markdown(&first_para);
                let _ = writeln!(out, "{stripped}");
            }
        }
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Renders the many matches (6+) condensed format.
fn render_many_matches(items: &[&IndexItem], query: &str) -> String {
    let mut out = String::new();
    let total = items.len();
    let _ = writeln!(out, "Found {total} items matching \"{query}\":");
    out.push('\n');

    let shown = total.min(MAX_SHOWN);
    for item in items.iter().take(shown) {
        let kind = item.kind.short_name();
        let path = &item.path;
        let summary = &item.summary;
        if summary.is_empty() {
            let _ = writeln!(out, "{kind:<7}  {path:<38}");
        } else {
            let _ = writeln!(out, "{kind:<7}  {path:<38}  {summary}");
        }
    }

    if total > MAX_SHOWN {
        let remaining = total - MAX_SHOWN;
        let _ = write!(out, "... ({remaining} more, use a more specific path)");
    }

    trim_trailing_newlines(&mut out);
    out
}

/// Extracts the first paragraph from doc text.
///
/// Priority: first blank line, then first 3 sentences, then first ~200 chars.
fn extract_first_paragraph(docs: &str) -> String {
    if docs.is_empty() {
        return String::new();
    }

    // Find first blank line
    if let Some(pos) = docs.find("\n\n") {
        return docs[..pos].to_string();
    }

    // Extract first 3 sentences
    let mut sentence_count = 0;
    let bytes = docs.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        if (bytes[i] == b'.' || bytes[i] == b'!' || bytes[i] == b'?') && bytes[i + 1] == b' ' {
            sentence_count += 1;
            if sentence_count >= 3 {
                return docs[..=i].to_string();
            }
        }
    }
    // Check last char for sentence end
    if let Some(last) = bytes.last() {
        if *last == b'.' || *last == b'!' || *last == b'?' {
            sentence_count += 1;
            if sentence_count <= 3 {
                return docs.to_string();
            }
        }
    }

    // Truncate at ~200 chars at word boundary
    if docs.len() <= 200 {
        return docs.to_string();
    }

    // Find first newline if <= 200 chars
    if let Some(nl) = docs.find('\n') {
        if nl <= 200 {
            return docs[..nl].to_string();
        }
    }

    let search = &docs[..200];
    if let Some(pos) = search.rfind(' ') {
        format!("{}...", &docs[..pos])
    } else {
        format!("{}...", &docs[..200])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item_full;
    use crate::types::{DocIndex, IndexItem, ItemKind, SourceSpan, TraitImplInfo};

    fn with_span(mut item: IndexItem, file: &str, line_start: u32, line_end: u32) -> IndexItem {
        item.span = SourceSpan {
            file: file.to_string(),
            line_start,
            line_end,
        };
        item
    }

    // ---- Ambiguous display: 2 matches (brief/few) ----

    #[test]
    fn render_ambiguous_two_matches_brief() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let item1 = make_item_full(
            "Error",
            "mycrate::de::Error",
            ItemKind::Trait,
            "pub trait Error: Sized",
            "When a Deserialize implementation encounters an error.",
            "When a Deserialize implementation encounters an error.",
        );
        let item2 = make_item_full(
            "Error",
            "mycrate::ser::Error",
            ItemKind::Trait,
            "pub trait Error: Sized",
            "When a Serialize implementation encounters an error.",
            "When a Serialize implementation encounters an error.",
        );

        index.add_item(item1);
        index.add_item(item2);

        let output = render_ambiguous(&index, &[0, 1], "Error");
        insta::assert_snapshot!(output);
    }

    // ---- Ambiguous display: 6 matches (condensed) ----

    #[test]
    fn render_ambiguous_six_matches_condensed() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let items = [
            (
                "Error",
                "mycrate::de::Error",
                ItemKind::Struct,
                "When deserialization encounters an error.",
            ),
            (
                "Error",
                "mycrate::ser::Error",
                ItemKind::Struct,
                "When serialization encounters an error.",
            ),
            (
                "Error",
                "mycrate::de::value::Error",
                ItemKind::Struct,
                "A minimal deserialization error.",
            ),
            (
                "Error",
                "mycrate::json::Error",
                ItemKind::Struct,
                "An error that occurred during parsing.",
            ),
            (
                "classify",
                "mycrate::json::Error::classify",
                ItemKind::Function,
                "Categorizes the cause of this error.",
            ),
            (
                "column",
                "mycrate::json::Error::column",
                ItemKind::Function,
                "One-indexed column number of the error.",
            ),
            (
                "line",
                "mycrate::json::Error::line",
                ItemKind::Function,
                "One-indexed line number of the error.",
            ),
        ];

        for (name, path, kind, summary) in &items {
            index.add_item(make_item_full(name, path, *kind, "", "", summary));
        }

        let indices: Vec<usize> = (0..7).collect();
        let output = render_ambiguous(&index, &indices, "Error");
        insta::assert_snapshot!(output);
    }

    // ---- Macro/trait disambiguation ----

    #[test]
    fn render_ambiguous_macro_trait_special() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let trait_item = make_item_full(
            "Serialize",
            "mycrate::Serialize",
            ItemKind::Trait,
            "pub trait Serialize",
            "A data structure that can be serialized.",
            "A data structure that can be serialized.",
        );
        let macro_item = make_item_full(
            "Serialize",
            "mycrate::Serialize",
            ItemKind::Macro,
            "#[derive(Serialize)]",
            "Derive macro for the Serialize trait.",
            "Derive macro for the Serialize trait.",
        );

        index.add_item(trait_item);
        index.add_item(macro_item);

        let output = render_ambiguous(&index, &[0, 1], "Serialize");
        insta::assert_snapshot!(output);
    }

    // ---- Impls display: non-synthetic first, then synthetic ----

    #[test]
    fn render_impls_type_non_synthetic_first_then_synthetic() {
        let item = make_item_full(
            "Mutex",
            "mycrate::Mutex",
            ItemKind::Struct,
            "pub struct Mutex<T: ?Sized>",
            "An asynchronous Mutex-like type.",
            "An asynchronous Mutex-like type.",
        );

        let trait_impls = vec![
            TraitImplInfo {
                trait_path: "Send".to_string(),
                is_synthetic: true,
            },
            TraitImplInfo {
                trait_path: "Sync".to_string(),
                is_synthetic: true,
            },
            TraitImplInfo {
                trait_path: "Debug".to_string(),
                is_synthetic: false,
            },
            TraitImplInfo {
                trait_path: "Clone".to_string(),
                is_synthetic: false,
            },
            TraitImplInfo {
                trait_path: "Default".to_string(),
                is_synthetic: false,
            },
        ];

        let output = render_impls_type(&item, &trait_impls);
        insta::assert_snapshot!(output);
    }

    // ---- Impls: no trait implementations ----

    #[test]
    fn render_impls_type_no_implementations() {
        let item = make_item_full(
            "Empty",
            "mycrate::Empty",
            ItemKind::Struct,
            "pub struct Empty",
            "",
            "",
        );

        let output = render_impls_type(&item, &[]);
        insta::assert_snapshot!(output);
    }

    // ---- Impls: trait with implementors ----

    #[test]
    fn render_impls_trait_with_implementors() {
        let item = make_item_full(
            "MyTrait",
            "mycrate::MyTrait",
            ItemKind::Trait,
            "pub trait MyTrait",
            "A trait.",
            "A trait.",
        );

        let implementors = vec![
            "mycrate::MyStruct".to_string(),
            "mycrate::OtherStruct".to_string(),
        ];

        let output = render_impls_trait(&item, &implementors);
        insta::assert_snapshot!(output);
    }

    // ---- Impls: trait with no implementors ----

    #[test]
    fn render_impls_trait_no_implementors() {
        let item = make_item_full(
            "MyTrait",
            "mycrate::MyTrait",
            ItemKind::Trait,
            "pub trait MyTrait",
            "A trait.",
            "A trait.",
        );

        let output = render_impls_trait(&item, &[]);
        insta::assert_snapshot!(output);
    }

    // ---- Impls: other item types ----

    #[test]
    fn render_impls_other_item() {
        let item = make_item_full(
            "utils",
            "mycrate::utils",
            ItemKind::Module,
            "",
            "Utility helpers.",
            "Utility helpers.",
        );

        let output = render_impls_other(&item);
        assert_eq!(output, "mod mycrate::utils has no trait implementations.");
    }

    // ---- Source view format with file header ----

    #[test]
    fn render_source_with_range() {
        let item = with_span(
            make_item_full(
                "Mutex",
                "mycrate::Mutex",
                ItemKind::Struct,
                "pub struct Mutex<T: ?Sized>",
                "",
                "",
            ),
            "src/sync/mutex.rs",
            42,
            147,
        );

        let source =
            "pub struct Mutex<T: ?Sized> {\n    s: semaphore::Semaphore,\n    c: UnsafeCell<T>,\n}";
        let output = render_source(&item, Some(source));
        insta::assert_snapshot!(output);
    }

    // ---- Source view: single line ----

    #[test]
    fn render_source_single_line() {
        let item = with_span(
            make_item_full(
                "MAX_SIZE",
                "mycrate::MAX_SIZE",
                ItemKind::Constant,
                "pub const MAX_SIZE: usize = 1024",
                "",
                "",
            ),
            "src/lib.rs",
            42,
            42,
        );

        let source = "pub const MAX_SIZE: usize = 1024;";
        let output = render_source(&item, Some(source));
        insta::assert_snapshot!(output);
    }

    // ---- Source unavailable message ----

    #[test]
    fn render_source_unavailable() {
        let item = make_item_full(
            "macro_generated",
            "mycrate::macro_generated",
            ItemKind::Function,
            "pub fn macro_generated()",
            "",
            "",
        );

        let output = render_source(&item, None);
        assert_eq!(
            output,
            "// source not available (macro-generated or built-in)"
        );
    }

    // ---- Source file not found ----

    #[test]
    fn render_source_file_not_found() {
        let item = with_span(
            make_item_full(
                "missing",
                "mycrate::missing",
                ItemKind::Function,
                "pub fn missing()",
                "",
                "",
            ),
            "src/foo.rs",
            1,
            10,
        );

        let output = render_source(&item, None);
        assert_eq!(
            output,
            "// source not available (Could not read src/foo.rs)"
        );
    }

    // ---- Ambiguous list mode ----

    #[test]
    fn render_ambiguous_list_mode() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        index.add_item(make_item_full(
            "Error",
            "mycrate::de::Error",
            ItemKind::Trait,
            "",
            "",
            "",
        ));
        index.add_item(make_item_full(
            "Error",
            "mycrate::ser::Error",
            ItemKind::Trait,
            "",
            "",
            "",
        ));
        index.add_item(make_item_full(
            "Error",
            "mycrate::json::Error",
            ItemKind::Struct,
            "",
            "",
            "",
        ));

        let output = render_ambiguous_list(&index, &[0, 1, 2]);
        insta::assert_snapshot!(output);
    }

    // ---- Source view with ambiguous matches ----

    #[test]
    fn render_source_ambiguous_format() {
        let item1 = with_span(
            make_item_full(
                "Mutex",
                "mycrate::sync::Mutex",
                ItemKind::Struct,
                "pub struct Mutex<T>",
                "",
                "",
            ),
            "src/sync/mutex.rs",
            42,
            100,
        );
        let item2 = with_span(
            make_item_full(
                "MutexGuard",
                "mycrate::sync::MutexGuard",
                ItemKind::Struct,
                "pub struct MutexGuard<'a, T>",
                "",
                "",
            ),
            "src/sync/mutex.rs",
            150,
            180,
        );

        let source1 = "pub struct Mutex<T> {\n    inner: T,\n}";
        let source2 = "pub struct MutexGuard<'a, T> {\n    lock: &'a Mutex<T>,\n}";

        let items_with_source: Vec<(&IndexItem, Option<&str>)> =
            vec![(&item1, Some(source1)), (&item2, Some(source2))];

        let output = render_source_ambiguous(&items_with_source);
        insta::assert_snapshot!(output);
    }

    // ---- First paragraph extraction ----

    #[test]
    fn extract_first_paragraph_blank_line() {
        let docs = "First paragraph.\n\nSecond paragraph.";
        assert_eq!(extract_first_paragraph(docs), "First paragraph.");
    }

    #[test]
    fn extract_first_paragraph_no_blank_line_short() {
        let docs = "A single short doc comment.";
        assert_eq!(extract_first_paragraph(docs), "A single short doc comment.");
    }

    #[test]
    fn extract_first_paragraph_long_text_truncated() {
        let long = "word ".repeat(100); // ~500 chars, no sentences
        let result = extract_first_paragraph(&long);
        assert!(
            result.len() <= 210,
            "result should be truncated: {}",
            result.len()
        );
        assert!(result.ends_with("..."));
    }
}
