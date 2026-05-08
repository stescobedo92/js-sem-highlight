//! Consultas de tokens terminales sobre el árbol tree-sitter.

use ropey::Rope;
use tower_lsp::lsp_types::Range;
use tree_sitter::{Node, Tree, TreeCursor};

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
    let mut cursor = tree.walk();
    collect_tokens(&mut cursor, rope, start_byte, end_byte, &mut out)?;
    Ok(out)
}

fn collect_tokens(
    cursor: &mut TreeCursor<'_>,
    rope: &Rope,
    start_byte: usize,
    end_byte: usize,
    out: &mut Vec<TokenSpan>,
) -> Result<(), OffsetError> {
    loop {
        let node = cursor.node();
        if node.end_byte() <= start_byte || node.start_byte() >= end_byte {
            // Fuera de rango: avanzar al siguiente sibling sin descender.
            if !cursor.goto_next_sibling() {
                return ascend_until_sibling(cursor);
            }
            continue;
        }

        if node.child_count() == 0 && node.start_byte() < node.end_byte() {
            out.push(token_from_node(node, rope)?);
        } else if cursor.goto_first_child() {
            continue;
        }

        if !cursor.goto_next_sibling() {
            return ascend_until_sibling(cursor);
        }
    }
}

fn ascend_until_sibling(cursor: &mut TreeCursor<'_>) -> Result<(), OffsetError> {
    while cursor.goto_parent() {
        if cursor.goto_next_sibling() {
            return Ok(());
        }
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
