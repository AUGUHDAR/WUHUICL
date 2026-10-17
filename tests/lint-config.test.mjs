import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function loadConfig() {
    const configUrl = pathToFileURL(path.join(repositoryRoot, 'eslint.config.js')).href;
    return (await import(`${configUrl}?test=${Date.now()}-${Math.random()}`)).default;
}

async function readFileText(filePath) {
    return readFile(filePath, 'utf8');
}

function explicitlyDisabledRules(config) {
    return config.flatMap((entry) =>
        Object.entries(entry.rules ?? {})
            .filter(([, severity]) => severity === 'off' || severity === 0)
            .map(([rule]) => rule)
    );
}

function runEslint(arguments_, cwd) {
    return spawnSync(
        process.execPath,
        [path.join(repositoryRoot, 'node_modules', 'eslint', 'bin', 'eslint.js'), ...arguments_],
        {
            cwd,
            encoding: 'utf8',
            timeout: 30000
        }
    );
}

async function filesBelow(root, directory) {
    const entries = await readdir(path.join(root, directory), { withFileTypes: true });
    const files = [];
    for (const entry of entries) {
        const relative = path.posix.join(directory, entry.name);
        if (entry.isDirectory()) files.push(...(await filesBelow(root, relative)));
        else if (/\.(?:ts|vue)$/.test(entry.name)) files.push(relative);
    }
    return files;
}

test('lint configuration parses TypeScript and Vue single-file components', async (t) => {
    const root = await mkdtemp(path.join(tmpdir(), 'wuhuicl-lint-'));
    t.after(() => rm(root, { force: true, recursive: true }));
    await mkdir(path.join(root, 'src'), { recursive: true });
    await writeFile(
        path.join(root, 'eslint.config.js'),
        `export { default } from ${JSON.stringify(pathToFileURL(path.join(repositoryRoot, 'eslint.config.js')).href)};\n`,
        'utf8'
    );
    await writeFile(path.join(root, 'package.json'), '{"type":"module"}\n', 'utf8');
    await writeFile(
        path.join(root, 'src', 'ExamplePanel.vue'),
        [
            '<script setup lang="ts">',
            'const props = defineProps<{ label: string }>();',
            'const emit = defineEmits<{ select: [value: string] }>();',
            'function select() {',
            '    emit("select", props.label);',
            '}',
            '</script>',
            '<template>',
            '    <button type="button" @click="select">{{ props.label }}</button>',
            '</template>',
            ''
        ].join('\n'),
        'utf8'
    );

    const result = runEslint(['src/ExamplePanel.vue'], root);

    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
});

test('lint configuration delegates template layout to Prettier', async (t) => {
    const root = await mkdtemp(path.join(tmpdir(), 'wuhuicl-lint-formatting-'));
    t.after(() => rm(root, { force: true, recursive: true }));
    await mkdir(path.join(root, 'src'), { recursive: true });
    await writeFile(
        path.join(root, 'eslint.config.js'),
        `export { default } from ${JSON.stringify(pathToFileURL(path.join(repositoryRoot, 'eslint.config.js')).href)};\n`,
        'utf8'
    );
    await writeFile(path.join(root, 'package.json'), '{"type":"module"}\n', 'utf8');
    await writeFile(
        path.join(root, 'src', 'FormattingOnly.vue'),
        [
            '<script setup lang="ts">',
            'const items = ["one", "two"];',
            '</script>',
            '<template>',
            '    <ul>',
            '        <li v-for="item in items" :key="item">{{ item }}</li>',
            '    </ul>',
            '</template>',
            ''
        ].join('\n'),
        'utf8'
    );

    const result = runEslint(['src/FormattingOnly.vue'], root);

    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
});

test('lint configuration keeps parser and correctness rules enabled', async () => {
    const config = await loadConfig();
    const disabled = new Set(explicitlyDisabledRules(config));
    const requiredRules = [
        'vue/no-parsing-error',
        'vue/no-mutating-props',
        'vue/valid-v-else-if',
        'vue/require-v-for-key',
        'vue/html-end-tags',
        '@typescript-eslint/no-unused-vars',
        '@typescript-eslint/no-unused-expressions',
        'no-control-regex',
        'no-empty',
        'vue/no-v-html',
        'vue/no-unused-components',
        'vue/require-explicit-emits',
        'vue/require-prop-types'
    ];

    for (const rule of requiredRules) {
        assert.equal(disabled.has(rule), false, `${rule} must not be globally disabled`);
    }
});

test('legacy lint exemptions are file-specific and preserve parser validity rules', async () => {
    const exemptions = JSON.parse(
        await readFileText(path.join(repositoryRoot, 'eslint.legacy-exemptions.json'))
    );
    const parserRules = new Set([
        'vue/no-parsing-error',
        'vue/valid-v-else-if',
        'vue/valid-v-for',
        'vue/valid-v-if',
        'vue/valid-v-model'
    ]);
    const sourceFiles = new Set([
        ...(await filesBelow(repositoryRoot, 'src')),
        ...(await filesBelow(repositoryRoot, 'shared'))
    ]);

    assert.match(exemptions.removalCondition, /Remove a file/);
    assert.equal(exemptions.defaultSeverity, 'warn');
    assert.ok(Array.isArray(exemptions.rules) && exemptions.rules.length > 0);
    for (const file of exemptions.legacyFiles) {
        assert.equal(path.isAbsolute(file), false, `${file} must be repository-relative`);
        assert.equal(file.includes('*'), false, `${file} must not be a glob`);
        assert.equal(
            sourceFiles.has(file) || file.startsWith('scripts/'),
            true,
            `${file} must exist`
        );
    }
    for (const rule of exemptions.rules) {
        assert.equal(parserRules.has(rule), false, `cannot downgrade parser validity ${rule}`);
    }
});

test('Prettier checks source files while honoring explicit file exemptions', async (t) => {
    const root = await mkdtemp(path.join(tmpdir(), 'wuhuicl-prettier-ignore-'));
    t.after(() => rm(root, { force: true, recursive: true }));
    await mkdir(path.join(root, 'src'), { recursive: true });
    await mkdir(path.join(root, 'shared'), { recursive: true });
    await writeFile(
        path.join(root, 'src', 'FormattingProbe.ts'),
        'export const sourceValue = {answer:42}\n',
        'utf8'
    );
    await writeFile(
        path.join(root, 'shared', 'FormattingProbe.ts'),
        'export const sharedValue = {answer:42}\n',
        'utf8'
    );
    const expectedFiles = [
        ...(await filesBelow(root, 'src')),
        ...(await filesBelow(root, 'shared'))
    ].sort();
    const ignorePaths = [
        path.join(repositoryRoot, '.prettierignore'),
        path.join(repositoryRoot, '.prettierignore.migration')
    ];

    const result = spawnSync(
        process.execPath,
        [
            path.join(repositoryRoot, 'node_modules', 'prettier', 'bin', 'prettier.cjs'),
            '--list-different',
            'src',
            'shared',
            '--ignore-path',
            ignorePaths[0],
            '--ignore-path',
            ignorePaths[1],
            '--log-level',
            'debug'
        ],
        {
            cwd: root,
            encoding: 'utf8',
            timeout: 30000
        }
    );
    const diagnostics = `${result.stdout}${result.stderr}`;
    const checkedFiles = diagnostics
        .split(/\r?\n/)
        .filter((line) => /^(?:src|shared)[\\/].+\.(?:ts|vue)$/.test(line.trim()))
        .map((line) => line.trim().replaceAll('\\', '/'))
        .sort();

    assert.equal(result.status, 1, diagnostics);
    assert.deepEqual(checkedFiles, expectedFiles, diagnostics);
});

test('formatting exemptions remain active and are documented for removal', async () => {
    const prettier = await import('prettier');
    const ignoreText = await readFile(
        path.join(repositoryRoot, '.prettierignore.migration'),
        'utf8'
    );
    const exemptions = ignoreText
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter((line) => line && !line.startsWith('#'));

    assert.match(ignoreText, /Remove each entry/);
    assert.equal(exemptions.includes('src/'), false);
    assert.equal(exemptions.includes('shared/'), false);

    for (const file of exemptions) {
        const absolute = path.join(repositoryRoot, file);
        const options = (await prettier.resolveConfig(absolute, { editorconfig: true })) ?? {};
        const source = await readFile(absolute, 'utf8');
        assert.equal(
            await prettier.check(source, { ...options, filepath: absolute }),
            false,
            `${file} is formatted; remove its migration exemption`
        );
    }
});
