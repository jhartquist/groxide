# CLI Redesign, Workspace Support, and Bug Fixes — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Simplify groxide's CLI to composable detail tiers (`-b`/`-d`/`-s` × `-r`), add workspace-wide querying, refactor caching, and fix 4 bugs from real-world AI agent usage.

**Architecture:** The CLI flags are restructured around two orthogonal axes: scope (default vs `-r` recursive) and detail (brief/default/docs/source). Workspace support detects virtual manifests and iterates over workspace members. Cache moves external crates to a global `~/.cache/groxide/` directory while local crates always rebuild (no mtime tracking).

**Tech Stack:** Rust, clap (derive), cargo_metadata, rustdoc-types, rmp-serde, insta (snapshots)

**Design doc:** `docs/plans/2026-03-05-cli-redesign-and-workspace-support-design.md`

---

## Phase 1: CLI Flag Restructure

### Task 1: Remove `-l`/`--list` flag

The default module/crate view already lists children. `-l` is redundant and confusing.

**Files:**
- Modify: `src/cli.rs:35-37` (remove `list` field)
- Modify: `src/lib.rs:724-726` (remove list dispatch branch)
- Modify: `src/lib.rs:749-751` (remove list branch in ambiguous)
- Modify: `tests/cli_happy.rs` (remove/update list-mode tests)
- Modify: `tests/cli_errors.rs` (remove conflict tests involving `--list`)

**Step 1: Remove the field from Cli struct**

In `src/cli.rs`, delete lines 35-37 (the `list` field). Remove `"list"` from all `conflicts_with_all` arrays on other flags. Update HELP_EXAMPLES to remove the `-l` example.

**Step 2: Remove list dispatch in `handle_output`**

In `src/lib.rs`:
- Remove the `cli.list` branch at line 724-726
- Remove the `cli.list` branch at line 749-751 (ambiguous case)
- Remove any `cli.list` references in the search handler

**Step 3: Update integration tests**

- Remove tests that use `--list` or `-l` in `cli_happy.rs` and `cli_errors.rs`
- Remove associated snapshots from `tests/snapshots/`
- Update any conflict tests that reference `list`

**Step 4: Run tests and update snapshots**

```bash
INSTA_UPDATE=always cargo test
cargo clippy --all-targets
```

**Step 5: Commit**

```bash
git add -A && git commit -m "Remove -l/--list flag (redundant with default container view)"
```

---

### Task 2: Add `-b`/`--brief` and `-d`/`--docs` flags

New detail tier flags. They conflict with each other and with `-s` (only one detail tier at a time).

**Files:**
- Modify: `src/cli.rs` (add `brief` and `docs` fields to Cli)

**Step 1: Write failing test**

In `tests/cli_errors.rs`, add a test that `--brief` and `--docs` conflict:

```rust
#[test]
fn brief_and_docs_conflict() {
    let output = grox()
        .args(["--brief", "--docs", "groxide_test_api"])
        .output()
        .expect("command runs");
    assert_eq!(output.status.code(), Some(2));
}
```

**Step 2: Add fields to Cli struct**

In `src/cli.rs`, add to the Cli struct:

```rust
/// Show only item names (compact output)
#[arg(short = 'b', long, conflicts_with_all = ["docs", "source"])]
pub brief: bool,

/// Show full rendered documentation per item
#[arg(short = 'd', long, conflicts_with_all = ["brief", "source"])]
pub docs: bool,
```

Update `source` field conflicts to include `"brief"` and `"docs"`.
Update `search` field conflicts to include `"brief"` and `"docs"`.
Update `impls` field conflicts to include `"brief"` and `"docs"`.

**Step 3: Run tests**

```bash
cargo test
```

**Step 4: Commit**

```bash
git add -A && git commit -m "Add -b/--brief and -d/--docs detail tier flags"
```

---

### Task 3: Change `--impls` to accept optional trait name filter

Change from `bool` to `Option<Option<String>>` — bare `--impls` or `--impls Clone`.

**Files:**
- Modify: `src/cli.rs:55-57` (change impls field type)
- Modify: `src/lib.rs:721-723` (update impls dispatch)
- Modify: `src/lib.rs:769-782` (update `render_impls` to accept filter)
- Modify: `src/render/ambiguous.rs` (add filtering to `render_impls_type`)

**Step 1: Write failing integration test**

In `tests/cli_happy.rs`:

```rust
#[test]
fn impls_filter_by_trait_name() {
    let output = grox()
        .args(["--impls", "Debug", "groxide_test_api::GenericStruct"])
        .output()
        .expect("command runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Debug"));
    // Should NOT contain other impls
    assert!(!stdout.contains("impl<T> GenericStruct<T>"));
}
```

**Step 2: Change `--impls` to optional value**

In `src/cli.rs`, change the impls field:

```rust
/// Show trait implementations, optionally filtered by trait name
#[arg(short = 'i', long, conflicts_with_all = ["source", "brief", "docs"],
      num_args = 0..=1, default_missing_value = "")]
pub impls: Option<String>,
```

**Step 3: Update dispatch in `lib.rs`**

Update the impls check from `cli.impls` (bool) to `cli.impls.is_some()`. Pass the filter value to `render_impls`:

```rust
if let Some(ref filter) = cli.impls {
    let trait_filter = if filter.is_empty() { None } else { Some(filter.as_str()) };
    let output = render_impls(&display, using_index, effective_idx, trait_filter);
    writeln!(w, "{output}").map_err(GroxError::Io)?;
}
```

**Step 4: Add filtering to `render_impls_type` in `src/render/ambiguous.rs`**

Add a `trait_filter: Option<&str>` parameter. When set, filter `trait_impls` to only those whose `trait_path` contains the filter string (case-insensitive).

**Step 5: Run tests and update snapshots**

```bash
INSTA_UPDATE=always cargo test
```

**Step 6: Commit**

```bash
git add -A && git commit -m "Add optional trait name filter to --impls flag"
```

---

### Task 4: Add `--clear-cache` flag

New flag that wipes `~/.cache/groxide/` and exits.

**Files:**
- Modify: `src/cli.rs` (add `clear_cache` field)
- Modify: `src/main.rs` or `src/lib.rs` (handle early return)
- Modify: `src/cache.rs` (add `clear_global_cache` function)

**Step 1: Add field to Cli**

```rust
/// Clear the global documentation cache and exit
#[arg(long)]
pub clear_cache: bool,
```

**Step 2: Add `clear_global_cache` to `src/cache.rs`**

```rust
/// Removes the global cache directory (`~/.cache/groxide/`).
///
/// Returns the path that was cleared, or `None` if the cache dir couldn't be determined.
pub(crate) fn clear_global_cache() -> Option<PathBuf> {
    let cache_dir = dirs::cache_dir()?.join("groxide");
    if cache_dir.exists() {
        let _ = fs::remove_dir_all(&cache_dir);
    }
    Some(cache_dir)
}
```

**Step 3: Handle in `run()`**

At the top of `run()` in `src/lib.rs`, before project discovery:

```rust
if cli.clear_cache {
    if let Some(path) = cache::clear_global_cache() {
        eprintln!("[grox] Cleared cache at {}", path.display());
    }
    return Ok(());
}
```

**Step 4: Write test**

In `tests/cli_happy.rs`:

```rust
#[test]
fn clear_cache_exits_successfully() {
    let output = Command::new(cargo_bin!("grox"))
        .arg("--clear-cache")
        .output()
        .expect("command runs");
    assert!(output.status.success());
}
```

**Step 5: Commit**

```bash
git add -A && git commit -m "Add --clear-cache flag to wipe global documentation cache"
```

---

### Task 5: Remove `-r`/`-s` conflict — make them composable

Currently `recursive` conflicts with `source`. Remove this conflict.

**Files:**
- Modify: `src/cli.rs:59-61` (remove `"source"` from recursive conflicts)
- Modify: `src/cli.rs:32-33` (remove `"recursive"` from source conflicts if present)

**Step 1: Write failing test**

```rust
#[test]
fn recursive_source_composable() {
    let output = grox()
        .args(["-r", "-s", "groxide_test_api"])
        .output()
        .expect("command runs");
    // Should not fail with conflict error
    assert_ne!(output.status.code(), Some(2));
}
```

**Step 2: Update conflict lists**

In `src/cli.rs`:
- `source` field: change `conflicts_with_all` to `["impls", "brief", "docs"]` (remove any recursive reference)
- `recursive` field: change `conflicts_with_all` to `["impls", "search"]` (remove `"source"`)
- Also update `recursive` to not conflict with `brief` or `docs` (they compose)

**Step 3: Run tests**

```bash
cargo test
```

**Step 4: Commit**

```bash
git add -A && git commit -m "Make -r composable with -s, -b, and -d detail flags"
```

---

## Phase 2: Rendering Tiers

### Task 6: Implement brief rendering mode (`-b`)

Brief mode shows only kind + name. Works for both single-item and recursive modes.

**Files:**
- Create: `src/render/brief.rs`
- Modify: `src/render/mod.rs` (add `pub(crate) mod brief;`)
- Modify: `src/lib.rs` (dispatch to brief renderer)

**Step 1: Write unit test for brief renderer**

In `src/render/brief.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item_full;
    use crate::types::{DocIndex, ItemKind, ChildRef};

    #[test]
    fn render_brief_module_shows_only_names() {
        // Build a module with children, verify output is just kind + name
        // No signatures, no summaries
    }

    #[test]
    fn render_brief_recursive_shows_only_names() {
        // Build items, verify recursive brief is kind + path only
    }
}
```

**Step 2: Implement `render_brief` and `render_brief_recursive`**

```rust
/// Renders items in brief mode: kind + name only, no signatures or docs.
pub(crate) fn render_brief(display: &DisplayItem<'_>) -> String { ... }

/// Renders recursive items in brief mode: kind + path only.
pub(crate) fn render_brief_recursive(items: &[&IndexItem], root_path: &str) -> String { ... }
```

Use the same grouped-by-module structure as `render_list_recursive` but only emit `{kind}  {path}` per item — no signature, no summary columns.

**Step 3: Wire into `handle_output` in `src/lib.rs`**

In the recursive block, check `cli.brief` before calling existing recursive renderer. In the default block, check `cli.brief` and dispatch to brief renderer.

**Step 4: Write integration test**

```rust
#[test]
fn brief_mode_crate_root() {
    let output = grox()
        .args(["-b", "groxide_test_api"])
        .output()
        .expect("command runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should have kind + name but NOT signatures
    assert!(stdout.contains("mod"));
    assert!(stdout.contains("containers"));
    assert!(!stdout.contains("pub fn"));
    insta::assert_snapshot!("brief_crate_root", stdout);
}
```

**Step 5: Run tests and update snapshots**

```bash
INSTA_UPDATE=always cargo test
```

**Step 6: Commit**

```bash
git add -A && git commit -m "Implement -b/--brief rendering mode (kind + name only)"
```

---

### Task 7: Implement docs rendering mode (`-d`)

Docs mode shows kind + name + signature + full rendered docs. For recursive mode, this means full docs per item.

**Files:**
- Modify: `src/render/list.rs` (add `render_list_with_docs` and `render_list_recursive_with_docs`)
- Modify: `src/lib.rs` (dispatch to docs-mode renderer)
- Modify: `src/render/mod.rs` (potentially add helper for rendering per-item docs)

**Step 1: Write integration test**

```rust
#[test]
fn docs_mode_function() {
    let output = grox()
        .args(["-d", "groxide_test_api::add"])
        .output()
        .expect("command runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show full docs, not just one-line summary
    assert!(stdout.contains("pub fn add"));
    insta::assert_snapshot!("docs_mode_function", stdout);
}
```

**Step 2: Implement docs-mode rendering**

For single items, `-d` is essentially the current default view (which already shows full docs). The key difference is in list/recursive contexts where the current default only shows one-line summaries.

For recursive mode with `-d`:
- Each item gets its full docs rendered (not just summary)
- Use a section-per-item format: header (kind + path + signature) then indented docs

**Step 3: Wire into dispatch**

In `src/lib.rs`, check `cli.docs` in both recursive and default branches.

**Step 4: Run tests**

```bash
INSTA_UPDATE=always cargo test
```

**Step 5: Commit**

```bash
git add -A && git commit -m "Implement -d/--docs rendering mode (full docs per item)"
```

---

### Task 8: Update source mode to include rendered docs

Currently `-s` shows only source code. Change to show rendered docs above source.

**Files:**
- Modify: `src/render/ambiguous.rs` (update `render_source` to prepend docs)
- Modify: `src/lib.rs:462-505` (update `handle_source` if needed)

**Step 1: Write failing test**

```rust
#[test]
fn source_mode_includes_docs() {
    let output = grox()
        .args(["-s", "groxide_test_api::add"])
        .output()
        .expect("command runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain both docs and source
    assert!(stdout.contains("Adds two numbers")); // doc comment content
    assert!(stdout.contains("a + b")); // source content
}
```

**Step 2: Update `render_source`**

In `src/render/ambiguous.rs`, modify `render_source` to prepend the item's rendered documentation (item.docs) above the source code, separated by a blank line. Use `strip_markdown` on the docs.

**Step 3: Update snapshots**

```bash
INSTA_UPDATE=always cargo test
```

**Step 4: Commit**

```bash
git add -A && git commit -m "Include rendered docs above source code in -s mode"
```

---

### Task 9: Implement recursive + source (`-r -s`)

The "dump everything" mode. Recursive traversal with source code per item.

**Files:**
- Modify: `src/lib.rs` (add recursive+source path in handle_output)
- Modify: `src/render/ambiguous.rs` or create helper for batch source rendering
- Modify: `src/lib.rs:507-539` (read_source_content needs to work for batch items)

**Step 1: Write integration test**

```rust
#[test]
fn recursive_source_mode() {
    let output = grox()
        .args(["-r", "-s", "groxide_test_api::containers"])
        .output()
        .expect("command runs");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain source for multiple items
    assert!(stdout.contains("Stack"));
    assert!(stdout.contains("Pair"));
    insta::assert_snapshot!("recursive_source_containers", stdout);
}
```

**Step 2: Implement recursive source rendering**

In `src/lib.rs`, when both `cli.recursive` and `cli.source` are set:
1. Collect all items recursively (existing `collect_children_recursive`)
2. For each item, read source content via `read_source_content`
3. Render each item with docs + source (using the updated `render_source`)
4. Concatenate with section separators

**Step 3: Run tests**

```bash
INSTA_UPDATE=always cargo test
```

**Step 4: Commit**

```bash
git add -A && git commit -m "Implement -r -s recursive source mode (dump everything)"
```

---

## Phase 3: Cache Refactor

### Task 10: Remove local crate caching

Local crates (`CurrentCrate`) should always rebuild — no mtime tracking needed.

**Files:**
- Modify: `src/cache.rs:46-57` (return `None` for CurrentCrate cache path)
- Modify: `src/cache.rs:170-182` (remove CurrentCrate metadata creation)
- Modify: `src/cache.rs:216-226` (remove CurrentCrate validation)
- Remove: mtime-related functions (`get_newest_source_mtime`, `walk_for_newest_mtime`)
- Update: related tests

**Step 1: Change `cache_path` to return `None` for `CurrentCrate`**

```rust
CrateSource::CurrentCrate { .. } => None, // Always rebuild, no caching
```

**Step 2: Remove mtime functions and `CacheMetadata::CurrentCrate` variant**

Remove `get_newest_source_mtime`, `walk_for_newest_mtime`, and the `CurrentCrate` variant from `CacheMetadata`. Update `create_header` and `is_cache_valid` accordingly.

**Step 3: Update tests**

Remove tests for CurrentCrate caching (round trip, stale detection, mtime). Keep tests for Dependency, External, and Stdlib caching.

**Step 4: Run tests**

```bash
cargo test
cargo clippy --all-targets
```

**Step 5: Commit**

```bash
git add -A && git commit -m "Remove local crate caching (always rebuild from rustdoc)"
```

---

### Task 11: Move Dependency crate cache to global location

Dependencies are versioned and immutable (like external crates). Move them to `~/.cache/groxide/deps/`.

**Files:**
- Modify: `src/cache.rs:58-68` (update Dependency cache path)

**Step 1: Update `cache_path` for Dependency**

```rust
CrateSource::Dependency { name, version, .. } => {
    let cache_dir = dirs::cache_dir()?;
    let filename = format!("{name}-{version}{feature_suffix}.groxide");
    Some(cache_dir.join("groxide").join("deps").join(filename))
}
```

**Step 2: Update tests**

Update `cache_path_dependency_under_target` test to expect global cache path instead.

**Step 3: Run tests**

```bash
cargo test
```

**Step 4: Commit**

```bash
git add -A && git commit -m "Move dependency cache to global ~/.cache/groxide/deps/"
```

---

### Task 12: Implement `--clear-cache` (wire up from Task 4)

Task 4 added the flag and stub. Now verify it works end-to-end by clearing real cache dirs.

**Step 1: Write integration test**

```rust
#[test]
fn clear_cache_removes_directory() {
    // Create a dummy file in cache dir
    let cache_dir = dirs::cache_dir().unwrap().join("groxide").join("test-clear");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("dummy.groxide"), b"test").unwrap();

    let output = Command::new(cargo_bin!("grox"))
        .arg("--clear-cache")
        .output()
        .expect("command runs");
    assert!(output.status.success());
    // The entire groxide cache dir should be gone
    assert!(!dirs::cache_dir().unwrap().join("groxide").exists());
}
```

**Step 2: Verify and run**

```bash
cargo test
```

**Step 3: Commit**

```bash
git add -A && git commit -m "Wire up --clear-cache to remove global cache directory"
```

---

## Phase 4: Workspace Support

### Task 13: Detect workspace root and list crates

When no path argument is given and CWD is at a virtual workspace root (or the user runs without specifying a crate), detect all workspace members and list them.

**Files:**
- Modify: `src/resolve.rs` (add `is_virtual_workspace` and `workspace_members` methods to ProjectContext)
- Modify: `src/lib.rs` (add workspace detection branch in `run`)

**Step 1: Add workspace introspection to `ProjectContext`**

In `src/resolve.rs`:

```rust
/// Returns `true` if the workspace has no root package (virtual manifest).
pub(crate) fn is_virtual_workspace(&self) -> bool {
    self.metadata.resolve.as_ref()
        .and_then(|r| r.root.as_ref())
        .is_none()
}

/// Returns all workspace member packages.
pub(crate) fn workspace_member_packages(&self) -> Vec<&Package> {
    self.metadata.workspace_members.iter()
        .map(|id| &self.metadata[id])
        .collect()
}
```

**Step 2: Add workspace branch in `run()`**

In `src/lib.rs`, after project context discovery, before path parsing:

```rust
// Workspace mode: no path given + virtual workspace
if cli.path.is_none() && ctx.as_ref().is_some_and(|c| c.is_virtual_workspace()) {
    return handle_workspace(&mut out, ctx.as_ref().unwrap(), cli);
}
```

**Step 3: Implement `handle_workspace`**

New function in `src/lib.rs`:

```rust
fn handle_workspace(
    w: &mut impl Write,
    ctx: &ProjectContext,
    cli: &Cli,
) -> Result<()> {
    let members = ctx.workspace_member_packages();
    let features = FeatureFlags::from_cli(cli);
    let feature_suffix = features.cache_suffix();

    for pkg in &members {
        let source = CrateSource::CurrentCrate {
            manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
            name: pkg.name.clone(),
            version: pkg.version.to_string(),
        };
        let (index, source) = load_or_build_index(source, &features, &feature_suffix, cli.private)?;

        // Resolve crate root
        let crate_name = resolve::normalize_crate_name(&pkg.name);
        let result = query::lookup(&index, &crate_name, None);

        if let QueryResult::Found { index: idx } = result {
            // Render this crate's view based on detail tier and recursive flag
            render_workspace_member(w, &index, idx, cli, &source)?;
        }
    }
    Ok(())
}
```

**Step 4: Implement `render_workspace_member`**

Renders one crate's contribution to workspace output. Respects `-b`, `-d`, `-s`, `-r` flags. Default: crate name header + top-level children with signatures.

**Step 5: Write integration test**

This is hard to test with the fixture crate (which is a single crate, not a workspace). Add a unit test using mock data or mark as `#[ignore]` for manual testing against the rloop workspace.

**Step 6: Commit**

```bash
git add -A && git commit -m "Add workspace-wide querying (detect virtual manifest, list all crates)"
```

---

### Task 14: Workspace recursive mode

When `-r` is used with workspace mode, recursively list all items from all crates.

**Files:**
- Modify: `src/lib.rs` (update `handle_workspace` to handle recursive)

**Step 1: Update `render_workspace_member` for recursive mode**

When `cli.recursive` is set, use `collect_children_recursive` for each crate and render the full tree. Apply the appropriate detail tier (`-b`, `-d`, `-s`).

**Step 2: Test manually against rloop workspace**

```bash
cd ~/projects/rloop/rloop_v5
grox -r -b   # Should show all items from all 8 crates, names only
```

**Step 3: Commit**

```bash
git add -A && git commit -m "Support -r flag in workspace mode (recursive all crates)"
```

---

## Phase 5: Bug Fixes

### Task 15: Feature-gate hints on empty search

When search returns 0 results and the crate has non-default features, show a hint.

**Files:**
- Modify: `src/lib.rs:391-460` (update `handle_search`)
- Modify: `src/search.rs` (optional: add feature count helper)
- Need access to crate features — may need to check `DocIndex` or `CrateSource`

**Step 1: Write integration test**

In `tests/cli_happy.rs` (or `cross_crate.rs` if it needs a real crate with features):

```rust
#[test]
fn search_zero_results_hints_features() {
    // Use the fixture crate which has an "unstable" feature
    let output = grox()
        .args(["-S", "unstable_item_that_only_exists_with_feature", "groxide_test_api"])
        .output()
        .expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should mention features
    assert!(stdout.contains("hint:") || stdout.contains("--all-features"));
}
```

Note: The fixture crate has an `unstable` feature. Add a feature-gated item to the fixture to make this testable.

**Step 2: Add feature-gated item to fixture**

In `test-fixtures/groxide_test_api/src/lib.rs`:

```rust
#[cfg(feature = "unstable")]
pub fn unstable_api() -> bool {
    true
}
```

**Step 3: Implement hint in `handle_search`**

After `search::search()` returns 0 results, check if the index was built without all features. If so, rebuild with all features, re-run search, and if that finds results, print the hint:

```rust
if results.is_empty() && !features.all_features {
    // Try searching with all features
    let all_features = FeatureFlags { all_features: true, ..features.clone() };
    // ... rebuild index, search again, report count
    eprintln!("hint: {} items found with --all-features", hidden_count);
}
```

This is expensive (rebuilds index) so only do it on 0 results. The cost is acceptable since the user is already getting no results.

**Step 4: Run tests**

```bash
cargo test
```

**Step 5: Commit**

```bash
git add -A && git commit -m "Show feature-gate hint when search returns 0 results"
```

---

### Task 16: Re-export chain following on lookup failure

When `grox egui::ViewportInPixels` fails, detect it's a re-export and resolve to the source crate.

**Files:**
- Modify: `src/lib.rs` (enhance `resolve_item` or add fallback after NotFound)
- The existing `try_follow_reexport` in `src/lib.rs:564-609` handles re-exports for *found* items. This task extends it to handle *not-found* items by searching dependency indexes.

**Step 1: Understand current re-export handling**

The current code follows re-exports only when an item is already found but is a "stub" (has `reexport_source` set). For the case where the item isn't found at all, we need a different approach.

**Step 2: Add re-export search fallback**

In `src/lib.rs`, after `resolve_item` returns `NotFound`, check if the query's last segment exists in any dependency's index:

```rust
QueryResult::NotFound { query, .. } => {
    // Try to find the item name in transitive deps
    if let Some(item_name) = query.rsplit("::").next() {
        // Search dependency indexes for this item name
        // If found, suggest the correct path
    }
}
```

This is a complex feature that may require loading multiple dependency indexes. Consider a simpler first pass: when the queried crate's index contains re-export stubs that match the query suffix, follow those stubs.

**Step 3: Implementation approach**

Look at the queried crate's index for items whose `reexport_source` field points to a dependency. If the failed query matches one of these re-exported paths, follow the re-export chain (load the source crate's index, resolve the canonical item there).

**Step 4: Write test (requires cross-crate, likely `#[ignore]`)**

```rust
#[test]
#[ignore]
fn reexport_chain_resolution() {
    // This would need a real crate with re-exports like egui/epaint
    // For now, test with fixture if possible
}
```

For the fixture crate, use the existing `reexports` module — query `groxide_test_api::reexports::Helper` which re-exports from `inner::Helper`.

**Step 5: Commit**

```bash
git add -A && git commit -m "Follow re-export chains on lookup failure"
```

---

### Task 17: docs.rs metadata fallback caching

Cache which (crate, version) pairs fail with docs.rs metadata so subsequent fetches skip to default features.

**Files:**
- Modify: `src/external.rs` or `src/docgen.rs` (where docs.rs metadata is tried)
- Create or modify: a small persistent file in `~/.cache/groxide/docsrs-failures.json`

**Step 1: Find where docs.rs metadata fallback happens**

Look in `src/docgen.rs` for the docs.rs metadata attempt and retry logic.

**Step 2: Add a simple JSON file for tracking failures**

```rust
/// Records that docs.rs metadata build failed for (crate, version).
fn record_docsrs_failure(name: &str, version: &str) {
    let cache_dir = dirs::cache_dir().unwrap_or_default().join("groxide");
    let path = cache_dir.join("docsrs-failures.json");
    // Read existing, add entry, write back
}

/// Checks if docs.rs metadata build is known to fail for (crate, version).
fn is_docsrs_known_failure(name: &str, version: &str) -> bool {
    // Read file, check entry
}
```

**Step 3: Skip docs.rs metadata when known to fail**

In the build function, check `is_docsrs_known_failure` before attempting docs.rs metadata. If known failure, go straight to default features.

**Step 4: Commit**

```bash
git add -A && git commit -m "Cache docs.rs metadata build failures to skip on retry"
```

---

## Phase 6: Documentation

### Task 18: Update documentation

Update all docs to reflect new CLI, remove `-l` references, document search syntax.

**Files:**
- Modify: `CLAUDE.md` (update flag descriptions, remove `-l`)
- Modify: `docs/SKILL.md` or equivalent (if exists)
- Modify: `src/cli.rs` HELP_EXAMPLES (update examples)
- Modify: `README.md` (update examples, flag table)

**Step 1: Update CLAUDE.md**

Remove all `-l`/`--list` references. Add `-b`/`--brief`, `-d`/`--docs` descriptions. Document search syntax (`|` for OR, space for AND). Document workspace mode. Document `--clear-cache`.

**Step 2: Update help examples in cli.rs**

```rust
const HELP_EXAMPLES: &str = "\
EXAMPLES:
    grox serde::Deserialize          Struct docs with methods
    grox tokio::sync::Mutex::lock    Full method documentation
    grox tokio -S \"spawn\"            Search across crate documentation
    grox -s tokio::sync::Mutex::new  View source code with docs
    grox -r tokio::sync             List all items recursively
    grox -r -b tokio                Structural skeleton (names only)
    grox -r -s mycrate              Dump entire crate with source
    grox --impls Clone wgpu::Device  Check if Device implements Clone
    grox axum::Router               Auto-fetch external crate
    grox std::collections::HashMap  Query standard library
    grox --json serde::Serialize    JSON output
    grox serde@1.0.210::Deserialize Pin to specific version
    grox --clear-cache              Wipe documentation cache";
```

**Step 3: Update README.md**

Update the flag table, examples, and any prose that references `-l`.

**Step 4: Commit**

```bash
git add -A && git commit -m "Update docs: new flag system, search syntax, workspace mode"
```

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 1 | 1-5 | CLI flag restructure (remove `-l`, add `-b`/`-d`, composable `-r`/`-s`) |
| 2 | 6-9 | Rendering tiers (brief, docs, source+docs, recursive+source) |
| 3 | 10-12 | Cache refactor (no local cache, global deps, `--clear-cache`) |
| 4 | 13-14 | Workspace support (detect, list, recursive) |
| 5 | 15-17 | Bug fixes (feature hints, re-exports, docs.rs fallback) |
| 6 | 18 | Documentation updates |

**Execution order:** Phases 1→2→4 (sequential dependency). Phase 3 and Phase 5 can be done in parallel with Phase 2. Phase 6 is last.

**Pre-commit checks (every commit):**
```bash
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```

**Snapshot updates when output changes:**
```bash
INSTA_UPDATE=always cargo test
```
