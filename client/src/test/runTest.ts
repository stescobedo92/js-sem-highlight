// Entry point for `@vscode/test-electron`. Downloads a VS Code build,
// launches it with the extension loaded in development mode, and runs the
// Mocha suite resolved by `./suite/index`.
//
// Usage (from `client/`):
//   npm run compile
//   node ./out/test/runTest.js
//
// In CI (Linux), wrap in xvfb-run for headless display.

import * as path from 'path';
import { runTests } from '@vscode/test-electron';

async function main() {
    try {
        const extensionDevelopmentPath = path.resolve(__dirname, '../../');
        const extensionTestsPath = path.resolve(__dirname, './suite/index');
        await runTests({
            extensionDevelopmentPath,
            extensionTestsPath,
            launchArgs: ['--disable-extensions'],
        });
    } catch (err) {
        console.error('Failed to run tests', err);
        process.exit(1);
    }
}

void main();
