# Groxide Rewrite Brief

Date: 2026-04-24

This is an as-built product and architecture brief for deciding whether to
rewrite groxide before publishing it as open source.

Sources reviewed:

- `README.md`
- `AGENTS.md`
- `docs/spec/*.md`
- `docs/plans/*.md`
- `docs/IMPLEMENTATION.md`
- `src/**/*.rs`
- `tests/**/*.rs`
- current CLI output against `test-fixtures/groxide_test_api`

Verification run:

- `cargo test` passes: 489 unit tests, 72 integration tests, 50 ignored slow/network/nightly tests.

Important context: the canonical spec files are partly stale. The current CLI
and tests reflect the March 2026 redesign: `--list` and `--all` were removed,
while `--brief`, `--docs`, `--recursive`, optional `--impls <TRAIT>`, workspace
handling, docs.rs metadata fallback, and global external/dependency cache were
added. Treat this document as the current as-built spec, not as a replacement
for rewriting the canonical spec files.

## Product Contract

Groxide is a Rust documentation query CLI. The core promise is:

```text
grox [OPTIONS] [PATH]
```

The path is the query. There are no subcommands. Output is plain text by
default, optimized for terminal and agent consumption.

Core user workflows:

- Query a crate root: `grox serde`
- Query a module: `grox tokio::sync`
- Query a type: `grox std::collections::HashMap`
- Query a method: `grox tokio::sync::Mutex::lock`
- Query a version-pinned crate: `grox serde@1.0.210::Deserialize`
- Query source: `grox -s tokio::sync::Mutex::new`
- Query recursively: `grox -r tokio::sync`
- Query a compact skeleton: `grox -r -b tokio`
- Query full recursive docs: `grox -r -d tokio`
- Search docs: `grox tokio -S "spawn"`
- Show impls: `grox --impls Clone wgpu::Device`
- Show README: `grox --readme tokio`
- Clear cache: `grox --clear-cache`

Hard output invariants:

- Documentation, search results, JSON, source, README, and ambiguity results go
  to stdout.
- Progress, status, hints, and errors go to stderr.
- Exit code 0 means successful command execution, including ambiguous matches
  and zero-result searches.
- Exit code 1 means item/crate/README not found.
- Exit code 2 means infrastructure/configuration/invalid-argument error.

## CLI Spec

Current flags:

| Flag | Meaning |
|------|---------|
| `-b`, `--brief` | Names-only compact output. Conflicts with docs, source, search, impls. |
| `-d`, `--docs` | Full rendered documentation per item. Conflicts with brief, search, impls. |
| `-s`, `--source` | Source code with file path and line numbers. Composes with `--docs`. Conflicts with brief and impls. |
| `-r`, `--recursive` | All descendants of a crate/module. Composes with brief/docs/source/json/kind/private. Conflicts with impls and search. |
| `-S`, `--search <QUERY>` | Full-text search. Space is AND, `|` is OR. Conflicts with brief/docs/source/impls. |
| `-i`, `--impls [TRAIT]` | Trait impls for types, optionally filtered by trait name. |
| `-j`, `--json` | JSON Lines output. |
| `-k`, `--kind <KIND>` | Filter by `fn`, `struct`, `enum`, `trait`, `type`, `const`, `mod`, or `macro`. |
| `-p`, `--private` | Include non-public items. |
| `--readme` | Raw crate README. |
| `--manifest-path <PATH>` | Explicit Cargo.toml. |
| `--features <LIST>` | Comma-separated features. |
| `--all-features` | Activate all features. |
| `--no-default-features` | Disable default features. |
| `--clear-cache` | Remove global groxide cache and exit. |

Path grammar:

- Empty path means current crate root.
- `crate` means a crate name if resolvable, otherwise maybe a local item name.
- `crate::item::path` means crate plus item path.
- `crate@version::item` means crates.io version pin.
- `crate@` is invalid.
- `@crate` is invalid and suggests `grox crate`.

## Crate Resolution Spec

With a Cargo project context:

1. Current crate by normalized name.
2. Direct dependencies via cargo metadata, including renamed dependencies.
3. Workspace members.
4. Transitive dependencies.
5. Standard library crates: `std`, `core`, `alloc`.
6. crates.io external fetch.

Without a Cargo project context:

1. `std`, `core`, and `alloc` resolve as stdlib.
2. Any other named crate resolves as external crates.io.
3. Empty path errors as `ManifestNotFound`.

Hyphen/underscore normalization applies when comparing crate names.

Single-segment reinterpretation:

- In a project, if an unknown single segment looks like a Rust item name,
  groxide reinterprets it as an item in the current crate.
- The heuristic treats uppercase names as item-like, hyphenated names as
  crate-like, simple underscore names as crate-like, and short lowercase words
  as item-like.

Current concern: this reinterpretation can swallow short crate names inside a
project. For example, an unknown `syn`, `url`, `cc`, or `h2` style query looks
item-like and is resolved against the current crate, with no later auto-fetch
fallback in the current implementation. The spec describes a later fallback,
but the code does not implement it.

## Rustdoc Generation Spec

All doc data ultimately comes from rustdoc JSON:

```text
cargo +nightly rustdoc --lib --output-format json -Z unstable-options
```

Generation behavior by source:

| Source | Behavior |
|--------|----------|
| Current crate | Runs from manifest parent, selects package, uses `--lib`, default cascade. |
| Dependency | Runs from workspace target dir, selects package, usually no explicit features. |
| External | Downloads source from crates.io, then generates in extracted source dir. |
| Stdlib | Uses nightly sysroot plus rust-src, isolated target dir in global cache. |

Feature behavior:

- Explicit user feature flags are passed through directly.
- For current and external crates, default generation tries docs.rs metadata
  when available.
- If docs.rs metadata is absent or fails, current and external generation tries
  `--all-features`.
- Final fallback for current and external crates is default features.
- Dependencies generally let Cargo's resolver determine features.
- Stdlib generation avoids `--all-features` unless the user explicitly passes
  feature flags.
- Known docs.rs metadata failures are cached by crate/version.

Private items:

- `--private` appends `--document-private-items`.

Notable implementation detail:

- `build_rustdoc_command` always includes `--document-hidden-items` for normal
  docgen. This is not reflected in the older specs.

## Cache Spec

Current implementation uses global cache for non-current sources:

| Source | Current implementation |
|--------|------------------------|
| Current crate | No serialized `DocIndex` cache. Rebuilds each query. |
| Dependency | `dirs::cache_dir()/groxide/deps/<name>-<version><features>.groxide` |
| Stdlib | `dirs::cache_dir()/groxide/stdlib/<name>-<toolchain><features>.groxide` |
| External | `dirs::cache_dir()/groxide/external/<name>-<version><features>.groxide` |

Cache file format:

- MessagePack via `rmp-serde`.
- Header includes groxide version, format version, created time, and
  source-specific metadata.
- Cache save is best effort and non-fatal.
- Debug builds invalidate if the groxide binary is newer than the cache.

Spec drift:

- Older specs and changelog still describe current-crate mtime cache and
  project `target/groxide` cache. The current design intentionally avoids that.

## Index Spec

`DocIndex` is the central queryable model:

- `items: Vec<IndexItem>`
- `path_map: full original-case path -> item indices`
- `name_map: lowercase simple name -> item indices`
- `suffix_map: lowercase path suffix -> item indices`
- `trait_impls: item index -> trait impls`
- `crate_name`, normalized with hyphens converted to underscores
- `crate_version`

`IndexItem` stores:

- full path
- simple name
- kind
- rendered signature
- raw docs
- summary
- source span
- child refs
- visibility/publicness
- trait-method body marker
- feature gate
- optional `reexport_source`

Index building is a four-pass transform from rustdoc JSON:

1. Build child to parent map.
2. Compute paths from rustdoc summaries, glob re-export hoisting, impl paths,
   trait item paths, and parent-chain fallback.
3. Convert rustdoc items into `IndexItem`, including use/re-export handling.
4. Link children and trait implementations.

This is the most complex pure subsystem. The complexity is mostly real:
rustdoc JSON does not give complete paths and relationships uniformly.

## Query Spec

Lookup is pure over a `DocIndex`.

Pipeline:

1. Exact path match.
2. Case-insensitive full path match.
3. Suffix match.
4. Name match for single-segment queries.
5. NotFound with Levenshtein suggestions.

Case behavior:

- All-lowercase queries are case-insensitive.
- Any uppercase character makes the query case-sensitive.

Ambiguity behavior:

- Re-export stubs are resolved or deduplicated where possible.
- Same identity re-exports are deduplicated.
- Duplicate `(path, kind)` pairs are deduplicated.
- A single crate-root primary item can auto-select over nested matches.
- Remaining matches are sorted and rendered as ambiguity output.

Method lookup:

- If normal lookup fails for `Type::method`, resolve the parent and search its
  children.
- Parent ambiguity bubbles up.
- Missing methods get method-level suggestions.

Re-export following:

- If a found item is a re-export stub, render dispatch may load the source crate
  and render the canonical item while preserving the queried stub path in text
  output.
- If lookup fails, the orchestration layer can search same-name re-export stubs
  and return those as matches.

Current concern:

- Cross-crate re-export following returns the source `DocIndex`, but not the
  source `CrateSource`. Recursive source rendering can therefore use the wrong
  source root when following a re-export.

## Search Spec

Search is per-crate full-text search.

Query syntax:

- Space means AND.
- `|` means OR.
- Empty groups are ignored.
- Empty/whitespace-only queries error.

Scoring:

- Exact name: 100.
- Name substring: 75.
- Signature match: 40.
- Module path match: 30.
- Docs match: 20.
- AND terms sum; any zero disqualifies.
- OR groups take max.

Search output:

- Max 20 results.
- Sorted by score descending, then path ascending.
- Deduped by path after truncation.
- Empty search results are success with `0 results`.
- JSON mode emits one result object per line.

Current concern:

- The older spec describes total-result reporting like `20 of 45 results`.
  The implementation returns only `Vec<SearchResult>`, so the renderer cannot
  report the untruncated total.

## Rendering Spec

Renderers are mostly pure string builders.

Default text:

- Crate root: header, docs, grouped top-level children.
- Module: header, docs, grouped children.
- Type: header, signature, docs, variants, methods, trait impls.
- Trait: header, signature, docs, required/provided methods.
- Leaf: header, signature, docs.

Other modes:

- Brief: kind/name skeleton.
- Recursive default: grouped by parent path, kind/path/signature rows.
- Recursive brief: grouped by parent path, kind/name rows.
- Recursive docs: grouped by parent path, each item with signature and docs.
- Recursive source: source blocks separated by rules.
- JSON: JSON Lines for containers/recursive/search/ambiguity, single JSON for
  leaf/type/trait doc views.
- Impls: type impls, optional trait filter; trait implementors are currently
  stubbed as empty.
- Source: kind/path/file-line header and verbatim source, optionally with docs.
- README: raw file contents.

Current concern:

- `DisplayLimits::default()` has `expand_all = true`, so default CLI rendering
  does not enforce the old 1500-character truncation limit. Unit tests exercise
  truncation only by manually setting `expand_all = false`. This conflicts with
  the token-efficiency goal and older specs.

## Tests As Spec

The most reliable behavior spec today is the test suite:

- Unit tests cover parsing, cache paths, docgen command construction, index
  conversion, query rules, search scoring, markdown stripping, rendering, and
  source formatting.
- `tests/cli_happy.rs` covers main happy-path CLI behavior and snapshots.
- `tests/cli_errors.rs` covers flag conflicts, invalid syntax, stderr/stdout,
  not-found behavior, and no-project mode.
- `tests/cross_crate.rs` and `tests/stress.rs` document intended real-world
  behavior but are ignored by default because they require nightly/network and
  can be slow.

If rewriting, promote the as-built behavior into fewer, higher-level
acceptance tests first. The existing 500-plus unit tests are useful, but they
also lock in incidental implementation details.

## Spec Drift To Resolve Before Rewrite

These should be decided explicitly before publishing or rewriting:

1. CLI spec drift: `docs/spec/04` and `docs/spec/05` still describe `--list`
   and `--all`; current code uses `--recursive`, `--brief`, and `--docs`.
2. Cache spec drift: older docs mention current-crate/project cache, but current
   code caches only non-current sources globally.
3. Truncation drift: specs and README promise token-efficient truncation;
   default code expands docs fully.
4. Multi-crate search drift: specs describe dependency-wide fallback search;
   code does not implement it.
5. Search total drift: specs describe total counts beyond 20; code cannot
   report them.
6. Trait implementor drift: `--impls` on traits is specified, but current
   render path returns no known implementors.
7. Changelog drift: changelog still lists removed `--list` and old cache policy.
8. README requirement drift: README says MSRV 1.85; Cargo.toml says Rust 1.88.

## What Feels Essential

These are the features that define groxide and should survive any rewrite:

- Path is the query.
- Rustdoc JSON index as the source of truth.
- Zero-setup resolution for current crate, dependencies, stdlib, and crates.io.
- Pure index/query/render layers.
- Smart default views by item kind.
- Recursive mode for agents.
- Brief mode for structure.
- Source mode for grounding.
- Search mode.
- JSON Lines mode.
- Strict stdout/stderr split.
- External cache and safe crates.io extraction.
- Re-export handling, at least for same-crate and common cross-crate stubs.

## What Can Be Simplified

### 1. Rewrite The Spec First

Create one current product spec before touching code:

- `docs/spec/current-cli.md`
- `docs/spec/current-resolution.md`
- `docs/spec/current-index.md`
- `docs/spec/current-rendering.md`

Delete or archive stale `--list`/`--all` sections. A rewrite without this step
will repeat old ambiguity.

### 2. Use A Typed Orchestration Pipeline

Current orchestration threads loosely related arguments through many functions.
Use explicit phases:

```rust
ParsedRequest
ResolvedCrate
LoadedIndex
ResolvedItem
RenderRequest
```

Each phase should either be pure or clearly I/O.

### 3. Make Resolution Non-Committing Until Lookup Finishes

The current item-like heuristic commits too early. Better:

1. Build candidate crate/item interpretations.
2. Try local/dependency crate match.
3. Try current-crate item lookup for item-like single segments.
4. If not found, continue to crates.io for crate-like or unknown short names.

This fixes the `syn`/`url`/`cc` style issue without removing the helpful
`grox Mutex` behavior.

### 4. Decide On Truncation Semantics

Pick one:

- Full docs by default, accepting less token efficiency.
- Truncated docs by default, with `--docs` meaning full docs.
- Default summaries for containers and full-but-truncated docs for leaves.

Then make `DisplayLimits` represent that policy. Today it says "limits" but
defaults to unlimited output.

### 5. Make Detail Tier A First-Class Enum

Replace scattered booleans with:

```rust
enum DetailTier {
    Brief,
    Default,
    Docs,
    Source,
}

struct RenderOptions {
    tier: DetailTier,
    recursive: bool,
    json: bool,
    kind: Option<ItemKind>,
    include_private: bool,
}
```

This makes composability obvious and reduces dispatch branching.

### 6. Return Search Metadata

Change search from:

```rust
fn search(...) -> Result<Vec<SearchResult>>
```

to:

```rust
struct SearchResults {
    shown: Vec<SearchResult>,
    total_before_truncation: usize,
    total_after_dedup_estimate: usize,
}
```

Then rendering can match the spec.

### 7. Treat Re-Export Following As A Target Switch

If a renderer follows a cross-crate re-export, it needs both:

- the source `DocIndex`
- the source `CrateSource`

Return a small object:

```rust
struct FollowedItem {
    index: DocIndex,
    source: CrateSource,
    item_index: usize,
    displayed_path: String,
    canonical_path: String,
}
```

This prevents source-mode bugs and makes annotations uniform.

### 8. Split Index Building Into Named Pass Objects

The index builder is complex but mostly legitimate. A rewrite should not make
it cleverer. It should make each pass auditable:

- `ParentMapPass`
- `PathPass`
- `ItemConversionPass`
- `RelationshipPass`

Each pass takes and returns a state struct. This would make rustdoc JSON quirks
easier to test without one 1400-line file carrying everything.

### 9. Keep Signature Rendering Isolated

Signature rendering is long because rustdoc type rendering is long. It should
remain isolated and heavily tested. Do not mix it into indexing or rendering.

### 10. Simplify Caching Policy

Keep the current simpler policy:

- no serialized cache for current crate
- global cache for dependency, external, stdlib
- feature suffix included in cache key
- private flag included or explicitly documented if not included

Current concern: `private` affects index contents but is not visibly part of
the serialized cache path. Either include it in the feature/cache key or make
private builds bypass the cache.

## Proposed Rebuild Architecture

```text
main.rs
  parse CLI, call run, map errors to exit codes

cli.rs
  raw clap structs, QueryPath parsing, RenderOptions conversion

run.rs
  small orchestration pipeline

resolver/
  project.rs      cargo metadata and workspace context
  crate.rs        source resolution and candidate planning
  source.rs       CrateSource and source-root calculation

docgen/
  command.rs      rustdoc command construction
  features.rs     feature cascade and docs.rs metadata
  external.rs     crates.io query/download/extract
  stdlib.rs       sysroot/rust-src handling

cache/
  key.rs          typed cache key
  store.rs        load/save/clear

index/
  model.rs        DocIndex, IndexItem, ItemKind
  build.rs        pass coordinator
  paths.rs        path computation
  relations.rs    children and impls
  signature.rs    signature rendering

query/
  lookup.rs       path/name/suffix lookup
  method.rs       method lookup
  ambiguity.rs    dedup and auto-selection
  suggestions.rs  Levenshtein
  search.rs       full-text search
  reexport.rs     re-export following

render/
  text.rs
  recursive.rs
  brief.rs
  docs.rs
  source.rs
  json.rs
  impls.rs
  ambiguous.rs
```

The main design goal is to make every module either pure or explicitly I/O.

## Rewrite Strategy

Recommended path if you rewrite:

1. Freeze current behavior with top-level acceptance snapshots.
2. Rewrite docs/spec to match current intended behavior.
3. Extract pure domain model and render options.
4. Build a new pure query/index core against fixture JSON.
5. Add docgen/cache/resolution I/O after pure behavior is stable.
6. Port integration tests.
7. Run ignored cross-crate/nightly tests before publishing.

Recommended path if you do not rewrite:

1. Update stale specs, README, changelog, and CONTRIBUTING.
2. Fix the single-segment short-crate fallback.
3. Fix or explicitly remove multi-crate search from specs.
4. Fix cache key handling for `--private`.
5. Decide and implement truncation semantics.
6. Return search totals.
7. Fix re-export source rendering to carry `CrateSource`.
8. Run `cargo fmt -- --check`, `cargo clippy --all-targets`, `cargo test`.

## Recommendation

A full rewrite is not required because the current project is already highly
tested and the core architecture is recognizable. But a targeted rewrite of
the orchestration, option modeling, cache keying, and spec files would pay off.

The most valuable "simpler fashion" is not fewer features. It is fewer hidden
states:

- one current spec
- one typed request/options model
- one non-committing resolution plan
- one cache-key model
- one render dispatch model
- pure query/render/index code separated from I/O

The codebase feels publishable after a spec refresh and a handful of correctness
fixes. A ground-up rewrite is worth it only if you want the public repository to
show a deliberately designed architecture from day one, not just working code.
