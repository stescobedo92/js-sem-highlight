# JS Semantic Highlight

A Rust LSP server that produces semantic tokens and visual lint hints for
JavaScript, TypeScript, JSX and TSX. Tree-sitter for incremental parsing,
oxc for scope resolution, `tower-lsp` for transport.

Designed to **augment** existing language servers (it does not replace
`tsserver`): it emits richer semantic tokens (parameter vs. local, unused,
defaultLibrary, deprecated, async, etc.) and lint diagnostics that surface
visually rather than only as squiggles.

## Status

This project is the implementation of the OpenSpec change
[`add-js-semantic-highlighter-lsp`](openspec/changes/add-js-semantic-highlighter-lsp/proposal.md).
See `tasks.md` for fine-grained progress.

## Build

```bash
# build the LSP server binary
cargo build --release --bin js-sem-highlight

# run all tests (unit + property + regression + E2E)
cargo test --workspace
```

## VS Code

Install from the marketplace (once published) **or** build locally:

```bash
cargo build --release --bin js-sem-highlight
scripts/package-extension.sh
code --install-extension client/js-sem-highlight-*.vsix
```

Settings (`settings.json`):

```jsonc
{
  "js-sem.enable": true,
  "js-sem.maxFileSizeKb": 512,
  "js-sem.rules": {
    "no-unused-vars": "hint",
    "prefer-const": "hint",
    "no-floating-promises": "hint"
  },
  "js-sem.ignore": ["**/node_modules/**", "**/dist/**"]
}
```

Recommended one-shot color setup:

> Command Palette → **JS Sem: Apply recommended semantic colors**

## Neovim (`nvim-lspconfig`)

```lua
local configs = require('lspconfig.configs')
local lspconfig = require('lspconfig')

if not configs.js_sem_highlight then
  configs.js_sem_highlight = {
    default_config = {
      cmd = { 'js-sem-highlight' },
      filetypes = { 'javascript', 'javascriptreact', 'typescript', 'typescriptreact' },
      root_dir = lspconfig.util.find_git_ancestor,
      single_file_support = true,
      init_options = {
        rules = { ['no-unused-vars'] = 'hint' },
        maxFileSizeKb = 512,
      },
    },
  }
end

lspconfig.js_sem_highlight.setup({})
```

## Helix (`languages.toml`)

```toml
[language-server.js-sem-highlight]
command = "js-sem-highlight"

[[language]]
name = "javascript"
language-servers = ["typescript-language-server", "js-sem-highlight"]

[[language]]
name = "typescript"
language-servers = ["typescript-language-server", "js-sem-highlight"]
```

## Zed (`languages.json`)

```jsonc
{
  "language_servers": ["js-sem-highlight"],
  "lsp": {
    "js-sem-highlight": {
      "binary": { "path": "js-sem-highlight" },
      "initialization_options": { "maxFileSizeKb": 512 }
    }
  }
}
```

## Compatibility

The server emits semantic tokens with `source: "js-sem"` and diagnostics
labeled the same way; it coexists with `tsserver` and `eslint-language-server`
without duplicating their output.

## Architecture

```
┌──────────────────┐
│  client (VSCode/ │
│  Neovim/Helix/   │
│  Zed)            │
└────────┬─────────┘
         │ LSP via stdio
         ▼
┌──────────────────────────────────────────────┐
│  crates/server (binary entry, panic hook)    │
└──────┬───────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────┐
│  crates/lsp                                  │
│   • Backend (impl LanguageServer)            │
│   • semantic-tokens encode/legend            │
│   • cache (delta resultIds)                  │
│   • config (initializationOptions schema)    │
└─┬────────────────────────────────────────────┘
  │
  ├──▶ crates/parsing  (tree-sitter, incremental, error-tolerant)
  ├──▶ crates/scopes   (oxc_parser + oxc_semantic, IdentifierRole)
  └──▶ crates/rules    (5 visual lint rules + framework)
```

## License

Dual-licensed under MIT or Apache-2.0, at your option.
