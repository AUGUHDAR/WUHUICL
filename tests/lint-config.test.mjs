import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

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

test('lint configuration ignores formatting-only template layout', async (t) => {
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
            '        <li v-for="item in items">{{ item }}</li>',
            '    </ul>',
            '</template>',
            ''
        ].join('\n'),
        'utf8'
    );

    const result = runEslint(['src/FormattingOnly.vue'], root);

    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
});
