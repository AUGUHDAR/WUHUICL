import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { copyFile } from 'node:fs/promises';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const checkerPath = path.join(repositoryRoot, 'scripts', 'check-architecture.mjs');

async function createFixture(t, files) {
    const root = await mkdtemp(path.join(tmpdir(), 'wuhuicl-architecture-'));
    t.after(() => rm(root, { force: true, recursive: true }));

    for (const [relativePath, contents] of Object.entries(files)) {
        const target = path.join(root, relativePath);
        await mkdir(path.dirname(target), { recursive: true });
        await writeFile(target, contents, 'utf8');
    }

    return root;
}

function runChecker(root) {
    return spawnSync(process.execPath, [checkerPath, '--root', root], {
        encoding: 'utf8',
        timeout: 15000
    });
}

function runStrictChecker(root) {
    return spawnSync(process.execPath, [checkerPath, '--root', root, '--strict-allowlist'], {
        encoding: 'utf8',
        timeout: 15000
    });
}

function runBaselineChecker(root, allowlist) {
    return spawnSync(process.execPath, [checkerPath, '--root', root, '--allowlist', allowlist], {
        encoding: 'utf8',
        timeout: 15000
    });
}

function output(result) {
    return `${result.stdout}${result.stderr}`;
}

test('allows feature barrels, own internals, core/shared imports, and IPC access', async (t) => {
    const root = await createFixture(t, {
        'scripts/legacy-source-allowlist.json': JSON.stringify({
            legacySourceFiles: ['src/components/LegacyPanel.vue'],
            legacyWindowKamuclAccesses: ['src/components/LegacyPanel.vue:2']
        }),
        'shared/platform.ts': 'export const platform = "test";\n',
        'src/App.vue': '<template><main /></template>\n',
        'src/api.ts': 'export const api = window.kamucl;\n',
        'src/components/LegacyPanel.vue':
            '<script setup lang="ts">\nconst bridge = window.kamucl;\n</script>\n',
        'src/core/ipc/bridge.ts': 'export const bridge = window.kamucl;\n',
        'src/core/ipc/index.ts':
            "export { bridge } from './bridge';\nexport const platform = 'test';\n",
        'src/features/accounts/data/state.ts': 'export const accountState = true;\n',
        'src/features/accounts/index.ts': "export { accountState } from './data/state';\n",
        'src/features/settings/data/state.ts':
            "import { accountState } from '../../accounts';\n" +
            "import { platform } from '@shared/platform';\n" +
            'export const settingsState = [accountState, platform];\n',
        'src/features/settings/index.ts': "export { settingsState } from './data/state';\n",
        'src/features/settings/ui/SettingsPanel.vue': [
            '<script setup lang="ts">',
            "import { settingsState } from '../data/state';",
            "import { accountState } from '../../accounts';",
            "import { bridge } from '../../../core/ipc';",
            "import { platform } from '@shared/platform';",
            'void [settingsState, accountState, bridge, platform];',
            '</script>',
            '<template><section /></template>',
            ''
        ].join('\n')
    });

    const result = runChecker(root);

    assert.equal(result.status, 0, output(result));
    assert.match(result.stdout, /Architecture check passed/);
    assert.match(result.stdout, /1 legacy source file\(s\)/);
});

test('reports forbidden directions, cross-feature internals, app imports, and IPC access', async (t) => {
    const root = await createFixture(t, {
        'src/App.vue': '<template><main /></template>\n',
        'src/app/bootstrap/index.ts': 'export const bootstrap = true;\n',
        'src/components/NewPanel.vue': '<template><section /></template>\n',
        'src/core/feature.ts': "import '../features/accounts';\n",
        'src/core/shell.ts': "import '../app/bootstrap';\n",
        'src/features/accounts/index.ts': 'export const accounts = true;\n',
        'src/features/accounts/ui/AppImport.vue':
            '<script setup lang="ts">\nimport App from "../../../App.vue";\n</script>\n',
        'src/features/accounts/ui/bootstrap.ts': "import '../../../app/bootstrap';\n",
        'src/features/accounts/ui/direct.ts': 'const platform = window.kamucl.platform;\n',
        'src/features/accounts/ui/asserted.ts':
            'const platform = (window as Window & { kamucl: unknown }).kamucl;\n',
        'src/features/accounts/ui/Internal.ts': 'export const internal = true;\n',
        'src/features/settings/index.ts': 'export const settings = true;\n',
        'src/features/settings/ui/CrossImport.vue': [
            '<script setup lang="ts">',
            "import { internal } from '../../accounts/ui/Internal';",
            "const state = import('../../accounts/data/state');",
            'void [internal, state];',
            '</script>',
            ''
        ].join('\n'),
        'src/shared/feature.ts': "import '../features/settings';\n",
        'src/shared/shell.ts': "import '../app/bootstrap';\n"
    });

    const result = runChecker(root);
    const diagnostics = output(result);

    assert.equal(result.status, 1, diagnostics);
    assert.match(diagnostics, /features\/settings\/ui\/CrossImport\.vue:2:1 .*feature-boundary/);
    assert.match(diagnostics, /features\/settings\/ui\/CrossImport\.vue:3:1 .*feature-boundary/);
    assert.match(diagnostics, /src\/core\/feature\.ts:1:1 .*core-feature-boundary/);
    assert.match(diagnostics, /src\/core\/shell\.ts:1:1 .*core-app-boundary/);
    assert.match(diagnostics, /src\/shared\/feature\.ts:1:1 .*shared-feature-boundary/);
    assert.match(diagnostics, /src\/shared\/shell\.ts:1:1 .*shared-app-boundary/);
    assert.match(diagnostics, /features\/accounts\/ui\/AppImport\.vue:2:1 .*feature-app-boundary/);
    assert.match(diagnostics, /features\/accounts\/ui\/bootstrap\.ts:1:1 .*feature-app-boundary/);
    assert.match(diagnostics, /features\/accounts\/ui\/direct\.ts:1:1 .*direct-ipc-access/);
    assert.match(diagnostics, /features\/accounts\/ui\/asserted\.ts:1:1 .*direct-ipc-access/);
    assert.match(diagnostics, /src\/components\/NewPanel\.vue:1:1 .*legacy-source/);
    assert.match(diagnostics, /Architecture check failed with \d+ violation\(s\)/);
});

test('requires explicit allowlisting for source files in legacy directories', async (t) => {
    const root = await createFixture(t, {
        'scripts/legacy-source-allowlist.json': JSON.stringify({
            legacySourceFiles: ['src/composables/legacy.ts'],
            legacyWindowKamuclAccesses: []
        }),
        'src/composables/legacy.ts': 'export const legacy = true;\n',
        'src/views/NewView.vue': '<template><main /></template>\n'
    });

    const result = runChecker(root);
    const diagnostics = output(result);

    assert.equal(result.status, 1, diagnostics);
    assert.match(diagnostics, /src\/views\/NewView\.vue:1:1 .*legacy-source/);
    assert.doesNotMatch(diagnostics, /src\/composables\/legacy\.ts:1 .*legacy-source/);
});

test('fails when the requested root does not exist', async (t) => {
    const root = path.join(tmpdir(), `wuhuicl-missing-${process.pid}-${Date.now()}`);
    t.after(() => rm(root, { force: true, recursive: true }));

    const result = runChecker(root);

    assert.equal(result.status, 2, output(result));
    assert.match(result.stderr, /root directory does not exist/);
});

test('strict allowlist mode reports stale migration entries', async (t) => {
    const root = await createFixture(t, {
        'scripts/legacy-source-allowlist.json': JSON.stringify({
            legacySourceFiles: ['src/components/MovedPanel.vue'],
            legacyWindowKamuclAccesses: ['src/features/settings/ui/View.vue:10']
        }),
        'src/App.vue': '<template><main /></template>\n'
    });

    const result = runStrictChecker(root);
    const diagnostics = output(result);

    assert.equal(result.status, 1, diagnostics);
    assert.match(diagnostics, /allowlist-entry.*src\/components\/MovedPanel\.vue/);
    assert.match(diagnostics, /allowlist-entry.*src\/features\/settings\/ui\/View\.vue:10/);
});

test('strict allowlist mode reports stale IPC exemption lines', async (t) => {
    const root = await createFixture(t, {
        'scripts/legacy-source-allowlist.json': JSON.stringify({
            legacySourceFiles: ['src/components/LegacyPanel.vue'],
            legacyWindowKamuclAccesses: ['src/components/LegacyPanel.vue:99']
        }),
        'src/components/LegacyPanel.vue':
            '<script setup lang="ts">\nconst bridge = window.kamucl;\n</script>\n'
    });

    const result = runStrictChecker(root);
    const diagnostics = output(result);

    assert.equal(result.status, 1, diagnostics);
    assert.match(diagnostics, /allowlist-entry.*LegacyPanel\.vue:99/);
});

test('idle trimming uses the public IPC facade', () => {
    const source = readFileSync(
        path.join(repositoryRoot, 'src', 'core', 'runtime', 'idleTrim.ts'),
        'utf8'
    );

    assert.doesNotMatch(source, /window\.kamucl/);
    assert.match(source, /subscribeIpc/);
});

test('package architecture command rejects stale migration entries', () => {
    const packageJson = JSON.parse(readFileSync(path.join(repositoryRoot, 'package.json'), 'utf8'));
    const command = packageJson.scripts['check:architecture'];

    assert.match(command, /--strict-allowlist/);
});

test('quality workflow runs the package architecture command', () => {
    const workflow = readFileSync(
        path.join(repositoryRoot, '.github', 'workflows', 'quality.yml'),
        'utf8'
    );

    assert.match(workflow, /run:\s+npm run check:architecture/);
});

test('supports an external allowlist path for a fixture root', async (t) => {
    const root = await mkdtemp(path.join(tmpdir(), 'wuhuicl-repository-baseline-'));
    t.after(() => rm(root, { force: true, recursive: true }));
    await mkdir(path.join(root, 'src'), { recursive: true });
    const allowlistName = 'allowlist.json';
    await copyFile(
        path.join(repositoryRoot, 'scripts', 'legacy-source-allowlist.json'),
        path.join(root, allowlistName)
    );
    await writeFile(path.join(root, 'src', 'App.vue'), '<template><main /></template>\n', 'utf8');

    const result = runBaselineChecker(root, allowlistName);

    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
});

test('fails with a source location when a Vue script block cannot be parsed', async (t) => {
    const root = await createFixture(t, {
        'src/features/settings/ui/Broken.vue': [
            '<script setup lang="ts">',
            'const broken: = 1;',
            '</script>',
            ''
        ].join('\n')
    });

    const result = runChecker(root);
    const diagnostics = output(result);

    assert.equal(result.status, 1, diagnostics);
    assert.match(diagnostics, /Broken\.vue:2:1 .*source-parse-error/);
});
