<script setup lang="ts">
/**
 * 跨启动器实例导入弹窗：扫描本机 PCL2 / HMCL / Prism 的实例，勾选后复制进当前游戏文件夹。
 *
 * 与整合包导入的差异：这里搬的是「已装好的实例」（含 mods/saves/config），
 * 不重新下载；因此不需要联网，也不需要 loader 重新安装。
 */
import { computed, ref, watch } from 'vue';
import { applyLauncherImport, errText, listFolders, scanLauncherInstances } from '../api';
import type { ImportCandidate } from '../api';
import { refreshInstalled, toast } from '../store';

const props = defineProps<{ open: boolean; defaultFolder?: string }>();
const emit = defineEmits<{ (event: 'close'): void }>();

const scanning = ref(false);
const scanned = ref(false);
const candidates = ref<ImportCandidate[]>([]);
const roots = ref<{ launcher: string; root: string; count: number }[]>([]);
const folders = ref<{ path: string; displayName?: string }[]>([]);
const targetFolder = ref('');
const selected = ref<Set<string>>(new Set());
const busy = ref(false);
const error = ref('');
/** 导入进度（逐实例）：让用户知道搬了几个、还剩几个 */
const progress = ref({ done: 0, total: 0, current: '' });
/** 导入失败明细：带实例名与原因，支持直接重试失败项 */
const failures = ref<{ path: string; name: string; reason: string }[]>([]);

const grouped = computed(() => {
    const map = new Map<string, ImportCandidate[]>();
    for (const c of candidates.value) {
        const list = map.get(c.launcher) ?? [];
        list.push(c);
        map.set(c.launcher, list);
    }
    return [...map.entries()];
});

function toggle(path: string) {
    const next = new Set(selected.value);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    selected.value = next;
}

async function doScan() {
    if (scanning.value) return;
    scanning.value = true;
    error.value = '';
    try {
        const r = await scanLauncherInstances();
        candidates.value = r.instances;
        roots.value = r.roots;
        scanned.value = true;
        // 默认全选：多数用户是「一次把所有旧实例搬过来」
        selected.value = new Set(r.instances.map((c) => c.path));
    } catch (e) {
        error.value = errText(e);
    } finally {
        scanning.value = false;
    }
}

async function loadFolders() {
    try {
        const r = await listFolders();
        folders.value = r.folders ?? [];
        if (!targetFolder.value)
            targetFolder.value = props.defaultFolder || r.active || folders.value[0]?.path || '';
    } catch {
        // 文件夹列表读取失败时让用户手填（后端仍会校验）
    }
}

async function doImport(list?: ImportCandidate[]) {
    if (busy.value || !targetFolder.value) return;
    const picked = list ?? candidates.value.filter((c) => selected.value.has(c.path));
    if (!picked.length) {
        error.value = '请至少选择一个要导入的实例';
        return;
    }
    busy.value = true;
    error.value = '';
    failures.value = [];
    progress.value = { done: 0, total: picked.length, current: '' };
    let ok = 0;
    try {
        for (const c of picked) {
            progress.value.current = c.name;
            try {
                await applyLauncherImport(c.path, targetFolder.value, c.name);
                ok += 1;
            } catch (e) {
                failures.value.push({ path: c.path, name: c.name, reason: errText(e) });
            }
            progress.value.done += 1;
        }
        if (ok) {
            toast(
                `已导入 ${ok} 个实例${failures.value.length ? `，${failures.value.length} 个失败` : ''}`,
                'success'
            );
            void refreshInstalled().catch(() => undefined);
        }
        if (failures.value.length) {
            error.value = '部分实例没能导入，可在下方重试失败项。';
        } else {
            selected.value = new Set();
            emit('close');
        }
    } finally {
        busy.value = false;
        progress.value.current = '';
    }
}

/** 只重试失败过的实例（成功的不重复搬） */
function retryFailed() {
    const list = candidates.value.filter((c) => failures.value.some((f) => f.path === c.path));
    if (list.length) void doImport(list);
}

watch(
    () => props.open,
    (open) => {
        if (!open) return;
        error.value = '';
        void loadFolders();
        if (!scanned.value) void doScan();
    }
);
</script>

<template>
    <Teleport to="body">
        <div
            v-if="open"
            class="modal-mask"
            data-ui="LauncherImport:mask"
            @pointerdown.self="emit('close')"
        >
            <div class="modal launcher-import-modal" data-ui="LauncherImport:modal">
                <h3 class="modal-title">从其他启动器导入实例</h3>

                <p class="li-hint">
                    自动扫描本机的 PCL2 / HMCL / Prism 实例。导入会复制 mods、存档、配置与资源包，
                    不会重新下载；源实例保持不动。
                </p>

                <div class="li-roots" v-if="roots.length">
                    <span v-for="r in roots" :key="r.root" class="li-root">
                        {{ r.launcher }} · {{ r.root }}（{{ r.count }}）
                    </span>
                </div>

                <div class="li-toolbar">
                    <button class="btn btn-ghost" :disabled="scanning" @click="doScan">
                        {{ scanning ? '扫描中…' : '重新扫描' }}
                    </button>
                    <span class="li-count">
                        {{
                            scanned
                                ? `共 ${candidates.length} 个可导入实例，已选 ${selected.size}`
                                : '尚未扫描'
                        }}
                    </span>
                </div>

                <div class="li-list">
                    <!-- 首次扫描才盖骨架；重扫时保留旧列表（避免每次白等，同「内容默认缓存」口径） -->
                    <div
                        v-if="scanning && !candidates.length"
                        class="li-skeleton"
                        role="status"
                        aria-live="polite"
                    >
                        <span class="spin"></span>
                        <span>正在扫描本机启动器…</span>
                    </div>
                    <div v-else-if="!candidates.length" class="li-empty">
                        <template v-if="scanned">
                            没有找到可导入的实例。<br />
                            可以点「重新扫描」，或确认 PCL2 / HMCL / Prism 装在本机的默认位置。
                        </template>
                        <template v-else>点击「重新扫描」开始</template>
                    </div>
                    <div v-for="[launcher, items] in grouped" :key="launcher" class="li-group">
                        <div class="li-group-title">{{ launcher }}</div>
                        <label v-for="c in items" :key="c.path" class="li-row">
                            <input
                                type="checkbox"
                                :checked="selected.has(c.path)"
                                @change="toggle(c.path)"
                            />
                            <span class="li-name">{{ c.name }}</span>
                            <span class="li-meta">
                                MC {{ c.mcVersion }}
                                <template v-if="c.loader">
                                    · {{ c.loader }} {{ c.loaderVersion }}</template
                                >
                                <template v-if="c.note"> · {{ c.note }}</template>
                            </span>
                        </label>
                    </div>
                </div>

                <div class="li-target">
                    <label>导入到</label>
                    <select v-if="folders.length" v-model="targetFolder" class="input">
                        <option v-for="f in folders" :key="f.path" :value="f.path">
                            {{ f.displayName || f.path }}
                        </option>
                    </select>
                    <!-- 没有已登记的游戏文件夹：置灰 + 说明怎么恢复（而不是留个空下拉让人猜） -->
                    <span v-else class="li-target-empty">
                        还没有游戏文件夹，先到设置里添加一个再导入
                    </span>
                </div>

                <!-- 导入进度：搬了几个、正在搬哪个 -->
                <div v-if="busy" class="li-progress" role="status" aria-live="polite">
                    <span class="spin"></span>
                    <span>
                        正在导入 {{ progress.done + 1 }}/{{ progress.total }}
                        <template v-if="progress.current"> · {{ progress.current }}</template>
                    </span>
                </div>

                <p v-if="error" class="li-error">{{ error }}</p>
                <!-- 失败明细：带实例名与原因，可只重试失败项 -->
                <div v-if="failures.length" class="li-failures">
                    <div v-for="f in failures" :key="f.path" class="li-failure">
                        <span class="li-failure-name">{{ f.name }}</span>
                        <span class="li-failure-reason">{{ f.reason }}</span>
                    </div>
                    <button class="btn btn-ghost btn-sm" :disabled="busy" @click="retryFailed">
                        重试失败项（{{ failures.length }}）
                    </button>
                </div>

                <div class="modal-actions">
                    <button class="btn btn-ghost" @click="emit('close')">取消</button>
                    <!-- 无可导入项时不用主按钮配色：禁用态的金钮仍读作「可点」，容易误以为能按 -->
                    <button
                        class="btn"
                        :class="selected.size && targetFolder ? 'btn-gold' : 'btn-ghost'"
                        :disabled="busy || !selected.size || !targetFolder"
                        @click="doImport()"
                    >
                        {{ busy ? '导入中…' : `导入选中（${selected.size}）` }}
                    </button>
                </div>
            </div>
        </div>
    </Teleport>
</template>

<style scoped>
.launcher-import-modal {
    width: 640px;
    max-height: 82vh;
    display: flex;
    flex-direction: column;
}
.li-hint {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.7;
    color: var(--text-dim);
}
.li-roots {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
}
.li-root {
    font-size: var(--text-xs);
    color: var(--text-dim);
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
}
.li-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
}
.li-count {
    font-size: var(--text-xs);
    color: var(--text-dim);
}
.li-list {
    flex: 1;
    min-height: 180px;
    max-height: 380px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--bg-2);
}
/* 空/加载态不需要 180px 的列表高度，收短避免大片留白 */
.li-list:has(> .li-empty),
.li-list:has(> .li-skeleton) {
    min-height: 0;
}
.li-empty {
    padding: var(--space-6) 0;
    text-align: center;
    font-size: var(--text-sm);
    line-height: 1.8;
    color: var(--text-dim);
}
/* 扫描中占位：与 li-empty 同高，加载完成不跳动 */
.li-skeleton {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-6) 0;
    font-size: var(--text-sm);
    color: var(--text-dim);
}
.li-group-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-dim);
    margin-bottom: var(--space-1);
}
.li-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    cursor: pointer;
}
.li-row:hover {
    background: var(--card);
}
.li-name {
    font-size: var(--text-sm);
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 220px;
}
.li-meta {
    font-size: var(--text-xs);
    color: var(--text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}
.li-target {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-sm);
}
.li-target label {
    color: var(--text-dim);
    white-space: nowrap;
}
.li-target-empty {
    font-size: var(--text-xs);
    color: var(--danger);
}
.li-error {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--danger);
    white-space: pre-wrap;
    max-height: 120px;
    overflow-y: auto;
}
/* 导入进度 */
.li-progress {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-dim);
}
/* 失败明细：逐条给原因，另有重试入口 */
.li-failures {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
}
.li-failure {
    display: flex;
    gap: var(--space-2);
    font-size: var(--text-xs);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--card);
    width: 100%;
    box-sizing: border-box;
}
.li-failure-name {
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
}
.li-failure-reason {
    color: var(--text-dim);
    word-break: break-all;
}
</style>
