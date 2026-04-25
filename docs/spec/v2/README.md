# V2 Specification

Status: draft.

This directory defines the behavioral destination for the rewrite. The current
implementation and the older `docs/spec/*.md` files are source material, but
the files in this directory are the contract for v2.

The implementation plan may change as the rewrite teaches us more. The product
contract should change only through explicit decisions.

## Spec Files

- `00-product-contract.md` - product shape, core workflows, invariants.
- `01-cli-contract.md` - flags, path grammar, validation, conflicts.
- `02-resolution-contract.md` - crate resolution, item lookup, ambiguity,
  search, methods, re-exports.
- `03-index-contract.md` - internal documentation model and rustdoc
  normalization rules.
- `04-output-contract.md` - plain text, JSON, source, README, search, impls,
  ambiguity, and truncation behavior.
- `05-acquisition-cache-errors.md` - cargo/rustdoc generation, crates.io,
  stdlib, cache, progress, errors, and exit codes.

## Spec Language

- **MUST** means required behavior.
- **SHOULD** means expected behavior unless a documented reason says otherwise.
- **MAY** means allowed behavior.
- **TBD** marks a decision that must be resolved before publication.

## Naming

`groxide` is the working project name. `<cmd>` is used for the binary name until
the naming ADR is finalized.
