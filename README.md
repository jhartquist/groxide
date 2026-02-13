# groxide

[![CI](https://github.com/jhartquist/groxide/actions/workflows/ci.yml/badge.svg)](https://github.com/jhartquist/groxide/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jhartquist/groxide/graph/badge.svg)](https://codecov.io/gh/jhartquist/groxide)
[![Crates.io](https://img.shields.io/crates/v/groxide.svg)](https://crates.io/crates/groxide)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/groxide.svg)](LICENSE-MIT)

Query Rust crate documentation from the terminal. Inspired by `go doc` — the path is the query, smart defaults by item kind.

## Why

LLM coding agents and humans both need fast, token-efficient access to crate docs without leaving the terminal. `groxide` resolves paths like `tokio::sync::Mutex`, auto-builds a queryable index from rustdoc JSON, and renders plain text output tuned for ~200-800 tokens per query.

## Features

- **Path is the query.** `grox serde::Deserialize` — no subcommands.
- **Smart defaults.** Output adapts by item kind (struct, module, function, etc.).
- **Progressive disclosure.** Crate -> module -> type -> method drill-down.
- **Token-efficient.** Truncation by default, `--all` to expand.
- **Zero setup.** Auto-builds and caches index on first use.
- **Auto-fetch.** Unknown crates are fetched from crates.io automatically.
- **Standard library.** Query `std`, `core`, and `alloc` directly.
- **Full-text search.** `grox -S "async file"` searches across docs.
- **Multiple output formats.** Plain text (default), JSON (`--json`), list (`--list`).

## Requirements

- Rust stable (MSRV 1.85)
- Rust nightly toolchain (for rustdoc JSON generation): `rustup toolchain install nightly`

## Installation

### From crates.io

```sh
cargo install groxide
```

### From source

```sh
git clone https://github.com/jhartquist/groxide.git
cd groxide
cargo install --path .
```

The binary is called `grox`.

### Agent skill

groxide ships with an [agent skill](https://agentskills.io) that teaches AI coding agents how to use it. Install the skill so your agent can query Rust docs autonomously:

**Claude Code:**

```sh
cp -r skills/groxide ~/.claude/skills/
```

**Or install from GitHub (works with Claude Code, Codex, Cursor, Copilot, and [20+ other tools](https://agentskills.io)):**

```sh
npx skills add jhartquist/groxide
```

## Quick start

```sh
# Query a struct — shows signature, docs, methods, trait impls
grox serde::Deserialize

# List module contents
grox tokio::sync -l

# Full method documentation
grox tokio::sync::Mutex::lock

# Search across documentation
grox -S "async file"

# View source code
grox -s tokio::sync::Mutex::new

# Auto-fetch an external crate from crates.io
grox axum::Router

# Query standard library
grox std::collections::HashMap

# JSON output for programmatic use
grox --json serde::Serialize

# Pin to a specific version
grox serde@1.0.210::Deserialize
```

## Usage

```
grox [OPTIONS] [PATH]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `[PATH]` | Rust path to query (e.g., `tokio::sync::Mutex`, `serde@1.0`) |

### Options

| Flag | Short | Description |
|------|-------|-------------|
| `--source` | `-s` | Show source code instead of docs |
| `--list` | `-l` | List children only (names + one-line summaries) |
| `--all` | `-a` | Expand everything (full docs, no truncation) |
| `--search <QUERY>` | `-S` | Full-text search across documentation |
| `--kind <KIND>` | `-k` | Filter by item kind: `fn`, `struct`, `enum`, `trait`, `type`, `const`, `mod`, `macro` |
| `--private` | `-p` | Include non-public items |
| `--json` | `-j` | JSON Lines output |
| `--impls` | `-i` | Show trait implementations (on types) or implementors (on traits) |
| `--readme` | | Show the crate's README |
| `--manifest-path <PATH>` | | Path to Cargo.toml |
| `--features <FEATURES>` | | Comma-separated list of features to activate |
| `--all-features` | | Activate all available features |
| `--no-default-features` | | Do not activate the `default` feature |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success (item found, search completed, ambiguous match rendered) |
| 1 | Not found (crate or item not found after all resolution) |
| 2 | Error (nightly missing, build failed, invalid arguments, etc.) |

## How it works

1. **Resolve** the crate: current project, dependency, workspace member, transitive dep, stdlib, or auto-fetch from crates.io.
2. **Generate** rustdoc JSON via `cargo +nightly rustdoc --output-format json`.
3. **Build** a queryable index from the JSON (4-pass algorithm: parent map, path computation, item conversion, relationships).
4. **Cache** the index to disk (`target/groxide/` for local, `~/.cache/groxide/` for external). Invalidated by mtime (current crate) or version (deps).
5. **Query** with a 5-stage pipeline: exact path -> case-insensitive -> suffix match -> name match -> not found.
6. **Render** plain text output with smart defaults per item kind, truncated to ~1500 chars.

## Project context

**Inside a Rust project** (directory with `Cargo.toml`): groxide reads the project's dependency graph from `Cargo.toml`. Running `grox` with no arguments shows the current crate's docs. Queries resolve through a priority chain: current crate → direct dependencies → workspace members → transitive dependencies → stdlib → crates.io auto-fetch. The index cache lives in `target/groxide/`.

**Outside a Rust project**: Only stdlib queries (`grox std::collections::HashMap`) and crates.io auto-fetch (`grox serde::Deserialize`) work. Running `grox` with no arguments will error since there is no current crate. The cache for external crates lives in `~/.cache/groxide/`.

## Output design

All documentation content goes to **stdout**. All status/progress messages go to **stderr**. This separation is critical for agent integration — agents pipe stdout into their context window and ignore stderr.

Status messages are prefixed with `[grox]`:

```
[grox] Building index for tokio 1.40.0...
[grox] Building index for tokio 1.40.0... done (2.3s)
```

## Security considerations

**Code execution**: groxide runs `cargo +nightly rustdoc` on crate source code. For your own project and its dependencies, this is the same trust model as `cargo build`. For auto-fetched external crates, groxide downloads source from crates.io and runs rustdoc on it — the same trust model as `cargo install`.

**Network access**: When auto-fetching, groxide contacts the crates.io API to resolve versions and download tarballs. No other network access occurs. Downloads are size-limited (500 MB) with timeouts.

**File system**: Index caches are written to `target/groxide/` (project-local) and `~/.cache/groxide/` (global). External crate tarballs are extracted with path-traversal protection.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions, testing, and development workflow.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
