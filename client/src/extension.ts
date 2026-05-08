// VS Code extension entry point.
//
// Spawns the `js-sem-highlight` Rust binary as a Language Server, registers
// dynamic semantic-tokens legend (read at runtime, not hardcoded), and exposes
// commands and configuration described in the spec.
//
// See: openspec/changes/add-js-semantic-highlighter-lsp/specs/vscode-client-extension/spec.md

import * as fs from 'fs';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    State,
    StateChangeEvent,
    TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let restartAttempts = 0;
const RESTART_BACKOFF_MS = [1000, 5000, 15000];
const RESTART_WINDOW_MS = 30_000;
let lastRestartTimestamps: number[] = [];

const SUPPORTED_LANGUAGES = [
    'javascript',
    'javascriptreact',
    'typescript',
    'typescriptreact',
];

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    const enabled = vscode.workspace.getConfiguration('js-sem').get<boolean>('enable', true);
    if (!enabled) {
        return;
    }

    const binaryPath = locateServerBinary(context);
    if (!binaryPath) {
        await vscode.window.showErrorMessage(
            `js-sem-highlight: no server binary bundled for platform ${process.platform}/${process.arch}. ` +
                `Please build from source or install a supported version.`,
        );
        return;
    }

    context.subscriptions.push(
        vscode.commands.registerCommand('js-sem.applyRecommendedColors', applyRecommendedColors),
        vscode.commands.registerCommand('js-sem.restartServer', () => restartServer(context)),
    );

    await startClient(binaryPath, context);
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}

// ============================================================================
//   Binary location
// ============================================================================

// Hardcoded mapping (platform, arch) → (relative bundled binary path,
// dev-mode binary basename). Both columns are string literals defined here at
// build time — they do NOT carry runtime input into `path.join`/`path.resolve`.
// We look up the entry by the runtime tuple but only ever use the literal
// values from this table to build paths, eliminating the path traversal
// vector flagged by CWE-22 heuristics.
type TargetEntry = {
    readonly bundledRelative: string;
    readonly devBasename: string;
};

const TARGET_TABLE: ReadonlyMap<string, TargetEntry> = new Map([
    ['darwin-arm64', { bundledRelative: 'server/darwin-arm64/js-sem-highlight', devBasename: 'js-sem-highlight' }],
    ['darwin-x64',   { bundledRelative: 'server/darwin-x64/js-sem-highlight',   devBasename: 'js-sem-highlight' }],
    ['linux-x64',    { bundledRelative: 'server/linux-x64/js-sem-highlight',    devBasename: 'js-sem-highlight' }],
    ['linux-arm64',  { bundledRelative: 'server/linux-arm64/js-sem-highlight',  devBasename: 'js-sem-highlight' }],
    ['win32-x64',    { bundledRelative: 'server/win32-x64/js-sem-highlight.exe', devBasename: 'js-sem-highlight.exe' }],
]);

const DEV_RELATIVE_DIRS: ReadonlyArray<string> = ['../target/release', '../target/debug'];

function targetKey(): string {
    return `${process.platform}-${process.arch}`;
}

function locateServerBinary(context: vscode.ExtensionContext): string | undefined {
    const entry = TARGET_TABLE.get(targetKey());
    if (!entry) {
        return undefined;
    }

    // `entry.bundledRelative` is a hardcoded literal from TARGET_TABLE.
    // `asAbsolutePath` is the VS Code-sanctioned way to resolve extension-
    // relative paths and does not allow escape outside the extension dir.
    const bundled = context.asAbsolutePath(entry.bundledRelative);
    if (fs.existsSync(bundled)) {
        return bundled;
    }

    // Dev fallback: only literal relative dirs from DEV_RELATIVE_DIRS plus a
    // literal basename from TARGET_TABLE are concatenated. No user input.
    for (const relDir of DEV_RELATIVE_DIRS) {
        const candidate = context.asAbsolutePath(`${relDir}/${entry.devBasename}`);
        if (fs.existsSync(candidate)) {
            return candidate;
        }
    }
    return undefined;
}

// ============================================================================
//   Client lifecycle + auto-restart
// ============================================================================

async function startClient(binaryPath: string, context: vscode.ExtensionContext): Promise<void> {
    const cfg = vscode.workspace.getConfiguration('js-sem');
    const initializationOptions = {
        rules: cfg.get<Record<string, string>>('rules') ?? {},
        ignore: cfg.get<string[]>('ignore') ?? [],
        maxFileSizeKb: cfg.get<number>('maxFileSizeKb') ?? 512,
    };

    const serverOptions: ServerOptions = {
        run: { command: binaryPath, transport: TransportKind.stdio },
        debug: { command: binaryPath, transport: TransportKind.stdio },
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: SUPPORTED_LANGUAGES.map((lang) => ({ scheme: 'file', language: lang })),
        synchronize: {
            configurationSection: 'js-sem',
        },
        initializationOptions,
        outputChannelName: 'JS Sem Highlight',
        traceOutputChannel: vscode.window.createOutputChannel('JS Sem Trace'),
    };

    client = new LanguageClient('js-sem-highlight', 'JS Sem Highlight', serverOptions, clientOptions);

    client.onDidChangeState(async (event: StateChangeEvent) => {
        if (event.newState === State.Stopped) {
            await maybeAutoRestart(binaryPath, context);
        }
    });

    try {
        await client.start();
    } catch (err) {
        await vscode.window.showErrorMessage(
            `js-sem-highlight failed to start: ${err instanceof Error ? err.message : String(err)}`,
        );
    }

    // Reenviar settings al server cuando el usuario los cambia. El server
    // también recibe initializationOptions iniciales — esto es para runtime.
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(async (e: vscode.ConfigurationChangeEvent) => {
            if (!e.affectsConfiguration('js-sem')) {
                return;
            }
            const newCfg = vscode.workspace.getConfiguration('js-sem');
            await client?.sendNotification('workspace/didChangeConfiguration', {
                settings: {
                    rules: newCfg.get('rules') ?? {},
                    ignore: newCfg.get('ignore') ?? [],
                    maxFileSizeKb: newCfg.get('maxFileSizeKb') ?? 512,
                },
            });
        }),
    );
}

async function maybeAutoRestart(binaryPath: string, context: vscode.ExtensionContext): Promise<void> {
    const now = Date.now();
    lastRestartTimestamps = lastRestartTimestamps.filter((t) => now - t < RESTART_WINDOW_MS);
    if (lastRestartTimestamps.length >= 3) {
        await vscode.window.showErrorMessage(
            'js-sem-highlight: server crashed 3 times in 30s. Auto-restart paused. Use "JS Sem: Restart server" when ready.',
        );
        return;
    }
    const delay = RESTART_BACKOFF_MS[Math.min(restartAttempts, RESTART_BACKOFF_MS.length - 1)];
    restartAttempts += 1;
    await new Promise((resolve) => setTimeout(resolve, delay));
    lastRestartTimestamps.push(Date.now());
    await startClient(binaryPath, context);
}

async function restartServer(context: vscode.ExtensionContext): Promise<void> {
    restartAttempts = 0;
    lastRestartTimestamps = [];
    if (client) {
        await client.stop();
    }
    const binary = locateServerBinary(context);
    if (binary) {
        await startClient(binary, context);
    }
}

// ============================================================================
//   Recommended semantic color theme application
// ============================================================================

async function applyRecommendedColors(): Promise<void> {
    const config = vscode.workspace.getConfiguration('editor');
    const existing = config.get<Record<string, unknown>>('semanticTokenColorCustomizations') ?? {};
    const existingRules = (existing.rules as Record<string, unknown> | undefined) ?? {};

    const recommended: Record<string, unknown> = {
        '*.unused': { foreground: '#888888', fontStyle: 'italic' },
        '*.defaultLibrary': { foreground: '#4ec9b0' },
        '*.deprecated': { fontStyle: 'strikethrough' },
        '*.async': { fontStyle: 'italic' },
        'parameter:javascript': { foreground: '#9cdcfe' },
        'parameter:typescript': { foreground: '#9cdcfe' },
    };

    // Preserve user keys; only fill the ones we recommend that are missing.
    const merged: Record<string, unknown> = { ...existingRules };
    for (const [k, v] of Object.entries(recommended)) {
        if (!(k in merged)) {
            merged[k] = v;
        }
    }

    const updated = { ...existing, rules: merged, enabled: true };
    await config.update(
        'semanticTokenColorCustomizations',
        updated,
        vscode.ConfigurationTarget.Global,
    );
    // Fire-and-forget: `showInformationMessage` only resolves when the user
    // clicks a button (or all message buttons disappear). Awaiting it would
    // hang in headless test environments where no UI interaction occurs.
    void vscode.window.showInformationMessage(
        'JS Sem: recommended semantic colors applied to your global settings.',
    );
}
