# Development, verification, and 0.x releases

## Required checks

The production crate has no third-party dependencies. Python 3 is required for
the differential and hard-memory CLI tests; no Python packages are required.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --release --all-targets
```

`tests/differential.py` uses Python's strict `csv.reader` and `csv.writer` as an
independent oracle for 350 arbitrary-ASCII and 250 structured UTF-8 fixtures,
then 100 filter/top/count/sum fixtures. Its seed is fixed; failed fixture labels,
input, output, and expectations are printed. `tests/fuzz_regression.rs` drives
5,000 deterministic arbitrary-byte inputs through tiny buffers and verifies that
accepted records survive writer/parser roundtrips.

The Unix-only CLI tests use a real closed pipe to assert EPIPE exits 0, and a
250,000-row pipeline under `RLIMIT_AS=64 MiB` to reject whole-input buffering.
Other platforms still run the remaining tests; these OS-specific regression
checks require Unix/Python as shown in CI.

## Fuzzing and sanitizers

`fuzz/` is a separate `cargo-fuzz` crate; its `libfuzzer-sys` dependency is not a
production dependency. The target uses small per-field/record limits and checks
both error freedom and writer/parser roundtrips.

Install the tools on a nightly toolchain and run a bounded smoke fuzz:

```sh
rustup toolchain install nightly --component rust-src
cargo install cargo-fuzz --locked
cargo +nightly fuzz run csv_parser --sanitizer address -- \
  -max_total_time=30 -print_final_stats=1
```

`cargo-fuzz` currently enables `-Zsanitizer` and `-Zbuild-std`; therefore the
ASan coverage-guided run requires nightly Rust and rust-src. Stable Rust cannot
execute that exact ASan configuration. CI runs the smoke fuzz only on the
manually dispatched or scheduled `fuzz` job, not on every stable test build.
Generated corpora, artifacts, and fuzz build output are intentionally ignored;
any discovered crash must first be minimized and committed as a deterministic
regression. This branch's host had stable Rust only, so its local ASan run was
not available; a Valgrind memcheck run supplements (not replaces) that CI job.

## Continuous integration

Pull requests and pushes to `master` run formatting, locked Clippy, debug and
release tests, and documentation on stable Rust with Python 3. The optional
nightly ASan fuzz smoke runs weekly and by manual dispatch. The workflow only
tests and reports; it never commits, merges, deletes branches, normalizes the
tree, or publishes a release.

## 0.x release policy

- This project is pre-1.0. The public contract includes the CLI arguments,
  stdout/stderr behavior, exit codes, and documented ordering/limits.
- **0.MINOR.PATCH:** a minor bump may intentionally change that contract; any
  such change must be explicit in `CHANGELOG.md` and this documentation. A
  patch bump is for compatible bug fixes and tests, with no intentional
  contract change. Security fixes may require a minor bump if the compatible
  behavior itself is unsafe.
- A release requires a clean, reviewed branch, all required CI checks green,
  and a bounded ASan fuzz smoke result. The changelog must identify the exact
  version and commit. Do not release directly from an unreviewed feature branch.
- A maintainer deliberately updates the version/changelog and creates the tag;
  feature work does not silently bump versions, merge its PR, delete its source
  branch, or publish artifacts. Review and merge are separate human actions.
- No default branch normalization or automatic merge/delete policy is enabled
  by this repository's CI.
