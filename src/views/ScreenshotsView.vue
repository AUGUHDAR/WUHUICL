<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type {
    ScreenshotCatalog,
    ScreenshotEntry,
    ScreenshotRequest,
    ScreenshotResult
} from '@shared/screenshots';
import { errText } from '../api';
import { activeInstalled, displayVersionName, store, toast } from '../store';
import { managedImageUrl } from '../managedAssets';
import SelectMenu from '../components/SelectMenu.vue';
import ResourceScopeFields from '../components/ResourceScopeFields.vue';
import ResourcePathChip from '../components/ResourcePathChip.vue';
import ResourcePagination from '../components/ResourcePagination.vue';
import ConfirmModal from '../components/ConfirmModal.vue';
import EmptyState from '../components/EmptyState.vue';
import UiGlyph from '../components/UiGlyph.vue';
import { useSweepSelect } from '../composables/sweepSelect';
const PAGE_SIZE = 24;
const data = ref<ScreenshotCatalog>({ entries: [], warnings: [], folderRoot: '', instances: [] });
const loading = ref(false),
    busy = ref(false),
    error = ref(''),
    query = ref(''),
    page = ref(1),
    selected = ref<string[]>([]),
    target = ref(''),
    confirm = ref<'' | 'trash'>(''),
    dragActive = ref(false);
const results = ref<ScreenshotResult[]>([]);
/** 缩略图缓存：`id:modified` → kamucl-asset URL（后端按 mtime 失效，跨页复用） */
const thumbs = ref<Record<string, string>>({});
const lightbox = ref(-1);
const fullImage = ref('');
let generation = 0,
    disposed = false;
const activeFolder = computed(() => store.settings?.activeFolder || store.settings?.gameDir || '');
/** 内容范围：instance=只看当前实例；folder=版本文件夹内所有实例的截图目录聚合 */
const scope = ref<'instance' | 'folder'>(
    localStorage.getItem('kamucl.screenshotScope') === 'folder' ? 'folder' : 'instance'
);
watch(scope, (v) => localStorage.setItem('kamucl.screenshotScope', v));
/** 排序同样记住用户的选择（页面切换/重启后保留） */
const sort = ref(localStorage.getItem('kamucl.screenshotSort') || 'new');
watch(sort, (v) => localStorage.setItem('kamucl.screenshotSort', v));
const scopeOptions = [
    { value: 'instance', label: '当前实例', description: '只看所选实例的截图目录' },
    {
        value: 'folder',
        label: '版本文件夹内的所有',
        description: '聚合文件夹内全部实例各自的 screenshots 目录'
    }
];
const currentTarget = computed(() => {
    const inst = store.resourceVersionId;
    const folder = activeInstalled.value.find((v) => v.id === inst)?.folder || activeFolder.value;
    return inst ? { id: inst, folder } : null;
});
const versionOptions = computed(() =>
    activeInstalled.value.map((v) => ({
        value: v.id,
        label: displayVersionName(v),
        description: [v.mcVersion, v.loader, v.loaderVersion, v.isolated ? '已隔离' : '共享']
            .filter(Boolean)
            .join(' · ')
    }))
);
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
                    ((e.instanceId && e.instanceId === currentTarget.value.id) ||
                        (!e.instanceId &&
                            e.shared &&
                            !!e.folder &&
                            e.folder === currentTarget.value.folder)))) &&
            `${e.name} ${e.source} ${e.directory}`
                .toLowerCase()
                .includes(query.value.trim().toLowerCase())
    )
);
const sorted = computed(() => {
    const rows = [...filtered.value];
    if (sort.value === 'name') rows.sort((a, b) => a.name.localeCompare(b.name, 'zh-Hans-CN'));
    else if (sort.value === 'old') rows.sort((a, b) => a.modified - b.modified);
    else rows.sort((a, b) => b.modified - a.modified);
    return rows;
});
const pages = computed(() => Math.max(1, Math.ceil(sorted.value.length / PAGE_SIZE)));
const rows = computed(() =>
    sorted.value.slice((page.value - 1) * PAGE_SIZE, page.value * PAGE_SIZE)
);
const targets = computed(() =>
    data.value.instances.map((i) => ({
        value: JSON.stringify([i.folder, i.id]),
        // 与录像/投影页同口径：下拉里显示实例名而非内部 id
        label: i.name || i.id,
        description: i.isolated ? `已隔离 · versions/${i.id}` : '未隔离 · 共用版本文件夹根'
    }))
);
const size = (n: number) =>
    n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
const dateText = (n: number) => (n ? new Date(n).toLocaleString() : '未知时间');
watch([query, sort, scope], () => {
    page.value = 1;
    selected.value = [];
});
watch(page, () => {
    selected.value = [];
});
const thumbKey = (entry: ScreenshotEntry) => `${entry.id}:${entry.modified}`;
const thumbUrl = (entry: ScreenshotEntry) => thumbs.value[thumbKey(entry)];
/** 请求按原始 id 发送；缓存按 `id:modified` 记账，文件变化（mtime 变）自动重新拉取 */
async function loadThumbs(width: number, entries: ScreenshotEntry[]) {
    const missing = entries.filter((e) => !thumbs.value[thumbKey(e)]);
    if (!missing.length) return;
    try {
        const r = await invoke<{ thumbs: Record<string, unknown> }>(
            'screenshots:thumbs',
            missing.map((e) => e.id),
            width
        );
        for (const entry of missing) {
            const value = r.thumbs?.[entry.id];
            if (typeof value === 'string') thumbs.value[thumbKey(entry)] = managedImageUrl(value);
        }
    } catch {
        /* 缩略图失败不打断页面：卡片保持占位框 */
    }
}
watch(
    rows,
    (list) => {
        if (list.length) void loadThumbs(480, list);
    },
    { immediate: true }
);
async function refresh(silent = false, force = false) {
    const id = ++generation;
    if (!silent) loading.value = true;
    error.value = '';
    try {
        const next = await invoke<ScreenshotCatalog>(
            'screenshots:list',
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
    } catch (e) {
        if (!disposed && id === generation) error.value = errText(e);
    } finally {
        if (!disposed && id === generation) loading.value = false;
    }
}
function drag(event: DragEvent, entry: ScreenshotEntry) {
    // 纵向拖 = 扫选接管；横向仍走文件拖出
    if (interceptFileDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (busy.value || loading.value || store.editMode) return;
    window.kamucl.send(
        'screenshots:drag',
        selected.value.includes(entry.id) ? [...selected.value] : [entry.id]
    );
}

/** PCL 式扫选：按住卡片纵向拖 = 沿途选中/反选（点击选中行为不变） */
const workspaceEl = ref<HTMLElement | null>(null);
const { interceptFileDrag } = useSweepSelect({
    container: () => workspaceEl.value,
    itemSelector: '.shot-card',
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
 * 定时轮询已取消——这类目录只会在用户自己导入/游戏内保存时变化，
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
        lightbox.value < 0 &&
        document.visibilityState === 'visible'
    ) {
        lastAutoRefresh = now;
        void refresh(true);
    }
}
function toggleSelect(id: string) {
    if (busy.value) return;
    selected.value = selected.value.includes(id)
        ? selected.value.filter((s) => s !== id)
        : [...selected.value, id];
}
function selectPage() {
    const ids = rows.value.map((e) => e.id);
    selected.value = ids.every((id) => selected.value.includes(id))
        ? selected.value.filter((id) => !ids.includes(id))
        : [...new Set([...selected.value, ...ids])];
}
/** 两个选择按钮同一套状态感知语义：标签永远说明下一次点击的结果，不搞隐藏的开关 */
const pageAllSelected = computed(
    () => rows.value.length > 0 && rows.value.every((e) => selected.value.includes(e.id))
);
const filteredAllSelected = computed(
    () => filtered.value.length > 0 && filtered.value.every((e) => selected.value.includes(e.id))
);
function toggleAllFiltered() {
    selected.value = filteredAllSelected.value ? [] : filtered.value.map((e) => e.id);
}
async function open(entry?: ScreenshotEntry) {
    try {
        await invoke('screenshots:open', currentTarget.value ?? {}, entry?.id);
    } catch (e) {
        toast(errText(e), 'error');
    }
}
async function importPick() {
    if (busy.value) return;
    busy.value = true;
    try {
        const result = await invoke<ScreenshotResult[] | null>(
            'screenshots:import',
            currentTarget.value ?? {}
        );
        if (result) {
            results.value = result;
            const failed = result.filter((r) => !r.ok);
            toast(
                `已导入 ${result.length - failed.length} 张${failed.length ? `，${failed.length} 张未完成` : ''}`,
                failed.length ? 'error' : 'success'
            );
            await refresh();
        }
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
async function dropImport(event: DragEvent) {
    dragActive.value = false;
    if (busy.value) return;
    const files = Array.from(event.dataTransfer?.files ?? [])
        .map((f) => window.kamucl.getFilePath(f))
        .filter(Boolean);
    if (!files.length) return;
    busy.value = true;
    try {
        const result = await invoke<ScreenshotResult[]>(
            'screenshots:importAt',
            files,
            currentTarget.value ?? {}
        );
        results.value = result;
        const failed = result.filter((r) => !r.ok);
        toast(
            `已导入 ${result.length - failed.length} 张${failed.length ? `，${failed.length} 张未完成` : ''}`,
            failed.length ? 'error' : 'success'
        );
        await refresh();
    } catch (e) {
        toast('导入失败：' + errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
async function execute(action: ScreenshotRequest['action']) {
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
        const result =
            action === 'export'
                ? await invoke<ScreenshotResult[] | null>('screenshots:operate', {
                      action,
                      ids
                  })
                : await invoke<ScreenshotResult[] | null>('screenshots:operate', {
                      action,
                      ids,
                      target: destination
                  });
        if (result) {
            results.value = result;
            const failed = result.filter((r) => !r.ok);
            selected.value = failed.map((r) => r.id);
            toast(
                `已完成 ${result.length - failed.length} 张${failed.length ? `，${failed.length} 张未完成` : ''}`,
                failed.length ? 'error' : 'success'
            );
            await refresh();
        }
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
/** 灯箱基于当前页 rows 定位（翻页由分页控件负责）；大图展示 1600px 缓存档，加载完成前先显示网格缩略图 */
const lightboxEntry = computed(() =>
    lightbox.value >= 0 ? rows.value[lightbox.value] : undefined
);
watch(lightboxEntry, async (entry) => {
    fullImage.value = '';
    if (!entry) return;
    try {
        const r = await invoke<{ thumbs: Record<string, unknown> }>(
            'screenshots:thumbs',
            [entry.id],
            1600
        );
        const value = r.thumbs?.[entry.id];
        if (typeof value === 'string' && lightboxEntry.value === entry)
            fullImage.value = managedImageUrl(value);
    } catch {
        /* 大图失败时保留网格缩略图 */
    }
});
function escape(e: KeyboardEvent) {
    if (e.key === 'Escape') {
        if (lightbox.value >= 0) lightbox.value = -1;
        else confirm.value = '';
    }
    if (lightbox.value >= 0 && e.key === 'ArrowRight')
        lightbox.value = Math.min(rows.value.length - 1, lightbox.value + 1);
    if (lightbox.value >= 0 && e.key === 'ArrowLeft')
        lightbox.value = Math.max(0, lightbox.value - 1);
}
onMounted(() => {
    void refresh();
    window.addEventListener('keydown', escape);
    window.addEventListener('focus', autoRefresh);
    store.resourceDropHandler = dropImport;
});
onUnmounted(() => {
    disposed = true;
    generation++;
    window.removeEventListener('keydown', escape);
    window.removeEventListener('focus', autoRefresh);
    if (store.resourceDropHandler === dropImport) store.resourceDropHandler = null;
});
</script>

<template>
    <section class="screenshots-page" data-ui="screenshots:page">
        <header class="page-head screenshot-heading" data-ui="screenshots:heading">
            <div data-ui="ScreenshotsView:3e7b827962e6">
                <h1 data-ui="ScreenshotsView:2ac7467a0648">
                    截图 <small>{{ data.entries.length }} 张</small>
                </h1>
                <p class="muted">汇总版本文件夹内各实例 screenshots 目录的游戏截图。</p>
            </div>
            <div class="actions">
                <button data-ui="ScreenshotsView:78a31a977742" class="btn btn-ghost" @click="open()">
                    打开截图文件夹</button
                ><button
                    data-ui="ScreenshotsView:1594a41b4b07"
                    class="btn btn-gold"
                    :disabled="busy"
                    @click="importPick"
                >
                    导入截图…</button
                ><button
                    data-ui="ScreenshotsView:54ee9bdbb21c"
                    class="btn btn-ghost"
                    :disabled="busy || loading"
                    @click="refresh(false, true)"
                >
                    <!-- 文本符号 ↻ 收编 UiGlyph refresh（inventory H；GameView 同款范式） -->
                    <span v-if="loading" class="spin"></span>
                    <UiGlyph v-else name="refresh" :size="14" />
                    {{ loading ? '读取中' : '刷新' }}
                </button>
            </div>
        </header>
        <div class="screenshot-library" data-ui="screenshots:library">
            <ResourcePathChip label="版本文件夹" :path="data.folderRoot" />
            <details class="screenshot-help">
                <summary>使用说明</summary>
                <p>
                    截图保存在游戏工作目录的
                    screenshots
                    文件夹：开启版本隔离的实例在 versions/实例名/screenshots，未隔离的实例共用版本文件夹根的 screenshots。「当前实例」只看所选实例；「版本文件夹内的所有」把这些目录聚合到一起，每张截图标注所属实例。只列各
                    screenshots
                    目录顶层的图片，不扫描其他位置。点击卡片选中，悬浮出现「查看」按钮看大图；选中后可复制到其他实例、提取到任意文件夹或移入系统回收站。按住缩略图可拖出原图，把图片文件拖进页面即导入当前实例。
                </p>
            </details>
        </div>
        <section
            ref="workspaceEl"
            class="screenshot-workspace card"
            data-ui="screenshots:workspace"
            :class="{ 'drop-hot': dragActive }"
            @dragover.prevent="dragActive = true"
            @dragleave.self="dragActive = false"
        >
            <div class="screenshot-context">
                <ResourceScopeFields
                    v-model:scope="scope"
                    :scope-options="scopeOptions"
                    v-model:instance-id="store.resourceVersionId"
                    :instance-options="versionOptions"
                    :instance-locked="scope === 'folder'"
                    :disabled="busy"
                />
                <label class="filter-field">
                    <span>排序</span
                    ><SelectMenu
                        v-model="sort"
                        :disabled="busy"
                        :options="[
                            { value: 'new', label: '最新优先' },
                            { value: 'old', label: '最旧优先' },
                            { value: 'name', label: '按名称' }
                        ]"
                    />
                </label>
            </div>
            <input
                data-ui="ScreenshotsView:a4bc662c17f4"
                v-model="query"
                class="input"
                aria-label="搜索截图"
                placeholder="搜索截图名称、实例或路径…"
                :disabled="busy"
            />
            <div data-ui="ScreenshotsView:03ebe3d01f67" v-if="error" class="card" role="alert">
                {{ error }}
                <button
                    data-ui="ScreenshotsView:d86b658466b7"
                    class="btn btn-ghost"
                    @click="refresh(false, true)"
                >
                    重试
                </button>
            </div>
            <details data-ui="ScreenshotsView:4886ef0bbbad" v-if="data.warnings.length" class="card">
                <summary>部分目录未读取（{{ data.warnings.length }}）</summary>
                <p
                    data-ui="ScreenshotsView:a95d7ac71e30"
                    v-for="warning in data.warnings"
                    :key="warning"
                >
                    {{ warning }}
                </p>
            </details>
            <div v-if="filtered.length" class="screenshot-controls" data-ui="screenshots:controls">
                <div class="actions">
                    <button
                        data-ui="ScreenshotsView:f5fef6266bd6"
                        class="btn btn-ghost"
                        :disabled="busy || !rows.length"
                        @click="selectPage"
                    >
                        {{ pageAllSelected ? '取消选择当前页' : '选择当前页' }}</button
                    ><button
                        data-ui="ScreenshotsView:8436cfb2245d"
                        class="btn btn-ghost"
                        :disabled="busy || !filtered.length"
                        @click="toggleAllFiltered"
                    >
                        {{
                            filteredAllSelected
                                ? '取消全选'
                                : `全选筛选结果（${filtered.length}）`
                        }}</button
                    ><span>已选 {{ selected.length }} 张</span
                    ><button
                        data-ui="ScreenshotsView:ed589ee8e06c"
                        v-if="selected.length"
                        class="btn btn-ghost"
                        :disabled="busy"
                        @click="selected = []"
                    >
                        清空
                    </button>
                </div>
                <div
                    data-ui="ScreenshotsView:c59dbb632ead"
                    v-if="selected.length"
                    class="actions operation-row"
                >
                    <SelectMenu
                        v-model="target"
                        :disabled="busy"
                        :options="targets"
                        placeholder="选择目标实例"
                    /><button
                        data-ui="ScreenshotsView:85f3f8414f9d"
                        class="btn btn-gold"
                        :disabled="busy || target === ''"
                        @click="execute('copy')"
                    >
                        复制到实例</button
                    ><button
                        data-ui="ScreenshotsView:000f81fa6db3"
                        class="btn btn-ghost"
                        :disabled="busy"
                        @click="execute('export')"
                    >
                        提取到文件夹…</button
                    ><button
                        data-ui="ScreenshotsView:1699f50ec9f0"
                        class="btn btn-danger"
                        :disabled="busy"
                        @click="confirm = 'trash'"
                    >
                        移入回收站
                    </button>
                </div>
            </div>
            <div
                data-ui="ScreenshotsView:69fb5a4a1573"
                v-if="loading && data.entries.length"
                class="status-strip"
                role="status"
            >
                正在更新截图列表…
            </div>
            <!-- 骨架屏与加载后的真实网格同构（同一网格、同一卡片形状），加载瞬间不跳动 -->
            <div
                data-ui="ScreenshotsView:7c2e91f4a8d0"
                v-else-if="loading && !data.entries.length"
                class="shot-grid"
                aria-hidden="true"
            >
                <div v-for="i in 12" :key="i" class="shot-card shot-skeleton">
                    <div class="shot-thumb skeleton-pulse"></div>
                    <div class="shot-meta">
                        <span class="skeleton-pulse skeleton-line"></span>
                        <span class="skeleton-pulse skeleton-line short"></span>
                    </div>
                </div>
            </div>
            <!-- 空态收编 EmptyState（inventory B）：双分支文案拆 title/description，两颗引导按钮进 #action -->
            <EmptyState
                data-ui="ScreenshotsView:9196e415ffb9"
                v-else-if="!rows.length && !error"
                icon="image"
                :title="data.entries.length ? '没有匹配的截图' : '暂无截图'"
                :description="
                    data.entries.length
                        ? '请调整搜索或筛选条件。'
                        : '游戏内按截图键（默认 F2）拍摄后会保存在实例的 screenshots 文件夹，回到这里刷新即可；也可以把图片文件拖进来，导入为当前实例的截图。'
                "
            >
                <template #action>
                    <button
                        data-ui="ScreenshotsView:c0ed13df1c01"
                        v-if="data.entries.length"
                        class="btn btn-ghost"
                        @click="query = ''"
                    >
                        清除筛选</button
                    ><button
                        data-ui="ScreenshotsView:b6f4c2a11d93"
                        v-else
                        class="btn btn-gold"
                        :disabled="busy"
                        @click="importPick"
                    >
                        导入截图…
                    </button>
                </template>
            </EmptyState>
            <div
                v-else-if="rows.length"
                :inert="loading || !!error"
                class="shot-grid"
                data-ui="screenshots:grid"
            >
                <article
                    data-ui="ScreenshotsView:2bb2004aef70"
                    v-for="(entry, index) in rows"
                    :key="entry.id"
                    class="shot-card"
                    :class="{ selected: selected.includes(entry.id) }"
                    :aria-pressed="selected.includes(entry.id)"
                    :title="selected.includes(entry.id) ? '点击取消选中' : '点击选中'"
                    :data-sweep-key="entry.id"
                    tabindex="0"
                    role="button"
                    @click="toggleSelect(entry.id)"
                    @keydown.enter.prevent="toggleSelect(entry.id)"
                >
                    <!-- 文本符号 ✓ 收编 UiGlyph check（inventory H；选中角标） -->
                    <span class="shot-check" aria-hidden="true"><UiGlyph name="check" :size="12" /></span>
                    <div
                        data-ui="ScreenshotsView:3867dfbdf0e7"
                        class="shot-thumb"
                        :draggable="!busy && !loading && !store.editMode"
                        title="按住拖出截图文件"
                        @dragstart="drag($event, entry)"
                    >
                        <img
                            v-if="thumbUrl(entry)"
                            :src="thumbUrl(entry)"
                            loading="lazy"
                            alt="游戏截图"
                        />
                        <span v-else class="shot-placeholder">加载中…</span>
                        <button
                            data-ui="ScreenshotsView:5d2c9f8a1b34"
                            class="btn btn-sm shot-view"
                            :disabled="busy"
                            :title="'查看大图：' + entry.name"
                            @click.stop="lightbox = index"
                            @keydown.enter.stop
                        >
                            查看
                        </button>
                    </div>
                    <div data-ui="ScreenshotsView:80aad3d6d4fa" class="shot-meta">
                        <strong :title="entry.name">{{ entry.name }}</strong>
                        <div class="muted">
                            {{ entry.source }} · {{ size(entry.size) }} ·
                            {{ dateText(entry.modified) }}
                        </div>
                    </div>
                </article>
            </div>
            <ResourcePagination v-model:page="page" :page-count="pages" :total="sorted.length" />
        </section>
        <details
            data-ui="ScreenshotsView:d26a22685a69"
            v-if="results.length"
            open
            class="card screenshot-results"
        >
            <summary>上次操作结果</summary>
            <p
                data-ui="ScreenshotsView:3b547d60eb96"
                v-for="result in results"
                :key="result.id"
                :class="{ failed: !result.ok }"
            >
                {{ result.name }}：{{ result.ok ? '已完成' : result.error
                }}<span data-ui="ScreenshotsView:3a6703ddc9e2" v-if="result.path" class="muted path">
                    → {{ result.path }}</span
                >
            </p>
        </details>
        <Teleport to="body">
            <div v-if="lightboxEntry" class="shot-lightbox" role="dialog" aria-label="查看截图">
                <header>
                    <strong :title="lightboxEntry.name">{{ lightboxEntry.name }}</strong
                    ><span class="muted"
                        >{{ lightboxEntry.source }} · {{ dateText(lightboxEntry.modified) }}</span
                    ><button class="btn" @click="lightbox = -1">关闭</button>
                </header>
                <img :src="fullImage || thumbUrl(lightboxEntry)" alt="游戏截图大图" />
                <footer>
                    <button class="btn" :disabled="lightbox === 0" @click="lightbox--">
                        上一张</button
                    ><button class="btn" @click="open(lightboxEntry)">打开所在位置</button
                    ><button
                        class="btn"
                        :disabled="lightbox === rows.length - 1"
                        @click="lightbox++"
                    >
                        下一张
                    </button>
                </footer>
            </div>
        </Teleport>
        <ConfirmModal
            :open="!!confirm"
            title="移入系统回收站"
            :message="`将所选 ${selected.length} 张原位置的截图移入系统回收站，可从系统回收站恢复。`"
            confirm-text="移入回收站"
            @cancel="confirm = ''"
            @confirm="execute('trash')"
        />
    </section>
</template>

<style scoped>
.screenshots-page {
    max-width: 1440px;
    margin: 0 auto;
    display: grid;
    gap: 12px;
    min-width: 0;
}
.screenshot-heading {
    display: flex;
    flex-direction: row;
    text-align: left;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
}
/* 头部按钮间距与录像/投影页同款（10px） */
.screenshot-heading .actions {
    gap: 10px;
}
.screenshot-heading h1 {
    margin: 0 0 8px;
    font-size: 24px;
}
.screenshot-heading small {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-dim);
}
.screenshot-heading p {
    margin: 4px 0;
}
.card {
    padding: 22px;
}
.screenshot-library {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 12px;
    flex-wrap: nowrap;
    min-width: 0;
}
.screenshot-library .path {
    min-width: 0;
    overflow-wrap: anywhere;
}
/* 使用说明与路径条并排：开关钉在右侧、整条可点；展开/收起时开关位置不变（控件不得因交互跳动） */
.screenshot-help {
    margin-left: auto;
    flex: 0 1 auto;
    min-width: 0;
    max-width: min(640px, 55%);
    line-height: 1.7;
}
.screenshot-help summary {
    display: block;
    text-align: right;
    cursor: pointer;
    white-space: nowrap;
}
/* display:block 会吃掉 <summary> 默认三角，这里显式补回展开方向箭头 */
.screenshot-help summary::after {
    content: " ▸";
}
.screenshot-help[open] summary::after {
    content: " ▾";
}
.shot-skeleton {
    pointer-events: none;
}
.skeleton-pulse {
    /* 骨架脉冲底与卡面(card-2→实底族)同源：半透底会在缝隙处透出更深层 */
    background: var(--surface-solid);
    animation: shot-skeleton-pulse 1.4s ease-in-out infinite;
}
@keyframes shot-skeleton-pulse {
    0%,
    100% {
        opacity: 0.5;
    }
    50% {
        opacity: 1;
    }
}
.skeleton-line {
    display: block;
    height: 13px;
    width: 72%;
    border-radius: 6px;
}
.skeleton-line.short {
    width: 46%;
}
.actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
}
.screenshot-workspace {
    position: relative;
    padding: 16px;
    display: grid;
    gap: 12px;
    min-height: 240px;
    align-content: start;
}
.screenshot-workspace.drop-hot {
    outline: 2px dashed var(--accent);
    outline-offset: -4px;
}
.screenshot-workspace.drop-hot::after {
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
.screenshot-workspace.sweeping {
    user-select: none;
}
.screenshot-context {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    flex-wrap: wrap;
}
.filter-field {
    display: grid;
    gap: 6px;
    flex: 0 1 200px;
    min-width: 0;
    font-size: 12px;
    color: var(--text-dim);
}
.filter-field > :deep(.select-menu-btn) {
    width: 100%;
}
.screenshot-controls {
    /* 与其他页行同水平呼吸（2026-11-14 实测修正）：选择钮/移入回收站不再贴工作区左右缘
       （截图页为卡片网格，无行内 checkbox/删除，贴缘的是这条操作条） */
    padding: var(--space-2) var(--space-4);
    gap: 8px;
    border-bottom: 1px solid var(--border);
}
.operation-row :deep(.select-menu-btn) {
    max-width: 360px;
}
.shot-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 14px;
}
.shot-card {
    position: relative;
    display: grid;
    grid-template-rows: auto auto;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--card-2);
    overflow: hidden;
    cursor: pointer;
    padding: 0;
}
.shot-card:hover,
.shot-card:focus-visible {
    border-color: var(--accent);
}
.shot-card.selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
}
.shot-check {
    position: absolute;
    top: 10px;
    left: 10px;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--card);
    color: transparent;
    pointer-events: none;
}
.shot-card.selected .shot-check {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
}
.shot-thumb {
    position: relative;
    aspect-ratio: 16/9;
    /* 缩略图占位底实底化（模糊只在 .shell 一层） */
    background: var(--surface-solid);
    cursor: grab;
}
.shot-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
}
.shot-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-dim);
    font-size: 12px;
}
.shot-view {
    position: absolute;
    right: 8px;
    bottom: 8px;
    opacity: 0;
    transition: opacity 0.12s ease;
}
.shot-card:hover .shot-view,
.shot-card:focus-within .shot-view {
    opacity: 1;
}
.shot-meta {
    display: grid;
    gap: 4px;
    padding: 10px 12px 12px;
}
.shot-meta strong {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.shot-meta .muted {
    font-size: 12px;
}
.path {
    overflow-wrap: anywhere;
    font-size: 12px;
}
.pagination {
    justify-content: center;
}
/* 空态（EmptyState 组件）在工作区内保持无卡片外观、高度与原占位一致 */
.screenshot-workspace > .empty-state {
    min-height: 220px;
}
.failed {
    color: var(--danger, #e76b75);
}
.screenshot-results {
    padding: 12px 16px;
    max-height: 300px;
    overflow: auto;
}
.screenshot-results summary {
    cursor: pointer;
}
.screenshot-results p {
    overflow-wrap: anywhere;
}
.shot-lightbox {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
    background: #101418fa;
    color: white;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 24px;
}
.shot-lightbox header,
.shot-lightbox footer {
    display: flex;
    gap: 16px;
    align-items: center;
    max-width: 100%;
}
.shot-lightbox header strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.shot-lightbox img {
    min-height: 0;
    flex: 1;
    max-width: 100%;
    object-fit: contain;
}
.shot-lightbox .btn {
    color: white;
    background: #293139;
}
@media (max-width: 1150px) {
    .screenshot-context {
        flex-direction: column;
        align-items: stretch;
    }
    .filter-field {
        flex: none;
    }
}
@media (max-width: 650px) {
    .screenshot-workspace {
        padding: 12px;
    }
    .card {
        padding: 16px;
    }
}
</style>
