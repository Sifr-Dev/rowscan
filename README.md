<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/banner-dark.svg">
    <img alt="sifr-query — built from zero" src=".github/banner-light.svg" width="100%">
  </picture>
</p>

# sifr-query

zero-dependency streaming CSV query engine in ~350 lines of Rust. filter,
select, group-by aggregates, top-N — stdin or file, CSV or Markdown out.
no pandas, no polars, no crates.

## usage

```sh
sifr-query data.csv filter country = ke            # rows matching an op
sifr-query data.csv group-by plan count            # aggregates: count|sum|avg|min|max
sifr-query data.csv top 10 age                     # 10 largest by a numeric column
sifr-query data.csv filter plan = pro select country,age
sifr-query data.csv filter country = ke group-by plan count --markdown
cat data.csv | sifr-query count                    # stdin works too
```

operators: `=`, `!=`, `>`, `>=`, `<`, `<=`, `contains` (numeric when both
sides parse as f64, string compare for `=`/`!=`/`contains` otherwise).

## honest scope

- aggregates apply to the numeric values **in the group-by column itself**
  (e.g. `group-by age avg` works; `group-by country avg` prints 0 — country
  is not numeric). `count` is always valid.
- `top` sorts by a numeric column.
- one file or stdin; no joins, no SQL parser, no persistence.
- duplicate-header CSVs are not validated; quoted fields with embedded
  newlines are handled (CRLF too).

## evidence (this machine, 2026-11-17)

```console
$ ./sifr-query /tmp/opencode/bench_100k.csv filter plan = pro count
33296
real 0m0.080s   # 100k rows, user ≈ 0.05s — ~2M rows/s single-thread
$ cargo test --release
test result: ok. 4 passed; 0 failed
```

## build

```sh
cargo build --release    # binary: target/release/sifr-query
cargo test --release     # 4 parser/compare/escape/column tests
```

## license

`MIT` — see `LICENSE`. built from zero.
