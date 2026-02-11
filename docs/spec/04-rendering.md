# Rendering Specification

Complete specification for all output formats in groxide. Covers plain text, list, JSON,
ambiguous display, source view, README view, and impls view. All renderers consume
`DisplayItem` (from `01-types-and-data-model.md`) and write to stdout.

Strict invariant: documentation content goes to stdout, status/progress messages go to stderr.

---

## 1. Output Dispatch

The CLI flags determine which renderer runs. Exactly one renderer handles each invocation.

```
match (flags, query_result) {
    (--readme, _)                       => readme_renderer
    (--search "query", _)               => search_renderer
    (_, NotFound)                       => error (exit 1)
    (--source, Found)                   => source_renderer (single item)
    (--source, Ambiguous)               => source_renderer (all matches, with separators)
    (--json, Ambiguous)                 => ambiguous_json_renderer
    (--list, Ambiguous)                 => ambiguous_list_renderer
    (_, Ambiguous)                      => ambiguous_renderer
    (--json, Found)                     => json_renderer
    (--list, Found)                     => list_renderer
    (--impls, Found)                    => impls_renderer
    (_, Found)                          => text_renderer (default)
}
```

Evaluation order matters: `--readme` and `--search` are checked before item resolution.
`--source` on an ambiguous result renders source for all matches. All other ambiguous
results go through the ambiguous renderer (with mode selection based on `--json` or `--list`).

### Flag interactions with rendering

| Flag | Effect on rendering |
|------|-------------------|
| `--all` | Disables all truncation (methods, trait impls, doc text) |
| `--kind` | Filters children in container views before rendering |
| `--private` | Includes non-public items in child listings |
| `--json` | Switches output format to JSON for any renderer |
| `--all` + `--list` | No visible effect (list is already unlimited) |
| `--all` + `--json` | No visible effect (JSON always shows all data) |
| `--all` + `--impls` | No visible effect (impls is already unlimited) |

---

## 2. Plain Text Renderer -- Crate Root

Triggered when the resolved item is a Module whose path equals the crate name.

### Format

```
mod <crate_name>

<doc_text, markdown-stripped, truncated to 1500 chars unless --all>

Modules:
  <name>                          <summary>
  <name>                          <summary>

Structs:
  <name>                          <summary>

Enums:
  <name>                          <summary>

Functions:
  <signature>                                                     <summary>

Type Aliases:
  <signature>                                                     <summary>
```

### Rules

- Header line: `mod <crate_name>` -- no signature rendered for modules.
- Feature gate annotation on header: `mod <crate_name>  [feature: <gate>]` (two spaces before bracket).
- Blank line after header.
- Doc text: markdown-stripped (see section 7), truncated (see section 8).
- Blank line after doc text (if doc text is non-empty).
- Modules section always first, followed by other top-level items grouped by `KindCategory`.
- Empty categories are omitted entirely (no header printed).
- Blank line between each non-empty category section.
- Items within each category sorted alphabetically by path.

### Example

```
mod serde

A framework for serializing and deserializing Rust data structures.

Modules:
  de                              Deserialization framework.
  ser                             Serialization framework.

Structs:
  Deserializer                    A structure for deserializing data.

Traits:
  Deserialize                     A data structure that can be deserialized.
  Serialize                       A data structure that can be serialized.

Functions:
  pub fn from_str<T>(s: &str) -> Result<T>                        Deserialize from a string.

Type Aliases:
  pub type Result<T> = std::result::Result<T, Error>               A specialized Result type.
```

### --kind filter on crate root

When `--kind` is specified on a crate root query, groxide treats the crate root as a regular
Module (not as the Crate variant). This ensures all children of the filtered kind are shown in
a single grouped listing rather than split into modules/non-modules.

---

## 3. Plain Text Renderer -- Module

Triggered when the resolved item is a Module whose path differs from the crate name.

### Format

```
mod <full_module_path>

<doc_text, markdown-stripped>

<category_1_header>:
  <item>                          <summary>

<category_2_header>:
  <item>                          <summary>
```

### Rules

- Header: `mod <full_path>` (e.g., `mod tokio::sync`).
- No signature rendered for modules.
- Children are grouped by `KindCategory` in display order.
- Rendering style per category is determined by `KindCategory::uses_signature_display()`.

### Category display order

Categories render in this fixed order (matching `KindCategory` `Ord` discriminant order):

| # | Category | Header | Item display style |
|---|----------|--------|--------------------|
| 1 | Modules | `Modules:` | name + summary (pad name to 30) |
| 2 | Structs | `Structs:` | name + summary (pad name to 30) |
| 3 | Enums | `Enums:` | name + summary (pad name to 30) |
| 4 | Unions | `Unions:` | name + summary (pad name to 30) |
| 5 | Traits | `Traits:` | name + summary (pad name to 30) |
| 6 | Functions | `Functions:` | signature + summary (pad sig to 58) |
| 7 | Type Aliases | `Type Aliases:` | signature + summary (pad sig to 58) |
| 8 | Constants | `Constants:` | signature + summary (pad sig to 58) |
| 9 | Statics | `Statics:` | signature + summary (pad sig to 58) |
| 10 | Macros | `Macros:` | name + summary (pad name to 30) |
| 11 | Primitives | `Primitives:` | name + summary (pad name to 30) |

Container-like items (Modules, Structs, Enums, Unions, Traits, Macros, Primitives) use
`name + summary`. Value-like items (Functions, Type Aliases, Constants, Statics) use
`signature + summary`.

### Example

```
mod tokio::sync

Synchronization primitives for use in asynchronous contexts.

Structs:
  Barrier                         A barrier enables multiple tasks to synchronize the beginning of some computation.
  Mutex                           An asynchronous Mutex-like type.
  Notify                          Notifies a single task to wake up.
  RwLock                          An asynchronous reader-writer lock.
  Semaphore                       Counting semaphore performing asynchronous permit acquisition.

Enums:
  TryLockError                    Error returned from try_lock.

Modules:
  mpsc                            A multi-producer, single-consumer queue.
  oneshot                         A one-shot channel is used for sending a single value.

Functions:
  pub fn broadcast::channel<T>(capacity: usize) -> (Sender, Receiver)  Creates a broadcast channel.
```

---

## 4. Plain Text Renderer -- Type (Struct/Enum/Union)

Triggered when the resolved item is a Struct, Enum, or Union.

### Format

```
<kind> <path>

<signature>

<doc_text, markdown-stripped, truncated>

Variants:
  <variant_signature>                                              <summary>

Methods:
  <method_signature>                                               <summary>

Trait Implementations: (showing 5 of 22, use --impls to expand)
  impl Clone
  impl Debug
  impl Default
  impl Display
  impl From
```

### Rules

- Header: `<kind_short_name> <path>` (e.g., `struct tokio::sync::Mutex`).
- Blank line, then signature, then blank line, then doc text.
- Sections appear in order: Variants (enums only), Methods, Trait Implementations.
- Each section preceded by a blank line.
- Variants: sorted alphabetically by path. Rendered with signature + summary (pad sig to 58).
- Methods: sorted alphabetically by path. Rendered with signature + summary (pad sig to 58).
- Method limit: 15 by default. When exceeded:
  `Methods: (showing 15 of <total>, use --all to expand)`
- Trait impl limit: 5 by default. When exceeded:
  `Trait Implementations: (showing 5 of <total>, use --impls to expand)`
- Trait impl ordering: non-synthetic first (alphabetically by trait path), then synthetic (alphabetically).
- Trait impl format: `  impl <TraitPath>` (2-space indent).
- `--all` removes the method limit and trait impl limit.

### Struct example

```
struct tokio::sync::Mutex

pub struct Mutex<T: ?Sized>

An asynchronous Mutex-like type.

The primary use of this type is to provide shared mutable access to data
that is held across .await boundaries.

Methods:
  pub fn blocking_lock(&self) -> MutexGuard<'_, T>                Blockingly locks this Mutex.
  pub fn get_mut(&mut self) -> &mut T                             Returns a mutable reference to the underlying data.
  pub fn into_inner(self) -> T                                    Consumes the mutex, returning the underlying data.
  pub fn lock(&self) -> impl Future<Output = MutexGuard<'_, T>>   Locks this mutex, causing the current task to yield.
  pub fn new(t: T) -> Self                                        Creates a new lock in an unlocked state.
  pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, TryLockError>  Attempts to acquire the lock.

Trait Implementations: (showing 5 of 12, use --impls to expand)
  impl Debug
  impl Default
  impl From
  impl Send
  impl Sync
```

### Enum example

```
enum serde_json::Value

pub enum Value

Represents any valid JSON value.

Variants:
  Array(Vec<Value>)                                                An array of values.
  Bool(bool)                                                       A boolean value.
  Null                                                             Represents a JSON null value.
  Number(Number)                                                   A JSON number.
  Object(Map<String, Value>)                                       An object (key-value map).
  String(String)                                                   A JSON string.

Methods:
  pub fn as_array(&self) -> Option<&Vec<Value>>                    If the Value is an Array, returns the array.
  pub fn as_bool(&self) -> Option<bool>                            If the Value is a Bool, returns the boolean.
  pub fn as_str(&self) -> Option<&str>                             If the Value is a String, returns the string.
  pub fn is_null(&self) -> bool                                    Returns true if the Value is a Null.

Trait Implementations: (showing 5 of 18, use --impls to expand)
  impl Clone
  impl Debug
  impl Default
  impl Display
  impl From
```

---

## 5. Plain Text Renderer -- Trait

Triggered when the resolved item is a Trait or TraitAlias.

### Format

```
trait <path>

<signature>

<doc_text>

Required Methods:
  <method_signature>                                               <summary>

Provided Methods:
  <method_signature>                                               <summary>
```

### Rules

- Header: `trait <path>`.
- If the trait has BOTH required and provided methods: use distinct headers
  `"Required Methods:"` and `"Provided Methods:"`, separated by a blank line.
- If the trait has ONLY one kind (all required or all provided): use the generic
  `"Methods:"` header.
- Required methods: `has_body == false`. Provided methods: `has_body == true`.
- Methods sorted alphabetically by path within each section.
- Method limit: 15 per section. When exceeded:
  `Required Methods: (showing 15 of <total>, use --all to expand)`
- Marker traits with no methods show:
  ```
  (no methods)
  ```
  This distinguishes an intentionally empty trait from truncation.

### Example (both required and provided)

```
trait tokio::io::AsyncRead

pub trait AsyncRead

Read bytes from a source asynchronously.

Required Methods:
  fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>>  Attempts to read from the source.

Provided Methods:
  fn chain<R>(self, next: R) -> Chain<Self, R>                    Creates an adaptor which chains this stream with another.
  fn take(self, limit: u64) -> Take<Self>                         Creates an adaptor which reads at most limit bytes.
```

### Example (only required methods)

```
trait Iterator

pub trait Iterator

An interface for dealing with iterators.

Methods:
  fn next(&mut self) -> Option<Self::Item>                        Advances the iterator and returns the next value.
```

### Example (marker trait, no methods)

```
trait Send

pub unsafe auto trait Send

Types that can be transferred across thread boundaries.

(no methods)
```

---

## 6. Plain Text Renderer -- Leaf Items

Triggered for all items that are not modules, types, or traits: functions, constants,
type aliases, macros, statics, variants, fields, primitives.

### Format

```
<kind> <path>

<signature>

<doc_text, full up to truncation limit>
```

### Rules

- Header: `<kind_short_name> <path>` (e.g., `fn tokio::spawn`).
- Signature is always rendered (never skipped).
- Doc text is the full doc comment (not just summary), truncated at 1500 characters.
- Items with no documentation: the doc section is omitted entirely (no blank line emitted).
  Output goes directly from signature to end.

### Function example

```
fn tokio::spawn

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>

Spawns a new asynchronous task, returning a JoinHandle for it.

The provided future will start running in the background immediately
when spawn is called, even if you don't await the returned JoinHandle.

Examples

  let handle = tokio::spawn(async {
      // async work here
      42
  });

  let result = handle.await.unwrap();
  assert_eq!(result, 42);
```

### Constant example

```
const std::f64::consts::PI

pub const PI: f64

Archimedes' constant (pi = 3.14159...).
```

### Type alias example

```
type tokio::io::Result

pub type Result<T> = std::result::Result<T, std::io::Error>

A specialized Result type for I/O operations.
```

### Variant example (queried directly)

```
variant serde_json::Value::Array

Array(Vec<Value>)

Represents a JSON array.
```

### Field example (queried directly)

```
field my_crate::Point::x

x: f64

The x coordinate.
```

### Macro example

```
macro serde::derive::Serialize

#[derive(Serialize)]

Derive macro for the Serialize trait.
```

---

## 7. Markdown Stripping

Doc text stored in `IndexItem.docs` is raw markdown. Before plain text rendering, groxide
strips markdown formatting. JSON output preserves the raw markdown (no stripping).

### Transformation rules

| Markdown element | Stripped output |
|-----------------|----------------|
| `# Heading` | `Heading` (all leading `#` and space removed) |
| `## Sub Heading` | `Sub Heading` |
| `**bold**` / `__bold__` | `bold` |
| `*italic*` / `_italic_` | `italic` |
| `` `inline code` `` | `inline code` (backticks removed) |
| `[link text](url)` | `link text` (URL removed) |
| `[link text][ref]` | `link text` |
| ` ```lang ... ``` ` | Indented by 2 spaces, blank line before and after |
| `> blockquote` | Plain text (`>` prefix removed) |
| `- bullet` / `* bullet` / `+ bullet` | Plain text (marker removed, indent preserved) |
| `1. numbered` | Plain text (marker removed, indent preserved) |
| `---` / `***` (horizontal rules) | Blank line |

### Code fence handling

Fenced code blocks (triple backtick) are converted to 2-space-indented blocks:

Input:
````markdown
Here is an example:

```rust
let x = 42;
let y = x + 1;
```
````

Output:
```
Here is an example:

  let x = 42;
  let y = x + 1;
```

Relative indentation within the code block is preserved. The language identifier
after the opening fence is stripped.

---

## 8. Truncation Rules

### DisplayLimits defaults

| Limit | Default value | Override |
|-------|-------------|---------|
| `max_methods` | 15 | `--all` |
| `max_trait_impls` | 5 | `--impls` for full list, `--all` for full list |
| `max_doc_length` | 1500 characters | `--all` |
| `expand_all` | false | `--all` sets to true |

When `expand_all` is true, all limits are set to `usize::MAX` (effectively infinite).

### Doc text truncation algorithm

When doc text exceeds `max_doc_length` (1500 characters by default), truncation proceeds
through a priority chain:

1. **Paragraph boundary** -- Find the last `\n\n` before the limit. If found, break there
   and append `...`.
2. **Sentence boundary** -- Find the last `. `, `! `, or `? ` (punctuation followed by
   whitespace) before the limit. If found, break after the punctuation (no `...` appended,
   since the sentence ended naturally).
3. **Word boundary** -- Find the last space character before the limit. Break there and
   append `...`.
4. **Hard truncate** -- Break at exactly `max_doc_length` at a safe UTF-8 boundary.
   Append `...`.

### UTF-8 safety

All truncation operations use `str::floor_char_boundary()` (or the equivalent manual
calculation for older Rust editions) to find a valid character boundary at or before the
cut point. This prevents splitting multi-byte characters (e.g., truncating in the middle
of a 3-byte CJK character).

Implementation:
```rust
fn safe_truncate_point(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    // Walk backwards from max_bytes to find a char boundary
    let mut pos = max_bytes;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}
```

### Methods truncation

When methods exceed `max_methods`:

```
Methods: (showing 15 of 47, use --all to expand)
  pub fn method_1(&self) -> T                                      First method summary.
  pub fn method_2(&self) -> T                                      Second method summary.
  ...15 methods shown...
```

The truncation notice is part of the section header line. Only the first `max_methods`
methods are rendered (alphabetical order maintained).

Same format for labeled sections:
```
Required Methods: (showing 15 of 23, use --all to expand)
```

### Trait implementations truncation

When trait impls exceed `max_trait_impls`:

```
Trait Implementations: (showing 5 of 22, use --impls to expand)
  impl Clone
  impl Debug
  impl Default
  impl Display
  impl From
```

Note: the escape hatch message says `--impls` (not `--all`), because `--impls` gives the
dedicated impls view. `--all` also works to expand them.

Ordering for truncated view: non-synthetic impls first (alphabetically), then synthetic
impls (alphabetically). This ensures user-defined impls like `Clone`, `Debug` appear
before auto-traits like `Send`, `Sync`.

### Search results truncation

Search results are capped at 20 results. When truncated, the header changes:
- Normal: `8 results for "query":`
- Truncated: `20 of 45 results for "query":`

### Ambiguous results truncation

For 6+ matches in default mode, at most 10 items are shown:
```
... (5 more, use a more specific path)
```

---

## 9. List Renderer

Triggered by `--list` / `-l`. Produces one line per child item.

### Format

```
<kind>  <path>  <summary>
```

No leading indent. Columns are dynamically sized from the items being listed.

### Column width calculation

```rust
max_kind_width = items.iter().map(|i| i.kind.short_name().len()).max()
max_path_width = items.iter().map(|i| i.path.len()).max()
```

Each line: `{kind:<max_kind_width$}  {path:<max_path_width$}  {summary}`

Two spaces separate each column.

### What gets listed by DisplayItem variant

| DisplayItem | Listed items |
|-------------|-------------|
| `Crate` | All top-level modules, then non-module top-level items (category order) |
| `Module` | All children (category order: modules, structs, enums, ..., macros, primitives) |
| `Type` | All methods |
| `Trait` | Required methods + provided methods |
| `Leaf` | Single item line |

### Sort order

Items within each category group maintain their alphabetical sort from the
grouping step. Categories appear in the standard display order (section 3).

### Feature gate annotation in list mode

Feature-gated items include the annotation after the summary:
```
mod   tokio::fs   Filesystem utilities.  [feature: fs]
```

### Example

```
struct  tokio::sync::Barrier        A barrier enables multiple tasks to synchronize.
struct  tokio::sync::Mutex          An asynchronous Mutex-like type.
struct  tokio::sync::Notify         Notifies a single task to wake up.
struct  tokio::sync::RwLock         An asynchronous reader-writer lock.
struct  tokio::sync::Semaphore      Counting semaphore performing asynchronous permit acquisition.
enum    tokio::sync::TryLockError   Error returned from try_lock.
mod     tokio::sync::mpsc           A multi-producer, single-consumer queue.
mod     tokio::sync::oneshot        A one-shot channel is used for sending a single value.
```

---

## 10. JSON Renderer -- Doc View

Triggered by `--json` on a found item. Produces a single JSON object to stdout.

### JSON structure: `JsonDocItem`

```json
{
  "path": "tokio::sync::Mutex",
  "kind": "struct",
  "signature": "pub struct Mutex<T: ?Sized>",
  "doc": "An asynchronous Mutex-like type.\n\nThe primary use...",
  "feature_gate": null,
  "methods": [
    {
      "name": "lock",
      "signature": "pub async fn lock(&self) -> MutexGuard<'_, T>",
      "summary": "Locks this mutex."
    },
    {
      "name": "new",
      "signature": "pub fn new(t: T) -> Self",
      "summary": "Creates a new lock in an unlocked state."
    }
  ],
  "trait_impls": ["Clone", "Debug", "Default", "Send", "Sync"],
  "variants": null
}
```

### Field definitions

| Field | Type | Present when |
|-------|------|-------------|
| `path` | `String` | Always |
| `kind` | `String` | Always (`ItemKind::short_name()`) |
| `signature` | `String` | Always (empty string for modules) |
| `doc` | `String` | Always (raw markdown, NOT stripped) |
| `feature_gate` | `String` or `null` | Always. `null` if no feature gate. |
| `methods` | `Array<JsonMethod>` or `null` | Present on Type and Trait items. `null`/omitted otherwise. |
| `trait_impls` | `Array<String>` or `null` | Present on Type items (trait paths). `null`/omitted otherwise. |
| `variants` | `Array<JsonVariant>` or `null` | Present on Enum items. `null`/omitted otherwise. |

Optional fields (`methods`, `trait_impls`, `variants`) are omitted entirely from the JSON
output when they are `None`/empty (via `#[serde(skip_serializing_if = "Option::is_none")]`).

### Sub-structures

**JsonMethod:**
```json
{"name": "lock", "signature": "pub async fn lock(&self) -> MutexGuard<'_, T>", "summary": "Locks this mutex."}
```

**JsonVariant:**
```json
{"name": "Array", "signature": "Array(Vec<Value>)", "summary": "An array of values."}
```

### Crate root / Module JSON

For crate root and module items, the JSON output consists of multiple JSON Lines:

- Line 1: the `JsonDocItem` for the crate/module itself.
- Lines 2+: one `JsonListItem` per child.

```json
{"path":"serde","kind":"mod","signature":"","doc":"A framework for serializing...","feature_gate":null}
{"path":"serde::de","kind":"mod","signature":"","summary":"Deserialization framework."}
{"path":"serde::ser","kind":"mod","signature":"","summary":"Serialization framework."}
{"path":"serde::Deserialize","kind":"trait","signature":"pub trait Deserialize<'de>","summary":"A data structure that can be deserialized."}
```

**JsonListItem** fields: `path`, `kind`, `signature`, `summary`. No `doc` field (summary only).

### Crate root JSON must include top_level_items

When rendering a crate root as JSON, the output includes BOTH modules and non-module
top-level items as `JsonListItem` entries. This ensures the JSON consumer gets the complete
picture of the crate's public API.

### Trait JSON specifics

For traits, the `methods` array includes all methods (required and provided). Each method
has an additional `"has_body"` field to distinguish them:

```json
{
  "path": "Iterator",
  "kind": "trait",
  "signature": "pub trait Iterator",
  "doc": "...",
  "methods": [
    {"name": "next", "signature": "fn next(&mut self) -> Option<Self::Item>", "summary": "Advances the iterator.", "has_body": false},
    {"name": "count", "signature": "fn count(self) -> usize", "summary": "Consumes the iterator.", "has_body": true}
  ]
}
```

---

## 11. JSON Renderer -- List Mode (`--json --list`)

When both `--json` and `--list` are active, or when `--json` is used on a container item
that naturally produces list output, the format is JSON Lines (one JSON object per line):

```json
{"path":"tokio::sync::Mutex","kind":"struct","signature":"pub struct Mutex<T: ?Sized>","summary":"An asynchronous Mutex-like type."}
{"path":"tokio::sync::RwLock","kind":"struct","signature":"pub struct RwLock<T: ?Sized>","summary":"An asynchronous reader-writer lock."}
{"path":"tokio::sync::Semaphore","kind":"struct","signature":"pub struct Semaphore","summary":"Counting semaphore."}
```

Each line is a `JsonListItem`: `{path, kind, signature, summary}`. No `doc` field.

### Search results JSON (`--json --search`)

JSON Lines with an additional `score` field:

```json
{"path":"tokio::sync::Mutex","kind":"struct","signature":"pub struct Mutex<T: ?Sized>","summary":"An asynchronous Mutex-like type.","score":100}
{"path":"tokio::sync::MutexGuard","kind":"struct","signature":"pub struct MutexGuard<'a, T: ?Sized>","summary":"RAII structure used to release the mutex.","score":75}
```

Fields: `path`, `kind`, `signature`, `summary`, `score` (integer).

---

## 12. Ambiguous Display

Triggered when `QueryResult::Ambiguous` is returned and the output mode is not `--source`.

### Pre-processing: deduplication

Before rendering, ambiguous indices are deduplicated by `(path, kind)` pair. This removes
true duplicates (e.g., `Debug::fmt` and `Display::fmt` resolving to the same path and kind)
while preserving intentional same-path different-kind items (e.g., trait `Parser` and macro
`Parser`).

If deduplication reduces to exactly 1 item, groxide renders it as a full `DisplayItem::Leaf`
instead of showing the ambiguous display.

### Mode selection

| Condition | Mode |
|-----------|------|
| `--json` flag | JSON mode |
| `--list` flag | List mode |
| No flag, and exactly 2 matches where one is a trait and one is a macro | Macro/trait special format |
| No flag, 2-5 matches | Few matches format |
| No flag, 6+ matches | Many matches format |

### Few matches (2-5): detailed format

```
Found <N> items matching "<query_name>":

--- <path_1> ---
<signature>
<first_paragraph_of_docs>

--- <path_2> ---
<signature>
<first_paragraph_of_docs>
```

**Priority display logic:**

Items are classified as "primary" or "deeply nested":
- **Primary**: 2-4 path segments AND not a variant. Get full `---` separator display
  with signature and first paragraph of docs.
- **Deeply nested**: 5+ path segments OR variants. Get compact display.

When both primary and deeply nested items exist:

```
Found 4 items matching "Error":

--- serde::de::Error ---
pub trait Error: Sized
When a Deserialize implementation encounters an error.

--- serde::ser::Error ---
pub trait Error: Sized
When a Serialize implementation encounters an error.

Also found in nested contexts:
  struct  serde::de::value::Error                A minimal deserialization error.
  struct  serde::ser::value::Error               A minimal serialization error.
```

Compact format for deeply nested items: `  {kind:<7}  {path:<38}  {summary}`

**First paragraph extraction:**
1. Find first blank line (`\n\n`) -- return text before it.
2. No blank line -- extract first 3 sentences (ending with `.`, `!`, or `?` followed by
   whitespace).
3. No sentences -- first newline (if <=200 chars) or truncate at ~200 chars at word
   boundary with `...`.

### Many matches (6+): condensed format

```
Found 14 items matching "Error":

struct  serde::de::Error                              When deserialization encounters an error.
struct  serde::ser::Error                              When serialization encounters an error.
trait   serde::de::Error                              Custom deserialization error handling.
trait   serde::ser::Error                              Custom serialization error handling.
struct  serde::de::value::Error                       A minimal deserialization error.
struct  serde_json::Error                             An error that occurred during parsing.
fn      serde_json::Error::classify                   Categorizes the cause of this error.
fn      serde_json::Error::column                     One-indexed column number of the error.
fn      serde_json::Error::line                       One-indexed line number of the error.
type    serde_json::Result                            Type alias for Result with serde_json::Error.
... (4 more, use a more specific path)
```

- Maximum 10 items shown (`MAX_SHOWN = 10`).
- Kind column: right-padded to 7 characters.
- Path column: right-padded to 38 characters.
- Continuation message shows count of remaining items.

### Macro/trait ambiguity: special format

When exactly 2 matches exist and one is a trait, the other a macro:

```
"Serialize" matches 2 items:

  trait  Serialize               A data structure that can be serialized.
  macro  Serialize               Derive macro for the Serialize trait.

Use --kind trait or --kind macro to select.
```

Kind column padded to 7 characters. Path column padded to 20 characters.

### Ambiguous list mode (`--list` with ambiguous)

One path per line, no formatting:

```
serde::de::Error
serde::ser::Error
serde_json::Error
```

### Ambiguous JSON mode (`--json` with ambiguous)

JSON Lines with `JsonAmbiguousMatch`:

```json
{"path":"serde::de::Error","kind":"trait","signature":"pub trait Error: Sized","summary":"When deserialization encounters an error."}
{"path":"serde::ser::Error","kind":"trait","signature":"pub trait Error: Sized","summary":"When serialization encounters an error."}
```

Fields: `path`, `kind`, `signature`, `summary`. No `doc` field -- uses `summary` only.

---

## 13. Source View (`--source`)

Triggered by `--source` / `-s`. Displays the source code for the resolved item.

### Format: single item

```
// <relative_file_path>:<start_line>-<end_line>
<source_code_verbatim>
```

- File path is relative to the crate root.
- Line range from the item's `SourceSpan`.
- Source code is reproduced exactly as written (no stripping, no indentation changes).

### Format: single-line item

When `line_start == line_end`:

```
// src/lib.rs:42
pub const MAX_SIZE: usize = 1024;
```

### Source root resolution

| CrateSource variant | Source root |
|---------------------|------------|
| `CurrentCrate` | `manifest_dir` (parent of `Cargo.toml`) |
| `Dependency` | `manifest_dir` |
| `Stdlib` | `<sysroot>/lib/rustlib/src/rust/library/` (requires `rust-src` component) |
| `External` (with version) | `~/.cache/groxide/<name>-<version>/` |
| `External` (no version) | Source not available |

The `SourceSpan.file` field contains a relative path (e.g., `src/sync/mutex.rs`). The
absolute path is constructed as `<source_root>/<SourceSpan.file>`.

### Unavailable source

When the item has no `SourceSpan` (empty file or zero line numbers):

```
// source not available (macro-generated or built-in)
```

### File not found

When the resolved file path does not exist:

```
// source not available (Could not read src/foo.rs: No such file or directory)
```

### Ambiguous matches with --source

When `--source` is combined with an ambiguous result, groxide renders source for all
matches with `---` separators:

```
--- tokio::sync::Mutex ---
// src/sync/mutex.rs:42-147
pub struct Mutex<T: ?Sized> {
    ...
}

--- tokio::sync::MutexGuard ---
// src/sync/mutex.rs:150-180
pub struct MutexGuard<'a, T: ?Sized> {
    ...
}
```

### Example

```
// src/sync/mutex.rs:42-147
pub struct Mutex<T: ?Sized> {
    s: semaphore::Semaphore,
    c: UnsafeCell<T>,
}
```

---

## 14. Impls View (`--impls`)

Triggered by `--impls`. Shows all trait implementations (for types) or all implementors
(for traits), with no truncation.

### On types (struct/enum/union)

```
<kind> <path>

<signature>

Trait Implementations:
  impl Clone
  impl Debug
  impl Default
  impl Display
  impl From
  impl Send (synthetic)
  impl Sync (synthetic)
```

- Header: kind + path.
- Blank line, signature, blank line.
- Non-synthetic impls first, sorted alphabetically by trait path.
- Synthetic impls second, sorted alphabetically by trait path.
- If no trait implementations: `No trait implementations.`

### On traits

```
trait <path>

<signature>

Implementors:
  my_crate::MyStruct
  my_crate::OtherStruct
```

- Header: kind + path.
- Blank line, signature, blank line.
- Implementor paths listed alphabetically.
- If no implementors: `No known implementors.`

### On other items (module, crate, leaf)

```
<kind> <path> has no trait implementations.
```

Single line, no header/signature structure.

### Example

```
struct tokio::sync::Mutex

pub struct Mutex<T: ?Sized>

Trait Implementations:
  impl Debug
  impl Default
  impl From
  impl Send
  impl Sync
  impl Freeze
  impl Unpin
  impl UnwindSafe
  impl RefUnwindSafe
```

---

## 15. README View (`--readme`)

Triggered by `--readme`. Prints the crate's README file to stdout with no processing.

### README file location

Resolved based on `CrateSource`:

| CrateSource variant | Search directory |
|---------------------|-----------------|
| `CurrentCrate` | Workspace root directory |
| `Dependency` | `manifest_dir` |
| `External` | `~/.cache/groxide/<name>-<version>/` |
| `Stdlib` | Not available |

### File name search order

groxide searches for README files in this order:

1. `README.md`
2. `README.MD`
3. `Readme.md`
4. `readme.md`
5. `README`
6. `README.txt`

First match wins.

### Output

Raw file contents printed to stdout. The README is NOT markdown-stripped -- it is output
exactly as written in the file.

### Error cases

- Stdlib crate: `README not available for standard library crate '<name>'` (stderr, exit 1).
- No README found: `No README found for <crate_name>` (stderr, exit 1).

### Example

````
# tokio

A runtime for writing reliable asynchronous applications with Rust.

## Overview

Tokio is an event-driven, non-blocking I/O platform for writing asynchronous
applications with the Rust programming language.

## Getting Started

Add the following to your `Cargo.toml`:

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```
````

---

## 16. Column Alignment

All column alignment uses right-padding (left-alignment) with 2-space gaps between columns.
All item lines in container/type listings are indented by 2 spaces.

### Summary line alignment (container-like items in listings)

Used for: Modules, Structs, Enums, Unions, Traits, Macros, Primitives in category listings.

```
  {name:<30}  {summary}
```

- 2-space indent.
- Name right-padded to 30 characters.
- 2 spaces between name and summary.
- If name exceeds 30 characters: minimal 2-space gap (no truncation of name).
- If summary is empty: only the name is printed.

Example:
```
  Mutex                           An asynchronous Mutex-like type.
  VeryLongStructNameThatExceeds30  A struct with a very long name.
```

### Indented line alignment (value-like items and methods/variants)

Used for: Functions, Type Aliases, Constants, Statics in category listings.
Also used for method lines and variant lines on types.

```
  {signature:<58}  {summary}
```

- 2-space indent.
- Signature right-padded to 58 characters.
- 2 spaces between signature and summary.
- If signature exceeds 58 characters: minimal 2-space gap (no truncation of signature).
- If summary is empty: only the signature is printed.

Example:
```
  pub fn new(t: T) -> Self                                        Creates a new lock.
  pub fn very_long_function_name_with_many_params(a: A, b: B, c: C) -> Result<D, E>  Description here.
```

### Condensed ambiguous alignment (6+ matches)

```
{kind:<7}  {path:<38}  {summary}
```

No indent. Kind padded to 7, path padded to 38.

### Macro/trait disambiguation alignment

```
  {kind:<7}  {path:<20}  {summary}
```

2-space indent. Kind padded to 7, path padded to 20.

### List mode alignment

```
{kind:<max_kind_width$}  {path:<max_path_width$}  {summary}
```

No indent. Dynamic column widths computed from the actual items.

### Search results alignment

```
{kind:<max_kind_width$}  {path_with_params:<max_path_width$}  {summary}
```

No indent. Dynamic column widths. For functions/methods, parameter names are extracted
from the signature and appended to the path (e.g., `tokio::fs::read(path)` instead of
`tokio::fs::read`). The `self`, `&self`, and `&mut self` parameters are excluded.

### Alignment constants summary

| Context | Column | Width | Notes |
|---------|--------|-------|-------|
| Category listing (container items) | name | 30 | Fixed |
| Category listing (value items) | signature | 58 | Fixed |
| Condensed ambiguous | kind | 7 | Fixed |
| Condensed ambiguous | path | 38 | Fixed |
| Macro/trait ambiguity | kind | 7 | Fixed |
| Macro/trait ambiguity | path | 20 | Fixed |
| List view | kind | dynamic | `max(kind.short_name().len())` |
| List view | path | dynamic | `max(path.len())` |
| Search results | kind | dynamic | `max(kind.short_name().len())` |
| Search results | path | dynamic | `max(path_with_params.len())` |

---

## 17. Search Results Renderer

Triggered by `--search "query"` / `-S "query"`.

### Plain text format

```
<count> results for "<query>":

<kind>  <path_with_params>  <summary>
<kind>  <path_with_params>  <summary>
```

When results are truncated (more than 20):
```
20 of 45 results for "mutex":

struct  tokio::sync::Mutex                                  An asynchronous Mutex-like type.
struct  tokio::sync::MutexGuard                             RAII structure used to release the mutex.
fn      tokio::sync::Mutex::lock(&self)                     Locks this mutex.
fn      tokio::sync::Mutex::new(t)                          Creates a new lock.
...
```

### Path display enhancement for functions

For function and method results, parameter names are extracted from the signature and
appended to the path for readability:

| Signature | Display path |
|-----------|-------------|
| `pub fn read(path: impl AsRef<Path>) -> Result<Vec<u8>>` | `tokio::fs::read(path)` |
| `pub fn new(x: f64, y: f64) -> Self` | `Point::new(x, y)` |
| `pub fn lock(&self) -> MutexGuard<'_, T>` | `Mutex::lock(&self)` becomes `Mutex::lock` |
| `pub fn len(&self) -> usize` | `Vec::len` (self params excluded) |
| `pub fn default() -> Self` | `Mutex::default` (no params) |

Rules for parameter extraction:
- Extract parameter names from the signature's parameter list.
- Exclude `self`, `&self`, `&mut self`.
- If remaining params are empty, show just the path (no parentheses).
- If remaining params exist, show `path(param1, param2)`.

### Column alignment

Dynamic column widths computed from the result set (see section 16).

### Empty results

```
0 results for "nonexistent":
```

Exit code 0 (empty search is not an error).

### Empty/whitespace query

Error: `search query cannot be empty` (exit code 2).

### JSON search results

See section 11 for the JSON format with `score` field.

---

## 18. Rendering Rules for Items With No Documentation

When an `IndexItem` has an empty `docs` string:
- The doc text section is omitted entirely.
- No blank line is emitted where doc text would go.
- The output goes directly from the signature to the next section (methods, children, etc.),
  or to end-of-output for leaf items.

Example (struct with no docs):
```
struct my_crate::InternalHelper

pub struct InternalHelper { pub value: u32 }

Methods:
  pub fn new(value: u32) -> Self                                   Creates a new helper.
```

Example (leaf with no docs):
```
fn my_crate::internal_helper

pub fn internal_helper(x: u32) -> u32
```

Note: signature is still rendered, followed by nothing. No trailing blank line.

---

## 19. Feature Gate Annotation

Items with a `feature_gate` value show the annotation on the header line.

### Format

```
<kind> <path>  [feature: <gate_name>]
```

Two spaces separate the path from the `[feature: ...]` annotation.

### Examples

```
mod tokio::fs  [feature: fs]

pub mod fs
```

```
fn tokio::fs::read  [feature: fs]

pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>>

Reads the entire contents of a file into a bytes vector.
```

### In list mode

The feature gate appears after the summary:

```
mod   tokio::fs   Filesystem utilities.  [feature: fs]
```

### In JSON mode

The `feature_gate` field is always present on `JsonDocItem`:
- `"feature_gate": "fs"` when a gate exists.
- `"feature_gate": null` when no gate exists.
