/**
 * 将本 run 构建的 Release 资产镜像到 Gitee（release.yml 的 gitee-release job 调用）。
 * 用同一批构建产物上传，保证双端资产字节一致（更新器对 Gitee 无 digest，靠 size 校验兜底）。
 * 环境变量：GITEE_TOKEN（缺省则警告并跳过）、VERSION、GITHUB_REPOSITORY、GITHUB_TOKEN。
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const TOKEN = process.env.GITEE_TOKEN;
if (!TOKEN) {
    console.warn('[gitee] 未配置 GITEE_TOKEN secret，跳过 Gitee 同步（仓库 Variables 设 GITEE_SYNC_ENABLED=false 可静音本提示）');
    process.exit(0);
}
const VERSION = process.env.VERSION;
const GH_REPO = process.env.GITHUB_REPOSITORY; // AUGUHDAR/WUHUICL
if (!VERSION || !GH_REPO) {
    console.error('[gitee] 缺少 VERSION 或 GITHUB_REPOSITORY');
    process.exit(1);
}
const DIST = 'dist';
const GH_HEADERS = {
    Authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
    Accept: 'application/vnd.github+json'
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function retry(label, fn, attempts = 3) {
    let last;
    for (let i = 1; i <= attempts; i++) {
        try {
            return await fn();
        } catch (e) {
            last = e;
            console.warn(`[gitee] ${label} 第 ${i} 次失败：${e.message}`);
            await sleep(2000 * i);
        }
    }
    throw new Error(`${label} 重试 ${attempts} 次仍失败：${last?.message}`);
}

// 1) 取 GitHub Release 的标题与正文做镜像（github-release job 已生成 notes）
let name = `WUHUICL ${VERSION}`;
let body = `WUHUICL ${VERSION}`;
try {
    const r = await fetch(`https://api.github.com/repos/${GH_REPO}/releases/tags/${VERSION}`, {
        headers: GH_HEADERS
    });
    if (r.ok) {
        const j = await r.json();
        name = j.name || name;
        body = j.body || body;
        console.log(`[gitee] 已取 GitHub Release 文案：${name}`);
    } else {
        console.warn(`[gitee] GitHub Release 读取失败（HTTP ${r.status}），用默认文案`);
    }
} catch (e) {
    console.warn(`[gitee] GitHub Release 读取异常：${e.message}，用默认文案`);
}

const base = `https://gitee.com/api/v5/repos/${GH_REPO}`;

// 2) 找或建 Gitee release（tag 需已在 Gitee 仓库存在：推送镜像时记得带 tag）
async function findGiteeRelease() {
    const r = await fetch(`${base}/releases/tags/${VERSION}?access_token=${TOKEN}`);
    if (r.ok) return r.json();
    return null;
}
let release = await retry('Gitee release 查询', findGiteeRelease);
if (!release) {
    console.log('[gitee] 创建 release…');
    release = await retry('Gitee release 创建', async () => {
        const r = await fetch(`${base}/releases`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ access_token: TOKEN, tag_name: VERSION, name, body })
        });
        if (!r.ok) {
            const text = await r.text();
            throw new Error(`HTTP ${r.status} ${text.slice(0, 300)}（若提示 tag 不存在：Gitee 镜像推送时需带上 tag）`);
        }
        return r.json();
    });
} else {
    console.log(`[gitee] release 已存在（id=${release.id}），仅补传资产`);
}

// 3) 上传资产（同名覆盖由 Gitee attach 语义决定；失败即退出非零，方便重跑 job）
const files = readdirSync(DIST)
    .filter((f) => f.startsWith('WUHUICL-'))
    .map((f) => join(DIST, f))
    .filter((f) => statSync(f).isFile());
if (!files.length) {
    console.error('[gitee] dist 里没有 WUHUICL-* 资产');
    process.exit(1);
}
for (const file of files) {
    const fileName = file.split(/[\\/]/).pop();
    await retry(`上传 ${fileName}`, async () => {
        const form = new FormData();
        form.append('file', new Blob([readFileSync(file)]), fileName);
        const r = await fetch(
            `${base}/releases/${release.id}/attach_files?access_token=${TOKEN}`,
            { method: 'POST', body: form }
        );
        if (!r.ok) {
            const text = await r.text();
            throw new Error(`HTTP ${r.status} ${text.slice(0, 300)}`);
        }
        console.log(`[gitee] 已上传 ${fileName}（${statSync(file).size} 字节）`);
    });
}
console.log(`[gitee] 同步完成：https://gitee.com/${GH_REPO}/releases`);
