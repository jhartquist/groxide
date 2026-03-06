# Groxide CLI Redesign, Workspace Support, and Bug Fixes

Date: 2026-03-05

## Context

Analysis of a 2-hour AI agent session (38 grox calls, 4 unfamiliar crates, zero web searches) revealed 6 issues causing 34% of queries to fail. Separately, two new features are needed: workspace-wide querying and tiered verbosity. This is also an opportunity to simplify the CLI surface.

## Design Decisions

### 1. Remove `-l`/`--list` Flag

The default view for modules/crates already lists children. `-l` provides a "slightly different list format" which is hard to explain and adds cognitive load. Remove it entirely.

### 2. Composable Detail Tiers

Four strictly additive tiers controlled by flags. Each tier is a superset of the previous:

| Short | Long | Detail Level |
|-------|------|-------------|
| `-b` | `--brief` | kind + name |
| *(default)* | | kind + name + signature |
| `-d` | `--docs` | kind + name + signature + full rendered docs |
| `-s` | `--source` | kind + name + signature + full rendered docs + source code |

Key changes:
- `-s`/`--source` always includes rendered docs above source code (docs + source, not source alone)
- `-b`/`--brief` is new (compact names-only output)
- `-d`/`--docs` is new (full docs without source)
- The old one-line summary truncation in list/recursive mode is replaced by this clean tier system: default shows signatures (no truncated docs), `-d` shows full docs

### 3. Composable Scope

Two scope modifiers compose with all detail tiers:

| Short | Long | Scope |
|-------|------|-------|
| *(default)* | | single item (+ children for containers) |
| `-r` | `--recursive` | all descendants |

All combinations are valid:

```
grox crate              # overview + top-level children with signatures
grox crate -b           # just top-level child names
grox crate -d           # top-level children with full docs
grox crate -r           # recursive tree with signatures
grox crate -r -b        # recursive, names only (structural skeleton)
grox crate -r -d        # recursive with full docs
grox crate -r -s        # recursive with docs + source (dump everything)
grox crate::mod::func   # single item with docs (smart default)
grox crate::mod::func -s # single item docs + source
```

### 4. Workspace-Wide Querying

Auto-detect workspace context:
- At workspace root (virtual manifest) or outside any specific crate: target is the workspace
- Inside a specific crate directory: target is that crate (current behavior)

Default workspace view (no flags): show each crate with its top-level children (same as running `grox <crate>` for each workspace member):

```
rloop_core
  mod    config
  mod    traits
  struct Task           fn new(name: &str) -> Self
  enum   TaskStatus     Pending, Running, Completed, Failed

rloop_db
  mod    memory
  mod    sqlite
  trait  Store          fn get(&self, id: &str) -> Result<Task>

...
```

With `-r`: recursive tree for all crates. With `-r -s`: full source dump of entire workspace. Detail flags compose identically.

Indexing: all workspace crates are indexed sequentially on first query. Parallel indexing deferred as future optimization if needed.

### 5. Global Cache for External Crates Only

**External crates** (versioned, immutable): cached at `~/.cache/groxide/` (respecting `XDG_CACHE_HOME`). Keyed by `(crate_name, version, features)`. Cached permanently.

**Local crates** (workspace members, current crate): no caching. Rebuild index every query. Cargo's incremental compilation in `target/` makes repeated `cargo rustdoc` fast. This eliminates mtime invalidation complexity.

**Cache management:**

```
grox --clear-cache    # wipes ~/.cache/groxide/ entirely
```

One flag, no arguments, clears everything. Indexes rebuild automatically on next query.

### 6. `--impls` Trait Filtering

Change `--impls` from a bare boolean flag to an optional value:

```
grox wgpu::Device --impls           # all trait impls (current behavior)
grox wgpu::Device --impls Clone     # does Device impl Clone? yes/no + impl block
```

When a trait name is provided, filter to only matching impls. Gives a targeted answer instead of a wall of implementations.

### 7. Feature-Gate Hints

When a search (`-S`) returns 0 results and the crate has non-default features, show a hint:

```
0 results for "viridis"
hint: 14 items found with --all-features
```

Implementation: on 0 results, check if the crate has non-default features. If so, internally re-run the search with all features enabled and report the count (without listing items).

### 8. Re-Export Chain Following

When a path lookup fails (e.g., `grox egui::ViewportInPixels`), automatically follow re-export chains to resolve the item in its source crate.

The lookup should transparently resolve `egui::ViewportInPixels` to `epaint::ViewportInPixels` by detecting the re-export in egui's rustdoc JSON and loading the source crate's index.

### 9. docs.rs Metadata Fallback Caching

Cache which `(crate_name, version)` pairs fail when built with docs.rs metadata. On subsequent fetches of the same crate+version, skip straight to default features. Stored alongside the global cache.

### 10. Search Documentation Update

The multi-term search syntax already works (`|` for OR, space for AND) but isn't well documented. Update CLAUDE.md/SKILL.md/help text to make this clear. The session failure was from using shell-escaped `\|` instead of bare `|`.

## CLI Flag Summary (After Changes)

| Short | Long | Type | Purpose |
|-------|------|------|---------|
| `-b` | `--brief` | bool | Names only (compact) |
| `-d` | `--docs` | bool | Full rendered documentation |
| `-s` | `--source` | bool | Rendered docs + source code |
| `-r` | `--recursive` | bool | All descendants |
| `-S` | `--search` | string | Full-text search |
| `-i` | `--impls` | optional string | Trait implementations (optionally filtered by trait name) |
| `-k` | `--kind` | KindFilter | Filter by item kind |
| `-p` | `--private` | bool | Include non-public items |
| `-j` | `--json` | bool | JSON Lines output |
| | `--readme` | bool | Show crate README |
| | `--all-features` | bool | Activate all features |
| | `--features` | string list | Specific features to activate |
| | `--no-default-features` | bool | Disable default features |
| | `--manifest-path` | path | Path to Cargo.toml |
| | `--clear-cache` | bool | Wipe global cache |

Removed: `-l`/`--list` (redundant with default container behavior)

## Rust-Analyzer LSP Complement

The rust-analyzer LSP plugin for Claude Code provides 9 tools (hover, goToDefinition, findReferences, workspaceSymbol, call hierarchies, etc.) that operate on local project source code with file+line positioning.

Groxide complements LSP by covering:
- External crate API browsing/searching (LSP can't browse full API of dependencies)
- cfg-gated source code that docs.rs hides
- Token-efficient structured output for AI agents
- Recursive workspace dumps for context building
- Version-pinned queries for API migration

No design changes needed to accommodate LSP — the tools serve different purposes with minimal overlap.

## Scope Summary

| Category | Items |
|----------|-------|
| CLI changes | Remove `-l`, add `-b`/`--brief`, add `-d`/`--docs`, make `-s` include docs, make `-r` composable with all detail flags, `--impls` optional filter, `--clear-cache` |
| New features | Workspace-wide querying, global external cache |
| Bug fixes | Feature-gate hints, re-export following, docs.rs fallback caching |
| Docs | Multi-term search syntax |
