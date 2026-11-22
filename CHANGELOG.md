# Changelog

## Unreleased

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
