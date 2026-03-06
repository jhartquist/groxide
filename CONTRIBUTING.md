# Contributing to groxide

## Prerequisites

- **Rust stable** (MSRV 1.85): `rustup toolchain install stable`
- **Rust nightly**: Required for rustdoc JSON generation. Install with `rustup toolchain install nightly`

## Building

```sh
# Check (faster than build during development)
cargo check

# Build
cargo build

# Install locally
cargo install --path .
```

The binary is called `grox`.

## Testing

```sh
# Run all tests
cargo test

# Run a specific test
cargo test test_name

# Run ignored tests (e.g., network-dependent or slow tests)
cargo test -- --ignored

# Run integration tests only
cargo test --test cli_happy
cargo test --test cli_errors
```

### Snapshot tests

This project uses [insta](https://insta.rs/) for snapshot testing. When code changes cause snapshot diffs:

```sh
# Update snapshots non-interactively
INSTA_UPDATE=always cargo test
```

Do **not** use `cargo insta accept` (it requires an interactive terminal).

### Test fixture

Tests use a fixture crate at `test-fixtures/groxide_test_api/` with a pre-generated rustdoc JSON file. Do not `cd` into the fixture directory to run tests — always run from the project root.

### Cache invalidation

After changing `src/index_builder.rs`, clear the index cache before testing:

```sh
rm -rf target/groxide/
```

## Pre-commit checks

Before every commit, all three checks must pass:

```sh
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```

## Code style

- **No `.unwrap()` in library code.** Use `?`, `.expect("invariant: reason")`, or combinators.
- **No `.clone()` to silence the borrow checker.** Fix the ownership design.
- **Match all enum variants explicitly.** No wildcard `_` on enums we control.
- **Error handling:** `thiserror` for error types, `?` for propagation, `.map_err()` for context.
- **Prefer `&str` over `String`** in function arguments.
- **Doc comments** on all public items, starting with a third-person verb ("Returns the...", "Creates a...").
- **Test names:** `{action}_{outcome}_{condition}` (e.g., `lookup_returns_found_when_exact_path_matches`).
- **Pedantic clippy:** Lint levels are configured in `Cargo.toml` under `[lints]`.

## Project structure

```
src/
  main.rs           Entry point (~15 lines)
  lib.rs            Orchestration (run function)
  cli.rs            CLI definition (clap derive)
  error.rs          Error types and exit codes
  types.rs          Core domain types
  resolve.rs        Crate resolution
  docgen.rs         Rustdoc JSON generation
  index_builder.rs  Index builder (4-pass algorithm)
  cache.rs          Disk cache
  query.rs          Query engine
  search.rs         Full-text search
  signature.rs      Signature rendering
  external.rs       External crate fetching
  stdlib.rs         Standard library support
  render/
    mod.rs          Render dispatch
    text.rs         Plain text output (single-item default view)
    brief.rs        Brief mode output (names only)
    docs.rs         Full docs mode output
    list.rs         List mode output (recursive)
    json.rs         JSON output
    ambiguous.rs    Ambiguous match display
tests/              Integration tests
test-fixtures/      Test fixture crate
docs/
  spec/             Specification files (source of truth)
  IMPLEMENTATION.md Task breakdown
```

## Adding dependencies

Use `cargo add`:

```sh
cargo add some-crate
cargo add some-crate --dev  # for dev-dependencies
```

Never hand-edit version strings in `Cargo.toml`.

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project: MIT OR Apache-2.0.
