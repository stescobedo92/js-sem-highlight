//! Pipeline de parsing incremental basado en tree-sitter.
//!
//! Mantiene un árbol por documento abierto, soporta JS/TS/JSX/TSX, y expone
//! consultas de tokens por rango. Es la fuente de verdad de rangos para todo
//! el servidor.

pub mod document;
pub mod language;
pub mod offset;
pub mod tokens;

pub use document::{Document, DocumentError};
pub use language::{Language, LanguageRegistry};
pub use offset::{lsp_range_to_byte_range, OffsetError};
pub use tokens::{TokenSpan, tokens_in_range};
