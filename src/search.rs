use std::collections::HashSet;

use crate::error::{GroxError, Result};
use crate::types::{DocIndex, ItemKind, SearchResult};

/// Searches all items in a `DocIndex` for matches against a query string.
///
/// Supports OR queries (split on `|`) and AND within groups (split on whitespace).
/// Scores results across 5 tiers: name exact (100), name substring (75),
/// signature (40), module path (30), docs (20). Returns up to 20 results,
/// sorted by score descending, with path-based dedup for re-exports.
///
/// # Errors
///
/// Returns `GroxError::InvalidQuery` if the query is empty or contains only
/// whitespace and pipes.
pub(crate) fn search(
    index: &DocIndex,
    query: &str,
    kind_filter: Option<ItemKind>,
) -> Result<Vec<SearchResult>> {
    let or_groups = parse_query(query)?;

    let mut scored: Vec<SearchResult> = Vec::new();

    for (idx, item) in index.items.iter().enumerate() {
        // Apply kind filter before scoring (performance optimization)
        if let Some(filter) = kind_filter {
            if !item.kind.matches_filter(filter) {
                continue;
            }
        }

        let entry = SearchEntry::from_item(item);
        let score = score_entry(&entry, &or_groups);
        if score > 0 {
            scored.push(SearchResult { index: idx, score });
        }
    }

    // Sort by score descending, then path ascending (alphabetical tiebreaker)
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| index.items[a.index].path.cmp(&index.items[b.index].path))
    });

    // Cap at 20
    scored.truncate(20);

    // Deduplicate by path (first occurrence wins — highest score)
    let mut seen = HashSet::new();
    scored.retain(|r| seen.insert(index.items[r.index].path.clone()));

    Ok(scored)
}

/// Pre-lowercased search entry for one item. All fields are already lowercased.
struct SearchEntry {
    name: String,
    path: String,
    signature: String,
    docs: String,
}

impl SearchEntry {
    /// Creates a search entry from an `IndexItem`, pre-lowercasing all fields.
    fn from_item(item: &crate::types::IndexItem) -> Self {
        let docs_end = truncate_to_char_boundary(&item.docs, 500);
        let docs_truncated = &item.docs[..docs_end];

        Self {
            name: item.name.to_lowercase(),
            path: item.path.to_lowercase(),
            signature: item.signature.to_lowercase(),
            docs: docs_truncated.to_lowercase(),
        }
    }
}

/// Finds the largest byte index <= `max_bytes` that falls on a UTF-8 char boundary.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Parses a search query into OR groups of AND terms.
///
/// Pipe (`|`) splits OR groups. Whitespace splits AND terms within a group.
/// Empty terms are ignored. Returns an error if no valid terms remain.
fn parse_query(query: &str) -> Result<Vec<Vec<String>>> {
    let query_lower = query.to_lowercase();
    let mut result = Vec::new();

    for or_part in query_lower.split('|') {
        let terms: Vec<String> = or_part.split_whitespace().map(String::from).collect();
        if !terms.is_empty() {
            result.push(terms);
        }
    }

    if result.is_empty() {
        return Err(GroxError::InvalidQuery {
            message: "search query cannot be empty".to_string(),
        });
    }

    Ok(result)
}

/// Scores a single term against a search entry.
///
/// Returns a score from the 5-tier system:
/// - 100: exact name match
/// - 75: name substring
/// - 40: signature match
/// - 30: module path match
/// - 20: docs match
/// - 0: no match
fn score_term(entry: &SearchEntry, term: &str) -> u32 {
    let mut score = 0;

    // Tier 1-2: Name matching (mutually exclusive)
    if entry.name == term {
        score = 100; // exact name match
    } else if entry.name.contains(term) {
        score = 75; // name substring
    }

    // Tier 3: Module path matching (only if no name match)
    if score == 0 {
        if let Some(module_path) = entry.path.rsplit_once("::").map(|(prefix, _)| prefix) {
            if module_path.contains(term) {
                score = 30;
            }
        }
    }

    // Tier 4: Signature matching (overrides path, not name)
    if entry.signature.contains(term) && score < 40 {
        score = 40;
    }

    // Tier 5: Docs matching (only if nothing else matched well)
    if entry.docs.contains(term) && score < 20 {
        score = 20;
    }

    score
}

/// Scores an AND group of terms against a search entry.
///
/// Sums per-term scores. If any term scores 0, the entry is disqualified.
fn score_and_terms(entry: &SearchEntry, terms: &[String]) -> u32 {
    let mut total = 0;
    for term in terms {
        let s = score_term(entry, term);
        if s == 0 {
            return 0; // ALL terms must match
        }
        total += s;
    }
    total
}

/// Scores an entry against all OR groups, returning the maximum score.
fn score_entry(entry: &SearchEntry, or_groups: &[Vec<String>]) -> u32 {
    or_groups
        .iter()
        .map(|group| score_and_terms(entry, group))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item;
    use crate::types::{IndexItem, ItemKind};

    fn make_item_with_sig(name: &str, path: &str, kind: ItemKind, signature: &str) -> IndexItem {
        let mut item = make_item(name, path, kind);
        item.signature = signature.to_string();
        item
    }

    fn make_item_with_docs(name: &str, path: &str, kind: ItemKind, docs: &str) -> IndexItem {
        let mut item = make_item(name, path, kind);
        item.docs = docs.to_string();
        item
    }

    fn build_test_index(items: Vec<IndexItem>) -> DocIndex {
        let mut index = DocIndex::new("testcrate".to_string(), "0.1.0".to_string());
        for item in items {
            index.add_item(item);
        }
        index
    }

    // ---- Exact name match scores 100 ----

    #[test]
    fn search_exact_name_match_scores_100() {
        let index = build_test_index(vec![make_item(
            "Mutex",
            "testcrate::sync::Mutex",
            ItemKind::Struct,
        )]);

        let results = search(&index, "mutex", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 100);
        assert_eq!(results[0].index, 0);
    }

    // ---- Substring match scores 75 ----

    #[test]
    fn search_substring_name_match_scores_75() {
        let index = build_test_index(vec![make_item(
            "HashMap",
            "testcrate::HashMap",
            ItemKind::Struct,
        )]);

        let results = search(&index, "map", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 75);
    }

    // ---- Signature match scores 40 ----

    #[test]
    fn search_signature_match_scores_40() {
        let index = build_test_index(vec![make_item_with_sig(
            "connect",
            "testcrate::connect",
            ItemKind::Function,
            "pub fn connect(addr: SocketAddr) -> TcpStream",
        )]);

        // "socketaddr" appears only in the signature, not in name
        let results = search(&index, "socketaddr", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 40);
    }

    // ---- Module path match scores 30 ----

    #[test]
    fn search_module_path_match_scores_30() {
        let index = build_test_index(vec![make_item(
            "Mutex",
            "testcrate::sync::Mutex",
            ItemKind::Struct,
        )]);

        // "sync" appears in the module path but not in name, signature, or docs
        let results = search(&index, "sync", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 30);
    }

    // ---- Doc match scores 20 ----

    #[test]
    fn search_doc_match_scores_20() {
        let index = build_test_index(vec![make_item_with_docs(
            "Mutex",
            "testcrate::Mutex",
            ItemKind::Struct,
            "A mutual exclusion primitive useful for protecting shared data.",
        )]);

        // "primitive" appears only in docs
        let results = search(&index, "primitive", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 20);
    }

    // ---- OR query finds both ----

    #[test]
    fn search_or_query_finds_both_terms() {
        let index = build_test_index(vec![
            make_item("Mutex", "testcrate::Mutex", ItemKind::Struct),
            make_item("RwLock", "testcrate::RwLock", ItemKind::Struct),
            make_item("Vec", "testcrate::Vec", ItemKind::Struct),
        ]);

        let results = search(&index, "Mutex | RwLock", None).unwrap();
        assert_eq!(results.len(), 2);

        let names: Vec<&str> = results
            .iter()
            .map(|r| index.items[r.index].name.as_str())
            .collect();
        assert!(names.contains(&"Mutex"));
        assert!(names.contains(&"RwLock"));
    }

    // ---- Results capped at 20 ----

    #[test]
    fn search_results_capped_at_20() {
        let items: Vec<IndexItem> = (0..30)
            .map(|i| {
                make_item_with_docs(
                    &format!("Item{i}"),
                    &format!("testcrate::Item{i}"),
                    ItemKind::Struct,
                    "searchable content here",
                )
            })
            .collect();
        let index = build_test_index(items);

        let results = search(&index, "searchable", None).unwrap();
        assert!(results.len() <= 20);
    }

    // ---- Empty query returns error ----

    #[test]
    fn search_empty_query_returns_error() {
        let index = build_test_index(vec![]);
        let result = search(&index, "", None);
        assert!(result.is_err());

        let result = search(&index, "   ", None);
        assert!(result.is_err());

        let result = search(&index, " | ", None);
        assert!(result.is_err());
    }

    // ---- Kind filter restricts results ----

    #[test]
    fn search_kind_filter_restricts_results() {
        let index = build_test_index(vec![
            make_item("Mutex", "testcrate::Mutex", ItemKind::Struct),
            make_item("mutex_new", "testcrate::mutex_new", ItemKind::Function),
        ]);

        // Without filter: both match "mutex"
        let all_results = search(&index, "mutex", None).unwrap();
        assert_eq!(all_results.len(), 2);

        // With struct filter: only Mutex
        let filtered = search(&index, "mutex", Some(ItemKind::Struct)).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(index.items[filtered[0].index].name, "Mutex");

        // With function filter: only mutex_new
        let filtered = search(&index, "mutex", Some(ItemKind::Function)).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(index.items[filtered[0].index].name, "mutex_new");
    }

    // ---- Sorting: score desc, path asc ----

    #[test]
    fn search_results_sorted_by_score_desc_then_path_asc() {
        let index = build_test_index(vec![
            make_item_with_docs(
                "Zeta",
                "testcrate::Zeta",
                ItemKind::Struct,
                "contains the word target in docs",
            ),
            make_item("target", "testcrate::target", ItemKind::Function),
            make_item("Alpha", "testcrate::Alpha", ItemKind::Struct),
        ]);
        // "target" is:
        // - exact name match for "target" function (score 100)
        // - doc match for "Zeta" (score 20)
        // - "Alpha" doesn't match

        let results = search(&index, "target", None).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].score, 100); // target (exact name)
        assert_eq!(results[1].score, 20); // Zeta (doc match)
    }

    // ---- Path-based dedup for re-exports ----

    #[test]
    fn search_deduplicates_by_path() {
        let index = build_test_index(vec![
            make_item("Mutex", "testcrate::sync::Mutex", ItemKind::Struct),
            make_item("Mutex", "testcrate::sync::Mutex", ItemKind::Struct),
        ]);

        let results = search(&index, "mutex", None).unwrap();
        // Both items have the same path — dedup should keep only the first
        assert_eq!(results.len(), 1);
    }

    // ---- AND query requires all terms ----

    #[test]
    fn search_and_query_requires_all_terms() {
        let index = build_test_index(vec![
            make_item_with_sig(
                "read_file",
                "testcrate::read_file",
                ItemKind::Function,
                "pub fn read_file(path: &Path) -> String",
            ),
            make_item("read", "testcrate::read", ItemKind::Function),
        ]);

        // "read file" requires both terms
        let results = search(&index, "read file", None).unwrap();
        // Only read_file matches both (name contains "read" and "file")
        assert_eq!(results.len(), 1);
        assert_eq!(index.items[results[0].index].name, "read_file");
    }

    // ---- Docs truncated to 500 chars for search ----

    #[test]
    fn search_docs_truncated_to_500_chars() {
        let long_docs = format!("{}unique_deep_marker", "x".repeat(600));
        let index = build_test_index(vec![make_item_with_docs(
            "LongDoc",
            "testcrate::LongDoc",
            ItemKind::Struct,
            &long_docs,
        )]);

        // "unique_deep_marker" is past 500 chars, should not be found
        let results = search(&index, "unique_deep_marker", None).unwrap();
        assert!(results.is_empty());
    }

    // ---- Tier interactions: signature overrides path but not name ----

    #[test]
    fn search_signature_overrides_path_score() {
        let mut item = make_item(
            "something",
            "testcrate::network::something",
            ItemKind::Function,
        );
        item.signature = "pub fn something(network: Network)".to_string();

        let index = build_test_index(vec![item]);

        // "network" appears in both module path (score 30) and signature (score 40)
        // Signature should win (40 > 30)
        let results = search(&index, "network", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 40);
    }

    #[test]
    fn search_name_not_overridden_by_signature() {
        let mut item = make_item("connect", "testcrate::connect", ItemKind::Function);
        item.signature = "pub fn connect()".to_string();

        let index = build_test_index(vec![item]);

        // "connect" matches name exactly (100) and signature (40)
        // Name should win (100 > 40)
        let results = search(&index, "connect", None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 100);
    }

    // ---- OR with empty groups handled gracefully ----

    #[test]
    fn search_or_with_empty_groups_handled() {
        let index = build_test_index(vec![make_item(
            "Mutex",
            "testcrate::Mutex",
            ItemKind::Struct,
        )]);

        // Leading/trailing pipes should not cause issues
        let results = search(&index, "|mutex", None).unwrap();
        assert_eq!(results.len(), 1);

        let results = search(&index, "mutex|", None).unwrap();
        assert_eq!(results.len(), 1);

        let results = search(&index, "mutex||rwlock", None).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ---- parse_query tests ----

    #[test]
    fn parse_query_single_term() {
        let groups = parse_query("read").unwrap();
        assert_eq!(groups, vec![vec!["read"]]);
    }

    #[test]
    fn parse_query_and_terms() {
        let groups = parse_query("async file").unwrap();
        assert_eq!(groups, vec![vec!["async", "file"]]);
    }

    #[test]
    fn parse_query_or_terms() {
        let groups = parse_query("read|write").unwrap();
        assert_eq!(groups, vec![vec!["read"], vec!["write"]]);
    }

    #[test]
    fn parse_query_mixed_and_or() {
        let groups = parse_query("async read|write").unwrap();
        assert_eq!(groups, vec![vec!["async", "read"], vec!["write"]]);
    }

    #[test]
    fn parse_query_empty_returns_error() {
        assert!(parse_query("").is_err());
        assert!(parse_query("   ").is_err());
        assert!(parse_query("|").is_err());
        assert!(parse_query("||").is_err());
    }

    #[test]
    fn parse_query_lowercases_input() {
        let groups = parse_query("Mutex").unwrap();
        assert_eq!(groups, vec![vec!["mutex"]]);
    }

    // ---- score_term unit tests ----

    #[test]
    fn score_term_exact_name_returns_100() {
        let entry = SearchEntry {
            name: "mutex".to_string(),
            path: "crate::mutex".to_string(),
            signature: String::new(),
            docs: String::new(),
        };
        assert_eq!(score_term(&entry, "mutex"), 100);
    }

    #[test]
    fn score_term_name_substring_returns_75() {
        let entry = SearchEntry {
            name: "hashmap".to_string(),
            path: "crate::hashmap".to_string(),
            signature: String::new(),
            docs: String::new(),
        };
        assert_eq!(score_term(&entry, "map"), 75);
    }

    #[test]
    fn score_term_signature_returns_40() {
        let entry = SearchEntry {
            name: "connect".to_string(),
            path: "crate::connect".to_string(),
            signature: "pub fn connect(addr: socketaddr)".to_string(),
            docs: String::new(),
        };
        assert_eq!(score_term(&entry, "socketaddr"), 40);
    }

    #[test]
    fn score_term_module_path_returns_30() {
        let entry = SearchEntry {
            name: "mutex".to_string(),
            path: "crate::sync::mutex".to_string(),
            signature: String::new(),
            docs: String::new(),
        };
        assert_eq!(score_term(&entry, "sync"), 30);
    }

    #[test]
    fn score_term_docs_returns_20() {
        let entry = SearchEntry {
            name: "mutex".to_string(),
            path: "crate::mutex".to_string(),
            signature: String::new(),
            docs: "thread-safe locking primitive".to_string(),
        };
        assert_eq!(score_term(&entry, "locking"), 20);
    }

    #[test]
    fn score_term_no_match_returns_0() {
        let entry = SearchEntry {
            name: "mutex".to_string(),
            path: "crate::mutex".to_string(),
            signature: String::new(),
            docs: String::new(),
        };
        assert_eq!(score_term(&entry, "zzzzz"), 0);
    }

    #[test]
    fn score_term_module_path_excludes_name_segment() {
        // "mutex" appears in the path as the last segment (the name),
        // but the module path (everything before last "::") doesn't contain "mutex"
        let entry = SearchEntry {
            name: "mutex".to_string(),
            path: "crate::sync::mutex".to_string(),
            signature: String::new(),
            docs: String::new(),
        };
        // "mutex" as a term: name exact match (100), not module path match
        assert_eq!(score_term(&entry, "mutex"), 100);
    }
}
