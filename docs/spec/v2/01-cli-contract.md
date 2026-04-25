# CLI Contract

Status: draft.

## Usage

```text
<cmd> [OPTIONS] [PATH]
```

The CLI MUST parse into a typed internal request before any cargo, filesystem,
cache, or network work begins.

## Path Grammar

```text
PATH          = [CRATE_SPEC ["::" ITEM_PATH]]
CRATE_SPEC    = CRATE_NAME | CRATE_NAME "@" VERSION
ITEM_PATH     = SEGMENT ["::" SEGMENT]*
CRATE_NAME    = Rust crate package name or dependency rename
VERSION       = complete semver, partial semver, or prerelease semver
SEGMENT       = Rust path segment
```

Rules:

- Empty path means current crate root when a project context exists.
- Empty path outside a project is an error.
- `serde` means crate name if resolvable, otherwise may be considered as a
  current-crate item candidate.
- `serde::Deserialize` means crate plus item path if `serde` resolves as a
  crate.
- `serde@1.0.210::Deserialize` always means crates.io version pin.
- `serde@` is invalid.
- `@serde` is invalid and SHOULD suggest removing the leading `@`.
- Hyphens and underscores are equivalent for crate name matching.
- Rust item path matching remains case-aware as specified in
  `02-resolution-contract.md`.

## Flags

| Flag | Meaning |
| ---- | ------- |
| `-b`, `--brief` | Names-only compact output. |
| `-d`, `--docs` | Full rendered documentation. |
| `-s`, `--source` | Source code with file path and line numbers. |
| `-r`, `--recursive` | Include descendants of containers. |
| `-S`, `--search <QUERY>` | Full-text search. Space means AND, `|` means OR. |
| `-i`, `--impls [TRAIT]` | Show trait implementations; optionally filter by trait. |
| `-j`, `--json` | JSON output. |
| `-p`, `--private` | Include non-public items. |
| `-k`, `--kind <KIND>` | Filter by item kind. |
| `--readme` | Show the crate README. |
| `--manifest-path <PATH>` | Use an explicit Cargo.toml. |
| `--features <LIST>` | Activate comma-separated features. |
| `--all-features` | Activate all features. |
| `--no-default-features` | Disable default features. |
| `--clear-cache` | Wipe global cache and exit. |

## Kind Values

`--kind` MUST accept:

- `fn`
- `struct`
- `enum`
- `trait`
- `type`
- `const`
- `mod`
- `macro`

Kind matching MAY group internal item kinds. For example, `type` may include
type aliases, associated types, and foreign types.

## Flag Composition

Allowed:

- `--recursive` with `--brief`, default, `--docs`, `--source`, `--json`,
  `--private`, and `--kind`.
- `--docs` with `--source`.
- `--json` with query, recursive, search, impls, source, README, and ambiguity
  output when meaningful.
- Feature flags with all modes that build rustdoc.

Conflicts:

- `--brief` conflicts with `--docs` and `--source`.
- `--search` conflicts with `--source`, `--docs`, `--brief`, and `--impls`.
- `--impls` conflicts with `--source`, `--docs`, `--brief`, and `--search`.
- `--readme` conflicts with `--source`, `--docs`, `--brief`, `--search`, and
  `--impls`.
- `--clear-cache` conflicts with all query/output modes.

Invalid flag combinations MUST fail during CLI validation with exit code `2`.

## Feature Flag Validation

- `--features` and `--all-features` MAY compose.
- `--features` and `--no-default-features` MAY compose.
- `--all-features` and `--no-default-features` MUST conflict unless a later ADR
  accepts cargo-compatible forwarding for that combination.

## Help Text

Help text MUST include:

- One-line purpose.
- Usage.
- All flags.
- At least one example for crate, type, method, search, source, stdlib,
  external crate, JSON, and version pin.

Examples MUST use the final command name after the naming ADR is accepted.
