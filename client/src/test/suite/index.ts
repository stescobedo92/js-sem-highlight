// Mocha entry. `@vscode/test-electron` calls `run()` here with VS Code's API
// already available (we run *inside* the extension host).

import * as path from 'path';
import Mocha from 'mocha';
import { glob } from 'glob';

export async function run(): Promise<void> {
    const mocha = new Mocha({
        ui: 'tdd',
        color: true,
        timeout: 20_000,
    });

    // `__dirname` is the compiled `out/test/suite/`; one level up gives us
    // `out/test/`. Both are inputs from the extension's own build output, not
    // user-controlled paths. We use `glob` with `absolute: true` so we never
    // concatenate the matched filename back into a path.
    const testsRoot = path.resolve(__dirname, '..');
    const matches = await glob('**/*.test.js', {
        cwd: testsRoot,
        absolute: true,
        nodir: true,
    });

    for (const absolutePath of matches) {
        mocha.addFile(absolutePath);
    }

    await new Promise<void>((resolve, reject) => {
        try {
            mocha.run((failures: number) => {
                if (failures > 0) {
                    reject(new Error(`${failures} test(s) failed`));
                } else {
                    resolve();
                }
            });
        } catch (err) {
            reject(err);
        }
    });
}
