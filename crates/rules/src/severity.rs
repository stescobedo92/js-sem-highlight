//! Severidad de regla y configuración.

use std::collections::HashMap;

use serde::Deserialize;
use tower_lsp::lsp_types::DiagnosticSeverity;

/// Severidad configurable por el usuario para una regla individual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Off,
    Hint,
    Warning,
    Error,
}

impl Default for RuleSeverity {
    fn default() -> Self {
        Self::Hint
    }
}

impl RuleSeverity {
    /// Convierte a la severidad LSP. `None` si la regla está apagada.
    #[must_use]
    pub const fn to_diagnostic_severity(self) -> Option<DiagnosticSeverity> {
        match self {
            Self::Off => None,
            Self::Hint => Some(DiagnosticSeverity::HINT),
            Self::Warning => Some(DiagnosticSeverity::WARNING),
            Self::Error => Some(DiagnosticSeverity::ERROR),
        }
    }

    /// `true` si la regla está apagada.
    #[must_use]
    pub const fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }
}

/// Mapa rule-id → severidad provista por el usuario en `initializationOptions`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct SeverityConfig {
    map: HashMap<String, RuleSeverity>,
}

impl SeverityConfig {
    /// Crea una configuración vacía (todas las reglas usan su `default_severity`).
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Devuelve la severidad efectiva para una regla, cayendo a `default` si
    /// el usuario no override-ó.
    #[must_use]
    pub fn effective(&self, rule_id: &str, default: RuleSeverity) -> RuleSeverity {
        self.map.get(rule_id).copied().unwrap_or(default)
    }

    /// Setter útil para tests.
    pub fn set(&mut self, rule_id: impl Into<String>, severity: RuleSeverity) {
        self.map.insert(rule_id.into(), severity);
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
    fn off_produces_no_lsp_severity() {
        assert!(RuleSeverity::Off.to_diagnostic_severity().is_none());
        assert!(RuleSeverity::Off.is_off());
    }

    #[test]
    fn warning_promotes_correctly() {
        assert_eq!(
            RuleSeverity::Warning.to_diagnostic_severity(),
            Some(DiagnosticSeverity::WARNING)
        );
    }

    #[test]
    fn config_default_falls_back() {
        let cfg = SeverityConfig::empty();
        assert_eq!(
            cfg.effective("any-rule", RuleSeverity::Hint),
            RuleSeverity::Hint
        );
    }

    #[test]
    fn config_override_applies() {
        let mut cfg = SeverityConfig::empty();
        cfg.set("no-unused-vars", RuleSeverity::Warning);
        assert_eq!(
            cfg.effective("no-unused-vars", RuleSeverity::Hint),
            RuleSeverity::Warning
        );
    }
}
