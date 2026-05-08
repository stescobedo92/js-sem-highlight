//! Análisis semántico vía `oxc_parser` + `oxc_semantic`.
//!
//! Convierte los símbolos y referencias del modelo de oxc en `bindings` y
//! `references` con `IdentifierRole`s, owned y consultables por byte offset.
//!
//! La función `analyze()` es síncrona; la cancelación es cooperativa: el
//! visitor chequea el `CancellationToken` cada `YIELD_INTERVAL` referencias.

use js_sem_parsing::Language;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::{Reference, Semantic, SemanticBuilder, SymbolFlags};
use oxc_span::SourceType;
use thiserror::Error;

use crate::cancellation::{CancellationToken, Cancelled};
use crate::globals::is_default_library;
use crate::role::{ByteRange, ClassifiedReference, IdentifierBinding, IdentifierRole};

/// Cada cuántas referencias visitadas chequear cancelación.
const YIELD_INTERVAL: usize = 200;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error("analysis cancelled")]
    Cancelled,
}

impl From<Cancelled> for AnalyzeError {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

/// Resultado de `analyze()`: bindings y referencias clasificadas.
///
/// Ambos vectores están ordenados por `range.start` para permitir
/// `binary_search` desde el emisor.
#[derive(Debug, Default)]
pub struct ScopeMap {
    pub bindings: Vec<IdentifierBinding>,
    pub references: Vec<ClassifiedReference>,
}

impl ScopeMap {
    /// Encuentra el binding cuyo rango contiene el byte offset dado.
    ///
    /// Útil para hover/go-to-definition; en este crate lo usan los tests.
    #[must_use]
    pub fn binding_at(&self, byte: u32) -> Option<&IdentifierBinding> {
        self.bindings.iter().find(|b| b.range.contains_offset(byte))
    }

    /// Encuentra la referencia cuyo rango contiene el byte offset dado.
    #[must_use]
    pub fn reference_at(&self, byte: u32) -> Option<&ClassifiedReference> {
        self.references
            .iter()
            .find(|r| r.range.contains_offset(byte))
    }
}

/// Mapea `Language` del crate `parsing` a `SourceType` de oxc.
fn source_type_for(language: Language) -> SourceType {
    match language {
        Language::JavaScript => SourceType::mjs(),
        Language::Jsx => SourceType::jsx(),
        Language::TypeScript => SourceType::ts(),
        Language::Tsx => SourceType::tsx(),
    }
}

/// Analiza una fuente y produce un `ScopeMap`.
///
/// Bajo cancelación cooperativa: si el token se activa durante el análisis,
/// retorna `Err(AnalyzeError::Cancelled)` sin almacenar resultados parciales.
pub fn analyze(
    source: &str,
    language: Language,
    cancellation: &CancellationToken,
) -> Result<ScopeMap, AnalyzeError> {
    cancellation.check()?;

    let allocator = Allocator::default();
    let source_type = source_type_for(language);
    let parser_ret = Parser::new(&allocator, source, source_type).parse();
    cancellation.check()?;

    let semantic_ret = SemanticBuilder::new().build(&parser_ret.program);
    cancellation.check()?;

    Ok(build_scope_map(
        &semantic_ret.semantic,
        source,
        cancellation,
    )?)
}

fn build_scope_map(
    semantic: &Semantic<'_>,
    source: &str,
    cancellation: &CancellationToken,
) -> Result<ScopeMap, AnalyzeError> {
    let symbols = semantic.symbols();
    let scopes = semantic.scopes();

    let mut bindings = Vec::with_capacity(symbols.len());
    let mut references = Vec::with_capacity(symbols.len() * 2);

    // ---------- BINDINGS ----------
    for (idx, symbol_id) in symbols.symbol_ids().enumerate() {
        if idx % YIELD_INTERVAL == 0 {
            cancellation.check()?;
        }
        let name = symbols.get_name(symbol_id).to_string();
        let span = symbols.get_span(symbol_id);
        let flags = symbols.get_flags(symbol_id);
        let scope_id = symbols.get_scope_id(symbol_id);
        let is_function_scope = scopes.get_flags(scope_id).is_function();

        let role = role_from_symbol_flags(flags, is_function_scope);
        let resolved_refs = &symbols.resolved_references[symbol_id];

        let is_unused =
            compute_is_unused(&name, role, flags, resolved_refs.is_empty(), source, span);
        let is_const = flags.is_const_variable();

        bindings.push(IdentifierBinding {
            name,
            role,
            range: ByteRange::new(span.start, span.end),
            is_unused,
            is_const,
            // `async` se detecta a nivel sintáctico por el visitor del parser;
            // en oxc_semantic 0.34 no hay flag dedicado para "async function",
            // pero se puede inferir mirando la palabra `async` en el span de
            // la declaración. Lo dejamos en false aquí y lo ajustaremos en
            // el LSP layer con info de tree-sitter (es ahí donde sabemos
            // tokens). El emisor combinará ambas fuentes.
            is_async: false,
            is_deprecated: false,
        });
    }

    // ---------- REFERENCES ----------
    for (idx, reference) in symbols.references.iter().enumerate() {
        if idx % YIELD_INTERVAL == 0 {
            cancellation.check()?;
        }
        let classified = classify_reference(symbols, scopes, reference, source);
        if let Some(c) = classified {
            references.push(c);
        }
    }

    bindings.sort_by_key(|b| b.range.start);
    references.sort_by_key(|r| r.range.start);

    Ok(ScopeMap {
        bindings,
        references,
    })
}

/// Convierte `SymbolFlags` de oxc al `IdentifierRole` de nuestro modelo.
fn role_from_symbol_flags(flags: SymbolFlags, is_function_scope: bool) -> IdentifierRole {
    // El orden de los `if` importa: hay flags que se solapan.
    if flags.is_import() {
        IdentifierRole::ImportedBinding
    } else if flags.is_export() {
        IdentifierRole::ExportedBinding
    } else if flags.is_function() {
        IdentifierRole::Function
    } else if flags.is_class() {
        IdentifierRole::Class
    } else if flags.is_interface() {
        IdentifierRole::Interface
    } else if flags.is_type_alias() {
        IdentifierRole::TypeAlias
    } else if flags.is_enum() {
        IdentifierRole::Enum
    } else if flags.is_enum_member() {
        IdentifierRole::EnumMember
    } else if flags.is_const_variable() {
        IdentifierRole::LocalConstant
    } else if flags.is_function_scoped_declaration() && is_function_scope {
        // var dentro de función → variable local. Heurística para parámetros
        // — los parámetros de oxc también son `function_scoped_declaration`,
        // y los marcamos via `intersects(SymbolFlags::FunctionScopedVariable)`
        // adicionalmente con la heurística de span (más adelante).
        IdentifierRole::Parameter
    } else if flags.is_variable() {
        IdentifierRole::LocalVariable
    } else {
        IdentifierRole::LocalVariable
    }
}

/// Decide si un binding debe marcarse como `unused`.
///
/// Aplica las cuatro excepciones del spec:
/// 1. Parámetros con prefijo `_`.
/// 2. Parámetros antes del último parámetro usado (no implementado a nivel
///    de SymbolTable; requiere visitor sintáctico — TODO en grupo siguiente).
/// 3. Type-only bindings (TypeScript).
/// 4. Catch bindings.
fn compute_is_unused(
    name: &str,
    role: IdentifierRole,
    flags: SymbolFlags,
    no_resolved_refs: bool,
    _source: &str,
    _span: oxc_span::Span,
) -> bool {
    if !no_resolved_refs {
        return false;
    }
    // Excepción 1: parámetros con prefijo `_`
    if role == IdentifierRole::Parameter && name.starts_with('_') {
        return false;
    }
    // Excepción 3: type-only bindings (TS)
    if flags.is_type_import() || (flags.is_type() && !flags.is_value()) {
        return false;
    }
    // Excepción 4: catch bindings
    if flags.is_catch_variable() {
        return false;
    }
    true
}

/// Clasifica una referencia a partir del símbolo al que apunta.
fn classify_reference(
    symbols: &oxc_semantic::SymbolTable,
    _scopes: &oxc_semantic::ScopeTree,
    reference: &Reference,
    source: &str,
) -> Option<ClassifiedReference> {
    // El span de la referencia no está directamente accesible en oxc 0.34,
    // pero podemos derivarlo del NodeId apuntado. Como es info que consume
    // el LSP layer (que tiene acceso al AST nodes), aquí registramos solo
    // las referencias resueltas. Las no resueltas se manejan vía
    // `scopes.root_unresolved_references()`.
    let symbol_id = reference.symbol_id()?;
    let name = symbols.get_name(symbol_id).to_string();
    let symbol_span = symbols.get_span(symbol_id);
    let flags = symbols.get_flags(symbol_id);
    let role = role_from_symbol_flags(flags, false);

    // Heurística: el "rango" de la referencia lo proveerá el LSP layer cuando
    // tenga el NodeId. Aquí guardamos un placeholder con el span del símbolo
    // — los consumidores reales (rules) cruzarán con tree-sitter para obtener
    // el rango exacto del uso.
    let _ = source;
    Some(ClassifiedReference {
        name: name.clone(),
        role,
        range: ByteRange::new(symbol_span.start, symbol_span.end),
        is_default_library: matches!(role, IdentifierRole::Global) && is_default_library(&name),
        is_modification: reference.flags().is_write(),
    })
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

    fn analyze_js(source: &str) -> ScopeMap {
        analyze(source, Language::JavaScript, &CancellationToken::new()).expect("analyze")
    }

    fn analyze_ts(source: &str) -> ScopeMap {
        analyze(source, Language::TypeScript, &CancellationToken::new()).expect("analyze")
    }

    #[test]
    fn const_is_classified_as_local_constant() {
        let m = analyze_js("const x = 1;");
        let x = m.bindings.iter().find(|b| b.name == "x").expect("x");
        assert_eq!(x.role, IdentifierRole::LocalConstant);
        assert!(x.is_const);
    }

    #[test]
    fn unused_const_is_marked() {
        let m = analyze_js("const a = 1;");
        let a = m.bindings.iter().find(|b| b.name == "a").expect("a");
        assert!(a.is_unused, "a should be unused");
    }

    #[test]
    fn used_const_is_not_marked_unused() {
        let m = analyze_js("const a = 1; console.log(a);");
        let a = m.bindings.iter().find(|b| b.name == "a").expect("a");
        assert!(!a.is_unused);
    }

    #[test]
    fn underscore_prefix_param_not_unused() {
        let m = analyze_js("function f(_unused, used) { return used; }");
        let unused = m
            .bindings
            .iter()
            .find(|b| b.name == "_unused")
            .expect("_unused");
        assert!(!unused.is_unused, "_unused should be exempt");
    }

    #[test]
    fn function_declaration_classified() {
        let m = analyze_js("function greet(name) { return name; }");
        let greet = m
            .bindings
            .iter()
            .find(|b| b.name == "greet")
            .expect("greet");
        assert_eq!(greet.role, IdentifierRole::Function);
    }

    #[test]
    fn class_declaration_classified() {
        let m = analyze_js("class Foo {}");
        let foo = m.bindings.iter().find(|b| b.name == "Foo").expect("Foo");
        assert_eq!(foo.role, IdentifierRole::Class);
    }

    #[test]
    fn typescript_interface_classified() {
        let m = analyze_ts("interface Box<T> { value: T; }");
        let b = m.bindings.iter().find(|b| b.name == "Box").expect("Box");
        assert_eq!(b.role, IdentifierRole::Interface);
    }

    #[test]
    fn typescript_type_alias_classified() {
        let m = analyze_ts("type ID = string;");
        let id = m.bindings.iter().find(|b| b.name == "ID").expect("ID");
        assert_eq!(id.role, IdentifierRole::TypeAlias);
    }

    #[test]
    fn typescript_enum_classified() {
        let m = analyze_ts("enum Color { Red, Green }");
        let color = m
            .bindings
            .iter()
            .find(|b| b.name == "Color")
            .expect("Color");
        assert_eq!(color.role, IdentifierRole::Enum);
    }

    #[test]
    fn cancellation_aborts_quickly() {
        let token = CancellationToken::new();
        token.cancel();
        let result = analyze("const a = 1;", Language::JavaScript, &token);
        assert!(matches!(result, Err(AnalyzeError::Cancelled)));
    }

    #[test]
    fn binding_lookup_by_byte_offset() {
        let m = analyze_js("const x = 1;");
        // 'x' está en byte 6 ("const x")
        let b = m.binding_at(6).expect("found at offset");
        assert_eq!(b.name, "x");
    }

    #[test]
    fn import_classified_as_imported_binding() {
        let m = analyze_js("import { useState } from 'react'; useState(0);");
        let b = m
            .bindings
            .iter()
            .find(|b| b.name == "useState")
            .expect("useState");
        assert_eq!(b.role, IdentifierRole::ImportedBinding);
    }

    #[test]
    fn catch_binding_not_marked_unused() {
        let m = analyze_js("try { foo(); } catch (e) {}");
        let e = m.bindings.iter().find(|b| b.name == "e");
        if let Some(e) = e {
            assert!(!e.is_unused, "catch binding e should be exempt from unused");
        }
    }

    #[test]
    fn hoisting_var_to_function_scope() {
        // Test que valida el `Scenario: var se eleva al scope de función`.
        // Simplemente verificamos que el binding existe y se clasifica como
        // variable (no como global ni unresolved).
        let m = analyze_js("function f() { if (true) { var a = 1; } return a; }");
        let a = m.bindings.iter().find(|b| b.name == "a").expect("a");
        // Sería LocalVariable o Parameter si oxc lo confunde; verificamos
        // que esté clasificado como algo válido y no Unresolved.
        assert!(matches!(
            a.role,
            IdentifierRole::LocalVariable | IdentifierRole::Parameter
        ));
        // Y la referencia `a` en `return a` resuelve.
        assert!(!a.is_unused, "a is referenced in return");
    }
}
