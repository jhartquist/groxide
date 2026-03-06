# groxide — Project Context

## What is this?

A Rust CLI tool that lets LLM coding agents (and humans) query crate documentation from the terminal. Inspired by `go doc` — the path is the query, smart defaults by item kind.

## Key files

- `docs/spec/` — The canonical specification (5 spec files). All implementation decisions should be checked against these.
- `docs/IMPLEMENTATION.md` — Task breakdown, dependency graph, and phased plan.

## Design principles

1. **Path is the query.** `grox serde::Deserialize` — no subcommands.
2. **Smart defaults.** 80% of queries need zero flags.
3. **Progressive disclosure.** Crate -> module -> type -> method drill-down.
4. **Token-efficient.** ~200-800 tokens per query, truncation by default.
5. **Zero setup.** Auto-builds and caches index on first use.
6. **Plain text output.** Not markdown. Indentation-based like `go doc`.

## CLI flags and detail tiers

**Detail tiers** (from least to most verbose):
- `-b`/`--brief` — Item names only (compact skeleton)
- *(default)* — Kind, path, and signature (single-item views include full docs)
- `-d`/`--docs` — Full rendered documentation per item
- `-s`/`--source` — Source code with file path and line numbers

`-b` is exclusive with `-d` and `-s`, but `-d` and `-s` compose: `-d -s` shows source with full docs included.

**Composability:** `-r`/`--recursive` works with any detail tier:
- `grox -r crate` — recursive default (kind, path, signature)
- `grox -r -b crate` — structural skeleton (names only)
- `grox -r -d crate` — recursive with full docs
- `grox -r -s crate` — dump entire crate with source
- `grox -r -d -s crate` — recursive with docs + source

**Orthogonal modifiers** (compose with any detail tier and `-r`):
- `-j`/`--json` — JSON Lines output
- `-p`/`--private` — Include non-public items
- `-k`/`--kind <KIND>` — Filter by item kind

**Other flags:**
- `-S`/`--search <QUERY>` — Full-text search. Use `|` for OR, space for AND.
- `-i`/`--impls [TRAIT]` — Show trait implementations; optionally filter by trait name (e.g., `--impls Clone`)
- `--readme` — Show crate README
- `--clear-cache` — Wipe the global documentation cache and exit

**Workspace mode:** When run inside a Cargo workspace (virtual manifest), groxide iterates over all workspace members. Queries resolve through: current crate → direct deps → workspace members → transitive deps → stdlib → crates.io auto-fetch.

## Tech stack

- Rust (edition 2021) + clap (derive) for CLI
- `rustdoc-types` for parsing rustdoc JSON
- `cargo_metadata` for dependency resolution
- `rmp-serde` for index cache serialization
- `cargo +nightly rustdoc --output-format json` for doc generation

## Architecture

```
CLI (clap) -> Query Engine -> Index Layer -> Cache Layer -> Rustdoc JSON Generation
```

- **CLI**: Parse args, dispatch to query engine
- **Crate Resolution**: Map first path segment to a crate source (current, dependency, @external, stdlib)
- **Index Layer**: Parse rustdoc JSON into queryable DocIndex with path/name/suffix maps
- **Cache Layer**: Serialize DocIndex to disk, invalidate by mtime or version
- **Query Engine**: Path resolution (case-insensitive, suffix matching, ambiguity handling)
- **Output**: Plain text renderer with smart defaults per item kind, truncation

## Pre-commit checks

**Before every commit, run the pre-commit checks:**

1. `cargo fmt -- --check` — formatting must be clean
2. `cargo clippy --all-targets` — zero warnings with pedantic (configured in Cargo.toml [lints])
3. `cargo test` — all tests must pass

Do not commit if any of these fail. Fix issues first.

## Important for automated agents

- **Working directory:** Always run `cargo test`, `cargo clippy`, and `cargo fmt` from the project root (`/Users/john/projects/groxide/groxide/`). NEVER cd into `test-fixtures/groxide_test_api/` to run tests — that's a fixture crate with no tests.
- **Snapshot updates:** When code changes cause snapshot test diffs, update them non-interactively with `INSTA_UPDATE=always cargo test`. Do NOT use `cargo insta accept` (requires a terminal).
- **Cache invalidation:** After changing `index_builder.rs`, delete `target/groxide/` to clear the index cache before testing.
- **Spec is truth:** Before implementing any task, read the relevant spec sections listed in `docs/IMPLEMENTATION.md`. The spec files are the single source of truth.

## Rust coding rules

- Use `cargo check` instead of `cargo build` during development — it's 2-10x faster.
- Use `cargo add <crate>` to add dependencies — never hand-edit version strings in Cargo.toml.
- No `.unwrap()` in library code. Use `?`, `.expect("invariant: reason")` for true invariants, or combinators (`.unwrap_or()`, `.map_err()`).
- No `.clone()` to silence the borrow checker. Fix the ownership design.
- Struct fields private by default. Expose via getter methods. Promote to `pub(crate)` then `pub` only when needed.
- Match all enum variants explicitly. No wildcard `_` on enums we control — the compiler should catch new variants.
- Error handling: `thiserror` for our error types, `?` for propagation, `.map_err()` for context.
- Prefer `&str` over `String` in function arguments. Use `Cow<'_, str>` when ownership is conditional.
- `///` doc comments on all public items, starting with a third-person verb ("Returns the...", "Creates a...").
- Naming: `as_`/`to_`/`into_` for conversions (free/allocating/consuming). No `get_` prefix on getters. `is_`/`has_` for boolean methods.
- Test names: `{action}_{outcome}_{condition}` (e.g., `lookup_returns_found_when_exact_path_matches`).
- `#[cfg(test)] mod tests` for unit tests, `tests/` directory for integration tests.

## Conventions

- All status/progress messages go to stderr, content to stdout
- Exit codes: 0 = success, 1 = not found, 2 = error
- Tests use a fixture crate at `test-fixtures/groxide_test_api/`
- Snapshot tests with `insta` for output format verification
- TDD: Write tests first, then implement
- One commit per task. Message format: `task N: <summary>`

## rloop task management

- To create tasks: `rloop task add --help`. Always use `opus` for the model.
- Tasks live in `.rloop/tasks/`
