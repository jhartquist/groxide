use crate::types::{IndexItem, ItemKind, SourceSpan};

/// Builds a test `IndexItem` with the given name, path, and kind.
///
/// All other fields are set to empty/default values.
pub(crate) fn make_item(name: &str, path: &str, kind: ItemKind) -> IndexItem {
    IndexItem {
        path: path.to_string(),
        name: name.to_string(),
        kind,
        signature: String::new(),
        docs: String::new(),
        summary: String::new(),
        span: make_source_span(),
        children: Vec::new(),
        is_public: true,
        has_body: false,
        feature_gate: None,
        reexport_source: None,
    }
}

/// Builds a test `IndexItem` with all text fields populated.
pub(crate) fn make_item_full(
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
        reexport_source: None,
    }
}

/// Creates an empty `SourceSpan` for test use.
pub(crate) fn make_source_span() -> SourceSpan {
    SourceSpan {
        file: String::new(),
        line_start: 0,
        line_end: 0,
    }
}

/// Builds a test `IndexItem` representing a re-export stub
/// (`pub use source::Item as Item`). Sets `reexport_source`, sets a
/// matching `pub use` signature, and leaves `children` empty so the item
/// passes `query::is_reexport_stub`.
pub(crate) fn make_reexport_stub(
    name: &str,
    path: &str,
    kind: ItemKind,
    source: &str,
) -> IndexItem {
    let mut item = make_item(name, path, kind);
    item.reexport_source = Some(source.to_string());
    item.signature = format!("pub use {source} as {name}");
    item
}
