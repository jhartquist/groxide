# 0001 Intentional Rewrite History

Status: accepted
Date: 2026-04-25

## Context

The current project is personal and unpublished. The rewrite is intended to
become a public Rust engineering portfolio piece, not merely a private cleanup.

A clean final codebase matters, but the history should also show the reasoning
sequence: product contract, vocabulary, interfaces, tests, implementation, and
refactoring.

## Decision

Rewrite v2 through intentional commits.

The history may include red commits when they introduce precise failing tests
that are satisfied by the following green commit. Red commits must be explicit
with `test(red):` commit messages.

Large unstructured dumps are discouraged. Each commit should make one decision
or one behavior visible.

## Consequences

- The rewrite branch can temporarily contain failing commits.
- The final public HEAD must be green.
- Reviewers can read the history as a design narrative.
- Work should pause for ADRs when a decision changes the product contract or
  architectural shape.
