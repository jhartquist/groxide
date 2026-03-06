# Architecture Cleanup for Open Source Release

**Date:** 2026-03-06
**Goal:** Transform organically-grown codebase into clean, modular architecture demonstrating deliberate design choices. Eliminate all hacks, duplication, and code smells. Preserve all existing tests and behavior.

**Approach:** Five phases, each building on the previous. Quick fixes first to establish a clean baseline, then module extraction, DRY abstractions, architectural improvements, and final polish.

---

## Phase 1: Quick Fixes

Independent tasks that remove obvious code smells. No dependencies between them.

### 1.1 Deduplicate STDLIB_CRATES constant

**Problem:** `const STDLIB_CRATES: &[&str] = &["std", "core", "alloc"]` is defined identically in both `src/resolve.rs:9` and `src/stdlib.rs:8`, with corresponding `is_stdlib_crate()` functions in each.

**Fix:** Define the constant once. Options:
- Add to `src/types.rs` (it's the shared types module)
- Create `src/constants.rs` (overkill for one constant)

**Recommendation:** Add to `src/types.rs` as `pub(crate) const STDLIB_CRATES`. Add a single `pub(crate) fn is_stdlib_crate(name: &str) -> bool` there. Remove both copies from resolve.rs and stdlib.rs. Update all call sites.

**Files:** `src/types.rs`, `src/resolve.rs`, `src/stdlib.rs`

### 1.2 Derive Clone for DocIndex, remove serialize roundtrip

**Problem:** `src/cache.rs:144-151` contains `serialize_index_ref()` which serializes a `&DocIndex` to bytes then immediately deserializes it back — a full O(n) binary roundtrip just to produce an owned copy. This exists because `CachedData` needs to own its `DocIndex` and `DocIndex` doesn't implement `Clone`.

The project rule "no `.clone()` to silence the borrow checker" was misapplied here. This isn't a borrow checker workaround — it's legitimate data ownership transfer for cache storage.

**Fix:** Add `#[derive(Clone)]` to `DocIndex` (and any fields that need it). Delete `serialize_index_ref()`. Replace its call site in `save_to_cache()` with `index.clone()`.

**Files:** `src/types.rs`, `src/cache.rs`

### 1.3 Fix CacheMetadata field naming inconsistency

**Problem:** In `src/cache.rs`, `CacheMetadata::Dependency` uses `package_version: String` while `CacheMetadata::External` uses `crate_version: String`. Same semantic concept, different field names.

**Fix:** Rename both to `version: String`. Update all match arms in `is_cache_valid()` and `create_header()`.

**Caveat:** This changes the serialized cache format. Bump `FORMAT_VERSION` so old caches are invalidated cleanly (they'll just be rebuilt).

**Files:** `src/cache.rs`

### 1.4 Fix O(n²) dedup in format_suggestions

**Problem:** `src/error.rs:23-29` uses `Vec::contains()` in a loop for deduplication — O(n²). While N ≤ 10 in practice, this is the wrong data structure choice and sets a bad pattern.

**Fix:** Use a `HashSet` for seen tracking, or sort + dedup. Since we need to preserve insertion order (first occurrence wins), use `Vec` + `HashSet::insert()` for the seen check.

**Files:** `src/error.rs`

---

## Phase 2: Module Extraction from lib.rs

**Depends on:** Phase 1 complete (clean baseline).

The goal is to reduce `src/lib.rs` from ~1521 lines with 3 mixed concerns down to ~800 lines of pure orchestration. Each extraction creates a new module with a clear single responsibility.

### 2.1 Extract src/reexport.rs

**Problem:** `lib.rs` contains ~150 lines of re-export following logic that is a self-contained concern:
- `try_follow_reexport()` (~60 lines) — follows cross-crate re-export stubs to canonical items
- `parse_reexport_source()` (~15 lines) — parses re-export source annotation from item docs
- `annotate_reexport()` (~55 lines) — annotates rendered output with re-export path info

**Fix:** Create `src/reexport.rs`. Move these three functions. They depend on `cache`, `docgen`, `query`, `resolve`, `cli::FeatureFlags` — all accessible via `crate::` paths. Update `lib.rs` to `mod reexport;` and call through `reexport::`.

**Files:** New `src/reexport.rs`, modified `src/lib.rs`

### 2.2 Extract render dispatch into render/dispatch.rs

**Problem:** `lib.rs` contains ~200 lines of render dispatch logic:
- `handle_output()` (~87 lines) — dispatches Found/Ambiguous/NotFound to renderers
- `render_recursive()` (~18 lines) — recursive mode dispatch
- `render_recursive_source()` (~27 lines) — recursive source mode
- `render_impls()` (~15 lines) — impl rendering dispatch

These belong in the render module since they're purely about choosing which renderer to call.

**Fix:** Create `src/render/dispatch.rs`. Move these functions. They depend on `render::*` submodules (already in scope), `types::*`, `query::is_reexport_stub`, and `reexport::*` (from 2.1). Wire into `render/mod.rs` as `pub(crate) mod dispatch;`. Update `lib.rs` to call `render::dispatch::handle_output()` etc.

**Files:** New `src/render/dispatch.rs`, modified `src/render/mod.rs`, modified `src/lib.rs`

### 2.3 Extract source-reading into render/source.rs or src/source.rs

**Problem:** `lib.rs` contains source-reading logic:
- `read_source_content()` (~35 lines) — reads source code from disk for any CrateSource variant
- `handle_source()` (~85 lines) — orchestrates source mode output
- `handle_recursive_source()` — partially overlaps with `render_recursive_source()`

**Fix:** Create `src/source.rs` with `read_source_content()` and `handle_source()`. These depend on `resolve::CrateSource` and filesystem operations. The render-dispatch portion (`render_recursive_source`) goes to `render/dispatch.rs` (task 2.2) and calls into `source::read_source_content()`.

**Files:** New `src/source.rs`, modified `src/lib.rs`, modified `src/render/dispatch.rs`

---

## Phase 3: DRY Abstractions

**Depends on:** Phase 2 complete (module structure stable).

Eliminate the most visible copy-paste patterns. Each task is independent within this phase.

### 3.1 ItemKind metadata table

**Problem:** `src/types.rs` has 4 exhaustive match blocks over all 33 `ItemKind` variants:
- `short_name()` (lines 33-51)
- `matches_filter()` (lines 67-77)
- `category()` (lines 80-96)
- `is_primary()` (lines 104-114)

Adding a new variant means updating 4 match arms. These are pure data mappings.

**Fix:** Define a const metadata struct and lookup table:
```rust
struct KindMeta {
    short_name: &'static str,
    category: KindCategory,
    is_primary: bool,
    filter: Option<KindFilter>,
}

impl ItemKind {
    const fn meta(&self) -> &'static KindMeta {
        &KIND_META[*self as usize]
    }
}
```

Then `short_name()`, `category()`, `is_primary()` become one-liners delegating to `self.meta()`. `matches_filter()` checks `self.meta().filter == Some(filter)`.

**Estimated savings:** ~60 lines, plus easier extensibility.

**Files:** `src/types.rs`

### 3.2 Shared suggestion scoring

**Problem:** `src/query.rs` has two near-identical functions:
- `compute_suggestions()` (lines 500-535) — scores all index items by Levenshtein distance
- `compute_method_suggestions()` (lines 605-635) — scores a parent's children by Levenshtein distance

Both: collect candidates via `could_match_within_distance()` prefilter → compute `levenshtein_distance()` → sort by (distance, name) → dedup by path → truncate to 5.

**Fix:** Extract:
```rust
fn collect_suggestions<'a>(
    candidates: impl Iterator<Item = (&'a str, &'a str)>,  // (name, display_path)
    query: &str,
    max: usize,
) -> Vec<String>
```

Both callers provide their own iterator over (name, path) pairs.

**Estimated savings:** ~30 lines.

**Files:** `src/query.rs`

### 3.3 Index builder child-collection helper

**Problem:** `src/index_builder.rs` has 3 places that match `ItemEnum::Struct/Enum/Union/Trait` to extract child IDs:
- `resolve_module_children()` — collects fields/variants for module listings
- `pass4_link_relationships()` — links children to parent items
- `resolve_use_children()` — resolves re-export targets

Each has a ~15-line match block doing the same thing.

**Fix:** Extract:
```rust
fn get_item_child_ids(item: &Item) -> Vec<Id> {
    match &item.inner {
        ItemEnum::Struct(s) => struct_field_ids(s),
        ItemEnum::Enum(e) => e.variants.clone(),
        ItemEnum::Union(u) => u.fields.clone(),
        ItemEnum::Trait(t) => t.items.clone(),
        _ => Vec::new(),
    }
}
```

**Estimated savings:** ~40 lines.

**Files:** `src/index_builder.rs`

### 3.4 Unify impl/trait path computation

**Problem:** `src/index_builder.rs` has two nearly identical functions:
- `compute_impl_paths()` (lines 188-216) — computes paths for methods inside impl blocks
- `compute_trait_item_paths()` (lines 229-258) — computes paths for items inside trait definitions

Both: iterate items → check if kind matches → check if path already exists → look up parent name → build child path from parent.

**Fix:** Extract a generic function parameterized by the item filter:
```rust
fn compute_descendant_paths<F>(&mut self, filter: F)
where
    F: Fn(&ItemEnum) -> Option<&[Id]>,
```

**Estimated savings:** ~30 lines.

**Files:** `src/index_builder.rs`

---

## Phase 4: Architectural Improvements

**Depends on:** Phase 3 complete. Phase 4.1 specifically depends on `render/dispatch.rs` existing (from 2.2).

### 4.1 OutputMode enum and dispatch consolidation

**Problem:** Three places in the codebase have the same if/else chain:
```rust
if cli.json { render_json(...) }
else if cli.brief { render_brief(...) }
else if cli.docs { render_docs(...) }
else { render_text(...) }
```

This appears in `handle_output()`, `render_recursive()`, and partially in `handle_search()`.

**Fix:** Define an enum in cli.rs (or render/):
```rust
pub(crate) enum OutputMode {
    Json,
    Brief,
    Text,
}

impl Cli {
    pub(crate) fn output_mode(&self) -> OutputMode { ... }
}
```

Then dispatch once per call site using `match cli.output_mode() { ... }`. This consolidates the decision logic and makes adding new output modes (e.g., TOML, compact) a single-point change.

Note: This is NOT a full trait-based renderer hierarchy — that would be over-engineering for a CLI tool. Just an enum to centralize the dispatch.

**Files:** `src/cli.rs`, `src/render/dispatch.rs`

### 4.2 Feature gate suffix consolidation

**Problem:** Feature gate formatting logic is scattered:
- `render/mod.rs:538` defines `feature_gate_suffix()` helper
- `render/text.rs:258-270` re-implements suffix composition in `render_name_line` and `render_signature_line`
- `render/list.rs` and `render/ambiguous.rs` each have their own composition

Changes to the format require updating 3+ places.

**Fix:** Make `feature_gate_suffix()` the single source of truth. All renderers call it and append the result. Remove inline re-implementations.

**Files:** `src/render/mod.rs`, `src/render/text.rs`, `src/render/list.rs`, `src/render/ambiguous.rs`

### 4.3 DocIndex accessor methods

**Problem:** `DocIndex` fields (`path_map`, `name_map`, `suffix_map`, `items`, `trait_impls`) are `pub(crate)` and accessed directly throughout the codebase. This makes it hard to change the index structure without updating every call site.

**Fix:** Add accessor methods:
```rust
impl DocIndex {
    pub(crate) fn lookup_by_path(&self, path: &str) -> &[usize] { ... }
    pub(crate) fn lookup_by_name(&self, name: &str) -> &[usize] { ... }
    pub(crate) fn lookup_by_suffix(&self, suffix: &str) -> &[usize] { ... }
    pub(crate) fn trait_impls_for(&self, item_idx: usize) -> &[TraitImpl] { ... }
}
```

Keep fields `pub(crate)` for now (gradual migration), but add methods and migrate call sites. Eventually fields can become private.

**Files:** `src/types.rs`, all files that access DocIndex fields directly

---

## Phase 5: Final Polish

**Depends on:** All previous phases complete.

### 5.1 Eliminate annotate_reexport blank-line hack

**Problem:** `annotate_reexport()` (now in `src/reexport.rs` after Phase 2) reconstructs rendered output by counting blank lines to find insertion points. This is fragile — if text rendering format changes, it breaks silently.

**Fix:** Two options:
- **Option A:** Have the text renderer return structured output `(header: String, body: String)` so annotation can insert between them without parsing.
- **Option B:** Have `handle_output()` pass the re-export annotation to the renderer as context, so it renders the annotation inline.

**Recommendation:** Option B is simpler — add an optional `reexport_annotation: Option<(&str, &str)>` parameter (original_path, source_path) to `render_text()`. The renderer places it correctly.

**Files:** `src/reexport.rs`, `src/render/text.rs`, `src/render/dispatch.rs`

### 5.2 RenderContext struct

**Problem:** Multiple functions pass overlapping parameter sets: `(index, limits, mode, kind_filter, private)`. Adding a new parameter means updating function signatures across the render pipeline.

**Fix:** Bundle into a context struct:
```rust
pub(crate) struct RenderContext<'a> {
    pub index: &'a DocIndex,
    pub limits: DisplayLimits,
    pub mode: OutputMode,
    pub kind_filter: Option<ItemKind>,
    pub include_private: bool,
}
```

Thread this through render dispatch functions instead of individual parameters.

**Files:** `src/render/dispatch.rs`, `src/render/mod.rs`, callers in `src/lib.rs`

### 5.3 Final review pass

Run a complete review:
1. `cargo fmt -- --check`
2. `cargo clippy --all-targets` — zero warnings
3. `cargo test` — all tests pass
4. `INSTA_UPDATE=always cargo test` if any snapshots changed
5. Manual review: no remaining `// TODO`, `// HACK`, `// FIXME`
6. Verify all `pub(crate)` boundaries are intentional
7. Check doc comments on all public items
8. Verify no `.unwrap()` in library code (test code is fine)

**Files:** All

---

## Dependency Graph

```
Phase 1 (all parallel):
  T73 (STDLIB_CRATES) ──┐
  T74 (Clone DocIndex) ──┤
  T75 (CacheMetadata) ───┤── no dependencies
  T76 (O(n²) dedup) ─────┘
          │
Phase 2 (parallel, depends on Phase 1):
  T77 (reexport.rs) ─────┐
  T78 (dispatch.rs) ──────┤── can be parallel
  T79 (source.rs) ────────┘
          │
Phase 3 (parallel, depends on Phase 2):
  T80 (ItemKind table) ──┐
  T81 (suggestion DRY) ──┤
  T82 (child helper) ────┤── all independent
  T83 (impl/trait DRY) ──┘
          │
Phase 4 (partially parallel, depends on Phase 3):
  T84 (OutputMode) ───────── depends on T78
  T85 (feature gate) ────┐
  T86 (DocIndex access) ─┘── independent
          │
Phase 5 (sequential, depends on all):
  T87 (reexport hack) ───── depends on T84
  T88 (RenderContext) ───── depends on T84
  T89 (final review) ────── depends on all
```

## Risk Assessment

- **Phase 1:** Zero risk. Mechanical changes with full test coverage.
- **Phase 2:** Low risk. Moving code between files. Tests don't change.
- **Phase 3:** Low risk. Extracting helpers doesn't change behavior.
- **Phase 4:** Medium risk. OutputMode enum changes dispatch pattern; feature gate consolidation touches rendering output. Run snapshot tests.
- **Phase 5:** Medium risk. Changing renderer interfaces. Must verify all snapshot tests.

## Success Criteria

- All existing tests pass without modification (except moving between modules)
- `cargo clippy --all-targets` — zero warnings
- No file exceeds ~800 lines
- No function exceeds ~50 lines
- No duplicated constants or near-identical function pairs
- No hacks (serialize roundtrip, blank-line counting, etc.)
- Clear module boundaries with single responsibilities
