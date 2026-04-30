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
        // Stubs sometimes advertise an internal source path that doesn't exist
        // verbatim in the source crate (e.g. std re-exports BTreeMap from
        // `alloc::collections::btree::map::BTreeMap`, but alloc's canonical
        // public path is `alloc::collections::BTreeMap`). Fall back to looking
        // up the trailing item name and preferring a non-stub match.
        QueryResult::Ambiguous { .. } | QueryResult::NotFound { .. } => {
            follow_by_name(&source_index, item_path).map(|idx| (source_index, idx))
        }
    }
}

/// Tries to descend into a prefix of the query that's a re-export of another
/// crate's module. For `std::vec::Vec` the system can't find `Vec` in std
/// directly because `std::vec` is a re-export of `alloc::vec`. This walks
/// down the segments looking for a module re-export stub whose remainder
/// resolves in the source crate.
pub(crate) fn try_resolve_via_prefix_reexport(
    query: &QueryPath,
    index: &DocIndex,
    ctx: Option<&crate::resolve::ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Option<(DocIndex, usize)> {
    // Need at least 2 item segments — one for the prefix that is a re-export,
    // one for the remainder we descend into.
    if query.item_segments.len() < 2 {
        return None;
    }

    let crate_name = &index.crate_name;

    // Try the longest prefix first, then shorter ones.
    for prefix_len in (1..query.item_segments.len()).rev() {
        let prefix_segments = &query.item_segments[..prefix_len];
        let remaining = &query.item_segments[prefix_len..];

        // Look for the prefix as a path in the current index. Try with and
        // without the crate name prefix to mirror the broader resolver.
        let with_crate = format!(
            "{}::{}",
            crate_name,
            prefix_segments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("::")
        );
        let without_crate = prefix_segments
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("::");

        let candidates = index.lookup_by_path(&with_crate);
        let candidates = if candidates.is_empty() {
            index.lookup_by_path(&without_crate)
        } else {
            candidates
        };

        // Prefer a re-export stub with reexport_source set.
        let Some(&stub_idx) = candidates
            .iter()
            .find(|&&i| index.items[i].reexport_source.is_some())
        else {
            continue;
        };
        let stub = &index.items[stub_idx];
        if stub.reexport_source.is_none() {
            continue;
        }

        // Found a re-export stub on a prefix. Construct a synthetic stub-like
        // item with the remaining segments tacked onto the source path, then
        // re-use try_follow_reexport's machinery via a manual descent.
        let source_path = stub.reexport_source.as_ref()?;
        let (source_crate, source_item_path) = source_path.split_once("::")?;

        // Resolve and load the source crate.
        let crate_query = QueryPath {
            crate_spec: CrateSpec::Named(source_crate.to_string()),
            item_segments: Vec::new(),
        };
        let (source, _) = crate::resolve_crate_source(ctx, crate_query).ok()?;
        let (source_index, _) =
            crate::load_or_build_index(source, features, feature_suffix, private, false).ok()?;

        // Build full query path inside the source crate: source_item_path + remaining.
        let mut full_segments: Vec<String> = source_item_path.split("::").map(String::from).collect();
        full_segments.extend(remaining.iter().cloned());
        let inner_query = QueryPath {
            crate_spec: CrateSpec::CurrentCrate,
            item_segments: full_segments,
        };
        let result = crate::resolve_item(&inner_query, &source_index, None);

        if let QueryResult::Found { index: idx } = result {
            return Some((source_index, idx));
        }

        // Final fallback: name search for the last queried segment in the source crate
        if let Some(idx) = follow_by_name(&source_index, &remaining.join("::")) {
            return Some((source_index, idx));
        }
    }

    None
}

/// Falls back to a name-based lookup in `source_index` for the last `::`
/// segment of `item_path`. Prefers non-stub items (real definitions) over
/// re-export stubs to avoid following cycles.
fn follow_by_name(source_index: &DocIndex, item_path: &str) -> Option<usize> {
    let item_name = item_path.rsplit("::").next()?;
    let candidates = source_index.lookup_by_name(&item_name.to_lowercase());
    if candidates.is_empty() {
        return None;
    }

    // Prefer a non-stub match with the exact name
    let exact_non_stub = candidates.iter().copied().find(|&idx| {
        let item = &source_index.items[idx];
        item.name == item_name && !crate::query::is_reexport_stub(item)
    });
    if exact_non_stub.is_some() {
        return exact_non_stub;
    }

    // Otherwise any exact-name match (will likely be a stub but no better option)
    candidates.iter().copied().find(|&idx| {
        let item = &source_index.items[idx];
        item.name == item_name
    })
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

    // Match the broader query convention: all-lowercase queries match
    // case-insensitively, anything with an uppercase character requires an
    // exact case match. Without this, querying `Vec` would match items
    // named `vec` (e.g. the std::vec module re-export stub).
    let case_strict = item_name.chars().any(char::is_uppercase);
    let name_matches = |item: &crate::types::IndexItem| -> bool {
        if case_strict {
            item.name == *item_name
        } else {
            item.name.eq_ignore_ascii_case(item_name)
        }
    };

    // Filter to re-export stubs whose reexport_source is set
    let mut reexport_matches: Vec<usize> = name_indices
        .iter()
        .copied()
        .filter(|&idx| {
            let item = &index.items[idx];
            item.reexport_source.is_some()
                && name_matches(item)
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
                name_matches(item) && kind_filter.is_none_or(|k| item.kind.matches_filter(k))
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
