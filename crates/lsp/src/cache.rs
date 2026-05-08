//! Cache de semantic tokens por URI para soportar `semanticTokens/full/delta`.
//!
//! LSP 3.17 permite al cliente pedir solo el delta entre dos versiones del
//! mismo documento, identificadas por `resultId`. Mantenemos el último set
//! emitido por documento + un contador monotónico para generar ids únicos.

use std::sync::atomic::{AtomicU64, Ordering};

use tower_lsp::lsp_types::SemanticToken;

/// Estado cacheado del último set de semantic tokens emitido para un documento.
#[derive(Debug, Clone)]
pub struct CachedTokenSet {
    pub result_id: String,
    pub tokens: Vec<SemanticToken>,
}

/// Generador monotónico de `resultId`s.
///
/// Compartido por todos los documentos: un counter global garantiza que dos
/// documentos nunca emitan el mismo id, lo cual simplifica la lógica de
/// invalidación en el cliente.
#[derive(Debug, Default)]
pub struct ResultIdGenerator {
    next: AtomicU64,
}

impl ResultIdGenerator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&self) -> String {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        format!("rid-{id}")
    }
}

/// Calcula el delta entre dos sets de semantic tokens en formato LSP.
///
/// La codificación LSP es delta-relativa, así que para producir un
/// `SemanticTokensDelta` decodificamos los enteros, encontramos el rango
/// modificado, y emitimos un único `SemanticTokensEdit` que reemplaza ese
/// rango. Es una aproximación conservadora pero correcta: el cliente recibe
/// menos data que un set completo en ediciones puntuales.
#[must_use]
pub fn compute_delta_edit(
    previous: &[SemanticToken],
    current: &[SemanticToken],
) -> Option<tower_lsp::lsp_types::SemanticTokensEdit> {
    use tower_lsp::lsp_types::SemanticTokensEdit;

    // Encuentra prefijo común.
    let common_prefix = previous
        .iter()
        .zip(current.iter())
        .take_while(|(a, b)| a == b)
        .count();
    // Encuentra sufijo común (sobre las porciones restantes).
    let prev_remaining = &previous[common_prefix..];
    let curr_remaining = &current[common_prefix..];
    let common_suffix = prev_remaining
        .iter()
        .rev()
        .zip(curr_remaining.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let prev_changed = &prev_remaining[..prev_remaining.len() - common_suffix];
    let curr_changed = &curr_remaining[..curr_remaining.len() - common_suffix];

    if prev_changed.is_empty() && curr_changed.is_empty() {
        return None;
    }

    // El campo `start` y `delete_count` están en unidades de "u32" (5 ints
    // por token). El campo `data` es la lista de SemanticToken nueva.
    let start_u32 = u32::try_from(common_prefix * 5).ok()?;
    let delete_count_u32 = u32::try_from(prev_changed.len() * 5).ok()?;

    Some(SemanticTokensEdit {
        start: start_u32,
        delete_count: delete_count_u32,
        data: Some(curr_changed.to_vec()),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::missing_const_for_fn, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_lossless)]
    use super::*;

    fn token(delta_line: u32, delta_start: u32, length: u32) -> SemanticToken {
        SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: 0,
            token_modifiers_bitset: 0,
        }
    }

    #[test]
    fn ids_are_unique() {
        let gen = ResultIdGenerator::new();
        let a = gen.next_id();
        let b = gen.next_id();
        assert_ne!(a, b);
    }

    #[test]
    fn compute_delta_no_change_returns_none() {
        let prev = vec![token(0, 0, 5)];
        let curr = vec![token(0, 0, 5)];
        assert!(compute_delta_edit(&prev, &curr).is_none());
    }

    #[test]
    fn compute_delta_appended_token() {
        let prev = vec![token(0, 0, 5)];
        let curr = vec![token(0, 0, 5), token(0, 6, 1)];
        let edit = compute_delta_edit(&prev, &curr).expect("delta exists");
        assert_eq!(edit.start, 5); // un token = 5 u32
        assert_eq!(edit.delete_count, 0);
        assert_eq!(edit.data.unwrap_or_default().len(), 1);
    }

    #[test]
    fn compute_delta_modified_middle() {
        let prev = vec![token(0, 0, 5), token(0, 6, 1), token(0, 8, 3)];
        let curr = vec![token(0, 0, 5), token(0, 6, 2), token(0, 8, 3)];
        let edit = compute_delta_edit(&prev, &curr).expect("delta exists");
        // Cambio en el token del medio, cuyo offset es 5 (1 token).
        assert_eq!(edit.start, 5);
        assert_eq!(edit.delete_count, 5);
        assert_eq!(edit.data.unwrap_or_default().len(), 1);
    }
}
