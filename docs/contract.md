# CSV and query contract

This is the behavioral contract for the unreleased 0.x line. It deliberately
specifies the existing small command set; it is not SQL and does not infer types
or normalize text.

## Input grammar

- Input is comma-separated UTF-8. The delimiter and quote character are fixed.
  A BOM, if present, is data in the first field, not an encoding marker.
- CR, LF, and CRLF terminate records outside quotes. Quoted CR, LF, and CRLF
  are preserved exactly, including a CRLF pair. `""` inside a quoted field is
  one literal quote.
- Quotes start a quoted field only at the beginning of a field. A quote in an
  unquoted field is literal, matching Python's `csv` module. After a closing
  quote, only another quote, comma, record terminator, or EOF is legal.
  Unterminated fields and trailing junk after a closing quote are errors.
- An empty physical line is an empty record and is ignored by the query engine.
  A line containing `""` is one empty field and is not ignored. CSV output uses
  LF terminators and quotes fields containing comma, quote, CR, or LF. A single
  empty output field is encoded as `""` so it remains distinguishable from an
  empty physical line. Other whitespace and text are never normalized.
- A zero-byte input, or one whose first physical line is empty, has no header
  and is an error. A header with fields but no data rows is valid. Non-header
  records must have exactly the header's field count; missing/extra fields are
  errors rather than being padded or truncated.
- Duplicate headers are accepted. Column names are matched ASCII
  case-insensitively and the first matching column wins. No trimming is applied
  to CSV column names; whitespace around names in `select` lists is trimmed.

Default limits per logical record are **1 MiB of content per field**, **16 MiB
of encoded input per record**, and **16,384 columns**. Record accounting
includes quote and delimiter bytes but excludes the record terminator. Limits
are intentionally fixed in the CLI. Library callers can select explicit limits.
Exceeding one is an error; the parser never switches to an unbounded buffer.

Malformed CSV, invalid UTF-8, invalid arguments, unknown columns/operators,
structural mismatches, numeric errors, and file/I/O errors produce a diagnostic
on stderr and exit status **2**, never a deliberate panic. Output already
streamed before a late error is not retracted. Closed stdout (EPIPE, as with
`| head`) is a clean exit **0** with no panic diagnostic.

## Commands and ordering

Stages run left to right; header resolution also follows that order. `count`
must be last and may occur once. `--markdown` may occur anywhere.

- `filter col = value` and `!=` compare as finite `f64` values when both sides
  parse as such; otherwise they compare the original strings exactly.
  `contains` is a case-sensitive substring test.
- `filter col >|>=|<|<= value` requires both operands to be finite `f64`
  values. Invalid, NaN, infinite, and overflowing numeric spellings are errors
  when an ordered comparison is evaluated; an invalid RHS is rejected up front.
- `select col[,col...]` retains/reorders/duplicates named columns as requested.
- `group-by col count` groups the exact field strings and counts rows.
  `sum|avg|min|max` use **that same column's values**, preserving the original
  0.1.0 syntax. Every value must be finite numeric. Groups are emitted in Rust
  `String`/UTF-8 byte lexicographic order, not first-seen or numeric-key order.
  There is no separate key-and-value aggregate syntax.
- `sum`/`avg` use Neumaier compensated `f64` summation in input order. This is
  not exact decimal arithmetic and rounding is still possible. Intermediate or
  final non-finite arithmetic is an error. Counts use checked `u64` arithmetic.
  Integer-looking values below `1e15` are printed without a decimal point;
  other finite `f64` values use Rust's shortest-roundtrip formatting.
- `top n col` requires finite numeric values (when `n > 0`), sorts descending
  numerically, keeps input order for ties (including `0` and `-0`), and emits
  at most `n` rows. `top 0` emits only the header.
- `count` counts rows reaching it and emits one bare decimal integer, not CSV.

Markdown output escapes pipes and renders CR/LF inside fields as `<br>`. It is
presentation output, not a CSV roundtrip format. Markdown uses a simple `---`
separator and does not buffer input to align columns.

## Streaming and memory limits

The input is never loaded as a whole string or row vector. Before a buffering
stage, memory is one logical record plus the input buffer. `top n` holds at most
`n` records and an `n`-element sorted result, `O(n log n)` work for `n > 0`.
Each grouping stage holds one state per distinct key and drains groups in sorted
order; `O(g)` memory for `g` distinct groups is unavoidable in this design.
Consequently the process is **not constant-memory** for high-cardinality groups
or a user-selected huge `n`; those sizes and normal allocator overhead must fit
in available memory. A malformed huge field cannot force unlimited allocation.

The streaming test drives 250,000 rows through filter/top under a hard 64 MiB
address-space limit. This is a regression bound for that pipeline, not a promise
that arbitrary queries fit in 64 MiB.

## Deliberate non-features

No alternate delimiters, comments, type inference, encoding conversion, joins,
SQL parser, persistence, parallel execution, or implicit numeric/string
normalization. Input records are not silently repaired or skipped (except empty
physical lines). Invalid input fails rather than being normalized.
