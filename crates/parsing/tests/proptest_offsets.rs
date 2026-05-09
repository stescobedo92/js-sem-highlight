//! Round-trip de conversiones LSP `Position` ↔ byte offset.
//!
//! Cubre el riesgo identificado en `design.md` (D6/Riesgo de UTF-16):
//! errores de conversión de offsets son la causa #1 de árboles tree-sitter
//! corruptos y son invisibles hasta que un usuario edita un archivo con
//! emojis o caracteres del plano suplementario.
//!
//! Estrategia: generar texto aleatorio con caracteres multi-byte y verificar
//! que `byte_to_lsp_position(lsp_position_to_byte(p)) == p` para cualquier
//! posición válida.

#![allow(clippy::expect_used)]

use js_sem_parsing::offset::{byte_to_lsp_position, lsp_position_to_byte};
use proptest::prelude::*;
use ropey::Rope;
use tower_lsp::lsp_types::Position;

/// Estrategia que genera fragmentos con probabilidad de caracteres exóticos.
///
/// Mezcla ASCII, BMP fuera de ASCII (`café`, kanji), y plano suplementario
/// (`🌍`, `𝄞`) para forzar el camino de UTF-16 surrogate pairs.
fn fragment_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // ASCII puro
        "[a-zA-Z0-9 \\n;()={}+]{0,40}",
        // BMP fuera de ASCII
        Just("café résumé naïve".to_string()),
        Just("日本語コード".to_string()),
        Just("κόσμε".to_string()),
        // Plano suplementario (cada char = 2 unidades UTF-16)
        Just("🌍".to_string()),
        Just("🚀🔥💯".to_string()),
        Just("𝄞".to_string()),
        // Mixto en una sola línea
        Just("const x = '🌍'; // café\n".to_string()),
        // Saltos de línea variados
        Just("\n\n\n".to_string()),
        Just("a\nb\r\nc".to_string()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 200,
        ..ProptestConfig::default()
    })]

    /// Para cada offset de byte válido en el rope, convertir a LSP y de vuelta
    /// debe producir el mismo offset.
    #[test]
    fn round_trip_byte_lsp_byte(fragments in proptest::collection::vec(fragment_strategy(), 1..6)) {
        let text: String = fragments.concat();
        let rope = Rope::from_str(&text);

        // Sample 32 byte offsets repartidos por el rope (incluye 0 y len).
        let len_bytes = rope.len_bytes();
        let samples = sample_offsets(len_bytes);

        for byte in samples {
            // El offset puede caer en medio de un char multi-byte; en ese caso
            // saltamos al inicio del char anterior, que es el invariante real.
            let safe_byte = align_to_char_boundary(&rope, byte);

            let pos = byte_to_lsp_position(&rope, safe_byte)
                .expect("byte_to_lsp_position should succeed for valid byte offsets");
            let recovered = lsp_position_to_byte(&rope, pos)
                .expect("lsp_position_to_byte should succeed for positions we just emitted");

            prop_assert_eq!(
                recovered, safe_byte,
                "round-trip mismatch: byte={} pos={:?}",
                safe_byte, pos
            );
        }
    }

    /// `byte_to_lsp_position` debe producir `Position`s monotónicamente
    /// crecientes para offsets crecientes.
    #[test]
    fn monotonic_positions(fragments in proptest::collection::vec(fragment_strategy(), 1..6)) {
        let text: String = fragments.concat();
        let rope = Rope::from_str(&text);

        let len_bytes = rope.len_bytes();
        let mut prev: Option<Position> = None;

        for byte in sample_offsets(len_bytes) {
            let safe = align_to_char_boundary(&rope, byte);
            let pos = byte_to_lsp_position(&rope, safe)
                .expect("byte_to_lsp_position");
            if let Some(p) = prev {
                prop_assert!(
                    (pos.line, pos.character) >= (p.line, p.character),
                    "non-monotonic at byte {}: {:?} < {:?}",
                    safe, pos, p
                );
            }
            prev = Some(pos);
        }
    }
}

/// Genera 32 offsets sample distribuidos sobre `[0, len_bytes]`.
fn sample_offsets(len: usize) -> Vec<usize> {
    if len == 0 {
        return vec![0];
    }
    (0..32)
        .map(|i| (len * i) / 31)
        .chain(std::iter::once(len))
        .collect()
}

/// Devuelve el byte offset más cercano (≤ `byte`) que esté en frontera de char.
/// Necesario porque `proptest` puede generar offsets que caen en medio de un
/// char UTF-8 multi-byte, lo que no es un input válido para nuestra API.
fn align_to_char_boundary(rope: &Rope, byte: usize) -> usize {
    if byte >= rope.len_bytes() {
        return rope.len_bytes();
    }
    // `byte_to_char` redondea hacia abajo automáticamente, así que basta con
    // ir char → byte de vuelta.
    let char_idx = rope.byte_to_char(byte);
    rope.char_to_byte(char_idx)
}
