<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/banner-dark.svg">
    <img alt="sifr-query — built from zero" src=".github/banner-light.svg" width="100%">
  </picture>
</p>

# sifr-query

zero-dependency streaming CSV query engine in Rust. filter, select, group-by
aggregates, top-N — stdin or file, CSV or Markdown out. no pandas, no polars,
no production crates.

## usage

```sh
sifr-query data.csv filter country = ke            # rows matching an op
sifr-query data.csv group-by plan count            # aggregates: count|sum|avg|min|max
sifr-query data.csv top 10 age                     # 10 largest by a numeric column
sifr-query data.csv filter plan = pro select country,age
sifr-query data.csv filter country = ke group-by plan count --markdown
cat data.csv | sifr-query count                    # stdin works too
cat data.csv | sifr-query | head                   # closed stdout is not a panic
```

operators: `=`, `!=`, `>`, `>=`, `<`, `<=`, `contains`. `=`/`!=` use numeric
comparison when both sides are finite `f64`, exact strings otherwise. Ordered
operators require finite numbers. `contains` is an exact, case-sensitive
substring check. Stages compose left to right; `count` is terminal.

## honest scope and contract

- Aggregates apply to the numeric values **in the group-by column itself**
  (e.g. `group-by age avg` works). `group-by country avg` is an error, not a
  zero aggregate. `count` works for any key. Groups are sorted by exact key.
- `top` is descending numeric order, stable for ties, and retains at most N rows.
  Invalid/NaN/infinite numbers and aggregate overflow are explicit errors.
- Quoted commas, quotes, CR, LF, and CRLF are preserved. Strict malformed CSV,
  invalid UTF-8, ragged rows, empty input, and invalid CLI arguments exit 2.
  Empty physical lines are ignored; no field normalization or row repair occurs.
- Streaming uses one record for non-buffering stages. Memory grows with distinct
  groups or requested top N, not with total input rows. Defaults cap each field
  at 1 MiB, each logical record at 16 MiB, and each record at 16,384 columns.
- One file or stdin; no joins, SQL parser, persistence, delimiter inference,
  type inference, or encoding conversion. Duplicate headers use the first match.

The full grammar, numeric/order rules, errors, memory caveats, and output
contract are in [docs/contract.md](docs/contract.md). The 0.x compatibility and
release rules are in [docs/development.md](docs/development.md). No normalization,
automatic merge, or automatic branch deletion is part of this change.

## build and verify

```sh
cargo build --release                  # binary: target/release/sifr-query
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --release --all-targets
```

Python 3 is needed for the independent CSV differential oracle and Unix memory
limit regression. The suite includes malformed/empty/ragged/numeric/overflow
and broken-pipe CLI tests, 700 randomized parser fixtures, 100 randomized
filter/top/count/sum fixtures, and 5,000 arbitrary-byte roundtrip cases.

A separate [cargo-fuzz target](fuzz/fuzz_targets/csv_parser.rs) provides
coverage-guided ASan fuzzing on nightly Rust:

```sh
rustup toolchain install nightly --component rust-src
cargo install cargo-fuzz --locked
cargo +nightly fuzz run csv_parser --sanitizer address -- \
  -max_total_time=30 -print_final_stats=1
```

See [verification and release policy](docs/development.md) for CI, sanitizer
requirements, and local-vs-CI evidence. Version changes are intentional release
actions; this branch leaves the package at 0.1.0.

## license

`MIT` — see `LICENSE`. built from zero.
