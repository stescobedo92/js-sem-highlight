//! Conversiones entre posiciones LSP (UTF-16, line/character) y byte offsets
//! del `Rope`.
//!
//! Es una de las fuentes clásicas de bugs en servidores LSP. La estrategia
//! aquí es: el `Rope` indexa por chars (Unicode scalar values); LSP indexa
//! por code units UTF-16. Convertimos en dos pasos: línea+UTF-16 → chars del
//! rope → bytes del rope.

use ropey::Rope;
use thiserror::Error;
use tower_lsp::lsp_types::{Position, Range};

#[derive(Debug, Error)]
pub enum OffsetError {
    #[error("line {0} out of range (rope has {1} lines)")]
    LineOutOfRange(u32, usize),
    #[error("UTF-16 character {0} out of range on line {1}")]
    CharacterOutOfRange(u32, u32),
}

/// Convierte una `Position` LSP (0-indexed, UTF-16) al byte offset del `Rope`.
pub fn lsp_position_to_byte(rope: &Rope, position: Position) -> Result<usize, OffsetError> {
    let line = position.line as usize;
    if line >= rope.len_lines() {
        return Err(OffsetError::LineOutOfRange(position.line, rope.len_lines()));
    }
    let line_start_char = rope.line_to_char(line);
    let line_slice = rope.line(line);

    let mut utf16_remaining = position.character;
    let mut char_offset = 0usize;
    for ch in line_slice.chars() {
        if utf16_remaining == 0 {
            break;
        }
        let units = ch.len_utf16() as u32;
        if utf16_remaining < units {
            return Err(OffsetError::CharacterOutOfRange(
                position.character,
                position.line,
            ));
        }
        utf16_remaining -= units;
        char_offset += 1;
    }
    Ok(rope.char_to_byte(line_start_char + char_offset))
}

/// Convierte un byte offset al `Position` LSP correspondiente.
pub fn byte_to_lsp_position(rope: &Rope, byte: usize) -> Result<Position, OffsetError> {
    let clamped = byte.min(rope.len_bytes());
    let char_idx = rope.byte_to_char(clamped);
    let line = rope.char_to_line(char_idx);
    let line_start_char = rope.line_to_char(line);
    let line_slice = rope.line(line);

    let mut utf16: u32 = 0;
    let mut consumed = 0usize;
    let target = char_idx - line_start_char;
    for ch in line_slice.chars() {
        if consumed == target {
            break;
        }
        utf16 += ch.len_utf16() as u32;
        consumed += 1;
    }
    Ok(Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: utf16,
    })
}

/// Convierte un `Range` LSP a un `(byte_start, byte_end)` cerrado-abierto.
pub fn lsp_range_to_byte_range(rope: &Rope, range: Range) -> Result<(usize, usize), OffsetError> {
    let start = lsp_position_to_byte(rope, range.start)?;
    let end = lsp_position_to_byte(rope, range.end)?;
    Ok((start, end))
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

    fn rope(s: &str) -> Rope {
        Rope::from_str(s)
    }

    #[test]
    fn ascii_round_trip() {
        let r = rope("hola\nmundo\n");
        let pos = Position {
            line: 1,
            character: 3,
        };
        let byte = lsp_position_to_byte(&r, pos).expect("convert");
        let back = byte_to_lsp_position(&r, byte).expect("convert back");
        assert_eq!(back, pos);
    }

    #[test]
    fn supplementary_plane_uses_two_utf16_units() {
        // 🌍 (U+1F30D) ocupa 2 code units en UTF-16, 1 char en Rust, 4 bytes UTF-8.
        let r = rope("a🌍b");
        let pos_after_emoji = Position {
            line: 0,
            character: 3,
        };
        let byte = lsp_position_to_byte(&r, pos_after_emoji).expect("convert");
        // 'a' (1 byte) + '🌍' (4 bytes) = 5
        assert_eq!(byte, 5);
    }

    #[test]
    fn position_at_end_of_line_is_valid() {
        let r = rope("abc\ndef\n");
        let pos = Position {
            line: 0,
            character: 3,
        };
        let byte = lsp_position_to_byte(&r, pos).expect("end of line");
        assert_eq!(byte, 3);
    }

    #[test]
    fn line_out_of_range() {
        let r = rope("only one line");
        let pos = Position {
            line: 5,
            character: 0,
        };
        assert!(matches!(
            lsp_position_to_byte(&r, pos),
            Err(OffsetError::LineOutOfRange(5, _))
        ));
    }

    #[test]
    fn range_conversion() {
        let r = rope("const x = 1;");
        let range = Range {
            start: Position {
                line: 0,
                character: 6,
            },
            end: Position {
                line: 0,
                character: 7,
            },
        };
        assert_eq!(lsp_range_to_byte_range(&r, range).expect("range"), (6, 7));
    }
}
