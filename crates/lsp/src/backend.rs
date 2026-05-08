//! `Backend`: la implementación de `LanguageServer` que orquesta todo.

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use js_sem_parsing::{Document, DocumentError};
use js_sem_rules::{default_registry, AnalysisContext, RuleEmission, RuleRegistry};
use js_sem_scopes::{analyze, CancellationToken, ScopeMap};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::{
    DiagnosticOptions, DiagnosticServerCapabilities, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentDiagnosticReportResult, FullDocumentDiagnosticReport, InitializeParams,
    InitializeResult, InitializedParams, MessageType, RelatedFullDocumentDiagnosticReport,
    SaveOptions, SemanticTokens, SemanticTokensDelta, SemanticTokensDeltaParams,
    SemanticTokensFullDeltaResult, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url, WorkDoneProgressOptions,
};
use tower_lsp::{Client, LanguageServer};

use crate::cache::{CachedTokenSet, ResultIdGenerator};
use crate::config::Config;
use crate::tokens::{
    encode_tokens, legend, token_type_index, EmittedToken, Modifiers, TokenModifierLegend,
};

/// Estado por documento mantenido por el servidor.
struct DocumentState {
    document: Document,
    scope_map: Option<Arc<ScopeMap>>,
    cached_tokens: Option<CachedTokenSet>,
}

pub struct Backend {
    pub client: Client,
    documents: DashMap<Url, DocumentState>,
    config: Arc<RwLock<Config>>,
    rules: Arc<RuleRegistry>,
    id_gen: Arc<ResultIdGenerator>,
}

impl Backend {
    /// Crea un nuevo `Backend` con el registro de reglas por defecto.
    #[must_use]
    pub fn new(client: Client) -> Self {
        let mut rules = default_registry();
        rules.lock();
        Self {
            client,
            documents: DashMap::new(),
            config: Arc::new(RwLock::new(Config::default())),
            rules: Arc::new(rules),
            id_gen: Arc::new(ResultIdGenerator::new()),
        }
    }

    /// Reanaliza un documento abriendo scopes (síncrono — ejecutado bajo
    /// `tokio::task::spawn_blocking` por el caller).
    fn run_scope_analysis(document: &Document) -> Result<ScopeMap, js_sem_scopes::AnalyzeError> {
        let source = document.rope.to_string();
        analyze(&source, document.language, &CancellationToken::new())
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // Aplica configuración inicial.
        if let Some(opts) = params.initialization_options {
            let new_cfg = Config::from_init_options(Some(opts));
            *self.config.write().await = new_cfg;
        }

        let semantic_tokens_options = SemanticTokensOptions {
            work_done_progress_options: WorkDoneProgressOptions::default(),
            legend: legend(),
            range: Some(true),
            full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
        };

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        will_save: None,
                        will_save_wait_until: None,
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                    },
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        semantic_tokens_options,
                    ),
                ),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        identifier: Some("js-sem".into()),
                        inter_file_dependencies: false,
                        workspace_diagnostics: false,
                        ..DiagnosticOptions::default()
                    },
                )),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "js-sem-highlight".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "js-sem-highlight initialized")
            .await;
    }

    // ============================================================================
    //   Document sync
    // ============================================================================

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let cfg = self.config.read().await;
        let path = params
            .text_document
            .uri
            .to_file_path()
            .ok()
            .map(PathBuf::from);
        let language_id = params.text_document.language_id.as_str();
        match Document::open_with_detection(
            path.as_deref(),
            Some(language_id),
            params.text_document.version,
            &params.text_document.text,
            cfg.max_file_size_bytes,
        ) {
            Ok(document) => {
                let scope_map = Backend::run_scope_analysis(&document).ok().map(Arc::new);
                self.documents.insert(
                    params.text_document.uri,
                    DocumentState {
                        document,
                        scope_map,
                        cached_tokens: None,
                    },
                );
            }
            Err(DocumentError::FileTooLarge { actual, limit }) => {
                tracing::info!(
                    actual_bytes = actual,
                    limit_bytes = limit,
                    uri = %params.text_document.uri,
                    "file too large; skipping"
                );
            }
            Err(err) => {
                tracing::warn!(error = ?err, uri = %params.text_document.uri, "did_open failed");
            }
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let mut entry = match self.documents.get_mut(&uri) {
            Some(e) => e,
            None => {
                tracing::warn!(uri = %uri, "did_change for unknown document");
                return;
            }
        };
        if let Err(err) = entry
            .document
            .apply_changes(params.text_document.version, &params.content_changes)
        {
            tracing::warn!(error = ?err, uri = %uri, "apply_changes failed");
            return;
        }
        // Re-analizar scopes (simplificado: sin debounce real, lo haremos en
        // un job task del servidor cuando integre la timer-task).
        let new_map = Backend::run_scope_analysis(&entry.document)
            .ok()
            .map(Arc::new);
        entry.scope_map = new_map;
        // Invalidar cache de tokens.
        entry.cached_tokens = None;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // No-op explícito para satisfacer la capability `save: { includeText: false }`
        // anunciada en `initialize`. El re-análisis ya ocurre en `did_change`,
        // así que no hace falta volver a parsear aquí. Solo dejamos un trace
        // a nivel debug para diagnóstico opcional.
        tracing::debug!(uri = %params.text_document.uri, "did_save received");
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // VS Code envía el settings dentro de `params.settings.<section>`. La
        // sección la define el cliente: nuestro contrato es `js-sem.*`.
        let new_cfg = Config::from_init_options(Some(params.settings));
        *self.config.write().await = new_cfg;
        self.client
            .log_message(MessageType::INFO, "configuration updated")
            .await;
    }

    // ============================================================================
    //   Semantic tokens
    // ============================================================================

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let mut entry = match self.documents.get_mut(&uri) {
            Some(e) => e,
            None => return Ok(None),
        };
        let tokens = compute_semantic_tokens(&entry.document, entry.scope_map.as_deref());
        let encoded = encode_tokens(&tokens);
        let result_id = self.id_gen.next_id();
        entry.cached_tokens = Some(CachedTokenSet {
            result_id: result_id.clone(),
            tokens: encoded.clone(),
        });
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: Some(result_id),
            data: encoded,
        })))
    }

    async fn semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> LspResult<Option<SemanticTokensFullDeltaResult>> {
        let uri = params.text_document.uri;
        let mut entry = match self.documents.get_mut(&uri) {
            Some(e) => e,
            None => return Ok(None),
        };

        // Calcular tokens actuales y nueva resultId siempre.
        let current = encode_tokens(&compute_semantic_tokens(
            &entry.document,
            entry.scope_map.as_deref(),
        ));
        let new_result_id = self.id_gen.next_id();

        // Si el cliente trajo un previousResultId que conocemos, intentar
        // computar el delta. Si no, degradar a respuesta full.
        let response = match entry.cached_tokens.as_ref() {
            Some(prev) if prev.result_id == params.previous_result_id => {
                if let Some(edit) = crate::cache::compute_delta_edit(&prev.tokens, &current) {
                    SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
                        result_id: Some(new_result_id.clone()),
                        edits: vec![edit],
                    })
                } else {
                    // Sin cambios reales: delta vacío.
                    SemanticTokensFullDeltaResult::TokensDelta(SemanticTokensDelta {
                        result_id: Some(new_result_id.clone()),
                        edits: vec![],
                    })
                }
            }
            _ => SemanticTokensFullDeltaResult::Tokens(SemanticTokens {
                result_id: Some(new_result_id.clone()),
                data: current.clone(),
            }),
        };

        // Actualizar cache con el set actual y el nuevo id.
        entry.cached_tokens = Some(CachedTokenSet {
            result_id: new_result_id,
            tokens: current,
        });

        Ok(Some(response))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> LspResult<Option<SemanticTokensRangeResult>> {
        let uri = params.text_document.uri;
        let entry = match self.documents.get(&uri) {
            Some(e) => e,
            None => return Ok(None),
        };
        let all = compute_semantic_tokens(&entry.document, entry.scope_map.as_deref());
        let in_range: Vec<EmittedToken> = all
            .into_iter()
            .filter(|t| {
                let after_start = t.line > params.range.start.line
                    || (t.line == params.range.start.line
                        && t.character >= params.range.start.character);
                let before_end = t.line < params.range.end.line
                    || (t.line == params.range.end.line
                        && t.character < params.range.end.character);
                after_start && before_end
            })
            .collect();
        Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
            result_id: None,
            data: encode_tokens(&in_range),
        })))
    }

    // ============================================================================
    //   Diagnostics (pull-based)
    // ============================================================================

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> LspResult<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri;
        let entry = match self.documents.get(&uri) {
            Some(e) => e,
            None => {
                return Ok(DocumentDiagnosticReportResult::Report(
                    DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                        related_documents: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: None,
                            items: vec![],
                        },
                    }),
                ));
            }
        };

        let cfg = self.config.read().await;
        let source = entry.document.rope.to_string();
        let ctx = AnalysisContext {
            source: &source,
            scope_map: entry.scope_map.as_deref(),
            filename: uri.path(),
        };

        let mut diagnostics = Vec::new();
        for rule in self.rules.iter() {
            let severity = cfg.rules.effective(rule.id(), rule.default_severity());
            if severity.is_off() {
                continue;
            }
            let target_severity = severity.to_diagnostic_severity();
            // Envolver `check` en `catch_unwind`: si una regla panicea,
            // capturamos, logueamos, y devolvemos `InternalError` sin matar
            // el server. Cumple `Scenario: Panic durante semanticTokens`.
            let emissions = match std::panic::catch_unwind(AssertUnwindSafe(|| rule.check(&ctx))) {
                Ok(v) => v,
                Err(payload) => {
                    let msg = panic_message(&payload);
                    tracing::error!(
                        rule = rule.id(),
                        message = %msg,
                        "rule panicked while checking; returning InternalError"
                    );
                    return Err(tower_lsp::jsonrpc::Error::internal_error());
                }
            };
            for emission in emissions {
                if let RuleEmission::Diagnostic(d) = emission {
                    diagnostics.push(tower_lsp::lsp_types::Diagnostic {
                        range: d.range,
                        severity: target_severity,
                        code: Some(tower_lsp::lsp_types::NumberOrString::String(d.code)),
                        code_description: None,
                        source: Some("js-sem".into()),
                        message: d.message,
                        related_information: None,
                        tags: if d.tags.is_empty() {
                            None
                        } else {
                            Some(d.tags)
                        },
                        data: None,
                    });
                }
            }
        }

        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: diagnostics,
                },
            }),
        ))
    }

    // ============================================================================
    //   Lifecycle
    // ============================================================================

    async fn shutdown(&self) -> LspResult<()> {
        self.documents.clear();
        Ok(())
    }
}

// ============================================================================
//   Token computation: combina parsing + scopes + reglas
// ============================================================================

/// Genera los semantic tokens "absolutos" de un documento.
///
/// MVP:
/// - Recorre el árbol tree-sitter para tokens estructurales (keyword, string,
///   number, regex, comment, operator).
/// - Cruza con el `ScopeMap` para clasificar identificadores y agregar
///   modifiers (`declaration`, `readonly`, `defaultLibrary`, `unused`).
fn compute_semantic_tokens(document: &Document, scope_map: Option<&ScopeMap>) -> Vec<EmittedToken> {
    use js_sem_parsing::tokens_in_range;
    use tower_lsp::lsp_types::{Position, Range};

    let total_lines = document.rope.len_lines();
    let last_line_chars = if total_lines > 0 {
        document.rope.line(total_lines - 1).len_chars()
    } else {
        0
    };
    let full_range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: u32::try_from(total_lines.saturating_sub(1)).unwrap_or(u32::MAX),
            character: u32::try_from(last_line_chars).unwrap_or(u32::MAX),
        },
    };

    let spans = match tokens_in_range(document.tree(), &document.rope, full_range) {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(error = ?err, "tokens_in_range failed");
            return vec![];
        }
    };
    let mut emitted = Vec::with_capacity(spans.len());
    let source = document.rope.to_string();

    for span in spans {
        let Some((token_type, modifiers)) = map_tree_sitter_kind(
            &span.kind,
            &source,
            span.byte_start,
            span.byte_end,
            scope_map,
        ) else {
            continue;
        };
        let length = u32::try_from(span.byte_end - span.byte_start).unwrap_or(u32::MAX);
        emitted.push(EmittedToken {
            line: span.range.start.line,
            character: span.range.start.character,
            length,
            token_type: token_type.index(),
            token_modifiers_bitset: modifiers.bits(),
        });
    }
    // tree-sitter cursor produce nodos en orden; pero nuestros filtros pueden
    // dejar gaps. Ordenar por (line, char) garantiza la precondición de
    // `encode_tokens`.
    emitted.sort_by_key(|t| (t.line, t.character));
    emitted
}

/// Mapea el `kind` de un nodo tree-sitter a `(TokenTypeLegend, Modifiers)`.
///
/// Devuelve `None` para nodos que no queremos emitir (identificadores
/// internos, `program`, etc.).
fn map_tree_sitter_kind(
    kind: &str,
    source: &str,
    byte_start: usize,
    byte_end: usize,
    scope_map: Option<&ScopeMap>,
) -> Option<(crate::tokens::TokenTypeLegend, Modifiers)> {
    use crate::tokens::TokenTypeLegend as T;

    let modifiers = Modifiers::new();

    let token_type = match kind {
        // Keywords
        "const" | "let" | "var" | "function" | "class" | "interface" | "type" | "enum" | "if"
        | "else" | "for" | "while" | "do" | "return" | "break" | "continue" | "throw" | "try"
        | "catch" | "finally" | "switch" | "case" | "default" | "import" | "export" | "from"
        | "as" | "new" | "delete" | "typeof" | "instanceof" | "in" | "of" | "void" | "yield"
        | "async" | "await" | "this" | "super" | "true" | "false" | "null" | "undefined" => {
            T::Keyword
        }
        "string" | "string_fragment" | "template_string" => T::String,
        "number" => T::Number,
        "regex" | "regex_pattern" | "regex_flags" => T::Regexp,
        "comment" => T::Comment,
        "decorator" => T::Decorator,
        "identifier"
        | "property_identifier"
        | "shorthand_property_identifier"
        | "type_identifier" => {
            // Si tenemos scope-map, intentar clasificar por byte offset.
            if let Some(map) = scope_map {
                let byte_u32 = u32::try_from(byte_start).unwrap_or(u32::MAX);
                if let Some(binding) = map.binding_at(byte_u32) {
                    return Some((token_type_index(binding.role), binding_modifiers(binding)));
                }
                if let Some(reference) = map.reference_at(byte_u32) {
                    let mut mods = Modifiers::new();
                    if reference.is_default_library {
                        mods = mods.with(TokenModifierLegend::DefaultLibrary);
                    }
                    if reference.is_modification {
                        mods = mods.with(TokenModifierLegend::Modification);
                    }
                    return Some((token_type_index(reference.role), mods));
                }
            }
            // Fallback: identificador desconocido → variable.
            T::Variable
        }
        _ => return None,
    };

    let _ = (source, byte_end);
    Some((token_type, modifiers))
}

/// Extrae un mensaje legible del payload de un panic.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<panic with non-string payload>".to_string()
    }
}

fn binding_modifiers(binding: &js_sem_scopes::IdentifierBinding) -> Modifiers {
    let mut m = Modifiers::new().with(TokenModifierLegend::Declaration);
    if binding.is_const {
        m = m.with(TokenModifierLegend::Readonly);
    }
    if binding.is_unused {
        m = m.with(TokenModifierLegend::Unused);
    }
    if binding.is_async {
        m = m.with(TokenModifierLegend::Async);
    }
    if binding.is_deprecated {
        m = m.with(TokenModifierLegend::Deprecated);
    }
    m
}
