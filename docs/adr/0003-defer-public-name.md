# 0003 Defer Public Name

Status: accepted
Date: 2026-04-25

## Context

The existing personal names `groxide` and `grox` are meaningful to the project,
but `grox` is already taken on crates.io. The user wants freedom to choose a
better name before publishing.

## Decision

Defer the final public name and command name.

Use `groxide` as the working project name. Use `<cmd>` in v2 specs when command
syntax is being described independent of the exact binary name.

Do not bake a final public name into v2 examples, package metadata, or release
docs until a naming ADR accepts it.

## Consequences

- The rewrite can proceed without blocking on naming.
- Specs remain valid if the binary name changes.
- A future ADR must settle package name, repository name, binary name, and
  crate metadata before publication.
