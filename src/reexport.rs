use crate::cli::{CrateSpec, FeatureFlags, QueryPath};
use crate::types::{DocIndex, ItemKind, QueryResult};

/// Parses the source crate path from a re-export stub item.
///
/// Checks the `reexport_source` field first (populated at index build time),
/// then falls back to parsing `"pub use {source}"` from the signature for
/// old-cache compatibility.
pub(crate) fn parse_reexport_source(item: &crate::types::IndexItem) -> Option<String> {
    // Prefer the structured field
    if let Some(ref source) = item.reexport_source {
        return Some(source.clone());
    }
    // Fallback: parse from signature for old cached indices
    let rest = item.signature.strip_prefix("pub use ")?;
    let source = if let Some(pos) = rest.find(" as ") {
        &rest[..pos]
    } else {
        rest.trim_end_matches(';').trim()
    };
    if source.is_empty() {
        return None;
    }
    Some(source.to_string())
}

/// Follows a cross-crate re-export to the canonical item in the source crate.
///
/// Returns the source crate's `DocIndex` and the index of the canonical item,
/// or `None` if the re-export cannot be followed (e.g., source crate unavailable).
pub(crate) fn try_follow_reexport(
    stub: &crate::types::IndexItem,
    ctx: Option<&crate::resolve::ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Option<(DocIndex, usize)> {
    let source_path = parse_reexport_source(stub)?;

    // Split into crate name + item path on first `::`
    let (crate_name, item_path) = source_path.split_once("::")?;

    // Resolve source crate
    let query_path = QueryPath {
        crate_spec: CrateSpec::Named(crate_name.to_string()),
        item_segments: Vec::new(),
    };
    let (source, _) = crate::resolve_crate_source(ctx, query_path).ok()?;

    // Load source crate index
    let (source_index, _source) =
        crate::load_or_build_index(source, features, feature_suffix, private, false).ok()?;

    // Query canonical item in source index
    let source_query = QueryPath {
        crate_spec: CrateSpec::CurrentCrate,
        item_segments: item_path.split("::").map(String::from).collect(),
    };
    let result = crate::resolve_item(&source_query, &source_index, None);

    match result {
        QueryResult::Found { index: idx } => Some((source_index, idx)),
        QueryResult::Ambiguous { .. } | QueryResult::NotFound { .. } => None,
    }
}

/// Attempts to resolve a `NotFound` query by searching for re-export stubs
/// in the index whose item name matches the last segment of the query.
///
/// When a crate re-exports an item (e.g., `pub use dep::Item`), rustdoc may
/// index the stub under a path that doesn't exactly match the user's query.
/// This function finds such stubs and returns them as a `Found` or `Ambiguous`
/// result so the caller can follow the re-export chain.
pub(crate) fn try_resolve_reexport_on_not_found(
    query: &QueryPath,
    index: &DocIndex,
    kind_filter: Option<ItemKind>,
) -> Option<QueryResult> {
    // Need at least one item segment to extract the name
    let item_name = query.item_segments.last()?;
    let name_lower = item_name.to_lowercase();

    // Search the index for items with the same name
    let name_indices = index.lookup_by_name(&name_lower);
    if name_indices.is_empty() {
        return None;
    }

    // Filter to re-export stubs whose reexport_source is set
    let mut reexport_matches: Vec<usize> = name_indices
        .iter()
        .copied()
        .filter(|&idx| {
            let item = &index.items[idx];
            item.reexport_source.is_some()
                && item.name.eq_ignore_ascii_case(item_name)
                && kind_filter.is_none_or(|k| item.kind.matches_filter(k))
        })
        .collect();

    if reexport_matches.is_empty() {
        // Also try non-stub items at a different path (same name, different module)
        reexport_matches = name_indices
            .iter()
            .copied()
            .filter(|&idx| {
                let item = &index.items[idx];
                item.name.eq_ignore_ascii_case(item_name)
                    && kind_filter.is_none_or(|k| item.kind.matches_filter(k))
            })
            .collect();
    }

    match reexport_matches.len() {
        0 => None,
        1 => Some(QueryResult::Found {
            index: reexport_matches[0],
        }),
        _ => Some(QueryResult::Ambiguous {
            indices: reexport_matches,
            query: query.item_segments.join("::"),
        }),
    }
}
