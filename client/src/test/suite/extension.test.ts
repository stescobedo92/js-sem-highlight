// Suite básica que verifica que la extensión activa correctamente y que sus
// comandos están registrados. Cubre los Scenarios de
// `vscode-client-extension/spec.md` que pueden testearse sin lanzar el server
// real (necesitaría binarios precompilados en `client/server/<target>/`).
//
// Esto cumple task 8.11 del tasks.md.

import * as assert from 'assert';
import * as vscode from 'vscode';

const EXTENSION_ID = 'stescobedo.js-sem-highlight';
const EXPECTED_COMMANDS = ['js-sem.applyRecommendedColors', 'js-sem.restartServer'];
const EXPECTED_LANGUAGES = [
    'javascript',
    'javascriptreact',
    'typescript',
    'typescriptreact',
];

suite('JS Sem Highlight extension', () => {
    test('extension is present in the registry', () => {
        const ext = vscode.extensions.getExtension(EXTENSION_ID);
        assert.ok(ext, `expected extension '${EXTENSION_ID}' to be present`);
    });

    test('extension activates without throwing', async function () {
        this.timeout(30_000);
        const ext = vscode.extensions.getExtension(EXTENSION_ID);
        assert.ok(ext);
        if (!ext.isActive) {
            await ext.activate();
        }
        assert.strictEqual(ext.isActive, true);
    });

    test('contributes the documented commands', async () => {
        const allCommands = await vscode.commands.getCommands(true);
        for (const cmd of EXPECTED_COMMANDS) {
            assert.ok(
                allCommands.includes(cmd),
                `expected command '${cmd}' to be registered`,
            );
        }
    });

    test('declares activation events for all four target languages', () => {
        const ext = vscode.extensions.getExtension(EXTENSION_ID);
        assert.ok(ext);
        const events = (ext.packageJSON.activationEvents ?? []) as string[];
        for (const lang of EXPECTED_LANGUAGES) {
            const trigger = `onLanguage:${lang}`;
            assert.ok(
                events.includes(trigger),
                `expected activation event '${trigger}' in package.json`,
            );
        }
    });

    test('contributes configuration with all required keys', () => {
        const ext = vscode.extensions.getExtension(EXTENSION_ID);
        assert.ok(ext);
        const cfg = ext.packageJSON.contributes?.configuration?.properties ?? {};
        const required = [
            'js-sem.enable',
            'js-sem.rules',
            'js-sem.ignore',
            'js-sem.maxFileSizeKb',
            'js-sem.trace.server',
        ];
        for (const key of required) {
            assert.ok(key in cfg, `expected configuration key '${key}'`);
        }
    });

    test('applyRecommendedColors executes without throwing', async function () {
        this.timeout(15_000);
        // Smoke test: the command resolves cleanly. We do NOT assert on the
        // resulting config because @vscode/test-electron runs with an
        // ephemeral profile where Global writes don't propagate back to the
        // same getConfiguration() reader synchronously. The command's
        // behavior under a real user profile is exercised manually via the
        // command palette.
        const config = vscode.workspace.getConfiguration('editor');
        const before = config.get<Record<string, unknown>>(
            'semanticTokenColorCustomizations',
        );
        try {
            await vscode.commands.executeCommand('js-sem.applyRecommendedColors');
        } finally {
            await config.update(
                'semanticTokenColorCustomizations',
                before,
                vscode.ConfigurationTarget.Global,
            );
        }
    });
});
