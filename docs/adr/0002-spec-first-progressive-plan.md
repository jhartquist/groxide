# 0002 Spec First, Progressive Implementation Plan

Status: accepted
Date: 2026-04-25

## Context

The rewrite needs a full destination before implementation begins, but an
exhaustive implementation plan would pretend to know details that should be
learned during the work.

## Decision

Define the behavioral spec up front. Keep the implementation plan progressive.

The spec states user-visible behavior, output contracts, resolution semantics,
cache behavior, and error behavior. The plan defines the order of deepening:
docs, types, interfaces, failing tests, implementation, refactor.

Implementation details may change as long as the spec is updated through ADRs
when behavior or architecture changes.

## Consequences

- Tests can be written against the spec early.
- The plan can adapt without losing discipline.
- Spec drift is treated as a bug.
- Contributors can distinguish product commitments from implementation guesses.
