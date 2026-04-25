# Resolution Contract

Status: draft.

## Resolution Principle

Resolution MUST avoid committing too early. A single user path may produce more
than one candidate interpretation. The resolver should keep plausible crate and
item interpretations alive until lookup proves which one is correct.

This fixes the class of bugs where short unknown crate names such as `syn`,
`url`, `cc`, or `h2` are swallowed as item-like queries inside a project.

## Project Context

Project discovery:

1. If `--manifest-path` is present, use that manifest and propagate discovery
   errors.
2. Otherwise, walk upward from the current directory looking for `Cargo.toml`.
3. If discovery fails without `--manifest-path`, continue in no-project mode.

Current package selection:

1. Root package for non-virtual workspaces.
2. Closest workspace member to current directory for virtual workspaces.
3. First workspace member as a deterministic fallback.

## Crate Resolution Order

With project context, a named crate candidate resolves in this order:

1. Current crate.
2. Direct dependencies, including renamed dependencies.
3. Workspace members.
4. Transitive dependencies.
5. Standard library crates: `std`, `core`, `alloc`.
6. crates.io external crate.

Without project context:

1. `std`, `core`, and `alloc`.
2. crates.io external crate.
3. Empty path fails with manifest-not-found.

Version-pinned crates skip project dependency resolution and go directly to
crates.io.

## Candidate Interpretation

For a single segment `PATH`, the resolver SHOULD consider:

- crate candidate: `PATH` as a crate name.
- item candidate: `PATH` as an item in the current crate, when project context
  exists and the segment is item-like.

The item-like heuristic affects ordering, not final truth. If item lookup fails,
crate resolution MUST still be allowed to continue to crates.io.

Heuristic defaults:

- Empty string: not item-like.
- Contains `-`: crate-like.
- Contains uppercase: item-like.
- Simple lowercase underscore names such as `serde_json`: crate-like.
- Complex snake_case names: item-like.
- Common method names such as `new`, `len`, `clone`, `default`: item-like.
- Short lowercase names are ambiguous, not final.

## Item Lookup

Lookup over a loaded index is pure.

Pipeline:

1. Exact full path match.
2. Case-insensitive full path match when the query is all lowercase.
3. Suffix path match.
4. Name match for single-segment item queries.
5. Not found with suggestions.

Case behavior:

- All-lowercase queries are case-insensitive.
- Any uppercase character makes the query case-sensitive.

`--kind` acts as a filter preference. If applying the kind filter produces no
item matches, the resolver MAY retry without the filter and report the broader
result.

## Method Lookup

If normal lookup fails for `Type::method`:

1. Resolve `Type`.
2. If `Type` is ambiguous, return parent ambiguity.
3. Search child functions and associated items for `method`.
4. If no method matches, return method-level suggestions.

## Ambiguity

Ambiguity is successful command execution unless a mode explicitly cannot render
ambiguous results.

Ambiguous results MUST be:

- Deduplicated by stable item identity.
- Deduplicated by duplicate `(path, kind)` pairs.
- Sorted deterministically.
- Rendered with enough path and kind information for the user to disambiguate.

At crate root, if one primary type-like item clearly dominates nested matches,
the resolver MAY auto-select it. The rule MUST be covered by tests.

## Re-exports

Re-export identity MUST be explicit in the model.

The resolver and renderer must be able to distinguish:

- The path the user queried.
- The re-export stub, if any.
- The canonical target item.
- The crate source that owns the canonical target.
- The display path chosen for output.

Cross-crate re-export following MUST preserve both the source index and the
source crate identity so source rendering cannot use the wrong filesystem root.

## Search

Search is pure over an index.

Query syntax:

- Space means AND.
- `|` means OR.
- Empty groups are ignored.
- Empty or whitespace-only search queries are invalid.

Scoring SHOULD prefer:

1. Exact name.
2. Name substring.
3. Signature match.
4. Module path match.
5. Documentation text match.

Search results MUST include the number shown and the total before truncation so
output can say, for example, `20 of 45 results`.

Zero-result search is success with exit code `0`.
