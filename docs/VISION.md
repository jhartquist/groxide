# Groxide Rewrite Vision

Status: draft for the v2 rewrite.

Working name: `groxide`. The public package and command names are intentionally
deferred. Specs use `<cmd>` when the exact binary name is not important.

## Purpose

Groxide is a command-line documentation reader for Rust crates. It lets a human
or coding agent ask for documentation by writing the Rust path they already
know:

```text
<cmd> serde::Deserialize
<cmd> tokio::sync::Mutex::lock
<cmd> -r -b axum
```

The project exists to make Rust documentation available from the terminal with
low ceremony, predictable output, and enough structure for automation.

## Audience

- Coding agents that need compact, trustworthy Rust API context.
- Rust engineers who want fast terminal access to crate documentation.
- Maintainers who want a small, legible Rust codebase that demonstrates strong
  modeling, testing, and error handling.

## Product Promise

The path is the query. A user should not need to learn a subcommand tree before
asking for a crate, module, type, function, method, README, search result, impl
list, or source view.

The tool should feel like `go doc` adapted to Rust's crate ecosystem and
rustdoc JSON model.

## Engineering Standard

The rewrite should be publishable as a Rust engineering portfolio piece:

- Clear domain types before plumbing.
- Small modules with deep interfaces.
- Pure behavior isolated from cargo, filesystem, cache, and network adapters.
- Tests that encode user-visible behavior without duplicating incidental
  implementation details.
- Output contracts that are stable, minimal, and easy to snapshot.
- Error messages that explain the next useful action.
- A git history that shows decisions in order instead of hiding them in a large
  initial dump.

## Design Principles

1. **Path first.** The positional path remains the primary interface.
2. **Smart defaults.** Common queries require no flags.
3. **Progressive disclosure.** Crate, module, type, method, docs, source, and
   recursive views deepen only when requested.
4. **Bounded output by default.** Default output is useful in terminals and
   agent contexts. Verbose modes are explicit.
5. **Plain text first.** Human output is indentation-based plain text, not
   Markdown.
6. **Structured output when requested.** JSON output is stable and intended for
   automation.
7. **Zero setup.** Inside a Rust project, current crate and dependency docs work
   without manual indexing. Outside a project, stdlib and crates.io queries work.
8. **Rustdoc JSON is the source of truth.** The internal model normalizes
   rustdoc JSON instead of inventing a separate parser.
9. **Adapters at the edge.** Cargo, crates.io, filesystem, cache, and stdout are
   edges. Resolution, indexing, search, and rendering are mostly pure.
10. **Every test earns its place.** Tests should cover a behavior or invariant
    once at the cheapest meaningful level.

## Non-goals

- Replacing docs.rs.
- Rendering full rustdoc HTML.
- Supporting every rustdoc edge case before the core contract is stable.
- Building an interactive TUI.
- Optimizing for all historical behavior of the current implementation.
- Preserving the current internal architecture.

## Success Criteria

The rewrite is ready to publish when:

- The v2 spec is current and matches implementation.
- `cargo fmt -- --check`, `cargo clippy --all-targets`, and `cargo test` pass.
- The CLI examples in the README are covered by tests.
- Every output mode has snapshot or contract tests.
- The cache can be cleared, rebuilt, and trusted.
- The public name, binary name, and crate metadata have an accepted ADR.
- The final history reads as a sequence of intentional design, red, green, and
  refactor commits.
