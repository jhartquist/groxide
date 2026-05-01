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
        crate::load_or_build_index(source, features, feature_suffix, private, false, ctx).ok()?;

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
            crate::load_or_build_index(source, features, feature_suffix, private, false, ctx)
                .ok()?;

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

/// Tries to resolve a query through cross-crate wildcard re-exports, e.g.
/// `pub use clap_builder::*` in clap's lib.rs makes `clap_builder`'s public
/// items reachable via `clap::Item`. The glob list is recorded at index-build
/// time; here we walk the query's parent path, find globs originating from
/// matching modules, load the source crate(s), and look up the remaining
/// segments there.
pub(crate) fn try_resolve_via_glob_reexport(
    query: &QueryPath,
    index: &DocIndex,
    ctx: Option<&crate::resolve::ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Option<(DocIndex, usize)> {
    if query.item_segments.is_empty() || index.glob_uses.is_empty() {
        return None;
    }

    // Determine the parent path the user is querying within. For
    // `clap::Args` the parent is "clap" (the crate root); for
    // `clap::sub::Args` it's "clap::sub". We try the longest prefix first
    // so child-module globs are preferred over crate-root globs.
    let crate_name = &index.crate_name;
    let mut parent_segments: Vec<String> = vec![crate_name.clone()];
    parent_segments.extend(query.item_segments[..query.item_segments.len() - 1].iter().cloned());
    let item_name = query.item_segments.last()?;

    while !parent_segments.is_empty() {
        let parent_path = parent_segments.join("::");
        let candidates: Vec<&crate::types::GlobUse> = index
            .glob_uses
            .iter()
            .filter(|g| {
                g.parent_path == parent_path
                    || (parent_segments.len() == 1 && g.parent_path.is_empty())
            })
            .collect();

        for glob in candidates {
            let source = glob.source_path.trim_start_matches("::");
            let (source_crate, source_item_path) =
                source.split_once("::").map_or((source, ""), |(c, p)| (c, p));
            if source_crate.is_empty() {
                continue;
            }

            // Resolve and load source crate.
            let crate_query = QueryPath {
                crate_spec: CrateSpec::Named(source_crate.to_string()),
                item_segments: Vec::new(),
            };
            let Ok((src, _)) = crate::resolve_crate_source(ctx, crate_query) else {
                continue;
            };
            let Ok((source_index, _)) =
                crate::load_or_build_index(src, features, feature_suffix, private, false, ctx)
            else {
                continue;
            };

            // Build inner query: source_item_path + remaining query segments
            // after `parent_segments`. For `grox clap::Args` with glob
            // `clap_builder::*`, source_item_path="" and remaining=["Args"].
            let mut inner_segments: Vec<String> = if source_item_path.is_empty() {
                Vec::new()
            } else {
                source_item_path.split("::").map(String::from).collect()
            };
            let suffix_start = parent_segments.len() - 1; // skip crate name
            inner_segments.extend(query.item_segments[suffix_start..].iter().cloned());

            let inner_query = QueryPath {
                crate_spec: CrateSpec::CurrentCrate,
                item_segments: inner_segments.clone(),
            };
            let result = crate::resolve_item(&inner_query, &source_index, None);
            if let QueryResult::Found { index: idx } = result {
                return Some((source_index, idx));
            }
            // Final fallback: search by trailing item name in the source crate.
            if let Some(idx) = follow_by_name(&source_index, item_name) {
                return Some((source_index, idx));
            }
        }

        parent_segments.pop();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{make_item, make_reexport_stub};
    use crate::types::{GlobUse, ItemKind};

    fn query_path(crate_name: &str, segments: &[&str]) -> QueryPath {
        QueryPath {
            crate_spec: CrateSpec::Named(crate_name.to_string()),
            item_segments: segments.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    // ---- try_resolve_reexport_on_not_found ----

    #[test]
    fn case_strict_match_does_not_treat_uppercase_vec_as_lowercase_vec() {
        // Mirrors the std re-export setup that broke `grox std::vec::Vec`:
        // both a `vec` module and a `vec` macro at std::vec, plus a struct
        // `Vec` at alloc::vec::Vec. The query "Vec" must NOT collapse into
        // matches against items named "vec".
        let mut index = DocIndex::new("std".to_string(), "1.0.0".to_string());
        index.add_item(make_reexport_stub(
            "vec",
            "std::vec",
            ItemKind::Module,
            "alloc::vec",
        ));
        index.add_item(make_reexport_stub(
            "vec",
            "std::vec",
            ItemKind::Macro,
            "alloc::vec",
        ));

        let q = query_path("std", &["vec", "Vec"]);
        let result = try_resolve_reexport_on_not_found(&q, &index, None);
        assert!(
            result.is_none(),
            "case-strict 'Vec' must not match items named 'vec', got {result:?}"
        );
    }

    #[test]
    fn case_insensitive_match_when_query_is_all_lowercase() {
        // All-lowercase queries fall back to case-insensitive matching, so
        // the mod and macro stubs both qualify.
        let mut index = DocIndex::new("std".to_string(), "1.0.0".to_string());
        index.add_item(make_reexport_stub(
            "vec",
            "std::vec",
            ItemKind::Module,
            "alloc::vec",
        ));
        index.add_item(make_reexport_stub(
            "vec",
            "std::vec",
            ItemKind::Macro,
            "alloc::vec",
        ));

        let q = query_path("std", &["vec"]);
        let result = try_resolve_reexport_on_not_found(&q, &index, None);
        assert!(
            matches!(result, Some(QueryResult::Ambiguous { ref indices, .. }) if indices.len() == 2),
            "lowercase 'vec' should match both mod and macro stubs, got {result:?}"
        );
    }

    #[test]
    fn exact_case_match_returns_found_singleton() {
        // Single struct named "Foo" — exact-case match returns Found.
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_reexport_stub(
            "Foo",
            "mycrate::Foo",
            ItemKind::Struct,
            "inner::Foo",
        ));

        let q = query_path("mycrate", &["Foo"]);
        let result = try_resolve_reexport_on_not_found(&q, &index, None);
        assert!(
            matches!(result, Some(QueryResult::Found { .. })),
            "exact-case 'Foo' should resolve to Found, got {result:?}"
        );
    }

    #[test]
    fn no_matches_returns_none() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("real_thing", "mycrate::real_thing", ItemKind::Struct));

        let q = query_path("mycrate", &["nonexistent"]);
        let result = try_resolve_reexport_on_not_found(&q, &index, None);
        assert!(result.is_none());
    }

    // ---- follow_by_name ----

    #[test]
    fn follow_by_name_prefers_non_stub_over_stub() {
        // When both a stub and a real definition share a name, prefer the
        // real definition so re-export following lands on actual content.
        let mut index = DocIndex::new("inner".to_string(), "0.1.0".to_string());
        // index 0 — stub
        index.add_item(make_reexport_stub(
            "Foo",
            "inner::a::Foo",
            ItemKind::Struct,
            "deeper::Foo",
        ));
        // index 1 — real definition
        index.add_item(make_item("Foo", "inner::b::Foo", ItemKind::Struct));

        let idx = follow_by_name(&index, "Foo").expect("name match should be found");
        assert_eq!(idx, 1, "should prefer the non-stub at index 1, got {idx}");
    }

    #[test]
    fn follow_by_name_returns_stub_when_only_stub_exists() {
        let mut index = DocIndex::new("inner".to_string(), "0.1.0".to_string());
        index.add_item(make_reexport_stub(
            "Foo",
            "inner::a::Foo",
            ItemKind::Struct,
            "deeper::Foo",
        ));

        let idx = follow_by_name(&index, "Foo").expect("stub fallback should match");
        assert_eq!(idx, 0);
    }

    #[test]
    fn follow_by_name_unknown_returns_none() {
        let mut index = DocIndex::new("inner".to_string(), "0.1.0".to_string());
        index.add_item(make_item("Bar", "inner::Bar", ItemKind::Struct));

        assert!(follow_by_name(&index, "Foo").is_none());
    }

    #[test]
    fn follow_by_name_handles_path_input_taking_last_segment() {
        // The function accepts a `::`-joined path and uses only the trailing
        // segment for the name lookup.
        let mut index = DocIndex::new("inner".to_string(), "0.1.0".to_string());
        index.add_item(make_item("Foo", "inner::deep::Foo", ItemKind::Struct));

        let idx = follow_by_name(&index, "anything::Foo").expect("trailing 'Foo' should match");
        assert_eq!(idx, 0);
    }

    // ---- try_resolve_via_prefix_reexport (early-exit branches) ----

    #[test]
    fn prefix_descent_returns_none_for_single_segment_query() {
        // Need at least 2 segments to have a "prefix" and a "remainder".
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let mut index = DocIndex::new("std".to_string(), "1.0.0".to_string());
        index.add_item(make_reexport_stub(
            "vec",
            "std::vec",
            ItemKind::Module,
            "alloc::vec",
        ));

        let q = query_path("std", &["vec"]);
        let result = try_resolve_via_prefix_reexport(&q, &index, None, &features, "", false);
        assert!(result.is_none(), "1-segment query has no prefix to descend through");
    }

    #[test]
    fn prefix_descent_returns_none_when_no_prefix_is_a_stub() {
        // No items in the index — no possible prefix matches.
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());

        let q = query_path("mycrate", &["a", "b", "c"]);
        let result = try_resolve_via_prefix_reexport(&q, &index, None, &features, "", false);
        assert!(result.is_none());
    }

    // ---- try_resolve_via_glob_reexport (early-exit + glob filtering) ----

    #[test]
    fn glob_descent_returns_none_when_no_glob_uses() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let index = DocIndex::new("clap".to_string(), "4.5.0".to_string());
        // glob_uses is empty by default

        let q = query_path("clap", &["Arg"]);
        let result = try_resolve_via_glob_reexport(&q, &index, None, &features, "", false);
        assert!(result.is_none());
    }

    #[test]
    fn glob_descent_returns_none_for_empty_segments() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let mut index = DocIndex::new("clap".to_string(), "4.5.0".to_string());
        index.glob_uses.push(GlobUse {
            parent_path: "clap".to_string(),
            source_path: "clap_builder".to_string(),
        });

        let q = query_path("clap", &[]);
        let result = try_resolve_via_glob_reexport(&q, &index, None, &features, "", false);
        assert!(
            result.is_none(),
            "no item segments — nothing to look up via glob"
        );
    }
}
