//! Token de cancelación cooperativa para análisis largos.
//!
//! El servidor LSP puede invalidar un análisis en curso cuando llega una nueva
//! edición. Queremos parar en el siguiente "punto de yield" (cada 200 nodos
//! visitados, según `scope-resolver/spec.md`) sin abortar el thread.
//!
//! Diseño: un `AtomicBool` compartido por `Arc`. El productor (servidor LSP)
//! llama a `cancel()`. El consumidor (visitor de scopes) chequea
//! `is_cancelled()` cada N nodos y hace `bail!` si está activo.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use thiserror::Error;

#[derive(Debug, Error)]
#[error("analysis was cancelled")]
pub struct Cancelled;

/// Token de cancelación clonable.
///
/// Clonar el token NO crea una nueva señal: todos los clones comparten el
/// mismo `AtomicBool` (`Arc`). Esto es deliberado — el productor y el
/// consumidor usan instancias clonadas del mismo token.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Crea un nuevo token NO cancelado.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marca el token como cancelado. Idempotente.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// Indica si `cancel()` fue invocado en este token o en algún clon.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    /// Conveniente: `Err(Cancelled)` si está cancelado, `Ok(())` en otro caso.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::missing_const_for_fn,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_lossless
    )]
    use super::*;

    #[test]
    fn fresh_token_is_not_cancelled() {
        let t = CancellationToken::new();
        assert!(!t.is_cancelled());
        assert!(t.check().is_ok());
    }

    #[test]
    fn cancel_propagates_to_clones() {
        let a = CancellationToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
        assert!(b.check().is_err());
    }

    #[test]
    fn cancel_is_idempotent() {
        let t = CancellationToken::new();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }
}
