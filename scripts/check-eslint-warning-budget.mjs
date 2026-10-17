#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const baselinePath = path.join(root, 'eslint.warning-budget.json');
const eslintPath = path.join(root, 'node_modules', 'eslint', 'bin', 'eslint.js');
const update = process.argv.includes('--update');

function identity(filePath, message) {
    return {
        file: filePath ? path.relative(root, filePath).split(path.sep).join('/') : '<no-file>',
        rule: message.ruleId ?? '(fatal)',
        messageId: message.messageId ?? '',
        line: message.line ?? 0
    };
}

function formatWarning(warning) {
    return `${warning.file}:${warning.line} [${warning.rule}] ${warning.messageId || '(no messageId)'}`;
}

export function compareWarnings(baseline, current) {
    const remaining = [...current];
    const removed = [];
    for (const expected of baseline) {
        const index = remaining.findIndex(
            (warning) =>
                warning.file === expected.file &&
                warning.rule === expected.rule &&
                warning.messageId === expected.messageId &&
                warning.line === expected.line
        );
        if (index === -1) removed.push(expected);
        else remaining.splice(index, 1);
    }
    return { added: remaining, removed };
}

function collectWarnings() {
    const result = spawnSync(process.execPath, [eslintPath, '.', '--format', 'json'], {
        cwd: root,
        encoding: 'utf8',
        maxBuffer: 64 * 1024 * 1024
    });
    if (result.error) throw result.error;
    if (!result.stdout) throw new Error(result.stderr || `eslint exited with ${result.status}`);

    const reports = JSON.parse(result.stdout);
    const warnings = [];
    let errors = 0;
    for (const report of reports) {
        errors += report.errorCount;
        for (const message of report.messages) {
            if (message.severity === 1) warnings.push(identity(report.filePath, message));
        }
    }
    return { warnings, errors, status: result.status };
}

function main() {
    const current = collectWarnings();
    if (current.errors > 0 || current.status > 1) {
        process.stderr.write(`ESLint reported ${current.errors} error(s).\n`);
        process.exitCode = 1;
        return;
    }

    if (update) {
        writeFileSync(baselinePath, `${JSON.stringify(current.warnings, null, 4)}\n`, 'utf8');
        process.stdout.write(
            `Updated ${path.basename(baselinePath)} with ${current.warnings.length} warning(s).\n`
        );
        return;
    }

    let baseline;
    try {
        baseline = JSON.parse(readFileSync(baselinePath, 'utf8'));
    } catch (error) {
        throw new Error(`cannot read ${path.basename(baselinePath)}: ${error.message}`);
    }
    const { added, removed } = compareWarnings(baseline, current.warnings);
    if (added.length || removed.length) {
        for (const warning of added)
            process.stderr.write(`[new warning] ${formatWarning(warning)}\n`);
        for (const warning of removed) {
            process.stderr.write(`[stale budget] ${formatWarning(warning)}\n`);
        }
        process.stderr.write(
            `ESLint warning budget mismatch: ${added.length} new, ${removed.length} stale. Remove fixed warnings with --update after reviewing the diff.\n`
        );
        process.exitCode = 1;
        return;
    }
    process.stdout.write(
        `ESLint warning budget passed: ${current.warnings.length} known warning(s).\n`
    );
}

main();
