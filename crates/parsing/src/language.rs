//! Detección y registro de gramáticas tree-sitter por dialecto.

use std::path::Path;
use std::sync::OnceLock;

use tree_sitter::Language as TsLanguage;

/// Dialecto soportado por el pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    JavaScript,
    TypeScript,
    Jsx,
    Tsx,
}

impl Language {
    /// Detecta el dialecto a partir de la extensión del archivo.
    ///
    /// Devuelve `None` si la extensión no corresponde a ninguno de los
    /// dialectos soportados.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "jsx" => Some(Self::Jsx),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    /// Detecta el dialecto a partir del `languageId` LSP.
    #[must_use]
    pub fn from_language_id(language_id: &str) -> Option<Self> {
        match language_id {
            "javascript" => Some(Self::JavaScript),
            "javascriptreact" => Some(Self::Jsx),
            "typescript" => Some(Self::TypeScript),
            "typescriptreact" => Some(Self::Tsx),
            _ => None,
        }
    }

    /// Detecta el dialecto a partir del path del archivo.
    ///
    /// La detección prioriza la extensión sobre cualquier otra heurística.
    /// Si la extensión no coincide, retorna `None` (el caller puede caer
    /// al `languageId` LSP).
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }

    /// Combina detección por path y por `languageId` siguiendo el orden del
    /// spec: extensión primero, luego `languageId`.
    #[must_use]
    pub fn detect(path: Option<&Path>, language_id: Option<&str>) -> Option<Self> {
        path.and_then(Self::from_path)
            .or_else(|| language_id.and_then(Self::from_language_id))
    }
}

/// Registro de gramáticas tree-sitter cacheadas.
///
/// Las gramáticas son `Send + Sync` y baratas de clonar, por lo que se
/// almacenan en `OnceLock` por dialecto.
pub struct LanguageRegistry;

impl LanguageRegistry {
    /// Devuelve la gramática tree-sitter correspondiente al dialecto.
    pub fn grammar(language: Language) -> &'static TsLanguage {
        static JS: OnceLock<TsLanguage> = OnceLock::new();
        static TS: OnceLock<TsLanguage> = OnceLock::new();
        static TSX: OnceLock<TsLanguage> = OnceLock::new();

        match language {
            Language::JavaScript | Language::Jsx => {
                JS.get_or_init(|| tree_sitter_javascript::LANGUAGE.into())
            }
            Language::TypeScript => {
                TS.get_or_init(|| tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            }
            Language::Tsx => TSX.get_or_init(|| tree_sitter_typescript::LANGUAGE_TSX.into()),
        }
    }
}

/// Detecta si la línea inicial es un shebang `#!` que invoca Node.js.
///
/// Sirve para clasificar archivos `.mjs` o sin extensión que dependen del
/// runtime Node. La detección es deliberadamente estrecha: solo `node` (con
/// o sin path absoluto) cuenta.
#[must_use]
pub fn has_node_shebang(source: &str) -> bool {
    let Some(first_line) = source.lines().next() else {
        return false;
    };
    if !first_line.starts_with("#!") {
        return false;
    }
    let rest = &first_line[2..];
    rest.contains("node") || rest.contains("nodejs")
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
    fn from_extension_known() {
        assert_eq!(Language::from_extension("js"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("MJS"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("cjs"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("mts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("jsx"), Some(Language::Jsx));
        assert_eq!(Language::from_extension("tsx"), Some(Language::Tsx));
    }

    #[test]
    fn from_extension_unknown() {
        assert_eq!(Language::from_extension("rs"), None);
        assert_eq!(Language::from_extension(""), None);
        assert_eq!(Language::from_extension("md"), None);
    }

    #[test]
    fn from_language_id_known() {
        assert_eq!(
            Language::from_language_id("javascript"),
            Some(Language::JavaScript)
        );
        assert_eq!(
            Language::from_language_id("javascriptreact"),
            Some(Language::Jsx)
        );
        assert_eq!(
            Language::from_language_id("typescript"),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_language_id("typescriptreact"),
            Some(Language::Tsx)
        );
    }

    #[test]
    fn detect_prefers_extension_over_language_id() {
        let path = Path::new("/proj/comp.tsx");
        assert_eq!(
            Language::detect(Some(path), Some("javascript")),
            Some(Language::Tsx),
            "la extensión debe ganar al languageId conflictivo"
        );
    }

    #[test]
    fn detect_falls_back_to_language_id() {
        assert_eq!(
            Language::detect(None, Some("typescriptreact")),
            Some(Language::Tsx)
        );
    }

    #[test]
    fn detect_returns_none_when_both_missing() {
        assert_eq!(Language::detect(None, None), None);
    }

    #[test]
    fn shebang_detection() {
        assert!(has_node_shebang("#!/usr/bin/env node\nconsole.log(1)"));
        assert!(has_node_shebang("#!/usr/local/bin/node"));
        assert!(has_node_shebang("#!/usr/bin/env nodejs"));
        assert!(!has_node_shebang("#!/bin/bash"));
        assert!(!has_node_shebang("// no shebang"));
        assert!(!has_node_shebang(""));
    }

    #[test]
    fn registry_returns_grammars_for_each_dialect() {
        let _ = LanguageRegistry::grammar(Language::JavaScript);
        let _ = LanguageRegistry::grammar(Language::Jsx);
        let _ = LanguageRegistry::grammar(Language::TypeScript);
        let _ = LanguageRegistry::grammar(Language::Tsx);
    }
}
