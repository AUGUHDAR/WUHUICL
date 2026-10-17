#!/usr/bin/env node

import { existsSync } from 'node:fs';
import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import ts from 'typescript';

const SOURCE_EXTENSIONS = new Set(['.ts', '.tsx', '.mts', '.cts', '.js', '.jsx', '.mjs', '.vue']);
const LEGACY_DIRECTORIES = ['components', 'composables', 'views'];
const ALLOWLIST_PATH = path.join('scripts', 'legacy-source-allowlist.json');
const DIRECT_IPC_FILES = new Set(['api.ts', 'shim.ts', 'shim-splash.ts']);

function usage() {
    return 'Usage: node scripts/check-architecture.mjs [--root <directory>]';
}

function parseArguments(argv) {
    let root = process.cwd();

    for (let index = 0; index < argv.length; index += 1) {
        const argument = argv[index];
        if (argument === '--help' || argument === '-h') {
            process.stdout.write(`${usage()}\n`);
            process.exit(0);
        }
        if (argument === '--root') {
            const value = argv[index + 1];
            if (!value) throw new Error('--root requires a directory');
            root = path.resolve(value);
            index += 1;
            continue;
        }
        throw new Error(`unknown argument: ${argument}`);
    }

    return root;
}

function toPosix(value) {
    return value.split(path.sep).join('/');
}

function relativePath(root, filePath) {
    return toPosix(path.relative(root, filePath));
}

async function walk(directory) {
    const entries = await readdir(directory, { withFileTypes: true });
    const files = [];

    for (const entry of entries) {
        const entryPath = path.join(directory, entry.name);
        if (entry.isDirectory()) files.push(...(await walk(entryPath)));
        else if (entry.isFile()) files.push(entryPath);
    }

    return files;
}

async function loadAllowlist(root) {
    const allowlistPath = path.join(root, ALLOWLIST_PATH);
    if (!existsSync(allowlistPath)) {
        return { legacySourceFiles: new Set(), legacyWindowKamuclAccesses: new Set() };
    }

    let parsed;
    try {
        parsed = JSON.parse(await readFile(allowlistPath, 'utf8'));
    } catch (error) {
        throw new Error(`cannot read ${ALLOWLIST_PATH}: ${error.message}`);
    }

    const readPaths = (key) => {
        const value = parsed[key];
        if (value === undefined) return new Set();
        if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
            throw new Error(
                `${ALLOWLIST_PATH}: ${key} must be an array of repository-relative paths`
            );
        }
        return new Set(value.map((item) => toPosix(item).replace(/^\.\//, '')));
    };

    return {
        legacySourceFiles: readPaths('legacySourceFiles'),
        legacyWindowKamuclAccesses: readPaths('legacyWindowKamuclAccesses'),
        removalConditions: parsed.removalConditions ?? {}
    };
}

function scriptBlocks(contents) {
    const blocks = [];
    const pattern = /<script\b[^>]*>([\s\S]*?)<\/script\s*>/gi;
    let match;

    while ((match = pattern.exec(contents)) !== null) {
        const body = match[1];
        const bodyOffset = match.index + match[0].indexOf(body);
        blocks.push({
            body,
            lineOffset: contents.slice(0, bodyOffset).split(/\r?\n/).length - 1
        });
    }

    return blocks;
}

function parseScript(contents, filePath) {
    if (path.extname(filePath).toLowerCase() !== '.vue') {
        return [{ body: contents, lineOffset: 0 }];
    }
    return scriptBlocks(contents);
}

function moduleSpecifier(specifier) {
    return specifier && ts.isStringLiteralLike(specifier) ? specifier : null;
}

function inspectTree(sourceFile, block) {
    const imports = [];
    const ipcAccesses = [];
    const literalSpecifier = (node) => {
        if (ts.isStringLiteralLike(node)) return node;
        if (node && ts.isNoSubstitutionTemplateLiteral(node)) return node;
        return null;
    };

    const addImport = (specifierNode) => {
        const literal = moduleSpecifier(specifierNode);
        if (!literal) return;
        imports.push({
            line: sourceFile.getLineAndCharacterOfPosition(literal.getStart(sourceFile)).line + 1,
            specifier: literal.text
        });
    };

    const visit = (node) => {
        if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
            addImport(node.moduleSpecifier);
        } else if (
            ts.isCallExpression(node) &&
            node.expression.kind === ts.SyntaxKind.ImportKeyword
        ) {
            addImport(node.arguments[0]);
        } else if (
            ts.isPropertyAccessExpression(node) &&
            node.name.text === 'kamucl' &&
            (node.expression.kind === ts.SyntaxKind.Identifier ||
                ts.isPropertyAccessExpression(node.expression))
        ) {
            const target = node.expression.getText(sourceFile);
            if (target === 'window' || target === 'globalThis') {
                ipcAccesses.push({
                    line:
                        sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile)).line + 1
                });
            }
        } else if (ts.isElementAccessExpression(node)) {
            const target = node.expression.getText(sourceFile);
            const key = literalSpecifier(node.argumentExpression)?.text;
            if (key === 'kamucl' && (target === 'window' || target === 'globalThis')) {
                ipcAccesses.push({
                    line:
                        sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile)).line + 1
                });
            }
        }

        if (node.kind !== ts.SyntaxKind.ImportType) ts.forEachChild(node, visit);
    };

    visit(sourceFile);
    return {
        imports: imports.map((entry) => ({ ...entry, line: entry.line + block.lineOffset })),
        ipcAccesses: ipcAccesses.map((entry) => ({ ...entry, line: entry.line + block.lineOffset }))
    };
}

async function inspectSourceFile(filePath) {
    const contents = await readFile(filePath, 'utf8');
    const imports = [];
    const ipcAccesses = [];

    for (const block of parseScript(contents, filePath)) {
        const sourceFile = ts.createSourceFile(
            filePath,
            block.body,
            ts.ScriptTarget.Latest,
            true,
            ts.ScriptKind.TS
        );
        const inspected = inspectTree(sourceFile, block, imports, ipcAccesses);
        imports.push(...inspected.imports);
        ipcAccesses.push(...inspected.ipcAccesses);
    }

    return { imports, ipcAccesses };
}

async function resolveImport(root, importer, specifier) {
    const withoutQuery = specifier.split(/[?#]/, 1)[0];
    let candidate;

    if (withoutQuery.startsWith('.')) {
        candidate = path.resolve(path.dirname(importer), withoutQuery);
    } else if (withoutQuery.startsWith('@shared/')) {
        candidate = path.join(root, 'shared', withoutQuery.slice('@shared/'.length));
    } else if (withoutQuery.startsWith('@/')) {
        candidate = path.join(root, 'src', withoutQuery.slice(2));
    } else {
        return null;
    }

    const candidates = [];
    if (!path.extname(candidate)) {
        for (const extension of SOURCE_EXTENSIONS) candidates.push(`${candidate}${extension}`);
        for (const extension of SOURCE_EXTENSIONS) {
            candidates.push(path.join(candidate, `index${extension}`));
        }
    } else if (candidate.endsWith('.js')) {
        for (const extension of ['.ts', '.tsx', '.vue']) {
            candidates.push(candidate.slice(0, -3) + extension);
        }
    }
    candidates.push(candidate);

    for (const possible of candidates) {
        try {
            if ((await stat(possible)).isFile()) return toPosix(relativePath(root, possible));
        } catch {
            // Keep checking TypeScript-compatible extensions and directory indexes.
        }
    }

    return toPosix(relativePath(root, candidate));
}

function featureName(sourcePath) {
    const parts = sourcePath.split('/');
    return parts[0] === 'src' && parts[1] === 'features' && parts.length > 3 ? parts[2] : null;
}

function appModule(sourcePath) {
    return sourcePath === 'src/App.vue' || sourcePath.startsWith('src/app/');
}

function legacySourceFile(sourcePath) {
    const parts = sourcePath.split('/');
    return (
        parts[0] === 'src' &&
        LEGACY_DIRECTORIES.includes(parts[1]) &&
        SOURCE_EXTENSIONS.has(path.extname(sourcePath).toLowerCase())
    );
}

function canDirectlyAccessIpc(sourcePath) {
    const parts = sourcePath.split('/');
    return (
        (parts[0] === 'src' && parts[1] === 'core' && parts[2] === 'ipc') ||
        (parts[0] === 'src' && parts.length === 2 && DIRECT_IPC_FILES.has(parts[1]))
    );
}

function addViolation(violations, file, line, rule, message) {
    violations.push({ file, line, rule, message });
}

function inspectImport(file, importerFeature, targetPath, specifier, line, violations) {
    if (!targetPath) return;
    const targetFeature = featureName(targetPath);

    if (importerFeature && targetFeature && targetFeature !== importerFeature) {
        const barrel = `src/features/${targetFeature}/index.ts`;
        if (targetPath !== barrel) {
            addViolation(
                violations,
                file,
                line,
                'feature-boundary',
                `features/${importerFeature} must import features/${targetFeature} through ${barrel}, not ${targetPath} (import "${specifier}")`
            );
        }
    }

    if (file.startsWith('src/core/')) {
        if (targetFeature) {
            addViolation(
                violations,
                file,
                line,
                'core-feature-boundary',
                `core must not import feature code: ${targetPath} (import "${specifier}")`
            );
        } else if (appModule(targetPath)) {
            addViolation(
                violations,
                file,
                line,
                'core-app-boundary',
                `core must not import application shell code: ${targetPath} (import "${specifier}")`
            );
        }
    }

    if (file.startsWith('src/shared/')) {
        if (targetFeature) {
            addViolation(
                violations,
                file,
                line,
                'shared-feature-boundary',
                `shared must not import feature code: ${targetPath} (import "${specifier}")`
            );
        } else if (appModule(targetPath)) {
            addViolation(
                violations,
                file,
                line,
                'shared-app-boundary',
                `shared must not import application shell code: ${targetPath} (import "${specifier}")`
            );
        }
    }

    if (importerFeature && appModule(targetPath)) {
        addViolation(
            violations,
            file,
            line,
            'feature-app-boundary',
            `feature code must not import the application shell: ${targetPath} (import "${specifier}")`
        );
    }
}

async function checkArchitecture(root) {
    const sourceRoot = path.join(root, 'src');
    try {
        if (!(await stat(sourceRoot)).isDirectory()) throw new Error();
    } catch {
        throw new Error(`source directory does not exist: ${sourceRoot}`);
    }

    const allowlist = await loadAllowlist(root);
    const files = (await walk(sourceRoot)).filter((file) =>
        SOURCE_EXTENSIONS.has(path.extname(file).toLowerCase())
    );
    const sourceFiles = new Set(files.map((file) => relativePath(root, file)));
    const violations = [];
    let allowedLegacySources = 0;

    for (const key of ['legacySourceFiles', 'legacyWindowKamuclAccesses']) {
        for (const allowed of allowlist[key]) {
            const sourcePath =
                key === 'legacyWindowKamuclAccesses'
                    ? allowed.split(':').slice(0, -1).join(':')
                    : allowed;
            if (!sourceFiles.has(sourcePath)) {
                addViolation(
                    violations,
                    toPosix(ALLOWLIST_PATH),
                    1,
                    'allowlist-entry',
                    `${key} entry does not name a current source file: ${allowed}`
                );
            }
        }
    }

    for (const filePath of files) {
        const file = relativePath(root, filePath);
        const importerFeature = featureName(file);
        const { imports, ipcAccesses } = await inspectSourceFile(filePath);

        for (const entry of imports) {
            const target = await resolveImport(root, filePath, entry.specifier);
            inspectImport(file, importerFeature, target, entry.specifier, entry.line, violations);
        }

        if (legacySourceFile(file) && !allowlist.legacySourceFiles.has(file)) {
            addViolation(
                violations,
                file,
                1,
                'legacy-source',
                `new source files belong under src/features, src/core, src/app, or src/shared; add a reviewed migration shim to ${toPosix(ALLOWLIST_PATH)} only when a shim is intentional`
            );
        } else if (legacySourceFile(file)) {
            allowedLegacySources += 1;
        }

        if (!canDirectlyAccessIpc(file)) {
            for (const access of ipcAccesses) {
                if (allowlist.legacyWindowKamuclAccesses.has(`${file}:${access.line}`)) continue;
                addViolation(
                    violations,
                    file,
                    access.line,
                    'direct-ipc-access',
                    'access window.kamucl only through src/core/ipc/**'
                );
            }
        }
    }

    violations.sort(
        (left, right) =>
            left.file.localeCompare(right.file) ||
            left.line - right.line ||
            left.rule.localeCompare(right.rule)
    );

    return {
        filesScanned: files.length,
        allowedLegacySources,
        violations
    };
}

async function main() {
    let root;
    try {
        root = parseArguments(process.argv.slice(2));
    } catch (error) {
        process.stderr.write(
            `Architecture check configuration error: ${error.message || usage()}\n`
        );
        process.exitCode = 2;
        return;
    }

    try {
        if (!(await stat(root)).isDirectory()) throw new Error();
    } catch {
        process.stderr.write(
            `Architecture check configuration error: root directory does not exist: ${root}\n`
        );
        process.exitCode = 2;
        return;
    }

    let result;
    try {
        result = await checkArchitecture(root);
    } catch (error) {
        process.stderr.write(`Architecture check configuration error: ${error.message}\n`);
        process.exitCode = 2;
        return;
    }

    if (result.violations.length > 0) {
        for (const violation of result.violations) {
            process.stderr.write(
                `${violation.file}:${violation.line}:1 [${violation.rule}] ${violation.message}\n`
            );
        }
        process.stderr.write(
            `Architecture check failed with ${result.violations.length} violation(s) across ${result.filesScanned} source file(s).\n`
        );
        process.exitCode = 1;
        return;
    }

    process.stdout.write(
        `Architecture check passed: ${result.filesScanned} source file(s), ${result.allowedLegacySources} legacy source file(s) explicitly allowlisted.\n`
    );
}

await main();
