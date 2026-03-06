# Code Review Fixes for groxide

## Context

A comprehensive code review (manual + 4 parallel agents + pmat analysis) identified maintainability and convention issues. No correctness bugs were found. The primary concern is `src/lib.rs` which has high cognitive complexity (top function scored 109 vs threshold of 25). Root causes: dead parameters inflating signatures, wildcard matches on project-owned enums violating CLAUDE.md conventions, duplicated rendering dispatch logic, and a few minor performance/clarity issues.

All 560 tests pass, clippy is clean, zero dead code. These fixes improve maintainability without changing behavior.

## Tasks (5 total, ordered by dependency)

### Task 68: Remove dead parameters from `resolve_item` and `render_impls`

**Why:** 3 unused params in `resolve_item` and 2 in `render_impls` add noise and contribute to `too_many_arguments` downstream.

**Files:** `src/lib.rs`

**Changes:**
1. `resolve_item` (line 285): Remove `_features: &FeatureFlags`, `_feature_suffix: &str`, `_private: bool` — keep only `query`, `index`, `kind_filter`
2. Update 4 call sites:
   - `run()` line 104: `resolve_item(&query_path, &index, kind_filter, &features, &feature_suffix, cli.private)` → `resolve_item(&query_path, &index, kind_filter)`
   - `try_follow_reexport()` line 767: `resolve_item(&source_query, &source_index, None, features, feature_suffix, private)` → `resolve_item(&source_query, &source_index, None)`
   - `handle_workspace()` line 1032: `resolve_item(&query_path, &index, None, &features, &feature_suffix, false)` → `resolve_item(&query_path, &index, None)`
   - `query_fixture()` test helper line 1178: `resolve_item(&query_path, index, kind_filter, &features, &feature_suffix, cli.private)` → `resolve_item(&query_path, index, kind_filter)`
3. `render_impls` (line 1114): Remove `_index: &DocIndex`, `_item_idx: usize` — keep only `display`, `trait_filter`
4. Update 1 call site at line 958: `render_impls(&display, using_index, effective_idx, trait_filter)` → `render_impls(&display, trait_filter)`
5. Check if `#[allow(clippy::too_many_arguments)]` on `handle_recursive_source` (line 596) can be removed after the parameter reduction propagates

**Verification:** `cargo test && cargo clippy --all-targets`

---

### Task 69: Match all enum variants explicitly (eliminate wildcards on owned enums)

**Why:** CLAUDE.md says "No wildcard `_` on enums we control." Wildcards hide missing cases when new variants are added.

**Files:** `src/error.rs`, `src/lib.rs`

**Changes:**
1. `GroxError::exit_code()` at `error.rs:149-153`: Replace `_ => EXIT_ERROR` with explicit match on all 12 variants:
   ```rust
   Self::ManifestNotFound | Self::CargoMetadataFailed { .. } | Self::NightlyNotAvailable
   | Self::RustdocFailed { .. } | Self::StdLibSourceMissing | Self::ExternalFetchFailed { .. }
   | Self::InvalidQuery { .. } | Self::JsonReadFailed { .. } | Self::JsonParseFailed { .. }
   | Self::CacheSerializationFailed { .. } | Self::Io(_) => EXIT_ERROR,
   ```
2. `try_follow_reexport()` at `lib.rs:777`: Replace `_ => None` with `QueryResult::Ambiguous { .. } | QueryResult::NotFound { .. } => None`
3. `resolve_crate_source()` at `lib.rs:189-191`: Replace `_ => None` with explicit `CrateSource::CurrentCrate { .. } | CrateSource::Dependency { .. } | CrateSource::Stdlib { .. } => None`

**Verification:** `cargo test && cargo clippy --all-targets`

---

### Task 70: Extract shared rendering dispatch helper to reduce `lib.rs` complexity

**Why:** `handle_workspace` has cognitive complexity 109 (threshold 25), largely from duplicating the rendering dispatch in `handle_output`. Extracting a shared helper cuts complexity by ~50% for the top 3 functions.

**Files:** `src/lib.rs`

**Changes:**
1. Extract a helper function for the recursive rendering dispatch pattern shared between `handle_output` (lines 930-945) and `handle_workspace` (lines 1072-1087):
   ```rust
   fn render_recursive(
       w: &mut impl Write,
       index: &DocIndex,
       idx: usize,
       cli: &Cli,
   ) -> Result<()> {
       let kind_filter = cli.kind.map(ItemKind::from);
       let mut items = render::collect_children_recursive(index, idx, cli.private);
       if let Some(filter) = kind_filter {
           items.retain(|item| item.kind.matches_filter(filter));
       }
       let root_path = &index.get(idx).path;
       let output = if cli.json {
           render::json::render_json_recursive(&items)
       } else if cli.brief {
           render::brief::render_brief_recursive(&items, root_path)
       } else if cli.docs {
           render::docs::render_docs_recursive(&items, root_path)
       } else {
           render::list::render_list_recursive(&items, root_path)
       };
       writeln!(w, "{output}").map_err(GroxError::Io)
   }
   ```
2. Replace the duplicated blocks in `handle_output` and `handle_workspace` with calls to this helper
3. Similarly extract the `--recursive --source` rendering pattern shared between `handle_recursive_source` and `handle_workspace` into a helper like `render_recursive_source()`
4. After extraction, check if `#[allow(clippy::too_many_arguments)]` on `handle_recursive_source` can be removed

**Verification:** `cargo test && cargo clippy --all-targets`. Snapshot tests will catch any output differences — run `INSTA_UPDATE=always cargo test` if snapshots need updating (they shouldn't since behavior is unchanged).

---

### Task 71: Fix `Vec::insert(0)` performance in `index_builder.rs`

**Why:** `reconstruct_path()` calls `segments.insert(0, ...)` in a loop (up to 20 iterations), causing O(n) shifts per iteration. Simple fix with no behavior change.

**Files:** `src/index_builder.rs`

**Changes:**
1. In `reconstruct_path()` (line 260-290): Change `segments.insert(0, parent_name.clone())` to `segments.push(parent_name.clone())`
2. After the loop (before `segments.join("::")`), add `segments.reverse()`

**Verification:** `cargo test && cargo clippy --all-targets`. Delete `target/groxide/` cache before testing per CLAUDE.md.

---

### Task 72: Minor cleanups (double iteration, `#[must_use]`)

**Why:** Small clarity and correctness improvements.

**Files:** `src/render/mod.rs`, `src/types.rs`, `src/cli.rs`

**Changes:**
1. `strip_inline_markdown()` at `render/mod.rs:401-407`: Bind `chars().next()` once:
   ```rust
   _ => {
       let ch = line[i..].chars().next().expect("invariant: valid index");
       result.push(ch);
       i += ch.len_utf8();
   }
   ```
2. Add `#[must_use]` to key value-returning functions:
   - `QueryPath::parse()` in `cli.rs`
   - `ItemKind::matches_filter()`, `is_primary()` in `types.rs`
   - `FeatureFlags::is_default()`, `cache_suffix()` in `cli.rs`

**Verification:** `cargo test && cargo clippy --all-targets`

---

## What we're NOT fixing (and why)

- **`use super::*` in test modules** (20 files): Extremely widespread, idiomatic Rust, massive churn for no practical benefit
- **`let _ = writeln!` in renderers** (63 occurrences): Writing to `String` via `fmt::Write` cannot fail; pattern is consistent and intentional
- **pmat CI/CD score** (Makefile, pre-commit hooks, benchmarks): Infrastructure changes outside the scope of code review fixes

## Verification (all tasks)

After all tasks:
```
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```
