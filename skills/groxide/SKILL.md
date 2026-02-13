---
name: groxide
description: >
  Query Rust crate documentation from the terminal using groxide (grox).
  Use when you need to look up Rust API docs, check type signatures, explore
  module contents, find methods on a type, or search crate documentation —
  without leaving the terminal or browsing docs.rs. Ideal for LLM coding agents
  working on Rust projects. Do NOT use for general Rust language questions,
  reading local source files, or managing Cargo dependencies.
license: MIT OR Apache-2.0
compatibility: Requires Rust nightly toolchain and cargo
allowed-tools: Bash(grox:*)
metadata:
  author: jhartquist
  version: "0.1.0"
---

# groxide — Query Rust Docs from the Terminal

The path is the query. No subcommands — just `grox <path>`.

Output goes to stdout (documentation content) and stderr (status messages).
Agents should capture stdout only. Exit codes: 0 = success, 1 = not found, 2 = error.

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

## Example Workflows

### Understanding an unfamiliar dependency

User asks: "How does the `tower` middleware system work?"

1. Start with the crate overview: `grox tower`
2. List the main exports: `grox tower -l`
3. Drill into the key trait: `grox tower::Service`
4. Read the core method: `grox tower::Service::call`
5. Search for patterns: `grox -S "middleware" tower`

### Finding the right function

User asks: "How do I read a file asynchronously with tokio?"

1. Search the crate: `grox -S "read file" tokio`
2. Check the result: `grox tokio::fs::File::open`
3. Browse related items: `grox tokio::fs -l`

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

## Troubleshooting

### "error: toolchain 'nightly' is not installed"

groxide requires Rust nightly for rustdoc JSON generation.
Fix: `rustup toolchain install nightly`

### First query for a crate is slow

groxide runs `cargo +nightly rustdoc` and builds an index on first use. This
can take 30-120 seconds for large crates. Subsequent queries use the cached
index and are fast. Do not interrupt the process.

### Exit code 1: item not found

The path does not match any item in the crate's public API.
- Check spelling (paths are case-insensitive but must match item names)
- Use `grox -S "keyword" crate_name` to search by documentation content
- Use `grox crate_name -l` to list what's available
- The item may be behind a feature flag — try `--all-features`

### Exit code 2: error

A tooling error occurred. Check stderr for the specific message. Common causes:
- Not in a Rust project directory (needed for dependency resolution)
- Crate name typo
- Network issues when fetching external crates

## Install

```sh
cargo install groxide
rustup toolchain install nightly  # required for rustdoc JSON generation
```
