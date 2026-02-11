# groxide Implementation Plan

Implementation tasks for building groxide from scratch. Each task references specific
sections of the spec files in `docs/spec/` — those are the single source of truth.

---

## Conventions

- **TDD**: Write tests first, then implement. Tests define expected behavior.
- **Pre-commit**: Every task ends with `cargo fmt`, `cargo clippy --all-targets -- -W clippy::pedantic -D warnings`, `cargo test` all passing.
- **Commits**: One commit per task. Message format: `task N: <summary>`
- **Spec references**: Each task lists the spec sections it implements. The implementer should read those sections before starting.

---

## Task Dependency Graph

```
1 → 2 → 3 ─┬→ 4 → 5 → 6 ─┬→ 8 → 9 ──┐
            │              │          │
            │              └→ 10 ─────┤
            │                         │
            │  11 → 12 → 13 → 14 ────┤
            │                         │
            └→ 15 → 16 → 17 → 18 ────┤
                                      │
                              19 ←────┘
                              ↓
                         20 → 21 → 22 → 23 → 24
```

Two parallel tracks after task 3:
- **Track A** (pure logic): fixture → index → signature → query → search
- **Track B** (I/O): resolve → docgen → cache → external/stdlib
- **Track C** (rendering): text → list → json → ambiguous/impls/source/readme

Tracks A, B, C converge at task 19 (orchestration).

---

## Phase 1: Foundation

### Task 1: Initialize groxide project

Create the project skeleton.

**Creates:**
- `Cargo.toml` with all metadata and dependencies (see spec 05 §9, architecture/04-project-setup.md for exact contents)
- `src/main.rs` — empty main
- `src/lib.rs` — empty
- `LICENSE-MIT`, `LICENSE-APACHE`
- `.github/workflows/ci.yml`
- `CLAUDE.md` with project conventions
- `mise.toml` with check/test/clippy tasks
- `.gitignore`
- Skeleton `README.md`

**Acceptance:** `cargo check` passes. `cargo clippy` passes. CI config is valid YAML.

**Spec:** 05 §9 (project setup requirements)

---

### Task 2: Error types & core domain types

**Creates:**
- `src/error.rs` — `GroxError` enum, exit codes, `Result<T>` alias, `format_suggestions()`
- `src/types.rs` — `ItemKind`, `KindCategory`, `GroupedItems`, `IndexItem`, `ChildRef`, `SourceSpan`, `TraitImplInfo`, `DocIndex`, `QueryResult`, `SearchResult`, `DisplayItem`, `DisplayLimits`

**Tests:**
- Error display messages contain expected text
- Exit code mapping (CrateNotFound → 1, ManifestNotFound → 2, etc.)
- `ItemKind::short_name()` for all variants
- `ItemKind::matches_filter()` grouping rules
- `ItemKind::category()` mapping
- `ItemKind::is_primary()` for expected kinds
- `KindCategory::header()` text
- `GroupedItems` grouping and sorting
- `DocIndex::add_item()` populates all three maps
- `DocIndex` suffix map generates correct keys
- Suggestion formatting (dedup, cap at 5, ellipsis)

**Acceptance:** `cargo test` passes. All types compile. No `todo!()` in error.rs or types.rs.

**Spec:** 01 (entire file)

---

### Task 3: CLI skeleton + QueryPath parsing

**Creates:**
- `src/cli.rs` — `Cli` struct (clap derive), `KindFilter` enum, `QueryPath`, `CrateSpec`, `FeatureFlags`
- `src/main.rs` — parse args, call `groxide::run()`, handle exit codes (~15 lines)
- `src/lib.rs` — `pub fn run(cli: Cli) -> Result<()>` stub returning `Ok(())`

**Tests:**
- `--help` produces expected output (snapshot)
- `--version` works
- Invalid flags produce clap errors
- Flag conflicts enforced (--search vs --source, etc.)
- `QueryPath::parse("")` → CurrentCrate, empty items
- `QueryPath::parse("tokio::sync::Mutex")` → Named("tokio"), items=["sync", "Mutex"]
- `QueryPath::parse("tokio@1.40.0::sync")` → Versioned, version="1.40.0", items=["sync"]
- `QueryPath::parse("@serde")` → error with helpful message about dropped @ prefix
- `QueryPath::parse("crate@")` → error about missing version
- `KindFilter` → `ItemKind` conversion for all 8 variants
- `FeatureFlags::cache_suffix()` — deterministic hash, empty for defaults

**Acceptance:** Binary builds and runs `grox --help`. All parse edge cases tested.

**Spec:** 05 §1-2, 01 §7

---

## Phase 2: Index Layer

### Task 4: Test fixture crate

**Creates:**
- `test-fixtures/groxide_test_api/` — a Rust library exercising all item kinds
- Must include: structs (with generics), enums (with variants), traits (with required + provided methods), functions, consts, type aliases, macros, statics, unions
- Must include: re-exports (`pub use`), glob re-exports (`pub use module::*`), nested modules (3+ levels), doc comments with examples, private items, feature-gated items (`#[cfg(feature = "...")]`)
- Pre-generate rustdoc JSON: `cargo +nightly rustdoc --output-format json -Z unstable-options` and commit the JSON file as `test-fixtures/groxide_test_api.json`

**Acceptance:** `cargo check` in fixture dir passes. JSON file exists and is valid. JSON contains all expected item kinds.

**Spec:** 03 §1 (rustdoc generation command)

---

### Task 5: Signature rendering

**Creates:**
- `src/signature.rs` — `render_signature(item: &rustdoc_types::Item, index: &rustdoc_types::Crate) -> String`

**Tests (using fixture JSON):**
- Struct signature: `pub struct Name<T: Bound>`
- Enum signature: `pub enum Name<T>`
- Function signature: `pub fn name(param: Type) -> ReturnType`
- Trait signature: `pub trait Name: SuperTrait`
- TypeAlias: `pub type Name = Type`
- Constant: `pub const NAME: Type`
- Macro: `macro_rules! name`
- Generic parameters with bounds and where clauses
- Method signature (function inside impl block)

**Acceptance:** All 18 ItemKind variants produce reasonable signatures. Tests pass.

**Spec:** 03 §9

---

### Task 6: Index builder

**Creates:**
- `src/index_builder.rs` — `build_index(krate: &rustdoc_types::Crate, crate_name: &str, crate_version: &str) -> DocIndex`

Implements the 4-pass algorithm:
1. Parent map construction
2. Path computation (BFS from root, glob re-export hoisting, max depth 20)
3. Item conversion (ItemEnum → IndexItem, visibility, re-exports, feature gates)
4. Children & relationships (impl methods, trait impls, module children)

Also implements the serde_json recursion limit fix (`parse_rustdoc_json()`).

**Tests (using fixture JSON):**
- Correct item count in resulting DocIndex
- Path map has expected paths
- Suffix map generates correct suffixes for a known item
- Name map is case-insensitive
- Re-exported items have correct paths
- Feature-gated items have `feature_gate` populated
- Public vs private items correctly flagged
- Trait impls stored in DocIndex.trait_impls (not on IndexItem)
- Children are correctly linked (struct has methods, module has children)
- Enum has variant children

**Acceptance:** Fixture JSON parses into a DocIndex with correct structure. All maps populated.

**Spec:** 03 §2-8, 01 §12

---

## Phase 3: Query & Search (pure logic, no I/O)

### Task 7: Query engine — core lookup

**Creates:**
- `src/query.rs` — `lookup(index: &DocIndex, query: &str, kind_filter: Option<ItemKind>) -> QueryResult`

Implements the 5-stage pipeline:
1. Exact path match
2. Case-insensitive path match
3. Suffix match (with non-duplicate filtering)
4. Name match
5. NotFound

Plus: case sensitivity rules (all-lowercase → case-insensitive, any uppercase → exact),
kind filter application with relaxation fallback.

**Tests (using synthetic DocIndex built in-test):**
- Exact path match returns Found
- Case-insensitive match ("mutex" finds "Mutex")
- Suffix match ("sync::Mutex" finds "tokio::sync::Mutex")
- Kind filter narrows results
- Kind filter relaxation when filter produces no results
- Multiple matches return Ambiguous
- No match returns NotFound
- Case sensitivity: "Mutex" only matches "Mutex", not "mutex"

**Acceptance:** All lookup stages work. Pure function, no I/O.

**Spec:** 02 §4

---

### Task 8: Query engine — ambiguity, suggestions, method lookup

**Creates (extends query.rs):**
- `classify_results()` — dedup sequence: stub resolution → auto-selection → (path,kind) dedup
- `generate_suggestions()` — Levenshtein distance ≤ 3, max 5
- `lookup_method()` — parent resolution + child search
- `looks_like_item_name()` — the heuristic for single-segment disambiguation

**Tests:**
- Crate-root auto-selection: single primary kind at root wins
- Dedup removes (path, kind) duplicates
- Re-export stubs resolved to canonical items
- Suggestions for typos (e.g., "Mutx" → "Mutex")
- Suggestion dedup and cap at 5
- Method lookup: "Mutex::lock" finds lock method
- Method lookup with ambiguous parent
- `looks_like_item_name("Mutex")` → true (uppercase)
- `looks_like_item_name("serde")` → false (lowercase, >4 chars)
- `looks_like_item_name("new")` → true (common method)
- `looks_like_item_name("my-crate")` → false (hyphen)

**Acceptance:** Full query engine works. Ambiguity correctly handled. Suggestions generated.

**Spec:** 02 §2, §5, §6

---

### Task 9: Search engine

**Creates:**
- `src/search.rs` — `search(index: &DocIndex, query: &str, kind_filter: Option<ItemKind>) -> Vec<SearchResult>`

Implements:
- Search index construction (pre-lowercase name, path, signature, first 500 chars docs)
- OR query parsing (split on `|`)
- 5-tier scoring: 100 (name exact) → 75 (name substring) → 40 (signature) → 30 (module path) → 20 (docs)
- Sort by score desc, cap at 20
- Path-based dedup for re-exports

**Tests (using synthetic DocIndex):**
- Exact name match scores 100
- Substring match scores 75
- Signature match scores 40
- Doc match scores 20
- OR query ("Mutex | RwLock") finds both
- Results capped at 20
- Empty query returns error
- Kind filter restricts results

**Acceptance:** Search produces correctly scored, sorted, deduped results. Pure function, no I/O.

**Spec:** 02 §7

---

## Phase 4: Rendering

### Task 10: Plain text renderer + markdown stripping + truncation

**Creates:**
- `src/render/mod.rs` — dispatch + `build_display_item()` + shared helpers
- `src/render/text.rs` — plain text rendering for all DisplayItem variants
- Markdown stripping logic (inline in render helpers or separate function)
- Truncation logic (sentence → paragraph → word → hard boundary, UTF-8 safe)

**Tests (snapshot tests with insta):**
- Crate root output format
- Module output format
- Struct with methods (truncated at 15)
- Enum with variants
- Trait with required + provided methods
- Function (leaf) with full docs
- Constant (leaf)
- Feature gate annotation in listings
- Truncation at ~1500 chars (verify `...` appended)
- Truncation respects UTF-8 boundaries (test with multi-byte chars)
- `--all` disables truncation
- Markdown stripping: bold, italic, links, code, code blocks, headers

**Acceptance:** All item kinds render correctly. Snapshot tests pass.

**Spec:** 04 §1-8

---

### Task 11: List + JSON + ambiguous + impls + source + readme renderers

**Creates:**
- `src/render/list.rs` — list mode (kind + path + summary, column-aligned)
- `src/render/json.rs` — JSON doc view + JSON Lines list mode
- `src/render/ambiguous.rs` — few (2-5) and many (6+) match display
- Impls rendering (in text.rs or ambiguous.rs)
- Source view rendering
- README view rendering

**Tests (snapshot tests):**
- List output format with column alignment
- JSON doc view for struct, trait, crate root (with top_level_items fix)
- JSON Lines list output
- Ambiguous display: 2 matches (brief), 6 matches (condensed)
- Impls display: non-synthetic first, then synthetic
- Source view format with file header
- Source unavailable message
- README found/not found
- `--json` combined with `--list`
- JSON ambiguous output (array)

**Acceptance:** All output modes work. JSON crate-root includes top_level_items.

**Spec:** 04 §9-19

---

## Phase 5: Resolution & I/O

### Task 12: Crate resolution & ProjectContext

**Creates:**
- `src/resolve.rs` — `ProjectContext`, `discover()`, `resolve_crate()`

Implements:
- Cargo.toml discovery (walk up from CWD or --manifest-path)
- cargo_metadata invocation for dependency list
- Resolution chain: current crate → deps → workspace → transitive → stdlib
- Hyphen-to-underscore normalization
- Renamed dependency handling

**Tests:**
- ProjectContext::discover() in a real cargo project (the groxide project itself)
- resolve_crate with known dependency name
- Hyphen normalization: "serde-json" → "serde_json"
- stdlib recognition: "std", "core", "alloc"
- Unknown crate returns error with suggestions

**Acceptance:** Resolution chain works for local project. Tests pass.

**Spec:** 02 §1

---

### Task 13: Rustdoc JSON generation

**Creates:**
- `src/docgen.rs` — `generate_rustdoc_json(source: &CrateSource, features: &FeatureFlags) -> Result<PathBuf>`

Implements:
- `cargo +nightly rustdoc --output-format json -Z unstable-options`
- Per-source feature flag handling
- --lib flag for non-workspace crates
- --document-private-items when --private
- Nightly detection with helpful error
- Platform-specific build failure: retry without --all-features

**Tests:**
- Nightly detection (check if nightly is available)
- JSON path construction (hyphen → underscore)
- Feature flag command construction (unit test on command builder, not execution)

**Acceptance:** Can generate rustdoc JSON for the groxide project itself.

**Spec:** 03 §1

---

### Task 14: Disk cache

**Creates:**
- `src/cache.rs` — `load_cached()`, `save_to_cache()`, cache path computation

Implements:
- Project cache: `target/groxide/<name>-<version>.groxide`
- Global cache: `~/.cache/groxide/<name>-<version>.groxide`
- rmp-serde serialization/deserialization
- Atomic writes (temp file + rename)
- Invalidation: mtime for current crate, version for deps
- Feature flag cache suffix

**Tests (using tempdir):**
- Round-trip: save DocIndex, load it back, verify equality
- Cache path includes version
- Feature suffix changes path
- Atomic write: temp file cleaned up
- Stale cache detected by mtime

**Acceptance:** Cache save/load works. Atomic writes confirmed.

**Spec:** 03 §10

---

### Task 15: External crate auto-fetch

**Creates:**
- `src/external.rs` — `fetch_external_crate()`, crates.io API, tar extraction

Implements:
- crates.io API for version resolution (exact, partial, latest)
- Download + gunzip + untar
- Path traversal protection
- Timeouts (connect=10s, read=30s)
- Generate rustdoc JSON for extracted crate

**Tests (network tests behind `#[ignore]`):**
- Version resolution for a known crate
- Path traversal protection (malicious tar entry)
- Cache directory structure
- Non-existent crate returns error

**Acceptance:** Can fetch and build docs for a small external crate.

**Spec:** 03 §11

---

### Task 16: Stdlib resolution

**Creates:**
- `src/stdlib.rs` — sysroot detection, stdlib doc generation, per-toolchain caching

**Tests:**
- Sysroot detection
- std/core/alloc recognition
- Toolchain hash extraction

**Acceptance:** Can generate/cache stdlib docs.

**Spec:** 03 §12

---

## Phase 6: Integration

### Task 17: Orchestration — lib.rs run()

**Creates/updates:**
- `src/lib.rs` — full `run()` implementation wiring all modules together

Implements:
- Complete flow: CLI → discover → resolve → load/build index → query → render → output
- Auto-fetch fallback when crate not in deps
- Single-segment reinterpretation
- Multi-crate search fallback
- --search dispatch
- --readme dispatch
- --source dispatch
- All stderr progress messages
- Exit code handling

**Tests (integration tests using fixture crate):**
- Basic query returns expected output
- Unknown item returns exit code 1
- Crate root query (no item segments)
- Search query returns results
- List mode output
- JSON mode output

**Acceptance:** End-to-end flow works for the fixture crate. All output modes produce correct results.

**Spec:** 05 §3

---

### Task 18: CLI integration tests — happy paths

**Creates:**
- `tests/cli_happy.rs` — end-to-end binary tests with assert_cmd

**Tests:**
- Default output for a struct
- Default output for a module
- Default output for a function
- `--list` mode
- `--json` mode
- `--json --list` combined
- `--search "query"` mode
- `--source` mode
- `--impls` mode
- `--all` expands truncation
- `--kind fn` filters to functions
- `--private` includes private items
- Exit code 0 for all above
- Snapshot tests for output format stability

**Acceptance:** All happy-path scenarios pass through the binary.

**Spec:** 05 §4-6, 04 (all sections for expected output)

---

### Task 19: CLI integration tests — edge cases & errors

**Creates:**
- `tests/cli_errors.rs` — error scenarios

**Tests:**
- Typo query with "Did you mean" suggestions
- Unknown crate name (exit code 1)
- Invalid flag combinations (exit code 2)
- Empty search query (error)
- `--version` output
- `--help` output (snapshot)
- Deep nested path query
- Unicode in doc comments
- `crate@version` syntax
- `@crate` syntax (helpful error about dropped prefix)

**Acceptance:** All error paths produce correct exit codes and messages.

**Spec:** 05 §4-5, 01 §11

---

## Phase 7: Validation & Polish

### Task 20: Cross-crate validation

- Test against real crates: serde, tokio, clap (available as transitive deps)
- Verify output quality matches qdoc's proven output
- Test auto-fetch path with a small external crate
- Verify typenum works (recursion limit fix)

**Acceptance:** Queries against real crates produce useful, correct output.

---

### Task 21: Stress test

- Port stress test scripts from qdoc
- Run top-20 crate stress test
- Compare results against qdoc baseline
- Verify typenum recursion fix
- Verify UTF-8 truncation crashes are fixed
- Document any regressions

**Acceptance:** ≥96% pass rate (matching qdoc's baseline). No crashes.

---

### Task 22: README, CHANGELOG, CONTRIBUTING

- Full README with description, badges, installation, usage examples
- CHANGELOG.md with v0.1.0 entry
- CONTRIBUTING.md (build, test, snapshot update, nightly requirement)

**Acceptance:** README renders correctly. All links valid.

---

### Task 23: Final polish

- `cargo fmt -- --check` clean
- `cargo clippy --all-targets -- -W clippy::pedantic -D warnings` clean
- `cargo test` all passing
- `cargo doc --no-deps` builds without warnings
- `cargo package --list` shows clean contents
- No remaining `todo!()` in code
- Review all `pub` items — minimize public API surface

**Acceptance:** All checks pass. Ready for v0.1.0.

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 1: Foundation | 1-3 | Project skeleton, types, CLI |
| 2: Index | 4-6 | Fixture, signatures, index builder |
| 3: Query/Search | 7-9 | Query engine, search engine |
| 4: Rendering | 10-11 | All output formats |
| 5: I/O | 12-16 | Resolution, docgen, cache, external, stdlib |
| 6: Integration | 17-19 | Orchestration, integration tests |
| 7: Polish | 20-23 | Validation, stress test, docs, final review |

**Total: 23 tasks** (down from 25 in the original plan — consolidated renderers and tests)

Phases 3, 4, and 5 can proceed in parallel after their respective prerequisites are met.
