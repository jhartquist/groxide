# groxide

[![Crates.io](https://img.shields.io/crates/v/groxide.svg)](https://crates.io/crates/groxide)
[![docs.rs](https://img.shields.io/docsrs/groxide.svg)](https://docs.rs/groxide)
[![CI](https://github.com/jhartquist/groxide/actions/workflows/ci.yml/badge.svg)](https://github.com/jhartquist/groxide/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Query Rust crate documentation from the terminal. Works on the current crate, its dependencies, the stdlib, and any crate on crates.io.

> [!WARNING]
> This project was built as an experiment in agentic engineering with heavy AI assistance. I didn't write the code by hand and I don't claim to understand every aspect of it. I came up with the idea, the general strategy, and the design plan. That being said, I've been using `grox` daily across several Rust projects and have tested it carefully. I plan to rewrite it by hand when I have time.

## What it looks like

Inside a Rust project, query any dependency by path. Method lookup gets you signature, docs, and examples:

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

Recursive listing shows what's in a module:

```
$ grox -r tokio::sync

tokio::sync:
  struct  tokio::sync::AcquireError                    pub struct AcquireError(_)    [feature: sync]
  struct  tokio::sync::Barrier                         pub struct Barrier    [feature: sync]
  struct  tokio::sync::Mutex                           pub struct Mutex<T: ?Sized>    [feature: sync]
  struct  tokio::sync::Notify                          pub struct Notify    [feature: sync]
  struct  tokio::sync::OnceCell                        pub struct OnceCell<T>    [feature: sync]
  struct  tokio::sync::RwLock                          pub struct RwLock<T: ?Sized>    [feature: sync]
  struct  tokio::sync::Semaphore                       pub struct Semaphore    [feature: sync]
  mod     tokio::sync::broadcast                       pub mod broadcast    [feature: sync]
  mod     tokio::sync::mpsc                            pub mod mpsc    [feature: sync]
  mod     tokio::sync::oneshot                         pub mod oneshot    [feature: sync]
  ...
```

It works outside a project too. Standard library queries and crates.io auto-fetch need no `Cargo.toml`:

```
$ grox std::collections::HashMap

struct std::collections::HashMap

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

## Install

```sh
cargo install groxide
```

Or from source:

```sh
git clone https://github.com/jhartquist/groxide.git
cd groxide
cargo install --path .
```

Requires Rust stable (MSRV 1.88) and a nightly toolchain for rustdoc JSON generation:

```sh
rustup toolchain install nightly
```

### Agent skill

`groxide` ships with an [agent skill](https://agentskills.io) that teaches AI coding agents how to use it.

For Claude Code:

```sh
cp -r skills/groxide ~/.claude/skills/
```

For other agents (Codex, Cursor, Copilot, and [20+ others](https://agentskills.io)):

```sh
npx skills add jhartquist/groxide
```

## Usage

```sh
# Method docs
grox tokio::sync::Mutex::lock

# Search across a crate's docs
grox tokio -S "spawn"

# Source code with file path and line numbers
grox -s tokio::sync::Mutex::new

# Recursive listing of a module
grox -r tokio::sync

# Brief skeleton (names only)
grox -r -b tokio

# Trait implementations (all)
grox --impls wgpu::Device

# Trait implementations filtered to a specific trait
grox --impls-of Clone wgpu::Device

# JSON Lines output
grox --json serde::Serialize

# Pin to a specific version
grox serde@1.0.210::Deserialize

# Wipe the cache
grox --clear-cache
```

## Reference

```
grox [OPTIONS] [PATH]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--brief` | `-b` | Item names only |
| `--docs` | `-d` | Full rendered documentation |
| `--source` | `-s` | Source with file path and line numbers |
| `--search <QUERY>` | `-S` | Full-text search (`\|` for OR, space for AND) |
| `--kind <KIND>` | `-k` | Filter by `fn`, `struct`, `enum`, `trait`, `type`, `const`, `mod`, `macro` |
| `--private` | `-p` | Include non-public items |
| `--json` | `-j` | JSON Lines |
| `--impls` | `-i` | Trait implementations |
| `--impls-of <TRAIT>` | | Filter trait implementations to one trait |
| `--recursive` | `-r` | List items recursively (composes with `-b`, `-d`, `-s`) |
| `--readme` | | Crate README |
| `--clear-cache` | | Wipe the cache |
| `--manifest-path <PATH>` | | Path to `Cargo.toml` |
| `--features <FEATURES>` | | Comma-separated features |
| `--all-features` | | All features |
| `--no-default-features` | | Skip the default feature |

`-d` and `-s` compose: `grox -d -s path` shows source with full docs. `-j`, `-p`, and `-k` are orthogonal and combine with any mode.

Exit codes: `0` success, `1` not found, `2` error.

## How it works

1. **Resolve** the crate: current project, dependency, workspace member, transitive dep, stdlib, or crates.io auto-fetch.
2. **Generate** rustdoc JSON via `cargo +nightly rustdoc --output-format json`.
3. **Build** a queryable index from the JSON.
4. **Cache** the index to disk. Current crate caches live in `<workspace_target>/groxide/<crate>.groxide` (invalidated when `Cargo.toml`, `src/**/*.rs`, or `build.rs` change). Dependency, stdlib, and external caches live under `~/.cache/groxide/` keyed on version (deps), toolchain hash (stdlib), or version (external). Feature flags and `--private` are hashed into the filename so they don't collide.
5. **Query** through a 5-stage pipeline: exact path, case-insensitive, suffix match, name match, not found.
6. **Render** plain text with smart defaults per item kind, truncated to ~1500 chars.

## Inside vs outside a Rust project

Inside a project (directory with `Cargo.toml`), `grox` reads the project's dependency graph. Running `grox` with no arguments shows the current crate's docs. Queries resolve through: current crate, direct dependencies, workspace members, transitive dependencies, stdlib, then crates.io auto-fetch. Each step matches by exact name (with `-`/`_` equivalence): `grox candle` in a workspace whose only members are `candle-pitch` and `candle-crepe` falls through to a crates.io fetch of `candle`. Use the full member name to target a workspace crate.

Outside a project, only stdlib queries and crates.io auto-fetch work. Running `grox` with no arguments errors out (no current crate). The cache for external crates lives in `~/.cache/groxide/`.

## Output for agents

All documentation content goes to **stdout**. All status messages go to **stderr**. This separation matters for LLM agents: they pipe stdout into context and ignore stderr.

Status messages are prefixed with `[grox]`:

```
[grox] Building index for tokio 1.40.0...
[grox] Building index for tokio 1.40.0... done (2.3s)
```

## Security

`grox` invokes `cargo +nightly rustdoc` on crate source. This runs the crate's `build.rs` and any proc macros it depends on; same code execution as `cargo build`, but **not** the same as `cargo install` (no built binary is executed).

Auto-fetch matters: when you query a crate that isn't already a dependency (e.g. `grox some-random-crate`), `grox` downloads it from crates.io and compiles it. If you wouldn't add it to `Cargo.toml`, don't query it. Pin a version (`grox crate@1.2.3`) if you want reproducibility.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
