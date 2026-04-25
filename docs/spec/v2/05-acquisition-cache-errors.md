# Acquisition, Cache, and Errors

Status: draft.

## Acquisition Principle

Acquisition is the only layer that talks to cargo, rustup, crates.io, the
filesystem cache, and rustdoc JSON files.

Given a crate source and build options, acquisition returns either a normalized
index or a precise error.

## Rustdoc Generation

All generated documentation ultimately comes from rustdoc JSON:

```text
cargo +nightly rustdoc --lib --output-format json -Z unstable-options
```

The implementation MUST detect missing nightly before attempting generation and
return a tooling error with exit code `2`.

The implementation SHOULD use one command builder for current crates,
dependencies, external crates, and stdlib unless a documented exception exists.

## Source Kinds

The acquisition layer MUST support:

- Current crate.
- Direct dependency.
- Workspace member.
- Transitive dependency.
- Standard library crate.
- External crates.io crate.

## Feature Policy

User-provided feature flags are authoritative and forwarded exactly.

Default feature strategy:

- Current crate: prefer useful docs, with platform-failure fallback.
- Dependency: use cargo's resolved feature set.
- External crate: use docs.rs metadata when available; otherwise use a
  deterministic fallback.
- Stdlib: do not invent feature flags.

Exact fallback order is implementation-specific until committed by ADR, but it
MUST be tested and documented before publication.

## External Crates

crates.io access MUST:

- Use a clear User-Agent.
- Have connect and read timeouts.
- Respect yanked versions when resolving partial versions.
- Avoid unsafe archive extraction.
- Skip or reject symlinks and hardlinks.
- Prevent path traversal.
- Use temporary directories and atomic promotion.
- Clean up failed downloads.

Network failures are infrastructure errors with exit code `2`.

Crate-not-found is a not-found error with exit code `1`.

## Standard Library

Stdlib acquisition MUST use the active nightly sysroot and `rust-src`.

Missing `rust-src` MUST produce an actionable error.

Stdlib cache keys MUST include the relevant toolchain identity.

## Cache

The cache is an optimization, not the source of truth.

Cache keys MUST include:

- Source kind.
- Crate name.
- Crate version when known.
- Feature flags.
- Private/public mode.
- Tool version or cache format version.
- Rust toolchain identity when relevant.

Cache writes SHOULD be atomic.

Cache corruption SHOULD invalidate that entry and rebuild when possible.

`--clear-cache` MUST remove the global cache and exit successfully if the cache
is absent.

## Current Crate Cache

TBD: v2 must decide whether current crate indexes are serialized. If they are,
invalidation must be exact enough to avoid stale local docs. If they are not,
the decision should be documented as a deliberate simplicity tradeoff.

## Progress Output

All progress goes to stderr.

Progress messages SHOULD use a stable prefix, for example:

```text
[groxide] building index for serde 1.0.210...
[groxide] using cached index for serde 1.0.210
```

The final prefix may change with the public name.

## Error Model

Errors MUST preserve:

- User-facing message.
- Exit code.
- Underlying cause when useful.
- Suggestions when available.

Core error classes:

- Invalid argument.
- Manifest not found.
- Crate not found.
- Item not found.
- Source unavailable.
- README not found.
- Search query invalid.
- Nightly not available.
- Rust source component missing.
- Rustdoc failed.
- External fetch failed.
- Cache read/write failed.
- Cache format unsupported.

Infrastructure errors exit `2`. Not-found errors exit `1`.
