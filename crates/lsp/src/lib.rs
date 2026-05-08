//! Servidor LSP: orquesta parsing, scopes y reglas, habla `tower-lsp`.
//!
//! Implementa los métodos LSP requeridos por `specs/lsp-server-runtime/spec.md`
//! y `specs/semantic-token-emitter/spec.md`.

pub mod backend;
pub mod cache;
pub mod config;
pub mod tokens;

pub use backend::Backend;
pub use config::Config;
pub use tokens::{legend, token_type_index, Modifiers, TokenModifierLegend, TokenTypeLegend};
