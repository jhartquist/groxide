# Product Contract

Status: draft.

## Core Shape

Groxide is a Rust documentation query CLI.

```text
<cmd> [OPTIONS] [PATH]
```

There are no user-facing subcommands. The path is the query.

## Core Workflows

The tool MUST support:

- Current crate root: `<cmd>`
- Named crate root: `<cmd> serde`
- Module: `<cmd> tokio::sync`
- Type: `<cmd> std::collections::HashMap`
- Function: `<cmd> serde_json::from_str`
- Method: `<cmd> tokio::sync::Mutex::lock`
- Version-pinned crate: `<cmd> serde@1.0.210::Deserialize`
- Recursive tree: `<cmd> -r tokio::sync`
- Brief skeleton: `<cmd> -r -b tokio`
- Full docs: `<cmd> -d tokio::sync::Mutex`
- Source view: `<cmd> -s tokio::sync::Mutex::new`
- Search: `<cmd> serde -S "deserialize visitor"`
- Trait impls: `<cmd> --impls Clone serde_json::Value`
- README: `<cmd> --readme tokio`
- JSON output: `<cmd> -j serde::Deserialize`
- Cache clear: `<cmd> --clear-cache`

## Detail Tiers

The renderer has four detail tiers:

| Tier | Flags | Meaning |
| ---- | ----- | ------- |
| Brief | `-b`, `--brief` | Item names only. |
| Default | none | Kind, path, signatures, summaries, and bounded docs. |
| Docs | `-d`, `--docs` | Full rendered documentation. |
| Source | `-s`, `--source` | Source code with file path and line numbers. |

`--brief` is exclusive with `--docs` and `--source`.

`--docs` and `--source` compose: `--docs --source` shows source with rendered
docs included.

`--recursive` composes with every tier.

## Orthogonal Modifiers

These modifiers SHOULD compose with all compatible modes:

- `--json`
- `--private`
- `--kind <KIND>`
- `--manifest-path <PATH>`
- `--features <LIST>`
- `--all-features`
- `--no-default-features`

## Output Invariants

- Documentation content MUST go to stdout.
- JSON content MUST go to stdout.
- Source content MUST go to stdout.
- README content MUST go to stdout.
- Progress, warnings, hints, and errors MUST go to stderr.
- Human output MUST be plain text, not Markdown.
- Default output MUST be bounded.
- Verbose output MUST require an explicit flag.

## Exit Codes

- `0`: command completed successfully, including zero-result search and
  ambiguity output.
- `1`: requested crate, item, source, or README was not found.
- `2`: invalid arguments, missing required tooling, rustdoc failure, network
  failure, cache corruption, or other infrastructure error.

## Compatibility Policy

Before the first public release, the v2 spec may change freely through ADRs.
After the first public release, behavior that affects CLI syntax, output shape,
exit codes, or cache compatibility requires a changelog entry and migration
note.
