# Types and Data Model

All core types for groxide. This is the single source of truth for data structures.

---

## 1. ItemKind

Every documented item has exactly one kind. This single enum replaces all kind-related
logic — display names, CLI filter matching, category grouping, and crate-root prioritization.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) enum ItemKind {
    Module,
    Struct,
    Enum,
    Union,
    Trait,
    TraitAlias,
    Function,      // includes methods — rustdoc treats all functions uniformly
    TypeAlias,
    AssocType,
    ForeignType,
    Constant,
    AssocConst,
    Static,
    Macro,
    ProcMacro,
    Variant,
    Field,
    Primitive,
}
```

### ItemKind methods

```rust
impl ItemKind {
    /// User-facing short name: "fn", "struct", "mod", etc.
    fn short_name(self) -> &'static str;

    /// Whether this kind matches a CLI `--kind` filter value.
    /// Grouping rules:
    ///   fn      → Function
    ///   struct   → Struct
    ///   enum     → Enum
    ///   trait    → Trait, TraitAlias
    ///   type     → TypeAlias, AssocType, ForeignType
    ///   const    → Constant, AssocConst
    ///   mod      → Module
    ///   macro    → Macro, ProcMacro
    /// All other kinds match only themselves.
    fn matches_filter(self, filter: Self) -> bool;

    /// Maps to KindCategory for display grouping.
    fn category(self) -> KindCategory;

    /// "Primary" kinds for crate-root auto-selection (see §6 QueryResult).
    /// Primary = Struct, Enum, Union, Trait, TraitAlias, TypeAlias.
    /// When an ambiguous query at the crate root has exactly one primary match,
    /// it auto-selects to Found instead of Ambiguous.
    fn is_primary(self) -> bool;
}
```

---

## 2. KindCategory

Groups items for display in module/crate listings. Variant order defines display order.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum KindCategory {
    Modules,
    Structs,
    Enums,
    Unions,
    Traits,
    Functions,
    TypeAliases,
    Constants,
    Statics,
    Macros,
    Variants,    // rarely top-level, but needed for ItemKind::category() totality
    Fields,      // same
    Primitives,
}
```

### KindCategory methods

```rust
impl KindCategory {
    /// Section header text: "Modules:", "Structs:", "Type Aliases:", etc.
    fn header(self) -> &'static str;

    /// Whether items in this group show signature+summary (Functions, TypeAliases,
    /// Constants, Statics) vs name+summary (everything else).
    fn uses_signature_display(self) -> bool;
}
```

`BTreeMap<KindCategory, _>` auto-sorts in display order because `Ord` uses discriminant order.

---

## 3. GroupedItems

Type alias replacing the old 11-field struct. ~20 lines replaces ~400.

```rust
pub(crate) type GroupedItems<'a> = BTreeMap<KindCategory, Vec<&'a IndexItem>>;

/// Groups items by category, sorts alphabetically within each group.
pub(crate) fn group_items<'a>(items: &[&'a IndexItem]) -> GroupedItems<'a>;

/// Total item count across all categories.
pub(crate) fn grouped_items_total(groups: &GroupedItems<'_>) -> usize;
```

Empty categories are simply absent from the map.

---

## 4. IndexItem

One entry per documented item. Stored in `DocIndex.items`, serialized to cache.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IndexItem {
    pub(crate) path: String,           // "tokio::sync::Mutex"
    pub(crate) name: String,           // "Mutex"
    pub(crate) kind: ItemKind,
    pub(crate) signature: String,      // "pub struct Mutex<T: ?Sized>"
    pub(crate) docs: String,           // full doc comment, raw text (not markdown)
    pub(crate) summary: String,        // first sentence of docs
    pub(crate) span: SourceSpan,
    pub(crate) children: Vec<ChildRef>,
    pub(crate) is_public: bool,
    pub(crate) has_body: bool,         // for trait methods: true=provided, false=required
    pub(crate) feature_gate: Option<String>,  // e.g., "fs" from #[cfg(feature = "fs")]
}
```

### Associated types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChildRef {
    pub(crate) index: usize,       // index into DocIndex.items
    pub(crate) kind: ItemKind,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceSpan {
    pub(crate) file: String,       // relative path, e.g., "src/lib.rs". Empty if unavailable.
    pub(crate) line_start: u32,    // 1-based. 0 if unavailable.
    pub(crate) line_end: u32,      // 1-based, inclusive. 0 if unavailable.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TraitImplInfo {
    pub(crate) trait_path: String,   // "Clone", "std::fmt::Debug"
    pub(crate) is_synthetic: bool,   // true for auto-traits (Send, Sync, etc.)
}
```

Notes:
- `Clone` on IndexItem is needed for multi-crate search merging.
- `Serialize`/`Deserialize` for rmp-serde cache.
- Trait impls are NOT on IndexItem — they live on DocIndex (see §5).
- `TraitImplInfo` has no `bounds` or `is_blanket` fields. Blanket impls are filtered out during index building.

---

## 5. DocIndex

The queryable index for one crate. Built from rustdoc JSON, cached to disk via rmp-serde.

```rust
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DocIndex {
    pub(crate) items: Vec<IndexItem>,

    /// Full path → item indices. Original case. Multiple items can share a path
    /// (e.g., derive macro and trait with same name).
    pub(crate) path_map: HashMap<String, Vec<usize>>,

    /// Lowercased simple name → item indices.
    pub(crate) name_map: HashMap<String, Vec<usize>>,

    /// Lowercased path suffix → item indices.
    /// "tokio::sync::Mutex" generates keys: "mutex", "sync::mutex", "tokio::sync::mutex".
    pub(crate) suffix_map: HashMap<String, Vec<usize>>,

    /// Trait implementations keyed by parent type's item index.
    /// Only types with actual trait impls have entries here.
    /// Stored separately from IndexItem to avoid serializing empty Vecs
    /// on the ~95% of items that have no impls.
    pub(crate) trait_impls: HashMap<usize, Vec<TraitImplInfo>>,

    pub(crate) crate_name: String,     // normalized: hyphens → underscores
    pub(crate) crate_version: String,  // e.g., "1.0.210". Empty if unknown.
}
```

### DocIndex methods

```rust
impl DocIndex {
    fn new(crate_name: String, crate_version: String) -> Self;

    /// Adds item and updates path_map, name_map, suffix_map.
    fn add_item(&mut self, item: IndexItem);

    fn get(&self, index: usize) -> &IndexItem;
    fn len(&self) -> usize;

    /// Trait impls for an item. Returns empty slice if none.
    fn item_trait_impls(&self, index: usize) -> &[TraitImplInfo];
}
```

---

## 6. QueryResult

Output of the query engine's lookup pipeline.

```rust
#[derive(Debug)]
pub(crate) enum QueryResult {
    /// Exactly one item matched.
    Found { index: usize },

    /// Multiple items matched. Indices ordered by priority:
    /// crate-root primary items first, then crate-root other, then nested
    /// (sorted by depth ascending, then alphabetically).
    Ambiguous { indices: Vec<usize>, query: String },

    /// Nothing matched. Item name suggestions: Levenshtein ≤ 3, max 5.
    /// (Crate name suggestions use max 10 — see spec 02 §1.7.)
    NotFound { query: String, suggestions: Vec<String> },
}
```

Exit codes: `Found` → 0, `Ambiguous` → 0 (not an error), `NotFound` → 1.

### Crate-root auto-selection rule

When a query produces multiple suffix matches, auto-select to `Found` if ALL of these hold:
1. Exactly one match has a "primary" kind (`is_primary() == true`)
2. That match is at the crate root (2-segment path: `crate::Item`)
3. No other matches are also at the crate root
4. No nested match is a 3-segment primary type (e.g., `crate::module::Type`) — these are
   common enough that auto-selecting away from them would be surprising

This handles the common case: `grox serde::Deserialize` where `Deserialize` is both a
trait at `serde::Deserialize` and a derive at `serde::de::Deserialize`. The crate-root
trait wins automatically.

### Deduplication sequence

When a query produces multiple matches, three dedup stages run in order:

1. **Re-export stub resolution** — If a match points to a `pub use` stub (an item whose
   signature starts with `pub use`), resolve it to the canonical item it re-exports.
   If the canonical item is already in the result set, drop the stub.

2. **Crate-root auto-selection** — Apply the rule above. If it selects a single item,
   return `Found`.

3. **Ambiguous dedup** — Remove entries with duplicate `(path, kind)` pairs. Keep the
   first occurrence (which is the higher-priority one due to sort order).

---

## 7. QueryPath and CrateSpec

Parsed from the CLI input string. Lives in `cli.rs`.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryPath {
    pub(crate) crate_spec: CrateSpec,
    pub(crate) item_segments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CrateSpec {
    /// No path provided — query the current crate root.
    CurrentCrate,

    /// First segment is a crate name (e.g., "tokio" from "tokio::sync::Mutex").
    /// Could be a dependency, stdlib, or auto-fetched external.
    Named(String),

    /// Explicit version pin: "tokio@1.40.0" or "tokio@1.40".
    Versioned { name: String, version: String },
}
```

### Parsing grammar

```
input       = ""                           → CurrentCrate, items=[]
            | versioned ( "::" path_rest )? → Versioned { name, version }, items=[...]
            | IDENT ( "::" path_rest )?     → Named(IDENT), items=[...]
            | IDENT                          → Named(IDENT), items=[]

versioned   = IDENT "@" VERSION
VERSION     = semver string (e.g., "1.0.210", "1.40", "1.40.0-alpha.1")
```

Rules:
- Split on first `@` only: `tokio@1.40.0::sync` → name="tokio", version="1.40.0", items=["sync"]
- `crate@` (trailing @, no version) → error: "missing version after @"
- `@crate` (leading @) → error: "the @ prefix is no longer supported, use: grox crate"
- A single segment with no `@` and no `::` → `Named(segment)`, items=[]
  (the resolution layer figures out if it's a crate name or an item name)

---

## 8. CrateSource

How a crate was resolved. Determines cache paths, rustdoc flags, source root.
Lives in `resolve.rs`.

```rust
#[derive(Debug, Clone)]
pub(crate) enum CrateSource {
    CurrentCrate { name: String, version: String, manifest_dir: PathBuf },
    Dependency { name: String, version: String, manifest_dir: PathBuf },
    Stdlib { name: String },
    External { name: String, version: Option<String> },
}
```

### CrateSource methods

```rust
impl CrateSource {
    fn name(&self) -> &str;
    fn version(&self) -> Option<&str>;
    fn is_current_crate(&self) -> bool;
}
```

`External.version` starts as `None` for auto-fetch, becomes `Some` after crates.io resolution.

---

## 9. DisplayItem and DisplayLimits

Built from DocIndex + IndexItem, consumed by renderers. Never stored or serialized.

```rust
pub(crate) enum DisplayItem<'a> {
    Crate {
        item: &'a IndexItem,
        children: GroupedItems<'a>,
    },
    Module {
        item: &'a IndexItem,
        children: GroupedItems<'a>,
    },
    Type {
        item: &'a IndexItem,
        methods: Vec<&'a IndexItem>,
        variants: Vec<&'a IndexItem>,
        trait_impls: &'a [TraitImplInfo],
    },
    Trait {
        item: &'a IndexItem,
        required_methods: Vec<&'a IndexItem>,
        provided_methods: Vec<&'a IndexItem>,
    },
    Leaf {
        item: &'a IndexItem,
    },
}
```

### Variant selection rules

Given an `IndexItem` and the `DocIndex`:

```
if item.kind == Module AND item.path == crate_name:
    → DisplayItem::Crate { children = grouped public children }
elif item.kind == Module:
    → DisplayItem::Module { children = grouped public children }
elif item.kind in [Struct, Enum, Union]:
    → DisplayItem::Type { methods, variants, trait_impls from DocIndex }
elif item.kind in [Trait, TraitAlias]:
    → DisplayItem::Trait { required (has_body=false), provided (has_body=true) }
else:
    → DisplayItem::Leaf
```

When `--private` is set, children include non-public items. Otherwise only `is_public == true`.

### DisplayLimits

```rust
pub(crate) struct DisplayLimits {
    pub(crate) max_methods: usize,       // default: 15
    pub(crate) max_trait_impls: usize,   // default: 5
    pub(crate) max_doc_length: usize,    // default: 1500 chars
    pub(crate) expand_all: bool,         // --all flag disables all truncation
}
```

---

## 10. SearchResult

Minimal struct from the search engine. Index + score only — display data is looked up
from DocIndex at render time (no string cloning).

```rust
#[derive(Debug)]
pub(crate) struct SearchResult {
    pub(crate) index: usize,    // into DocIndex.items
    pub(crate) score: u32,      // higher = better. Tiers: 100/75/40/30/20
}
```

Scoring tiers (see spec 02 for full algorithm):
- 100 = exact name match
- 75 = name substring
- 40 = signature match
- 30 = module path match
- 20 = doc text match

For multi-term queries: AND terms (space-separated) sum scores (any 0 disqualifies).
OR groups (pipe `|` separated) take the maximum. See spec 02 §7 for full algorithm.
Results sorted by score descending, capped at 20.

---

## 11. Error Hierarchy

Distinct error variants. No catch-all. Each maps to exit code 1 or 2.

```rust
#[derive(Debug, Error)]
pub enum GroxError {
    #[error("not in a Rust project\n\nRun grox in a Rust project directory, or specify --manifest-path.\nTo query external crates: grox <crate>\nTo query stdlib: grox std, grox core, grox alloc")]
    ManifestNotFound,

    #[error("failed to read cargo metadata\n\n{details}")]
    CargoMetadataFailed { details: String },

    #[error("crate '{name}' not found{}", format_suggestions(suggestions))]
    CrateNotFound { name: String, suggestions: Vec<String> },

    #[error("nightly toolchain required\n\nRun: rustup toolchain install nightly")]
    NightlyNotAvailable,

    #[error("no item matching \"{query}\"{}{}", crate_ctx, format_suggestions(suggestions))]
    ItemNotFound { query: String, crate_name: Option<String>, suggestions: Vec<String> },

    #[error("rustdoc generation failed\n\n{stderr}")]
    RustdocFailed { stderr: String },

    #[error("standard library source not available\n\nRun: rustup component add rust-src")]
    StdLibSourceMissing,

    #[error("failed to fetch '{name}' from crates.io\n\n{details}")]
    ExternalFetchFailed { name: String, details: String },

    #[error("{message}")]
    InvalidQuery { message: String },

    #[error("failed to read {}: {source}", path.display())]
    JsonReadFailed { path: PathBuf, source: io::Error },

    #[error("failed to parse rustdoc JSON: {details}")]
    JsonParseFailed { details: String },

    #[error("cache error: {message}")]
    CacheSerializationFailed { message: String },

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}
```

### Exit codes

```rust
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_NOT_FOUND: i32 = 1;
pub const EXIT_ERROR: i32 = 2;

impl GroxError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::CrateNotFound { .. } | Self::ItemNotFound { .. } => EXIT_NOT_FOUND,
            _ => EXIT_ERROR,
        }
    }
}
```

Suggestions are deduped, displayed capped at 5 (with "... N more" if truncated), formatted as:
```
Did you mean:
  tokio::sync::Mutex
  tokio::sync::MutexGuard
```

---

## 12. serde_json Recursion Limit

### Problem

Crates like `typenum` generate deeply nested types that exceed serde_json's default
recursion limit (128), crashing during rustdoc JSON parsing.

### Fix

```rust
fn parse_rustdoc_json(json: &str) -> serde_json::Result<rustdoc_types::Crate> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    deserializer.disable_recursion_limit();
    rustdoc_types::Crate::deserialize(&mut deserializer)
}
```

`disable_recursion_limit()` is safe here because the input is trusted (generated by
`cargo rustdoc`, not user-supplied). The OS stack (~8MB) provides an implicit safety net.

This applies only to the rustdoc JSON parsing path. Cache deserialization (rmp-serde)
is flat and unaffected.

---

## 13. Type Location Summary

| Type | File | Visibility |
|------|------|-----------|
| `ItemKind` | `types.rs` | `pub(crate)` |
| `KindCategory` | `types.rs` | `pub(crate)` |
| `GroupedItems` (alias) | `types.rs` | `pub(crate)` |
| `IndexItem` | `types.rs` | `pub(crate)` |
| `ChildRef` | `types.rs` | `pub(crate)` |
| `SourceSpan` | `types.rs` | `pub(crate)` |
| `TraitImplInfo` | `types.rs` | `pub(crate)` |
| `DocIndex` | `types.rs` | `pub(crate)` |
| `QueryResult` | `types.rs` | `pub(crate)` |
| `DisplayItem` | `types.rs` | `pub(crate)` |
| `DisplayLimits` | `types.rs` | `pub(crate)` |
| `SearchResult` | `types.rs` | `pub(crate)` |
| `QueryPath` | `cli.rs` | `pub(crate)` |
| `CrateSpec` | `cli.rs` | `pub(crate)` |
| `KindFilter` | `cli.rs` | `pub(crate)` |
| `CrateSource` | `resolve.rs` | `pub(crate)` |
| `GroxError` | `error.rs` | `pub` |
| `Result<T>` (alias) | `error.rs` | `pub` |
