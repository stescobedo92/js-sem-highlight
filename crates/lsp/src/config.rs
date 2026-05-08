//! Configuración por workspace.
//!
//! Se entrega vía `initializationOptions` en el primer mensaje y puede
//! actualizarse en runtime via `workspace/didChangeConfiguration`.

use std::collections::HashSet;

use js_sem_rules::SeverityConfig;
use serde::Deserialize;
use serde_json::Value;

const DEFAULT_MAX_FILE_SIZE_KB: usize = 512;

/// Configuración efectiva del servidor.
#[derive(Debug, Clone)]
pub struct Config {
    pub max_file_size_bytes: usize,
    pub rules: SeverityConfig,
    pub ignore_patterns: Vec<String>,
    pub enabled_modifiers: HashSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut enabled_modifiers = HashSet::new();
        enabled_modifiers.insert("unused".into());
        enabled_modifiers.insert("defaultLibrary".into());
        enabled_modifiers.insert("readonly".into());
        enabled_modifiers.insert("declaration".into());
        Self {
            max_file_size_bytes: DEFAULT_MAX_FILE_SIZE_KB * 1024,
        rules: SeverityConfig::empty(),
        ignore_patterns: vec![
            "**/node_modules/**".into(),
            "**/dist/**".into(),
            "**/*.min.js".into(),
        ],
            enabled_modifiers,
        }
    }
}

/// Esquema serde para `initializationOptions`.
///
/// Todos los campos son opcionales: si faltan, se usa el default.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawInitOptions {
    pub max_file_size_kb: Option<usize>,
    pub rules: Option<SeverityConfig>,
    pub ignore: Option<Vec<String>>,
    pub token_modifiers: Option<std::collections::HashMap<String, bool>>,
}

impl Config {
    /// Construye una `Config` desde el JSON de `initializationOptions`.
    ///
    /// Si el JSON falla de validar (e.g. `maxFileSizeKb` no es número), se
    /// loguea el error y se conserva el default para ese campo.
    #[must_use]
    pub fn from_init_options(value: Option<Value>) -> Self {
        let mut cfg = Self::default();
        let Some(value) = value else {
            return cfg;
        };
        let raw: RawInitOptions = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(error = %err, "invalid initializationOptions; using defaults");
                return cfg;
            }
        };
        if let Some(kb) = raw.max_file_size_kb {
            cfg.max_file_size_bytes = kb * 1024;
        }
        if let Some(rules) = raw.rules {
            cfg.rules = rules;
        }
        if let Some(ignore) = raw.ignore {
            cfg.ignore_patterns = ignore;
        }
        if let Some(modifiers) = raw.token_modifiers {
            cfg.enabled_modifiers = modifiers
                .into_iter()
                .filter_map(|(k, v)| if v { Some(k) } else { None })
                .collect();
        }
        cfg
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::missing_const_for_fn, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_lossless)]
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let cfg = Config::default();
        assert_eq!(cfg.max_file_size_bytes, 512 * 1024);
        assert!(cfg.ignore_patterns.iter().any(|p| p.contains("node_modules")));
    }

    #[test]
    fn parses_valid_options() {
        let value = serde_json::json!({
            "maxFileSizeKb": 2048,
            "rules": { "no-unused-vars": "warning", "prefer-const": "off" },
            "ignore": ["**/build/**"]
        });
        let cfg = Config::from_init_options(Some(value));
        assert_eq!(cfg.max_file_size_bytes, 2048 * 1024);
        assert_eq!(
            cfg.rules.effective("no-unused-vars", js_sem_rules::RuleSeverity::Hint),
            js_sem_rules::RuleSeverity::Warning
        );
        assert_eq!(cfg.ignore_patterns, vec!["**/build/**".to_string()]);
    }

    #[test]
    fn invalid_options_fall_back_to_defaults() {
        let value = serde_json::json!({
            "maxFileSizeKb": "no-soy-numero"
        });
        let cfg = Config::from_init_options(Some(value));
        // Invalid → default 512 KB
        assert_eq!(cfg.max_file_size_bytes, 512 * 1024);
    }

    #[test]
    fn missing_options_uses_defaults() {
        let cfg = Config::from_init_options(None);
        assert_eq!(cfg.max_file_size_bytes, 512 * 1024);
    }
}
