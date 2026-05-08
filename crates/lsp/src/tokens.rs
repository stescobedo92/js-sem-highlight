//! Leyenda de semantic tokens y codificación delta-relativa LSP 3.17.
//!
//! Estructura: cuatro piezas
//! 1. `TokenTypeLegend`: enum con los 21 tipos del spec, mapeable a su índice.
//! 2. `TokenModifierLegend`: enum con los 11 modifiers + bitmask helpers.
//! 3. `legend()`: produce la `SemanticTokensLegend` declarada en `initialize`.
//! 4. `encode_tokens()`: convierte un `Vec<EmittedToken>` ordenado por posición
//!    a la representación delta-relativa de LSP (5 enteros por token).

use js_sem_scopes::IdentifierRole;
use tower_lsp::lsp_types::{SemanticToken, SemanticTokensLegend};

// ============================================================================
//   Token Types (21)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TokenTypeLegend {
    Namespace = 0,
    Class = 1,
    Enum = 2,
    Interface = 3,
    Type = 4,
    TypeParameter = 5,
    Parameter = 6,
    Variable = 7,
    Property = 8,
    EnumMember = 9,
    Function = 10,
    Method = 11,
    Macro = 12,
    Keyword = 13,
    Modifier = 14,
    Comment = 15,
    String = 16,
    Number = 17,
    Regexp = 18,
    Operator = 19,
    Decorator = 20,
}

impl TokenTypeLegend {
    /// Nombre LSP estándar (debe coincidir con la convención `SemanticTokenType`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::TypeParameter => "typeParameter",
            Self::Parameter => "parameter",
            Self::Variable => "variable",
            Self::Property => "property",
            Self::EnumMember => "enumMember",
            Self::Function => "function",
            Self::Method => "method",
            Self::Macro => "macro",
            Self::Keyword => "keyword",
            Self::Modifier => "modifier",
            Self::Comment => "comment",
            Self::String => "string",
            Self::Number => "number",
            Self::Regexp => "regexp",
            Self::Operator => "operator",
            Self::Decorator => "decorator",
        }
    }

    /// Índice numérico tal como aparece en la leyenda LSP.
    #[must_use]
    pub const fn index(self) -> u32 {
        self as u32
    }

    /// Lista completa en orden de la leyenda.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Namespace,
            Self::Class,
            Self::Enum,
            Self::Interface,
            Self::Type,
            Self::TypeParameter,
            Self::Parameter,
            Self::Variable,
            Self::Property,
            Self::EnumMember,
            Self::Function,
            Self::Method,
            Self::Macro,
            Self::Keyword,
            Self::Modifier,
            Self::Comment,
            Self::String,
            Self::Number,
            Self::Regexp,
            Self::Operator,
            Self::Decorator,
        ]
    }
}

// ============================================================================
//   Token Modifiers (11)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TokenModifierLegend {
    Declaration = 0,
    Definition = 1,
    Readonly = 2,
    Static = 3,
    Deprecated = 4,
    Abstract = 5,
    Async = 6,
    Modification = 7,
    Documentation = 8,
    DefaultLibrary = 9,
    Unused = 10,
}

impl TokenModifierLegend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::Definition => "definition",
            Self::Readonly => "readonly",
            Self::Static => "static",
            Self::Deprecated => "deprecated",
            Self::Abstract => "abstract",
            Self::Async => "async",
            Self::Modification => "modification",
            Self::Documentation => "documentation",
            Self::DefaultLibrary => "defaultLibrary",
            Self::Unused => "unused",
        }
    }

    #[must_use]
    pub const fn bit(self) -> u32 {
        1u32 << (self as u32)
    }

    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Declaration,
            Self::Definition,
            Self::Readonly,
            Self::Static,
            Self::Deprecated,
            Self::Abstract,
            Self::Async,
            Self::Modification,
            Self::Documentation,
            Self::DefaultLibrary,
            Self::Unused,
        ]
    }
}

/// Wrapper alrededor de un bitmask de modifiers para ergonomía.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers(u32);

impl Modifiers {
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn with(self, m: TokenModifierLegend) -> Self {
        Self(self.0 | m.bit())
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, m: TokenModifierLegend) -> bool {
        (self.0 & m.bit()) != 0
    }
}

// ============================================================================
//   Legend constructor
// ============================================================================

/// Construye la `SemanticTokensLegend` que se devuelve en `initialize`.
///
/// El orden DEBE coincidir con los índices declarados en `TokenTypeLegend`
/// y `TokenModifierLegend` para que los bitmasks/índices sean consistentes.
#[must_use]
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TokenTypeLegend::all()
            .iter()
            .map(|t| t.as_str().into())
            .collect(),
        token_modifiers: TokenModifierLegend::all()
            .iter()
            .map(|m| m.as_str().into())
            .collect(),
    }
}

/// Mapea un `IdentifierRole` al `TokenTypeLegend` correspondiente.
#[must_use]
pub fn token_type_index(role: IdentifierRole) -> TokenTypeLegend {
    match role {
        IdentifierRole::Parameter => TokenTypeLegend::Parameter,
        IdentifierRole::LocalVariable | IdentifierRole::LocalConstant => TokenTypeLegend::Variable,
        IdentifierRole::ImportedBinding | IdentifierRole::ExportedBinding => {
            TokenTypeLegend::Variable
        }
        IdentifierRole::Function => TokenTypeLegend::Function,
        IdentifierRole::Class => TokenTypeLegend::Class,
        IdentifierRole::TypeAlias => TokenTypeLegend::Type,
        IdentifierRole::Interface => TokenTypeLegend::Interface,
        IdentifierRole::Enum => TokenTypeLegend::Enum,
        IdentifierRole::EnumMember => TokenTypeLegend::EnumMember,
        IdentifierRole::Property => TokenTypeLegend::Property,
        IdentifierRole::Method => TokenTypeLegend::Method,
        IdentifierRole::Global | IdentifierRole::Unresolved => TokenTypeLegend::Variable,
    }
}

// ============================================================================
//   Delta-relative encoding
// ============================================================================

/// Token "absoluto" que el emisor produce.
///
/// Antes de mandarlo al cliente, debe convertirse a la forma delta-relativa
/// (line offset desde token previo, char offset desde token previo si están
/// en la misma línea, length, type, modifiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmittedToken {
    pub line: u32,
    pub character: u32,
    pub length: u32,
    pub token_type: u32,
    pub token_modifiers_bitset: u32,
}

/// Codifica una lista de tokens absolutos a la representación delta-relativa
/// de LSP 3.17.
///
/// Precondiciones:
/// - `tokens` está ordenado por (line, character) ascendente.
/// - No hay solapamientos: para cualquier i, `tokens[i]` termina antes de que
///   empiece `tokens[i+1]` (o `i+1` está en una línea posterior).
///
/// El primer token tiene `deltaLine = line` y `deltaStartChar = character`.
#[must_use]
pub fn encode_tokens(tokens: &[EmittedToken]) -> Vec<SemanticToken> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut prev_line: u32 = 0;
    let mut prev_char: u32 = 0;
    for (i, t) in tokens.iter().enumerate() {
        let (delta_line, delta_start) = if i == 0 {
            (t.line, t.character)
        } else if t.line == prev_line {
            (0, t.character - prev_char)
        } else {
            (t.line - prev_line, t.character)
        };
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: t.token_type,
            token_modifiers_bitset: t.token_modifiers_bitset,
        });
        prev_line = t.line;
        prev_char = t.character;
    }
    out
}

// ============================================================================
//   Tests
// ============================================================================

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
    fn legend_has_21_types_and_11_modifiers() {
        let l = legend();
        assert_eq!(l.token_types.len(), 21);
        assert_eq!(l.token_modifiers.len(), 11);
    }

    #[test]
    fn legend_is_deterministic() {
        let a = legend();
        let b = legend();
        assert_eq!(a.token_types, b.token_types);
        assert_eq!(a.token_modifiers, b.token_modifiers);
    }

    #[test]
    fn token_type_indices_are_stable() {
        assert_eq!(TokenTypeLegend::Variable.index(), 7);
        assert_eq!(TokenTypeLegend::Keyword.index(), 13);
        assert_eq!(TokenTypeLegend::Decorator.index(), 20);
    }

    #[test]
    fn modifier_bits_are_unique() {
        let mut accum = 0u32;
        for m in TokenModifierLegend::all() {
            assert_eq!(accum & m.bit(), 0, "duplicate bit at {:?}", m);
            accum |= m.bit();
        }
    }

    #[test]
    fn modifiers_combine_correctly() {
        let m = Modifiers::new()
            .with(TokenModifierLegend::Declaration)
            .with(TokenModifierLegend::Readonly);
        assert!(m.contains(TokenModifierLegend::Declaration));
        assert!(m.contains(TokenModifierLegend::Readonly));
        assert!(!m.contains(TokenModifierLegend::Async));
    }

    #[test]
    fn encode_first_token_is_absolute() {
        let toks = vec![EmittedToken {
            line: 5,
            character: 12,
            length: 3,
            token_type: TokenTypeLegend::Variable.index(),
            token_modifiers_bitset: 0,
        }];
        let enc = encode_tokens(&toks);
        assert_eq!(enc[0].delta_line, 5);
        assert_eq!(enc[0].delta_start, 12);
    }

    #[test]
    fn encode_same_line_uses_char_delta() {
        let toks = vec![
            EmittedToken {
                line: 0,
                character: 0,
                length: 5,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            EmittedToken {
                line: 0,
                character: 6,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let enc = encode_tokens(&toks);
        assert_eq!(enc[1].delta_line, 0);
        assert_eq!(enc[1].delta_start, 6);
    }

    #[test]
    fn encode_new_line_resets_char() {
        let toks = vec![
            EmittedToken {
                line: 0,
                character: 10,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
            EmittedToken {
                line: 2,
                character: 4,
                length: 1,
                token_type: 0,
                token_modifiers_bitset: 0,
            },
        ];
        let enc = encode_tokens(&toks);
        assert_eq!(enc[1].delta_line, 2);
        assert_eq!(enc[1].delta_start, 4);
    }

    #[test]
    fn role_to_token_type_mapping() {
        assert_eq!(
            token_type_index(IdentifierRole::Parameter),
            TokenTypeLegend::Parameter
        );
        assert_eq!(
            token_type_index(IdentifierRole::Class),
            TokenTypeLegend::Class
        );
        assert_eq!(
            token_type_index(IdentifierRole::Function),
            TokenTypeLegend::Function
        );
        assert_eq!(
            token_type_index(IdentifierRole::TypeAlias),
            TokenTypeLegend::Type
        );
        assert_eq!(
            token_type_index(IdentifierRole::EnumMember),
            TokenTypeLegend::EnumMember
        );
    }
}
