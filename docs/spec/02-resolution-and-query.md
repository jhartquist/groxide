# Resolution and Query Engine

Crate resolution, path resolution, query engine, search engine, and multi-crate search.

This spec is self-contained. The only other file you need is `01-types-and-data-model.md`
for type definitions (`QueryPath`, `CrateSpec`, `CrateSource`, `DocIndex`, `IndexItem`,
`QueryResult`, `SearchResult`, `ItemKind`, `GroxError`).

---

## 1. Crate Resolution Chain

### 1.1 ProjectContext::discover()

`ProjectContext::discover(manifest_path_override: Option<&Path>) -> Result<Self>`

Two modes:
- **Explicit path**: If `manifest_path_override` is `Some`, use that `Cargo.toml` directly.
- **Auto-discovery**: Walk up from `std::env::current_dir()` checking each directory for
  `Cargo.toml`. Stop at filesystem root, returning `GroxError::ManifestNotFound` if none found.

After locating `Cargo.toml`, run `cargo_metadata::MetadataCommand` to get the full dependency
graph, workspace info, and target directory.

**Current package determination** (three-tier fallback):

1. **Root package**: If `metadata.resolve.root` exists (non-virtual workspace), use that package.
2. **Closest workspace member to CWD**: For virtual workspaces, iterate all workspace members
   and pick the one whose manifest directory is closest to the current directory (in either
   direction: CWD inside the package, or package inside CWD). Distance = number of path
   components between them.
3. **First workspace member**: If proximity resolution fails, return the first workspace member.

**Caller integration in `lib.rs`**:

```
let ctx = if cli.manifest_path.is_some() {
    Some(ProjectContext::discover(cli.manifest_path.as_deref())?)  // error propagates
} else {
    ProjectContext::discover(None).ok()  // silently returns None
};
```

Discovery failure is non-fatal when no explicit `--manifest-path` was given. With `ctx = None`,
groxide still supports stdlib queries and auto-fetched external crates.

### 1.2 Resolution Order

Given a `CrateSpec::Named(name)`, `resolve_crate()` tries these sources in strict order:

```
1. Current crate name match (with hyphen/underscore normalization)
   -> CrateSource::CurrentCrate

2. Direct dependencies of current crate (via resolve graph, handles renames)
   -> CrateSource::Dependency

3. Workspace members
   -> CrateSource::Dependency

4. Transitive dependencies (all packages in cargo metadata)
   -> CrateSource::Dependency

5. Standard library check (std | core | alloc)
   -> CrateSource::Stdlib

6. Auto-fetch from crates.io (NEW in groxide)
   -> CrateSource::External { name, version: None }

7. If auto-fetch fails (crate not found on crates.io):
   -> GroxError::CrateNotFound with Levenshtein suggestions
```

For `CrateSpec::CurrentCrate`: resolve directly to the current package from `ProjectContext`.
If no current package, return `GroxError::ManifestNotFound`.

For `CrateSpec::Versioned { name, version }`: skip dependency resolution entirely. Go directly
to `CrateSource::External { name, version: Some(version) }`. This means `grox serde@1.0.210`
always fetches from crates.io even if serde is a project dependency at a different version.

**Fast paths** (bypass project context):
- `CrateSpec::Named("std" | "core" | "alloc")` when `ctx` is `None` -> `CrateSource::Stdlib`
  immediately.
- When `ctx` is `None` and name is not stdlib -> auto-fetch from crates.io.

### 1.3 Hyphen-to-Underscore Normalization

Cargo crate names can use hyphens (`my-crate`) but Rust code uses underscores (`my_crate`).
Two crate names match if either form equals the other:

```
function crate_names_match(query, package_name) -> bool:
    return query == package_name
        OR normalize(query) == package_name
        OR query == normalize(package_name)

function normalize(name) -> String:
    return name.replace('-', '_')
```

This applies at every resolution step: current crate, deps, workspace members, transitive deps.

### 1.4 Renamed Dependencies

Dependencies renamed via `[dependencies] foo = { package = "bar" }` are resolved through the
cargo metadata resolve graph. Each resolve node's `deps` list contains entries where `dep.name`
is the rename (used in code) and `dep.pkg` is the actual package ID. Match the query against
`dep.name` with hyphen/underscore normalization.

### 1.5 Feature Flag Handling

| Source | Default feature strategy |
|--------|------------------------|
| CurrentCrate | `--all-features` first, fallback to default features on platform build failure |
| Dependency | No feature flags (Cargo resolver uses workspace-specified features) |
| Stdlib | No feature flags (stdlib has no user-facing features) |
| External | Default features only (no `--all-features`) |

**Platform build failure detection** (triggers fallback to default features): check stderr for:
- `failed to run custom build command`
- `linker` AND `error`
- `could not find` AND `native`
- `ld: library not found` / `ld: framework not found` / `ld: cannot find`
- `Unable to find` / `not found in PATH`
- `LINK : fatal error`
- `error occurred: Command`
- `is not recognized as an internal or external command`
- `cannot specify features for packages outside of workspace`

When the user explicitly passes `--features`, `--all-features`, or `--no-default-features`,
forward those flags exactly as given. No fallback logic.

### 1.6 Auto-Fetch Flow

When step 6 of the resolution chain triggers:

```
1. Print to stderr:
   [grox] <name> not in project deps, fetching latest from crates.io...

2. Resolve version:
   a. No version specified -> query crates.io API for max_version
   b. Complete semver (e.g., "1.40.0") -> use as-is
   c. Partial semver (e.g., "1.40" or "1") -> query crates.io,
      find latest non-yanked version matching the prefix
   d. Pre-release (e.g., "1.40.0-alpha.1") -> use as-is

3. crates.io API:
   - URL: https://crates.io/api/v1/crates/{name}
   - User-Agent: groxide/<version>
   - Timeout: 10s connect + 30s read
   - Response contains crate_info.max_version and versions[] with yanked flags

4. Download:
   - URL: https://crates.io/api/v1/crates/{name}/{version}/download
   - Size limit: 500 MB max
   - Extract to <cache_dir>/<name>-<version>.tmp/
   - Strip top-level <name>-<version>/ prefix from tarball paths
   - Security: skip symlinks/hardlinks, canonicalize paths,
     validate they stay within extraction directory
   - On success: atomic rename .tmp -> final directory
   - On failure: remove .tmp directory

5. Build rustdoc JSON:
   cargo +nightly rustdoc --lib --output-format json -Z unstable-options
   (runs from extracted source directory, default features only)
   Add `-- --document-private-items` only when `--private` CLI flag is set.

6. If crate not found on crates.io (404):
   -> GroxError::CrateNotFound { name, suggestions: [] }
   -> exit code 1

7. If network/download/build failure:
   -> GroxError::ExternalFetchFailed or GroxError::RustdocFailed
   -> exit code 2
```

**Partial version matching**: `is_partial_version()` accepts 1 or 2 numeric dot-separated
components. `find_latest_matching_version()` queries all non-yanked versions from the crates.io
response and returns the highest version whose prefix matches.

### 1.7 Crate Name Suggestions

When resolution fails at the error step, compute Levenshtein distance between the query and
every package name in cargo metadata (or, outside a project, return empty suggestions).

- Normalize both names (hyphens -> underscores) before distance computation.
- Pre-filter with `could_match_within_distance` (length difference > 3 -> skip).
- Also include candidates where one name is a substring of the other.
- Threshold: distance <= 3.
- Sort by distance ascending, then alphabetically.
- Cap at 10 suggestions.

---

## 2. looks_like_item_name() Heuristic

This function determines whether a single-segment query is likely an item name (worth
searching the current crate and dependencies) or a crate name (should go to auto-fetch).
It gates multi-crate search and influences whether single-segment resolution reinterprets
a failed crate lookup as an item lookup.

```pseudocode
function looks_like_item_name(query: &str) -> bool:
    const COMMON_METHODS = [
        "clone", "default", "into", "from", "borrow", "deref",
        "format", "parse", "write", "read", "open", "close",
        "send", "recv"
    ]

    // Rule 1: Empty string -> not an item name
    if query.is_empty():
        return false

    // Rule 2: Contains hyphen -> definitely a crate name
    // Hyphens are forbidden in Rust identifiers
    if query.contains('-'):
        return false

    // Rule 3: Any uppercase character -> item name
    // PascalCase types, SCREAMING_SNAKE constants
    if query.chars().any(|c| c.is_uppercase()):
        return true

    // Rule 4: Contains underscores -> heuristic
    if query.contains('_'):
        segments = query.split('_')
        // Short, simple underscore names look like crate names: serde_json, tokio_util
        if segments.len() <= 3
           AND all segments are lowercase/digit
           AND average_segment_length <= 6:
            return false   // likely crate name
        // Complex snake_case -> likely function/method name
        return true

    // Rule 5: Single lowercase word, no underscores
    // Short words are likely method names
    if query.len() <= 4:
        return true    // "new", "len", "pop", "push"

    // Rule 6: Check common method names
    if query in COMMON_METHODS:
        return true    // "clone", "default", "parse", etc.

    // Rule 7: Default -> crate name
    // Longer single words: serde, tokio, regex, reqwest
    return false
```

**Known limitations**: Short crate names like `url` (3 chars), `syn` (3 chars), `cc` (2 chars),
`h2` (2 chars), and `nom` (3 chars) are misclassified as item names by the `len <= 4` rule.
This is acceptable: these will fail item lookup in the current crate, then fall through to
auto-fetch. The heuristic only controls whether groxide tries local lookup first, not whether
it skips auto-fetch entirely.

**Decision matrix**:

| Pattern | Result | Examples |
|---------|--------|---------|
| Contains `-` | false (crate) | `serde-json` |
| Has uppercase | true (item) | `HashMap`, `Vec`, `MAX_SIZE` |
| `_`-separated, <= 3 short segments | false (crate) | `serde_json`, `tokio_util` |
| `_`-separated, complex | true (item) | `my_longer_function_name` |
| <= 4 chars, no `_` | true (item) | `new`, `len`, `pop` |
| In COMMON_METHODS | true (item) | `clone`, `default`, `parse` |
| > 4 chars, single word | false (crate) | `serde`, `tokio`, `regex` |

---

## 3. Path Resolution Pipeline

The complete flow from CLI input to resolved item. This is the orchestration logic in
`lib.rs::resolve_item()`.

### 3.1 Full Pipeline

```pseudocode
function resolve_query(cli_input: &str, ctx: Option<&ProjectContext>):
    // Step 1: Parse the query
    query_path = QueryPath::parse(cli_input)

    // Step 2: Resolve CrateSpec to CrateSource
    (source, query_path) = resolve_crate_source(ctx, query_path)

    // Step 3: Load or build DocIndex for the resolved crate
    index = load_or_build_index(source)

    // Step 4: Resolve item within the index
    result = resolve_item(query_path, index, kind_filter, source, ctx)

    return (result, index, source)
```

### 3.2 Crate Source Resolution with Reinterpretation

```pseudocode
function resolve_crate_source(ctx, query_path):
    match query_path.crate_spec:
        CrateSpec::CurrentCrate:
            // Empty query or no crate specified -> current crate
            source = ctx.resolve_crate(CrateSpec::CurrentCrate)
            return (source, query_path)

        CrateSpec::Versioned { name, version }:
            // Explicit version pin -> always external
            source = CrateSource::External { name, version: Some(version) }
            return (source, query_path)

        CrateSpec::Named(name):
            // Try to resolve as a crate name
            match ctx.resolve_crate(CrateSpec::Named(name)):
                Ok(source):
                    return (source, query_path)
                Err(_):
                    // SINGLE-SEGMENT REINTERPRETATION:
                    // If the query has no item segments (just "Mutex", not "crate::Mutex"),
                    // and crate resolution failed, reinterpret as an item in the current crate.
                    if query_path.item_segments.is_empty():
                        new_query = QueryPath {
                            crate_spec: CrateSpec::CurrentCrate,
                            item_segments: vec![name],
                        }
                        source = ctx.resolve_crate(CrateSpec::CurrentCrate)
                        return (source, new_query)
                    else:
                        // Multi-segment query with unknown crate -> propagate error
                        // (or auto-fetch if looks_like_crate_name)
                        ...
```

The key insight: `grox Mutex` is parsed as `Named("Mutex")`. Crate resolution fails (no crate
named Mutex). Because there are no item segments, reinterpret as `CurrentCrate, items=["Mutex"]`.
The query engine then finds `Mutex` via suffix matching in the current crate's index.

For `grox serde::Deserialize`, the crate spec is `Named("serde")` with items `["Deserialize"]`.
Crate resolution succeeds (either as a dep or via auto-fetch), so no reinterpretation occurs.

### 3.3 Item Resolution (resolve_item)

Called after crate resolution and index loading. Orchestrates multiple lookup strategies.

```pseudocode
function resolve_item(query_path, index, kind_filter, source, ctx):
    segments = query_path.item_segments
    crate_name = normalize(source.name())   // hyphens -> underscores

    // Strategy 0: Empty query -> crate root module
    if segments is empty:
        return index.lookup([crate_name], None)

    // Strategy 1: Prepend crate name for exact path matching
    // Handles re-exports where item_segments is ["Deserialize"] but the
    // path in the index is "serde::Deserialize"
    full_segments = [crate_name] + segments
    result = index.lookup(full_segments, kind_filter)

    // Strategy 2: Bare segments (suffix matching, case-insensitive)
    if result is NotFound:
        result = index.lookup(segments, kind_filter)

    // Strategy 3: Relax kind filter
    // User may query --kind fn SomeModule (a module, not a function)
    if result is NotFound AND kind_filter is Some:
        unfiltered = index.lookup(segments, None)
        if unfiltered is not NotFound:
            result = unfiltered

    // Strategy 4: Method lookup
    // For "Type::method", resolve parent then search children
    if result is NotFound AND segments.len() >= 2:
        parent_segments = segments[0..n-1]
        method_name = segments[n-1]
        method_result = index.lookup_method(parent_segments, method_name, kind_filter)
        if method_result is not NotFound:
            result = method_result

    // Strategy 5: Multi-crate search (see section 8)
    if result is NotFound
       AND segments.len() == 1
       AND source is CurrentCrate
       AND looks_like_item_name(segments[0]):
        dep_result = try_multi_crate_search(segments, kind_filter, ctx)
        if dep_result is Some:
            result = dep_result

    return result
```

---

## 4. Query Engine -- Lookup Algorithm

`DocIndex::lookup()` is the core matching function. It operates on the three index maps
defined in `01-types-and-data-model.md` (section 5): `path_map`, `suffix_map`, `name_map`.

Key casing: `path_map` stores **original-case** keys. `name_map` and `suffix_map` store
**lowercased** keys. The `suffix_map` includes the full lowercased path as one of its
entries (when i=0 in the suffix generation loop), so it can serve as a case-insensitive
path lookup too.

### 4.1 Full Pseudocode

```pseudocode
function DocIndex::lookup(segments: &[&str], kind_filter: Option<ItemKind>) -> QueryResult:
    query_path = segments.join("::")
    query_lower = query_path.to_lowercase()

    // ---- Stage 1: Exact path match (original case) ----
    // path_map stores original-case keys.
    if path_map[query_path] exists:
        indices = path_map[query_path]
        filtered = apply_kind_filter(indices, kind_filter)
        // CRITICAL: Return immediately on exact path match.
        // Do NOT fall through to suffix matching.
        // Without this early return, "grox cli" would match both
        // "grox::cli" (module) and "grox::cli::Cli" (struct) via suffix.
        return classify_results(filtered, query_path)

    // ---- Stage 2: Case-insensitive path match ----
    // suffix_map contains the full lowercased path as one of its entries.
    // Use it for case-insensitive full-path lookup.
    if suffix_map[query_lower] exists:
        // Filter to only items whose full lowercased path equals query_lower
        indices = suffix_map[query_lower].filter(i =>
            items[i].path.to_lowercase() == query_lower
            AND items[i].path.split("::").count() == query_path.split("::").count())
        if indices is not empty:
            filtered = apply_kind_filter(indices, kind_filter)
            case_filtered = apply_case_sensitivity(filtered, query_path)
            if case_filtered is not empty:
                return classify_results(case_filtered, query_path)
            // If case filter removed all results, fall through to suffix matching

    // ---- Stage 3: Suffix match ----
    if suffix_map[query_lower] exists:
        indices = suffix_map[query_lower]
        filtered = apply_kind_filter(indices, kind_filter)
        case_filtered = apply_case_sensitivity(filtered, query_path)

        // Sub-step 3a: Exact suffix matches
        // "Exact suffix" = query segments match the LAST N segments of item path
        query_segments = query_lower.split("::")
        exact_suffix = case_filtered WHERE:
            item_segments = items[idx].path.split("::")
            item_segments.len() >= query_segments.len()
            offset = item_segments.len() - query_segments.len()
            item_segments[offset..].lowercased() == query_segments

        if exact_suffix is not empty:
            // Sub-step 3b: Non-duplicate preference
            // Among exact suffixes, prefer items where the query segment
            // does NOT appear earlier in the path.
            non_duplicate = exact_suffix WHERE:
                if query_segments.len() == 1:
                    query_seg = query_segments[0]
                    item_segments_lower = items[idx].path.to_lowercase().split("::")
                    offset = item_segments_lower.len() - 1
                    if offset == 0: pass  // no prefix, always non-duplicate
                    else: query_seg NOT IN item_segments_lower[..offset]
                else:
                    // Multi-segment queries always pass non-duplicate filter
                    pass

            // Sub-step 3c: Return best matches
            if non_duplicate is not empty:
                return classify_results(non_duplicate, query_path)
            return classify_results(exact_suffix, query_path)

        // Suffix map had results but none were exact suffix matches
        if case_filtered is not empty:
            return classify_results(case_filtered, query_path)

    // ---- Stage 4: Name match (single-segment only) ----
    if segments.len() == 1:
        name_lower = segments[0].to_lowercase()
        if name_map[name_lower] exists:
            indices = name_map[name_lower]
            filtered = apply_kind_filter(indices, kind_filter)
            case_filtered = apply_case_sensitivity(filtered, segments[0])
            if case_filtered is not empty:
                return classify_results(case_filtered, query_path)

    // ---- Stage 5: Not found ----
    suggestions = compute_suggestions(query_path)
    return NotFound { query: query_path, suggestions }
```

### 4.2 Case Sensitivity Rules

Inspired by `go doc`: lowercase query matches either case, uppercase matches exactly.

```pseudocode
function apply_case_sensitivity(indices: &[usize], query: &str) -> Vec<usize>:
    // Rule: if query contains NO uppercase characters -> case-insensitive (return all)
    if query.chars().all(|c| !c.is_uppercase()):
        return indices.to_vec()

    // Query has uppercase -> case-sensitive exact match
    if query contains "::":
        // Multi-segment: compare as suffix of item path (case-sensitive)
        query_segments = query.split("::")
        return indices WHERE:
            item_segments = items[idx].path.split("::")
            offset = item_segments.len() - query_segments.len()
            item_segments[offset..] == query_segments   // exact case comparison
    else:
        // Single-segment: compare item.name exactly
        return indices WHERE:
            items[idx].name == query
```

The "all-lowercase" check uses `!c.is_uppercase()`, which treats digits, underscores, and
`::` as non-uppercase. So `sync::mutex` is all-lowercase and matches case-insensitively.

**Examples**:

| Query | Behavior | Rationale |
|-------|----------|-----------|
| `mutex` | Matches `Mutex`, `mutex`, `MUTEX` | All-lowercase -> case-insensitive |
| `Mutex` | Matches only `Mutex` | Has uppercase -> exact match |
| `sync::mutex` | Matches `sync::Mutex`, `Sync::mutex` | All-lowercase -> case-insensitive |
| `sync::Mutex` | Matches only suffix `sync::Mutex` exactly | Has uppercase -> exact |
| `Map` | Does NOT match `HashMap` | Uppercase requires exact name match |

### 4.3 Kind Filter Application

`apply_kind_filter(indices, kind_filter)` removes items whose kind does not match the filter.
Uses `ItemKind::matches_filter()` which implements grouped matching:

| Filter | Matches |
|--------|---------|
| `fn` | Function |
| `struct` | Struct |
| `enum` | Enum |
| `trait` | Trait, TraitAlias |
| `type` | TypeAlias, AssocType, ForeignType |
| `const` | Constant, AssocConst |
| `mod` | Module |
| `macro` | Macro, ProcMacro |

Kind filter is applied at every lookup stage (path_map, suffix_map, name_map) immediately
after retrieving indices. The kind filter relaxation in Strategy 3 of `resolve_item()` handles
cases where the user's filter doesn't match (e.g., `--kind fn SomeModule`).

---

## 5. Query Engine -- Method Lookup

### 5.1 When Method Lookup Triggers

Strategy 4 in `resolve_item()`: when the query has 2+ segments and all previous strategies
returned NotFound.

Example: `tokio::sync::Mutex::lock` -> item_segments = `["sync", "Mutex", "lock"]`.
Strategies 1-3 fail because `lock` is stored as a child of `Mutex`, not as a top-level path.
Strategy 4 splits into parent = `["sync", "Mutex"]`, method = `"lock"`.

### 5.2 Algorithm

```pseudocode
function DocIndex::lookup_method(
    parent_segments: &[&str],
    method_name: &str,
    kind_filter: Option<ItemKind>
) -> QueryResult:

    // Resolve parent with NO kind filter (parent could be struct, enum, trait, etc.)
    parent_result = self.lookup(parent_segments, None)

    match parent_result:
        Found { index: parent_idx }:
            parent_item = self.items[parent_idx]

            // Search children for matching method name
            matching_children = parent_item.children WHERE:
                child.name.to_lowercase() == method_name.to_lowercase()
                AND (kind_filter is None OR child.kind.matches_filter(kind_filter))

            // Apply case sensitivity to matching children
            case_filtered = apply_case_sensitivity(matching_children, method_name)

            if case_filtered is not empty:
                full_path = parent_item.path + "::" + method_name
                return classify_results(case_filtered, full_path)

            // Parent found, method not found -> suggest similar method names
            return NotFound {
                query: parent_item.path + "::" + method_name,
                suggestions: compute_method_suggestions(parent_idx, method_name)
            }

        Ambiguous { .. }:
            // Ambiguous parent -> bubble up parent ambiguity as-is
            // User must disambiguate the parent first
            return parent_result

        NotFound { .. }:
            // Parent not found -> bubble up
            return parent_result
```

### 5.3 Method Suggestions

When the parent is found but no child matches:

```pseudocode
function compute_method_suggestions(parent_idx, method_name):
    method_lower = method_name.to_lowercase()
    candidates = []

    for each child in items[parent_idx].children:
        if NOT could_match_within_distance(method_lower, child.name.to_lowercase(), 3):
            continue
        distance = levenshtein_distance(method_lower, child.name.to_lowercase())
        if distance <= 3:
            candidates.push((items[parent_idx].path + "::" + child.name, distance))

    sort candidates by (distance ASC, path ASC)
    deduplicate by path
    return first 5
```

---

## 6. Query Engine -- Ambiguity and Suggestions

### 6.1 classify_results()

Converts a list of matching indices into a `QueryResult`.

```pseudocode
function classify_results(indices: &[usize], query: &str) -> QueryResult:
    if indices.is_empty():
        return NotFound { query, suggestions: compute_suggestions(query) }

    // Run deduplication sequence
    deduped = deduplicate(indices)

    if deduped.len() == 1:
        return Found { index: deduped[0] }

    // Try crate-root auto-selection
    if let Some(selected) = try_auto_select(deduped):
        return Found { index: selected }

    // Sort remaining results by priority
    sorted = sort_by_priority(deduped)
    return Ambiguous { indices: sorted, query }
```

### 6.2 Deduplication Sequence

Three stages, in order:

**Stage 1: Re-export stub resolution.** If a match points to a `pub use` stub (signature
starts with `pub use ` and has no children), resolve it to the canonical item it re-exports.
If the canonical item is already in the result set, drop the stub.

```pseudocode
function is_reexport_stub(item) -> bool:
    return item.signature.starts_with("pub use ") AND item.children.is_empty()
```

**Stage 2: Crate-root auto-selection.** See section 6.3.

**Stage 3: (path, kind) dedup.** Remove entries with duplicate `(path, kind)` pairs. Keep the
first occurrence (highest-priority due to sort order).

```pseudocode
function deduplicate_by_path_kind(indices) -> Vec<usize>:
    seen = HashSet<(String, ItemKind)>
    return indices WHERE (items[idx].path, items[idx].kind) is newly inserted into seen
```

Items with the same path but different kinds (e.g., trait `Parser` and proc-macro `Parser`)
are preserved -- they are genuinely different items.

### 6.3 Crate-Root Auto-Selection

When multiple items match, auto-select to `Found` if ALL conditions hold:

1. Exactly one match has a "primary" kind (`ItemKind::is_primary()` == true).
   Primary kinds: Struct, Enum, Union, Trait, TraitAlias, TypeAlias.
2. That match is at the crate root (2-segment path: `crate::Item`).
3. No other matches are at the crate root.
4. No nested match is a 3-level primary type (exactly 3 segments AND primary kind).

```pseudocode
function try_auto_select(indices) -> Option<usize>:
    // Categorize each item
    crate_root_primary = []   // 2 segments AND is_primary()
    crate_root_other = []     // 2 segments AND NOT is_primary()
    nested = []               // 3+ segments

    for idx in indices:
        depth = items[idx].path.split("::").count()
        if depth == 2 AND items[idx].kind.is_primary():
            crate_root_primary.push(idx)
        elif depth == 2:
            crate_root_other.push(idx)
        else:
            nested.push(idx)

    // Auto-select if exactly one crate-root primary, no other crate-root items,
    // and no 3-level primary nested items
    has_3_level_primary = nested.any(|idx|
        items[idx].path.split("::").count() == 3 AND items[idx].kind.is_primary()
    )

    if crate_root_primary.len() == 1
       AND crate_root_other.is_empty()
       AND NOT has_3_level_primary:
        return Some(crate_root_primary[0])

    return None
```

This handles common cases: `grox serde::Deserialize` where `Deserialize` is both a trait at
`serde::Deserialize` and a derive macro at `serde::de::Deserialize`. The crate-root trait wins.

### 6.4 Result Ordering

When auto-selection doesn't apply, results are sorted for the `Ambiguous` variant:

1. Crate-root primary items (sorted alphabetically by path)
2. Crate-root other items (sorted alphabetically by path)
3. Nested items (sorted by depth ascending, then alphabetically by path)

### 6.5 "Did you mean?" Suggestions

When no matches are found (`NotFound`), compute Levenshtein-based suggestions.

```pseudocode
function compute_suggestions(query: &str) -> Vec<String>:
    query_lower = query.to_lowercase()
    last_segment = last segment of query (split by "::")
    candidates = []

    for each item in self.items:
        // Pre-filter: cheap O(1) check
        if NOT could_match(query_lower, item.path.to_lowercase(), 3)
           AND NOT could_match(query_lower, item.name.to_lowercase(), 3)
           AND NOT could_match(last_segment, item.name.to_lowercase(), 3):
            skip

        // Compute actual distances
        path_dist = levenshtein(query_lower, item.path.to_lowercase())
        name_dist = levenshtein(query_lower, item.name.to_lowercase())
        seg_dist  = levenshtein(last_segment, item.name.to_lowercase())
        distance  = min(path_dist, name_dist, seg_dist)

        if distance <= 3:
            candidates.push((item.path, distance))

    sort candidates by (distance ASC, path ASC)
    deduplicate by path
    return first 5
```

**Pre-filter heuristic** (`could_match_within_distance`):

```pseudocode
function could_match_within_distance(s1, s2, max_dist) -> bool:
    // Length difference alone exceeds max distance -> impossible
    if abs(s1.len() - s2.len()) > max_dist:
        return false

    // For strings > 2 chars: different first char + length diff at limit -> unlikely
    if s1.len() > 2 AND s2.len() > 2:
        if s1[0] != s2[0] AND abs(s1.len() - s2.len()) >= max_dist:
            return false

    return true
```

**Levenshtein distance**: Standard edit distance using two-row optimization (O(min(m,n)) space).
Operations: insertion (cost 1), deletion (cost 1), substitution (cost 1 if different, 0 if same).
No early termination at threshold.

---

## 7. Search Engine

The search engine provides full-text search across all items in a crate's `DocIndex`.
Triggered by the `--search` / `-S` flag.

### 7.1 Search Index Construction

For each item in `DocIndex.items`, create a search entry with pre-lowercased fields:

```pseudocode
for each (id, item) in doc_index.items:
    name_lower     = item.name.to_lowercase()
    path_lower     = item.path.to_lowercase()
    signature_lower = item.signature.to_lowercase()
    docs_lower     = item.docs[..min(500, item.docs.len())].to_lowercase()
```

Key details:
- Docs truncated to first 500 characters for search matching. Full docs remain in `DocIndex`.
- All fields pre-lowercased at construction time. All search matching is case-insensitive.

### 7.2 Query Parsing

Queries support two combinators:
- **Whitespace** = AND (all terms must match)
- **Pipe `|`** = OR (any group must match)

Pipe binds more loosely than whitespace. No grouping syntax.

```pseudocode
function parse_query(query: &str) -> Vec<Vec<String>>:
    query_lower = query.to_lowercase()
    or_parts = query_lower.split('|')

    result = []
    for each or_part in or_parts:
        and_terms = or_part.split_whitespace()
        if and_terms is not empty:
            result.push(and_terms)

    return result   // outer = OR groups, inner = AND terms
```

| Input | Parsed |
|-------|--------|
| `"read"` | `[["read"]]` |
| `"async file"` | `[["async", "file"]]` |
| `"read\|write"` | `[["read"], ["write"]]` |
| `"async read\|write"` | `[["async", "read"], ["write"]]` |

Empty terms are ignored: `"\|read"`, `"read\|"`, `"read\|\|write"` are handled gracefully.
A query of just `"\|"` or empty string -> `GroxError::InvalidQuery`.

No phrase matching. `"async fn"` is two AND terms -- items must contain both "async" and "fn"
somewhere, not necessarily adjacent.

### 7.3 Scoring Algorithm

**Single-term scoring** (`score_term`):

```pseudocode
function score_term(entry, term: &str) -> u32:
    score = 0

    // Tier 1-2: Name matching (mutually exclusive)
    if entry.name_lower == term:
        score = 100                         // exact name match
    else if entry.name_lower.contains(term):
        score = 75                          // name substring

    // Tier 3: Module path matching (only if no name match)
    if score == 0 AND entry.path_lower.contains(term):
        module_path = everything before last "::" in entry.path_lower
        if module_path exists AND module_path.contains(term):
            score = 30

    // Tier 4: Signature matching (overrides path, not name)
    if entry.signature_lower.contains(term) AND score < 40:
        score = 40

    // Tier 5: Docs matching (only if nothing else matched well)
    if entry.docs_lower.contains(term) AND score < 20:
        score = 20

    return score
```

Tier interactions:
- Name exact (100) and name substring (75) are mutually exclusive (else-if).
- Module path (30) only checked when name didn't match (score == 0 guard).
- Signature (40) overrides module path (40 > 30) but not name (40 < 75).
- Docs (20) only contributes if nothing else matched (score < 20).

**AND combination**: Sum scores across AND terms. Any term scoring 0 disqualifies the entry.

```pseudocode
function score_and_terms(entry, terms: &[String]) -> u32:
    total = 0
    for each term in terms:
        s = score_term(entry, term)
        if s == 0: return 0     // ALL terms must match
        total += s
    return total
```

**OR combination**: Take maximum across OR groups.

```pseudocode
function score_entry(entry, or_groups: &[Vec<String>]) -> u32:
    return max(score_and_terms(entry, group) for each group in or_groups)
```

### 7.4 Sorting and Truncation

1. Score all entries. Filter by kind_filter before scoring (kind filter applied first as a
   performance optimization -- scoring is the expensive part).
2. Sort results: **score descending**, then **path ascending** (alphabetical tiebreaker).
3. Record `total_count` (total matches before truncation).
4. Truncate to **20** results.

### 7.5 Deduplication

After truncation, deduplicate by path. First occurrence (highest score) wins.

```pseudocode
function deduplicate_search_results(results, total_count):
    seen = HashSet<String>
    deduped = results WHERE seen.insert(result.path) is true

    // Estimate deduplicated total proportionally
    if results is empty:
        deduped_total = 0
    else:
        ratio = deduped.len() / results.len()
        deduped_total = ceil(total_count * ratio)

    return (deduped, deduped_total)
```

### 7.6 Edge Cases

- **Empty/whitespace-only query**: Return `GroxError::InvalidQuery` before search begins.
- **No results**: Return `([], 0)`. Exit code 0 (not an error).
- **Very long docs**: Only first 500 chars indexed. Matches deep in docs are not found by
  search (still findable by name/path/signature).
- **Search is single-crate**: The `--search` flag operates on one `DocIndex` at a time.
  To search a dependency: `grox tokio --search "spawn"`.

---

## 8. Multi-Crate Search

### 8.1 Trigger Conditions

Multi-crate search across dependencies activates ONLY when ALL conditions hold:

1. The query returned `NotFound` in the current crate's index.
2. `item_segments` has exactly 1 element (single name, not a path).
3. The crate source is `CurrentCrate` (not an explicit dependency or external query).
4. `looks_like_item_name(segments[0])` returns true.

### 8.2 Algorithm

```pseudocode
function try_multi_crate_search(segments, kind_filter, ctx):
    // Print status to stderr
    eprintln!("[grox] searching dependencies...")

    results = []

    // Search all DIRECT dependencies (not transitive)
    for each direct_dep in ctx.direct_dependencies():
        // Build or load the dep's DocIndex (graceful per-dep failure)
        match load_or_build_index(dep_source):
            Ok(dep_index):
                result = dep_index.lookup(segments, kind_filter)
                if result is Found or Ambiguous:
                    results.push(MultiCrateResult {
                        source: dep_source,
                        index: dep_index,
                        result: result,
                    })
            Err(_):
                continue   // skip this dep, try next
```

### 8.3 Result Aggregation

| Dep Matches | Behavior |
|-------------|----------|
| 0 dependencies matched | Return None (fall back to NotFound from current crate) |
| 1 dependency matched | Return that dep's (result, index, source) directly |
| 2+ dependencies matched | Merge all matching items into a single Ambiguous result |

For the 2+ case: combine all matching item indices into a unified `DocIndex` (clone items
from each dep index into a merged index). Return as `Ambiguous` with items from multiple crates.

### 8.4 Interaction with Auto-Fetch

Multi-crate search and auto-fetch are distinct features:
- **Multi-crate search**: triggered for item-like names (`Mutex`), searches existing dep indices.
- **Auto-fetch**: triggered for crate-like names (`serde`), downloads from crates.io.

They never conflict because `looks_like_item_name()` returns true for the multi-crate path
and false for the auto-fetch path (for the same query, the opposite heuristic applies at the
crate resolution step).

The resolution sequence for a single-segment query is:

```
1. Parse as CrateSpec::Named(name)
2. Try crate resolution (deps, workspace, stdlib) -> not found
3. Reinterpret as item in current crate -> try lookup -> not found
4. looks_like_item_name(name)?
   YES -> multi-crate search across dependencies
   NO  -> auto-fetch from crates.io
5. If multi-crate search finds nothing: return NotFound
6. If auto-fetch fails: return CrateNotFound
```

The key ordering: local item search ALWAYS runs before auto-fetch or multi-crate search.
