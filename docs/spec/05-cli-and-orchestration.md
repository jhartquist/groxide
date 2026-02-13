# CLI and Orchestration

Complete specification for groxide's command-line interface, `main.rs` entry point,
`lib.rs` orchestration flow, stderr messages, exit codes, and smart defaults.

Prerequisite reading: `01-types-and-data-model.md` (all types referenced here are
defined there).

---

## 1. CLI Definition (clap derive)

```rust
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// Query Rust crate documentation from the terminal
#[derive(Parser, Debug)]
#[command(name = "grox")]
#[command(version)]
#[command(about = "Query Rust crate documentation from the terminal", long_about = None)]
#[command(after_long_help = HELP_EXAMPLES)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Rust path to query (e.g., `tokio::sync::Mutex`, `serde@1.0`)
    pub path: Option<String>,

    /// Show source code instead of docs
    #[arg(short = 's', long, conflicts_with_all = ["list", "impls"])]
    pub source: bool,

    /// List children only (names + one-line summaries)
    #[arg(short = 'l', long, conflicts_with_all = ["source", "impls"])]
    pub list: bool,

    /// Expand everything (full docs for all children, no truncation)
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Full-text search across documentation
    #[arg(short = 'S', long, conflicts_with_all = ["source", "list", "impls"])]
    pub search: Option<String>,

    /// Filter by item kind
    #[arg(short = 'k', long)]
    pub kind: Option<KindFilter>,

    /// Include non-public items
    #[arg(short = 'p', long)]
    pub private: bool,

    /// JSON Lines output
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Show trait implementations (on types) or implementors (on traits)
    #[arg(short = 'i', long, conflicts_with_all = ["source", "list"])]
    pub impls: bool,

    /// Show the crate's README
    #[arg(long, conflicts_with_all = ["source", "list", "search", "impls"])]
    pub readme: bool,

    /// Path to Cargo.toml
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Comma-separated list of features to activate
    #[arg(long, value_delimiter = ',')]
    pub features: Vec<String>,

    /// Activate all available features
    #[arg(long)]
    pub all_features: bool,

    /// Do not activate the `default` feature
    #[arg(long)]
    pub no_default_features: bool,
}
```

### KindFilter enum

```rust
/// Item kinds accepted by the --kind flag.
/// Parsed case-insensitively by clap's ValueEnum.
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
pub(crate) enum KindFilter {
    /// Functions and methods
    Fn,
    /// Structs
    Struct,
    /// Enums
    Enum,
    /// Traits (includes TraitAlias)
    Trait,
    /// Type aliases (includes AssocType, ForeignType)
    Type,
    /// Constants (includes AssocConst)
    Const,
    /// Modules
    Mod,
    /// Macros (includes ProcMacro)
    Macro,
}
```

`KindFilter` converts to `ItemKind` for the query/search engine via `ItemKind::matches_filter()`.
Invalid `--kind` values produce a clap error (exit 2).

### Flag field summary

| Flag | Short | Field type | Default | Description |
|------|-------|-----------|---------|-------------|
| `path` | (positional) | `Option<String>` | `None` | Rust path to query |
| `--source` | `-s` | `bool` | `false` | Show source code instead of docs |
| `--list` | `-l` | `bool` | `false` | List children only |
| `--all` | `-a` | `bool` | `false` | Disable all truncation |
| `--search` | `-S` | `Option<String>` | `None` | Full-text search query |
| `--kind` | `-k` | `Option<KindFilter>` | `None` | Filter by item kind |
| `--private` | `-p` | `bool` | `false` | Include non-public items |
| `--json` | `-j` | `bool` | `false` | JSON Lines output |
| `--impls` | `-i` | `bool` | `false` | Show trait implementations |
| `--readme` | | `bool` | `false` | Show crate README |
| `--manifest-path` | | `Option<PathBuf>` | `None` | Path to Cargo.toml |
| `--features` | | `Vec<String>` | `[]` | Features to activate |
| `--all-features` | | `bool` | `false` | All features |
| `--no-default-features` | | `bool` | `false` | No default features |
| `--help` | `-h` | | | Print help (clap built-in) |
| `--version` | `-V` | | | Print version (clap built-in) |

### Flag conflicts

Enforced via clap `conflicts_with_all` attributes. Conflicting combinations produce a
clap error to stderr and exit code 2.

| Flag | Conflicts with |
|------|---------------|
| `--source` (`-s`) | `--list`, `--impls` |
| `--list` (`-l`) | `--source`, `--impls` |
| `--search` (`-S`) | `--source`, `--list`, `--impls` |
| `--impls` (`-i`) | `--source`, `--list` |
| `--readme` | `--source`, `--list`, `--search`, `--impls` |

### Flags that combine freely

- `--all` (`-a`) -- combines with any mode. Removes truncation in default mode. No effect on list (already unlimited), search, or source.
- `--kind` (`-k`) -- combines with `--search`, `--list`, `--json`, and default mode. Filters children in container views and search results. When kind filter produces no matches in item lookup, groxide retries without the filter (kind acts as preference, not hard constraint).
- `--private` (`-p`) -- combines with any mode. Includes non-public items in child listings and search results.
- `--json` (`-j`) -- combines with `--search` (JSON search results), `--list` (no separate effect; uses JSON doc format), `--kind`, and default mode. Produces JSON Lines output.
- Feature flags (`--features`, `--all-features`, `--no-default-features`) -- affect doc generation, combine with all modes.

### Help text

The `grox --help` output:

```
Query Rust crate documentation from the terminal

Usage: grox [FLAGS] [PATH]

Arguments:
  [PATH]  Rust path to query (e.g., `tokio::sync::Mutex`, `serde@1.0`)

Options:
  -s, --source                   Show source code instead of docs
  -l, --list                     List children only (names + one-line summaries)
  -a, --all                      Expand everything (full docs for all children)
  -S, --search <QUERY>           Full-text search across documentation
  -k, --kind <KIND>              Filter by item kind [possible values: fn, struct, enum, trait, type, const, mod, macro]
  -p, --private                  Include non-public items
  -j, --json                     JSON Lines output
  -i, --impls                    Show trait implementations (on types) or implementors (on traits)
      --readme                   Show the crate's README
      --manifest-path <PATH>     Path to Cargo.toml
      --features <FEATURES>      Comma-separated list of features to activate
      --all-features             Activate all available features
      --no-default-features      Do not activate the `default` feature
  -h, --help                     Print help
  -V, --version                  Print version

EXAMPLES:
    grox serde::Deserialize          Struct docs with methods
    grox tokio::sync -l              List module contents
    grox tokio::sync::Mutex::lock    Full method documentation
    grox -S "async file"             Search across documentation
    grox -s tokio::sync::Mutex::new  View source code
    grox axum::Router                Auto-fetch external crate from crates.io
    grox std::collections::HashMap   Query standard library
    grox --json serde::Serialize     JSON output for programmatic use
    grox serde@1.0.210::Deserialize  Pin to specific version
```

The `after_long_help` text is stored as a `const HELP_EXAMPLES: &str` in `cli.rs`.

---

## 2. main.rs Structure

`main.rs` is approximately 15 lines. All logic lives in `lib.rs`.

```rust
use clap::Parser;
use groxide::cli::Cli;
use groxide::error::EXIT_SUCCESS;
use std::process;

fn main() {
    let cli = Cli::parse();
    match groxide::run(&cli) {
        Ok(()) => process::exit(EXIT_SUCCESS),
        Err(e) => {
            eprintln!("{e}");
            process::exit(e.exit_code());
        }
    }
}
```

Responsibilities:
- Parse CLI arguments via clap.
- Call `groxide::run()`.
- On success, exit 0.
- On error, print the error to stderr (the `Display` impl on `GroxError` formats
  actionable messages), then exit with the error's exit code.

No other logic. No imports beyond clap, the crate's public API, and `std::process`.

---

## 3. lib.rs::run() -- Orchestration Flow

### Signature

```rust
pub fn run(cli: &Cli) -> Result<()>
```

Takes parsed CLI arguments, resolves the crate and item, writes documentation
output to stdout. Status/progress messages go to stderr. Returns `Ok(())` on
success, `Err(GroxError)` on failure.

### Full pseudocode

```
fn run(cli: &Cli) -> Result<()>:
    let stdout = io::stdout()
    let out = stdout.lock()

    // ── Step 1: Discover project context ──────────────────────────────
    // ProjectContext wraps cargo_metadata: workspace root, packages, deps.
    // Optional — groxide works outside Rust projects for stdlib/external queries.
    let ctx = if cli.manifest_path.is_some():
        // Explicit --manifest-path: propagate errors (user asked for this path)
        Some(ProjectContext::discover(cli.manifest_path.as_deref())?)
    else:
        // No explicit path: silently fall back to no-project mode
        ProjectContext::discover(None).ok()

    // ── Step 2: Parse query path ──────────────────────────────────────
    let query_path = QueryPath::parse(cli.path.as_deref().unwrap_or(""))
    // QueryPath has crate_spec (CurrentCrate | Named | Versioned) + item_segments

    // ── Step 3: Resolve crate source ──────────────────────────────────
    let (source, query_path) = resolve_crate_source(&ctx, query_path)?
    // See §3.1 for resolution chain details

    // ── Step 4: Load or build DocIndex ────────────────────────────────
    let features = FeatureFlags::from_cli(cli)
    let cache_config = CacheConfig::new(ctx.as_ref())
    let (index, source) = load_or_build_index(source, ctx.as_ref(), &cache_config, &features)?
    // See §3.2 for cache/build details. Stderr messages printed here.

    // ── Step 5: Handle --readme (early return) ────────────────────────
    if cli.readme:
        return handle_readme(&mut out, &source, ctx.as_ref())

    // ── Step 6: Handle --search (early return) ────────────────────────
    if let Some(query) = &cli.search:
        return handle_search(&mut out, &index, query, cli)

    // ── Step 7: Resolve item in index ─────────────────────────────────
    let kind_filter = cli.kind.map(|k| k.into())  // KindFilter -> ItemKind
    let result = resolve_item(&query_path, &index, kind_filter.as_ref(),
                              &source, ctx.as_ref(), &cache_config, &features)
    // See §3.3 for resolution pipeline (includes method lookup, multi-crate fallback)

    // ── Step 8: Handle --source (early return) ────────────────────────
    if cli.source:
        return handle_source(&mut out, &result, &index, &source)

    // ── Step 9: Render output ─────────────────────────────────────────
    handle_output(&mut out, &result, &index, cli)
    // Dispatches to default text / list / json / impls / ambiguous renderers
```

### 3.1 resolve_crate_source() -- Crate Resolution Chain

```
fn resolve_crate_source(ctx: &Option<ProjectContext>, query: QueryPath)
    -> Result<(CrateSource, QueryPath)>
```

Resolution depends on `CrateSpec`:

**CrateSpec::CurrentCrate** (no path provided):
- If `ctx` exists: resolve to current package -> `CrateSource::CurrentCrate`
- If `ctx` is None: return `Err(GroxError::ManifestNotFound)`

**CrateSpec::Versioned { name, version }** (e.g., `serde@1.0.210`):
- Always resolves to `CrateSource::External { name, version: Some(version) }`
- Version pin overrides any local dependency version

**CrateSpec::Named(name)** -- the most complex case. Resolution order:

```
1. Current crate name match (hyphen/underscore normalized)
   → CrateSource::CurrentCrate

2. Direct dependencies (via cargo metadata resolve graph, handles renames)
   → CrateSource::Dependency

3. Workspace members
   → CrateSource::Dependency

4. Transitive dependencies (all packages in cargo metadata)
   → CrateSource::Dependency

5. Standard library (name is "std", "core", or "alloc")
   → CrateSource::Stdlib

6. Auto-fetch decision:
   If ctx is None (outside project):
     → CrateSource::External { name, version: None }
     stderr: "[grox] <name> not in project deps, fetching latest from crates.io..."

   If ctx exists but name not found in steps 1-5:
     If item_segments is empty (single-segment query like `grox Mutex`):
       Call looks_like_item_name(name):
         true  → Reinterpret as item in current crate:
                 CrateSpec becomes CurrentCrate, name becomes item_segments[0]
         false → CrateSource::External { name, version: None }
                 stderr: "[grox] <name> not in project deps, fetching latest from crates.io..."
     If item_segments is non-empty:
       → CrateSource::External { name, version: None }
       stderr: "[grox] <name> not in project deps, fetching latest from crates.io..."
```

Steps 1-5 are fully offline. Step 6 requires network on first fetch only.

### 3.2 load_or_build_index() -- Cache and Build

```
fn load_or_build_index(source, ctx, cache_config, features)
    -> Result<(DocIndex, CrateSource)>

1. Compute cache path from CrateSource + feature suffix
2. Try load_cached(path, source):
   - Check cache existence
   - Check cache validity (mtime for current crate, version for deps, etc.)
   - If valid: deserialize with rmp-serde, return DocIndex
   - stderr (on hit): nothing (cache hits are silent)

3. On cache miss:
   stderr: "[grox] Building index for <crate> <version>..."

   a. For External crates (version not yet resolved):
      - Call external::fetch_crate(name, version, cache_dir)
      - stderr: "[grox] Downloading <name> <version>..."
      - This resolves version (latest from crates.io if None)
      - Returns (source_dir, resolved_version)
      - Update CrateSource with resolved version

   b. Generate rustdoc JSON:
      - Call docgen::generate_json(source, ctx, features)
      - stderr: "[grox] Generating docs for <name>..."
      - Runs: cargo +nightly rustdoc --lib --output-format json [feature flags]
      - For current crate: --all-features by default, fallback on failure:
        stderr: "[grox] Build with --all-features failed, retrying with default features..."
      - Returns path to JSON file

   c. Parse rustdoc JSON:
      - fs::read_to_string(json_path)
      - serde_json::Deserializer with disable_recursion_limit()
      - Deserialize into rustdoc_types::Crate

   d. Build index:
      - index_builder::build_index(&krate) -> DocIndex

   e. Save to cache (best-effort):
      - cache::save_cache(path, &index, source)
      - On failure: stderr: "[grox] Warning: failed to save cache: <error>"
      - Command still succeeds

   stderr: "[grox] Building index for <crate> <version>... done (<time>s)"

4. Return (DocIndex, CrateSource)
```

### 3.3 resolve_item() -- Item Resolution Pipeline

```
fn resolve_item(query_path, index, kind_filter, source, ctx, cache_config, features)
    -> QueryResult

1. If item_segments is empty:
   Lookup crate root module (normalized name, hyphens -> underscores)
   Return result

2. Try lookup with crate name prepended (full path match):
   full_segments = [crate_name] ++ item_segments
   result = index.lookup(full_segments, kind_filter)

3. If not found, try item_segments only (suffix matching):
   result = index.lookup(item_segments, kind_filter)

4. If not found and kind_filter is set, retry without kind filter:
   result = index.lookup(item_segments, None)
   (kind filter acts as preference, not hard constraint)

5. If not found and 2+ segments, try method lookup:
   parent_segments = item_segments[..len-1]
   method_name = item_segments[len-1]
   result = index.lookup_method(parent_segments, method_name, kind_filter)

6. If still not found, single-segment, current crate, and looks_like_item_name:
   Try multi-crate search across cached dependency indices
   stderr: "[grox] Searching dependencies..."
   - Load cached DocIndex for each dependency
   - Run lookup in each
   - Single dep match: return that result with its index/source
   - Multiple dep matches: combine into unified index, return Ambiguous

7. Return QueryResult (Found / Ambiguous / NotFound)
```

### 3.4 handle_readme() -- README Mode

```
fn handle_readme(out, source, ctx) -> Result<()>

Lookup order for README file:
  README.md, README.MD, Readme.md, readme.md, README, README.txt

README location by source:
  CurrentCrate  → workspace root directory
  Dependency    → package manifest parent directory
  External      → ~/.cache/groxide/<name>-<version>/
  Stdlib        → not available
                  stderr: "README not available for standard library crate '<name>'"
                  return Err(ItemNotFound)

If found: print raw content to stdout, return Ok(())
If not found:
  stderr: "No README found for <crate_name>"
  return Err(ItemNotFound) → exit 1
```

### 3.5 handle_search() -- Search Mode

```
fn handle_search(out, index, query, cli) -> Result<()>

1. Validate query is not empty/whitespace:
   Empty → Err(InvalidQuery { "search query cannot be empty" }) → exit 2

2. Run search::search(index, query, kind_filter, max_results=20)
   Returns (Vec<SearchResult>, total_count)

3. Deduplicate results by path (keep first occurrence)

4. Render:
   If cli.json: JSON Lines, one object per result with score field
   Else: plain text table
     "N results for "<query>":"
     or "N of M results for "<query>":" (when truncated)
     <kind>  <path_with_params>  <summary>

5. Print to stdout, return Ok(())
   Empty results → "0 results for "<query>":" → exit 0 (not an error)
```

Search targets the crate identified by the first path segment. Additional path
segments are ignored for search scoping. Module-level search filtering is not
supported; use `--kind` or more specific search terms to narrow results.

### 3.6 handle_source() -- Source Mode

```
fn handle_source(out, result, index, source) -> Result<()>

Single match:
  Resolve source root from CrateSource:
    CurrentCrate / Dependency → parent dir of Cargo.toml
    Stdlib → <sysroot>/lib/rustlib/src/rust/library/ (requires rust-src)
    External (with version) → ~/.cache/groxide/<name>-<version>/
    External (no version) → source not available
  Extract source lines from file at SourceSpan
  Output:
    // <relative_file_path>:<line_start>-<line_end>
    <source code verbatim>
  If span unavailable: "// source not available (macro-generated or built-in)"
  If file not found: "// source not available (<error details>)"

Ambiguous match:
  Render source for each item with separators:
    --- <path_1> ---
    // <file>:<lines>
    <source>

    --- <path_2> ---
    // <file>:<lines>
    <source>

NotFound:
  Return Err(ItemNotFound) → exit 1
```

### 3.7 handle_output() -- Default/List/JSON/Impls Rendering

```
fn handle_output(out, result, index, cli) -> Result<()>

Found { index }:
  1. Build DisplayItem from IndexItem + DocIndex:
     - Variant selection based on ItemKind (see types spec §9)
     - Apply --private flag (include/exclude non-public children)
     - Apply --kind filter (filter children in container views)
  2. Dispatch by mode:
     cli.list  → render_list(display_item, out)
     cli.impls → render_impls(display_item, out)
     cli.json  → render_json(display_item, out)
     else      → render_text(display_item, limits, out)
       where limits = DisplayLimits::expanded() if cli.all
                      DisplayLimits::default() otherwise
  3. Flush stdout, return Ok(())

Ambiguous { indices, query }:
  Dispatch by mode:
    cli.json → JSON Lines, one object per match (path, kind, signature, summary)
    cli.list → list format, one line per match (kind, path, summary)
    else     → ambiguous rendering (see §6 smart defaults for format by count)
  Return Ok(()) — ambiguous matches are NOT an error (exit 0)

NotFound { query, suggestions }:
  Return Err(ItemNotFound { query, crate_name, suggestions }) → exit 1
```

### 3.8 Private helper functions in lib.rs

All helper functions take `&mut impl Write` so they are testable with `Vec<u8>`:

```rust
/// Resolves CrateSpec to CrateSource, with single-segment item reinterpretation.
fn resolve_crate_source(ctx: &Option<ProjectContext>, query: QueryPath)
    -> Result<(CrateSource, QueryPath)>;

/// Loads DocIndex from cache or builds from rustdoc JSON.
fn load_or_build_index(source: CrateSource, ctx: Option<&ProjectContext>,
                       cache: &CacheConfig, features: &FeatureFlags)
    -> Result<(DocIndex, CrateSource)>;

/// Resolves an item with all fallback strategies.
fn resolve_item(query: &QueryPath, index: &DocIndex, kind_filter: Option<&ItemKind>,
                source: &CrateSource, ctx: Option<&ProjectContext>,
                cache: &CacheConfig, features: &FeatureFlags) -> QueryResult;

/// Handles --readme mode.
fn handle_readme(w: &mut impl Write, source: &CrateSource,
                 ctx: Option<&ProjectContext>) -> Result<()>;

/// Handles --search mode.
fn handle_search(w: &mut impl Write, index: &DocIndex,
                 query: &str, cli: &Cli) -> Result<()>;

/// Handles --source mode.
fn handle_source(w: &mut impl Write, result: &QueryResult,
                 index: &DocIndex, source: &CrateSource) -> Result<()>;

/// Handles default/list/json/impls output.
fn handle_output(w: &mut impl Write, result: &QueryResult,
                 index: &DocIndex, cli: &Cli) -> Result<()>;

/// Tries multi-crate search in cached dependency indices.
fn try_multi_crate_search(query: &QueryPath, kind_filter: Option<&ItemKind>,
                          ctx: Option<&ProjectContext>, cache: &CacheConfig)
    -> Option<(QueryResult, DocIndex, CrateSource)>;
```

---

## 4. Exit Codes

```rust
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_NOT_FOUND: i32 = 1;
pub const EXIT_ERROR: i32 = 2;
```

| Code | Meaning | When |
|------|---------|------|
| 0 | Success | Item found and rendered, search completed (even 0 results), ambiguous match rendered, `--help`, `--version` |
| 1 | Not found | Crate not found (after all resolution including auto-fetch), item not found (after all fallback strategies), README not found or not available for stdlib |
| 2 | Error | Nightly missing, rustdoc failed, cargo metadata failed, invalid CLI arguments (clap), empty search query, I/O error, cache format error, JSON parse error, external fetch failed, stdlib source missing |

Exit code mapping in `GroxError::exit_code()`:

```rust
impl GroxError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::CrateNotFound { .. } | Self::ItemNotFound { .. } => EXIT_NOT_FOUND,
            _ => EXIT_ERROR,
        }
    }
}
```

Key invariants:
- Ambiguous matches return exit 0 (not an error -- the user gets useful output).
- Empty search results return exit 0 (the search completed successfully).
- Exit code 1 enables speculative queries: `grox some::Path 2>/dev/null && echo "exists"`.

---

## 5. Stderr Message Catalog

All status/progress messages go to stderr with `[grox]` prefix. Error messages
go to stderr via `GroxError`'s `Display` impl. Content goes exclusively to stdout.

### Status messages

| Message | When |
|---------|------|
| `[grox] Building index for <crate> <version>...` | Cache miss, starting index build |
| `[grox] Building index for <crate> <version>... done (<time>s)` | Index build completed |
| `[grox] Build with --all-features failed, retrying with default features...` | Platform-specific build failure fallback for current crate |
| `[grox] <name> not in project deps, fetching latest from crates.io...` | Auto-fetch triggered (no version specified) |
| `[grox] <name> not in project deps, fetching <version> from crates.io...` | Auto-fetch triggered (version specified) |
| `[grox] Downloading <name> <version>...` | Downloading crate tarball from crates.io |
| `[grox] Generating docs for <name>...` | Running cargo rustdoc |
| `[grox] Searching dependencies...` | Multi-crate search fallback triggered |
| `[grox] Using cached <name> <version>` | (Not printed -- cache hits are silent by default) |
| `[grox] Warning: failed to save cache: <error>` | Cache save failed (non-fatal) |

Implementation: use `eprintln!("[grox] ...")` for all status messages. These are
ephemeral progress indicators -- they help the user or agent understand what is
happening during slow operations (index building, network fetches).

### Error messages

Error messages use `GroxError`'s `Display` implementation. They are printed by
`main.rs` via `eprintln!("{e}")`. The format is:

```
<message>

<details>

<actionable suggestion>
```

Examples (from the `GroxError` variants):

```
not in a Rust project

Run grox in a Rust project directory, or specify --manifest-path.
To query external crates: grox <crate>
To query stdlib: grox std, grox core, grox alloc
```

```
nightly toolchain required

Run: rustup toolchain install nightly
```

```
no item matching "Mutx" in crate 'tokio'

Did you mean:
  tokio::sync::Mutex
  tokio::sync::MutexGuard
```

### Non-error stderr messages (not from GroxError)

These are printed directly in `lib.rs` helper functions:

```
README not available for standard library crate '<name>'
No README found for <crate_name>
```

Both are printed to stderr before returning `Err(ItemNotFound)`.

---

## 6. Smart Defaults by Item Kind

The output adapts based on what the query path resolves to. This is the core
design principle: the item kind determines the output, not a mode flag.

### Default display per kind

| Item kind | Default display | With `--all` |
|-----------|----------------|-------------|
| Crate root (no path, or path == crate name) | Doc comment + children grouped by kind (Modules, Structs, Enums, ...) with one-line summaries | Same, no doc truncation |
| Module | Doc comment (truncated to 1500 chars) + children grouped by kind with one-line summaries | Same, no doc truncation |
| Struct / Enum / Union | Signature + doc (1500 chars) + variants (enum) + methods (max 15) + trait impls (max 5) | All methods, all impls, full doc |
| Trait | Signature + doc + required methods + provided methods (max 15 total) | No truncation on methods or doc |
| Function / Method | Signature + doc (1500 chars) | Full doc (no char limit) |
| Constant / Static | Signature + doc (1500 chars) | Full doc |
| TypeAlias | Signature + doc (1500 chars) | Full doc |
| Macro / ProcMacro | Signature + doc (1500 chars) | Full doc |
| Variant / Field | Signature + doc (1500 chars) | Full doc |

Rule: leaf items get docs (not just summaries). Container items get docs + a listing
of children. The 1500-char limit exists to prevent a single item with extensive
documentation from overwhelming an agent's context window.

### Truncation limits (DisplayLimits)

```rust
pub(crate) struct DisplayLimits {
    pub(crate) max_methods: usize,       // default: 15
    pub(crate) max_trait_impls: usize,   // default: 5
    pub(crate) max_doc_length: usize,    // default: 1500 chars
    pub(crate) expand_all: bool,         // --all flag disables all truncation
}
```

When items are truncated, groxide always shows the count of hidden items and how
to see more:

```
Methods: (showing 15 of 47, use --all to expand)
```

```
Trait Implementations: (showing 5 of 12, use --impls to expand)
```

Doc text truncation follows this priority:
1. Break at paragraph boundary (`\n\n`) and append `...`
2. Break at sentence boundary (`. ` or `! ` or `? `) — no suffix appended (sentence ended naturally)
3. Break at word boundary (space) and append `...`
4. Hard truncate at safe UTF-8 boundary and append `...`

All text truncation uses `floor_char_boundary()` to prevent panics on non-ASCII.

### Ambiguous match display

When multiple items match a query, display depends on count:

**2 items, trait + macro (derive disambiguation):**
```
"clap::Parser" matches 2 items:

  trait  clap::Parser      Parse command-line arguments into Self
  macro  clap::Parser      Derive macro for the Parser trait

Use --kind trait or --kind macro to select.
```

**2-5 items (brief docs for each):**
```
Found 3 items matching "Sender":

--- tokio::sync::mpsc::Sender ---
pub struct Sender<T> { /* private fields */ }
Send values to the associated Receiver.

--- tokio::sync::broadcast::Sender ---
pub struct Sender<T> { /* private fields */ }
Sending half of the broadcast channel.

--- tokio::sync::watch::Sender ---
pub struct Sender<T> { /* private fields */ }
Sends values to the associated Receiver.
```

**6+ items (condensed table):**
```
Found 12 items matching "error":

struct  tokio::io::Error                    Custom I/O error type
enum    serde::de::Error                    Deserialization error
trait   std::error::Error                   Base error trait
fn      tokio::io::Error::new               Creates a new I/O error
fn      tokio::io::Error::kind              Returns the error kind
... (7 more, use a more specific path)
```

Max 10 items shown in condensed view.

### List mode (`--list`)

```
<kind>  <path>  <summary>
```

One line per child item, column-aligned. Kinds rendered as short names
(`fn`, `struct`, `mod`, etc.). What gets listed depends on the queried item:

| Queried item | Listed items |
|-------------|-------------|
| Crate root | All top-level items grouped by kind |
| Module | All children grouped by kind |
| Struct / Enum | All methods |
| Trait | All methods (required + provided) |
| Leaf item | Single line for the item itself |

### Source mode (`--source`)

```
// <relative_file>:<line_start>-<line_end>

<source code verbatim>
```

### JSON mode (`--json`)

JSON Lines output (one JSON object per line). For doc view of a single item:

```json
{"path":"tokio::sync::Mutex","kind":"struct","signature":"pub struct Mutex<T: ?Sized>","doc":"An asynchronous Mutex...","methods":[{"name":"lock","signature":"pub async fn lock(&self) -> MutexGuard<'_, T>","summary":"Locks this mutex."}],"trait_impls":["Send","Sync","Debug","Default","From"]}
```

For crate root/module, first line is the container, then one line per child:

```json
{"path":"mycrate","kind":"mod","signature":"","doc":"..."}
{"path":"mycrate::foo","kind":"mod","signature":"","summary":"..."}
{"path":"mycrate::Bar","kind":"struct","signature":"pub struct Bar","summary":"..."}
```

For search results:

```json
{"path":"tokio::fs::read","kind":"fn","signature":"pub async fn read(path)","summary":"Reads the entire contents...","score":100}
```

For ambiguous matches:

```json
{"path":"tokio::sync::mpsc::Sender","kind":"struct","signature":"pub struct Sender<T>","summary":"Send values to the associated Receiver."}
{"path":"tokio::sync::broadcast::Sender","kind":"struct","signature":"pub struct Sender<T>","summary":"Sending half of the broadcast channel."}
```

### Impls mode (`--impls`)

On types: all trait implementations (no truncation):
```
struct tokio::sync::Mutex

pub struct Mutex<T: ?Sized>

Trait Implementations:
  impl Clone
  impl Debug
  impl Default
  impl From
  impl Send (synthetic)
  impl Sync (synthetic)
```

On traits: all known implementors.

On other items: `"<kind> <path> has no trait implementations."`

With `--json --impls`: JSON Lines per implementation:
```json
{"trait":"Debug","synthetic":false}
{"trait":"Send","synthetic":true}
```

### README mode (`--readme`)

Raw README content printed to stdout with no processing.

---

## 7. Output Routing

This is a strict invariant, never violated:

| Output type | Channel |
|-------------|---------|
| Documentation content | stdout |
| List output | stdout |
| Search results | stdout |
| JSON output | stdout |
| Ambiguous match output | stdout |
| Source code | stdout |
| README content | stdout |
| Error messages | stderr (via `GroxError` Display, printed in `main.rs`) |
| Build status messages (`[grox] Building...`) | stderr (via `eprintln!`) |
| Auto-fetch status messages | stderr |
| Cache warnings | stderr |
| README not found messages | stderr |

This separation is critical for agents: they pipe stdout into their context window
and can safely ignore stderr. Mixing content and errors on the same stream breaks
tool integration.

---

## 8. Feature Flags Handling

```rust
pub(crate) struct FeatureFlags {
    pub(crate) all_features: bool,
    pub(crate) no_default_features: bool,
    pub(crate) features: Vec<String>,
}

impl FeatureFlags {
    /// Create from CLI args.
    pub(crate) fn from_cli(cli: &Cli) -> Self;

    /// Returns true if no feature flags were explicitly set by the user.
    pub(crate) fn is_default(&self) -> bool;

    /// Apply flags to a cargo Command (adds --all-features, --no-default-features,
    /// --features as appropriate).
    pub(crate) fn apply_to_command(&self, cmd: &mut Command);

    /// Compute a stable cache suffix. Returns "" for default flags,
    /// "-feat_<16-hex-hash>" for non-default. Hash uses DJB2 on a
    /// canonical string representation.
    pub(crate) fn cache_suffix(&self) -> String;
}
```

### Feature strategy by crate source

| Source | Default strategy | Rationale |
|--------|-----------------|-----------|
| Current crate | `--all-features` | Maximizes documented items. Falls back to default features on platform build failures. |
| Project dependency | No feature flags | Cargo's feature resolver unifies from workspace. |
| External crate (auto-fetch) | Default features | Safe default; user can override. |
| Standard library | No feature flags | Stdlib has no user-facing features. |

CLI flags (`--all-features`, `--features`, `--no-default-features`) override these
defaults for ALL source types when explicitly provided.

### Cache suffix for feature variants

Non-default feature flags produce a deterministic hash suffix appended to the cache
filename: `<name>-<version>-feat_<16-hex-hash>.groxide`. This prevents cache
collisions when the same crate is queried with different feature combinations.

Default flags produce no suffix, so the common case uses the clean filename.

---

## 9. Project Setup Requirements

| Property | Value |
|----------|-------|
| Binary name | `grox` |
| Crate name | `groxide` |
| Edition | 2021 |
| MSRV | 1.85 |
| License | MIT OR Apache-2.0 |

### Dependencies

```toml
[dependencies]
cargo_metadata = "0.19"
clap = { version = "4.5", features = ["derive"] }
dirs = "6"
flate2 = "1.0"
rmp-serde = "1.3"
rustdoc-types = "0.57"
semver = "1.0"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tar = "0.4"
thiserror = "2"
ureq = { version = "2.10", features = ["json"] }

[dev-dependencies]
assert_cmd = "2.0"
insta = { version = "1.40", features = ["filters"] }
tempfile = "3"
```

### Cargo.toml binary section

```toml
[[bin]]
name = "grox"
path = "src/main.rs"
```

### Pre-commit checks

Before every commit, all three must pass:

1. `cargo fmt -- --check`
2. `cargo clippy --all-targets -- -W clippy::pedantic -D warnings`
3. `cargo test`

Or use the task runner: `mise run check`.
