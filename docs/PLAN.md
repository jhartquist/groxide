# Rewrite Plan

Status: draft.

This plan defines how to build the rewrite so the history shows the product
being derived intentionally.

## Development Pattern

The rewrite uses a top-down, type-first, red-green-refactor pattern.

In order:

1. Define the product contract.
2. Define domain vocabulary as Rust types.
3. Define module interfaces with compiling placeholders.
4. Add focused failing tests for one behavior.
5. Implement the smallest production change that passes those tests.
6. Refactor for depth and locality without changing behavior.
7. Repeat down the next subtree.

## Commit Discipline

Commit types:

- `design:` product docs, specs, ADRs, naming, architecture decisions.
- `types:` domain types and invariants.
- `interface:` module interfaces and typed placeholders.
- `test(red):` failing tests that introduce one behavioral obligation.
- `feat(green):` implementation that satisfies the immediately prior red test.
- `refactor:` structure-only change with tests passing before and after.
- `docs:` user-facing docs that do not change product decisions.
- `chore:` tooling, CI, formatting, metadata.

Red commits are allowed when they are intentional:

- The test must compile.
- The failure must be specific and expected.
- The commit message must say `test(red):`.
- The next commit should normally be the corresponding green commit.
- Red commits should not accumulate.

Green and refactor commits must pass:

```text
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```

During early skeleton work, `cargo check` may be the gate until tests exist.

## Phase Tree

### Phase 0: Product Contract

Goal: make the destination explicit before code.

Creates:

- Vision.
- V2 spec.
- ADRs.
- Rewrite plan.

Gate:

- No production code required.
- Naming can remain unresolved if captured by ADR.

### Phase 1: Project Skeleton

Goal: create the smallest compiling Rust project.

Creates:

- `Cargo.toml`
- `src/main.rs`
- `src/lib.rs`
- CI.
- License files.
- README skeleton.

Gate:

- `cargo check` passes.
- No CLI behavior promised beyond help/version if clap is already present.

### Phase 2: Domain Types

Goal: encode the vocabulary before behavior.

Creates types for:

- Parsed request.
- Path query.
- Crate spec.
- Crate source.
- Detail tier.
- Output mode.
- Feature flags.
- Item kind.
- Source span.
- Documentation item.
- Re-export identity.
- Search result.
- Error model.

Pattern:

- Add type with constructors/invariants.
- Add focused unit tests for conversion and validation.
- Avoid cargo/rustdoc/network adapters.

### Phase 3: Module Interfaces

Goal: define the main seams without filling them in.

Modules:

- CLI request parser.
- Command lifecycle.
- Crate resolver.
- Acquisition adapter.
- Rustdoc normalizer.
- Index/query engine.
- Search engine.
- Renderer.
- Source materializer.
- Cache.

Rules:

- Interfaces return domain types, not raw rustdoc/cargo types.
- Placeholder implementations may use typed errors or `todo!()` only when tests
  do not exercise them yet.
- Avoid committing reachable `todo!()` in a green commit.

### Phase 4: CLI Request Parsing

Goal: convert argv into a typed request.

Red tests:

- Path grammar.
- Flag conflicts.
- Detail tier composition.
- Feature flag validation.
- Kind parsing.

Green implementation:

- Clap parse.
- Internal request conversion.
- Validation errors.

### Phase 5: Output With Fake Data

Goal: lock the output contract before rustdoc enters the system.

Red tests:

- Crate root default.
- Module default.
- Type default.
- Brief recursive.
- Docs mode.
- Source mode.
- Search output.
- Ambiguity output.
- JSON output.

Green implementation:

- Pure renderers over hand-built in-memory documentation items.

### Phase 6: Resolution With Fake Indexes

Goal: implement path and item behavior without cargo.

Red tests:

- Exact path.
- Case-insensitive lookup.
- Suffix lookup.
- Name lookup.
- Method lookup.
- Kind preference and relaxation.
- Ambiguity dedup.
- Re-export identity.
- Single-segment crate-vs-item fallback.

Green implementation:

- Pure resolver over fake indexes and fake crate sources.

### Phase 7: Index Model

Goal: build a stable queryable model independent of rustdoc JSON.

Red tests:

- Item insertion.
- Lookup map behavior.
- Stable identity.
- Child relationships.
- Trait impl storage.
- Re-export addressability.

Green implementation:

- In-memory index and indexing invariants.

### Phase 8: Rustdoc Normalization

Goal: convert rustdoc JSON into the stable model.

Red tests:

- Fixture crate conversion.
- Path reconstruction.
- Method paths.
- Feature gates.
- Visibility.
- Glob re-exports.
- Trait impls.
- Source spans.

Green implementation:

- Named normalization passes.

### Phase 9: Acquisition and Cache

Goal: connect real cargo, stdlib, crates.io, and cache behavior.

Red tests:

- Command construction.
- Cache key construction.
- Cache read/write round trip.
- Cache invalidation.
- Safe archive extraction.
- Missing nightly.
- Missing rust-src.
- crates.io not found.

Green implementation:

- Edge adapters around the pure core.

### Phase 10: Vertical CLI Slices

Goal: wire the full command lifecycle one workflow at a time.

Order:

1. Current crate root.
2. Current crate item.
3. Dependency item.
4. Stdlib item.
5. External crate item.
6. Recursive.
7. Brief.
8. Docs.
9. Source.
10. Search.
11. Impls.
12. README.
13. JSON.
14. Clear cache.

Each slice should follow red-green-refactor.

### Phase 11: Publication Hardening

Goal: make the repository ready for public release.

Tasks:

- Resolve public name and binary name.
- Update README.
- Add examples.
- Add contribution guide.
- Add changelog.
- Run full CI locally.
- Review dependency licenses.
- Review ignored slow/network tests.
- Tag first public release.

## Branch Policy

The rewrite branch may contain intentional red commits. The published default
branch should end at a green commit.

Before opening the repository publicly, review whether to preserve the complete
red-green history or squash early exploratory commits that do not meet this
plan.

## Open Decisions

- Final project name.
- Final binary name.
- Default documentation truncation cap.
- Whether current crate indexes are serialized.
- Exact external feature fallback strategy.
- JSON schema versioning format.
- Whether first release preserves all red commits or only green milestones.
