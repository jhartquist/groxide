---
name: groxide
description: >
  Query Rust crate documentation from the terminal using groxide (grox).
  Use when you need to look up Rust API docs, check type signatures, explore
  module contents, find methods on a type, or search crate documentation —
  without leaving the terminal or browsing docs.rs. Ideal for LLM coding agents
  working on Rust projects.
license: MIT OR Apache-2.0
allowed-tools: Bash(grox:*)
metadata:
  author: jhartquist
  version: "0.1"
---

# groxide — Query Rust Docs from the Terminal

The path is the query. No subcommands — just `grox <path>`.

Output goes to stdout (documentation content) and stderr (status messages).
Agents should capture stdout only. Exit codes: 0 = success, 1 = not found, 2 = error.

## Install

```sh
cargo install groxide
rustup toolchain install nightly  # required for rustdoc JSON generation
```

## Common Patterns

### Look up a type

```sh
grox serde::Deserialize
```

Shows the type signature, doc comments, methods, and trait implementations.
Truncated by default (~200-800 tokens). Use `--all` to expand everything.

### Look up a method

```sh
grox tokio::sync::Mutex::lock
```

Shows full method signature and documentation.

### List module contents

```sh
grox tokio::sync -l
```

One-line summaries of all public items in the module. Useful for discovering
what a module offers before drilling into specific items.

### Search documentation

```sh
grox -S "async read" tokio
```

Full-text search across all items in a crate. Returns up to 20 results with
kind, path, and summary. Combine with `--kind fn` to narrow results.

### View source code

```sh
grox -s tokio::sync::Mutex::new
```

Shows the source code with file path and line numbers.

### Get JSON output

```sh
grox --json serde::Serialize
```

JSON Lines format with structured fields: path, kind, signature, doc, methods,
trait_impls. Useful when you need to parse the output programmatically.

### Query standard library

```sh
grox std::collections::HashMap
grox std::fs::File::open
```

Works with `std`, `core`, and `alloc`.

### Show trait implementations

```sh
grox --impls std::sync::Arc
```

Lists all trait implementations on a type, or all known implementors of a trait.

### Show crate README

```sh
grox --readme tokio
```

### Auto-fetch external crates

```sh
grox axum::Router
```

Crates not in your project's dependencies are automatically fetched from
crates.io, documented, and indexed. No manual setup needed.

### Pin to a specific version

```sh
grox serde@1.0.210::Deserialize
```

The `@version` syntax fetches a specific version from crates.io, bypassing
dependency resolution.

## Flags Reference

| Flag | Short | Purpose |
|------|-------|---------|
| `--all` | `-a` | Disable truncation, expand everything |
| `--list` | `-l` | List children (names + one-line summaries) |
| `--source` | `-s` | Show source code |
| `--search <Q>` | `-S` | Full-text search |
| `--json` | `-j` | JSON Lines output |
| `--kind <K>` | `-k` | Filter by kind: `fn`, `struct`, `enum`, `trait`, `type`, `const`, `mod`, `macro` |
| `--impls` | `-i` | Show trait implementations or implementors |
| `--private` | `-p` | Include non-public items |
| `--readme` | | Show crate README |
| `--features` | | Activate specific features |
| `--all-features` | | Activate all features |

## Tips for Effective Use

- **Start broad, then drill down.** Query the crate first (`grox tokio`), then
  a module (`grox tokio::sync`), then a type (`grox tokio::sync::Mutex`).
- **Use `--list` to orient.** When you don't know what's in a module, `-l` gives
  a quick overview without the noise of full documentation.
- **Use `--json` for structured data.** When you need to extract specific fields
  (signatures, method lists), JSON is more reliable to parse than plain text.
- **Search before guessing paths.** If you're not sure of the exact path,
  `grox -S "keyword" crate_name` finds items by documentation content.
- **Check exit codes.** Exit 1 means the item doesn't exist — don't retry, try
  a different path or use search. Exit 2 means a tooling error.
- **Don't use `--all` by default.** The truncated output is designed to fit
  agent context windows. Only expand when you need the full picture.
- **Run from a Rust project directory** when possible. This gives groxide access
  to the full dependency graph and enables queries like `grox` (current crate)
  with no arguments.
