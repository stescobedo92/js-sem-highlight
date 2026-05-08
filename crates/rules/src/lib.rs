//! Reglas visuales de lint.
//!
//! Cada regla implementa `VisualLintRule`, recibe un `AnalysisContext` y emite
//! cero o más `RuleEmission` (token modifiers, diagnostics, o ambos).
//!
//! Las reglas se registran al arrancar el servidor (antes de `initialized`) en
//! un `RuleRegistry` global. Después del lock, intentos de registro retornan
//! `RegistrationLocked`.

mod context;
mod registry;
mod rules;
mod severity;

pub use context::{dedupe_emissions, AnalysisContext, RuleDiagnostic, RuleEmission, TokenModifier};
pub use registry::{RegistrationError, RuleRegistry};
#[cfg(debug_assertions)]
pub use rules::PanickingRule;
pub use rules::{
    ConsistentReturnTypes, NoDeprecatedApi, NoFloatingPromises, NoUnusedVars, PreferConst,
};
pub use severity::{RuleSeverity, SeverityConfig};

/// Trait que toda regla debe implementar.
pub trait VisualLintRule: Send + Sync {
    /// Identificador estable, usado para configuración (`rules.<id>`).
    fn id(&self) -> &'static str;

    /// Severidad por defecto.
    fn default_severity(&self) -> RuleSeverity;

    /// Ejecuta la regla. El runtime ya filtró por configuración: si la regla
    /// está en `"off"`, no llama a `check`.
    fn check(&self, ctx: &AnalysisContext<'_>) -> Vec<RuleEmission>;
}

/// Crea un registry pre-cargado con las cinco reglas iniciales del spec.
///
/// Esto es lo que llama el binario al arrancar.
///
/// En debug builds Y con la env var `JS_SEM_INJECT_PANIC_RULE=1` activa, también
/// se registra `PanickingRule` para soportar el test E2E 7.7. Esta inyección
/// nunca está disponible en builds release (gated por `cfg(debug_assertions)`).
#[must_use]
pub fn default_registry() -> RuleRegistry {
    let mut r = RuleRegistry::new();
    // Los `let _` están permitidos porque los IDs son únicos por construcción
    // y `try_register` solo falla por duplicados o lock.
    let _ = r.try_register(Box::new(NoUnusedVars));
    let _ = r.try_register(Box::new(NoFloatingPromises));
    let _ = r.try_register(Box::new(NoDeprecatedApi));
    let _ = r.try_register(Box::new(PreferConst));
    let _ = r.try_register(Box::new(ConsistentReturnTypes));

    #[cfg(debug_assertions)]
    {
        if std::env::var_os("JS_SEM_INJECT_PANIC_RULE").is_some() {
            let _ = r.try_register(Box::new(PanickingRule));
        }
    }

    r
}
