//! Catálogo cerrado de globales reconocidos como `defaultLibrary`.
//!
//! Este es el set ENV-agnóstico: incluye lo que es global tanto en navegador
//! como en Node, más lo que es estándar ECMAScript. Si un usuario tiene un
//! global propio (`gtag`, `_paq`, `dataLayer`), NO va aquí — se clasificará
//! como `Unresolved`, lo cual también es información útil de color.
//!
//! El array DEBE estar ordenado alfabéticamente para que `binary_search`
//! funcione. Hay un test que lo valida.

/// Globales reconocidos como `defaultLibrary`.
pub const DEFAULT_LIBRARY_GLOBALS: &[&str] = &[
    "Array",
    "Buffer",
    "Date",
    "Error",
    "JSON",
    "Map",
    "Math",
    "Object",
    "Promise",
    "RangeError",
    "RegExp",
    "Set",
    "Symbol",
    "TypeError",
    "URL",
    "URLSearchParams",
    "clearInterval",
    "clearTimeout",
    "console",
    "document",
    "fetch",
    "globalThis",
    "process",
    "setInterval",
    "setTimeout",
    "window",
];

/// Indica si un nombre forma parte del catálogo `defaultLibrary`.
#[must_use]
pub fn is_default_library(name: &str) -> bool {
    DEFAULT_LIBRARY_GLOBALS.binary_search(&name).is_ok()
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
    fn catalog_is_sorted() {
        let mut sorted = DEFAULT_LIBRARY_GLOBALS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, DEFAULT_LIBRARY_GLOBALS);
    }
}
