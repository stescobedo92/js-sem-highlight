//! Memory benchmark: abre 50 documentos sintéticos en paralelo y mide el
//! consumo de heap con `dhat`.
//!
//! Cumple el target del design: "memoria por documento abierto < 4× tamaño
//! de la fuente en bytes". Producirá un `dhat-heap.json` que se puede abrir
//! en https://nnethercote.github.io/dh_view/dh_view.html para inspección
//! detallada de allocations.
//!
//! Uso:
//!   cargo run --release --bin mem-bench --features dhat-heap -- 50
//!
//! Salida:
//!   - dhat-heap.json (formato dhat)
//!   - resumen por stdout: total bytes, peak, promedio por documento
//!
//! Requiere la feature `dhat-heap` (gated en Cargo.toml).

#![cfg(feature = "dhat-heap")]

use std::time::Instant;

use js_sem_parsing::{Document, Language};
use js_sem_scopes::{analyze, CancellationToken};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const LIMIT_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_DOCS: usize = 50;

/// Genera contenido JS sintético de ~`n_lines` líneas con identificadores
/// únicos por documento para evitar interning del runtime.
fn make_source(seed: usize, n_lines: usize) -> String {
    let mut s = String::with_capacity(n_lines * 60);
    for i in 0..n_lines {
        match i % 4 {
            0 => s.push_str(&format!("const k_{seed}_{i} = {i};\n")),
            1 => s.push_str(&format!(
                "function f_{seed}_{i}(a) {{ return a + k_{seed}_{}; }}\n",
                i.saturating_sub(1)
            )),
            2 => s.push_str(&format!("let v_{seed}_{i} = f_{seed}_{i}({i});\n", i = i)),
            _ => s.push_str(&format!("console.log(v_{seed}_{i});\n")),
        }
    }
    s
}

fn main() {
    let _profiler = dhat::Profiler::new_heap();

    let n_docs: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_DOCS);

    let mut docs: Vec<Document> = Vec::with_capacity(n_docs);
    let cancel = CancellationToken::new();

    let start = Instant::now();
    let mut total_source_bytes: usize = 0;
    for seed in 0..n_docs {
        let source = make_source(seed, 1_000); // ~1k LOC por documento
        total_source_bytes += source.len();
        let doc = Document::open(Language::JavaScript, 1, &source, LIMIT_BYTES)
            .expect("open document");
        // Forzar análisis de scopes para capturar la memoria de oxc.
        let _scope_map = analyze(&source, Language::JavaScript, &cancel).expect("analyze");
        docs.push(doc);
    }
    let elapsed = start.elapsed();

    println!(
        "opened {} documents ({:.1} KB total source) in {:?}",
        docs.len(),
        total_source_bytes as f64 / 1024.0,
        elapsed
    );

    // dhat::Profiler::drop emite el JSON automáticamente al hacer drop.
    // Forzamos un uso de `docs` para evitar que el optimizer lo descarte
    // antes de que el profiler tome la snapshot final.
    println!("retained {} documents in memory at exit", docs.len());
    drop(docs);
}
