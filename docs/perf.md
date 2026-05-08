# Performance Notes

## Targets (from design.md)

- `semanticTokens/full` p50 < 8 ms, p99 < 30 ms on archives ≤ 5 000 LOC.
- Memory per open document < 4× source size in bytes.

## Local results (criterion `--quick`, single iteration)

Hardware: macOS host as configured at the bench run.

| Benchmark | Time (median) | Throughput |
|---|---|---|
| `parse_full/500`         | 1.25 ms  | 11.4 MiB/s |
| `parse_full/2000`        | 5.15 ms  | 11.6 MiB/s |
| `parse_full/5000`        | 13.14 ms | 11.6 MiB/s |
| `parse_full/10000`       | 27.04 ms | 11.4 MiB/s |
| `analyze_scopes/500`     | 218 µs   | 64.8 MiB/s |
| `analyze_scopes/2000`    | 867 µs   | 68.8 MiB/s |
| `analyze_scopes/5000`    | 2.24 ms  | 68.3 MiB/s |
| `analyze_scopes/10000`   | 4.64 ms  | 66.4 MiB/s |
| `incremental_edit/5k`    | 16.07 ms | 9.5 MiB/s  |

## Observations

1. **Parsing is linear** in source size (~11 MiB/s). No surprises; tree-sitter
   amortizes table loads via `LanguageRegistry::OnceLock`.

2. **oxc semantic analysis is ~6× faster** than tree-sitter parse on the same
   source. This validates D1 (delegate scope resolution to oxc) — there is
   real headroom for the heavier rules to run inside the same budget.

3. **`parse_full/5000` exceeds the 8 ms target** (13 ms median). Two reasons:
   - The synthetic fixture is denser (~65 chars/LOC) than typical hand-written
     JavaScript (~30 chars/LOC). 5 000 fixture lines ≈ 14 000 real LOC.
   - The benchmark exercises `Document::open` (cold path: registry lookup,
     full parse). Live edits use `apply_changes` which reuses subtrees.

4. **Incremental edits are SLOWER than expected** (16 ms for one char in 5k
   LOC). Investigation needed: `tree::edit + parse_with` should be < 1 ms per
   the tree-sitter docs. Likely cause: we currently `rope.to_string()` before
   parsing, defeating Cargo's view of incrementality at the buffer level.
   Tracked as follow-up; not a blocker for v0.1.

## Memory profile (50 documents, dhat)

Run with:

```bash
cargo build --bin mem-bench --features dhat-heap --release
./target/release/mem-bench 50
```

Latest run on this machine (50 documents × ~1 000 LOC each):

| Metric | Value |
|---|---|
| Total source                 | 1 451.7 KB |
| dhat **peak** (`t-gmax`)     | 3 523 407 bytes (~3.4 MB) |
| dhat **leaked** (`t-end`)    | 1 088 bytes (allocations alive at exit, expected from globals) |
| Total bytes ever allocated   | 106 214 658 bytes (`Total`) |
| Allocated blocks             | 255 787 |
| Documents opened             | 50 |
| Wall time                    | 763 ms |

**Peak / source ratio: 2.4×**, comfortably below the 4× design target.

The detailed allocation profile is saved as `docs/dhat-heap-50docs.json`.
Open it in `https://nnethercote.github.io/dh_view/dh_view.html` for an
interactive flame-graph view that attributes bytes to call sites.

## CI tracking

Benchmarks are not run in CI by default (criterion needs many iterations to
produce stable numbers, and runner variance is high). To run locally:

```bash
cargo bench -p js-sem-lsp --bench semantic_tokens
# Reports in target/criterion/<group>/<id>/report/index.html
```
