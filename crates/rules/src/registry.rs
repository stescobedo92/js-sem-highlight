//! Registry de reglas.
//!
//! Encapsula:
//! - Almacenamiento por id (HashMap)
//! - Validación de id duplicado (fail-fast en arranque)
//! - Lock post-`initialized` (rechaza nuevas reglas en runtime)
//!
//! Se construye antes de aceptar requests; se sirve via `Arc` después.

use std::collections::HashMap;

use thiserror::Error;

use crate::VisualLintRule;

#[derive(Debug, Error)]
pub enum RegistrationError {
    #[error("duplicate rule id: {0}")]
    Duplicate(&'static str),
    #[error("registration is locked (after `initialized`)")]
    Locked,
}

/// Almacén ordenado de reglas.
#[derive(Default)]
pub struct RuleRegistry {
    rules: HashMap<&'static str, Box<dyn VisualLintRule>>,
    /// Orden de inserción para deduplicación determinística.
    order: Vec<&'static str>,
    locked: bool,
}

impl std::fmt::Debug for RuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleRegistry")
            .field("rules", &self.order)
            .field("locked", &self.locked)
            .finish()
    }
}

impl RuleRegistry {
    /// Crea un registry vacío y desbloqueado.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra una nueva regla. Retorna `Err` si:
    /// - El id ya está registrado.
    /// - El registry está locked (post-`initialized`).
    pub fn try_register(
        &mut self,
        rule: Box<dyn VisualLintRule>,
    ) -> Result<(), RegistrationError> {
        if self.locked {
            return Err(RegistrationError::Locked);
        }
        let id = rule.id();
        if self.rules.contains_key(id) {
            return Err(RegistrationError::Duplicate(id));
        }
        self.rules.insert(id, rule);
        self.order.push(id);
        Ok(())
    }

    /// Bloquea el registry. Las llamadas subsiguientes a `try_register`
    /// retornarán `Locked`.
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// `true` si el registry está locked.
    #[must_use]
    pub const fn is_locked(&self) -> bool {
        self.locked
    }

    /// Encuentra una regla por id.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&dyn VisualLintRule> {
        self.rules.get(id).map(|r| r.as_ref())
    }

    /// Itera reglas en orden de inserción.
    pub fn iter(&self) -> impl Iterator<Item = &dyn VisualLintRule> {
        self.order
            .iter()
            .filter_map(|id| self.rules.get(id).map(|r| r.as_ref()))
    }

    /// Cantidad de reglas registradas.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// `true` si no hay reglas registradas.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::missing_const_for_fn, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_lossless)]
    use super::*;
    use crate::context::{AnalysisContext, RuleEmission};
    use crate::severity::RuleSeverity;

    struct DummyRule(&'static str);
    impl VisualLintRule for DummyRule {
        fn id(&self) -> &'static str {
            self.0
        }
        fn default_severity(&self) -> RuleSeverity {
            RuleSeverity::Hint
        }
        fn check(&self, _ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
            vec![]
        }
    }

    #[test]
    fn register_and_find() {
        let mut r = RuleRegistry::new();
        r.try_register(Box::new(DummyRule("foo"))).expect("ok");
        assert!(r.find("foo").is_some());
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut r = RuleRegistry::new();
        r.try_register(Box::new(DummyRule("foo"))).expect("ok");
        let err = r.try_register(Box::new(DummyRule("foo"))).expect_err("dup");
        assert!(matches!(err, RegistrationError::Duplicate("foo")));
    }

    #[test]
    fn lock_rejects_new_registration() {
        let mut r = RuleRegistry::new();
        r.try_register(Box::new(DummyRule("foo"))).expect("ok");
        r.lock();
        assert!(r.is_locked());
        let err = r.try_register(Box::new(DummyRule("bar"))).expect_err("locked");
        assert!(matches!(err, RegistrationError::Locked));
    }

    #[test]
    fn iter_preserves_insertion_order() {
        let mut r = RuleRegistry::new();
        r.try_register(Box::new(DummyRule("c"))).expect("ok");
        r.try_register(Box::new(DummyRule("a"))).expect("ok");
        r.try_register(Box::new(DummyRule("b"))).expect("ok");
        let ids: Vec<_> = r.iter().map(VisualLintRule::id).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }
}
