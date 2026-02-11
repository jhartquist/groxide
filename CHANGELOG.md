# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-01-01

### Added

- Core CLI with path-based queries (`grox serde::Deserialize`).
- Rustdoc JSON index builder with 4-pass algorithm (parent map, path computation, item conversion, relationships).
- Disk cache with mtime invalidation (current crate) and version invalidation (dependencies).
- 5-stage query engine: exact path, case-insensitive, suffix match, name match, not found.
- Full-text search with 5-tier scoring (`--search`).
- Plain text renderer with smart defaults per item kind.
- List mode (`--list`), JSON Lines mode (`--json`), source mode (`--source`), impls mode (`--impls`).
- Crate resolution chain: current crate, dependencies, workspace, transitive deps, stdlib, auto-fetch.
- Auto-fetch external crates from crates.io with version pinning (`serde@1.0.210`).
- Standard library support (`std`, `core`, `alloc`).
- README display (`--readme`).
- Kind filtering (`--kind fn`, `--kind struct`, etc.).
- Private item display (`--private`).
- Feature flag support (`--features`, `--all-features`, `--no-default-features`).
- Truncation with progressive disclosure (paragraph, sentence, word, hard boundary).
- Ambiguous match handling with suggestions and "Did you mean" hints.
- Single-segment query reinterpretation (e.g., `grox Mutex` searches current crate).
- Multi-crate search fallback across cached dependency indices.
- Token-efficient output (~200-800 tokens per query).
