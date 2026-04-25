# Index Contract

Status: draft.

## Purpose

The index is the stable internal documentation model. It hides rustdoc JSON
quirks from resolution, search, rendering, and source views.

Rustdoc JSON types SHOULD NOT leak outside acquisition and normalization
modules.

## Item Kinds

The model MUST represent at least:

- Module
- Struct
- Enum
- Union
- Trait
- Trait alias
- Function
- Type alias
- Associated type
- Constant
- Associated constant
- Static
- Macro
- Proc macro
- Variant
- Field
- Primitive
- Foreign type

Display filters may group these into the CLI kind values from
`01-cli-contract.md`.

## Documentation Item

Each item MUST store:

- Stable item identity.
- Full Rust path.
- Simple name.
- Kind.
- Signature.
- Raw documentation text.
- Summary.
- Visibility.
- Source span when available.
- Child item references.
- Feature gate when available.
- Re-export information when applicable.

Fields are private by default in implementation. Public access should go through
methods that preserve invariants.

## Stable Identity

Stable identity MUST be able to distinguish duplicate display paths, derive
macros, re-export stubs, canonical targets, and source crate ownership.

Identity MUST be independent from vector position when serialized cache format
would otherwise make reordering fragile.

## Lookup Maps

An index SHOULD maintain:

- Full path map.
- Case-insensitive full path map.
- Simple name map.
- Suffix map.
- Child relationship map.
- Trait implementation map.

Maps are implementation details. Tests should prefer public lookup behavior
unless the map itself is the behavior being specified.

## Normalization Pipeline

Rustdoc JSON normalization SHOULD be split into named passes:

1. Input validation and crate metadata.
2. Parent and child relationship discovery.
3. Path computation.
4. Re-export and glob import normalization.
5. Signature rendering.
6. Item conversion.
7. Trait implementation linking.
8. Search field preparation.

Each pass should have focused tests and deterministic ordering.

## Re-export Rules

The index MUST represent `pub use` items as first-class re-export data, not as
string patterns in signatures.

Glob re-exports MUST be normalized deterministically.

Duplicate reachable paths to the same canonical item MUST be preserved as
addressable paths but deduplicated during ambiguity rendering where appropriate.

## Source Spans

Source spans MUST use one-based inclusive line numbers.

Unavailable spans MUST be represented explicitly, not as ambiguous zero values
where possible.

Source paths SHOULD be relative to the owning crate root when possible.

## Visibility

By default, the index may include hidden rustdoc items needed for correct
relationships, but public rendering MUST exclude non-public items unless
`--private` is active.

If `--private` changes the generated rustdoc JSON or index contents, that choice
MUST be reflected in cache keys.

## Serialization

Serialized indexes MUST include:

- Format version.
- Tool version.
- Crate name.
- Crate version when known.
- Source kind.
- Feature flags.
- Private/public mode.
- Rust toolchain identity when relevant.

Cache format changes MUST bump the format version.
