//! Modelo de roles e identificadores resueltos.
//!
//! La salida de `analyze()` se modela en dos vectores:
//!
//! - `bindings`: cada *declaración* (parámetro, `const`, `function`, `class`, etc.).
//! - `references`: cada *uso* de un identificador (incluyendo no resueltos).
//!
//! Un consumidor (semantic-token-emitter) consulta por byte offset para
//! decidir qué tipo y modifiers emitir.

/// Rango byte (cerrado-abierto) en la fuente original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

impl ByteRange {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn contains_offset(self, byte: u32) -> bool {
        byte >= self.start && byte < self.end
    }
}

/// Rol de un identificador resuelto.
///
/// 15 variantes según `scope-resolver/spec.md` Requirement: Clasificación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentifierRole {
    Parameter,
    LocalVariable,
    LocalConstant,
    ImportedBinding,
    ExportedBinding,
    Function,
    Class,
    TypeAlias,
    Interface,
    Enum,
    EnumMember,
    Property,
    Method,
    Global,
    Unresolved,
}

impl IdentifierRole {
    /// Si el rol corresponde a una declaración (no a una referencia).
    #[must_use]
    pub const fn is_declaration_kind(self) -> bool {
        matches!(
            self,
            Self::Parameter
                | Self::LocalVariable
                | Self::LocalConstant
                | Self::Function
                | Self::Class
                | Self::TypeAlias
                | Self::Interface
                | Self::Enum
                | Self::EnumMember
                | Self::ImportedBinding
                | Self::ExportedBinding
        )
    }
}

/// Información de una declaración (binding) en el código fuente.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct IdentifierBinding {
    /// Nombre del símbolo.
    pub name: String,
    /// Rol asignado en la clasificación.
    pub role: IdentifierRole,
    /// Rango del identificador (no del statement completo).
    pub range: ByteRange,
    /// Indica si la regla de no-usados marcará este binding como `unused`.
    pub is_unused: bool,
    /// `true` si la declaración usa `const` (modifier `readonly`).
    pub is_const: bool,
    /// `true` si la función fue declarada con `async`.
    pub is_async: bool,
    /// `true` si el símbolo ha sido marcado como `@deprecated` (`JSDoc` adyacente).
    pub is_deprecated: bool,
}

/// Información de una referencia (uso) a un identificador.
#[derive(Debug, Clone)]
pub struct ClassifiedReference {
    /// Nombre del identificador.
    pub name: String,
    /// Rol resuelto. `Global`, `Unresolved` o el rol de la declaración.
    pub role: IdentifierRole,
    /// Rango del identificador en la fuente.
    pub range: ByteRange,
    /// Para `Global`: indica si el nombre está en el catálogo `defaultLibrary`.
    pub is_default_library: bool,
    /// `true` si esta referencia escribe (left-hand side de asignación a una
    /// variable previamente declarada).
    pub is_modification: bool,
}
