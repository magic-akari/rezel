# Parser instruction benchmarks

This crate compares Rezel with the official Tree-sitter runtime on pinned,
real-world source datasets. The benchmark parses every listed file separately
and reuses one parser instance for the complete dataset.

PHP uses the package's default `Template` entry point with pinned WordPress 7.1
sources, preserving opening tags and any interleaved non-PHP text. Every Rezel
dataset is accepted once by a strict parser during setup; the measured region
then uses the ordinary recovering parser, matching the public default without
letting recovery hide an invalid benchmark input.

Restore the ignored source files and run the Gungraun benchmark with:

```sh
mise run benchmark:datasets
mise run benchmark:gungraun
```

During corpus work, one language can be restored and validated independently:

```sh
cargo run --locked --manifest-path tools/benchmarks/parser/Cargo.toml \
  --bin restore-datasets -- --language php
```

Run the complete comparison with:

```sh
mise run benchmark:compare
```

This command creates a named baseline from `HEAD^` with Rezel, measures every
backend registered in `backends.toml` at `HEAD`, and writes two reports from
the shared results:

- the current Rezel implementation against every current comparison backend;
- the current Rezel implementation against Rezel at `HEAD^`.

Use `--head` and `--base` to select other committed revisions:

```sh
mise run benchmark:compare -- --head HEAD --base main
```

Both revisions use the benchmark harness, dependencies, dataset manifest, and
file order from the selected head revision. The command archives committed Git
revisions into temporary directories, restores that head revision's datasets,
and does not switch the working tree. The bare repository cache is reused;
network access is needed when a pinned commit is not cached yet.
Generated reports, normalized JSON, logs, and raw Gungraun data are written to
`target/comparisons` and remain ignored by Git.

The first command needs Git and network access. It stores bare repositories in
`datasets/git-cache` and exports only the files listed in `datasets.toml` to
`datasets/sources`. Both directories are ignored by Git.

Gungraun requires Linux, Valgrind, and `gungraun-runner`. Dataset restoration,
file I/O, parser construction, and setup parses run outside the measured
function. The measured region contains one full recovering parse per file,
with no incremental syntax tree.

Additional Rust implementations can be added as backend modules. Native or
non-Rust implementations can share `datasets.toml` through a Gungraun binary
benchmark and register their stable benchmark identity in `backends.toml`.
Only parse-only measurements should be registered for horizontal comparison;
whole-process measurements include startup costs and are not interchangeable.
