# groxide

[![CI](https://github.com/jhartquist/groxide/actions/workflows/ci.yml/badge.svg)](https://github.com/jhartquist/groxide/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/jhartquist/groxide/graph/badge.svg)](https://codecov.io/gh/jhartquist/groxide)
[![Crates.io](https://img.shields.io/crates/v/groxide.svg)](https://crates.io/crates/groxide)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/groxide.svg)](LICENSE-MIT)

Query Rust crate documentation from the terminal. Inspired by `go doc` — the path is the query, smart defaults by item kind.

## What it looks like

Inside a Rust project, query any dependency by path. Here's a project that depends on `tokio`:

**Method lookup** — get signature, docs, and examples instantly:

```
$ grox tokio::sync::Mutex::lock

fn tokio::sync::mutex::Mutex::lock

pub async fn lock(&self) -> MutexGuard<'_, T>

Locks this mutex, causing the current task to yield until the lock has
been acquired.  When the lock has been acquired, function returns a
[MutexGuard].

If the mutex is available to be acquired immediately, then this call
will typically not yield to the runtime. However, this is not guaranteed
under all circumstances.

Cancel safety

This method uses a queue to fairly distribute locks in the order they
were requested. Cancelling a call to lock makes you lose your place in
the queue.

Examples

  use tokio::sync::Mutex;

  let mutex = Mutex::new(1);

  let mut n = mutex.lock().await;
  *n = 2;
```

**Module listing** — discover what's in a module:

```
$ grox tokio::sync -l

mod     tokio::sync::broadcast                    A multi-producer, multi-consumer broadcast queue.
mod     tokio::sync::futures                      Named future types.
mod     tokio::sync::mpsc                         A multi-producer, single-consumer queue for sending ...
mod     tokio::sync::oneshot                      A one-shot channel is used for sending a single ...
mod     tokio::sync::watch                        A multi-producer, multi-consumer channel that only ...
struct  tokio::sync::AcquireError                 Error returned from the [`Semaphore::acquire`] function.
struct  tokio::sync::Barrier                      A barrier enables multiple tasks to synchronize the ...
struct  tokio::sync::Mutex                        An asynchronous `Mutex`-like type.
# ... (30 items total)
```

**Works from anywhere** — query the standard library or auto-fetch from crates.io with no project needed:

```
$ grox std::collections::HashMap

struct std::collections::hash_map::HashMap

pub struct HashMap<K, V, S = crate::hash::RandomState, A: Allocator = crate::alloc::Global>

A [hash map] implemented with quadratic probing and SIMD lookup.

By default, HashMap uses a hashing algorithm selected to provide
resistance against HashDoS attacks. ...

Methods:
  pub fn capacity(&self) -> usize              Returns the number of elements the map can hold ...
  pub fn clear(&mut self)                      Clears the map, removing all key-value pairs.
  pub fn contains_key<Q>(&self, k: &Q) -> bool Returns `true` if the map contains a value for ...
  pub fn get<Q>(&self, k: &Q) -> Option<&V>    Returns a reference to the value corresponding ...
  pub fn insert(&mut self, k: K, v: V) -> Option<V>  Inserts a key-value pair into the map.
  ...
```

## Why

LLM coding agents and humans both need fast, token-efficient access to crate docs without leaving the terminal. `groxide` resolves paths like `tokio::sync::Mutex`, auto-builds a queryable index from rustdoc JSON, and renders plain text output tuned for ~200-800 tokens per query.

## Features

- **Path is the query.** `grox serde::Deserialize` — no subcommands.
- **Smart defaults.** Output adapts by item kind (struct, module, function, etc.).
- **Progressive disclosure.** Crate -> module -> type -> method drill-down.
- **Token-efficient.** Output adapted per item kind for concise results.
- **Zero setup.** Auto-builds and caches index on first use.
- **Auto-fetch.** Unknown crates are fetched from crates.io automatically — works outside a project.
- **Standard library.** Query `std`, `core`, and `alloc` directly — works anywhere.
- **Full-text search.** `grox tokio -S "spawn"` searches across a crate's docs.
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

**Inside a Rust project** — queries your dependencies automatically:

```sh
# Query a dependency's type (serde is in your Cargo.toml)
grox serde::Deserialize

# List module contents
grox tokio::sync -l

# Full method documentation
grox tokio::sync::Mutex::lock

# Search across a crate's documentation
grox tokio -S "spawn"

# View source code
grox -s tokio::sync::Mutex::new

# JSON output for programmatic use
grox --json serde::Serialize
```

**Works anywhere** — no Cargo.toml needed:

```sh
# Query standard library
grox std::collections::HashMap

# Auto-fetch any crate from crates.io
grox axum::Router

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
| `--search <QUERY>` | `-S` | Full-text search across documentation |
| `--kind <KIND>` | `-k` | Filter by item kind: `fn`, `struct`, `enum`, `trait`, `type`, `const`, `mod`, `macro` |
| `--private` | `-p` | Include non-public items |
| `--json` | `-j` | JSON Lines output |
| `--impls` | `-i` | Show trait implementations (on types) or implementors (on traits) |
| `--recursive` | `-r` | List all public items recursively in a crate or module tree |
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
