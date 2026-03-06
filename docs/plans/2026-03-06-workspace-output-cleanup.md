# Workspace Output Cleanup

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Clean up workspace mode output: `crate` keyword for crate roots, single building progress line, drop `==` separators, double blank line between crates.

**Architecture:** Two independent changes — (1) render-level `mod` → `crate` for `DisplayItem::Crate`, (2) restructure `handle_workspace` to batch index building with a single stderr message and cleaner output formatting.

**Tech Stack:** Rust, insta snapshots

---

### Task 1: Change `mod` to `crate` for crate root display

**Files:**
- Modify: `src/render/text.rs:35` — `render_crate` header line
- Modify: `src/render/brief.rs:17-18` — split `Crate|Module` match arm
- Modify: `src/render/brief.rs:147` — test assertion
- Snapshots auto-updated: `render_crate_root_output_format.snap`, `render_feature_gate_annotation_in_listings.snap`

**Step 1: Change `render_crate` header in text.rs**

In `src/render/text.rs:35`, change:
```rust
let _ = writeln!(out, "mod {}{gate}", item.name);
```
to:
```rust
let _ = writeln!(out, "crate {}{gate}", item.name);
```

**Step 2: Split `Crate|Module` match arm in brief.rs**

In `src/render/brief.rs:17-18`, the current code:
```rust
DisplayItem::Crate { item, children } | DisplayItem::Module { item, children } => {
    let _ = writeln!(out, "{} {}", item.kind.short_name(), item.path);
```

Change to:
```rust
DisplayItem::Crate { item, children } => {
    let _ = writeln!(out, "crate {}", item.path);
    let max_kind = children
        .values()
        .flat_map(|items| items.iter())
        .map(|i| i.kind.short_name().len())
        .max()
        .unwrap_or(0);
    for items in children.values() {
        for child in items {
            let kind = child.kind.short_name();
            let _ = writeln!(out, "  {kind:<max_kind$}  {}", child.name);
        }
    }
}
DisplayItem::Module { item, children } => {
    let _ = writeln!(out, "{} {}", item.kind.short_name(), item.path);
```

Note: The child-rendering loop body is identical for both arms. Duplication is acceptable here — it's 7 lines and extracting a helper for "write header then children" adds complexity for no reuse beyond these two call sites.

**Step 3: Update brief test assertion**

In `src/render/brief.rs:147`, change:
```rust
assert!(output.contains("mod mycrate"), "header: {output}");
```
to:
```rust
assert!(output.contains("crate mycrate"), "header: {output}");
```

But wait — this test uses `build_display_item(&index, 0, ...)` which builds a `DisplayItem::Module` (not `Crate`) since the test creates a plain module item. The test name is `render_brief_module_shows_kind_and_name_only` — so the `"mod mycrate"` assertion is correct for a module. Check whether `build_display_item` returns `Crate` or `Module` for index 0 when that item is the crate root.

Check `src/render/mod.rs` for `build_display_item` to see how it decides `Crate` vs `Module`. If the crate root item (matching `index.crate_name`) produces `DisplayItem::Crate`, then this test item named `"mycrate"` with crate_name `"mycrate"` WILL match and should assert `"crate mycrate"`. If it's just based on `ItemKind`, it'll stay `Module`. Verify and adjust.

**Step 4: Update snapshots**

Run: `INSTA_UPDATE=always cargo test`

Two snapshots will update (`mod mycrate` → `crate mycrate`):
- `render_crate_root_output_format.snap`
- `render_feature_gate_annotation_in_listings.snap`

**Step 5: Run pre-commit checks**

```bash
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```

**Step 6: Commit**

```bash
git add -A
git commit -m "task: change 'mod' to 'crate' for crate root display in text and brief renderers"
```

---

### Task 2: Single building line and clean workspace output

**Files:**
- Modify: `src/lib.rs:212-277` — extract `load_or_build_index_inner`
- Modify: `src/lib.rs:1001-1078` — restructure `handle_workspace`

**Step 1: Extract `load_or_build_index_inner` from `load_or_build_index`**

The current `load_or_build_index` (lib.rs:212-277) does:
1. Check cache → return early if hit
2. `eprint!("[grox] Building index for {name} {version}...")`
3. Build index (fetch/generate JSON, parse, build)
4. Save cache
5. `eprintln!(" done ({elapsed:.1}s)")`

Extract a new function that does steps 1, 3, 4 (no printing):

```rust
/// Loads `DocIndex` from cache or builds from rustdoc JSON, without printing progress.
fn load_or_build_index_quiet(
    source: CrateSource,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Result<(DocIndex, CrateSource)> {
    // Cache check
    let cache_file = cache::cache_path(&source, feature_suffix);
    if let Some(ref path) = cache_file {
        if let Some(index) = cache::load_cached(path, &source) {
            return Ok((index, source));
        }
    }

    // Build (existing code from load_or_build_index, lines ~234-271, without eprint)
    let (json_path, source) = if let CrateSource::External { ... } = source {
        // ... external fetch ...
    } else {
        // ... local docgen ...
    };

    let json_str = std::fs::read_to_string(&json_path).map_err(|e| ...)?;
    let krate = index_builder::parse_rustdoc_json(&json_str)?;
    let crate_name = resolve::normalize_crate_name(source.name());
    let crate_version = source.version().unwrap_or("");
    let index = index_builder::build_index(&krate, &crate_name, crate_version);

    // Save cache
    if let Some(ref path) = cache_file {
        cache::save_to_cache(path, &index, &source);
    } else if let Some(path) = cache::cache_path(&source, feature_suffix) {
        cache::save_to_cache(&path, &index, &source);
    }

    Ok((index, source))
}
```

Then simplify `load_or_build_index` to wrap it:

```rust
fn load_or_build_index(
    source: CrateSource,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Result<(DocIndex, CrateSource)> {
    // Check cache first (silent on hit)
    let cache_file = cache::cache_path(&source, feature_suffix);
    if let Some(ref path) = cache_file {
        if let Some(index) = cache::load_cached(path, &source) {
            return Ok((index, source));
        }
    }

    let start = Instant::now();
    let name = source.name().to_string();
    let version = source.version().unwrap_or("").to_string();
    eprint!("[grox] Building index for {name} {version}...");

    let result = load_or_build_index_quiet(source, features, feature_suffix, private);

    match &result {
        Ok(_) => {
            let elapsed = start.elapsed().as_secs_f64();
            eprintln!(" done ({elapsed:.1}s)");
        }
        Err(_) => {
            eprintln!(); // newline so error doesn't run onto "Building..." line
        }
    }

    result
}
```

Wait — this double-checks the cache (once in `load_or_build_index`, once in `_quiet`). Better approach: have `_quiet` skip the cache check, and have the public function handle it. Or just inline the messaging into the existing function and add the `eprintln!()` on error path. Simpler:

**Revised Step 1: Just add error-path newline and extract a `_quiet` variant that skips all printing.**

Keep the existing `load_or_build_index` as-is but add `eprintln!()` before returning errors. Then create `load_or_build_index_quiet` as a copy without the `eprint!` / `eprintln!` lines. Yes there's code duplication — but these are private 30-line functions and extracting shared guts adds complexity for no external benefit.

Actually, simplest: just add a `quiet: bool` parameter to `load_or_build_index`. When `quiet`, skip the eprint/eprintln calls. Single function, no duplication.

```rust
fn load_or_build_index(
    source: CrateSource,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
    quiet: bool,
) -> Result<(DocIndex, CrateSource)> {
```

Add `if !quiet { eprint!(...) }` around the two print sites. Add `if !quiet { eprintln!(); }` on error path. Update the one call site in `run()` to pass `quiet: false`.

**Step 2: Restructure `handle_workspace`**

Replace the current `handle_workspace` (lib.rs:1005-1078) with:

```rust
fn handle_workspace(w: &mut impl Write, ctx: &ProjectContext, cli: &Cli) -> Result<()> {
    let members = ctx.workspace_member_packages();
    let features = FeatureFlags::from_cli(cli);
    let feature_suffix = features.cache_suffix();

    // Phase 1: Build all indices
    let start = Instant::now();
    eprint!("[grox] Building workspace indices...");

    let mut results: Vec<(&cargo_metadata::Package, DocIndex, CrateSource)> = Vec::new();
    let mut errors: Vec<(String, GroxError)> = Vec::new();

    for pkg in &members {
        let source = CrateSource::CurrentCrate {
            manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
            name: pkg.name.clone(),
            version: pkg.version.to_string(),
        };

        match load_or_build_index(source, &features, &feature_suffix, cli.private, true) {
            Ok((index, source)) => results.push((pkg, index, source)),
            Err(e) => errors.push((pkg.name.clone(), e)),
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(" done ({elapsed:.1}s)");

    for (name, e) in &errors {
        eprintln!("[grox] Failed to build index for {name}: {e}");
    }

    // Phase 2: Render results
    let mut first = true;
    for (pkg, index, _source) in &results {
        let query_path = QueryPath {
            crate_spec: CrateSpec::CurrentCrate,
            item_segments: Vec::new(),
        };
        let result = resolve_item(&query_path, index, None);

        if !first {
            // Double blank line between crates
            writeln!(w).map_err(GroxError::Io)?;
        }
        first = false;

        match result {
            QueryResult::Found { index: idx } => {
                if cli.recursive && cli.source {
                    render_recursive_source(w, index, idx, _source, cli)?;
                } else if cli.recursive {
                    render_recursive(w, index, idx, cli)?;
                } else {
                    let display = render::build_display_item(index, idx, cli.private);
                    if cli.json {
                        let output = render::json::render_json(&display);
                        writeln!(w, "{output}").map_err(GroxError::Io)?;
                    } else if cli.brief {
                        let output = render::brief::render_brief(&display);
                        writeln!(w, "{output}").map_err(GroxError::Io)?;
                    } else {
                        let limits = DisplayLimits::default();
                        let output = render::text::render_text(&display, &limits);
                        writeln!(w, "{output}").map_err(GroxError::Io)?;
                    }
                }
            }
            QueryResult::NotFound { .. } | QueryResult::Ambiguous { .. } => {
                eprintln!("[grox] Could not resolve crate root for {}", pkg.name);
            }
        }
    }

    Ok(())
}
```

Key changes from current code:
- **Two-phase**: build all first, then render all
- **Single progress line**: `[grox] Building workspace indices... done (Xs)`
- **Errors reported after done**: `[grox] Failed to build index for X: ...`
- **No `== name version ==` separators**: the `crate` header from render is enough
- **Double blank line between crates**: `writeln!(w)` produces one blank line; `render_text` output already starts with `crate ...` on its own line, so one `writeln!` between entries = one blank separator line. For double blank line, use two `writeln!` calls or a single `write!(w, "\n\n")`.

Note on the `_source` variable — it's needed for `render_recursive_source`. Use a more descriptive binding if clippy complains.

**Step 3: Run pre-commit checks**

```bash
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```

**Step 4: Commit**

```bash
git add -A
git commit -m "task: single building line and clean output for workspace mode"
```
