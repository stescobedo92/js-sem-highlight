//! Contexto y emisiones del framework de reglas.

use js_sem_scopes::ScopeMap;
use tower_lsp::lsp_types::{DiagnosticSeverity, DiagnosticTag, Range};

/// Información que recibe una regla durante `check`.
///
/// Es read-only: las reglas no mutan el contexto.
pub struct AnalysisContext<'a> {
    /// Texto fuente completo del documento.
    pub source: &'a str,
    /// Resultado del análisis semántico (puede ser `None` si los scopes
    /// todavía no terminaron — algunas reglas degradan en ese caso).
    pub scope_map: Option<&'a ScopeMap>,
    /// Nombre del archivo (sin path), usado en mensajes de diagnóstico.
    pub filename: &'a str,
}

/// Modificador de token que una regla puede sumar al bitmask LSP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenModifier {
    Declaration,
    Definition,
    Readonly,
    Static,
    Deprecated,
    Abstract,
    Async,
    Modification,
    Documentation,
    DefaultLibrary,
    Unused,
}

/// Diagnóstico emitido por una regla, sin la severidad final (la decide el
/// runtime aplicando `SeverityConfig`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDiagnostic {
    /// Rango LSP del diagnóstico.
    pub range: Range,
    /// Severidad sugerida (puede ser sobreescrita por `SeverityConfig`).
    pub severity: DiagnosticSeverity,
    /// Tags LSP estándar (`Unnecessary`, `Deprecated`).
    pub tags: Vec<DiagnosticTag>,
    /// Mensaje legible para el usuario.
    pub message: String,
    /// Código de regla, idéntico al `id()` de la regla emisora.
    pub code: String,
}

/// Una emisión: token modifier o diagnóstico.
#[derive(Debug, Clone)]
pub enum RuleEmission {
    TokenModifier {
        range: Range,
        modifier: TokenModifier,
    },
    Diagnostic(RuleDiagnostic),
}

/// Deduplica diagnósticos exactamente iguales (mismo range, severity, code,
/// message), conservando el primero por orden de aparición.
///
/// Cumple Requirement: "Diagnósticos exactamente duplicados" del spec.
/// Los token modifiers NO se deduplican (son aditivos via bitmask).
#[must_use]
pub fn dedupe_emissions(emissions: Vec<RuleEmission>) -> Vec<RuleEmission> {
    /// Tupla "Hash-able" derivada del `RuleDiagnostic`. Range y
    /// `DiagnosticSeverity` de `tower_lsp` no son `Hash`, así que los aplastamos
    /// a primitivos.
    type Key = (u32, u32, u32, u32, i32, String, String);

    fn key_of(d: &RuleDiagnostic) -> Key {
        // DiagnosticSeverity tiene 4 variantes (ERROR, WARNING, INFO, HINT).
        // No es primitivo, pero implementa Eq y podemos derivar un i32 con
        // una tabla simple en lugar de `as`.
        let sev: i32 = match d.severity {
            DiagnosticSeverity::ERROR => 1,
            DiagnosticSeverity::WARNING => 2,
            DiagnosticSeverity::INFORMATION => 3,
            DiagnosticSeverity::HINT => 4,
            _ => 0,
        };
        (
            d.range.start.line,
            d.range.start.character,
            d.range.end.line,
            d.range.end.character,
            sev,
            d.code.clone(),
            d.message.clone(),
        )
    }

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(emissions.len());
    for emission in emissions {
        match &emission {
            RuleEmission::Diagnostic(d) => {
                if seen.insert(key_of(d)) {
                    out.push(emission);
                }
            }
            RuleEmission::TokenModifier { .. } => out.push(emission),
        }
    }
    out
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
    use tower_lsp::lsp_types::Position;

    fn diag(code: &str, msg: &str) -> RuleEmission {
        RuleEmission::Diagnostic(RuleDiagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            severity: DiagnosticSeverity::HINT,
            tags: vec![],
            message: msg.to_string(),
            code: code.to_string(),
        })
    }

    #[test]
    fn dedupe_removes_duplicate_diagnostics() {
        let input = vec![diag("r1", "x"), diag("r1", "x"), diag("r2", "y")];
        let out = dedupe_emissions(input);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn dedupe_preserves_token_modifiers() {
        let modifier = RuleEmission::TokenModifier {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            modifier: TokenModifier::Unused,
        };
        let input = vec![modifier.clone(), modifier];
        let out = dedupe_emissions(input);
        assert_eq!(out.len(), 2, "token modifiers are additive, not deduped");
    }
}
