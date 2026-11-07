<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref, watch } from 'vue';
import type {
    ProjectionCatalog,
    ProjectionEntry,
    ProjectionRequest,
    ProjectionResult
} from '@shared/projections';
import { errText } from '../api';
import { activeInstalled, store, toast } from '../store';
import ContentSkeleton from '../components/ContentSkeleton.vue';
import SelectMenu from '../components/SelectMenu.vue';
import ResourceScopeFields from '../components/ResourceScopeFields.vue';
import ResourcePathChip from '../components/ResourcePathChip.vue';
import ConfirmModal from '../components/ConfirmModal.vue';
import ResourcePagination from '../components/ResourcePagination.vue';
import { useSweepSelect } from '../composables/sweepSelect';
import type { SchematicPreviewSource } from '../lab/schematic-preview/backend';
import ProjectionConvert from '../components/ProjectionConvert.vue';
import ProjectionThumb from '../components/ProjectionThumb.vue';
const SchematicWorkshopView = defineAsyncComponent(() => import('./SchematicWorkshopView.vue'));
const converting = ref<ProjectionEntry>();
/** 渲染子页开关与来源：rendering 为 null = 空工坊（自行导入/拖入/从实例选择） */
const renderMode = ref(false);
const rendering = ref<SchematicPreviewSource | null>(null);
const renderKey = ref(0);
const data = ref<ProjectionCatalog>({
    entries: [],
    warnings: [],
    library: '',
    instances: [],
    othersHaveProjections: false
});
const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    query = ref(''),
    kind = ref('all'),
    source = ref('all'),
    page = ref(1),
    selected = ref<string[]>([]),
    target = ref(''),
    confirm = ref<'trash' | 'dispatch' | ''>('');
/** 排序跨会话记忆（与截图/录像页同口径） */
const sort = ref(localStorage.getItem('kamucl.projectionSort') || 'new');
watch(sort, (v) => localStorage.setItem('kamucl.projectionSort', v));
const results = ref<ProjectionResult[]>([]);
let generation = 0,
    disposed = false;
const activeFolder = computed(() => store.settings?.activeFolder || store.settings?.gameDir || '');
/** 内容范围：instance=只看当前实例；folder=版本文件夹内的所有（含收藏与共享目录） */
const scope = ref<'instance' | 'folder'>(
    localStorage.getItem('kamucl.projectionScope') === 'folder' ? 'folder' : 'instance'
);
watch(scope, (v) => localStorage.setItem('kamucl.projectionScope', v));
const currentTarget = computed(() => {
    const inst = store.resourceVersionId;
    const folder = activeInstalled.value.find((v) => v.id === inst)?.folder || activeFolder.value;
    return inst ? { id: inst, folder } : null;
});
const scopeOptions = [
    { value: 'instance', label: '当前实例', description: '只看所选实例的投影目录' },
    {
        value: 'folder',
        label: '版本文件夹内的所有',
        description: '聚合文件夹内全部实例各自的 schematics 目录'
    }
];
watch(
    () => JSON.stringify([activeFolder.value, store.resourceVersionId, scope.value]),
    () => {
        void refresh(true);
    }
);
watch(
    () => store.fsRefreshTick,
    () => {
        if (!busy.value) void refresh();
    }
);
const invoke = <T,>(channel: string, ...args: unknown[]) =>
    window.kamucl.invoke(channel, ...args) as Promise<T>;
const filtered = computed(() =>
    data.value.entries.filter(
        (e) =>
            (scope.value === 'folder' ||
                (scope.value === 'instance' &&
                    !!currentTarget.value &&
                    (e.instanceId === currentTarget.value.id ||
                        (!e.instanceId &&
                            !!e.folder &&
                            e.folder === currentTarget.value.folder)))) &&
            (kind.value === 'all' || e.kind === kind.value) &&
            (source.value === 'all' ||
                (source.value === 'shared' ? !e.instanceId : !!e.instanceId)) &&
            `${e.name} ${e.source} ${e.directory}`
                .toLowerCase()
                .includes(query.value.trim().toLowerCase())
    )
);
const sorted = computed(() => {
    const list = [...filtered.value];
    if (sort.value === 'name') list.sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans-CN'));
    else if (sort.value === 'old') list.sort((a, b) => a.modified - b.modified);
    else list.sort((a, b) => b.modified - a.modified);
    return list;
});
const pages = computed(() => Math.max(1, Math.ceil(sorted.value.length / 40)));
const rows = computed(() => sorted.value.slice((page.value - 1) * 40, page.value * 40));
const targets = computed(() =>
    data.value.instances.map((i) => ({ value: JSON.stringify([i.folder, i.id]), label: i.name }))
);
/** 工坊解析器只认 litematic / schem（Sponge v2/v3）；.schematic 是旧版 MCEdit，.nbt 是原版结构 */
const canRender = (entry: ProjectionEntry) =>
    !entry.error && (entry.kind === 'litematic' || entry.kind === 'schem');
const thumbSource = (entry: ProjectionEntry): SchematicPreviewSource | null =>
    canRender(entry) ? { kind: 'external', path: entryPath(entry) } : null;
const entryPath = (entry: ProjectionEntry) => {
    // entry.name 已带扩展名（扫描按后缀过滤）；directory 是真实父目录
    const sep = entry.directory.includes('\\') ? '\\' : '/';
    const dir = entry.directory.replace(/[\\/]+$/, '');
    return `${dir}${sep}${entry.name}`;
};
function renderEntry(entry: ProjectionEntry) {
    if (!canRender(entry)) return;
    renderKey.value += 1;
    rendering.value = { kind: 'external', path: entryPath(entry) };
    renderMode.value = true;
}
function openWorkshop() {
    renderKey.value += 1;
    rendering.value = null;
    renderMode.value = true;
}
const size = (n: number) =>
    n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
/** 当前实例为空、但该文件夹其它实例里有投影 → 空状态给出切换入口，避免误以为"没有投影" */
const othersHaveProjections = ref(false);
watch([query, kind, source, sort, scope], () => {
    page.value = 1;
    selected.value = [];
});
async function refresh(silent = false, force = false) {
    const id = ++generation;
    if (!silent) loading.value = true;
    error.value = '';
    try {
        const next = await invoke<ProjectionCatalog>(
            'projections:list',
            scope.value,
            currentTarget.value ?? {},
            // force：用户点「刷新」= 要一份最新真相；静默刷新/首屏走短 TTL 缓存
            force
        );
        if (disposed || id !== generation) return;
        data.value = next;
        if (!targets.value.some((t) => t.value === target.value)) target.value = '';
        selected.value = selected.value.filter((id) => next.entries.some((e) => e.id === id));
        page.value = Math.min(page.value, pages.value);
        if (!silent) target.value = '';
        // 「当前实例为空但同文件夹别的实例有投影」由后端随结果一并给出（只数列目录），
        // 不再让前端补发一次 scope=folder 的完整扫描
        othersHaveProjections.value = !!next.othersHaveProjections;
    } catch (e) {
        if (!disposed && id === generation) error.value = errText(e);
    } finally {
        if (!disposed && id === generation) loading.value = false;
    }
}
function drag(event: DragEvent, entry: ProjectionEntry) {
    // 纵向拖 = 扫选接管；横向仍走文件拖出
    if (interceptFileDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (busy.value || loading.value || store.editMode) return;
    window.kamucl.send(
        'projections:drag',
        selected.value.includes(entry.id) ? [...selected.value] : [entry.id]
    );
}

/** PCL 式扫选：按住行纵向拖 = 沿途选中/反选 */
const workspaceEl = ref<HTMLElement | null>(null);
const { interceptFileDrag } = useSweepSelect({
    container: () => workspaceEl.value,
    itemSelector: '.recording-row',
    enabled: () => !busy.value && !loading.value && !store.editMode,
    isSelected: (key) => selected.value.includes(key),
    setMany: (keys, want) => {
        const set = new Set(selected.value);
        for (const k of keys) want ? set.add(k) : set.delete(k);
        selected.value = [...set];
    }
});
/**
 * 自动刷新只在「真的可能变了」时发生：窗口重新获得焦点（从外面拷了文件回来）。
 * 定时轮询已取消——投影只会在用户自己导入/游戏内保存时变化，
 * 每 15 秒重新列一遍目录纯属白烧磁盘。
 */
let lastAutoRefresh = 0;
function autoRefresh() {
    const now = Date.now();
    if (now - lastAutoRefresh < 4000) return;
    if (
        !busy.value &&
        !loading.value &&
        !selected.value.length &&
        !target.value &&
        document.visibilityState === 'visible'
    ) {
        lastAutoRefresh = now;
        void refresh(true);
    }
}
function selectPage() {
    const ids = rows.value.map((e) => e.id);
    selected.value = ids.every((id) => selected.value.includes(id))
        ? selected.value.filter((id) => !ids.includes(id))
        : [...new Set([...selected.value, ...ids])];
}
/** 两个选择按钮同一套状态感知语义：标签永远说明下一次点击的结果（与截图/录像页同款） */
const pageAllSelected = computed(
    () => rows.value.length > 0 && rows.value.every((e) => selected.value.includes(e.id))
);
const filteredAllSelected = computed(
    () => sorted.value.length > 0 && sorted.value.every((e) => selected.value.includes(e.id))
);
function toggleAllFiltered() {
    selected.value = filteredAllSelected.value ? [] : sorted.value.map((e) => e.id);
}
async function open(entry?: ProjectionEntry) {
    try {
        // 无参 = 打开当前实例的 schematics；有参 = 定位到该文件
        await invoke('projections:open', entry?.id, currentTarget.value ?? {});
    } catch (e) {
        toast(errText(e), 'error');
    }
}
async function execute(action: ProjectionRequest['action'] | 'import') {
    if (busy.value) return;
    const ids = [...selected.value],
        destination =
            target.value === ''
                ? undefined
                : data.value.instances.find(
                      (i) => JSON.stringify([i.folder, i.id]) === target.value
                  );
    confirm.value = '';
    busy.value = true;
    try {
        // collect 默认落到当前实例；dispatch 必须带所选目标实例
        const effectiveTarget =
            action === 'dispatch' ? destination : (destination ?? currentTarget.value ?? {});
        const result =
            action === 'import'
                ? await invoke<ProjectionResult[] | null>(
                      'projections:import',
                      currentTarget.value ?? {}
                  )
                : await invoke<ProjectionResult[] | null>('projections:operate', {
                      action,
                      ids,
                      target: effectiveTarget
                  });
        if (result) {
            results.value = result;
            const failed = result.filter((r) => !r.ok);
            selected.value = failed.map((r) => r.id);
            toast(
                `已完成 ${result.length - failed.length} 项${failed.length ? `，${failed.length} 项未完成` : ''}`,
                failed.length ? 'error' : 'success'
            );
            await refresh(false, true);
        }
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
function escape(e: KeyboardEvent) {
    if (e.key === 'Escape') confirm.value = '';
}
/** 拖入投影文件一键导入（projections:importAt，落到当前实例 schematics，跳过对话框） */
const dragActive = ref(false);
async function dropProjections(event: DragEvent) {
    dragActive.value = false;
    if (busy.value || renderMode.value) return;
    const files = Array.from(event.dataTransfer?.files ?? [])
        .map((f) => window.kamucl.getFilePath(f))
        .filter(Boolean);
    if (!files.length) return;
    busy.value = true;
    try {
        const result = await invoke<ProjectionResult[] | null>(
            'projections:importAt',
            files,
            currentTarget.value ?? {}
        );
        if (result) {
            results.value = result;
        }
        toast(`已导入 ${files.length} 个投影文件`, 'success');
        await refresh(true, true);
    } catch (e) {
        toast('导入失败：' + errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
onMounted(() => {
    void refresh();
    window.addEventListener('keydown', escape);
    window.addEventListener('focus', autoRefresh);
    store.resourceDropHandler = dropProjections;
});
onUnmounted(() => {
    disposed = true;
    generation++;
    window.removeEventListener('keydown', escape);
    window.removeEventListener('focus', autoRefresh);
    if (store.resourceDropHandler === dropProjections) store.resourceDropHandler = null;
});
</script>

<template>
    <section class="recordings-page" :class="{ 'render-mode': renderMode }" data-ui="projections:page">
        <template v-if="renderMode">
            <header class="page-head recording-heading" data-ui="projections:render-heading">
                <div>
                    <h1>投影渲染</h1>
                    <p class="muted">3D 查看与编辑；拖入 .litematic / .schem 或从实例选择也可以。</p>
                </div>
                <button class="btn btn-ghost" @click="renderMode = false">← 返回投影列表</button>
            </header>
            <SchematicWorkshopView :key="renderKey" :auto-open="rendering" class="projection-workshop-embed" />
        </template>
        <template v-else>
        <ProjectionConvert
            v-if="converting"
            :entry="converting"
            @close="converting = undefined"
            @done="refresh(false, true)"
        />
        <header class="page-head recording-heading" data-ui="projections:heading">
            <div data-ui="ProjectionsView:3e7b827962e6">
                <h1 data-ui="ProjectionsView:2ac7467a0648">
                    投影 <small>{{ data.entries.length }} 个</small>
                </h1>
                <p class="muted">
                    汇总各实例 schematics 目录中的投影文件；支持子目录（如 schematics/sync）。
                </p>
            </div>
            <div class="actions">
                <button
                    data-ui="ProjectionsView:78a31a977742"
                    class="btn btn-ghost"
                    @click="open()"
                >
                    打开投影文件夹</button
                ><button data-ui="projections:workshop" class="btn btn-ghost" @click="openWorkshop">
                    渲染工坊</button
                ><button
                    data-ui="ProjectionsView:1594a41b4b07"
                    class="btn btn-gold"
                    :disabled="busy"
                    @click="execute('import')"
                >
                    导入投影…</button
                ><button
                    data-ui="ProjectionsView:54ee9bdbb21c"
                    class="btn btn-ghost"
                    :disabled="busy || loading"
                    @click="refresh(false, true)"
                >
                    {{ loading ? '读取中…' : '↻ 刷新' }}
                </button>
            </div>
        </header>
        <div class="recording-context">
            <ResourceScopeFields
                v-model:scope="scope"
                :scope-options="scopeOptions"
                v-model:instance-id="store.resourceVersionId"
                :instance-options="
                    data.instances.map((i) => ({
                        value: i.id,
                        label: i.name,
                        description: i.isolated ? '已隔离' : '共享'
                    }))
                "
                :instance-locked="scope === 'folder'"
                :disabled="busy"
            />
        </div>
        <div class="recording-library" data-ui="projections:library">
            <ResourcePathChip label="导入保存位置" :path="data.library" />
            <details class="recording-help">
                <summary>使用说明</summary>
                <p>
                    默认只显示当前实例的投影；切到「版本文件夹内的所有」可看整个文件夹。投影存放在各实例的
                    schematics 目录，可再分子目录（如
                    schematics/sync），这里会一并列出。按住投影名称可拖出，勾选后可一起拖出。复制时校验完整性，同名文件自动添加序号。通过“转换”检查格式与游戏版本差异，再生成副本。
                </p>
            </details>
        </div>
        <section
            ref="workspaceEl"
            class="recording-workspace card"
            :class="{ 'drop-hot': dragActive }"
            @dragover.prevent="dragActive = true"
            @dragleave.self="dragActive = false"
        >
            <div class="recording-filters" data-ui="projections:filters">
                <input
                    data-ui="ProjectionsView:a4bc662c17f4"
                    v-model="query"
                    class="input"
                    aria-label="搜索投影"
                    placeholder="搜索投影名称、实例或路径…"
                    :disabled="busy"
                />
                <SelectMenu
                    v-model="kind"
                    :disabled="busy"
                    :options="[
                        { value: 'all', label: '全部格式' },
                        ...['litematic', 'schem', 'schematic', 'nbt'].map((v) => ({
                            value: v,
                            label: '.' + v
                        }))
                    ]"
                />
                <SelectMenu
                    v-model="source"
                    :disabled="busy"
                    :options="[
                        { value: 'all', label: '全部位置' },
                        { value: 'shared', label: '游戏文件夹根' },
                        { value: 'instances', label: '实例目录' }
                    ]"
                />
                <SelectMenu
                    v-model="sort"
                    :disabled="busy"
                    :options="[
                        { value: 'new', label: '最新优先' },
                        { value: 'old', label: '最旧优先' },
                        { value: 'name', label: '按名称' }
                    ]"
                />
            </div>
            <div data-ui="ProjectionsView:03ebe3d01f67" v-if="error" class="card" role="alert">
                {{ error }}
                <button
                    data-ui="ProjectionsView:d86b658466b7"
                    class="btn btn-ghost"
                    @click="refresh(false, true)"
                >
                    重试
                </button>
            </div>
            <details
                data-ui="ProjectionsView:4886ef0bbbad"
                v-if="data.warnings.length"
                class="card"
            >
                <summary>部分目录未读取（{{ data.warnings.length }}）</summary>
                <p
                    data-ui="ProjectionsView:a95d7ac71e30"
                    v-for="warning in data.warnings"
                    :key="warning"
                >
                    {{ warning }}
                </p>
            </details>
            <div v-if="filtered.length" class="recording-controls" data-ui="projections:controls">
                <div class="actions">
                    <button
                        data-ui="ProjectionsView:f5fef6266bd6"
                        class="btn btn-ghost"
                        :disabled="busy || !rows.length"
                        @click="selectPage"
                    >
                        {{ pageAllSelected ? '取消选择当前页' : '选择当前页' }}</button
                    ><button
                        data-ui="ProjectionsView:8436cfb2245d"
                        class="btn btn-ghost"
                        :disabled="busy || !sorted.length"
                        @click="toggleAllFiltered"
                    >
                        {{
                            filteredAllSelected
                                ? '取消全选'
                                : `全选筛选结果（${sorted.length}）`
                        }}</button
                    ><span>已选 {{ selected.length }} 项</span
                    ><button
                        data-ui="ProjectionsView:ed589ee8e06c"
                        v-if="selected.length"
                        class="btn btn-ghost"
                        :disabled="busy"
                        @click="selected = []"
                    >
                        清空
                    </button>
                </div>
                <div
                    data-ui="ProjectionsView:c59dbb632ead"
                    v-if="selected.length"
                    class="actions operation-row"
                >
                    <button
                        data-ui="ProjectionsView:85f3f8414f9d"
                        class="btn btn-gold"
                        :disabled="busy"
                        @click="execute('collect')"
                    >
                        复制到当前实例</button
                    ><button
                        data-ui="ProjectionsView:000f81fa6db3"
                        class="btn btn-ghost"
                        :disabled="busy"
                        @click="execute('export')"
                    >
                        提取到文件夹…</button
                    ><SelectMenu
                        v-model="target"
                        :disabled="busy"
                        :options="targets"
                        placeholder="选择目标实例"
                    /><button
                        data-ui="ProjectionsView:fa2131a8e18d"
                        class="btn btn-ghost"
                        :disabled="busy || target === ''"
                        @click="confirm = 'dispatch'"
                    >
                        复制到实例</button
                    ><button
                        data-ui="ProjectionsView:1699f50ec9f0"
                        class="btn btn-danger"
                        :disabled="busy"
                        @click="confirm = 'trash'"
                    >
                        移入回收站
                    </button>
                </div>
            </div>
            <div
                data-ui="ProjectionsView:69fb5a4a1573"
                v-if="loading && data.entries.length"
                class="status-strip"
                role="status"
            >
                正在更新投影列表…
            </div>
            <ContentSkeleton
                v-if="loading && !data.entries.length"
                class="card"
                label="正在读取投影目录…"
            />
            <div
                data-ui="ProjectionsView:9196e415ffb9"
                v-else-if="!rows.length && !error"
                class="card empty"
            >
                <span>{{
                    data.entries.length
                        ? '没有匹配的投影，请调整搜索或筛选。'
                        : othersHaveProjections
                          ? '当前实例还没有投影，但该文件夹的其它实例里有。'
                          : '暂无投影。可导入 .litematic、.schem、.schematic 或原版结构 .nbt。'
                }}</span
                ><button
                    data-ui="ProjectionsView:c0ed13df1c01"
                    v-if="data.entries.length || othersHaveProjections"
                    class="btn btn-ghost"
                    @click="
                        othersHaveProjections
                            ? (scope = 'folder')
                            : ((query = ''), (kind = 'all'), (source = 'all'));
                    "
                >
                    {{ othersHaveProjections ? '查看版本文件夹内的所有' : '清除筛选' }}
                </button>
            </div>
            <div
                v-else-if="rows.length"
                :inert="loading || !!error"
                class="recording-list"
                data-ui="projections:list"
            >
                <article
                    data-ui="ProjectionsView:2bb2004aef70"
                    v-for="entry in rows"
                    :key="entry.id"
                    class="recording-row"
                    :class="{ selected: selected.includes(entry.id) }"
                    :data-sweep-key="entry.id"
                >
                    <input
                        data-ui="ProjectionsView:d29a579f673a"
                        v-model="selected"
                        type="checkbox"
                        :value="entry.id"
                        :aria-label="`选择 ${entry.name}`"
                        :disabled="busy"
                    />
                    <ProjectionThumb :id="entry.id" :source="thumbSource(entry)" />
                    <div
                        data-ui="ProjectionsView:3867dfbdf0e7"
                        class="recording-info"
                        :draggable="!busy && !loading && !store.editMode"
                        title="按住拖出投影文件"
                        @dragstart="drag($event, entry)"
                    >
                        <strong
                            data-ui="ProjectionsView:6f5f6ab090cd"
                            tabindex="0"
                            :title="entry.name"
                            >{{ entry.name }}</strong
                        >
                        <div data-ui="ProjectionsView:80aad3d6d4fa" class="muted">
                            {{ '.' + entry.kind }} · {{ entry.source }} · {{ size(entry.size) }} ·
                            {{ entry.blocks ?? '?' }} 方块 ·
                            {{ entry.gameVersion || entry.dataVersion || '版本未知' }} ·
                            {{ new Date(entry.modified).toLocaleString() }}
                        </div>
                        <p v-if="entry.error" class="scan-error" role="status">{{ entry.error }}</p>
                        <div
                            data-ui="ProjectionsView:2185a2a82be6"
                            class="muted path"
                            :title="entry.directory"
                        >
                            {{ entry.directory }}
                        </div>
                    </div>
                    <button
                        data-ui="ProjectionsView:e41d05dae554"
                        class="btn btn-ghost btn-sm"
                        @click="open(entry)"
                    >
                        定位文件</button
                    ><button
                        data-ui="projections:render"
                        class="btn btn-ghost btn-sm"
                        :disabled="!canRender(entry)"
                        :title="canRender(entry) ? '在渲染工坊中打开' : '渲染工坊仅支持 .litematic / .schem'"
                        @click="renderEntry(entry)"
                    >
                        渲染</button
                    ><button
                        class="btn btn-ghost btn-sm"
                        :disabled="!!entry.error || busy"
                        @click="converting = entry"
                    >
                        转换
                    </button>
                </article>
            </div>
            <ResourcePagination v-model:page="page" :page-count="pages" :total="sorted.length" />
        </section>
        <details
            data-ui="ProjectionsView:d26a22685a69"
            v-if="results.length"
            open
            class="card recording-results"
        >
            <summary>上次操作结果</summary>
            <p
                data-ui="ProjectionsView:3b547d60eb96"
                v-for="result in results"
                :key="result.id"
                :class="{ failed: !result.ok }"
            >
                {{ result.name }}：{{ result.ok ? '已完成' : result.error
                }}<span
                    data-ui="ProjectionsView:3a6703ddc9e2"
                    v-if="result.path"
                    class="muted path"
                >
                    → {{ result.path }}</span
                >
            </p>
        </details>
        <ConfirmModal
            :open="!!confirm"
            :title="confirm === 'trash' ? '移入系统回收站' : '复制投影到实例'"
            :message="
                confirm === 'trash'
                    ? `将所选 ${selected.length} 个原位置的投影移入系统回收站，可从系统回收站恢复。`
                    : `将 ${selected.length} 个投影复制到所选实例的 schematics 目录，保留源文件。请确认目标游戏版本、加载器与投影模组兼容；操作期间目标游戏需关闭。`
            "
            :confirm-text="confirm === 'trash' ? '移入回收站' : '确认复制'"
            @cancel="confirm = ''"
            @confirm="execute(confirm as 'trash' | 'dispatch')"
        />
        </template>
    </section>
</template>

<style scoped>
.recordings-page {
    max-width: 1440px;
    margin: 0 auto;
    display: grid;
    gap: 20px;
    min-width: 0;
}
.page-head h1 {
    margin: 0 0 8px;
}
.card {
    padding: 22px;
}
.recording-library {
    display: flex;
    align-items: flex-start;
    gap: 20px;
    flex-wrap: nowrap;
    min-width: 0;
}
.recording-library h3 {
    margin: 0;
}
.actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
}
.recording-context {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    flex-wrap: wrap;
}
.recording-filters {
    display: grid;
    grid-template-columns: minmax(200px, 1.6fr) 150px 150px;
    gap: 12px;
}
.recording-controls {
    display: grid;
    gap: 16px;
}
.recording-controls p {
    margin: 0;
}
.operation-row :deep(.select-menu-btn) {
    max-width: 360px;
}
.recording-list {
    overflow: visible;
}
.recording-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 0;
    border-bottom: 1px solid var(--border);
}
.recording-row:last-child {
    border: 0;
}
.recording-info {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 6px;
}
.recording-info[draggable='true'] {
    cursor: grab;
}
.recording-info strong {
    overflow-wrap: anywhere;
}
.path {
    overflow-wrap: anywhere;
    font-size: 12px;
}
.recording-info .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.pagination {
    justify-content: center;
}
.empty {
    text-align: center;
    padding: 40px;
}
.failed {
    color: var(--danger, #e76b75);
}
.recording-results {
    max-height: 300px;
    overflow: auto;
}
.recording-results p {
    overflow-wrap: anywhere;
}
@media (max-width: 800px) {
    .recording-filters {
        grid-template-columns: 1fr 1fr;
    }
    .recording-filters input {
        grid-column: 1/-1;
    }
    .recording-row {
        gap: 10px;
    }
    .card {
        padding: 16px;
    }
}
.recordings-page {
    gap: 12px;
}
/* 渲染子页：整页锁定为视口高度，工坊吃满剩余空间（内容在工坊内部滚动，不外溢） */
.render-mode {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 100px);
    overflow: hidden;
}
.render-mode .projection-workshop-embed {
    flex: 1;
    min-height: 0;
    height: auto;
}
@media (max-width: 900px) {
    .render-mode {
        height: auto;
        overflow: visible;
    }
    .render-mode .projection-workshop-embed {
        height: auto;
        min-height: calc(100vh - 12rem);
    }
}
.recording-heading {
    display: flex;
    flex-direction: row;
    text-align: left;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
}
.recording-heading h1 {
    font-size: 24px;
}
.recording-heading small {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-dim);
}
.recording-heading p {
    margin: 4px 0;
}
.recording-library {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
}
.recording-library .path {
    min-width: 0;
    overflow-wrap: anywhere;
}
/* 使用说明与路径条并排：开关钉在右侧、整条可点；展开/收起时开关位置不变（控件不得因交互跳动） */
.recording-help {
    margin-left: auto;
    flex: 0 1 auto;
    min-width: 0;
    max-width: min(640px, 55%);
    line-height: 1.7;
}
.recording-help summary {
    display: block;
    text-align: right;
    cursor: pointer;
    white-space: nowrap;
}
/* display:block 会吃掉 <summary> 默认三角，这里显式补回展开方向箭头 */
.recording-help summary::after {
    content: " ▸";
}
.recording-help[open] summary::after {
    content: " ▾";
}
.recording-workspace {
    padding: 16px;
    display: grid;
    gap: 12px;
    min-height: 240px;
    align-content: start;
    position: relative;
}
.recording-workspace.sweeping {
    user-select: none;
}
.recording-workspace.drop-hot {
    outline: 2px dashed var(--accent);
    outline-offset: -4px;
}
.recording-workspace.drop-hot::after {
    content: '松开导入到当前实例';
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--accent);
    color: var(--on-accent);
    padding: 6px 14px;
    border-radius: 999px;
    font-size: 13px;
    z-index: 5;
    pointer-events: none;
}
.recording-filters {
    grid-template-columns: minmax(190px, 1.5fr) 130px 130px 130px;
}
.recording-controls {
    padding: 8px 0;
    gap: 8px;
    border-bottom: 1px solid var(--border);
}
.recording-controls .actions {
    gap: 8px;
}
.recording-row {
    padding: 12px 0;
    min-height: 72px;
}
.recording-row:hover {
    background: var(--hover);
}
.recording-row.selected {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
}
.recording-info strong {
    word-break: normal;
    overflow-wrap: break-word;
}
.recording-workspace > .empty {
    background: transparent;
    border: 0;
    min-height: 220px;
    box-shadow: none;
}
.recording-results {
    padding: 12px 16px;
}
.recording-results summary {
    cursor: pointer;
}
@media (max-width: 1150px) {
    .recording-filters {
        grid-template-columns: 1fr 1fr;
    }
    .recording-filters > input {
        grid-column: 1/-1;
    }
}
@media (max-width: 650px) {
    .recording-filters {
        display: flex;
        flex-direction: column;
    }
    .recording-workspace {
        padding: 12px;
    }
}
</style>
