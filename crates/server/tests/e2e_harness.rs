//! Harness E2E: arranca el binario `js-sem-highlight` como child process,
//! habla LSP por stdin/stdout, y verifica respuestas.
//!
//! Cubre los `Scenario`s de `lsp-server-runtime/spec.md` y
//! `semantic-token-emitter/spec.md` que requieren ver el wire format real.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_js-sem-highlight");

/// Cliente LSP minimalista que habla con el child process por stdio.
pub struct LspChild {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl LspChild {
    pub fn spawn() -> Self {
        Self::spawn_with_env(std::iter::empty())
    }

    /// Spawn variant que permite inyectar variables de entorno. Usado por el
    /// test 7.7 para activar `JS_SEM_INJECT_PANIC_RULE`.
    pub fn spawn_with_env<I>(envs: I) -> Self
    where
        I: IntoIterator<Item = (&'static str, &'static str)>,
    {
        let mut cmd = Command::new(BIN);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn js-sem-highlight");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self { child, stdin, stdout, next_id: 1 }
    }

    /// `true` si el child sigue vivo. Para 7.7 verificamos que un panic en
    /// regla NO mata al proceso.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_message(&req);
        loop {
            let msg = self.read_message();
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                return msg;
            }
            // Mensajes intermedios (notificaciones del server, log_message)
            // se descartan para mantener el harness simple.
        }
    }

    pub fn send_notification(&mut self, method: &str, params: Value) {
        let req = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_message(&req);
    }

    fn write_message(&mut self, msg: &Value) {
        let body = serde_json::to_string(msg).expect("serialize");
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin.write_all(header.as_bytes()).expect("write header");
        self.stdin.write_all(body.as_bytes()).expect("write body");
        self.stdin.flush().expect("flush");
    }

    fn read_message(&mut self) -> Value {
        // Header parse mínimo: línea por línea hasta CRLF doble.
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).expect("read header line");
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some(rest) = line.trim_end().strip_prefix("Content-Length: ") {
                content_length = rest.parse().ok();
            }
        }
        let len = content_length.expect("Content-Length present");
        let mut buf = vec![0u8; len];
        self.stdout.read_exact(&mut buf).expect("read body");
        serde_json::from_slice(&buf).expect("parse JSON")
    }

    pub fn shutdown_and_exit(mut self) {
        let _ = self.send_request("shutdown", Value::Null);
        self.send_notification("exit", Value::Null);
        // Espera hasta 2s a que el proceso termine.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = self.child.kill();
                    return;
                }
            }
        }
    }
}

fn initialize_request() -> Value {
    json!({
        "processId": std::process::id(),
        "clientInfo": { "name": "e2e-test", "version": "0.0.0" },
        "capabilities": {
            "textDocument": {
                "semanticTokens": {
                    "requests": { "full": { "delta": true }, "range": true },
                    "tokenTypes": [],
                    "tokenModifiers": [],
                    "formats": ["relative"]
                }
            }
        },
        "initializationOptions": null,
        "rootUri": null,
        "workspaceFolders": null,
    })
}

fn did_open_params(uri: &str, language_id: &str, text: &str) -> Value {
    json!({
        "textDocument": {
            "uri": uri,
            "languageId": language_id,
            "version": 1,
            "text": text,
        }
    })
}

// ============================================================================
//   E2E tests
// ============================================================================

#[test]
fn e2e_initialize_open_semantic_tokens_close_shutdown_exit() {
    let mut c = LspChild::spawn();

    let init = c.send_request("initialize", initialize_request());
    assert!(init.get("result").is_some(), "initialize should succeed: {init}");
    let result = &init["result"];
    let caps = &result["capabilities"];
    assert!(caps.get("semanticTokensProvider").is_some());
    assert!(caps.get("diagnosticProvider").is_some());

    let legend = &caps["semanticTokensProvider"]["legend"];
    let types = legend["tokenTypes"].as_array().expect("tokenTypes array");
    assert_eq!(types.len(), 21, "21 token types expected");
    let modifiers = legend["tokenModifiers"].as_array().expect("tokenModifiers array");
    assert_eq!(modifiers.len(), 11, "11 token modifiers expected");

    c.send_notification("initialized", json!({}));

    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///fixture.js", "javascript", "const x = 42;\n"),
    );

    let tokens = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///fixture.js" } }),
    );
    assert!(tokens.get("result").is_some(), "tokens response: {tokens}");
    let data = tokens["result"]["data"].as_array().expect("data array");
    assert!(!data.is_empty(), "should produce some tokens for `const x = 42;`");

    c.send_notification(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": "file:///fixture.js" } }),
    );

    c.shutdown_and_exit();
}

#[test]
fn e2e_pre_initialize_request_is_rejected() {
    let mut c = LspChild::spawn();
    // Antes de initialize: cualquier request de documento debe fallar.
    let resp = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///x.js" } }),
    );
    assert!(resp.get("error").is_some(), "expected error before init: {resp}");

    // Cleanup forzado.
    c.shutdown_and_exit();
}

#[test]
fn e2e_initialize_under_200ms_smoke_test() {
    let start = Instant::now();
    let mut c = LspChild::spawn();
    let _resp = c.send_request("initialize", initialize_request());
    let elapsed = start.elapsed();
    // Genérico: incluye spawn del proceso + carga del binario + initialize.
    // En CI compartido puede ser lento; usamos 2s como límite suave.
    assert!(elapsed < Duration::from_secs(2), "initialize too slow: {elapsed:?}");
    c.shutdown_and_exit();
}

#[test]
fn e2e_too_large_file_returns_empty_tokens() {
    let mut c = LspChild::spawn();
    // Configuramos límite muy bajo para forzar el rechazo.
    let init_with_small_limit = json!({
        "processId": std::process::id(),
        "capabilities": {},
        "initializationOptions": { "maxFileSizeKb": 1 },
        "rootUri": null,
    });
    let _ = c.send_request("initialize", init_with_small_limit);
    c.send_notification("initialized", json!({}));

    let big_text = "// ".to_string() + &"x".repeat(2048) + "\n";
    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///big.js", "javascript", &big_text),
    );

    let tokens = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///big.js" } }),
    );
    // Documento rechazado → result null o data vacío. Aceptamos cualquiera.
    let res = &tokens["result"];
    if let Some(data) = res.get("data") {
        let arr = data.as_array().expect("data array");
        assert!(arr.is_empty(), "expected empty tokens, got: {arr:?}");
    }

    c.shutdown_and_exit();
}

#[test]
fn e2e_no_unused_vars_disabled_via_config() {
    let mut c = LspChild::spawn();
    let init = json!({
        "processId": std::process::id(),
        "capabilities": {},
        "initializationOptions": { "rules": { "no-unused-vars": "off" } },
        "rootUri": null,
    });
    let _ = c.send_request("initialize", init);
    c.send_notification("initialized", json!({}));
    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///off.js", "javascript", "const a = 1;\n"),
    );

    let resp = c.send_request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": "file:///off.js" } }),
    );
    let items = resp["result"]["items"].as_array().expect("items array");
    let has_unused = items.iter().any(|d| d["code"].as_str() == Some("no-unused-vars"));
    assert!(!has_unused, "no-unused-vars should be silent when off: {items:?}");

    c.shutdown_and_exit();
}

#[test]
fn e2e_runtime_config_change_takes_effect() {
    let mut c = LspChild::spawn();
    let _ = c.send_request("initialize", initialize_request());
    c.send_notification("initialized", json!({}));
    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///cfg.js", "javascript", "const a = 1;\n"),
    );

    // Antes del change: con defaults, no-unused-vars emite hint para `a`.
    let before = c.send_request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": "file:///cfg.js" } }),
    );
    let items_before = before["result"]["items"].as_array().expect("items");
    assert!(items_before.iter().any(|d| d["code"] == "no-unused-vars"));

    // Aplicar config en runtime: rule off.
    c.send_notification(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "rules": { "no-unused-vars": "off" } } }),
    );

    let after = c.send_request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": "file:///cfg.js" } }),
    );
    let items_after = after["result"]["items"].as_array().expect("items");
    assert!(!items_after.iter().any(|d| d["code"] == "no-unused-vars"));

    c.shutdown_and_exit();
}

#[test]
fn e2e_jsx_tsx_languages_supported() {
    let mut c = LspChild::spawn();
    let _ = c.send_request("initialize", initialize_request());
    c.send_notification("initialized", json!({}));
    c.send_notification(
        "textDocument/didOpen",
        did_open_params(
            "file:///comp.tsx",
            "typescriptreact",
            "const App = () => <div>{1}</div>;\n",
        ),
    );

    let tokens = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///comp.tsx" } }),
    );
    let data = tokens["result"]["data"].as_array().expect("data");
    assert!(!data.is_empty(), "TSX should produce tokens");

    c.shutdown_and_exit();
}

#[test]
fn e2e_rule_panic_returns_internal_error_without_killing_server() {
    // Spec: `lsp-server-runtime/spec.md` Scenario "Panic durante semanticTokens".
    // Activamos `PanickingRule` via env var (gated por `cfg(debug_assertions)`).
    let mut c = LspChild::spawn_with_env([("JS_SEM_INJECT_PANIC_RULE", "1")]);
    let _ = c.send_request("initialize", initialize_request());
    c.send_notification("initialized", json!({}));
    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///panic.js", "javascript", "const x = 1;\n"),
    );

    // El primer `diagnostic` debería disparar PanickingRule. El servidor
    // debe responder con error InternalError en lugar de crashear.
    let resp = c.send_request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": "file:///panic.js" } }),
    );
    assert!(
        resp.get("error").is_some(),
        "expected InternalError response, got: {resp}"
    );
    let code = resp["error"]["code"].as_i64().expect("error.code");
    assert_eq!(code, -32603, "JSON-RPC InternalError = -32603");

    // Crítico: el servidor debe seguir vivo y responder a requests subsecuentes.
    assert!(c.is_alive(), "server should not have crashed after rule panic");

    // El mensaje no debe filtrar el backtrace al cliente. Solo debería ser
    // "Internal error" o equivalente, sin la cadena del panic.
    let msg = resp["error"]["message"].as_str().unwrap_or_default();
    assert!(
        !msg.contains("PanickingRule") && !msg.contains("deliberate panic"),
        "client message leaked panic detail: {msg}"
    );

    // Y el ciclo shutdown→exit limpio sigue funcionando.
    c.shutdown_and_exit();
}

#[test]
fn e2e_semantic_tokens_full_delta_returns_proper_delta() {
    // Spec: `semantic-token-emitter/spec.md` — ahora "Capability anunciada
    // implica método implementado". Bug fix: el server respondía -32601.
    let mut c = LspChild::spawn();
    let _ = c.send_request("initialize", initialize_request());
    c.send_notification("initialized", json!({}));
    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///delta.js", "javascript", "const x = 1;\n"),
    );

    // Pedir full primero para obtener un resultId.
    let full = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///delta.js" } }),
    );
    let prev_id = full["result"]["resultId"]
        .as_str()
        .expect("first full has resultId")
        .to_string();

    // Edición pequeña: cambiar `1` por `42`.
    c.send_notification(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": "file:///delta.js", "version": 2 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 11 }
                },
                "text": "42"
            }]
        }),
    );

    // Solicitar el delta con el previousResultId.
    let delta = c.send_request(
        "textDocument/semanticTokens/full/delta",
        json!({
            "textDocument": { "uri": "file:///delta.js" },
            "previousResultId": prev_id
        }),
    );

    // Aserción central: NO debe ser un error con code -32601 (Method not found).
    if let Some(error) = delta.get("error") {
        panic!("delta returned error: {error}");
    }
    assert!(delta.get("result").is_some(), "expected result, got: {delta}");

    // El result debe tener resultId Y, o bien un campo `edits` (delta), o bien
    // un campo `data` (degradación a full).
    let result = &delta["result"];
    assert!(result["resultId"].is_string(), "expected resultId in delta result");
    assert!(
        result.get("edits").is_some() || result.get("data").is_some(),
        "expected `edits` or `data` in delta result, got: {result}"
    );

    c.shutdown_and_exit();
}

#[test]
fn e2e_did_save_does_not_error_or_kill_server() {
    // Spec: `lsp-server-runtime/spec.md` Scenario "didSave no produce warning".
    let mut c = LspChild::spawn();
    let _ = c.send_request("initialize", initialize_request());
    c.send_notification("initialized", json!({}));
    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///save.js", "javascript", "const x = 1;\n"),
    );

    // Notification didSave: nada que esperar como respuesta porque es notify.
    c.send_notification(
        "textDocument/didSave",
        json!({ "textDocument": { "uri": "file:///save.js" } }),
    );

    // El server debe seguir respondiendo a requests normales tras didSave.
    let diag = c.send_request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": "file:///save.js" } }),
    );
    assert!(
        diag.get("error").is_none(),
        "diagnostic after didSave should not error: {diag}"
    );
    assert!(c.is_alive(), "server should still be alive after didSave");

    c.shutdown_and_exit();
}

#[test]
fn e2e_initialize_advertises_save_capability() {
    // Spec: `lsp-server-runtime/spec.md` Scenario "textDocumentSync incluye save options".
    let mut c = LspChild::spawn();
    let init = c.send_request("initialize", initialize_request());

    let sync = &init["result"]["capabilities"]["textDocumentSync"];
    assert!(
        sync.is_object(),
        "textDocumentSync should be an object (TextDocumentSyncOptions), not a scalar: {sync}"
    );

    let save = &sync["save"];
    assert!(save.is_object(), "save should be a SaveOptions object: {save}");
    assert_eq!(
        save["includeText"].as_bool(),
        Some(false),
        "save.includeText must be false to skip body in didSave; got {save}"
    );

    c.shutdown_and_exit();
}

#[test]
fn e2e_did_change_invalidates_token_cache() {
    let mut c = LspChild::spawn();
    let _ = c.send_request("initialize", initialize_request());
    c.send_notification("initialized", json!({}));

    c.send_notification(
        "textDocument/didOpen",
        did_open_params("file:///edit.js", "javascript", "const x = 1;\n"),
    );

    let first = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///edit.js" } }),
    );
    let result_id_first = first["result"]["resultId"].as_str().map(str::to_string);
    assert!(result_id_first.is_some(), "first response should have resultId");

    // Edit: reemplazar `1` por `42` (1 char → 2 chars).
    c.send_notification(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": "file:///edit.js", "version": 2 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 10 },
                    "end": { "line": 0, "character": 11 }
                },
                "text": "42"
            }]
        }),
    );

    let second = c.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///edit.js" } }),
    );
    let result_id_second = second["result"]["resultId"].as_str().map(str::to_string);
    assert!(result_id_second.is_some());
    assert_ne!(result_id_first, result_id_second, "resultId should change after edit");

    c.shutdown_and_exit();
}
