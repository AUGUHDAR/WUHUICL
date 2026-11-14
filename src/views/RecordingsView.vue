<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type {
    RecordingCatalog,
    RecordingEntry,
    RecordingRequest,
    RecordingResult
} from '@shared/recordings';
import { errText } from '../api';
import { activeInstalled, store, toast } from '../store';
import ContentSkeleton from '../components/ContentSkeleton.vue';
import SelectMenu from '../components/SelectMenu.vue';
import ResourceScopeFields from '../components/ResourceScopeFields.vue';
import ResourcePathChip from '../components/ResourcePathChip.vue';
import ResourcePagination from '../components/ResourcePagination.vue';
import ConfirmModal from '../components/ConfirmModal.vue';
import EmptyState from '../components/EmptyState.vue';
import UiGlyph from '../components/UiGlyph.vue';
import { useSweepSelect } from '../composables/sweepSelect';
const data = ref<RecordingCatalog>({ entries: [], warnings: [], library: '', instances: [] });
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
/** 排序跨会话记忆（与截图页同口径） */
const sort = ref(localStorage.getItem('kamucl.recordingSort') || 'new');
watch(sort, (v) => localStorage.setItem('kamucl.recordingSort', v));
const results = ref<RecordingResult[]>([]);
let generation = 0,
    disposed = false;
const activeFolder = computed(() => store.settings?.activeFolder || store.settings?.gameDir || '');
/** 内容范围：instance=只看当前实例；folder=版本文件夹内的所有（含收藏与共享目录） */
const scope = ref<'instance' | 'folder'>(
    localStorage.getItem('kamucl.recordingScope') === 'folder' ? 'folder' : 'instance'
);
watch(scope, (v) => localStorage.setItem('kamucl.recordingScope', v));
const currentTarget = computed(() => {
    const inst = store.resourceVersionId;
    const folder =
        activeInstalled.value.find((v) => v.id === inst)?.folder || activeFolder.value;
    return inst ? { id: inst, folder } : null;
});
const scopeOptions = [
    { value: 'instance', label: '当前实例', description: '只看所选实例的录像' },
    {
        value: 'folder',
        label: '版本文件夹内的所有',
        description: '聚合文件夹内全部实例与集中收藏的录像'
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
            (source.value === 'all' || (source.value === 'library' ? e.library : !e.library)) &&
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
const size = (n: number) =>
    n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
watch([query, kind, source, sort, scope], () => {
    page.value = 1;
    selected.value = [];
});
async function refresh(silent = false, force = false) {
    const id = ++generation;
    if (!silent) loading.value = true;
    error.value = '';
    try {
        const next = await invoke<RecordingCatalog>(
            'recordings:list',
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
function drag(event: DragEvent, entry: RecordingEntry) {
    // 纵向拖 = 扫选接管；横向仍走文件拖出
    if (interceptFileDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (busy.value || loading.value || store.editMode) return;
    window.kamucl.send(
        'recordings:drag',
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
/** 两个选择按钮同一套状态感知语义：标签永远说明下一次点击的结果（与截图页同款） */
const pageAllSelected = computed(
    () => rows.value.length > 0 && rows.value.every((e) => selected.value.includes(e.id))
);
const filteredAllSelected = computed(
    () => sorted.value.length > 0 && sorted.value.every((e) => selected.value.includes(e.id))
);
function toggleAllFiltered() {
    selected.value = filteredAllSelected.value ? [] : sorted.value.map((e) => e.id);
}
/** 拖入录像文件直接导入集中收藏（recordings:importAt，与按钮导入同一条路径） */
const dragActive = ref(false);
async function dropImport(event: DragEvent) {
    dragActive.value = false;
    if (busy.value) return;
    const files = Array.from(event.dataTransfer?.files ?? [])
        .map((f) => window.kamucl.getFilePath(f))
        .filter(Boolean);
    if (!files.length) return;
    busy.value = true;
    try {
        const result = await invoke<RecordingResult[] | null>('recordings:importAt', files);
        if (result) {
            results.value = result;
            const failed = result.filter((r) => !r.ok);
            toast(
                `已导入 ${result.length - failed.length} 个${
                    failed.length ? `，${failed.length} 个未完成` : ''
                }`,
                failed.length ? 'error' : 'success'
            );
        }
        await refresh(true);
    } catch (e) {
        toast('导入失败：' + errText(e), 'error');
    } finally {
        busy.value = false;
    }
}
async function open(entry?: RecordingEntry) {
    try {
        await invoke('recordings:open', entry?.id);
    } catch (e) {
        toast(errText(e), 'error');
    }
}
async function execute(action: RecordingRequest['action'] | 'import') {
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
            action === 'import'
                ? await invoke<RecordingResult[] | null>('recordings:import')
                : await invoke<RecordingResult[] | null>('recordings:operate', {
                      action,
                      ids,
                      target: destination
                  });
        if (result) {
            results.value = result;
            const failed = result.filter((r) => !r.ok);
            selected.value = failed.map((r) => r.id);
            toast(
                `已完成 ${result.length - failed.length} 项${failed.length ? `，${failed.length} 项未完成` : ''}`,
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
function escape(e: KeyboardEvent) {
    if (e.key === 'Escape') confirm.value = '';
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
    <section class="recordings-page" data-ui="recordings:page">
        <header class="page-head recording-heading" data-ui="recordings:heading">
            <div data-ui="RecordingsView:3e7b827962e6">
                <h1 data-ui="RecordingsView:2ac7467a0648">
                    录像 <small>{{ data.entries.length }} 个</small>
                </h1>
                <p class="muted">汇总全部绑定目录的 ReplayMod 与 Flashback 录像。</p>
            </div>
            <div class="actions">
                <button data-ui="RecordingsView:78a31a977742" class="btn btn-ghost" @click="open()">
                    打开收藏文件夹</button
                ><button
                    data-ui="RecordingsView:1594a41b4b07"
                    class="btn btn-gold"
                    :disabled="busy"
                    @click="execute('import')"
                >
                    导入录像…</button
                ><button
                    data-ui="RecordingsView:54ee9bdbb21c"
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
        <div class="recording-library" data-ui="recordings:library">
            <ResourcePathChip label="导入与收藏保存位置" :path="data.library" />
            <details class="recording-help">
                <summary>使用说明</summary>
                <p>
                    默认只显示当前实例的录像；切到「版本文件夹内的所有」可看整个文件夹。原录像保持原位。按住录像名称可拖出，勾选后可一起拖出。复制时校验完整性，同名文件自动添加序号。播放、渲染请在对应模组内完成；跨版本复制前请确认兼容性。
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
            <div class="recording-filters" data-ui="recordings:filters">
                <input
                    data-ui="RecordingsView:a4bc662c17f4"
                    v-model="query"
                    class="input"
                    aria-label="搜索录像"
                    placeholder="搜索录像名称、实例或路径…"
                    :disabled="busy"
                />
                <SelectMenu
                    v-model="kind"
                    :disabled="busy"
                    :options="[
                        { value: 'all', label: '全部格式' },
                        { value: 'replaymod', label: 'ReplayMod' },
                        { value: 'flashback', label: 'Flashback' }
                    ]"
                />
                <SelectMenu
                    v-model="source"
                    :disabled="busy"
                    :options="[
                        { value: 'all', label: '全部位置' },
                        { value: 'library', label: '集中收藏' },
                        { value: 'instances', label: '游戏目录' }
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
            <div data-ui="RecordingsView:03ebe3d01f67" v-if="error" class="card" role="alert">
                {{ error }}
                <button
                    data-ui="RecordingsView:d86b658466b7"
                    class="btn btn-ghost"
                    @click="refresh(false, true)"
                >
                    重试
                </button>
            </div>
            <details data-ui="RecordingsView:4886ef0bbbad" v-if="data.warnings.length" class="card">
                <summary>部分目录未读取（{{ data.warnings.length }}）</summary>
                <p
                    data-ui="RecordingsView:a95d7ac71e30"
                    v-for="warning in data.warnings"
                    :key="warning"
                >
                    {{ warning }}
                </p>
            </details>
            <div v-if="filtered.length" class="recording-controls" data-ui="recordings:controls">
                <div class="actions">
                    <button
                        data-ui="RecordingsView:f5fef6266bd6"
                        class="btn btn-ghost"
                        :disabled="busy || !rows.length"
                        @click="selectPage"
                    >
                        {{ pageAllSelected ? '取消选择当前页' : '选择当前页' }}</button
                    ><button
                        data-ui="RecordingsView:8436cfb2245d"
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
                        data-ui="RecordingsView:ed589ee8e06c"
                        v-if="selected.length"
                        class="btn btn-ghost"
                        :disabled="busy"
                        @click="selected = []"
                    >
                        清空
                    </button>
                </div>
                <div
                    data-ui="RecordingsView:c59dbb632ead"
                    v-if="selected.length"
                    class="actions operation-row"
                >
                    <button
                        data-ui="RecordingsView:85f3f8414f9d"
                        class="btn btn-gold"
                        :disabled="busy"
                        @click="execute('collect')"
                    >
                        复制到收藏</button
                    ><button
                        data-ui="RecordingsView:000f81fa6db3"
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
                        data-ui="RecordingsView:fa2131a8e18d"
                        class="btn btn-ghost"
                        :disabled="busy || target === ''"
                        @click="confirm = 'dispatch'"
                    >
                        复制到实例</button
                    ><button
                        data-ui="RecordingsView:1699f50ec9f0"
                        class="btn btn-danger"
                        :disabled="busy"
                        @click="confirm = 'trash'"
                    >
                        移入回收站
                    </button>
                </div>
            </div>
            <div
                data-ui="RecordingsView:69fb5a4a1573"
                v-if="loading && data.entries.length"
                class="status-strip"
                role="status"
            >
                正在更新录像列表…
            </div>
            <ContentSkeleton
                v-if="loading && !data.entries.length"
                class="card"
                label="正在读取录像目录…"
            />
            <!-- 空态收编 EmptyState（inventory B）：双分支文案拆 title/description，清除筛选进 #action -->
            <EmptyState
                data-ui="RecordingsView:9196e415ffb9"
                v-else-if="!rows.length && !error"
                icon="clip"
                :title="data.entries.length ? '没有匹配的录像' : '暂无录像'"
                :description="
                    data.entries.length
                        ? '请调整搜索或筛选条件。'
                        : '可导入 .mcpr 或 Flashback .zip，或在游戏中完成录制并保存。'
                "
            >
                <template v-if="data.entries.length" #action>
                    <button
                        data-ui="RecordingsView:c0ed13df1c01"
                        class="btn btn-ghost"
                        @click="
                            query = '';
                            kind = 'all';
                            source = 'all';
                        "
                    >
                        清除筛选
                    </button>
                </template>
            </EmptyState>
            <div
                v-else-if="rows.length"
                :inert="loading || !!error"
                class="recording-list"
                data-ui="recordings:list"
            >
                <article
                    data-ui="RecordingsView:2bb2004aef70"
                    v-for="entry in rows"
                    :key="entry.id"
                    class="recording-row"
                    :class="{ selected: selected.includes(entry.id) }"
                    :data-sweep-key="entry.id"
                >
                    <input
                        data-ui="RecordingsView:d29a579f673a"
                        v-model="selected"
                        type="checkbox"
                        :value="entry.id"
                        :aria-label="`选择 ${entry.name}`"
                        :disabled="busy"
                    />
                    <div
                        data-ui="RecordingsView:3867dfbdf0e7"
                        class="recording-info"
                        :draggable="!busy && !loading && !store.editMode"
                        title="按住拖出录像文件"
                        @dragstart="drag($event, entry)"
                    >
                        <strong
                            data-ui="RecordingsView:6f5f6ab090cd"
                            tabindex="0"
                            :title="entry.name"
                            >{{ entry.name }}</strong
                        >
                        <div data-ui="RecordingsView:80aad3d6d4fa" class="muted">
                            {{ entry.kind === 'replaymod' ? 'ReplayMod' : 'Flashback' }} ·
                            {{ entry.source }} · {{ size(entry.size) }} ·
                            {{ new Date(entry.modified).toLocaleString() }}
                        </div>
                        <div
                            data-ui="RecordingsView:2185a2a82be6"
                            class="muted path"
                            :title="entry.directory"
                        >
                            {{ entry.directory }}
                        </div>
                    </div>
                    <button
                        data-ui="RecordingsView:e41d05dae554"
                        class="btn btn-ghost btn-sm"
                        @click="open(entry)"
                    >
                        定位文件
                    </button>
                </article>
            </div>
            <ResourcePagination v-model:page="page" :page-count="pages" :total="sorted.length" />
        </section>
        <details
            data-ui="RecordingsView:d26a22685a69"
            v-if="results.length"
            open
            class="card recording-results"
        >
            <summary>上次操作结果</summary>
            <p
                data-ui="RecordingsView:3b547d60eb96"
                v-for="result in results"
                :key="result.id"
                :class="{ failed: !result.ok }"
            >
                {{ result.name }}：{{ result.ok ? '已完成' : result.error
                }}<span data-ui="RecordingsView:3a6703ddc9e2" v-if="result.path" class="muted path">
                    → {{ result.path }}</span
                >
            </p>
        </details>
        <ConfirmModal
            :open="!!confirm"
            :title="confirm === 'trash' ? '移入系统回收站' : '复制录像到实例'"
            :message="
                confirm === 'trash'
                    ? `将所选 ${selected.length} 个原位置的录像移入系统回收站，可从系统回收站恢复。`
                    : `将 ${selected.length} 个录像复制到所选实例的对应模组目录，保留源文件。请确认目标游戏版本、加载器与录像模组兼容；操作期间目标游戏需关闭。`
            "
            :confirm-text="confirm === 'trash' ? '移入回收站' : '确认复制'"
            @cancel="confirm = ''"
            @confirm="execute(confirm as 'trash' | 'dispatch')"
        />
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
}
.recording-workspace.drop-hot {
    outline: 2px dashed var(--accent);
    outline-offset: -4px;
}
.recording-workspace.drop-hot::after {
    content: '松开导入到集中收藏';
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
.recording-workspace {
    position: relative;
}
.recording-workspace.sweeping {
    user-select: none;
}
.recording-filters {
    grid-template-columns: minmax(190px, 1.5fr) 130px 130px 130px;
}
.recording-controls {
    /* 与下方行同水平呼吸（--space-4）：选择钮/移入回收站不再贴工作区左右缘 */
    padding: var(--space-2) var(--space-4);
    gap: 8px;
    border-bottom: 1px solid var(--border);
}
.recording-controls .actions {
    gap: 8px;
}
.recording-row {
    /* 行水平呼吸（2026-11-14 实测修正：checkbox 贴左缘/定位钮贴右缘）：hover/选中底色铺满整行，
       左 --space-4 右 --space-4 让行首 checkbox 与行尾按钮各留一档呼吸 */
    padding: var(--space-3) var(--space-4);
    min-height: 72px;
}
/* 行 hover 统一规则（inventory L，2026-11-14 修正版；对照 GameView .installed-row）：
   行背景 var(--hover) + 行内操作整组苏醒（card-2 底 + border-strong 边 + 文字提亮），无位移；
   accent 只在指向按钮本身 :hover 时出现。全页仅此一处 hover 定义（原 814/938 两处已归一）。 */
.recording-row:hover,
.recording-row:focus-within {
    background: var(--hover);
}
.recording-row:hover :is(.btn):not(:disabled),
.recording-row:focus-within :is(.btn):not(:disabled) {
    background: var(--card-2);
    border-color: var(--border-strong);
    color: var(--text);
}
.recording-row.selected {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
}
.recording-info strong {
    word-break: normal;
    overflow-wrap: break-word;
}
/* 空态（EmptyState 组件）在工作区内保持无卡片外观、高度与原占位一致 */
.recording-workspace > .empty-state {
    min-height: 220px;
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
