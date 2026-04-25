# Output Contract

Status: draft.

## General Rules

Human output MUST be plain text.

Markdown from rustdoc comments SHOULD be stripped or rendered into plain text.

Output MUST be deterministic across runs for the same input index.

Default output MUST be bounded. Full documentation requires `--docs`.

## Default Detail

Crate and module views:

- Header with kind and path.
- Bounded crate/module docs when present.
- Grouped direct children.
- Child rows use names or signatures plus summaries.

Type views:

- Header with kind and path.
- Signature.
- Bounded docs.
- Important child sections such as variants, methods, associated items, and
  trait implementations.
- Long sections are capped with a hint for expansion.

Leaf views:

- Header with kind and path.
- Signature when meaningful.
- Bounded docs.

Default documentation text SHOULD be capped. The exact cap is TBD before
implementation, but the policy is not: default output is not unlimited.

## Brief Detail

`--brief` prints compact structure:

- Containers show item names grouped by kind.
- Recursive brief output shows the documentation tree.
- Brief output omits documentation text and source.

## Docs Detail

`--docs` prints full rendered documentation for the selected item or recursive
set.

For recursive docs, every item SHOULD include path, kind, signature when
meaningful, and full docs.

## Source Detail

`--source` prints:

- Header with kind and path.
- Source file path and line range.
- Verbatim source with line numbers.

If `--docs --source` is used, rendered docs appear before the source block.

Unavailable source MUST produce a useful error or unavailable-source record.

Recursive source output separates source blocks with a stable divider.

## Recursive Output

`--recursive` applies the selected detail tier to all descendants of the
resolved crate or module.

Recursive output MUST be deterministic and grouped by parent path or tree path.

`--kind` filters recursive descendants.

## JSON Output

`--json` is for automation.

JSON output MUST be stable and versioned. The first public release SHOULD
document the schema.

JSON Lines is preferred for lists, recursive output, search, and ambiguity.

Single item output MAY be one JSON object.

JSON records SHOULD include:

- Record kind.
- Item kind.
- Path.
- Signature.
- Summary or docs according to detail tier.
- Source span when available.
- Re-export metadata when applicable.

## Search Output

Human search output MUST show:

- Query.
- Number shown.
- Total matches before truncation.
- Ranked results with score, kind, path, and summary.

Zero-result search MUST print a clear zero-results message to stdout and exit
successfully.

JSON search output MUST include score and match metadata.

## Impls Output

For type queries, `--impls [TRAIT]` shows trait implementations for the type.

For trait queries, `--impls` shows implementors when the index can determine
them. If implementor discovery is unsupported for a source, output MUST say so
clearly instead of silently returning an empty list.

Optional trait filters match simple names and full paths.

## README Output

`--readme` prints the raw README content for the resolved crate.

README output is content, so it goes to stdout.

If no README is available, exit code is `1`.

## Ambiguity Output

Ambiguity output MUST include:

- The original query.
- All visible candidates after deduplication.
- Kind.
- Path.
- Source crate when helpful.
- A hint showing how to query a more specific path.

Ambiguity in JSON mode MUST emit structured candidate records.
