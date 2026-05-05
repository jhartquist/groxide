# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-05-05

Initial release.

### Added

- Path-based query CLI: `grox <path>` resolves Rust crate items by path with smart defaults per item kind.
- Crate resolution chain: current crate, direct dependencies, workspace members, transitive deps, stdlib, then crates.io auto-fetch.
- 5-stage query pipeline (exact path, case-insensitive, suffix match, name match, not found) with "Did you mean" suggestions.
- Cross-crate re-export resolution: queries follow `pub use` re-exports, including wildcard re-exports.
- Render modes: default (per-kind), `--brief`, `--docs`, `--source`, `--json`, `--readme`.
- Recursive listing (`--recursive`), composable with `-b`, `-d`, `-s`.
- Trait implementations: `--impls` lists all, `--impls-of <TRAIT>` filters to a specific trait.
- Full-text search (`--search`) with `|` (OR) and space (AND) combinators.
- Standard library support (`std`, `core`, `alloc`).
- Auto-fetch from crates.io with version pinning (e.g. `serde@1.0.210`).
- Feature flag support (`--features`, `--all-features`, `--no-default-features`).
- Kind filter (`--kind`) and private-item display (`--private`).
- Rustdoc JSON index built via `cargo +nightly rustdoc --output-format json`, cached on disk and invalidated by source mtime (current crate) or version (dependencies, stdlib, external crates).
- Truncation with progressive disclosure (paragraph, sentence, word, hard boundary) targeting roughly 200-800 tokens per query.
- Status messages on stderr, content on stdout, for clean LLM-agent integration.
- Agent skill bundle under `skills/groxide/`, installable via `npx skills add` or by copying into `~/.claude/skills/`.
