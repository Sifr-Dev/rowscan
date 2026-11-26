# Changelog

## Unreleased

- Fix Markdown table corruption for fields containing a backslash: the writer
  escaped `|` without escaping the backslash that precedes it, so a field such as
  `c\|d` emitted `\\|`, which a GFM renderer decodes as a literal backslash plus a
  real cell delimiter. That split one field into two columns and shifted every
  following cell in the row. Backslashes are now escaped before pipes.
- Add a CommonMark-decoder CLI regression, an escaping unit test, a
  backslash/pipe/newline roundtrip corpus test, and 150 randomized Markdown
  fixtures in the Python differential oracle (which previously never exercised
  `--markdown`).
- Replace whole-input CSV parsing and query materialization with incremental records,
  streaming stages, sorted distinct-key aggregation, and bounded top-N heaps.
- Preserve quoted delimiters and CR/LF/CRLF exactly; reject malformed quotes, invalid
  UTF-8, ragged rows, non-finite numeric inputs, and arithmetic overflow with exit 2.
- Make `top` deterministic and stable for numeric ties, including signed zero.
- Handle closed stdout as success instead of panicking.
- Add Python CSV differential tests, deterministic arbitrary-byte roundtrips, CLI
  failure tests, a libFuzzer target, and a hard address-space streaming regression.
- Document the CSV/query contract, memory limits, CI, and 0.x release policy.

No version has been released from this branch; `Cargo.toml` remains at 0.1.0.
