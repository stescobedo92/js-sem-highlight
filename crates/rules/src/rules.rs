//! Implementaciones concretas de las cinco reglas del spec.
//!
//! Cada regla es un `unit struct` (sin estado): el contexto se pasa por
//! parámetro y la regla solo lee. Esto las hace `Send + Sync` por
//! construcción y trivialmente clonables.

use js_sem_scopes::IdentifierRole;
use tower_lsp::lsp_types::{DiagnosticSeverity, DiagnosticTag, Position, Range};

use crate::context::{AnalysisContext, RuleDiagnostic, RuleEmission, TokenModifier};
use crate::severity::RuleSeverity;
use crate::VisualLintRule;

// ---------- Helpers ----------

/// Construye un `Range` placeholder a partir de byte offsets.
///
/// El LSP layer reemplazará esto con coordenadas reales del documento; por
/// ahora producimos un rango con `character` = byte offset, lo cual es
/// suficiente para tests donde el texto es ASCII.
const fn ascii_range(start: u32, end: u32) -> Range {
    Range {
        start: Position {
            line: 0,
            character: start,
        },
        end: Position {
            line: 0,
            character: end,
        },
    }
}

// ---------- no-unused-vars ----------

pub struct NoUnusedVars;

impl VisualLintRule for NoUnusedVars {
    fn id(&self) -> &'static str {
        "no-unused-vars"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Hint
    }

    fn check(&self, ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
        let Some(scopes) = ctx.scope_map else {
            return vec![];
        };
        let mut out = Vec::new();
        for binding in &scopes.bindings {
            if !binding.is_unused {
                continue;
            }
            let range = ascii_range(binding.range.start, binding.range.end);
            // Modifier: marca el token como atenuado.
            out.push(RuleEmission::TokenModifier {
                range,
                modifier: TokenModifier::Unused,
            });
            // Diagnostic: hint con tag `Unnecessary` (VS Code lo atenúa).
            out.push(RuleEmission::Diagnostic(RuleDiagnostic {
                range,
                severity: DiagnosticSeverity::HINT,
                tags: vec![DiagnosticTag::UNNECESSARY],
                message: format!(
                    "'{}' is declared but its value is never read.",
                    binding.name
                ),
                code: self.id().to_string(),
            }));
        }
        out
    }
}

// ---------- no-floating-promises ----------

pub struct NoFloatingPromises;

impl VisualLintRule for NoFloatingPromises {
    fn id(&self) -> &'static str {
        "no-floating-promises"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Hint
    }

    fn check(&self, ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
        // Heurística simplificada (la versión completa requiere el AST):
        // detectamos llamadas a funciones declaradas `async` cuyo retorno
        // se descarta. Un visitor sintáctico sobre tree-sitter haría mejor
        // trabajo; aquí marcamos casos obvios donde una expresión es
        // statement directo y empieza con palabra `await`-able.
        //
        // Estrategia mínima: buscar identificadores con role Function que
        // se llamen y cuyo binding venga de una declaración async.
        // Sin info de AST, NO emitimos nada — esta regla queda como
        // pass-through hasta que el LSP layer enriquezca el contexto.
        let _ = ctx;
        vec![]
    }
}

// ---------- no-deprecated-api ----------

pub struct NoDeprecatedApi;

impl VisualLintRule for NoDeprecatedApi {
    fn id(&self) -> &'static str {
        "no-deprecated-api"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Hint
    }

    fn check(&self, ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
        let Some(scopes) = ctx.scope_map else {
            return vec![];
        };
        let mut out = Vec::new();
        for binding in &scopes.bindings {
            if !binding.is_deprecated {
                continue;
            }
            let range = ascii_range(binding.range.start, binding.range.end);
            out.push(RuleEmission::TokenModifier {
                range,
                modifier: TokenModifier::Deprecated,
            });
            out.push(RuleEmission::Diagnostic(RuleDiagnostic {
                range,
                severity: DiagnosticSeverity::HINT,
                tags: vec![DiagnosticTag::DEPRECATED],
                message: format!("'{}' is marked as deprecated.", binding.name),
                code: self.id().to_string(),
            }));
        }
        out
    }
}

// ---------- prefer-const ----------

pub struct PreferConst;

impl VisualLintRule for PreferConst {
    fn id(&self) -> &'static str {
        "prefer-const"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Hint
    }

    fn check(&self, ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
        let Some(scopes) = ctx.scope_map else {
            return vec![];
        };
        let mut out = Vec::new();
        // Estrategia: para cada binding `LocalVariable` (no constante),
        // si NO hay ninguna referencia con `is_modification`, sugerir const.
        for binding in &scopes.bindings {
            if binding.role != IdentifierRole::LocalVariable || binding.is_const {
                continue;
            }
            let mutated = scopes
                .references
                .iter()
                .any(|r| r.name == binding.name && r.is_modification);
            if mutated {
                continue;
            }
            let range = ascii_range(binding.range.start, binding.range.end);
            out.push(RuleEmission::Diagnostic(RuleDiagnostic {
                range,
                severity: DiagnosticSeverity::HINT,
                tags: vec![],
                message: format!(
                    "'{}' is never reassigned. Use 'const' instead.",
                    binding.name
                ),
                code: self.id().to_string(),
            }));
        }
        out
    }
}

// ---------- panic-on-check (TEST-ONLY) ----------
//
// Sólo se compila en debug builds y se registra a demanda mediante la env var
// `JS_SEM_INJECT_PANIC_RULE=1`. Se usa por el test E2E 7.7 para verificar que
// el panic hook del servidor convierte panics en `InternalError` sin matar el
// proceso. NUNCA está disponible en builds release.

#[cfg(debug_assertions)]
pub struct PanickingRule;

#[cfg(debug_assertions)]
impl VisualLintRule for PanickingRule {
    fn id(&self) -> &'static str {
        "test-only-panic"
    }
    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Hint
    }
    #[allow(clippy::panic)]
    fn check(&self, _ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
        panic!("PanickingRule: deliberate panic from a test-only rule");
    }
}

// ---------- consistent-return-types ----------

pub struct ConsistentReturnTypes;

impl VisualLintRule for ConsistentReturnTypes {
    fn id(&self) -> &'static str {
        "consistent-return-types"
    }

    fn default_severity(&self) -> RuleSeverity {
        RuleSeverity::Hint
    }

    fn check(&self, _ctx: &AnalysisContext<'_>) -> Vec<RuleEmission> {
        // Igual que no-floating-promises: requiere análisis a nivel AST que
        // se hará en una iteración del LSP layer. Pass-through estable.
        vec![]
    }
}

// ---------- Tests ----------

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
    use js_sem_scopes::{analyze, CancellationToken};

    fn ctx_for<'a>(source: &'a str, scope_map: &'a js_sem_scopes::ScopeMap) -> AnalysisContext<'a> {
        AnalysisContext {
            source,
            scope_map: Some(scope_map),
            filename: "test.js",
        }
    }

    #[test]
    fn no_unused_vars_emits_for_unused_const() {
        let source = "const a = 1;";
        let map = analyze(
            source,
            js_sem_parsing::Language::JavaScript,
            &CancellationToken::new(),
        )
        .expect("analyze");
        let ctx = ctx_for(source, &map);
        let emissions = NoUnusedVars.check(&ctx);
        // Un binding unused → un TokenModifier + un Diagnostic.
        assert_eq!(emissions.len(), 2);
        assert!(emissions.iter().any(|e| matches!(
            e,
            RuleEmission::TokenModifier {
                modifier: TokenModifier::Unused,
                ..
            }
        )));
        assert!(emissions
            .iter()
            .any(|e| matches!(e, RuleEmission::Diagnostic(_))));
    }

    #[test]
    fn no_unused_vars_silent_when_used() {
        let source = "const a = 1; console.log(a);";
        let map = analyze(
            source,
            js_sem_parsing::Language::JavaScript,
            &CancellationToken::new(),
        )
        .expect("analyze");
        let ctx = ctx_for(source, &map);
        assert!(NoUnusedVars.check(&ctx).is_empty());
    }

    #[test]
    fn no_unused_vars_returns_empty_without_scope_map() {
        let ctx = AnalysisContext {
            source: "const a = 1;",
            scope_map: None,
            filename: "test.js",
        };
        assert!(NoUnusedVars.check(&ctx).is_empty());
    }

    #[test]
    fn prefer_const_silent_for_actual_const() {
        let source = "const a = 1; console.log(a);";
        let map = analyze(
            source,
            js_sem_parsing::Language::JavaScript,
            &CancellationToken::new(),
        )
        .expect("analyze");
        let ctx = ctx_for(source, &map);
        assert!(PreferConst.check(&ctx).is_empty());
    }

    #[test]
    fn rule_ids_are_stable() {
        assert_eq!(NoUnusedVars.id(), "no-unused-vars");
        assert_eq!(NoFloatingPromises.id(), "no-floating-promises");
        assert_eq!(NoDeprecatedApi.id(), "no-deprecated-api");
        assert_eq!(PreferConst.id(), "prefer-const");
        assert_eq!(ConsistentReturnTypes.id(), "consistent-return-types");
    }

    #[test]
    fn default_severity_is_hint_for_all() {
        for rule in [
            NoUnusedVars.default_severity(),
            NoFloatingPromises.default_severity(),
            NoDeprecatedApi.default_severity(),
            PreferConst.default_severity(),
            ConsistentReturnTypes.default_severity(),
        ] {
            assert_eq!(rule, RuleSeverity::Hint);
        }
    }
}
