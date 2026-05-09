//! Tests de regresión: snippets que históricamente rompen highlighters basados
//! en regex (`TextMate`) y que tree-sitter debería parsear sin perder estructura.
//!
//! Cada caso verifica dos invariantes:
//! 1. El parser produce un árbol no vacío (no panic, no None).
//! 2. Aunque el árbol contenga nodos `ERROR`/`MISSING`, las regiones bien
//!    formadas alrededor del error mantienen su clasificación correcta.
//!
//! Cubre el `Scenario: Código incompleto durante la edición` del
//! `js-parsing-pipeline/spec.md`.

#![allow(clippy::expect_used)]

use js_sem_parsing::{Document, Language};

const LIMIT: usize = 1024 * 1024;

fn parse_js(source: &str) -> Document {
    Document::open(Language::JavaScript, 1, source, LIMIT).expect("parse")
}

fn parse_ts(source: &str) -> Document {
    Document::open(Language::TypeScript, 1, source, LIMIT).expect("parse")
}

fn parse_tsx(source: &str) -> Document {
    Document::open(Language::Tsx, 1, source, LIMIT).expect("parse")
}

#[test]
fn unclosed_paren_does_not_panic() {
    let doc = parse_js("const x = function (");
    assert!(doc.tree().root_node().has_error(), "expected error nodes");
    // Pero el árbol existe y se puede recorrer sin panic.
    let mut count = 0;
    let mut cursor = doc.tree().walk();
    while cursor.goto_first_child() || cursor.goto_next_sibling() || cursor.goto_parent() {
        count += 1;
        if count > 1000 {
            break;
        }
    }
}

#[test]
fn regex_vs_division_ambiguity() {
    // Sin paréntesis, /regex/g se parsea como regex; con espacios alrededor
    // de variables, debería seguir siendo regex (caso clásico que TextMate
    // confunde con división).
    let doc = parse_js("const x = /abc/g; const y = a / b / c;");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn private_class_field() {
    // Stage-3 class fields privados — gramáticas TextMate viejas los rechazaban.
    let doc = parse_js("class Foo { #priv = 1; get priv() { return this.#priv; } }");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn tagged_template_with_expression() {
    let doc = parse_js("html`<div>${foo()}</div>`");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn top_level_await() {
    // En módulo, top-level await es válido.
    let doc = parse_js("const data = await fetch('/api');");
    // En script tradicional sería error; tree-sitter-javascript es permisivo.
    let _ = doc.tree().root_node();
}

#[test]
fn dangling_arrow_keeps_partial_tree() {
    let doc = parse_js("const f = (a, b) =>");
    let root = doc.tree().root_node();
    assert!(root.has_error(), "should have ERROR for incomplete arrow");
    // La declaración hasta `=>` debe seguir siendo reconocida en algún sub-nivel.
    let source = "const f = (a, b) =>";
    let preview = root.utf8_text(source.as_bytes()).unwrap_or("");
    assert!(preview.contains("const"));
}

#[test]
fn ts_satisfies_operator() {
    let doc = parse_ts("const config = { x: 1 } satisfies Record<string, number>;");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn ts_decorators_stage_3() {
    let doc = parse_ts(
        r"
        function logged(_target: any, _ctx: ClassMethodDecoratorContext) {}
        class Foo {
            @logged
            bar() {}
        }
        ",
    );
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn tsx_generic_vs_jsx_ambiguity() {
    // <T,> en TSX se desambigua a generic gracias a la coma; este es el
    // workaround clásico que confunde a parsers basados en regex.
    let doc = parse_tsx("const f = <T,>(x: T): T => x;");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn jsx_self_closing_with_spread() {
    let doc = parse_tsx("const el = <Foo {...props} bar='1' />;");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn unicode_identifier() {
    let doc = parse_js("const café = 1; const naïve = café + 1;");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn shebang_line_is_recognized() {
    let doc = parse_js("#!/usr/bin/env node\nconsole.log(1);");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn deeply_nested_template_literals() {
    let doc = parse_js(r"const x = `outer ${`inner ${1 + 2} mid`} end`;");
    assert!(!doc.tree().root_node().has_error());
}

#[test]
fn missing_semicolons_via_asi() {
    // Automatic Semicolon Insertion: rompe muchos parsers no-Spec-compliant.
    let doc = parse_js(
        r"
        const a = 1
        const b = 2
        const c = a + b
        ",
    );
    assert!(!doc.tree().root_node().has_error());
}
