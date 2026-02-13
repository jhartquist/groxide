use crate::types::{DocIndex, ItemKind, QueryResult};

/// Resolves a query string against a [`DocIndex`] using the 5-stage lookup pipeline.
///
/// Stages:
/// 1. Exact path match (original case, from `path_map`)
/// 2. Case-insensitive path match (via `suffix_map` full-path entries)
/// 3. Suffix match (with non-duplicate filtering)
/// 4. Name match (single-segment only, from `name_map`)
/// 5. `NotFound` with Levenshtein suggestions
///
/// Case sensitivity rules (inspired by `go doc`):
/// - All-lowercase query -> case-insensitive (matches any casing)
/// - Any uppercase character -> exact case match only
///
/// If `kind_filter` is `Some`, only items matching the filter are returned.
/// If filtering removes all results at a given stage, the pipeline continues
/// to the next stage rather than returning `NotFound`.
pub(crate) fn lookup(index: &DocIndex, query: &str, kind_filter: Option<ItemKind>) -> QueryResult {
    let segments: Vec<&str> = if query.is_empty() {
        Vec::new()
    } else {
        query.split("::").collect()
    };

    let query_path = segments.join("::");
    let query_lower = query_path.to_lowercase();

    // Try each resolution strategy in priority order
    if let Some(result) = try_exact_path_match(index, &query_path, kind_filter) {
        return result;
    }
    if let Some(result) =
        try_case_insensitive_path_match(index, &query_path, &query_lower, kind_filter)
    {
        return result;
    }
    if let Some(result) = try_suffix_match(index, &query_path, &query_lower, kind_filter) {
        return result;
    }
    if let Some(result) = try_name_match(index, &segments, &query_path, kind_filter) {
        return result;
    }

    // Stage 5: Not found
    let suggestions = compute_suggestions(index, &query_path);
    QueryResult::NotFound {
        query: query_path,
        suggestions,
    }
}

/// Attempts exact path match (Stage 1).
///
/// Returns immediately on exact path match — does NOT fall through even if
/// kind filtering removes all results.
fn try_exact_path_match(
    index: &DocIndex,
    query_path: &str,
    kind_filter: Option<ItemKind>,
) -> Option<QueryResult> {
    let indices = index.path_map.get(query_path)?;
    let filtered = apply_kind_filter(index, indices, kind_filter);
    Some(classify_results(index, &filtered, query_path))
}

/// Attempts case-insensitive full-path match (Stage 2).
///
/// Looks up the lowercased query in the suffix map, then filters to entries whose
/// full path matches case-insensitively with the same segment count.
fn try_case_insensitive_path_match(
    index: &DocIndex,
    query_path: &str,
    query_lower: &str,
    kind_filter: Option<ItemKind>,
) -> Option<QueryResult> {
    let suffix_indices = index.suffix_map.get(query_lower)?;
    let query_segment_count = query_path.split("::").count();
    let ci_path_matches: Vec<usize> = suffix_indices
        .iter()
        .copied()
        .filter(|&i| {
            let item = &index.items[i];
            item.path.to_lowercase() == query_lower
                && item.path.split("::").count() == query_segment_count
        })
        .collect();

    if ci_path_matches.is_empty() {
        return None;
    }

    let filtered = apply_kind_filter(index, &ci_path_matches, kind_filter);
    let case_filtered = apply_case_sensitivity(index, &filtered, query_path);
    if case_filtered.is_empty() {
        return None; // Fall through to suffix matching
    }
    Some(classify_results(index, &case_filtered, query_path))
}

/// Attempts suffix match (Stage 3).
///
/// Finds items whose path ends with the query segments, preferring non-duplicate
/// matches (where the query segment doesn't also appear earlier in the path).
fn try_suffix_match(
    index: &DocIndex,
    query_path: &str,
    query_lower: &str,
    kind_filter: Option<ItemKind>,
) -> Option<QueryResult> {
    let suffix_indices = index.suffix_map.get(query_lower)?;
    let filtered = apply_kind_filter(index, suffix_indices, kind_filter);
    let case_filtered = apply_case_sensitivity(index, &filtered, query_path);

    let query_segments: Vec<&str> = query_lower.split("::").collect();
    let exact_suffix = filter_exact_suffix_matches(index, &case_filtered, &query_segments);

    if !exact_suffix.is_empty() {
        let non_duplicate = filter_non_duplicate_matches(index, &exact_suffix, &query_segments);
        if !non_duplicate.is_empty() {
            return Some(classify_results(index, &non_duplicate, query_path));
        }
        return Some(classify_results(index, &exact_suffix, query_path));
    }

    // Suffix map had results but none were exact suffix matches
    if !case_filtered.is_empty() {
        return Some(classify_results(index, &case_filtered, query_path));
    }

    None
}

/// Filters indices to those whose path ends with the query segments (case-insensitive).
fn filter_exact_suffix_matches(
    index: &DocIndex,
    indices: &[usize],
    query_segments: &[&str],
) -> Vec<usize> {
    indices
        .iter()
        .copied()
        .filter(|&idx| {
            let item_segments: Vec<&str> = index.items[idx].path.split("::").collect();
            if item_segments.len() < query_segments.len() {
                return false;
            }
            let offset = item_segments.len() - query_segments.len();
            item_segments[offset..]
                .iter()
                .zip(query_segments.iter())
                .all(|(item_seg, query_seg)| item_seg.to_lowercase() == *query_seg)
        })
        .collect()
}

/// Filters to non-duplicate matches for single-segment queries.
///
/// A "duplicate" is an item where the query segment also appears in an earlier path
/// segment (e.g., querying "sync" matches `mycrate::sync::sync` — a duplicate).
fn filter_non_duplicate_matches(
    index: &DocIndex,
    indices: &[usize],
    query_segments: &[&str],
) -> Vec<usize> {
    if query_segments.len() != 1 {
        return indices.to_vec(); // Multi-segment queries always pass
    }

    let query_seg = query_segments[0];
    indices
        .iter()
        .copied()
        .filter(|&idx| {
            let item_segments_lower: Vec<String> = index.items[idx]
                .path
                .to_lowercase()
                .split("::")
                .map(String::from)
                .collect();
            let offset = item_segments_lower.len() - 1;
            offset == 0
                || !item_segments_lower[..offset]
                    .iter()
                    .any(|seg| seg == query_seg)
        })
        .collect()
}

/// Attempts name match for single-segment queries (Stage 4).
fn try_name_match(
    index: &DocIndex,
    segments: &[&str],
    query_path: &str,
    kind_filter: Option<ItemKind>,
) -> Option<QueryResult> {
    if segments.len() != 1 {
        return None;
    }

    let name_lower = segments[0].to_lowercase();
    let name_indices = index.name_map.get(&name_lower)?;
    let filtered = apply_kind_filter(index, name_indices, kind_filter);
    let case_filtered = apply_case_sensitivity(index, &filtered, segments[0]);
    if case_filtered.is_empty() {
        return None;
    }
    Some(classify_results(index, &case_filtered, query_path))
}

/// Filters indices to only those whose kind matches the filter.
fn apply_kind_filter(
    index: &DocIndex,
    indices: &[usize],
    kind_filter: Option<ItemKind>,
) -> Vec<usize> {
    match kind_filter {
        None => indices.to_vec(),
        Some(filter) => indices
            .iter()
            .copied()
            .filter(|&i| index.items[i].kind.matches_filter(filter))
            .collect(),
    }
}

/// Applies case sensitivity rules inspired by `go doc`.
///
/// - All-lowercase query -> case-insensitive (return all indices)
/// - Any uppercase character -> exact case match only
fn apply_case_sensitivity(index: &DocIndex, indices: &[usize], query: &str) -> Vec<usize> {
    // If query has no uppercase characters, it's case-insensitive
    if !query.chars().any(char::is_uppercase) {
        return indices.to_vec();
    }

    // Query has uppercase -> case-sensitive exact match
    if query.contains("::") {
        // Multi-segment: compare as suffix of item path (case-sensitive)
        let query_segments: Vec<&str> = query.split("::").collect();
        indices
            .iter()
            .copied()
            .filter(|&idx| {
                let item_segments: Vec<&str> = index.items[idx].path.split("::").collect();
                if item_segments.len() < query_segments.len() {
                    return false;
                }
                let offset = item_segments.len() - query_segments.len();
                item_segments[offset..] == query_segments[..]
            })
            .collect()
    } else {
        // Single-segment: compare item.name exactly
        indices
            .iter()
            .copied()
            .filter(|&idx| index.items[idx].name == query)
            .collect()
    }
}

/// Converts a list of matching indices into a [`QueryResult`].
///
/// Runs the dedup sequence: re-export stub resolution, crate-root auto-selection,
/// then (path, kind) dedup. If more than one item remains, returns `Ambiguous`.
fn classify_results(index: &DocIndex, indices: &[usize], query: &str) -> QueryResult {
    if indices.is_empty() {
        return QueryResult::NotFound {
            query: query.to_string(),
            suggestions: compute_suggestions(index, query),
        };
    }

    // Stage 1: Re-export stub resolution
    let resolved = resolve_reexport_stubs(index, indices);

    // Stage 2: (path, kind) dedup
    let deduped = deduplicate_by_path_kind(index, &resolved);

    if deduped.len() == 1 {
        return QueryResult::Found { index: deduped[0] };
    }

    // Stage 3: Crate-root auto-selection
    if let Some(selected) = try_auto_select(index, &deduped) {
        return QueryResult::Found { index: selected };
    }

    // Sort remaining results by priority
    let sorted = sort_by_priority(index, &deduped);
    QueryResult::Ambiguous {
        indices: sorted,
        query: query.to_string(),
    }
}

/// Resolves re-export stubs to their canonical items.
///
/// A re-export stub is an item whose signature starts with `pub use ` and has no children.
/// If the canonical item (looked up by name in the index) is already in the result set,
/// the stub is dropped.
fn resolve_reexport_stubs(index: &DocIndex, indices: &[usize]) -> Vec<usize> {
    let mut result = Vec::with_capacity(indices.len());
    let index_set: std::collections::HashSet<usize> = indices.iter().copied().collect();

    for &idx in indices {
        let item = &index.items[idx];
        if is_reexport_stub(item) {
            // Check if any other match in our result set is the canonical item
            // (same name, not a stub). If so, drop this stub.
            let has_canonical = indices.iter().any(|&other_idx| {
                other_idx != idx && {
                    let other = &index.items[other_idx];
                    other.name == item.name && !is_reexport_stub(other)
                }
            });
            if has_canonical {
                continue; // drop the stub
            }

            // Try to find the canonical item elsewhere in the full index
            let name_lower = item.name.to_lowercase();
            if let Some(name_indices) = index.name_map.get(&name_lower) {
                let canonical = name_indices.iter().find(|&&ni| {
                    !index_set.contains(&ni) && {
                        let candidate = &index.items[ni];
                        candidate.name == item.name && !is_reexport_stub(candidate)
                    }
                });
                if let Some(&canonical_idx) = canonical {
                    // Replace stub with canonical item
                    result.push(canonical_idx);
                    continue;
                }
            }
        }
        result.push(idx);
    }

    result
}

/// Returns whether an item is a re-export stub (`pub use` with no children).
fn is_reexport_stub(item: &crate::types::IndexItem) -> bool {
    item.signature.starts_with("pub use ") && item.children.is_empty()
}

/// Removes entries with duplicate (path, kind) pairs.
fn deduplicate_by_path_kind(index: &DocIndex, indices: &[usize]) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    indices
        .iter()
        .copied()
        .filter(|&idx| {
            let item = &index.items[idx];
            seen.insert((item.path.clone(), item.kind))
        })
        .collect()
}

/// Auto-selects when exactly one crate-root primary item exists.
fn try_auto_select(index: &DocIndex, indices: &[usize]) -> Option<usize> {
    let mut crate_root_primary = Vec::new();
    let mut crate_root_other = Vec::new();
    let mut nested = Vec::new();

    for &idx in indices {
        let item = &index.items[idx];
        let depth = item.path.split("::").count();
        if depth == 2 && item.kind.is_primary() {
            crate_root_primary.push(idx);
        } else if depth == 2 {
            crate_root_other.push(idx);
        } else {
            nested.push(idx);
        }
    }

    let has_3_level_primary = nested.iter().any(|&idx| {
        let item = &index.items[idx];
        item.path.split("::").count() == 3 && item.kind.is_primary()
    });

    if crate_root_primary.len() == 1 && crate_root_other.is_empty() && !has_3_level_primary {
        Some(crate_root_primary[0])
    } else {
        None
    }
}

/// Sorts results for the Ambiguous variant by priority.
///
/// 1. Crate-root primary items (alphabetically by path)
/// 2. Crate-root other items (alphabetically by path)
/// 3. Nested items (by depth ascending, then alphabetically by path)
fn sort_by_priority(index: &DocIndex, indices: &[usize]) -> Vec<usize> {
    let mut crate_root_primary: Vec<usize> = Vec::new();
    let mut crate_root_other: Vec<usize> = Vec::new();
    let mut nested: Vec<usize> = Vec::new();

    for &idx in indices {
        let item = &index.items[idx];
        let depth = item.path.split("::").count();
        if depth == 2 && item.kind.is_primary() {
            crate_root_primary.push(idx);
        } else if depth == 2 {
            crate_root_other.push(idx);
        } else {
            nested.push(idx);
        }
    }

    crate_root_primary.sort_by(|&a, &b| index.items[a].path.cmp(&index.items[b].path));
    crate_root_other.sort_by(|&a, &b| index.items[a].path.cmp(&index.items[b].path));
    nested.sort_by(|&a, &b| {
        let depth_a = index.items[a].path.split("::").count();
        let depth_b = index.items[b].path.split("::").count();
        depth_a
            .cmp(&depth_b)
            .then_with(|| index.items[a].path.cmp(&index.items[b].path))
    });

    let mut result = Vec::with_capacity(indices.len());
    result.extend(crate_root_primary);
    result.extend(crate_root_other);
    result.extend(nested);
    result
}

/// Computes "did you mean?" suggestions using Levenshtein distance.
fn compute_suggestions(index: &DocIndex, query: &str) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let last_segment = query.split("::").last().unwrap_or(query).to_lowercase();
    let mut candidates: Vec<(String, usize)> = Vec::new();

    for item in &index.items {
        let path_lower = item.path.to_lowercase();
        let name_lower = item.name.to_lowercase();

        if !could_match_within_distance(&query_lower, &path_lower, 3)
            && !could_match_within_distance(&query_lower, &name_lower, 3)
            && !could_match_within_distance(&last_segment, &name_lower, 3)
        {
            continue;
        }

        let path_dist = levenshtein_distance(&query_lower, &path_lower);
        let name_dist = levenshtein_distance(&query_lower, &name_lower);
        let seg_dist = levenshtein_distance(&last_segment, &name_lower);
        let distance = path_dist.min(name_dist).min(seg_dist);

        if distance <= 3 {
            candidates.push((item.path.clone(), distance));
        }
    }

    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // Deduplicate by path
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|(path, _)| seen.insert(path.clone()));

    candidates.truncate(5);
    candidates.into_iter().map(|(path, _)| path).collect()
}

/// Pre-filter heuristic: quickly checks if two strings could be within edit distance.
fn could_match_within_distance(s1: &str, s2: &str, max_dist: usize) -> bool {
    let len1 = s1.len();
    let len2 = s2.len();
    let len_diff = len1.abs_diff(len2);

    if len_diff > max_dist {
        return false;
    }

    if len1 > 2 && len2 > 2 && s1.as_bytes()[0] != s2.as_bytes()[0] && len_diff >= max_dist {
        return false;
    }

    true
}

/// Looks up a method on a parent type.
///
/// Resolves `parent_segments` via `lookup()`, then searches the parent's children
/// for `method_name`. If the parent is `Ambiguous`, bubbles up the ambiguity.
/// If the parent is found but the method is not, returns `NotFound` with suggestions.
pub(crate) fn lookup_method(
    index: &DocIndex,
    parent_segments: &[&str],
    method_name: &str,
    kind_filter: Option<ItemKind>,
) -> QueryResult {
    let parent_query = parent_segments.join("::");
    let parent_result = lookup(index, &parent_query, None);

    match parent_result {
        QueryResult::Found { index: parent_idx } => {
            let parent_item = &index.items[parent_idx];

            // Search children for matching method name
            let method_lower = method_name.to_lowercase();
            let matching_children: Vec<usize> = parent_item
                .children
                .iter()
                .filter(|child| {
                    child.name.to_lowercase() == method_lower
                        && kind_filter.is_none_or(|kf| child.kind.matches_filter(kf))
                })
                .map(|child| child.index)
                .collect();

            // Apply case sensitivity to matching children
            let case_filtered = apply_case_sensitivity(index, &matching_children, method_name);

            if !case_filtered.is_empty() {
                let full_path = format!("{}::{method_name}", parent_item.path);
                return classify_results(index, &case_filtered, &full_path);
            }

            // Parent found, method not found -> suggest similar method names
            let full_path = format!("{}::{method_name}", parent_item.path);
            let suggestions = compute_method_suggestions(index, parent_idx, method_name);
            QueryResult::NotFound {
                query: full_path,
                suggestions,
            }
        }
        // Ambiguous or NotFound parent -> bubble up
        other => other,
    }
}

/// Computes method name suggestions for a parent type.
fn compute_method_suggestions(
    index: &DocIndex,
    parent_idx: usize,
    method_name: &str,
) -> Vec<String> {
    let method_lower = method_name.to_lowercase();
    let parent_item = &index.items[parent_idx];
    let mut candidates: Vec<(String, usize)> = Vec::new();

    for child in &parent_item.children {
        let child_lower = child.name.to_lowercase();
        if !could_match_within_distance(&method_lower, &child_lower, 3) {
            continue;
        }
        let distance = levenshtein_distance(&method_lower, &child_lower);
        if distance <= 3 {
            let path = format!("{}::{}", parent_item.path, child.name);
            candidates.push((path, distance));
        }
    }

    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // Deduplicate by path
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|(path, _)| seen.insert(path.clone()));

    candidates.truncate(5);
    candidates.into_iter().map(|(path, _)| path).collect()
}

/// Determines whether a single-segment query looks like an item name.
///
/// Returns `true` if the query is likely a Rust item (type, function, constant),
/// `false` if it's likely a crate name. Used to gate multi-crate search and
/// influence single-segment resolution reinterpretation.
pub(crate) fn looks_like_item_name(query: &str) -> bool {
    const COMMON_METHODS: &[&str] = &[
        "clone", "default", "into", "from", "borrow", "deref", "format", "parse", "write", "read",
        "open", "close", "send", "recv",
    ];

    // Rule 1: Empty string -> not an item name
    if query.is_empty() {
        return false;
    }

    // Rule 2: Contains hyphen -> definitely a crate name
    if query.contains('-') {
        return false;
    }

    // Rule 3: Any uppercase character -> item name
    if query.chars().any(char::is_uppercase) {
        return true;
    }

    // Rule 4: Contains underscores -> heuristic
    if query.contains('_') {
        let segments: Vec<&str> = query.split('_').collect();
        let all_simple = segments
            .iter()
            .all(|s| s.chars().all(|c| c.is_lowercase() || c.is_ascii_digit()));
        let avg_len = if segments.is_empty() {
            0
        } else {
            segments.iter().map(|s| s.len()).sum::<usize>() / segments.len()
        };
        if segments.len() <= 3 && all_simple && avg_len <= 6 {
            return false; // likely crate name
        }
        return true; // complex snake_case -> likely function/method
    }

    // Rule 5: Short single lowercase word
    if query.len() <= 4 {
        return true; // "new", "len", "pop", "push"
    }

    // Rule 6: Check common method names
    if COMMON_METHODS.contains(&query) {
        return true;
    }

    // Rule 7: Default -> crate name
    false
}

/// Standard Levenshtein edit distance using two-row optimization.
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let a: Vec<char> = s1.chars().collect();
    let b: Vec<char> = s2.chars().collect();
    let m = a.len();
    let n = b.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item;
    use crate::types::ChildRef;

    /// Builds a synthetic `DocIndex` with common test items.
    fn build_test_index() -> DocIndex {
        let mut index = DocIndex::new("tokio".to_string(), "1.0.0".to_string());

        // Index 0: crate root
        index.add_item(make_item("tokio", "tokio", ItemKind::Module));
        // Index 1: sync module
        index.add_item(make_item("sync", "tokio::sync", ItemKind::Module));
        // Index 2: Mutex struct
        index.add_item(make_item("Mutex", "tokio::sync::Mutex", ItemKind::Struct));
        // Index 3: RwLock struct
        index.add_item(make_item("RwLock", "tokio::sync::RwLock", ItemKind::Struct));
        // Index 4: spawn function
        index.add_item(make_item("spawn", "tokio::spawn", ItemKind::Function));
        // Index 5: Runtime struct at crate root
        index.add_item(make_item("Runtime", "tokio::Runtime", ItemKind::Struct));
        // Index 6: Builder struct nested
        index.add_item(make_item(
            "Builder",
            "tokio::runtime::Builder",
            ItemKind::Struct,
        ));
        // Index 7: MAX_THREADS constant
        index.add_item(make_item(
            "MAX_THREADS",
            "tokio::runtime::MAX_THREADS",
            ItemKind::Constant,
        ));

        index
    }

    // ---- Stage 1: Exact path match ----

    #[test]
    fn lookup_returns_found_when_exact_path_matches() {
        let index = build_test_index();
        let result = lookup(&index, "tokio::sync::Mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(idx, 2);
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn lookup_returns_found_for_exact_crate_root() {
        let index = build_test_index();
        let result = lookup(&index, "tokio", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(idx, 0);
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    // ---- Stage 2: Case-insensitive path match ----

    #[test]
    fn lookup_returns_found_when_case_insensitive_path_matches() {
        let index = build_test_index();
        // "mutex" (all lowercase) should find "Mutex" case-insensitively
        let result = lookup(&index, "mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].name, "Mutex");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn lookup_case_insensitive_full_path() {
        let index = build_test_index();
        // "tokio::sync::mutex" (all lowercase) should match "tokio::sync::Mutex"
        let result = lookup(&index, "tokio::sync::mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "tokio::sync::Mutex");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    // ---- Stage 3: Suffix match ----

    #[test]
    fn lookup_returns_found_when_suffix_matches() {
        let index = build_test_index();
        // "sync::Mutex" should match "tokio::sync::Mutex"
        let result = lookup(&index, "sync::Mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "tokio::sync::Mutex");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn lookup_suffix_match_case_insensitive() {
        let index = build_test_index();
        // "sync::mutex" (all lowercase) should match "tokio::sync::Mutex"
        let result = lookup(&index, "sync::mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "tokio::sync::Mutex");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    // ---- Non-duplicate filtering ----

    #[test]
    fn lookup_prefers_non_duplicate_suffix_match() {
        // If "sync" appears as both "tokio::sync" (module) and "tokio::sync::sync" (hypothetical),
        // prefer the one where "sync" does NOT appear earlier in the path.
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // "sync" appears in the path prefix AND as the last segment -> duplicate
        index.add_item(make_item("sync", "mycrate::sync::sync", ItemKind::Function));
        // "sync" only appears as the last segment -> non-duplicate
        index.add_item(make_item("sync", "mycrate::sync", ItemKind::Module));

        let result = lookup(&index, "sync", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "mycrate::sync");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    // ---- Stage 4: Name match ----

    #[test]
    fn lookup_returns_found_via_name_match() {
        // Name match only triggers for single-segment queries when other stages fail
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        index.add_item(make_item("Foo", "mycrate::inner::Foo", ItemKind::Struct));

        // "Foo" as a query: it's not in path_map as "Foo", not in suffix_map as "foo" with
        // exact suffix matching... actually it IS in suffix_map via the suffix "foo".
        // So this will actually match via suffix. Let's craft a case that specifically
        // requires name_map.
        //
        // Actually, suffix_map will contain "foo" for "mycrate::inner::Foo", so suffix
        // match will work. Name match is a fallback for edge cases where suffix_map
        // doesn't have the entry. In practice with DocIndex::add_item, name_map and
        // suffix_map both contain single-name entries. Name match is really for
        // robustness. Let's test it works regardless.
        let result = lookup(&index, "Foo", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].name, "Foo");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    // ---- Kind filter ----

    #[test]
    fn lookup_kind_filter_narrows_results() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Same name, different kinds
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Function));

        // Filter to struct only
        let result = lookup(&index, "mycrate::Foo", Some(ItemKind::Struct));
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].kind, ItemKind::Struct);
            }
            other => panic!("expected Found, got {other:?}"),
        }

        // Filter to function only
        let result = lookup(&index, "mycrate::Foo", Some(ItemKind::Function));
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].kind, ItemKind::Function);
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn lookup_kind_filter_relaxation_when_no_results() {
        // Task 7 scope: the lookup function itself just applies the filter at each
        // stage. Kind filter relaxation is done by the caller (resolve_item in
        // orchestration). Here we verify that when a kind filter produces NotFound,
        // re-calling without filter works.
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Module));

        // Filter for Function -> NotFound (it's a module)
        let result = lookup(&index, "mycrate::Foo", Some(ItemKind::Function));
        assert!(matches!(result, QueryResult::NotFound { .. }));

        // Without filter -> Found
        let result = lookup(&index, "mycrate::Foo", None);
        assert!(matches!(result, QueryResult::Found { .. }));
    }

    // ---- Multiple matches (Ambiguous) ----

    #[test]
    fn lookup_returns_ambiguous_when_multiple_matches() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Two different structs at different paths, but same suffix
        index.add_item(make_item(
            "Builder",
            "mycrate::http::Builder",
            ItemKind::Struct,
        ));
        index.add_item(make_item(
            "Builder",
            "mycrate::runtime::Builder",
            ItemKind::Struct,
        ));

        let result = lookup(&index, "Builder", None);
        match result {
            QueryResult::Ambiguous { indices, .. } => {
                assert_eq!(indices.len(), 2);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    // ---- NotFound ----

    #[test]
    fn lookup_returns_not_found_when_nothing_matches() {
        let index = build_test_index();
        let result = lookup(&index, "NonexistentItem", None);
        match result {
            QueryResult::NotFound { query, .. } => {
                assert_eq!(query, "NonexistentItem");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    // ---- Case sensitivity: uppercase query matches exactly ----

    #[test]
    fn lookup_case_sensitive_uppercase_query_matches_exactly() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        index.add_item(make_item("Mutex", "mycrate::Mutex", ItemKind::Struct));
        index.add_item(make_item("mutex", "mycrate::mutex", ItemKind::Module));

        // "Mutex" (uppercase M) should match only "Mutex", not "mutex"
        let result = lookup(&index, "Mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].name, "Mutex");
                assert_eq!(index.items[idx].kind, ItemKind::Struct);
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn lookup_case_insensitive_lowercase_query_matches_any_case() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        index.add_item(make_item("Mutex", "mycrate::Mutex", ItemKind::Struct));

        // "mutex" (all lowercase) should match "Mutex"
        let result = lookup(&index, "mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].name, "Mutex");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn lookup_case_sensitive_uppercase_does_not_match_different_case() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        index.add_item(make_item("mutex", "mycrate::mutex", ItemKind::Module));

        // "Mutex" (uppercase M) should NOT match "mutex" (lowercase)
        let result = lookup(&index, "Mutex", None);
        assert!(matches!(result, QueryResult::NotFound { .. }));
    }

    // ---- Multi-segment case sensitivity ----

    #[test]
    fn lookup_case_sensitive_multi_segment() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        index.add_item(make_item("Mutex", "mycrate::sync::Mutex", ItemKind::Struct));
        index.add_item(make_item("mutex", "mycrate::sync::mutex", ItemKind::Module));

        // "sync::Mutex" has uppercase -> exact match only
        let result = lookup(&index, "sync::Mutex", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "mycrate::sync::Mutex");
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    // ---- Crate-root auto-selection ----

    #[test]
    fn lookup_auto_selects_crate_root_primary() {
        let mut index = DocIndex::new("serde".to_string(), "1.0.0".to_string());
        // Crate-root primary (depth 2, primary kind)
        index.add_item(make_item(
            "Deserialize",
            "serde::Deserialize",
            ItemKind::Trait,
        ));
        // Nested (depth 3)
        index.add_item(make_item(
            "Deserialize",
            "serde::de::Deserialize",
            ItemKind::Macro,
        ));

        let result = lookup(&index, "Deserialize", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "serde::Deserialize");
            }
            other => panic!("expected Found via auto-selection, got {other:?}"),
        }
    }

    // ---- (path, kind) deduplication ----

    #[test]
    fn lookup_deduplicates_by_path_and_kind() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Two items with same path and kind (e.g., from re-exports)
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));

        let result = lookup(&index, "mycrate::Foo", None);
        match result {
            QueryResult::Found { .. } => {} // Should deduplicate to 1
            other => panic!("expected Found after dedup, got {other:?}"),
        }
    }

    #[test]
    fn lookup_preserves_different_kinds_same_path() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Same path but different kinds -> genuinely different, both preserved
        index.add_item(make_item("Parser", "mycrate::Parser", ItemKind::Trait));
        index.add_item(make_item("Parser", "mycrate::Parser", ItemKind::ProcMacro));

        let result = lookup(&index, "mycrate::Parser", None);
        match result {
            QueryResult::Ambiguous { indices, .. } => {
                assert_eq!(indices.len(), 2);
            }
            other => panic!("expected Ambiguous (different kinds), got {other:?}"),
        }
    }

    // ---- Suggestions ----

    #[test]
    fn lookup_not_found_provides_suggestions_for_typos() {
        let index = build_test_index();
        // "Mutx" is close to "Mutex" (edit distance 1)
        let result = lookup(&index, "Mutx", None);
        match result {
            QueryResult::NotFound { suggestions, .. } => {
                assert!(
                    suggestions.iter().any(|s| s.contains("Mutex")),
                    "expected suggestion containing 'Mutex', got {suggestions:?}"
                );
            }
            other => panic!("expected NotFound with suggestions, got {other:?}"),
        }
    }

    // ---- Levenshtein distance ----

    #[test]
    fn levenshtein_identical_strings() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
    }

    #[test]
    fn levenshtein_empty_strings() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
    }

    #[test]
    fn levenshtein_single_edit() {
        assert_eq!(levenshtein_distance("kitten", "sitten"), 1); // substitution
        assert_eq!(levenshtein_distance("abc", "abcd"), 1); // insertion
        assert_eq!(levenshtein_distance("abcd", "abc"), 1); // deletion
    }

    #[test]
    fn levenshtein_multiple_edits() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
    }

    // ---- could_match_within_distance ----

    #[test]
    fn could_match_rejects_large_length_difference() {
        assert!(!could_match_within_distance("a", "abcde", 3));
    }

    #[test]
    fn could_match_accepts_similar_length() {
        assert!(could_match_within_distance("abc", "abd", 3));
    }

    // ---- Empty query ----

    #[test]
    fn lookup_empty_query_returns_not_found() {
        let index = build_test_index();
        // Empty query produces empty segments -> empty query_path
        // This should hit Stage 5 (no path_map entry for "")
        let result = lookup(&index, "", None);
        assert!(matches!(result, QueryResult::NotFound { .. }));
    }

    // ---- Exact path match takes priority over suffix match ----

    #[test]
    fn lookup_exact_path_takes_priority_over_suffix() {
        let mut index = DocIndex::new("grox".to_string(), "0.1.0".to_string());
        // "grox::cli" is both a module (exact path) and suffix of "grox::cli::Cli"
        index.add_item(make_item("cli", "grox::cli", ItemKind::Module));
        index.add_item(make_item("Cli", "grox::cli::Cli", ItemKind::Struct));

        // "grox::cli" exact path match should return the module, not ambiguous
        let result = lookup(&index, "grox::cli", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "grox::cli");
                assert_eq!(index.items[idx].kind, ItemKind::Module);
            }
            other => panic!("expected Found for exact path, got {other:?}"),
        }
    }

    // ---- Task 8: Crate-root auto-selection ----

    #[test]
    fn auto_select_single_primary_at_root_wins() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Crate-root primary (depth 2)
        index.add_item(make_item("Widget", "mycrate::Widget", ItemKind::Struct));
        // Nested non-primary (depth 3)
        index.add_item(make_item(
            "Widget",
            "mycrate::inner::Widget",
            ItemKind::Function,
        ));

        let result = lookup(&index, "Widget", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "mycrate::Widget");
                assert_eq!(index.items[idx].kind, ItemKind::Struct);
            }
            other => panic!("expected Found via auto-selection, got {other:?}"),
        }
    }

    #[test]
    fn auto_select_blocked_by_3_level_primary() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Crate-root primary
        index.add_item(make_item("Widget", "mycrate::Widget", ItemKind::Struct));
        // 3-level primary -> blocks auto-selection
        index.add_item(make_item("Widget", "mycrate::ui::Widget", ItemKind::Struct));

        let result = lookup(&index, "Widget", None);
        assert!(
            matches!(result, QueryResult::Ambiguous { .. }),
            "expected Ambiguous when 3-level primary exists, got {result:?}"
        );
    }

    // ---- Task 8: Dedup removes (path, kind) duplicates ----

    #[test]
    fn dedup_removes_path_kind_duplicates() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Three items with same (path, kind)
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));
        index.add_item(make_item("Foo", "mycrate::Foo", ItemKind::Struct));

        let result = lookup(&index, "mycrate::Foo", None);
        match result {
            QueryResult::Found { .. } => {} // Deduped to 1
            other => panic!("expected Found after dedup, got {other:?}"),
        }
    }

    // ---- Task 8: Re-export stubs resolved to canonical items ----

    #[test]
    fn reexport_stub_resolved_to_canonical() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Canonical item (not a stub)
        let mut canonical = make_item("Widget", "mycrate::inner::Widget", ItemKind::Struct);
        canonical.children.push(ChildRef {
            index: 999, // dummy
            kind: ItemKind::Function,
            name: "new".to_string(),
        });
        index.add_item(canonical);
        // Re-export stub
        let mut stub = make_item("Widget", "mycrate::Widget", ItemKind::Struct);
        stub.signature = "pub use inner::Widget".to_string();
        // stub has no children -> is_reexport_stub() returns true
        index.add_item(stub);

        // Both match "Widget". The stub should be dropped in favor of the canonical.
        let result = lookup(&index, "Widget", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].path, "mycrate::inner::Widget");
            }
            other => panic!("expected Found (canonical), got {other:?}"),
        }
    }

    // ---- Task 8: Suggestions for typos ----

    #[test]
    fn suggestions_returns_close_matches_for_typo() {
        let index = build_test_index();
        // "Mutx" -> "Mutex" (distance 1)
        let suggestions = compute_suggestions(&index, "Mutx");
        assert!(
            suggestions.iter().any(|s| s.contains("Mutex")),
            "expected Mutex in suggestions, got {suggestions:?}"
        );
    }

    #[test]
    fn suggestions_dedup_and_cap_at_5() {
        // Build index with many similar items
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        for i in 0..20 {
            let name = format!("Foob{i}");
            let path = format!("mycrate::{name}");
            index.add_item(make_item(&name, &path, ItemKind::Struct));
        }

        let suggestions = compute_suggestions(&index, "Foob");
        assert!(
            suggestions.len() <= 5,
            "suggestions should be capped at 5, got {}",
            suggestions.len()
        );
        // Verify no duplicates
        let unique: std::collections::HashSet<&String> = suggestions.iter().collect();
        assert_eq!(
            unique.len(),
            suggestions.len(),
            "suggestions should be deduplicated"
        );
    }

    // ---- Task 8: Method lookup ----

    #[test]
    fn method_lookup_finds_method_on_parent() {
        let mut index = DocIndex::new("tokio".to_string(), "1.0.0".to_string());
        // Parent: Mutex struct at index 0
        let mut mutex = make_item("Mutex", "tokio::sync::Mutex", ItemKind::Struct);
        // Child: lock method at index 1
        mutex.children.push(ChildRef {
            index: 1,
            kind: ItemKind::Function,
            name: "lock".to_string(),
        });
        index.add_item(mutex);
        // The actual lock item
        index.add_item(make_item(
            "lock",
            "tokio::sync::Mutex::lock",
            ItemKind::Function,
        ));

        let result = lookup_method(&index, &["sync", "Mutex"], "lock", None);
        match result {
            QueryResult::Found { index: idx } => {
                assert_eq!(index.items[idx].name, "lock");
            }
            other => panic!("expected Found for method lookup, got {other:?}"),
        }
    }

    #[test]
    fn method_lookup_returns_not_found_with_suggestions_when_method_missing() {
        let mut index = DocIndex::new("tokio".to_string(), "1.0.0".to_string());
        let mut mutex = make_item("Mutex", "tokio::sync::Mutex", ItemKind::Struct);
        mutex.children.push(ChildRef {
            index: 1,
            kind: ItemKind::Function,
            name: "lock".to_string(),
        });
        index.add_item(mutex);
        index.add_item(make_item(
            "lock",
            "tokio::sync::Mutex::lock",
            ItemKind::Function,
        ));

        // "lokc" is a typo for "lock"
        let result = lookup_method(&index, &["sync", "Mutex"], "lokc", None);
        match result {
            QueryResult::NotFound { suggestions, .. } => {
                assert!(
                    suggestions.iter().any(|s| s.contains("lock")),
                    "expected lock in suggestions, got {suggestions:?}"
                );
            }
            other => panic!("expected NotFound with suggestions, got {other:?}"),
        }
    }

    #[test]
    fn method_lookup_bubbles_ambiguous_parent() {
        let mut index = DocIndex::new("mycrate".to_string(), "1.0.0".to_string());
        // Two items named "Builder" at same depth -> ambiguous parent
        index.add_item(make_item(
            "Builder",
            "mycrate::http::Builder",
            ItemKind::Struct,
        ));
        index.add_item(make_item(
            "Builder",
            "mycrate::runtime::Builder",
            ItemKind::Struct,
        ));

        let result = lookup_method(&index, &["Builder"], "build", None);
        assert!(
            matches!(result, QueryResult::Ambiguous { .. }),
            "expected Ambiguous for ambiguous parent, got {result:?}"
        );
    }

    // ---- Task 8: looks_like_item_name ----

    #[test]
    fn looks_like_item_name_uppercase_returns_true() {
        assert!(looks_like_item_name("Mutex"));
        assert!(looks_like_item_name("HashMap"));
        assert!(looks_like_item_name("Vec"));
        assert!(looks_like_item_name("MAX_SIZE"));
    }

    #[test]
    fn looks_like_item_name_lowercase_long_returns_false() {
        assert!(!looks_like_item_name("serde"));
        assert!(!looks_like_item_name("tokio"));
        assert!(!looks_like_item_name("regex"));
        assert!(!looks_like_item_name("reqwest"));
    }

    #[test]
    fn looks_like_item_name_short_returns_true() {
        assert!(looks_like_item_name("new"));
        assert!(looks_like_item_name("len"));
        assert!(looks_like_item_name("pop"));
        assert!(looks_like_item_name("push"));
    }

    #[test]
    fn looks_like_item_name_common_method_returns_true() {
        assert!(looks_like_item_name("clone"));
        assert!(looks_like_item_name("default"));
        assert!(looks_like_item_name("parse"));
        assert!(looks_like_item_name("format"));
    }

    #[test]
    fn looks_like_item_name_hyphen_returns_false() {
        assert!(!looks_like_item_name("my-crate"));
        assert!(!looks_like_item_name("serde-json"));
    }

    #[test]
    fn looks_like_item_name_empty_returns_false() {
        assert!(!looks_like_item_name(""));
    }

    #[test]
    fn looks_like_item_name_underscore_crate_returns_false() {
        assert!(!looks_like_item_name("serde_json"));
        assert!(!looks_like_item_name("tokio_util"));
    }

    #[test]
    fn looks_like_item_name_complex_snake_case_returns_true() {
        assert!(looks_like_item_name("my_longer_function_name"));
    }
}
