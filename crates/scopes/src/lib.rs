//! Resolución de scopes y clasificación de identificadores.
//!
//! Construye un `ScopeMap` desde una fuente JS/TS/JSX/TSX usando `oxc_parser`
//! + `oxc_semantic` y traduce los símbolos/referencias de oxc a un modelo
//! `IdentifierRole` consumido por el resto del servidor.
//!
//! Cubre el contrato de `specs/scope-resolver/spec.md`.

mod cancellation;
mod globals;
mod role;
mod scope_map;

pub use cancellation::{CancellationToken, Cancelled};
pub use globals::{is_default_library, DEFAULT_LIBRARY_GLOBALS};
pub use role::{ClassifiedReference, IdentifierBinding, IdentifierRole};
pub use scope_map::{analyze, AnalyzeError, ScopeMap};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::missing_const_for_fn, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_lossless)]
    use super::*;

    #[test]
    fn known_globals_resolve() {
        assert!(is_default_library("console"));
        assert!(is_default_library("Math"));
        assert!(is_default_library("setTimeout"));
    }

    #[test]
    fn unknown_globals_dont_match() {
        assert!(!is_default_library("gtag"));
        assert!(!is_default_library("$"));
        assert!(!is_default_library(""));
    }
}
