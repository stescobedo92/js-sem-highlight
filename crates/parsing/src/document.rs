//! Estado por documento: rope + árbol tree-sitter incremental.

use std::path::Path;

use ropey::Rope;
use thiserror::Error;
use tower_lsp::lsp_types::TextDocumentContentChangeEvent;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::language::{has_node_shebang, Language, LanguageRegistry};
use crate::offset::{lsp_range_to_byte_range, OffsetError};

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("file too large: {actual} bytes (limit {limit})")]
    FileTooLarge { actual: usize, limit: usize },
    #[error("language not supported for path {0}")]
    LanguageNotSupported(String),
    #[error("parser failed to produce a tree")]
    ParseFailed,
    #[error("invalid version: incoming={incoming}, current={current}")]
    InvalidVersion { incoming: i32, current: i32 },
    #[error(transparent)]
    Offset(#[from] OffsetError),
}

/// Representación interna de un documento abierto.
#[derive(Debug)]
pub struct Document {
    pub language: Language,
    pub version: i32,
    pub rope: Rope,
    tree: Tree,
}

impl Document {
    /// Abre un documento nuevo: parsea el contenido inicial.
    ///
    /// `max_file_size_bytes` permite rechazar archivos demasiado grandes
    /// devolviendo `FileTooLarge`.
    pub fn open(
        language: Language,
        version: i32,
        text: &str,
        max_file_size_bytes: usize,
    ) -> Result<Self, DocumentError> {
        if text.len() > max_file_size_bytes {
            return Err(DocumentError::FileTooLarge {
                actual: text.len(),
                limit: max_file_size_bytes,
            });
        }
        let mut parser = Parser::new();
        parser
            .set_language(LanguageRegistry::grammar(language))
            .map_err(|_| DocumentError::ParseFailed)?;
        let tree = parser.parse(text, None).ok_or(DocumentError::ParseFailed)?;
        Ok(Self {
            language,
            version,
            rope: Rope::from_str(text),
            tree,
        })
    }

    /// Detecta el lenguaje desde un path + languageId y abre el documento.
    pub fn open_with_detection(
        path: Option<&Path>,
        language_id: Option<&str>,
        version: i32,
        text: &str,
        max_file_size_bytes: usize,
    ) -> Result<Self, DocumentError> {
        let language = Language::detect(path, language_id)
            .or_else(|| {
                if has_node_shebang(text) {
                    Some(Language::JavaScript)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                DocumentError::LanguageNotSupported(
                    path.map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<no-path>".into()),
                )
            })?;
        Self::open(language, version, text, max_file_size_bytes)
    }

    /// Acceso de solo lectura al árbol tree-sitter actual.
    #[must_use]
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Aplica una secuencia de cambios LSP incrementales al documento,
    /// reusando el árbol previo cuando es posible.
    pub fn apply_changes(
        &mut self,
        new_version: i32,
        changes: &[TextDocumentContentChangeEvent],
    ) -> Result<(), DocumentError> {
        if new_version <= self.version {
            return Err(DocumentError::InvalidVersion {
                incoming: new_version,
                current: self.version,
            });
        }
        for change in changes {
            self.apply_one_change(change)?;
        }
        self.version = new_version;
        Ok(())
    }

    fn apply_one_change(
        &mut self,
        change: &TextDocumentContentChangeEvent,
    ) -> Result<(), DocumentError> {
        let Some(range) = change.range else {
            // Cambio "full sync" (range ausente): reemplazar todo y reparsear.
            self.rope = Rope::from_str(&change.text);
            let mut parser = Parser::new();
            parser
                .set_language(LanguageRegistry::grammar(self.language))
                .map_err(|_| DocumentError::ParseFailed)?;
            self.tree = parser
                .parse(change.text.as_str(), None)
                .ok_or(DocumentError::ParseFailed)?;
            return Ok(());
        };

        let (start_byte, old_end_byte) = lsp_range_to_byte_range(&self.rope, range)?;

        let start_point = Self::point_from_byte(&self.rope, start_byte);
        let old_end_point = Self::point_from_byte(&self.rope, old_end_byte);

        let start_char = self.rope.byte_to_char(start_byte);
        let old_end_char = self.rope.byte_to_char(old_end_byte);
        self.rope.remove(start_char..old_end_char);
        self.rope.insert(start_char, &change.text);

        let new_end_byte = start_byte + change.text.len();
        let new_end_point = Self::point_from_byte(&self.rope, new_end_byte);

        let edit = InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position: start_point,
            old_end_position: old_end_point,
            new_end_position: new_end_point,
        };
        self.tree.edit(&edit);

        let mut parser = Parser::new();
        parser
            .set_language(LanguageRegistry::grammar(self.language))
            .map_err(|_| DocumentError::ParseFailed)?;
        let rope_text = self.rope.to_string();
        let new_tree = parser
            .parse(rope_text.as_str(), Some(&self.tree))
            .ok_or(DocumentError::ParseFailed)?;
        self.tree = new_tree;
        Ok(())
    }

    fn point_from_byte(rope: &Rope, byte: usize) -> Point {
        let clamped = byte.min(rope.len_bytes());
        let char_idx = rope.byte_to_char(clamped);
        let row = rope.char_to_line(char_idx);
        let line_start_char = rope.line_to_char(row);
        let line_start_byte = rope.char_to_byte(line_start_char);
        Point {
            row,
            column: clamped - line_start_byte,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::missing_const_for_fn, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_lossless)]
    use super::*;
    use tower_lsp::lsp_types::{Position, Range};

    const LIMIT: usize = 1024 * 1024;

    #[test]
    fn open_simple_js() {
        let doc = Document::open(Language::JavaScript, 1, "const x = 1;", LIMIT).expect("open");
        assert_eq!(doc.version, 1);
        assert!(!doc.tree().root_node().has_error());
    }

    #[test]
    fn open_with_too_large_file_rejects() {
        let big = "x".repeat(LIMIT + 1);
        let err = Document::open(Language::JavaScript, 1, &big, LIMIT).expect_err("rejects");
        assert!(matches!(err, DocumentError::FileTooLarge { .. }));
    }

    #[test]
    fn open_tolerates_invalid_syntax() {
        let doc = Document::open(Language::JavaScript, 1, "const x = function (", LIMIT)
            .expect("tolerant parse");
        assert!(doc.tree().root_node().has_error());
    }

    #[test]
    fn apply_change_updates_rope_and_tree() {
        let mut doc = Document::open(Language::JavaScript, 1, "const x = 1;", LIMIT).expect("open");
        let changes = vec![TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position { line: 0, character: 10 },
                end: Position { line: 0, character: 11 },
            }),
            range_length: None,
            text: "42".to_string(),
        }];
        doc.apply_changes(2, &changes).expect("apply");
        assert_eq!(doc.version, 2);
        assert_eq!(doc.rope.to_string(), "const x = 42;");
        assert!(!doc.tree().root_node().has_error());
    }

    #[test]
    fn rejects_old_version() {
        let mut doc = Document::open(Language::JavaScript, 5, "const x = 1;", LIMIT).expect("open");
        let changes = vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "let y = 2;".to_string(),
        }];
        let err = doc.apply_changes(3, &changes).expect_err("rejects");
        assert!(matches!(err, DocumentError::InvalidVersion { .. }));
    }
}
