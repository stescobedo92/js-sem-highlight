# Changelog

All notable changes to **JS Semantic Highlight** are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.1.1 (2026-05-08)

### Fixed
- **`semantic_tokens/full/delta` handler now responds correctly.** The server
  advertised the capability `semanticTokensProvider.full = { delta: true }`
  but the handler was missing, so VS Code received `Method not found (-32601)`
  on every edit. The handler is now wired to the existing
  `cache::compute_delta_edit` algorithm and returns either a
  `SemanticTokensDelta` (when the client provides a known `previousResultId`)
  or a full `SemanticTokens` set (graceful degradation).
- **`textDocument/didSave` no longer produces spurious WARN logs.** Added an
  explicit no-op handler and declared `textDocumentSync.save = { includeText: false }`
  so well-behaved clients omit the buffer body and tower-lsp's default impl
  no longer warns.

### Changed
- `textDocumentSync` capability is now declared as `TextDocumentSyncOptions`
  (extended form) rather than `TextDocumentSyncKind`. LSP 3.6+ clients
  consume this without changes; the capabilities surface is otherwise
  identical.

### Added
- Three E2E regression tests:
  - `e2e_semantic_tokens_full_delta_returns_proper_delta`
  - `e2e_did_save_does_not_error_or_kill_server`
  - `e2e_initialize_advertises_save_capability`

### Compatibility
This is a fully backward-compatible patch release. Reinstall via:

```bash
code --uninstall-extension stescobedo.js-sem-highlight
code --install-extension client/js-sem-highlight-0.1.1.vsix
```

## 0.1.0 (2026-05-08)

Initial release. See `openspec/changes/archive/2026-05-08-add-js-semantic-highlighter-lsp/`
for the originating change proposal and design notes.
