<script setup lang="ts">
/**
 * 通用文件管理视图：模组 / 资源包 / 光影包共用。
 * 两种内容范围：当前实例 = versions/<id>/<rel>（后端按隔离状态折算）；
 * 版本文件夹内的所有 = 共享根目录 + 每个已装实例的该子目录聚合（按行标注所属实例）。
 */
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import {
    getModIcons,
    importResources,
    applyModUpdates,
    checkModUpdates,
    errText,
    fsPath,
    listFs,
    listFsAggregate,
    modCatalogAt,
    modIconsAt,
    openDir,
    removeFs,
    selectFiles,
    setModsEnabledAt,
    setModsLockedAt,
    toggleDisableFs
} from '../api';
import {
    displayVersionName as versionLabel,
    activeInstalled,
    selectedInstance,
    refreshInstalled,
    store,
    toast
} from '../store';
import { resourceDisplayName, pageSelection } from '@shared/uiPresentation';
import ContentSkeleton from './ContentSkeleton.vue';
import EmptyState from './EmptyState.vue';
import UiGlyph from './UiGlyph.vue';
import ModMigrationModal from './ModMigrationModal.vue';
import ModVersionModal from './ModVersionModal.vue';
import type { ManagedMod, ModOperationResult } from '@shared/modManagement';
import type { AggregatedFs } from '../api';
import ConfirmModal from './ConfirmModal.vue';
import DupCleanModal from './DupCleanModal.vue';
import SelectMenu from './SelectMenu.vue';
import TabPills, { type TabPillItem } from './TabPills.vue';
import ResourceScopeFields from './ResourceScopeFields.vue';
import ResourcePathChip from './ResourcePathChip.vue';
import ResourcePagination from './ResourcePagination.vue';
import { useSweepSelect } from '../composables/sweepSelect';
import type { FsEntry, ModUpdateEntry, ModUpdateReport } from '@shared/types';

/** 聚合视图里的每一行：FsEntry 携带 instanceId/instanceName/directory。
 *  更新信息不再挂到行上——检测结果按文件名索引在 `updateOf(row)` 里查（同一行在任何分类下都一致）。 */
type Row = FsEntry & { instanceId?: string; instanceName?: string };

/** 行身份键：聚合视图里同名文件可能属于不同实例，必须带上实例区分 */
const rowKey = (e: Row) => (e.instanceId ? e.instanceId + '/' + e.name : e.name);
/** 行的资源目录（聚合视图用后端返回的绝对目录；实例视图用解析出的目录） */
const rowDir = (e: Row) => e.directory || '';

function dragResource(event: DragEvent, entry: Row) {
    // 纵向拖 = PCL 式扫选接管；横向仍走文件拖出
    if (interceptFileDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    if (store.editMode || batchBusy.value || loading.value || !currentVersion.value) return;
    // 拖出走实例目录形状（versions/<id>/<rel>）；聚合行用后端给的目录反推
    const files = selection.value.has(rowKey(entry))
        ? visibleEntries.value.filter((e) => selection.value.has(rowKey(e)))
        : [entry];
    if (scope.value === 'folder') {
        for (const file of files) {
            window.kamucl.send('fs:dragAt', rowDir(file), [file.name]);
        }
        return;
    }
    window.kamucl.send(
        'fs:drag',
        effectiveRel.value,
        files.map((f) => f.name),
        currentVersion.value.folder || activeFolder.value
    );
}

/** PCL 式扫选（仅模组页有选择模型；资源包/光影包无批量操作不接） */
const listCard = ref<HTMLElement | null>(null);
const { interceptFileDrag } = useSweepSelect({
    container: () => listCard.value,
    itemSelector: '.fm-row',
    enabled: () =>
        props.rel === 'mods' && !batchBusy.value && !loading.value && !store.editMode,
    isSelected: (key) => selection.value.has(key),
    setMany: (keys, selected) => {
        const next = new Set(selection.value);
        for (const k of keys) selected ? next.add(k) : next.delete(k);
        selection.value = next;
    }
});
const props = defineProps<{
    /** 页面标题，如「模组」 */
    title: string;
    /** 标题下的一句说明 */
    subtitle?: string;
    /** 相对游戏目录的子目录：mods / resourcepacks / shaderpacks */
    rel: string;
    /** 打开目录按钮文字 */
    openLabel: string;
    /** 空状态文案 */
    emptyText: string;
    /** 标题旁与空状态的内联 SVG 图标 */
    icon: string;
}>();

const page = ref(1);
const PAGE_SIZE = 100;
const entries = ref<Row[]>([]);
const loading = ref(true);
const loadError = ref('');
const resolvedPath = ref('');
const opening = ref(false);

// ---------------- 版本上下文（模组/资源包/光影包按游戏版本管理） ----------------
/** 当前选中版本（默认第一个已装版本；store.resourceVersionId 三页共享） */
const activeFolder = computed(() => store.settings?.activeFolder || store.settings?.gameDir || '');
const availableVersions = activeInstalled;
const currentVersion = computed(() => {
    const list = availableVersions.value;
    if (!list.length) return null;
    return list.find((v) => v.id === store.resourceVersionId) ?? list[0];
});

/** 内容范围：instance=只看当前实例；folder=版本文件夹根的共享目录 */
const scope = ref<'instance' | 'folder'>(
    localStorage.getItem('kamucl.fileScope.' + props.rel) === 'folder' ? 'folder' : 'instance'
);
watch(scope, (v) => localStorage.setItem('kamucl.fileScope.' + props.rel, v));
const scopeOptions = [
    { value: 'instance', label: '当前实例', description: '只看所选实例的该子目录' },
    {
        value: 'folder',
        label: '版本文件夹内的所有',
        description: '共享目录 + 每个实例分别列出，操作按行定位回所属实例'
    }
];

/** 实际管理的相对目录：实例视图 = versions/<id>/<rel>（后端按隔离状态折算）；
 * 文件夹视图 = <rel>（版本文件夹根的共享目录） */
const effectiveRel = computed(() => {
    const v = currentVersion.value;
    if (!v) return '';
    return scope.value === 'folder' ? props.rel : `versions/${v.id}/${props.rel}`;
});

/** 目录不存在时视为空列表（隔离版本刚开启、尚未产生该子目录） */
let loadedContext = '';
let loadGeneration = 0;
let updateOperation = 0;
/** 聚合视图的实例清单与告警（实例视图为空） */
const aggregateInstances = ref<AggregatedFs['instances']>([]);
const aggregateWarnings = ref<string[]>([]);

async function load() {
    const generation = ++loadGeneration;
    const v = currentVersion.value;
    const context = JSON.stringify([
        effectiveRel.value,
        v?.folder || activeFolder.value,
        scope.value
    ]);
    if (context !== loadedContext) {
        entries.value = [];
        resolvedPath.value = '';
        selection.value = new Set();
        page.value = 1;
    }
    loadedContext = context;
    loadError.value = '';
    resolvedPath.value = '';
    if (!v) {
        loading.value = false;
        return;
    }
    loading.value = true;
    try {
        const folder = v.folder || activeFolder.value;
        if (scope.value === 'folder') {
            // 版本文件夹内的所有：共享根 + 各实例聚合
            const agg = await listFsAggregate(props.rel, folder);
            if (generation === loadGeneration) {
                entries.value = agg.entries;
                aggregateInstances.value = agg.instances;
                aggregateWarnings.value = agg.warnings;
                // 展示用的聚合视图路径，按平台选分隔符
                resolvedPath.value = [folder, props.rel].join(
                    window.kamucl.platform === 'win32' ? '\\' : '/'
                );
                page.value = Math.min(page.value, pageCount.value);
                selection.value = new Set(
                    [...selection.value].filter((key) =>
                        agg.entries.some((e) => rowKey(e as Row) === key)
                    )
                );
                void loadCatalog(generation);
            }
        } else {
            const rel = effectiveRel.value;
            const [result, directory] = await Promise.all([
                listFs(rel, folder),
                fsPath(rel, folder)
            ]);
            if (generation === loadGeneration) {
                entries.value = result;
                aggregateInstances.value = [];
                aggregateWarnings.value = [];
                resolvedPath.value = directory;
                page.value = Math.min(page.value, pageCount.value);
                selection.value = new Set(
                    [...selection.value].filter((name) => result.some((e) => e.name === name))
                );
                void loadCatalog(generation);
            }
        }
    } catch (e) {
        if (generation === loadGeneration) loadError.value = errText(e);
    } finally {
        if (generation === loadGeneration) loading.value = false;
    }
}

const importing = ref(false);
/** 拖拽与右上角「导入X…」共用同一条导入路径（同一功能一个入口，接受多种来源） */
async function importFiles(files: string[]) {
    const v = currentVersion.value;
    if (!v) {
        toast('请先选择当前文件夹中的游戏版本', 'error');
        return;
    }
    if (importing.value || !files.length) return;
    importing.value = true;
    try {
        const count = await importResources(files, v.id, v.folder || activeFolder.value, props.rel);
        toast(`已导入 ${count} 项到 ${v.id} · ${props.title}`, 'success');
        await load();
    } catch (e) {
        toast('导入失败：' + errText(e), 'error');
    } finally {
        importing.value = false;
    }
}
async function onPickImport() {
    if (!currentVersion.value || importing.value) return;
    const ext = props.rel === 'mods' ? 'jar' : 'zip';
    const files = await selectFiles([`${props.title}|*.${ext}`]).catch(() => [] as string[]);
    if (files.length) await importFiles(files);
}
async function dropResources(event: DragEvent) {
    const files = Array.from(event.dataTransfer?.files ?? [])
        .map((f) => window.kamucl.getFilePath(f))
        .filter(Boolean);
    await importFiles(files);
}
onUnmounted(() => {
    if (store.resourceDropHandler === dropResources) store.resourceDropHandler = null;
});
onMounted(async () => {
    store.resourceDropHandler = dropResources;
    if (!store.installed.length) await refreshInstalled();
    if (!store.resourceVersionId && store.installed.length) {
        store.resourceVersionId = store.installed[0].id;
    }
    void load();
});

watch([effectiveRel, activeFolder], () => {
    updateOperation++;
    dupOpen.value = false;
    delModal.open = false;
    updatePanel.checking = false;
    updatePanel.applying = false;
    updatePanel.report = null;
    migrationOpen.value = false;
    void load();
});
onUnmounted(() => {
    loadGeneration++;
    updateOperation++;
});
watch(
    () => store.fsRefreshTick,
    () => void load()
);

// ---------------- 路径显示（超长中间省略 + 点击复制） ----------------
/** 清理重复 MOD 弹窗 */
const dupOpen = ref(false);
const migrationOpen = ref(false);

/** 中间省略的路径：versions/neo…2.0.75/mods */
const fullPath = computed(() => resolvedPath.value);
const pageMods = computed(() => visibleEntries.value.filter(isModEntry).map((e) => rowKey(e)));
const pageChecked = computed(() => pageSelection(pageMods.value, selection.value));
function togglePageSelection() {
    const next = new Set(selection.value);
    for (const key of pageMods.value) pageChecked.value.all ? next.delete(key) : next.add(key);
    selection.value = next;
}
/** 批量条「全选筛选结果 ↔ 取消全选」同一按钮状态感知（与截图页同款语义） */
const allFilteredSelected = computed(() => {
    const keys = displayEntries.value.filter(isModEntry).map(rowKey);
    return keys.length > 0 && keys.every((k) => selection.value.has(k));
});
function toggleAllFiltered() {
    if (allFilteredSelected.value) selection.value = new Set();
    else selectAll(true);
}
const resourceCount = computed(() =>
    props.rel === 'mods'
        ? entries.value.filter(isModEntry).length
        : entries.value.filter((e) => e.isDir || /\.zip$/i.test(e.name)).length
);
const readableName = (e: FsEntry) =>
    props.rel === 'resourcepacks' ? resourceDisplayName(e.name) : e.name;

/** 聚合视图的标题栏说明：N 个实例 */
const scopeSummary = computed(() =>
    scope.value === 'folder'
        ? aggregateInstances.value.length
            ? `共享目录 + ${aggregateInstances.value.length} 个实例`
            : '仅共享目录（各实例使用共享目录）'
        : '当前实例'
);

// ---------------- 顶栏搜索联动（过滤文件名） ----------------
const batchResults = ref<ModOperationResult[]>([]),
    catalogError = ref('');
/** 分类 tab 与排序跨会话记忆（内容组织：记住用户的选择）；恢复只认合法值，不直接落回「更新」tab */
const storedFilter = localStorage.getItem('kamucl.' + props.rel + '.modFilter');
const storedSort = localStorage.getItem('kamucl.' + props.rel + '.sort');
const localSearch = ref(''),
    modFilter = ref(
        ['all', 'enabled', 'disabled'].includes(storedFilter) ? storedFilter : 'all'
    ),
    sortBy = ref(['name', 'date', 'size'].includes(storedSort) ? storedSort : 'name'),
    /** 目录 → 该目录的模组清单（聚合视图下按实例目录分别缓存） */
    catalog = ref<Record<string, ManagedMod>>({}),
    selection = ref(new Set<string>()),
    batchBusy = ref(false),
    switchFile = ref('');
const keyword = computed(() => (localSearch.value || store.searchKeyword).trim().toLowerCase());
/** 取某一行的目录清单条目（聚合视图按 rowKey 分桶：实例/文件名） */
const catalogOf = (e: Row) => catalog.value[rowKey(e)];
const filtered = computed(() => {
    const rows = entries.value.filter(
        (e) =>
            (!keyword.value ||
                (e.name + ' ' + (catalogOf(e)?.name || ''))
                    .toLowerCase()
                    .includes(keyword.value)) &&
            (props.rel !== 'mods' ||
                modFilter.value === 'all' ||
                // updates 放行全部行，可更新条件由 displayEntries 叠一层（见上）
                modFilter.value === 'updates' ||
                (modFilter.value === 'enabled' && /\.jar$/i.test(e.name)) ||
                (modFilter.value === 'disabled' && /\.jar\.disabled$/i.test(e.name)))
    );
    return rows.sort((a, b) =>
        sortBy.value === 'date'
            ? b.mtime - a.mtime
            : sortBy.value === 'size'
              ? b.size - a.size
              : a.name.localeCompare(b.name, 'zh-CN', { numeric: true })
    );
});
/** 聚合视图按目录分组：批量操作必须逐目录调用，不能整页传一个实例 */
function groupByDir(rows: Row[]): Map<string, Row[]> {
    const map = new Map<string, Row[]>();
    for (const row of rows) {
        const dir = rowDir(row) || resolvedPath.value;
        map.set(dir, [...(map.get(dir) || []), row]);
    }
    return map;
}
async function loadCatalog(generation: number) {
    if (props.rel !== 'mods') return;
    const v = currentVersion.value;
    if (!v) return;
    try {
        // 目录 → 实例 id（聚合视图按行定位；实例视图只有一个目录）
        const dirToInstance = new Map<string, string>();
        for (const e of entries.value) dirToInstance.set(rowDir(e), e.instanceId ?? '');
        const dirs = scope.value === 'folder' ? [...dirToInstance.keys()].filter(Boolean) : [''];
        const buckets = await Promise.all(
            dirs.map(async (dir) =>
                dir
                    ? { dir, list: await modCatalogAt(dir) }
                    : {
                          dir: '',
                          list: (await window.kamucl.invoke(
                              'mods:catalog',
                              v.id,
                              v.folder || activeFolder.value
                          )) as ManagedMod[]
                      }
            )
        );
        if (generation !== loadGeneration) return;
        const next: Record<string, ManagedMod> = {};
        for (const { dir, list } of buckets) {
            const instanceId = dir ? (dirToInstance.get(dir) ?? '') : '';
            for (const m of list) next[instanceId ? instanceId + '/' + m.fileName : m.fileName] = m;
        }
        catalog.value = next;
        catalogError.value = '';
    } catch (e) {
        if (generation === loadGeneration) {
            catalog.value = {};
            catalogError.value = errText(e);
        }
    }
}
function selectMod(key: string, checked: boolean) {
    const s = new Set(selection.value);
    checked ? s.add(key) : s.delete(key);
    selection.value = s;
}
function selectAll(all = false) {
    const s = new Set(selection.value);
    for (const e of (all ? displayEntries.value : visibleEntries.value).filter(isModEntry))
        s.add(rowKey(e));
    selection.value = s;
}
/** 「更新」分类批量条：勾选即所选，更新操作只作用于其中可更新且未锁定的文件 */
/** 选中项里可更新的数量（PCL 底栏「更新」按钮的可用性判定）：任何分类下选中都算 */
const selectedUpdatableCount = computed(
    () =>
        entries.value.filter(
            (e) => updateOf(e)?.update && selection.value.has(rowKey(e)) && !catalogOf(e)?.locked
        ).length
);
/** 批量启停/锁定：从选择键还原出行，再按目录分组逐目录调用（聚合视图不能整页传一个实例） */
async function batch(action: 'enable' | 'disable' | 'lock' | 'unlock') {
    const v = currentVersion.value;
    if (!v || batchBusy.value || loading.value || loadError.value) return;
    const generation = loadGeneration;
    batchBusy.value = true;
    try {
        const rows = entries.value.filter((e) => selection.value.has(rowKey(e)));
        const byDir = groupByDir(rows);
        const channel =
            action === 'lock' || action === 'unlock' ? 'mods:setLockedAt' : 'mods:setEnabledAt';
        const flag = action === 'lock' || action === 'enable';
        const all: ModOperationResult[] = [];
        for (const [dir, group] of byDir) {
            const results = (await window.kamucl.invoke(
                channel,
                dir,
                group.map((e) => e.name),
                flag
            )) as ModOperationResult[];
            // 回填实例前缀，保证结果里的 fileName 仍能定位回选中键
            const prefix = scope.value === 'folder' ? (group[0]?.instanceId ?? '') : '';
            for (const r of results)
                all.push(prefix ? { ...r, fileName: prefix + '/' + r.fileName } : r);
        }
        const failed = all.filter((r) => !r.ok);
        toast(
            '已处理 ' +
                (all.length - failed.length) +
                ' 项' +
                (failed.length ? '；' + failed.length + ' 项失败：' + failed[0].error : ''),
            failed.length ? 'error' : 'success'
        );
        if (generation === loadGeneration) {
            batchResults.value = all.map((r) => ({
                ...r,
                fileName: r.fileName.split('/').pop() || r.fileName
            }));
            const remaining = new Set(selection.value);
            for (const r of all) r.ok ? remaining.delete(r.fileName) : remaining.add(r.fileName);
            selection.value = remaining;
            await load();
        }
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        batchBusy.value = false;
    }
}
/** 单行锁定/解锁：按该行所在目录调用（实例视图与聚合视图统一） */
async function toggleLock(entry: Row) {
    const v = currentVersion.value;
    if (!v || batchBusy.value) return;
    const locked = !catalogOf(entry)?.locked;
    const generation = loadGeneration;
    batchBusy.value = true;
    try {
        const results = (
            scope.value === 'folder'
                ? await setModsLockedAt(rowDir(entry), [entry.name], locked)
                : ((await window.kamucl.invoke(
                      'mods:setLocked',
                      v.id,
                      v.folder || activeFolder.value,
                      [entry.name],
                      locked
                  )) as ModOperationResult[])
        ) as ModOperationResult[];
        const failed = results.filter((r) => !r.ok);
        toast(
            failed.length
                ? '操作失败：' + failed[0].error
                : `${locked ? '已锁定' : '已解除锁定'} ${entry.name}`,
            failed.length ? 'error' : 'success'
        );
        if (generation === loadGeneration) await load();
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        batchBusy.value = false;
    }
}
watch([effectiveRel, activeFolder, scope], () => {
    selection.value = new Set();
    batchResults.value = [];
    catalog.value = {};
    catalogError.value = '';
    // 搜索词/分类/排序是用户的选择，切实例或范围时保留（内容组织：记住用户的选择）
    switchFile.value = '';
});
watch([localSearch, modFilter, sortBy], () => (page.value = 1));
watch(modFilter, (v) => localStorage.setItem('kamucl.' + props.rel + '.modFilter', v));
watch(sortBy, (v) => localStorage.setItem('kamucl.' + props.rel + '.sort', v));

// ---------------- MOD 更新检测（PCL 口径：静默后台检测 + 行内箭头） ----------------
// 与 PCL 的等价关系：
//   1. 进模组页就开始查，不等用户点任何「更新」入口（PCL 的 LocalResourceOnlineLoader）；
//   2. 结果按行合并进普通列表——可更新行在名字后面挂一个下载箭头，其余行完全不动；
//   3. 「可更新」只是顶部一颗筛选药丸，**可更新数为 0 时整颗药丸不渲染**
//      （PCL：BtnFilterCanUpdate.Visibility = Filter = CanUpdate OrElse UpdateCount > 0）；
//   4. 没有「更新」专属页面、没有独立列表、没有整页加载态——检测期间照常浏览文件。
const updatePanel = reactive({
    checking: false,
    /** 只在「用户主动重新检测」失败时置位；静默失败走 note，不打断浏览 */
    error: '',
    /** 静默降级提示（联网失败但还有缓存可用） */
    note: '',
    report: null as ModUpdateReport | null,
    applying: false,
    itemState: {} as Record<string, 'start' | 'ok' | 'error' | undefined>,
    itemError: {} as Record<string, string>
});
/** 检测结果按文件名索引：行渲染直接查表，不再另建一份列表 */
const updateByFile = computed(() => {
    const map = new Map<string, ModUpdateEntry>();
    for (const e of updatePanel.report?.entries ?? []) map.set(e.fileName, e);
    return map;
});
/** 当前实例里可更新的模组数（顶部药丸角标 & 药丸显隐的唯一依据） */
const updatableCount = computed(() => updatePanel.report?.entries.filter((e) => e.update).length ?? 0);
const updateOf = (e: Row): ModUpdateEntry | undefined => updateByFile.value.get(e.name);
const unmatchedCount = computed(
    () => updatePanel.report?.entries.filter((e) => !e.source).length ?? 0
);
const latestCount = computed(
    () =>
        updatePanel.report?.entries.filter((e) => e.alreadyLatest || (e.source && !e.update))
            .length ?? 0
);
/** 「可更新」筛选是否生效（PCL 的 FilterType.CanUpdate） */
const updatesTab = computed(() => props.rel === 'mods' && modFilter.value === 'updates');
/** 统一展示行：可更新筛选只是 filtered 之外的一层叠加，行本身还是原始文件行 */
const displayEntries = computed<Row[]>(() => {
    if (!updatesTab.value) return filtered.value;
    return filtered.value.filter((e) => !!updateOf(e)?.update);
});
const pageCount = computed(() => Math.max(1, Math.ceil(displayEntries.value.length / PAGE_SIZE)));
const visibleEntries = computed(() =>
    displayEntries.value.slice((page.value - 1) * PAGE_SIZE, page.value * PAGE_SIZE)
);
const modIcons = ref<Record<string, string>>({});
let iconGeneration = 0;
let iconTimer: ReturnType<typeof setTimeout> | undefined;
watch([visibleEntries, effectiveRel, activeFolder, scope], () => {
    const generation = ++iconGeneration;
    clearTimeout(iconTimer);
    modIcons.value = {};
    const version = currentVersion.value;
    if (!version) return;
    const rows = visibleEntries.value.filter(
        (e) => !e.isDir && /\.(?:jar(?:\.disabled)?|zip)$/i.test(e.name)
    );
    if (!rows.length) return;
    const folder = version.folder || activeFolder.value;
    iconTimer = setTimeout(() => {
        // 聚合视图按每行所属目录分别取图标（图标缓存按目录身份区分）
        const request =
            scope.value === 'folder'
                ? Promise.all(
                      [...new Map(rows.map((r) => [rowDir(r), r])).keys()].map(async (dir) => {
                          const names = rows.filter((r) => rowDir(r) === dir).map((r) => r.name);
                          const icons = await modIconsAt(dir, names, props.rel);
                          const instanceId = rows.find((r) => rowDir(r) === dir)?.instanceId ?? '';
                          return Object.entries(icons).map(
                              ([name, icon]) =>
                                  [instanceId ? instanceId + '/' + name : name, icon] as const
                          );
                      })
                  ).then((groups) => Object.fromEntries(groups.flat()))
                : getModIcons(
                      version.id,
                      rows.map((r) => r.name),
                      folder,
                      props.rel
                  );
        void Promise.resolve(request)
            .then((icons) => {
                if (generation === iconGeneration) modIcons.value = icons;
            })
            .catch(() => {});
    }, 120);
});
onUnmounted(() => {
    iconGeneration++;
    clearTimeout(iconTimer);
});
watch([keyword, pageCount], () => {
    page.value = Math.min(page.value, pageCount.value);
});

/** 资源页→社区资源：带上当前实例与资源类型（PCL 式实例语境闭环，落到社区页预筛选） */
const REL_TO_KIND = {
    mods: 'mod',
    resourcepacks: 'resourcepack',
    shaderpacks: 'shader'
} as const;
function goCommunityDownload() {
    if (!currentVersion.value) return;
    store.communityHandoff = {
        kind: REL_TO_KIND[props.rel],
        versionId: currentVersion.value.id,
        nonce: Date.now()
    };
    store.currentView = 'community';
}

async function onOpenDir() {
    if (!currentVersion.value) return;
    opening.value = true;
    try {
        // 实例视图打开该实例的资源目录；聚合视图打开共享根目录
        const rel = scope.value === 'folder' ? props.rel : effectiveRel.value;
        await openDir(rel, currentVersion.value?.folder || activeFolder.value);
    } catch (e) {
        toast('打开文件夹失败：' + errText(e), 'error');
    } finally {
        opening.value = false;
    }
}

const delModal = reactive({ open: false, target: null as Row | null, busy: false });

function onRemove(entry: Row) {
    delModal.open = true;
    delModal.target = entry;
}

// ---------------- 模组禁用/启用（仅模组页；.jar ↔ .jar.disabled，运行中由主进程阻止） ----------------
const isModEntry = (e: Row) =>
    props.rel === 'mods' && !e.isDir && /\.jar(\.disabled)?$/i.test(e.name);
const isDisabledMod = (e: Row) => /\.jar\.disabled$/i.test(e.name);
/** 资源文件（jar/zip）：无图时用页面同款类型图标兜底，与文件夹、普通文件区分 */
const isArchiveEntry = (e: Row) => !e.isDir && /\.(?:jar(?:\.disabled)?|zip)$/i.test(e.name);
const toggling = ref('');

async function onToggleDisable(entry: Row, event?: Event) {
    if (event?.target) (event.target as HTMLInputElement).checked = !isDisabledMod(entry);
    if (!currentVersion.value) return;
    if (toggling.value) return;
    toggling.value = rowKey(entry);
    const generation = loadGeneration;
    try {
        // 聚合视图按行的目录定位；实例视图走 versions/<id>/<rel>
        if (scope.value === 'folder') {
            await window.kamucl.invoke('fs:toggleDisableAt', rowDir(entry), entry.name);
            if (generation !== loadGeneration) return;
            toast(
                isDisabledMod(entry)
                    ? `已启用 ${entry.name.replace(/\.disabled$/i, '')}`
                    : `已禁用 ${entry.name}`,
                'success'
            );
            await load();
        } else {
            const result = await toggleDisableFs(
                effectiveRel.value,
                entry.name,
                currentVersion.value?.folder || activeFolder.value
            );
            if (generation !== loadGeneration) return;
            entries.value = result as Row[];
            toast(
                isDisabledMod(entry)
                    ? `已启用 ${entry.name.replace(/\.disabled$/i, '')}`
                    : `已禁用 ${entry.name}`,
                'success'
            );
        }
    } catch (e) {
        toast(errText(e), 'error');
    } finally {
        toggling.value = '';
    }
}

async function onConfirmRemove() {
    if (!currentVersion.value) return;
    const entry = delModal.target;
    if (!entry || delModal.busy) return;
    delModal.busy = true;
    const generation = loadGeneration;
    try {
        // 聚合视图按行的目录定位；实例视图走 versions/<id>/<rel>
        if (scope.value === 'folder') {
            await window.kamucl.invoke('fs:removeAt', rowDir(entry), entry.name);
            if (generation !== loadGeneration) return;
            delModal.open = false;
            await load();
            return;
        }
        const result = await removeFs(
            effectiveRel.value,
            entry.name,
            currentVersion.value?.folder || activeFolder.value
        );
        if (generation !== loadGeneration) return;
        entries.value = result;
        delModal.open = false;
        toast(`已移入回收站：${entry.name}`, 'success');
    } catch (e) {
        toast('删除失败：' + errText(e), 'error');
    } finally {
        delModal.busy = false;
    }
}

function fmtSize(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

const fmtDate = (ts: number) => {
    const d = new Date(ts);
    return Number.isNaN(d.getTime()) ? '' : d.toLocaleDateString('zh-CN');
};

/** PCL 口径四分类 tab（TabPills 共享药丸）。
 *  「可更新」这颗只在真有更新时才出现，且完全隐藏——不是置灰、不是 0（PCL 同款判定）。 */
const jarCount = computed(
    () => entries.value.filter((e) => !e.isDir && /\.jar$/i.test(e.name)).length
);
const disabledCount = computed(() => entries.value.filter(isDisabledMod).length);
const modTabs = computed(() => {
    // PCL 的排列：全部 → 可更新 → 启用 → 禁用（可更新紧跟「全部」，因为它是最常点的第二颗）
    const tabs: TabPillItem[] = [{ value: 'all', label: '全部', count: jarCount.value + disabledCount.value }];
    // 正在筛选时可更新项暂时为 0（例如刚更新完清零）也要留着这颗，否则用户会被困住
    if (updatableCount.value > 0 || (updatesTab.value && !updatePanel.checking))
        tabs.push({
            value: 'updates',
            label: '可更新',
            count: updatableCount.value,
            disabled: scope.value === 'folder',
            disabledReason: '更新检测只针对当前实例，请先切回「当前实例」'
        });
    tabs.push(
        { value: 'enabled', label: '已启用', count: jarCount.value },
        { value: 'disabled', label: '已禁用', count: disabledCount.value }
    );
    return tabs;
});
function setTab(v: string) {
    modFilter.value = v;
}

/** 静默检测：进页面就跑，不挡列表、不显示加载占位。PCL 的 LocalResourceOnlineLoader 等价物。 */
let checkedFor = '';
/** 检测自己的操作号。**不能复用 loadGeneration**：文件列表每次 load() 都会推进代数，
 *  两者交错时 finally 里的守卫会把 checking 永久钉在 true，页面就一直显示「正在检查」。 */
let updateOperationId = 0;
async function silentCheck(reason: 'enter' | 'manual' = 'enter') {
    const v = currentVersion.value;
    if (props.rel !== 'mods' || !v || updatePanel.applying) return;
    // 聚合视图没有「当前实例」这一说，检测对象不唯一：清掉标记，切回来时重新查一次
    if (scope.value === 'folder') {
        checkedFor = '';
        return;
    }
    const key = v.id + '|' + (v.folder || activeFolder.value);
    if (reason === 'enter' && (updatePanel.checking || checkedFor === key)) return;
    checkedFor = key;
    const ticket = ++updateOperationId;
    updatePanel.checking = true;
    updatePanel.error = '';
    updatePanel.note = '';
    try {
        // manual = 用户点「重新检测」，强制跳过 6 小时缓存
        const report = await checkModUpdates(v.id, v.folder, reason === 'manual');
        // 已被更新的请求取代（切了实例/又点了一次）：只让最新那次写结果
        if (ticket !== updateOperationId) return;
        updatePanel.report = report;
        // 静默降级：联网失败但后端回退了缓存，只留一句提示，列表照常用
        updatePanel.note = report.error
            ? `更新信息暂未刷新（${report.error}），显示的是上次的结果`
            : '';
        updatePanel.error =
            report.error && !report.entries.some((e) => e.source) ? report.error : '';
        // 行图标由 modIcons 那一套负责（行本来就是正常扫描出来的文件行），这里不再另取一份
    } catch (e) {
        // 静默检测失败不弹错误条：用户没请求，不该被红字打断
        if (ticket === updateOperationId && reason === 'manual') updatePanel.error = errText(e);
    } finally {
        // 无条件收尾：检查状态属于这个面板，不该被列表加载代数牵连
        if (ticket === updateOperationId) updatePanel.checking = false;
    }
}
const onCheckUpdates = () => silentCheck('manual');

// 进页面 / 切实例 / 切回「当前实例」都重新对一次（后端有 6 小时缓存，重复调用几乎无成本）
watch(
    [effectiveRel, activeFolder, scope, () => currentVersion.value?.id],
    () => {
        if (props.rel !== 'mods' || scope.value === 'folder') {
            updatePanel.report = null;
            checkedFor = '';
            return;
        }
        updatePanel.itemState = {};
        updatePanel.itemError = {};
        void silentCheck('enter');
    },
    { immediate: true }
);
// 「可更新」筛选只在有东西可更新时存在；数量归零就自动退回「全部」，避免卡在一个不存在的分类里
watch(updatableCount, (n) => {
    if (n === 0 && updatesTab.value && !updatePanel.checking) setTab('all');
});
// 文件变动（导入/删除/更新成功）后结论就旧了：清掉缓存标记，重新查一遍
watch(
    () => store.fsRefreshTick,
    () => {
        if (props.rel !== 'mods' || scope.value === 'folder') return;
        checkedFor = '';
        void silentCheck('enter');
    }
);

async function applyUpdates(fileNames: string[]) {
    const v = currentVersion.value;
    const report = updatePanel.report;
    if (!v || !report || updatePanel.applying) return;
    // 锁定状态按行的实际键查（聚合视图是 instanceId/文件名，直接按 fileName 查会漏掉）
    const lockedNames = new Set(
        entries.value.filter((e) => catalogOf(e)?.locked).map((e) => e.name)
    );
    const targets = report.entries
        .filter((e) => e.update && fileNames.includes(e.fileName) && !lockedNames.has(e.fileName))
        .map((e) => ({
            fileName: e.fileName,
            oldSha1: e.sha1,
            url: e.update!.url,
            targetName: e.update!.fileName,
            sha1: e.update!.sha1,
            size: e.update!.size
        }));
    if (!targets.length) return;
    /*
     * 大批量更新前自动拍快照（「后悔药」的自动触发点）。
     * 阈值与后端 snapshots::should_auto_snapshot 同一口径（≥30）。
     * 失败不拦更新：备份是保险，不是前置条件——但会提示玩家备份没做成。
     */
    if (targets.length >= 30) {
        try {
            await window.kamucl.invoke(
                'snapshots:create',
                v.folder,
                v.id,
                v.id,
                `批量更新 ${targets.length} 个模组前自动备份`,
                true
            );
            toast(`已自动备份当前状态（${targets.length} 个模组待更新）`, 'info');
        } catch (e) {
            toast('自动备份失败，仍会继续更新：' + errText(e), 'error');
        }
    }
    updatePanel.applying = true;
    const operation = ++updateOperation;
    for (const target of targets) {
        updatePanel.itemState[target.fileName] = 'start';
        delete updatePanel.itemError[target.fileName];
    }
    try {
        const results = await applyModUpdates(v.id, targets, v.folder);
        if (operation !== updateOperation) {
            toast('原实例的模组更新已结束', 'info');
            return;
        }
        let okCount = 0;
        for (const r of results) {
            updatePanel.itemState[r.fileName] = r.ok ? 'ok' : 'error';
            if (r.ok) okCount++;
            else updatePanel.itemError[r.fileName] = r.error ?? '未知错误';
        }
        if (okCount) {
            toast(`已更新 ${okCount} 个 MOD`, 'success');
            // 更新成功的行立刻摘掉箭头（不必等下一轮检测）
            updatePanel.report = {
                ...report,
                entries: report.entries.filter((e) => updatePanel.itemState[e.fileName] !== 'ok')
            };
            // 更新成功的文件不再留在选择集里
            const done = results.filter((r) => r.ok).map((r) => r.fileName);
            const next = new Set(selection.value);
            for (const f of done) next.delete(f);
            selection.value = next;
            await load();
            // 文件名变了、hash 也变了：清掉检测标记，让新文件重新走一次（新 hash 必然缓存未命中）
            checkedFor = '';
            void silentCheck('enter');
        }
        const failed = results.filter((r) => !r.ok);
        if (failed.length) toast(`${failed.length} 个更新失败：${failed[0].error ?? ''}`, 'error');
    } catch (e) {
        if (operation === updateOperation)
            for (const target of targets) {
                updatePanel.itemState[target.fileName] = 'error';
                updatePanel.itemError[target.fileName] = errText(e);
            }
        toast('更新失败：' + errText(e), 'error');
    } finally {
        if (operation === updateOperation) updatePanel.applying = false;
    }
}

/** 行内更新箭头的 tooltip：PCL 同样用「当前版本 / 最新版本 + 点击/右键说明」交代下一步 */
function rowUpdateTitle(e: Row): string {
    const info = updateOf(e);
    if (!info?.update) return '';
    const state = updatePanel.itemState[e.name];
    if (catalogOf(e)?.locked) return '此模组已锁定：点击查看可切换的版本';
    if (state === 'ok') return '已更新，原文件已替换';
    if (state === 'error') return updatePanel.itemError[e.name] || '更新失败，点击重试';
    return [
        `当前版本：${info.currentVersion || '未知'}`,
        `最新版本：${info.update.versionNumber || info.update.fileName}`,
        '点击更新，右键查看更新日志'
    ].join('\n');
}

/** 行内箭头点击：锁定项跳到「切换版本」，否则就是单条更新 */
function onRowUpdate(e: Row) {
    const info = updateOf(e);
    if (!info?.update) return;
    if (catalogOf(e)?.locked) {
        switchFile.value = e.name;
        return;
    }
    void applyUpdates([e.name]);
}

/** 右键箭头看更新日志（PCL 同款交互），没识别到来源时不假装有链接 */
function openChangelog(e: Row) {
    const url = updateOf(e)?.changelogUrl;
    if (!url) {
        toast('此模组未匹配到 Modrinth 来源，无法查看更新日志', 'info');
        return;
    }
    window.open(url, '_blank', 'noopener,noreferrer');
}

</script>

<template>
    <div
        class="page page--resource file-manager"
        :data-resource="props.rel"
    >
        <header class="fm-head">
            <div data-ui="FileManager:9ace27a3aaa9" class="page-head fm-head-left">
                <h1 data-ui="FileManager:72e6bc71f100" class="page-title">
                    {{ props.title }}
                    <small v-if="currentVersion && !loading">{{ resourceCount }} 个</small>
                </h1>
                <p v-if="props.subtitle" class="muted">{{ props.subtitle }}</p>
            </div>
            <div data-ui="FileManager:7a94152a1114" class="fm-actions">
                <button
                    class="btn btn-gold"
                    :disabled="importing || !currentVersion"
                    :aria-label="'导入' + props.title"
                    @click="onPickImport"
                >
                    {{ importing ? '导入中…' : `导入${props.title}…` }}
                </button>
                <button
                    class="btn btn-ghost"
                    :disabled="!currentVersion"
                    :aria-label="'下载' + props.title"
                    @click="goCommunityDownload"
                >
                    {{ `下载${props.title}…` }}
                </button>
                <button
                    class="btn btn-ghost"
                    :disabled="opening || !currentVersion"
                    @click="onOpenDir"
                >
                    {{ props.openLabel }}
                </button>
                <button
                    class="btn btn-ghost"
                    :disabled="loading || !currentVersion"
                    aria-label="刷新文件列表"
                    @click="load"
                >
                    <!-- 文本符号 ↻ 收编 UiGlyph refresh（inventory H；GameView tool-refresh 同款范式） -->
                    <span v-if="loading" class="spin spin-sm"></span>
                    <UiGlyph v-else name="refresh" :size="14" />
                    {{ loading ? '读取中' : '刷新' }}
                </button>
                <details
                    data-ui="FileManager:4aeae736e670"
                    v-if="props.rel === 'mods'"
                    class="file-more"
                    @keydown.esc="($event.currentTarget as HTMLDetailsElement).open = false"
                >
                    <summary class="btn btn-ghost" aria-label="模组管理更多操作">更多</summary>
                    <div
                        data-ui="FileManager:bd84b9640929"
                        class="file-more-actions"
                        @click="
                            ($event.currentTarget as HTMLElement)
                                .closest('details')
                                ?.removeAttribute('open')
                        "
                    >
                        <button
                            class="btn btn-ghost"
                            :disabled="!currentVersion || scope === 'folder'"
                            :title="
                                scope === 'folder'
                                    ? '版本迁移只针对当前实例，请先切回「当前实例」'
                                    : ''
                            "
                            @click="migrationOpen = true"
                        >
                            版本迁移</button
                        ><button
                            class="btn btn-ghost"
                            :disabled="!currentVersion || scope === 'folder'"
                            :title="
                                scope === 'folder'
                                    ? '清理重复只针对当前实例，请先切回「当前实例」'
                                    : ''
                            "
                            @click="dupOpen = true"
                        >
                            清理重复
                        </button>
                    </div>
                </details>
            </div>
        </header>
        <div v-if="availableVersions.length" class="fm-context">
            <ResourceScopeFields
                v-model:scope="scope"
                :scope-options="scopeOptions"
                v-model:instance-id="store.resourceVersionId"
                :instance-options="
                    availableVersions.map((v) => ({
                        value: v.id,
                        label: versionLabel(v),
                        description: [
                            v.mcVersion,
                            v.loader,
                            v.loaderVersion,
                            v.isolated ? '已隔离' : '共享'
                        ]
                            .filter(Boolean)
                            .join(' · ')
                    }))
                "
                :instance-locked="scope === 'folder'"
                :disabled="loading"
            />
            <span
                v-if="scope === 'folder'"
                data-ui="FileManager:fm-scope-summary"
                class="fm-scope-summary muted"
                >{{ scopeSummary }}（同名文件按实例分别列出）</span
            >
            <div class="fm-context-row">
                <ResourcePathChip label="当前目录" :path="fullPath" />
                <details class="fm-help">
                    <summary>使用说明</summary>
                    <p>
                        「当前实例」只看所选实例的该子目录；「版本文件夹内的所有」会聚合共享目录与各实例的同名目录，每行标注所属实例。把文件拖进页面即可导入，也可以点右上角「导入{{
                            props.title
                        }}…」选择文件。{{
                            props.rel === 'mods'
                                ? '模组可用行内开关启用/禁用；「更多」菜单提供版本迁移与清理重复。'
                                : ''
                        }}
                    </p>
                </details>
            </div>
        </div>
        <!-- 未安装任何版本时提示：收编 EmptyState（UX「空状态指引」：说明+入口按钮，对照 GameView 示范） -->
        <EmptyState
            data-ui="FileManager:de9f15303347"
            v-if="!currentVersion"
            class="card"
            icon="warning"
            title="当前游戏文件夹没有可选版本"
            description="请先到「游戏版本」页安装或选择一个版本，再回到这里管理文件。"
        >
            <template #action>
                <button class="btn btn-gold btn-sm" @click="store.currentView = 'game'">
                    去安装或选择版本
                </button>
            </template>
        </EmptyState>

        <div data-ui="FileManager:4c9771e53b3b" v-if="currentVersion" class="fm-controls">
            <input
                data-ui="FileManager:ec68a12c2ae9"
                v-model="localSearch"
                class="input fm-search"
                :aria-label="'搜索' + props.title"
                :placeholder="'搜索文件名或' + props.title + '名称…'"
            />
            <TabPills
                v-if="props.rel === 'mods'"
                data-ui="FileManager:mod-tabs"
                :items="modTabs"
                :model-value="modFilter"
                aria-label="模组分类"
                @update:model-value="setTab"
            />
            <div class="fm-sort">
                <SelectMenu
                    v-model="sortBy"
                    :options="[
                        { value: 'name', label: '名称排序' },
                        { value: 'date', label: '最近修改' },
                        { value: 'size', label: '文件大小' }
                    ]"
                />
            </div>
        </div>
        <div
            data-ui="FileManager:50137bbe5c59"
            v-if="selection.size && props.rel === 'mods'"
            class="fm-batch card"
        >
            <strong>已选 {{ selection.size }} 项</strong
            ><!-- PCL 底栏「更新」按钮口径：始终在位，选中项里没有可更新的就置灰 -->
            <button
                class="btn btn-gold btn-sm"
                :disabled="updatePanel.applying || !selectedUpdatableCount"
                :title="selectedUpdatableCount ? undefined : '选中项里没有可更新的模组'"
                @click="applyUpdates([...selection])"
            >
                {{
                    updatePanel.applying ? '正在更新…' : `更新选中（${selectedUpdatableCount}）`
                }}</button
            ><button
                data-ui="FileManager:8a3d5842bdcc"
                class="btn btn-ghost btn-sm"
                :disabled="batchBusy"
                @click="toggleAllFiltered"
            >
                {{
                    allFilteredSelected
                        ? '取消全选'
                        : `全选筛选结果（${displayEntries.filter(isModEntry).length}）`
                }}</button
            ><button
                data-ui="FileManager:3f98d111f11c"
                class="btn btn-ghost btn-sm"
                :disabled="batchBusy"
                @click="batch('enable')"
            >
                批量启用</button
            ><button
                data-ui="FileManager:f9e4a6481640"
                class="btn btn-ghost btn-sm"
                :disabled="batchBusy"
                @click="batch('disable')"
            >
                批量禁用</button
            ><button
                data-ui="FileManager:91b8dde939da"
                class="btn btn-ghost btn-sm"
                :disabled="batchBusy"
                @click="batch('lock')"
            >
                锁定</button
            ><button
                data-ui="FileManager:d3ca28eade0c"
                class="btn btn-ghost btn-sm"
                :disabled="batchBusy"
                @click="batch('unlock')"
            >
                解除锁定
            </button>
        </div>
        <p data-ui="FileManager:080a7432d0c7" v-if="catalogError" class="muted" role="alert">
            模组识别或锁定记录读取失败：{{ catalogError }}。可刷新重试，写入操作仍会由后端校验。
        </p>
        <p
            data-ui="FileManager:fm-aggregate-warnings"
            v-if="scope === 'folder' && aggregateWarnings.length"
            class="muted"
            role="status"
        >
            有 {{ aggregateWarnings.length }} 个实例的目录未能读取：{{
                aggregateWarnings.join('；')
            }}。可刷新重试。
        </p>
        <details
            data-ui="FileManager:3bdbccefe0d1"
            v-if="batchResults.length"
            class="fm-results card"
            :open="batchResults.some((r) => !r.ok)"
        >
            <summary data-ui="FileManager:fb44f4241998">
                上次批量操作：{{ batchResults.filter((r) => r.ok).length }} 项成功 ·
                {{ batchResults.filter((r) => !r.ok).length }} 项失败
            </summary>
            <div data-ui="FileManager:9d10b3e8ea6e" class="fm-result-list">
                <p
                    data-ui="FileManager:b6f08c17dc51"
                    v-for="r in batchResults"
                    :key="r.fileName"
                    :class="{ failed: !r.ok }"
                >
                    <strong>{{ r.fileName }}</strong
                    ><span>{{ r.ok ? '已完成' : r.error }}</span>
                </p>
            </div>
        </details>
        <!-- 文件列表：更新检测只是行内标记与一颗筛选药丸，列表本身始终是同一个 -->
        <div
            data-ui="FileManager:cdc99c60dc51"
            v-if="currentVersion"
            ref="listCard"
            class="card fm-card"
            :aria-busy="loading"
        >
            <!-- 静默检测的极简状态：仅在「正在查 / 上次联网失败」时占一条细提示，不挡列表 -->
            <div
                v-if="updatePanel.checking || updatePanel.note"
                class="status-strip upd-strip"
                role="status"
            >
                <template v-if="updatePanel.checking">
                    <span class="spin spin-sm"></span>
                    <span>正在后台检查模组更新…</span>
                </template>
                <template v-else>
                    <span>{{ updatePanel.note }}</span>
                </template>
                <button
                    class="btn btn-ghost btn-sm"
                    :disabled="updatePanel.checking || updatePanel.applying"
                    @click="onCheckUpdates"
                >
                    重新检测
                </button>
            </div>
            <div
                data-ui="FileManager:aaf1f853f556"
                v-if="entries.length && (loading || loadError)"
                class="status-strip"
                role="status"
            >
                {{ loading ? '正在刷新，暂时保留上次的文件列表…' : '刷新失败：' + loadError
                }}<button
                    data-ui="FileManager:1168663564f5"
                    v-if="loadError"
                    class="btn btn-ghost btn-sm"
                    @click="load"
                >
                    重试
                </button>
            </div>
            <ContentSkeleton
                v-if="loading && !entries.length"
                label="正在读取文件列表…"
                retry
                @retry="load"
            />
            <!-- 空态收编 EmptyState（inventory B）：错误=原因+重试入口；空目录=页面图标走 #icon 槽；筛选无果=引导回全部 -->
            <EmptyState
                data-ui="FileManager:12ff8f1f3706"
                v-else-if="loadError && !entries.length"
                icon="warning"
                title="读取失败"
                :description="loadError"
            >
                <template #action>
                    <button
                        data-ui="FileManager:9605a11dc3ac"
                        class="btn btn-ghost btn-sm"
                        @click="load"
                    >
                        重试
                    </button>
                </template>
            </EmptyState>
            <EmptyState
                data-ui="FileManager:c33c3939fd8c"
                v-else-if="!entries.length"
                :title="props.emptyText"
                :description="
                    scope === 'folder'
                        ? `已查找共享目录与 ${aggregateInstances.length} 个实例的该子目录，都没有文件。`
                        : '也可以直接把文件拖进本页面导入。'
                "
            >
                <template #icon>
                    <span
                        data-ui="FileManager:a3b8daa1f95d"
                        class="empty-icon"
                        v-html="props.icon"
                    ></span>
                </template>
            </EmptyState>
            <EmptyState
                data-ui="FileManager:40d26ad64938"
                v-else-if="!displayEntries.length"
                :title="
                    updatesTab
                        ? updatePanel.checking
                            ? '正在检查更新…'
                            : '没有可更新的模组'
                        : '当前搜索或筛选条件没有匹配的文件'
                "
            >
                <template v-if="updatesTab && !updatePanel.checking" #action>
                    <button
                        data-ui="FileManager:upd-back-all"
                        class="btn btn-ghost btn-sm"
                        @click="setTab('all')"
                    >
                        回到全部
                    </button>
                </template>
            </EmptyState>
            <div
                data-ui="FileManager:893880828b4a"
                v-else
                class="fm-list"
                :inert="loading || !!loadError"
            >
                <div class="fm-table-head">
                    <input
                        v-if="props.rel === 'mods'"
                        type="checkbox"
                        aria-label="选择当前页模组"
                        :checked="pageChecked.all"
                        :indeterminate="pageChecked.partial"
                        :disabled="batchBusy || !pageMods.length"
                        @change="togglePageSelection"
                    /><span v-else></span><span>名称</span><span>大小</span
                    ><span class="fm-date">修改时间</span
                    ><span v-if="props.rel === 'mods'">启用</span><span>操作</span>
                </div>
                <div
                    data-ui="FileManager:9628c4638932"
                    v-for="e in visibleEntries"
                    :key="rowKey(e)"
                    class="fm-row"
                    :class="{ 'fm-row-disabled': isDisabledMod(e) }"
                    :data-sweep-key="isModEntry(e) ? rowKey(e) : undefined"
                >
                    <input
                        data-ui="FileManager:8d3101b3257d"
                        v-if="isModEntry(e)"
                        type="checkbox"
                        :aria-label="'选择 ' + e.name"
                        :checked="selection.has(rowKey(e))"
                        :disabled="batchBusy"
                        @change="selectMod(rowKey(e), ($event.target as HTMLInputElement).checked)"
                    />
                    <span v-if="props.rel === 'mods' && !isModEntry(e)" aria-hidden="true"></span>
                    <span
                        data-ui="FileManager:0cf0b1354f9a"
                        class="fm-file-icon"
                        :draggable="!store.editMode && !batchBusy && !loading"
                        @dragstart="dragResource($event, e)"
                    >
                        <img
                            data-ui="FileManager:e82feec99cd2"
                            v-if="modIcons[rowKey(e)]"
                            :src="modIcons[rowKey(e)]"
                            draggable="false"
                            alt=""
                            @error="delete modIcons[rowKey(e)]"
                        />
                        <!-- 手绘文件夹/文件 SVG 收编 UiGlyph（inventory H；尺寸由 .fm-file-icon :deep(svg) 撑满容器） -->
                        <UiGlyph v-else-if="e.isDir" name="folder" />
                        <span
                            v-else-if="isArchiveEntry(e)"
                            class="fm-file-icon-kind"
                            v-html="props.icon"
                        ></span>
                        <UiGlyph v-else name="file" />
                    </span>
                    <span
                        data-ui="FileManager:8c8c91106eaa"
                        class="fm-name"
                        tabindex="0"
                        :title="e.name + ' · 按住拖到桌面或文件夹'"
                        :draggable="!store.editMode && !batchBusy && !loading"
                        @dragstart="dragResource($event, e)"
                        ><span class="fm-name-line"
                            >{{ readableName(e)
                            }}<!-- PCL 口径：可更新行在名字后紧跟一个箭头按钮，其余行没有这个元素 -->
                            <button
                                v-if="updateOf(e)?.update"
                                data-ui="FileManager:row-update"
                                class="fm-upd-btn"
                                :class="{
                                    'is-start': updatePanel.itemState[e.name] === 'start',
                                    'is-ok': updatePanel.itemState[e.name] === 'ok',
                                    'is-error': updatePanel.itemState[e.name] === 'error'
                                }"
                                :disabled="
                                    updatePanel.applying ||
                                    !!catalogOf(e)?.locked ||
                                    updatePanel.itemState[e.name] === 'ok'
                                "
                                :title="rowUpdateTitle(e)"
                                :aria-label="'更新 ' + (updateOf(e)?.name || e.name)"
                                @click.stop="onRowUpdate(e)"
                                @contextmenu.prevent.stop="openChangelog(e)"
                            >
                                <!-- 手绘上传箭头 SVG 收编 UiGlyph upload（inventory H；同语义：装入新版本） -->
                                <UiGlyph name="upload" :size="14" />
                            </button>
                        </span>
                        <small
                            data-ui="FileManager:a0d9b4c442aa"
                            v-if="readableName(e) !== e.name"
                            class="fm-internal"
                            >{{ e.name }}</small
                        ><small
                            v-else-if="catalogOf(e)?.name && catalogOf(e).name !== e.name"
                            class="fm-internal"
                            >{{ catalogOf(e)!.name }}</small
                        ><small
                            v-if="isDisabledMod(e) || catalogOf(e)?.locked"
                            class="fm-internal"
                            >{{
                                [
                                    isDisabledMod(e) ? '已禁用' : '',
                                    catalogOf(e)?.locked ? '已锁定' : ''
                                ]
                                    .filter(Boolean)
                                    .join(' · ')
                            }}</small
                        ><!-- 版本对比只在「可更新」筛选里展开；全部列表靠箭头 tooltip 交代，避免每行都多一行字 --><small
                            v-if="updateOf(e)?.update && updatesTab"
                            class="fm-internal fm-upd-ver"
                            >{{ updateOf(e)!.currentVersion || '未知' }} →
                            <b>{{ updateOf(e)!.update!.versionNumber }}</b>
                            <template v-if="updatePanel.itemState[e.name] === 'error'">
                                <span class="fm-upd-err">{{
                                    updatePanel.itemError[e.name] || '更新失败，请重试'
                                }}</span>
                            </template>
                            <template v-else-if="updatePanel.itemState[e.name] === 'ok'">
                                <span>已更新，原文件已替换</span>
                            </template>
                        </small
                        ><small
                            v-if="scope === 'folder' && e.instanceId"
                            data-ui="FileManager:fm-instance"
                            class="fm-instance"
                            :title="'所属实例：' + e.instanceName"
                            >{{ e.instanceName }}</small
                        ></span
                    >

                    <span data-ui="FileManager:558cc8ea9415" class="muted fm-meta">{{
                        e.isDir ? '文件夹' : fmtSize(e.size)
                    }}</span>
                    <span data-ui="FileManager:bf6ae194335f" class="muted fm-meta fm-date">{{
                        fmtDate(e.mtime)
                    }}</span>

                    <label
                        data-ui="FileManager:7fe2dcd1bb05"
                        v-if="isModEntry(e)"
                        class="switch fm-toggle"
                        :title="isDisabledMod(e) ? '启用模组' : '禁用模组'"
                        ><input
                            data-ui="FileManager:eeccaf57cd1c"
                            type="checkbox"
                            role="switch"
                            :aria-label="(isDisabledMod(e) ? '启用 ' : '禁用 ') + e.name"
                            :checked="!isDisabledMod(e)"
                            :disabled="!!toggling || batchBusy"
                            @change="onToggleDisable(e, $event)" /><span
                            data-ui="FileManager:902dfce79a14"
                            class="switch-ui"
                        ></span
                    ></label>
                    <span
                        v-if="props.rel === 'mods' && !isModEntry(e)"
                        aria-hidden="true"
                    ></span>
                    <!-- PCL PageInstanceMod 行模型（inventory M，多数决落地）：高频操作行内直出——
                         启停开关 + 删除常驻行内；⋯ 折叠只收低频（锁定/切换版本，PCL 无此概念）。
                         资源包/光影包行没有低频操作，折叠不渲染。 -->
                    <div class="fm-ops">
                        <details
                            v-if="isModEntry(e)"
                            class="file-more"
                            @keydown.esc="($event.currentTarget as HTMLDetailsElement).open = false"
                        >
                            <summary
                                data-ui="FileManager:859ab7a92cdd"
                                class="btn btn-ghost btn-sm"
                                :aria-label="'更多操作 ' + e.name"
                            >
                                <!-- 文本符号 ⋯ 收编 UiGlyph more（inventory H；GameView 行内同口径） -->
                                <UiGlyph name="more" :size="16" />
                            </summary>
                            <div
                                class="file-more-actions"
                                @click="
                                    ($event.currentTarget as HTMLElement)
                                        .closest('details')
                                        ?.removeAttribute('open')
                                "
                            >
                                <button
                                    data-ui="FileManager:40ebd7f85d68"
                                    class="btn btn-ghost btn-sm"
                                    :disabled="batchBusy"
                                    :aria-label="
                                        (catalogOf(e)?.locked ? '解除锁定 ' : '锁定 ') + e.name
                                    "
                                    :title="
                                        catalogOf(e)?.locked
                                            ? '已锁定：不参与批量更新，点击解除'
                                            : '锁定此模组版本'
                                    "
                                    @click="toggleLock(e)"
                                >
                                    {{ catalogOf(e)?.locked ? '解锁' : '锁定' }}</button
                                ><button
                                    data-ui="FileManager:fd277c185a8b"
                                    class="btn btn-ghost btn-sm"
                                    :disabled="batchBusy"
                                    :aria-label="'切换版本 ' + e.name"
                                    @click="switchFile = e.name"
                                >
                                    切换版本
                                </button>
                            </div>
                        </details>
                        <button
                            data-ui="FileManager:537aea904555"
                            class="btn btn-danger btn-sm fm-remove"
                            :disabled="batchBusy"
                            @click="onRemove(e)"
                        >
                            删除
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <ResourcePagination
            v-if="currentVersion"
            v-model:page="page"
            :page-count="pageCount"
            :total="displayEntries.length"
        />
        <ModVersionModal
            v-if="switchFile && currentVersion"
            :source="currentVersion"
            :file-name="switchFile"
            @close="switchFile = ''"
            @done="
                load();
                toast('模组版本已切换', 'success');
            "
        />
        <ModMigrationModal
            v-if="migrationOpen && currentVersion"
            :source="currentVersion"
            @close="migrationOpen = false"
        />
        <!-- 删除文件二次确认 -->
        <ConfirmModal
            :open="delModal.open"
            title="删除文件"
            :message="`确定要删除「${delModal.target?.name}」吗？文件将移入系统回收站，可从回收站恢复。`"
            :busy="delModal.busy"
            @cancel="delModal.open = false"
            @confirm="onConfirmRemove"
        />

        <!-- 清理重复 MOD（仅模组页） -->
        <DupCleanModal
            v-if="props.rel === 'mods' && dupOpen && currentVersion"
            :open="dupOpen"
            :version-id="currentVersion?.id ?? ''"
            :rel="effectiveRel"
            :folder="currentVersion?.folder || activeFolder"
            @close="dupOpen = false"
            @deleted="load"
        />
    </div>
</template>

<style scoped>
.file-more {
    position: relative;
}
.file-more summary {
    list-style: none;
}
/* 「更多」details 两档尺寸写死（inventory I：头部非 sm、行内 sm）。
   头部 = 常规档按钮（与头部其他按钮同级等高等宽）；行内 = btn-sm 小档（模板已带 btn-sm）。 */
.fm-actions .file-more > summary {
    min-width: 36px;
}
.fm-row .file-more > summary {
    min-width: 32px;
    padding-left: 6px;
    padding-right: 6px;
}
.file-more[open] {
    z-index: 5;
}
.file-more-actions {
    position: absolute;
    right: 0;
    top: 100%;
    display: grid;
    gap: var(--space-2);
    padding: var(--space-2);
    min-width: 150px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-solid);
    box-shadow: var(--shadow);
}
/* 行内操作区（M：⋯ 折叠 + 行内删除同组；启动停开关在左侧独立列） */
.fm-ops {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    justify-content: flex-end;
}
.fm-row .fm-ops {
    flex-wrap: nowrap;
}
.fm-list {
    overflow: visible;
}
.fm-batch {
    position: sticky;
    top: 0;
    z-index: 6;
}
.fm-name {
    overflow-wrap: break-word;
    word-break: normal;
}

.fm-results {
    padding: 12px 16px !important;
    margin-bottom: 16px;
}
.fm-results summary {
    cursor: pointer;
    font-size: 12px;
}
.fm-result-list {
    max-height: 180px;
    overflow: auto;
    overscroll-behavior: contain;
}
.fm-result-list p {
    display: flex;
    gap: 12px;
    justify-content: space-between;
    font-size: 12px;
    min-height: 28px;
    align-items: center;
}
.fm-result-list p.failed {
    color: var(--danger);
}
.fm-result-list strong {
    overflow-wrap: anywhere;
}
.fm-controls {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 16px;
}
.fm-controls .fm-search {
    flex: 1 1 220px;
    min-width: 0;
}
.fm-sort {
    width: 140px;
    flex-shrink: 0;
}
.fm-sort :deep(.select-menu-btn) {
    width: 100%;
}
/* 四分类 tab 已收编为 TabPills 共享药丸（UI_SYSTEM.md §5），旧分段容器样式删除 */
/* 使用说明：与路径条并排，开关钉在右侧（同录像/截图/投影页） */
.fm-help {
    margin-left: auto;
    flex: 0 1 auto;
    min-width: 0;
    max-width: min(640px, 55%);
    line-height: 1.7;
    font-size: 12px;
}
.fm-help summary {
    display: block;
    text-align: right;
    cursor: pointer;
    white-space: nowrap;
    color: var(--text-dim);
}
.fm-help summary::after {
    content: " ▸";
}
.fm-help[open] summary::after {
    content: " ▾";
}
.fm-help p {
    margin: 8px 0 0;
}
.fm-batch {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding: 12px 16px !important;
    margin-bottom: 16px;
}
.fm-internal {
    display: block;
    color: var(--text-dim);
    font-size: 11px;
    margin-top: 4px;
}
/* 聚合视图：标注该行来自哪个实例 */
.fm-instance {
    display: inline-block;
    margin-top: 4px;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--card-2);
    color: var(--text-dim);
    font-size: 10px;
    line-height: 1.6;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.fm-row input[type='checkbox'] {
    accent-color: var(--accent);
    flex-shrink: 0;
}
@media (max-width: 1000px) {
    .fm-row {
        flex-wrap: wrap;
    }
    .fm-name {
        min-width: 180px !important;
    }
}

.fm-toggle {
    flex-shrink: 0;
}

/* 页面骨架与宽度由全局 .page.page--resource 提供（UI_SYSTEM.md §4，1440 宽度族） */

/* 三页（模组/资源包/光影包）与录像族共用一套头部结构：标题+说明在左、操作在右 */
.fm-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
}
.fm-head-left p {
    margin: 4px 0 0;
    font-size: 12px;
}
.fm-head-left {
    flex: 1;
    min-width: 140px;
}
.fm-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    max-width: 100%;
}
:deep(.fm-ver-select) {
    max-width: 240px;
    min-width: 180px;
}

.fm-card {
    padding: var(--space-2);
}
.fm-list {
    display: flex;
    flex-direction: column;
}
.fm-name[draggable='true'],
.fm-file-icon[draggable='true'] {
    cursor: grab;
}
.fm-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--row-h);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    transition: background 0.15s ease;
}
.fm-file-icon {
    display: flex;
    width: 32px;
    height: 32px;
    color: var(--accent);
    flex-shrink: 0;
}
/* UiGlyph 的 svg 在子组件作用域内，:deep 才能撑满容器（与原内联 SVG 等效） */
.fm-file-icon :deep(svg) {
    width: 100%;
    height: 100%;
}
.fm-file-icon img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 6px;
}
.fm-name {
    white-space: normal;
    flex: 1;
    min-width: 0;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    user-select: text;
}
.fm-meta {
    font-size: var(--text-xs);
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
}
.fm-date {
    width: 86px;
    text-align: right;
}
/* 行内删除（M：PCL 模型常驻行内）：走全局 btn-danger 档（danger-soft 底 + danger 边），
   不再沿用折叠时代的隐形 ghost 覆写——行内直出的破坏性操作要看得见（UX「不可用≠消失」同源纪律） */
.fm-remove {
    flex-shrink: 0;
}
.fm-toggle {
    flex-shrink: 0;
}
/* 禁用状态成套表达（UX"状态最好用成套视觉语言"）：底色圈定作用范围 + 置灰 + 删除线 + 标签，
   不只靠一条删除线。--text-3/--line 为未定义令牌，修正为 --text-dim/--border。 */
.fm-row-disabled {
    opacity: 0.7;
    background: color-mix(in srgb, var(--text-dim) 7%, transparent);
}
.fm-row-disabled .fm-name {
    text-decoration: line-through;
    text-decoration-color: var(--text-dim);
}
.fm-disabled-tag {
    flex-shrink: 0;
    font-size: var(--text-xs);
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--card-2);
    color: var(--text-dim);
    border: 1px solid var(--border);
}
.empty-icon {
    display: flex;
    width: 44px;
    height: 44px;
    color: var(--accent);
    opacity: 0.8;
}
.empty-icon :deep(svg) {
    width: 100%;
    height: 100%;
}

/* 「更新」分类：行内版本迁移线/错误（与其余行同结构，仅多小字）与统计条 */
.fm-upd-ver {
    display: block;
    font-size: 12px;
    color: var(--text-dim);
}
.fm-upd-ver b {
    color: var(--accent-2);
    font-weight: 600;
}
.fm-upd-err {
    color: var(--danger);
}
/* 行内更新箭头（PCL 口径）：名字后紧跟，低存在感，悬停才提起注意力 */
.fm-name-line {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    max-width: 100%;
}
.fm-upd-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent);
    opacity: 0.5;
    cursor: pointer;
    border-radius: 4px;
    transition:
        opacity 0.15s ease,
        background 0.15s ease;
}
.fm-upd-btn:hover:not(:disabled) {
    opacity: 1;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
}
.fm-upd-btn:disabled {
    cursor: default;
}
.fm-upd-btn.is-start {
    opacity: 0.9;
    animation: fm-upd-pulse 1s ease-in-out infinite;
}
.fm-upd-btn.is-ok {
    color: var(--success, var(--accent));
    opacity: 0.55;
}
.fm-upd-btn.is-error {
    color: var(--danger);
    opacity: 0.85;
}
@keyframes fm-upd-pulse {
    0%,
    100% {
        opacity: 0.35;
    }
    50% {
        opacity: 1;
    }
}
/* 静默检测提示条：一行细字，不出现在页面主体里抢版面 */
.upd-strip {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 12px;
    color: var(--text-dim);
}
.upd-strip .btn {
    margin-left: auto;
}
.spin-sm {
    width: 12px;
    height: 12px;
    flex: none;
    border-width: 2px;
}

@media (max-width: 1180px) {
    .fm-head {
        flex-wrap: wrap;
        align-items: flex-start;
    }
    .fm-head-left {
        flex-basis: 100%;
    }
    .fm-actions {
        width: 100%;
        flex-wrap: wrap;
        flex-shrink: 1;
        gap: 8px;
    }
    .fm-ver-select {
        flex: 1;
        max-width: none;
        min-width: 180px;
    }
}

.file-manager .fm-head {
    margin-bottom: 0;
    gap: 16px;
}
.fm-head .page-title small {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-dim);
    margin-left: 8px;
}
.fm-context {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 16px;
}
.fm-context > label {
    display: grid;
    gap: 6px;
    flex: 0 1 440px;
    min-width: 0;
    font-size: 12px;
    color: var(--text-dim);
}
.fm-context .fm-ver-select {
    width: 100%;
    max-width: none;
}
/* 聚合范围说明：一句话讲清这次列的是哪些目录 */
.fm-scope-summary {
    flex-basis: 100%;
    font-size: 12px;
}
.fm-context-row {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    flex-basis: 100%;
}
.fm-card {
    padding: 0 16px !important;
    overflow: visible;
}
.fm-table-head,
.fm-row {
    display: grid;
    /* 末列 auto：操作区宽度随内容（非模组行=删除，模组行=⋯+删除），不再按折叠时代的 40px 定宽（inventory M） */
    grid-template-columns: 36px minmax(0, 1fr) 88px 132px auto;
    gap: 12px;
    align-items: center;
}
.fm-table-head {
    min-height: 36px;
    font-size: 12px;
    color: var(--text-dim);
    border-bottom: 1px solid var(--border);
}
.fm-row {
    min-height: 72px;
    padding: 12px 0;
    flex-wrap: nowrap;
}
/* 同级相邻行分隔（UX：边界感；比边框弱一档） */
.fm-list > .fm-row + .fm-row {
    border-top: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
    border-radius: 0;
}
.fm-row .fm-name {
    min-width: 0 !important;
    white-space: normal;
    overflow-wrap: break-word;
}
.fm-internal {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
}
.fm-file-icon {
    width: 36px;
    height: 36px;
}
.fm-file-icon img {
    object-fit: contain;
}
/* 类型化兜底图标（无图资源用页面同款 SVG，不用文字字符） */
.fm-file-icon-kind {
    display: flex;
    width: 100%;
    height: 100%;
    color: var(--accent);
    opacity: 0.85;
}
.fm-file-icon-kind :deep(svg) {
    width: 100%;
    height: 100%;
}
.fm-meta {
    width: auto;
    min-width: 0;
    text-align: right;
    font-size: 12px;
    white-space: nowrap;
}
.fm-table-head > span:nth-child(3) {
    text-align: right;
}
.fm-date {
    text-align: left;
}
.file-manager[data-resource='mods'] .fm-table-head {
    grid-template-columns: 18px minmax(0, 1fr) 88px 132px 42px auto;
}
.file-manager[data-resource='mods'] .fm-row {
    grid-template-columns: 18px 32px minmax(0, 1fr) 88px 132px 42px auto;
}
.file-manager[data-resource='mods'] .fm-table-head > span:nth-child(2) {
    grid-column: 2/4;
}
.file-manager[data-resource='mods'] .fm-table-head {
    grid-template-columns: 18px 32px minmax(0, 1fr) 88px 132px 42px 40px;
}
.file-manager[data-resource='mods'] .fm-file-icon {
    width: 32px;
    height: 32px;
}
.fm-row:has(> input:checked) {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
}
/* 扫选进行中：禁文本选择，避免拖出一片选区 */
.fm-card.sweeping {
    user-select: none;
}
@media (max-width: 1150px) {
    .fm-table-head,
    .fm-row {
        grid-template-columns: 32px minmax(0, 1fr) 70px auto;
    }
    .fm-date {
        display: none !important;
    }
    .file-manager[data-resource='mods'] .fm-row,
    .file-manager[data-resource='mods'] .fm-table-head {
        grid-template-columns: 18px 28px minmax(0, 1fr) 70px 38px auto;
    }
    .fm-context > label {
        flex: 1 1 260px;
    }
}
@media (max-width: 700px) {
    .fm-head {
        align-items: flex-start;
        flex-wrap: wrap;
    }
    .fm-actions {
        gap: 8px;
    }
    .fm-table-head,
    .fm-row {
        gap: 8px;
    }
    .fm-file-icon {
        width: 28px;
        height: 28px;
    }
    .file-manager[data-resource='mods'] .fm-row,
    .file-manager[data-resource='mods'] .fm-table-head {
        grid-template-columns: 18px 24px minmax(0, 1fr) 38px auto;
    }
    .file-manager[data-resource='mods'] .fm-meta,
    .file-manager[data-resource='mods'] .fm-table-head > span:nth-child(3) {
        display: none;
    }
}

.file-manager {
    display: flex;
    flex-direction: column;
    gap: 12px !important;
}
.file-manager .fm-head,
.file-manager .fm-context,
.file-manager .fm-controls {
    margin: 0;
}
.fm-context :deep(.fm-ver-select) {
    max-width: none;
    width: 100%;
}
.fm-row {
    border-radius: 0;
    box-shadow: none;
    background: transparent;
}
/* 行 hover 统一规则（inventory L，2026-11-14 用户拍板修正版；对照 GameView .installed-row 注释块）：
   行 hover = 行背景 var(--hover) + 操作区低存在感控件整组苏醒
   （card-2 背景 + border-strong 边框 + 文字提亮），无位移；
   主操作的 accent 点亮只发生在指向按钮本身 :hover 时（.fm-upd-btn 自带规则），行 hover 不点亮 accent。
   行内删除保持 btn-danger 档位不参与苏醒：破坏性操作的红色身份优先于通用提亮。 */
.fm-row:hover :is(.file-more > summary):not(:disabled),
.fm-row:focus-within :is(.file-more > summary):not(:disabled) {
    background: var(--card-2);
    border-color: var(--border-strong);
    color: var(--text);
}
.fm-row:hover {
    background: var(--hover);
}
.fm-actions .file-more > summary {
    font-size: 14px;
}
.fm-head {
    flex-wrap: wrap;
}
.fm-row-disabled {
    opacity: 1;
    /* 禁用状态成套表达：删除线 + 标签之外，再垫一层底色，与启用行拉开 */
    background: color-mix(in srgb, var(--border) 18%, transparent);
}
</style>
