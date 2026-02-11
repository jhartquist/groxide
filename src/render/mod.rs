pub(crate) mod text;

use crate::types::{
    group_items, DisplayItem, DisplayLimits, DocIndex, GroupedItems, IndexItem, ItemKind,
};

/// Builds a `DisplayItem` from a `DocIndex` and an item index.
///
/// Determines the appropriate variant based on the item's kind and whether
/// it is the crate root.
pub(crate) fn build_display_item(
    index: &DocIndex,
    item_index: usize,
    include_private: bool,
) -> DisplayItem<'_> {
    let item = index.get(item_index);

    match item.kind {
        ItemKind::Module => {
            let is_crate_root = item.path == index.crate_name;
            let children = collect_children(index, item, include_private);
            if is_crate_root {
                DisplayItem::Crate { item, children }
            } else {
                DisplayItem::Module { item, children }
            }
        }
        ItemKind::Struct | ItemKind::Enum | ItemKind::Union => {
            let (methods, variants) = collect_type_children(index, item);
            let trait_impls = index.item_trait_impls(item_index);
            DisplayItem::Type {
                item,
                methods,
                variants,
                trait_impls,
            }
        }
        ItemKind::Trait | ItemKind::TraitAlias => {
            let (required, provided) = collect_trait_methods(index, item);
            DisplayItem::Trait {
                item,
                required_methods: required,
                provided_methods: provided,
            }
        }
        ItemKind::Function
        | ItemKind::TypeAlias
        | ItemKind::AssocType
        | ItemKind::ForeignType
        | ItemKind::Constant
        | ItemKind::AssocConst
        | ItemKind::Static
        | ItemKind::Macro
        | ItemKind::ProcMacro
        | ItemKind::Variant
        | ItemKind::Field
        | ItemKind::Primitive => DisplayItem::Leaf { item },
    }
}

/// Collects public children of a module/crate item, grouped by category.
fn collect_children<'a>(
    index: &'a DocIndex,
    item: &'a IndexItem,
    include_private: bool,
) -> GroupedItems<'a> {
    let children: Vec<&IndexItem> = item
        .children
        .iter()
        .map(|c| index.get(c.index))
        .filter(|c| include_private || c.is_public)
        .collect();
    group_items(&children)
}

/// Collects methods and variants for a type item (struct/enum/union).
fn collect_type_children<'a>(
    index: &'a DocIndex,
    item: &'a IndexItem,
) -> (Vec<&'a IndexItem>, Vec<&'a IndexItem>) {
    let mut methods = Vec::new();
    let mut variants = Vec::new();
    for child_ref in &item.children {
        let child = index.get(child_ref.index);
        if child.kind == ItemKind::Variant {
            variants.push(child);
        } else if child.kind == ItemKind::Function {
            methods.push(child);
        }
    }
    methods.sort_by(|a, b| a.name.cmp(&b.name));
    variants.sort_by(|a, b| a.name.cmp(&b.name));
    (methods, variants)
}

/// Collects required and provided methods for a trait item.
fn collect_trait_methods<'a>(
    index: &'a DocIndex,
    item: &'a IndexItem,
) -> (Vec<&'a IndexItem>, Vec<&'a IndexItem>) {
    let mut required = Vec::new();
    let mut provided = Vec::new();
    for child_ref in &item.children {
        let child = index.get(child_ref.index);
        if child.kind == ItemKind::Function {
            if child.has_body {
                provided.push(child);
            } else {
                required.push(child);
            }
        }
    }
    required.sort_by(|a, b| a.name.cmp(&b.name));
    provided.sort_by(|a, b| a.name.cmp(&b.name));
    (required, provided)
}

/// Strips markdown formatting from doc text for plain text display.
///
/// Handles: headings, bold, italic, inline code, links, code fences,
/// blockquotes, list markers, and horizontal rules.
pub(crate) fn strip_markdown(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_code_fence = false;
    let lines = input.lines();

    for line in lines {
        if in_code_fence {
            if line.trim_start().starts_with("```") {
                in_code_fence = false;
                // Blank line after code block is handled by the next line processing
            } else {
                // Indent code block content by 2 spaces
                output.push_str("  ");
                output.push_str(line);
                output.push('\n');
            }
            continue;
        }

        // Check for code fence opening
        if line.trim_start().starts_with("```") {
            in_code_fence = true;
            // Ensure blank line before code block if output doesn't already end with one
            if !output.is_empty() && !output.ends_with("\n\n") {
                if output.ends_with('\n') {
                    output.push('\n');
                } else {
                    output.push_str("\n\n");
                }
            }
            continue;
        }

        // Horizontal rules
        let trimmed = line.trim();
        if is_horizontal_rule(trimmed) {
            // Output a blank line
            if !output.is_empty() && !output.ends_with("\n\n") {
                if output.ends_with('\n') {
                    output.push('\n');
                } else {
                    output.push_str("\n\n");
                }
            }
            continue;
        }

        // Headings
        if let Some(heading_text) = strip_heading(trimmed) {
            output.push_str(heading_text);
            output.push('\n');
            continue;
        }

        // Blockquotes
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let stripped = strip_inline_markdown(rest);
            output.push_str(&stripped);
            output.push('\n');
            continue;
        }
        if trimmed == ">" {
            output.push('\n');
            continue;
        }

        // List markers (unordered)
        if let Some(stripped_line) = strip_list_marker(line) {
            let stripped = strip_inline_markdown(stripped_line);
            output.push_str(&stripped);
            output.push('\n');
            continue;
        }

        // Numbered list markers
        if let Some(stripped_line) = strip_numbered_list_marker(line) {
            let stripped = strip_inline_markdown(stripped_line);
            output.push_str(&stripped);
            output.push('\n');
            continue;
        }

        // Regular line — strip inline markdown
        let stripped = strip_inline_markdown(line);
        output.push_str(&stripped);
        output.push('\n');
    }

    // Remove trailing newline(s) to match expectations
    while output.ends_with('\n') {
        output.pop();
    }

    output
}

/// Returns true if the line is a horizontal rule (---, ***, ___).
fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 3 {
        return false;
    }
    let first = chars[0];
    (first == '-' || first == '*' || first == '_') && chars.iter().all(|&c| c == first)
}

/// Strips heading markers from a line, returning the heading text.
fn strip_heading(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('#') {
        return None;
    }
    let without_hashes = trimmed.trim_start_matches('#');
    if without_hashes.is_empty() || without_hashes.starts_with(' ') {
        Some(without_hashes.trim_start())
    } else {
        None
    }
}

/// Strips unordered list markers (-, *, +) from a line, preserving indent level.
fn strip_list_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        // Ensure it's not a bold/emphasis marker (** or *word*)
        return Some(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("+ ") {
        return Some(rest);
    }
    None
}

/// Strips numbered list markers (1., 2., etc.) from a line.
fn strip_numbered_list_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let rest_str = chars.as_str();
    // Find the `. ` after digits
    for (i, ch) in rest_str.char_indices() {
        if ch == '.' {
            let after_dot = &rest_str[i + 1..];
            if after_dot.starts_with(' ') {
                return Some(after_dot.trim_start());
            }
            return None;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
    }
    None
}

/// Strips inline markdown formatting from a single line.
///
/// Handles: bold, italic, inline code, links, and reference links.
pub(crate) fn strip_inline_markdown(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            // Bold: ** or __
            b'*' if i + 1 < len && bytes[i + 1] == b'*' => {
                // Find closing **
                if let Some(end) = find_closing_marker(&bytes[i + 2..], *b"**") {
                    let inner = &line[i + 2..i + 2 + end];
                    result.push_str(&strip_inline_markdown(inner));
                    i = i + 2 + end + 2;
                } else {
                    result.push('*');
                    i += 1;
                }
            }
            b'_' if i + 1 < len && bytes[i + 1] == b'_' => {
                if let Some(end) = find_closing_marker(&bytes[i + 2..], *b"__") {
                    let inner = &line[i + 2..i + 2 + end];
                    result.push_str(&strip_inline_markdown(inner));
                    i = i + 2 + end + 2;
                } else {
                    result.push('_');
                    i += 1;
                }
            }
            // Italic: single * or _
            b'*' => {
                if let Some(end) = find_closing_single(&bytes[i + 1..], b'*') {
                    let inner = &line[i + 1..i + 1 + end];
                    result.push_str(&strip_inline_markdown(inner));
                    i = i + 1 + end + 1;
                } else {
                    result.push('*');
                    i += 1;
                }
            }
            b'_' => {
                if let Some(end) = find_closing_single(&bytes[i + 1..], b'_') {
                    let inner = &line[i + 1..i + 1 + end];
                    result.push_str(&strip_inline_markdown(inner));
                    i = i + 1 + end + 1;
                } else {
                    result.push('_');
                    i += 1;
                }
            }
            // Inline code
            b'`' => {
                if let Some(end) = find_closing_single(&bytes[i + 1..], b'`') {
                    let inner = &line[i + 1..i + 1 + end];
                    result.push_str(inner);
                    i = i + 1 + end + 1;
                } else {
                    result.push('`');
                    i += 1;
                }
            }
            // Links: [text](url) or [text][ref]
            b'[' => {
                if let Some((text, skip)) = parse_link(&line[i..]) {
                    result.push_str(text);
                    i += skip;
                } else {
                    result.push('[');
                    i += 1;
                }
            }
            _ => {
                result.push(line[i..].chars().next().expect("invariant: valid index"));
                i += line[i..]
                    .chars()
                    .next()
                    .expect("invariant: valid index")
                    .len_utf8();
            }
        }
    }

    result
}

/// Finds the position of a 2-byte closing marker in a byte slice.
fn find_closing_marker(bytes: &[u8], marker: [u8; 2]) -> Option<usize> {
    (0..bytes.len().saturating_sub(1)).find(|&i| bytes[i] == marker[0] && bytes[i + 1] == marker[1])
}

/// Finds the position of a single-byte closing marker.
fn find_closing_single(bytes: &[u8], marker: u8) -> Option<usize> {
    for (i, &b) in bytes.iter().enumerate() {
        if b == marker {
            return Some(i);
        }
    }
    None
}

/// Parses a markdown link at the current position, returning (text, bytes consumed).
fn parse_link(s: &str) -> Option<(&str, usize)> {
    if !s.starts_with('[') {
        return None;
    }

    // Find closing ]
    let close_bracket = s[1..].find(']')? + 1;
    let text = &s[1..close_bracket];
    let after = &s[close_bracket + 1..];

    // [text](url)
    if after.starts_with('(') {
        let close_paren = after.find(')')?;
        return Some((text, close_bracket + 1 + close_paren + 1));
    }

    // [text][ref]
    if let Some(stripped) = after.strip_prefix('[') {
        let close_bracket2 = stripped.find(']')? + 1;
        return Some((text, close_bracket + 1 + close_bracket2 + 1));
    }

    None
}

/// Truncates doc text according to the spec's priority chain.
///
/// Priority: paragraph boundary -> sentence boundary -> word boundary -> hard truncate.
/// All truncation is UTF-8 safe.
pub(crate) fn truncate_doc(text: &str, limits: &DisplayLimits) -> String {
    if limits.expand_all {
        return text.to_string();
    }

    let max_len = limits.max_doc_length;
    if text.len() <= max_len {
        return text.to_string();
    }

    let safe_max = safe_truncate_point(text, max_len);
    let search_region = &text[..safe_max];

    // 1. Paragraph boundary: last \n\n before limit
    if let Some(pos) = search_region.rfind("\n\n") {
        if pos > 0 {
            let truncated = text[..pos].trim_end();
            return format!("{truncated}...");
        }
    }

    // 2. Sentence boundary: last `. `, `! `, or `? ` before limit
    if let Some(pos) = find_last_sentence_boundary(search_region) {
        // Break after the punctuation (include the punctuation mark)
        let truncated = &text[..=pos];
        return truncated.to_string();
    }

    // 3. Word boundary: last space before limit
    if let Some(pos) = search_region.rfind(' ') {
        if pos > 0 {
            let truncated = text[..pos].trim_end();
            return format!("{truncated}...");
        }
    }

    // 4. Hard truncate at safe UTF-8 boundary
    let truncated = &text[..safe_max];
    format!("{truncated}...")
}

/// Finds the byte position of the last sentence-ending punctuation before a space.
fn find_last_sentence_boundary(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut last_pos = None;
    for i in 0..bytes.len().saturating_sub(1) {
        if (bytes[i] == b'.' || bytes[i] == b'!' || bytes[i] == b'?') && bytes[i + 1] == b' ' {
            last_pos = Some(i);
        }
    }
    last_pos
}

/// Finds a safe UTF-8 truncation point at or before `max_bytes`.
fn safe_truncate_point(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut pos = max_bytes;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Renders the feature gate annotation suffix.
///
/// Returns `"  [feature: <gate>]"` if a gate exists, empty string otherwise.
pub(crate) fn feature_gate_suffix(feature_gate: Option<&String>) -> String {
    match feature_gate {
        Some(gate) => format!("  [feature: {gate}]"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChildRef, SourceSpan};

    fn make_item(name: &str, path: &str, kind: ItemKind) -> IndexItem {
        IndexItem {
            path: path.to_string(),
            name: name.to_string(),
            kind,
            signature: String::new(),
            docs: String::new(),
            summary: String::new(),
            span: SourceSpan {
                file: String::new(),
                line_start: 0,
                line_end: 0,
            },
            children: Vec::new(),
            is_public: true,
            has_body: false,
            feature_gate: None,
        }
    }

    // ---- build_display_item ----

    #[test]
    fn build_display_item_returns_crate_when_path_equals_crate_name() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("mycrate", "mycrate", ItemKind::Module));
        let di = build_display_item(&index, 0, false);
        assert!(matches!(di, DisplayItem::Crate { .. }));
    }

    #[test]
    fn build_display_item_returns_module_when_not_crate_root() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("sub", "mycrate::sub", ItemKind::Module));
        let di = build_display_item(&index, 0, false);
        assert!(matches!(di, DisplayItem::Module { .. }));
    }

    #[test]
    fn build_display_item_returns_type_for_struct() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));
        let di = build_display_item(&index, 0, false);
        assert!(matches!(di, DisplayItem::Type { .. }));
    }

    #[test]
    fn build_display_item_returns_trait_for_trait() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("MyTrait", "mycrate::MyTrait", ItemKind::Trait));
        let di = build_display_item(&index, 0, false);
        assert!(matches!(di, DisplayItem::Trait { .. }));
    }

    #[test]
    fn build_display_item_returns_leaf_for_function() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("foo", "mycrate::foo", ItemKind::Function));
        let di = build_display_item(&index, 0, false);
        assert!(matches!(di, DisplayItem::Leaf { .. }));
    }

    #[test]
    fn build_display_item_collects_methods_and_variants() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut enum_item = make_item("Color", "mycrate::Color", ItemKind::Enum);
        let variant = make_item("Red", "mycrate::Color::Red", ItemKind::Variant);
        let method = make_item("name", "mycrate::Color::name", ItemKind::Function);
        index.add_item(variant);
        index.add_item(method);
        enum_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Variant,
                name: "Red".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "name".to_string(),
            },
        ];
        index.add_item(enum_item);

        let di = build_display_item(&index, 2, false);
        match di {
            DisplayItem::Type {
                methods, variants, ..
            } => {
                assert_eq!(methods.len(), 1);
                assert_eq!(variants.len(), 1);
            }
            _ => panic!("expected Type variant"),
        }
    }

    #[test]
    fn build_display_item_splits_required_and_provided_methods() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let mut required = make_item("poll", "mycrate::MyTrait::poll", ItemKind::Function);
        required.has_body = false;
        let mut provided = make_item("chain", "mycrate::MyTrait::chain", ItemKind::Function);
        provided.has_body = true;
        index.add_item(required);
        index.add_item(provided);
        let mut trait_item = make_item("MyTrait", "mycrate::MyTrait", ItemKind::Trait);
        trait_item.children = vec![
            ChildRef {
                index: 0,
                kind: ItemKind::Function,
                name: "poll".to_string(),
            },
            ChildRef {
                index: 1,
                kind: ItemKind::Function,
                name: "chain".to_string(),
            },
        ];
        index.add_item(trait_item);

        let di = build_display_item(&index, 2, false);
        match di {
            DisplayItem::Trait {
                required_methods,
                provided_methods,
                ..
            } => {
                assert_eq!(required_methods.len(), 1);
                assert_eq!(required_methods[0].name, "poll");
                assert_eq!(provided_methods.len(), 1);
                assert_eq!(provided_methods[0].name, "chain");
            }
            _ => panic!("expected Trait variant"),
        }
    }

    // ---- strip_markdown ----

    #[test]
    fn strip_markdown_removes_bold() {
        assert_eq!(strip_markdown("**bold**"), "bold");
        assert_eq!(strip_markdown("__bold__"), "bold");
    }

    #[test]
    fn strip_markdown_removes_italic() {
        assert_eq!(strip_markdown("*italic*"), "italic");
        assert_eq!(strip_markdown("_italic_"), "italic");
    }

    #[test]
    fn strip_markdown_removes_inline_code() {
        assert_eq!(strip_markdown("`code`"), "code");
    }

    #[test]
    fn strip_markdown_removes_links() {
        assert_eq!(strip_markdown("[link text](url)"), "link text");
        assert_eq!(strip_markdown("[link text][ref]"), "link text");
    }

    #[test]
    fn strip_markdown_removes_headings() {
        assert_eq!(strip_markdown("# Heading"), "Heading");
        assert_eq!(strip_markdown("## Sub Heading"), "Sub Heading");
        assert_eq!(strip_markdown("### Third"), "Third");
    }

    #[test]
    fn strip_markdown_handles_code_fences() {
        let input = "Here is an example:\n\n```rust\nlet x = 42;\nlet y = x + 1;\n```";
        let output = strip_markdown(input);
        assert!(output.contains("  let x = 42;"));
        assert!(output.contains("  let y = x + 1;"));
        assert!(!output.contains("```"));
    }

    #[test]
    fn strip_markdown_handles_blockquotes() {
        assert_eq!(strip_markdown("> quoted text"), "quoted text");
    }

    #[test]
    fn strip_markdown_handles_list_markers() {
        assert_eq!(strip_markdown("- bullet"), "bullet");
        assert_eq!(strip_markdown("* bullet"), "bullet");
        assert_eq!(strip_markdown("+ bullet"), "bullet");
    }

    #[test]
    fn strip_markdown_handles_numbered_lists() {
        assert_eq!(strip_markdown("1. item"), "item");
        assert_eq!(strip_markdown("42. item"), "item");
    }

    #[test]
    fn strip_markdown_handles_horizontal_rules() {
        let input = "before\n\n---\n\nafter";
        let output = strip_markdown(input);
        assert!(output.contains("before"));
        assert!(output.contains("after"));
        assert!(!output.contains("---"));
    }

    #[test]
    fn strip_markdown_combined() {
        let input = "# Title\n\nSome **bold** and *italic* text with `code`.\n\n[link](url) here.";
        let expected = "Title\n\nSome bold and italic text with code.\n\nlink here.";
        assert_eq!(strip_markdown(input), expected);
    }

    // ---- truncate_doc ----

    #[test]
    fn truncate_doc_no_truncation_when_under_limit() {
        let limits = DisplayLimits::default();
        let text = "Short text.";
        assert_eq!(truncate_doc(text, &limits), "Short text.");
    }

    #[test]
    fn truncate_doc_respects_expand_all() {
        let limits = DisplayLimits {
            expand_all: true,
            ..DisplayLimits::default()
        };
        let text = "a".repeat(2000);
        assert_eq!(truncate_doc(&text, &limits), text);
    }

    #[test]
    fn truncate_doc_at_paragraph_boundary() {
        let limits = DisplayLimits {
            max_doc_length: 50,
            ..DisplayLimits::default()
        };
        let text = "First paragraph here.\n\nSecond paragraph that goes over the limit by a lot.";
        let result = truncate_doc(text, &limits);
        assert!(result.ends_with("..."), "result: {result}");
        assert!(result.contains("First paragraph here."));
    }

    #[test]
    fn truncate_doc_at_sentence_boundary() {
        let limits = DisplayLimits {
            max_doc_length: 40,
            ..DisplayLimits::default()
        };
        let text = "First sentence. Second sentence goes on and on past the limit here.";
        let result = truncate_doc(text, &limits);
        assert_eq!(result, "First sentence.");
    }

    #[test]
    fn truncate_doc_at_word_boundary() {
        let limits = DisplayLimits {
            max_doc_length: 20,
            ..DisplayLimits::default()
        };
        let text = "Oneword anotherword yetanother";
        let result = truncate_doc(text, &limits);
        assert!(result.ends_with("..."), "result: {result}");
        assert!(!result.contains("yetanother"));
    }

    #[test]
    fn truncate_doc_hard_truncate() {
        let limits = DisplayLimits {
            max_doc_length: 10,
            ..DisplayLimits::default()
        };
        let text = "abcdefghijklmnop";
        let result = truncate_doc(text, &limits);
        assert!(result.ends_with("..."), "result: {result}");
        assert!(result.len() <= 13 + 3); // 10 chars + "..."
    }

    #[test]
    fn truncate_doc_respects_utf8_boundaries() {
        let limits = DisplayLimits {
            max_doc_length: 5,
            ..DisplayLimits::default()
        };
        // Each CJK char is 3 bytes. 5 bytes can hold 1 CJK char (3 bytes) but not 2.
        let text = "日本語テスト";
        let result = truncate_doc(text, &limits);
        // Should not panic and should end with ...
        assert!(result.ends_with("..."), "result: {result}");
        // Verify it's valid UTF-8 (the fact it compiled means it is, but let's be explicit)
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_doc_at_1500_chars() {
        let limits = DisplayLimits::default();
        // Create text that exceeds 1500 chars
        let paragraph1 = "x".repeat(1000);
        let paragraph2 = "y".repeat(600);
        let text = format!("{paragraph1}\n\n{paragraph2}");
        let result = truncate_doc(&text, &limits);
        assert!(result.ends_with("..."), "result should end with ...");
        // Should truncate at the paragraph boundary
        assert!(result.len() < text.len());
    }

    // ---- feature_gate_suffix ----

    #[test]
    fn feature_gate_suffix_none_returns_empty() {
        assert_eq!(feature_gate_suffix(None), "");
    }

    #[test]
    fn feature_gate_suffix_some_returns_annotation() {
        let gate = "fs".to_string();
        assert_eq!(feature_gate_suffix(Some(&gate)), "  [feature: fs]");
    }
}
