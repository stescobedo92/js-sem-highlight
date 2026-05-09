//! Consultas de tokens terminales sobre el árbol tree-sitter.

use ropey::Rope;
use tower_lsp::lsp_types::Range;
use tree_sitter::{Node, Tree};

use crate::offset::{byte_to_lsp_position, lsp_range_to_byte_range, OffsetError};

/// Un token terminal del árbol con metadatos suficientes para el emisor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpan {
    pub kind: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub range: Range,
}

/// Devuelve los tokens terminales (nodos hoja con texto) que intersectan el
/// rango LSP solicitado, ordenados ascendentemente por byte_start.
pub fn tokens_in_range(
    tree: &Tree,
    rope: &Rope,
    range: Range,
) -> Result<Vec<TokenSpan>, OffsetError> {
    let (start_byte, end_byte) = lsp_range_to_byte_range(rope, range)?;
    let mut out = Vec::new();
    collect_tokens(tree.root_node(), rope, start_byte, end_byte, &mut out)?;
    Ok(out)
}

fn collect_tokens(
    node: Node<'_>,
    rope: &Rope,
    start_byte: usize,
    end_byte: usize,
    out: &mut Vec<TokenSpan>,
) -> Result<(), OffsetError> {
    if node.end_byte() <= start_byte || node.start_byte() >= end_byte {
        return Ok(());
    }

    if node.child_count() == 0 {
        if node.start_byte() < node.end_byte() {
            out.push(token_from_node(node, rope)?);
        }
        return Ok(());
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens(child, rope, start_byte, end_byte, out)?;
    }

    Ok(())
}

fn token_from_node(node: Node<'_>, rope: &Rope) -> Result<TokenSpan, OffsetError> {
    let start = byte_to_lsp_position(rope, node.start_byte())?;
    let end = byte_to_lsp_position(rope, node.end_byte())?;
    Ok(TokenSpan {
        kind: node.kind().to_string(),
        byte_start: node.start_byte(),
        byte_end: node.end_byte(),
        range: Range { start, end },
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        clippy::missing_const_for_fn
    )]

    use tower_lsp::lsp_types::{Position, Range};

    use crate::{Document, Language};

    use super::tokens_in_range;

    const LIMIT: usize = 1024 * 1024;

    fn token_texts(source: &str, range: Range) -> Vec<String> {
        let doc = Document::open(Language::TypeScript, 1, source, LIMIT).expect("parse");
        tokens_in_range(doc.tree(), &doc.rope, range)
            .expect("tokens")
            .into_iter()
            .map(|token| source[token.byte_start..token.byte_end].to_string())
            .collect()
    }

    #[test]
    fn multiline_file_does_not_stop_after_first_statement() {
        let source = "const greeting = \"hola\";\nlet counter = 0;\nconst unused = 42;\n";
        let texts = token_texts(
            source,
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 3,
                    character: 0,
                },
            },
        );

        assert!(texts.contains(&"greeting".to_string()));
        assert!(texts.contains(&"counter".to_string()));
        assert!(texts.contains(&"unused".to_string()));
        assert!(texts.contains(&"42".to_string()));
    }

    #[test]
    fn partial_range_returns_only_intersecting_lines() {
        let source = "const first = 1;\nconst second = 2;\nconst third = 3;\n";
        let texts = token_texts(
            source,
            Range {
                start: Position {
                    line: 1,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 0,
                },
            },
        );

        assert!(!texts.contains(&"first".to_string()));
        assert!(texts.contains(&"second".to_string()));
        assert!(!texts.contains(&"third".to_string()));
    }
}
