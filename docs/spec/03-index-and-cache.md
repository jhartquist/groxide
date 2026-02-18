# Index Building, Cache Strategy, and Crate Fetching

How groxide builds a `DocIndex` from rustdoc JSON, caches it to disk, fetches external crates
from crates.io, and resolves stdlib crate sources.

Depends on: `01-types-and-data-model.md` (for `DocIndex`, `IndexItem`, `ItemKind`, `ChildRef`,
`SourceSpan`, `TraitImplInfo`, `CrateSource`, `GroxError`).

---

## 1. Rustdoc JSON Generation

### 1.1 Base Command

All rustdoc invocations use the nightly toolchain:

```
cargo +nightly rustdoc --lib --output-format json -Z unstable-options
```

The `--lib` flag is always present except when building the current workspace crate
(where `-p <name>` within a workspace context already selects the library target).

### 1.2 Per-Source Variations

| Source | Working Dir | Package Selection | Feature Strategy |
|--------|------------|-------------------|------------------|
| `CurrentCrate` | workspace root | `-p <name>` | `--all-features`, fallback to default on platform failure |
| `Dependency` | workspace root | `-p <name>` | None — let cargo resolver unify features from `Cargo.toml` |
| `Stdlib` | N/A | `--manifest-path <path>` + `--target-dir <dir>` | None (stdlib has no user-facing features) |
| `External` | extracted source dir | None (single crate) | Default features only |

### 1.3 Private Items

When `--private` is passed on the groxide CLI, append `-- --document-private-items` to the
rustdoc invocation. Without `--private`, omit it — the default `cargo rustdoc` output only
includes public items.

### 1.4 Current Crate Feature Fallback

When building with `--all-features` and the command fails, check stderr for platform build
failure patterns before retrying with default features:

```
PATTERNS (any match triggers fallback):
  "failed to run custom build command"
  "linker" AND "error"
  "could not find" AND "native"
  "ld: library not found"
  "ld: framework not found"
  "ld: cannot find"
  "Unable to find"
  "not found in PATH"
  "LINK : fatal error"
  "error occurred: Command"
  "is not recognized as an internal or external command"
  "cannot specify features for packages outside of workspace"
```

If stderr matches any pattern, retry without `--all-features`. If the retry also fails,
return `GroxError::RustdocFailed` with the stderr from the second attempt.

If stderr does not match any pattern, return `GroxError::RustdocFailed` immediately (the
failure is not platform-related and retrying won't help).

### 1.5 User-Specified Feature Flags

When the user passes `--features`, `--all-features`, or `--no-default-features` on the
groxide CLI, forward the exact flags to `cargo rustdoc` with no fallback logic.

### 1.6 JSON Output Path

The rustdoc JSON file is written to:

```
<target_dir>/doc/<crate_name>.json
```

Where `<crate_name>` has hyphens converted to underscores: `rmp-serde` produces
`rmp_serde.json`. The `<target_dir>` comes from cargo metadata for project crates,
or is explicitly set for stdlib/external crates.

### 1.7 Nightly Detection

Before any rustdoc invocation, verify the nightly toolchain is available:

```
rustup run nightly rustc --version
```

If this command fails (exit code != 0), return `GroxError::NightlyNotAvailable`.

### 1.8 Status Messages

All status messages go to stderr with `[grox]` prefix:

```
[grox] Building index for serde 1.0.210...
[grox] Build with --all-features failed, retrying with default features...
[grox] Building index for serde 1.0.210... done (3.2s)
```

---

## 2. Index Builder — Overview

### 2.1 Input

`rustdoc_types::Crate` parsed from rustdoc JSON. The JSON is parsed with
`serde_json::Deserializer::from_str` with recursion limit disabled
(see `01-types-and-data-model.md` §12) to handle deeply nested types.

### 2.2 Output

`DocIndex` (see `01-types-and-data-model.md` §5) — a fully populated index with all items,
lookup maps, and trait impl information.

### 2.3 Algorithm Structure

The builder runs four sequential passes:

```
Pass 1: build_parent_map()        — child→parent reverse lookup
Pass 2: compute_paths()           — full path for every item
           ├── seed from krate.paths
           ├── hoist_glob_reexport_paths()
           ├── compute_impl_paths()
           ├── compute_trait_item_paths()
           └── reconstruct_path()  — fallback: walk parent chain
Pass 3: convert_items()           — rustdoc Item → IndexItem, populate maps
Pass 4: link_relationships()      — attach children + trait impls to DocIndex
```

### 2.4 Builder State

```rust
struct IndexBuilder<'a> {
    krate: &'a Crate,
    index: DocIndex,
    id_to_index: HashMap<Id, usize>,     // rustdoc Id → items[] index
    id_to_path: HashMap<Id, String>,     // rustdoc Id → full path string
    blanket_impl_items: HashSet<Id>,     // IDs to exclude (auto-generated methods)
    child_to_parent: HashMap<Id, Id>,    // reverse lookup for path reconstruction
}
```

---

## 3. Index Builder — Pass 1: Parent Map

**Purpose:** Create a reverse lookup `child_to_parent: HashMap<Id, Id>` so that
`reconstruct_path` can walk up the tree when `krate.paths` lacks an entry.

**Algorithm:**

```pseudocode
FOR (parent_id, item) IN krate.index:
    child_ids = MATCH item.inner:
        Module(m)  → m.items
        Struct(s)  → s.fields (if StructKind::Plain) ∪ flatten(s.impls → impl.items)
        Enum(e)    → e.variants ∪ flatten(e.impls → impl.items)
        Union(u)   → flatten(u.impls → impl.items)
        Trait(t)   → t.items
        Impl(i)    → i.items
        _          → ∅

    FOR child_id IN child_ids:
        child_to_parent[child_id] = parent_id
```

For structs/enums/unions, impl blocks are resolved eagerly: look up each `impl_id` in
`krate.index`, extract `impl_data.items`, and record those method IDs as children of the
type. This means a method's parent is the **type**, not the impl block.

---

## 4. Index Builder — Pass 2: Path Computation

### 4.1 Seed from `krate.paths`

```pseudocode
FOR (id, summary) IN krate.paths:
    id_to_path[id] = summary.path.join("::")
```

This gives paths for all exported items (modules, types, functions at module level). Items
not in `krate.paths` (methods, associated items, private items) must have their paths
reconstructed in subsequent steps.

### 4.2 Glob Re-export Path Hoisting

**Purpose:** When `mod foo` contains `pub use bar::*;`, items from `bar` become addressable
at `foo`'s path level.

**Must run before impl path computation** so method paths inherit the hoisted parent path.

```pseudocode
overrides = []

FOR (id, item) IN krate.index WHERE item IS Module:
    parent_path = id_to_path[id] OR CONTINUE

    // Sort child IDs for deterministic processing order
    sorted_children = SORT(module.items)

    FOR child_id IN sorted_children:
        child = krate.index[child_id] OR CONTINUE
        IF child IS Use AND use_item.is_glob:
            target_id = use_item.id OR CONTINUE
            target = krate.index[target_id] OR CONTINUE
            IF target IS Module:
                // Sort target children for deterministic output
                sorted_target_children = SORT(target_module.items)
                FOR tc_id IN sorted_target_children:
                    tc = krate.index[tc_id] OR CONTINUE
                    tc_name = tc.name OR CONTINUE
                    overrides.PUSH(tc_id, "{parent_path}::{tc_name}")

FOR (id, path) IN overrides:
    id_to_path[id] = path    // overwrites existing paths
```

**Determinism requirement:** Process glob re-export items in sorted order (by item ID or
name) to ensure deterministic output across runs. Without sorting, `HashMap` iteration order
is non-deterministic, meaning the same crate could produce different paths on different runs.

### 4.3 Impl Block Path Computation

**Purpose:** Methods inside `impl Foo { fn bar() {} }` don't appear in `krate.paths`. Their
path is `Foo::bar`.

```pseudocode
new_paths = []

FOR item IN krate.index.values() WHERE item IS Impl:
    parent_path = resolve_type_path(impl.for_) OR CONTINUE
    FOR child_id IN impl.items:
        IF child_id NOT IN id_to_path:
            child = krate.index[child_id] OR CONTINUE
            child_name = child.name OR CONTINUE
            new_paths.PUSH(child_id, "{parent_path}::{child_name}")

FOR (id, path) IN new_paths:
    id_to_path.entry(id).or_insert(path)    // does NOT overwrite
```

**`resolve_type_path(ty)`:**

```pseudocode
MATCH ty:
    Type::ResolvedPath(path) → id_to_path[path.id] OR FALLBACK path.path
    _                        → None
```

Only `Type::ResolvedPath` is handled. Impl blocks for primitives, tuples, slices, etc.
will not have their methods' paths computed here — they fall through to `reconstruct_path`.

### 4.4 Trait Item Path Computation

**Purpose:** Methods inside `trait Foo { fn bar(); }` don't always appear in `krate.paths`.

```pseudocode
new_paths = []

FOR (trait_id, item) IN krate.index WHERE item IS Trait:
    trait_path = id_to_path[trait_id] OR CONTINUE
    FOR child_id IN trait.items:
        IF child_id NOT IN id_to_path:
            child = krate.index[child_id] OR CONTINUE
            child_name = child.name OR CONTINUE
            new_paths.PUSH(child_id, "{trait_path}::{child_name}")

FOR (id, path) IN new_paths:
    id_to_path.entry(id).or_insert(path)    // does NOT overwrite
```

### 4.5 Fallback: Path Reconstruction

For items still without a path after steps 4.1–4.4, walk up the `child_to_parent` chain:

```pseudocode
FUNCTION reconstruct_path(id, item) → Option<String>:
    item_name = item.name OR RETURN None
    segments = [item_name]
    current_id = id
    depth = 0

    LOOP:
        depth += 1
        IF depth > 20: BREAK    // prevent infinite loops from cyclic references

        parent_id = child_to_parent[current_id] OR BREAK
        parent_item = krate.index[parent_id] OR BREAK

        IF id_to_path[parent_id] EXISTS:
            RETURN Some("{parent_path}::{segments.join('::')}")

        parent_name = parent_item.name OR BREAK
        segments.INSERT_FRONT(parent_name)
        current_id = parent_id

    RETURN Some(segments.join("::"))
```

**Max depth:** 20 levels. This prevents infinite loops from cyclic parent references in
malformed rustdoc JSON.

---

## 5. Index Builder — Pass 3: Item Conversion

### 5.1 Blanket Impl Filtering

Before converting items, collect all item IDs that belong to blanket or synthetic impl
blocks. These are auto-generated methods like `borrow`, `into`, `try_from` that add noise.

```pseudocode
FOR item IN krate.index.values() WHERE item IS Impl:
    IF impl.trait_ IS SOME AND (impl.blanket_impl IS SOME OR impl.is_synthetic):
        FOR child_id IN impl.items:
            blanket_impl_items.INSERT(child_id)
```

### 5.2 Item Conversion

```pseudocode
items_to_convert = krate.index.entries()
SORT items_to_convert BY id    // deterministic order

FOR (id, item) IN items_to_convert:
    IF id IN blanket_impl_items: SKIP

    IF item IS Use:
        index_item = convert_use_item(id, item)    // see §5.5
    ELSE:
        path = id_to_path[id] OR SKIP
        name = item.name OR SKIP
        kind = convert_item_kind(item.inner) OR SKIP
        signature = render_signature(item, krate) OR fallback_signature(visibility, kind, name)
        docs = item.docs OR ""
        summary = extract_summary(docs)
        span = extract_span(item)
        is_public = check_visibility(item)
        has_body = MATCH item.inner: Function(f) → f.has_body, _ → false
        feature_gate = extract_feature_gate(item)

        index_item = IndexItem {
            path, name, kind, signature, docs, summary, span,
            children: [],    // filled in Pass 4
            is_public, has_body, feature_gate
        }

    IF index_item IS SOME:
        idx = index.items.len()
        id_to_index[id] = idx
        index.add_item(index_item)    // updates path_map, name_map, suffix_map
```

### 5.3 `ItemEnum` → `ItemKind` Mapping

| `ItemEnum` variant | `ItemKind` |
|--------------------|------------|
| `Module` | `Module` |
| `Struct` | `Struct` |
| `Enum` | `Enum` |
| `Union` | `Union` |
| `Trait` | `Trait` |
| `TraitAlias` | `TraitAlias` |
| `Function` | `Function` |
| `TypeAlias` | `TypeAlias` |
| `AssocType` | `AssocType` |
| `AssocConst` | `AssocConst` |
| `Constant` | `Constant` |
| `Static` | `Static` |
| `Macro` | `Macro` |
| `ProcMacro` | `ProcMacro` |
| `Variant` | `Variant` |
| `StructField` | `Field` |
| `ExternType` | `ForeignType` |
| `Primitive` | `Primitive` |
| `Impl` | Skip — traversed for children/impls only |
| `Use` | Handled by `convert_use_item` |
| All others | Skip (return `None`) |

### 5.4 Visibility Rules

```pseudocode
FUNCTION check_visibility(item) → bool:
    MATCH item.visibility:
        Public                                    → true
        Default AND item IS Variant               → true   // enum variants are implicitly public
        Default AND item IS Function              → true   // trait methods use Default visibility
        Default                                   → false
        Crate | Restricted                        → false
```

**Caveat:** The `Default + Function → true` heuristic treats all `Default`-visibility
functions as public. This is safe when `cargo rustdoc` is invoked without
`--document-private-items` (the default), because only public items appear in the output.
When `--private` is used, private functions with `Default` visibility will be incorrectly
marked as public — acceptable for the private-items use case.

### 5.5 Re-export Handling (`pub use`)

Re-exports are handled by `convert_use_item` instead of the standard conversion path.

**Filter conditions (skip if any are true):**
- `use_item.is_glob` — glob re-exports are handled by path hoisting (§4.2) and module
  child resolution (§5 Pass 4), not as standalone items
- Item is not public
- `use_item.name` is empty

**Deduplication:** If the referenced item (`use_item.id`) already has a path in `id_to_path`,
only create the re-export if its path differs from the original. This avoids duplicate entries
for same-crate re-exports.

**Path construction:**
1. Try `id_to_path[id]` (the Use item's own ID)
2. Fallback: `"{root_path}::{name}"` where `root_path` is `id_to_path[krate.root]`

**Kind resolution:**
1. Try looking up the referenced item in `krate.index` and converting its `ItemEnum`
2. Try looking up the referenced item in `krate.paths` and converting its summary kind
3. Fallback: `ItemKind::Struct` (most common re-export target)

**Synthetic fields:**
- `docs`: item's own docs, or empty string as fallback
- `signature`: `"pub use {source} as {name}"`
- `is_public`: always `true`
- `has_body`: always `false`

### 5.6 Summary Extraction

```pseudocode
FUNCTION extract_summary(docs: &str) → String:
    IF docs IS EMPTY: RETURN ""

    chars = docs.char_indices()

    FOR (byte_pos, ch) IN chars:
        IF ch == '!' OR ch == '?':
            // If followed by whitespace → sentence boundary
            IF next char is whitespace:
                RETURN docs[..byte_pos+ch.len()]

        IF ch == '.':
            IF at end of string:
                RETURN docs[..byte_pos+1]
            IF next char is whitespace:
                // Look ahead: if next non-whitespace char is uppercase → boundary
                // This avoids breaking on "version 1.56.0" or "U.S. patent"
                IF next non-whitespace char is uppercase:
                    RETURN docs[..byte_pos+1]

    // No sentence terminator found: take first line
    first_line = docs.split('\n').next()
    IF first_line.len() > 100:
        RETURN first_line[..100] + "..."
    ELSE:
        RETURN first_line
```

### 5.7 Source Span Extraction

```pseudocode
FUNCTION extract_span(item) → SourceSpan:
    IF item.span IS SOME:
        RETURN SourceSpan {
            file: span.filename.to_string(),
            line_start: span.begin.0 as u32,    // saturating to u32
            line_end: span.end.0 as u32,
        }
    ELSE:
        RETURN SourceSpan { file: "", line_start: 0, line_end: 0 }
```

---

## 6. Index Builder — Pass 4: Children and Relationships

### 6.1 Child Resolution

For each item in `krate.index` that has an entry in `id_to_index`:

```pseudocode
FOR (id, item) IN krate.index:
    parent_idx = id_to_index[id] OR CONTINUE

    child_ids = MATCH item.inner:
        Module(m)      → resolve_module_children(m)    // see §6.2
        Struct(s)      → s.fields (if Plain) ∪ resolve_inherent_impl_items(s.impls)
        Enum(e)        → e.variants ∪ resolve_inherent_impl_items(e.impls)
        Union(u)       → resolve_inherent_impl_items(u.impls)
        Trait(t)       → t.items
        _              → ∅

    children: Vec<ChildRef> = []
    FOR child_id IN child_ids:
        child_idx = id_to_index[child_id] OR CONTINUE
        child_item = index.items[child_idx]
        children.PUSH(ChildRef {
            index: child_idx,
            kind: child_item.kind,
            name: child_item.name.clone()
        })

    IF children NOT EMPTY:
        index.items[parent_idx].children = children

    // Trait impls: see §6.3
    trait_impls = extract_trait_impls(item)
    IF trait_impls NOT EMPTY:
        index.trait_impls[parent_idx] = trait_impls
```

### 6.2 Module Child Resolution with Glob Expansion

For modules, expand glob re-exports inline so that `pub use submodule::*;` shows the
submodule's items as direct children:

```pseudocode
FUNCTION resolve_module_children(module) → Vec<Id>:
    result = []
    FOR child_id IN module.items:
        child = krate.index[child_id] OR CONTINUE
        IF child IS Use AND use_item.is_glob:
            target_id = use_item.id OR CONTINUE
            target = krate.index[target_id] OR CONTINUE
            IF target IS Module:
                result.EXTEND(target.items)
            // skip the Use item itself
        ELSE:
            result.PUSH(child_id)
    RETURN result
```

### 6.3 Inherent Impl Item Resolution

`resolve_inherent_impl_items(impls)` returns child IDs from impl blocks, but **only for
inherent impls** (where `impl_data.trait_` is `None`). Trait impl items are handled
separately via `extract_trait_impls`.

```pseudocode
FUNCTION resolve_inherent_impl_items(impl_ids) → Vec<Id>:
    result = []
    FOR impl_id IN impl_ids:
        impl_item = krate.index[impl_id] OR CONTINUE
        IF impl_item IS Impl AND impl_data.trait_ IS NONE:
            result.EXTEND(impl_data.items)
    RETURN result
```

### 6.4 Trait Impl Extraction

For structs, enums, and unions, extract trait implementation information:

```pseudocode
FUNCTION extract_trait_impls(item) → Vec<TraitImplInfo>:
    impls_list = MATCH item.inner:
        Struct(s) → s.impls
        Enum(e)   → e.impls
        Union(u)  → u.impls
        _         → RETURN []

    result = []
    FOR impl_id IN impls_list:
        impl_item = krate.index[impl_id] OR CONTINUE
        IF impl_item IS NOT Impl: CONTINUE
        IF impl_data.trait_ IS NONE: CONTINUE    // inherent impl, skip
        IF impl_data.blanket_impl IS SOME: CONTINUE    // filter out blanket impls

        trait_ref = impl_data.trait_
        result.PUSH(TraitImplInfo {
            trait_path: trait_ref.path,
            is_synthetic: impl_data.is_synthetic,
        })

    RETURN result
```

**Blanket impl filtering:** Any impl where `impl_data.blanket_impl` is `Some` is excluded.
This removes auto-generated impls like `impl<T> From<T> for T` that are noise for
documentation queries. Synthetic impls (auto-traits like `Send`, `Sync`) are kept but
marked with `is_synthetic: true`.

---

## 7. Feature Gate Extraction

Feature gates are extracted from rustdoc JSON's `item.attrs` field.

```pseudocode
FUNCTION extract_feature_gate(item) → Option<String>:
    FOR attr IN item.attrs:
        // Match pattern: #[doc(cfg(feature = "feature_name"))]
        IF attr MATCHES regex `#\[doc\(cfg\(feature\s*=\s*"([^"]+)"\)\)\]`:
            RETURN Some(captured_group_1)
    RETURN None
```

Items without feature gate attributes have `feature_gate: None`.

Feature gate information is used during rendering to annotate items with their required
features (e.g., `[feature = "fs"]` annotation on tokio items).

---

## 8. Suffix Map Population

When `DocIndex::add_item()` is called, the three lookup maps are updated:

### 8.1 path_map

Key: original-case full path. Value: list of item indices.

```pseudocode
path_map.entry(item.path.clone()).or_default().push(index)
```

### 8.2 name_map

Key: lowercased simple name. Value: list of item indices.

```pseudocode
name_map.entry(item.name.to_lowercase()).or_default().push(index)
```

### 8.3 suffix_map

Every item generates N entries where N is the number of path segments.

```pseudocode
lower_path = item.path.to_lowercase()
segments = lower_path.split("::")

FOR i IN 0..segments.len():
    suffix = segments[i..].join("::")
    suffix_map.entry(suffix).or_default().push(index)
```

Example: `"tokio::sync::Mutex"` generates:
- `"mutex"` → [index]
- `"sync::mutex"` → [index]
- `"tokio::sync::mutex"` → [index]

---

## 9. Signature Rendering

### 9.1 Entry Point

```rust
fn render_signature(item: &rustdoc_types::Item, krate: &Crate) -> Option<String>
```

Returns `None` for items without a renderable signature (impl blocks, use items).
If `render_signature` returns `None`, the caller constructs a fallback:
`"{visibility}{kind_short_name} {name}"`.

### 9.2 Per-Kind Rendering Rules

**Struct:**
```
{visibility}struct {Name}{generics}{fields}
```
Fields rendering depends on `StructKind`:
- `Plain`: `{ pub field1: Type, pub field2: Type }` (public fields inline; private fields
  shown as `/* private fields */` if any exist)
- `Tuple`: `({Type1}, {Type2})`
- `Unit`: (nothing after generics)

**Enum:**
```
{visibility}enum {Name}{generics}
```
Variants are not shown in the signature (they appear as children).

**Union:**
```
{visibility}union {Name}{generics} { {fields} }
```

**Trait:**
```
{visibility}trait {Name}{generics}: {SuperTraits}
```
SuperTraits omitted if empty.

**Function:**
```
{visibility}[const ][async ][unsafe ][extern "{ABI}" ]fn {name}{generics}({params}) -> {ReturnType}
```
Where:
- `const`, `async`, `unsafe` are present only when true
- `extern "ABI"` present only when ABI is not the default (`"Rust"`)
- Return type omitted when the return is unit `()`
- Parameters include `self`/`&self`/`&mut self` as first param when present

**TypeAlias:**
```
{visibility}type {Name}{generics} = {Type}
```

**Constant:**
```
{visibility}const {NAME}: {Type}
```

**Static:**
```
{visibility}static [mut ]{NAME}: {Type}
```

**Macro:**
```
macro_rules! {name}
```

**ProcMacro:**
```
{visibility}{proc_macro_kind} {name}
```
Where `proc_macro_kind` is one of: `#[proc_macro]`, `#[proc_macro_derive]`,
`#[proc_macro_attribute]`.

**Module:**
```
{visibility}mod {name}
```

**Variant:** depends on `VariantKind`:
- `Plain`: `{Name}`
- `Tuple`: `{Name}({Type1}, {Type2})`
- `Struct`: `{Name} { {field1}: {Type1}, {field2}: {Type2} }`

**Field:**
```
{visibility}{name}: {Type}
```

**AssocType:**
```
type {Name}{generics}[: {Bounds}][ = {Type}]
```

**AssocConst:**
```
const {NAME}: {Type}
```

### 9.3 Type Rendering

The signature renderer recursively renders `rustdoc_types::Type` variants:

| `Type` variant | Rendered as |
|----------------|-------------|
| `ResolvedPath` | `path.path` string (e.g., `"Vec<T>"`) |
| `Generic` | The generic parameter name (e.g., `"T"`) |
| `Primitive` | The primitive name (e.g., `"i32"`, `"bool"`) |
| `BorrowedRef` | `&[lifetime] [mut] Type` |
| `RawPointer` | `*const Type` or `*mut Type` |
| `Tuple` | `(T1, T2, ...)` or `()` for unit |
| `Slice` | `[T]` |
| `Array` | `[T; N]` |
| `ImplTrait` | `impl Bound1 + Bound2` |
| `DynTrait` | `dyn Trait + Bounds` |
| `FunctionPointer` | `[for<...>] [unsafe] [extern "ABI"] fn(params) -> Ret` |
| `QualifiedPath` | `<Self as Trait>::Name` |
| `Infer` | `_` |
| `Pat` | Renders the inner type |

### 9.4 Generics Rendering

```
<{lifetime_params}, {type_params}, {const_params}>{where_clause}
```

- Lifetime params: `'a`, `'b: 'a` (with outlives bounds)
- Type params: `T`, `T: Clone`, `T: Clone + Send`, `T = DefaultType`
- Const params: `const N: usize`
- Where clause: ` where T: Display, U: Into<String>` (only when non-empty)

Omit the `<...>` entirely when there are no generic parameters.

---

## 10. Cache Strategy

### 10.1 Cache File Format

Each `.groxide` cache file contains a `CacheHeader` followed by the `DocIndex`, both
serialized with rmp-serde (MessagePack):

```rust
struct CachedData {
    header: CacheHeader,
    index: DocIndex,
}

struct CacheHeader {
    grox_version: String,          // e.g., "0.1.0"
    format_version: u32,           // bump when serialization format changes
    created_at: u64,               // UNIX epoch seconds
    metadata: CacheMetadata,       // source-specific invalidation data
}

enum CacheMetadata {
    CurrentCrate { newest_source_mtime: u64 },
    Dependency { package_version: String },
    StdLib { toolchain_version: String },
    External { crate_version: String },
}
```

### 10.2 Cache Directories

**Project cache** (for current crate and its dependencies):
```
<workspace_root>/target/groxide/
```

**Global cache** (for stdlib and external crates):
```
~/.cache/groxide/           # Linux, macOS: ~/Library/Caches/groxide/
```

On macOS, `dirs::cache_dir()` returns `~/Library/Caches/`. On Linux, it returns
`~/.cache/` (following XDG spec).

### 10.3 Cache File Paths

| Source | Path |
|--------|------|
| `CurrentCrate` | `target/groxide/<name>-<version>.groxide` |
| `Dependency` | `target/groxide/<name>-<version>.groxide` |
| `Stdlib` | `~/.cache/groxide/stdlib/<name>-<toolchain_hash>.groxide` |
| `External` | `~/.cache/groxide/external/<name>-<version>.groxide` |

**Feature-suffixed paths:** When non-default feature flags are specified (`--features`,
`--all-features`, `--no-default-features`), append a deterministic hash suffix:

```
<name>-<version>-feat_<hash>.groxide
```

The hash is computed as:
1. Build canonical feature string from flags: `"all"`, `"nodef"`, `"f=feat1,feat2"` (features
   sorted alphabetically), joined by `;`
2. Compute DJB2 hash of the canonical string
3. Format as 16-digit lowercase hex

Example: `serde-1.0.210-feat_0a1b2c3d4e5f6a7b.groxide`

**DJB2 hash function:**
```pseudocode
FUNCTION djb2_hash(s: &str) → u64:
    hash = 5381u64
    FOR byte IN s.bytes():
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64)
    RETURN hash
```

### 10.4 Cache Invalidation Rules

| Source | Invalidation Trigger | Metadata Checked |
|--------|---------------------|-----------------|
| `CurrentCrate` | Newest `.rs` mtime in `src/` > cached mtime | `newest_source_mtime` |
| `Dependency` | Package version changed (from cargo metadata) | `package_version` |
| `Stdlib` | Nightly toolchain version changed | `toolchain_version` |
| `External` | Version string changed | `crate_version` |
| All sources | `grox_version` mismatch | Header field |
| All sources | `format_version` mismatch | Header field |

**CurrentCrate mtime scanning:**
```pseudocode
FUNCTION get_newest_source_mtime(manifest_dir) → u64:
    src_dir = manifest_dir / "src"
    IF NOT src_dir.exists(): RETURN 0

    newest = 0u64
    walk_dir_recursively(src_dir):
        FOR file WHERE extension == ".rs":
            mtime = file.metadata().modified() as unix_epoch_secs
            newest = MAX(newest, mtime)
    RETURN newest
```

**Debug-only binary mtime check:** In debug builds (`#[cfg(debug_assertions)]`), also
invalidate when the groxide binary is newer than the cache file. This catches index-building
logic changes during development without requiring a `format_version` bump.

### 10.5 Atomic Writes

Cache saves use write-to-temp-then-rename:

```pseudocode
FUNCTION save_cache(path, index, source):
    header = create_header(source)
    data = CachedData { header, index }
    bytes = rmp_serde::to_vec(data)

    temp_path = path.with_extension("tmp.{PID}")    // PID avoids races
    fs::write(temp_path, bytes)
    fs::rename(temp_path, path)                       // atomic on POSIX

    ON RENAME FAILURE:
        fs::remove_file(temp_path)                    // cleanup
        RETURN Err
```

**Cache save errors are non-fatal.** If `save_cache` fails, log a warning to stderr and
continue — the query still succeeds, the user just won't get caching benefits.

### 10.6 Cache Loading

```pseudocode
FUNCTION load_cached(path, source) → Option<DocIndex>:
    IF NOT path.exists(): RETURN None

    // Debug-only: invalidate if binary is newer
    #[cfg(debug_assertions)]
    IF is_binary_newer_than_cache(path): RETURN None

    data = read_and_deserialize(path) OR RETURN None
    IF NOT is_cache_valid(data.header, source): RETURN None
    RETURN Some(data.index)
```

---

## 11. External Crate Fetching

### 11.1 Trigger Conditions

External crate fetching activates in two cases:

1. **Version-pinned query:** `CrateSpec::Versioned { name, version }` — always fetches
   from crates.io, even if the crate is a project dependency at a different version.
2. **Auto-fetch:** `CrateSpec::Named(name)` where the name is not found in the current
   crate, dependencies, workspace members, transitive dependencies, or stdlib crates.

### 11.2 crates.io API

**Endpoint:** `https://crates.io/api/v1/crates/<name>`

**HTTP configuration:**
- User-Agent: `grox/<version>` (from `CARGO_PKG_VERSION`)
- Timeout: connect=10s, read=30s

**Response structure:**
```rust
struct CrateResponse {
    crate_info: CrateInfo,       // contains max_version: String
    versions: Vec<VersionInfo>,  // each has num: String, yanked: bool
}
```

### 11.3 Version Resolution

```pseudocode
FUNCTION resolve_version(name, version_input) → Result<String>:
    // Case 1: Complete semver (e.g., "1.40.0")
    IF semver::Version::parse(version_input).is_ok():
        RETURN Ok(version_input)

    // Case 2: Partial semver (e.g., "1.40" or "1")
    IF is_partial_version(version_input):
        response = query_crates_io(name)
        matching = response.versions
            .filter(NOT yanked)
            .filter(version_matches_partial(version, version_input))
            .sort_descending()
        IF matching IS EMPTY:
            RETURN Err(ExternalFetchFailed)
        RETURN Ok(matching.first().to_string())

    // Case 3: Other (pre-release, etc.) — use as-is
    RETURN Ok(version_input)

FUNCTION is_partial_version(v) → bool:
    parts = v.split('.')
    RETURN (parts.len() == 1 OR parts.len() == 2)
           AND all parts are parseable as u64

FUNCTION version_matches_partial(version, partial) → bool:
    parts = partial.split('.')
    MATCH parts.len():
        1 → version.major == parts[0]
        2 → version.major == parts[0] AND version.minor == parts[1]
        _ → false
```

**No version specified:** Query `crates.io` and use `crate_info.max_version`.

### 11.4 Download and Extraction

**Download URL:** `https://crates.io/api/v1/crates/<name>/<version>/download`

**Size limit:** 500 MB max download (prevent resource exhaustion).

**Extraction directory:** `~/.cache/groxide/<name>-<version>/`

**Algorithm:**

```pseudocode
FUNCTION download_and_extract(name, version, target_dir):
    // 1. Download
    response = HTTP GET download_url (with size limit)
    body = response.read_to_end(limit=500MB)

    // 2. Prepare temp directory
    temp_dir = target_dir + ".tmp"    // string append, NOT Path::with_extension
    IF temp_dir.exists(): remove_dir_all(temp_dir)
    create_dir_all(temp_dir)

    // 3. Extract tarball
    decoder = GzDecoder(body)
    archive = tar::Archive(decoder)
    prefix = "{name}-{version}"       // top-level dir in tarball

    FOR entry IN archive.entries():
        // Security: skip symlinks and hard links
        IF entry.is_symlink() OR entry.is_hard_link(): SKIP

        // Strip top-level prefix
        stripped_path = entry.path().strip_prefix(prefix)
        target_path = temp_dir / stripped_path

        // Security: path traversal protection
        canonical_temp = temp_dir.canonicalize()
        create_dir_all(target_path.parent())
        canonical_target = target_path.parent().canonicalize() / target_path.filename()
        IF NOT canonical_target.starts_with(canonical_temp):
            RETURN Err("path traversal attempt")

        // Extract regular files and directories only
        IF entry.is_file() OR entry.is_dir():
            entry.unpack(target_path)

    // 4. Atomic rename
    IF target_dir.exists(): remove_dir_all(target_dir)
    rename(temp_dir, target_dir)

    ON ANY ERROR:
        remove_dir_all(temp_dir)    // cleanup
        RETURN Err
```

**Why string append for `.tmp`:** `Path::with_extension` treats the patch version digit
as an extension — `"crate-1.4.0"` becomes `"crate-1.4.tmp"` instead of `"crate-1.4.0.tmp"`.

### 11.5 Rustdoc Generation for External Crates

After extraction, generate rustdoc JSON from the extracted source:

```
cargo +nightly rustdoc --lib --output-format json -Z unstable-options
```

Run from `<cache_dir>/<name>-<version>/` (the extracted source directory). No `-p` flag
needed — it's a single crate, not a workspace.

**Feature strategy:** Default features only. External crates often have optional dependencies
that fail on the current platform. The user can override with `--features` / `--all-features`.

**Feature-specific JSON caching:** When non-default features are used, the rustdoc JSON is
copied to a feature-suffixed path within the extraction directory:
`target/doc/<name><feat_suffix>.json`. This allows multiple feature combinations to coexist.

### 11.6 Complete External Fetch Flow

```pseudocode
FUNCTION fetch_external_crate(name, version_opt, features) → Result<(PathBuf, String)>:
    // 1. Resolve version
    exact_version = IF version_opt IS SOME:
        resolve_version(name, version_opt)
    ELSE:
        query_latest_version(name)    // crate_info.max_version

    // 2. Check if JSON already cached
    json_path = compute_json_path(name, exact_version, features)
    IF json_path.exists():
        eprintln!("[grox] Using cached {name} {exact_version}")
        RETURN Ok(json_path, exact_version)

    // 3. Download source if needed
    crate_dir = cache_dir / "{name}-{exact_version}"
    IF NOT crate_dir / "Cargo.toml" EXISTS:
        eprintln!("[grox] Fetching {name} {exact_version} from crates.io...")
        download_and_extract(name, exact_version, crate_dir)

    // 4. Generate rustdoc JSON
    generate_rustdoc_json(crate_dir, name, exact_version, features)

    // 5. Verify output
    IF NOT json_path.exists():
        RETURN Err(ExternalFetchFailed)

    RETURN Ok(json_path, exact_version)
```

---

## 12. Stdlib Resolution

### 12.1 Recognized Crates

Exactly three crate names: `std`, `core`, `alloc`. Checked by a simple match statement —
no other stdlib crates are supported.

### 12.2 Sysroot Detection

```
rustc +nightly --print sysroot
```

Returns a path like `/Users/user/.rustup/toolchains/nightly-aarch64-apple-darwin`.
Validate that the returned path exists on disk.

### 12.3 rust-src Component Check

The stdlib source lives at:
```
<sysroot>/lib/rustlib/src/rust/library/
```

If this directory does not exist, return `GroxError::StdLibSourceMissing` which tells the
user to run `rustup component add rust-src`.

### 12.4 Toolchain Hash for Cache Key

```
rustc +nightly --version --verbose
```

Parse the verbose output for the `commit-hash:` line:
```
rustc 1.83.0 (90b35a623 2024-11-26)
binary: rustc
commit-hash: 90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf
commit-date: 2024-11-26
host: aarch64-apple-darwin
release: 1.83.0
```

Extract the commit hash (`90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf`). If the
`commit-hash:` line is missing, fall back to DJB2 hash of the first line (which contains
the short version string).

The toolchain hash is used in both the cache file name and the `--target-dir` path
to ensure per-toolchain isolation.

### 12.5 Rustdoc Invocation for Stdlib

```
cargo +nightly rustdoc
    --manifest-path <sysroot>/lib/rustlib/src/rust/library/<crate>/Cargo.toml
    --target-dir <cache_dir>/stdlib/target-<crate>-<toolchain_hash>
    [--all-features]
    --output-format json
    -Z unstable-options
```

Key differences from project crates:
- Uses `--manifest-path` instead of `-p` (not in a workspace)
- Uses explicit `--target-dir` pointing to the global cache directory
- Same `--all-features` with platform fallback strategy as current crate

After successful generation, the JSON output is at:
```
<target_dir>/doc/<crate>.json
```

### 12.6 Stdlib Cache Layout

```
~/.cache/groxide/stdlib/
├── std-<toolchain_hash>.groxide            # serialized DocIndex
├── core-<toolchain_hash>.groxide
├── alloc-<toolchain_hash>.groxide
└── target-<crate>-<toolchain_hash>/        # build artifacts (can be large)
    └── doc/<crate>.json                     # generated rustdoc JSON
```

### 12.7 Complete Stdlib Flow

```pseudocode
FUNCTION generate_stdlib_json(crate_name, features) → Result<PathBuf>:
    // 1. Validate
    IF crate_name NOT IN ["std", "core", "alloc"]:
        RETURN Err(RustdocFailed)

    // 2. Locate source
    sysroot = get_sysroot()
    rust_src = check_rust_src_available(sysroot)
    crate_dir = rust_src / crate_name
    manifest_path = crate_dir / "Cargo.toml"

    // 3. Determine target dir
    toolchain_hash = get_toolchain_hash()
    cache_dir = global_cache_dir / "stdlib"
    target_dir = cache_dir / "target-{crate_name}-{toolchain_hash}"

    // 4. Build with fallback
    eprintln!("[grox] Building index for {crate_name}...")
    IF features.is_default():
        TRY run_rustdoc(manifest_path, target_dir, all_features=true)
        ON PLATFORM FAILURE:
            eprintln!("[grox] Build with --all-features failed, retrying...")
            run_rustdoc(manifest_path, target_dir, all_features=false)
    ELSE:
        run_rustdoc_with_flags(manifest_path, target_dir, features)

    // 5. Return JSON path
    json_path = target_dir / "doc" / "{crate_name}.json"
    RETURN Ok(json_path)
```

---

## 13. Error Handling Summary

| Operation | Error Variant | Exit Code |
|-----------|--------------|-----------|
| Nightly toolchain missing | `NightlyNotAvailable` | 2 |
| Rustdoc command fails | `RustdocFailed { stderr }` | 2 |
| rust-src not installed | `StdLibSourceMissing` | 2 |
| crates.io network error | `ExternalFetchFailed { name, details }` | 2 |
| crates.io 404 (crate not found) | `CrateNotFound { name, suggestions }` | 1 |
| Version not found on crates.io | `ExternalFetchFailed { name, details }` | 2 |
| Tarball extraction failure | `ExternalFetchFailed { name, details }` | 2 |
| Path traversal in tarball | `ExternalFetchFailed { name, details }` | 2 |
| JSON file read failure | `JsonReadFailed { path, source }` | 2 |
| JSON parse failure | `JsonParseFailed { details }` | 2 |
| Cache read/write failure | `CacheSerializationFailed { message }` | 2 |
| Cache save failure | Non-fatal — log warning to stderr | N/A |

All errors in this spec map to exit code 2 (infrastructure/configuration error). Item-level
"not found" errors (exit code 1) are handled by the query engine, not the index/cache layer.
