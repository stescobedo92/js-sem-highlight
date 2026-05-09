//! Benchmarks de `compute_semantic_tokens` por tamaño de archivo.
//!
//! Targets de design.md: p50 < 8 ms en 5k LOC. Estos benchmarks corren
//! con `cargo bench -p js-sem-lsp` y producen reportes HTML en
//! `target/criterion/`.

#![allow(clippy::expect_used)]

use std::fmt::Write as _;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use js_sem_parsing::{Document, Language};
use js_sem_scopes::{analyze, CancellationToken};

const LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// Genera un fixture sintético JS de aproximadamente `n_lines` líneas.
///
/// El generador mezcla declaraciones (`const`/`let`/`function`) y llamadas
/// para activar todos los caminos del pipeline (parsing + scopes + reglas).
fn fixture_js(n_lines: usize) -> String {
    let mut out = String::with_capacity(n_lines * 60);
    for i in 0..n_lines {
        match i % 5 {
            0 => {
                let _ = writeln!(out, "const k_{i} = {i};");
            }
            1 => {
                let _ = writeln!(out, "let v_{i} = k_{} + 1;", i.saturating_sub(1));
            }
            2 => {
                let _ = writeln!(out, "function f_{i}(a, b) {{ return a + b + v_{i}; }}");
            }
            3 => {
                let _ = writeln!(out, "const r_{i} = f_{i}(k_{i}, v_{i});");
            }
            _ => {
                let _ = writeln!(out, "console.log(r_{}, k_{});", i.saturating_sub(1), i);
            }
        }
    }
    out
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_full");
    for &lines in &[500usize, 2_000, 5_000, 10_000] {
        let source = fixture_js(lines);
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(lines), &source, |b, source| {
            b.iter(|| {
                let doc = Document::open(Language::JavaScript, 1, black_box(source), LIMIT_BYTES)
                    .expect("open");
                black_box(doc);
            });
        });
    }
    group.finish();
}

fn bench_analyze(c: &mut Criterion) {
    let mut group = c.benchmark_group("analyze_scopes");
    let token = CancellationToken::new();
    for &lines in &[500usize, 2_000, 5_000, 10_000] {
        let source = fixture_js(lines);
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(lines), &source, |b, source| {
            b.iter(|| {
                let map =
                    analyze(black_box(source), Language::JavaScript, &token).expect("analyze");
                black_box(map);
            });
        });
    }
    group.finish();
}

fn bench_incremental_edit(c: &mut Criterion) {
    use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent};

    let source = fixture_js(5_000);
    let mut group = c.benchmark_group("incremental_edit");
    group.throughput(Throughput::Bytes(source.len() as u64));
    group.bench_function(BenchmarkId::from_parameter("5k_lines_one_char"), |b| {
        b.iter(|| {
            let mut doc =
                Document::open(Language::JavaScript, 1, &source, LIMIT_BYTES).expect("open");
            // Edit barato: insertar un espacio en línea 100 col 0.
            let change = TextDocumentContentChangeEvent {
                range: Some(Range {
                    start: Position {
                        line: 100,
                        character: 0,
                    },
                    end: Position {
                        line: 100,
                        character: 0,
                    },
                }),
                range_length: None,
                text: " ".into(),
            };
            doc.apply_changes(2, &[change]).expect("apply");
            black_box(doc);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_parse, bench_analyze, bench_incremental_edit);
criterion_main!(benches);
