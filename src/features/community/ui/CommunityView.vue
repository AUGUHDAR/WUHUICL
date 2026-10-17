<script setup lang="ts">
import {
    computed,
    nextTick,
    onBeforeUnmount,
    onMounted,
    onUnmounted,
    reactive,
    ref,
    watch
} from 'vue';
import {
    communityFiles,
    communitySearch,
    errText,
    getManifest,
    getModTargets
} from '../../../api';
import {
    store,
    toast,
    selectedInstance,
    displayVersionName as versionLabel
} from '../../../store';
import { instanceKey } from '@shared/modCompatibility';
import { communityFileMatchesInstance, usesCommunityLoader } from '@shared/communityPolicy';
import { mcmodSearchUrl } from '@shared/communityLinks';
import CommunitySectionTabs from './CommunitySectionTabs.vue';
import CommunitySearchCard from './CommunitySearchCard.vue';
import CommunityResultList from './CommunityResultList.vue';
import CommunityDownloadQueue from './CommunityDownloadQueue.vue';
import CommunityDownloadModal from './CommunityDownloadModal.vue';
import CommunityFavorites from './CommunityFavorites.vue';
import CommunityModDetails from './CommunityModDetails.vue';
import { readCommunitySession, saveCommunitySession } from '../data/communitySession';
import {
    initialCommunityQuery,
    initialCommunityVersionSelection,
    chooseCommunityInstance,
    usableCommunityInstance,
    type CommunityVersionSource
} from '../data/communityVersionSelection';
import { useCommunityDownloadQueue } from '../data/useCommunityDownloadQueue';
import {
    favorites,
    favoriteBusy,
    loadFavorites,
    toggleProject
} from '../data/modFavorites';
const previousSession = readCommunitySession();
onMounted(() => void loadFavorites());
import type {
    CommunityFile,
    CommunityKind,
    CommunityProjectReference,
    CommunityResult,
    CommunitySource,
    LoaderName
} from '@shared/types';
import type { InstalledVersion } from '@shared/types';

// ---------------- 资源外链（源页面 + MC 百科介绍） ----------------
/** CurseForge 的 URL 分类段（按当前搜索分类推断） */
const CF_KIND_SEGMENT: Record<CommunityKind, string> = {
    mod: 'mc-mods',
    modpack: 'modpacks',
    resourcepack: 'texture-packs',
    shader: 'shaders',
    datapack: 'data-packs'
};

/** 资源的源站网页链接（Modrinth/CurseForge） */
function sourceUrl(r: CommunityProjectReference, kind: CommunityKind = query.kind): string {
    if (r.source === 'modrinth') return `https://modrinth.com/project/${r.slug || r.projectId}`;
    return `https://www.curseforge.com/minecraft/${CF_KIND_SEGMENT[kind] ?? 'mc-mods'}/${r.slug || r.projectId}`;
}

function openMcmod(item: CommunityProjectReference) {
    const url = mcmodSearchUrl({ ...item, slug: item.slug ?? '' });
    if (url) openExternal(url);
    else toast('该项目没有可用于检索的英文名称', 'error');
}

function openExternal(url: string) {
    window.open(url, '_blank');
}
const currentInstance = selectedInstance;
const allTargets = ref<InstalledVersion[]>([]);
const downloadQueue = useCommunityDownloadQueue();
const detailProject = ref<CommunityProjectReference | null>(null);
const communityTab = ref<'browse' | 'favorites'>(previousSession?.tab ?? 'browse');
const favoriteSearch = ref(previousSession?.favoriteSearch ?? '');
function selectTab(tab: 'browse' | 'favorites') {
    communityTab.value = tab;
}

// ---------------- 搜索条件 ----------------
const PAGE_SIZE = 20;

/** 完整 MC 版本列表（与游戏下载页同一数据源：远程版本清单，正式版为主） */
const manifestVersions = ref<string[]>([]);
const manifestLoading = ref(false);

async function loadManifest() {
    if (manifestVersions.value.length || manifestLoading.value) return;
    manifestLoading.value = true;
    try {
        const list = await getManifest();
        manifestVersions.value = list.filter((v) => v.type === 'release').map((v) => v.id);
    } catch {
        /* 清单失败时回退到已安装版本 */
        const set = new Set<string>();
        for (const v of store.installed) if (v.mcVersion) set.add(v.mcVersion);
        manifestVersions.value = [...set].sort().reverse();
    } finally {
        manifestLoading.value = false;
    }
}

const versionInput = ref(previousSession?.versionInput ?? '');

onMounted(() => void loadManifest());

const query = reactive(initialCommunityQuery(previousSession));
const versionSelection = reactive(initialCommunityVersionSelection(previousSession));
const installedVersionOptions = computed(() =>
    store.installed.filter(usableCommunityInstance).map((v) => ({
        value: instanceKey(v),
        label: versionLabel(v),
        description: `Minecraft ${v.mcVersion} · ${v.loader || '原版'} · ${v.folder}`
    }))
);
const filterInstance = computed(() =>
    store.installed.find((v) => instanceKey(v) === versionSelection.instance)
);
const versionSummary = computed(() =>
    query.mcVersion ? `Minecraft ${query.mcVersion}` : '全部 Minecraft 版本'
);
const loaderSummary = computed(() => {
    if (!query.loader) return '全部加载器';
    return {
        forge: 'Forge',
        fabric: 'Fabric',
        quilt: 'Quilt',
        neoforge: 'NeoForge'
    }[query.loader];
});
const manualLoaderMismatch = computed(
    () =>
        versionSelection.source === 'installed' &&
        versionSelection.loaderSource === 'manual' &&
        filterInstance.value &&
        (filterInstance.value.loader ?? '') !== query.loader
);
const supportsLoader = computed(() => usesCommunityLoader(query.kind));
const usesPagination = computed(() => !supportsLoader.value);

// ---------------- 搜索与列表 ----------------
const results = ref<CommunityResult[]>(previousSession?.results ?? []);
const loading = ref(false);
const loadingMore = ref(false);
const searched = ref(previousSession?.searched ?? false); // 是否已发起过搜索（区分初始空态）
const loadError = ref(previousSession?.error ?? '');
const offset = ref(previousSession?.offset ?? 0);
const hasMore = ref(previousSession?.hasMore ?? false);
const currentPage = ref(previousSession?.page ?? 1);
const totalResults = ref(previousSession?.total ?? 0);
const searchWarnings = ref<string[]>(previousSession?.warnings ?? []);
const totalPages = computed(() => Math.max(1, Math.ceil(totalResults.value / PAGE_SIZE)));
const visiblePages = computed(() => {
    const start = Math.max(1, Math.min(currentPage.value - 2, totalPages.value - 4));
    return Array.from({ length: Math.min(5, totalPages.value) }, (_, i) => start + i);
});

let searchGeneration = 0;
let disposed = false;
onBeforeUnmount(() => {
    saveCommunitySession({
        query: { ...query },
        tab: communityTab.value,
        favoriteSearch: favoriteSearch.value,
        versionInput: versionInput.value,
        versionSelection: { ...versionSelection },
        results: results.value,
        searched: searched.value,
        error: loadError.value,
        offset: offset.value,
        hasMore: hasMore.value,
        page: currentPage.value,
        total: totalResults.value,
        warnings: searchWarnings.value,
        scrollTop: document.querySelector<HTMLElement>('.content')?.scrollTop ?? 0,
        topKeyword: store.searchKeyword,
        interrupted: loading.value || loadingMore.value
    });
    disposed = true;
    searchGeneration++;
    fileGeneration++;
    openGeneration++;
});
async function doSearch(reset: boolean, page = currentPage.value) {
    if (disposed) return;
    if (!reset && (loading.value || loadingMore.value)) return;
    const generation = ++searchGeneration;
    if (reset) {
        offset.value = 0;
        currentPage.value = 1;
        totalResults.value = 0;
        searchWarnings.value = [];
    }
    const paged = usesPagination.value;
    if (paged && !reset) currentPage.value = page;
    const requestedOffset = paged ? (currentPage.value - 1) * PAGE_SIZE : offset.value;
    const first = reset || paged;
    if (first) loading.value = true;
    else loadingMore.value = true;
    loadError.value = '';
    searched.value = true;
    try {
        const response = await communitySearch({
            keyword: query.keyword.trim(),
            kind: query.kind,
            source: query.source,
            mcVersion: query.mcVersion || undefined,
            loader: supportsLoader.value ? query.loader || undefined : undefined,
            sort: query.sort,
            offset: requestedOffset,
            limit: PAGE_SIZE
        });
        if (generation !== searchGeneration) return;
        const list = response.items;
        if (first) results.value = list;
        else results.value = [...results.value, ...list];
        totalResults.value = response.total;
        searchWarnings.value = response.warnings ?? [];
        hasMore.value = requestedOffset + PAGE_SIZE < response.total;
        offset.value = requestedOffset + PAGE_SIZE;
    } catch (e) {
        if (generation !== searchGeneration) return;
        loadError.value = errText(e);
        if (!first) toast('加载失败：' + loadError.value, 'error');
    } finally {
        if (generation === searchGeneration) {
            loading.value = false;
            loadingMore.value = false;
        }
    }
}

const onSearch = () => void doSearch(true);
const onLoadMore = () => void doSearch(false);
async function goToPage(page: number) {
    if (loading.value || page < 1 || page > totalPages.value || page === currentPage.value) return;
    await doSearch(false, page);
}

onUnmounted(() => {
    searchGeneration++;
    fileGeneration++;
    if (topSearchTimer) clearTimeout(topSearchTimer);
});
function useCurrentInstance() {
    if (currentInstance.value) useInstance(instanceKey(currentInstance.value));
}
versionInput.value = query.mcVersion;

/** 按具体实例筛选：选中实例即带入其 MC 版本与 Loader */
function useInstance(id: string) {
    const v = store.installed.find((x) => instanceKey(x) === id);
    if (!v) return;
    const choice = chooseCommunityInstance(v, versionSelection, query.loader ?? '');
    if (!choice) return;
    Object.assign(versionSelection, choice.selection);
    query.mcVersion = choice.mcVersion;
    query.loader = choice.loader;
    versionInput.value = query.mcVersion;
    onFilterChange();
}
function chooseVersionSource(source: CommunityVersionSource) {
    const previousSource = versionSelection.source;
    versionSelection.source = source;
    if (source === 'all') {
        versionSelection.instance = '';
        query.mcVersion = '';
        versionInput.value = '';
        if (versionSelection.loaderSource === 'instance') {
            query.loader = '';
            versionSelection.loaderSource = 'all';
        }
        onFilterChange();
    } else if (source === 'installed' && previousSource !== 'installed') {
        versionSelection.instance = '';
        query.mcVersion = '';
        versionInput.value = '';
        if (versionSelection.loaderSource === 'instance') {
            query.loader = '';
            versionSelection.loaderSource = 'all';
        }
        onFilterChange();
    } else if (source === 'custom') {
        versionSelection.instance = '';
        if (versionSelection.loaderSource === 'instance')
            versionSelection.loaderSource = query.loader ? 'manual' : 'all';
        void nextTick(() =>
            document
                .querySelector<HTMLInputElement>('[data-ui="community:custom-version"] input')
                ?.focus()
        );
    }
}
function customVersionChanged() {
    versionSelection.source = query.mcVersion ? 'custom' : 'all';
    versionSelection.instance = '';
    versionInput.value = query.mcVersion ?? '';
    onFilterChange();
}
function loaderChanged() {
    versionSelection.loaderSource = query.loader ? 'manual' : 'all';
    onFilterChange();
}
function useInstanceLoader() {
    query.loader = filterInstance.value?.loader ?? '';
    versionSelection.loaderSource = 'instance';
    onFilterChange();
}

/** 切换条件后自动重新搜索 */
function onFilterChange() {
    if (communityTab.value === 'browse') void doSearch(true);
}

function onReset() {
    query.keyword = '';
    query.kind = 'mod';
    query.source = 'all';
    query.mcVersion = '';
    query.loader = '';
    query.sort = 'relevance';
    versionInput.value = '';
    Object.assign(versionSelection, { source: 'all', instance: '', loaderSource: 'all' });
    void doSearch(true);
}

onMounted(async () => {
    // A route-local snapshot keeps the query, loaded pages and scroll without retaining DOM.
    const newKeyword = store.searchKeyword.trim();
    if (!previousSession || (newKeyword && newKeyword !== previousSession.topKeyword.trim())) {
        query.keyword = newKeyword;
        void doSearch(true);
    } else {
        await nextTick();
        const content = document.querySelector<HTMLElement>('.content');
        if (content) content.scrollTop = previousSession.scrollTop;
        if (previousSession.interrupted)
            loadError.value = '上次查询在离开页面时中断，已保留原结果；请重试。';
    }
});

// ---------------- 顶栏搜索联动：顶栏输入防抖驱动社区搜索 ----------------
let topSearchTimer: ReturnType<typeof setTimeout> | null = null;
watch(
    () => store.searchKeyword,
    (kw) => {
        if (topSearchTimer) clearTimeout(topSearchTimer);
        topSearchTimer = setTimeout(() => {
            if (communityTab.value === 'favorites') favoriteSearch.value = kw.trim();
            else {
                query.keyword = kw.trim();
                void doSearch(true);
            }
        }, 400);
    }
);

const itemKey = (r: CommunityProjectReference) => `${r.source}:${r.projectId}`;
// ---------------- 下载模态框 ----------------
const modal = reactive({
    open: false,
    item: null as CommunityProjectReference | null,
    kind: 'mod' as CommunityKind,
    files: [] as CommunityFile[],
    loadingFiles: false,
    filesError: '',
    fileId: '',
    versionId: '',
    mcVersion: '',
    loader: '' as LoaderName | ''
});

const isModpack = computed(() => modal.kind === 'modpack');
const selectedFile = computed(() => modal.files.find((f) => f.fileId === modal.fileId) ?? null);
const targetOptions = computed(() =>
    allTargets.value.filter(
        (v) =>
            !v.failed &&
            !v.incomplete &&
            (modal.kind !== 'mod' ||
                (selectedFile.value && communityFileMatchesInstance(selectedFile.value, v)))
    )
);
watch(targetOptions, (options) => {
    if (!options.some((v) => instanceKey(v) === modal.versionId)) {
        const selected =
            options.find(
                (v) =>
                    v.id === currentInstance.value?.id && v.folder === currentInstance.value?.folder
            ) ?? options[0];
        modal.versionId = selected ? instanceKey(selected) : '';
    }
});
let fileGeneration = 0,
    openGeneration = 0;
async function loadFiles() {
    if (!modal.item) return;
    const generation = ++fileGeneration,
        item = modal.item;
    modal.loadingFiles = true;
    modal.filesError = '';
    modal.files = [];
    modal.fileId = '';
    try {
        const files = await communityFiles(item.source, item.projectId, {
            kind: modal.kind,
            mcVersion: modal.mcVersion || undefined,
            loader: usesCommunityLoader(modal.kind) ? modal.loader || undefined : undefined
        });
        if (generation !== fileGeneration || !modal.open) return;
        modal.files = files;
        modal.fileId = (files.find((f) => f.releaseType === 'release') ?? files[0])?.fileId ?? '';
        if (!files.length)
            modal.filesError = usesCommunityLoader(modal.kind)
                ? '当前 Minecraft / Loader 条件下没有文件，可手动调整筛选。'
                : '当前 Minecraft 版本下没有文件，可调整版本筛选。';
    } catch (e) {
        if (generation === fileGeneration) modal.filesError = '获取文件列表失败：' + errText(e);
    } finally {
        if (generation === fileGeneration) modal.loadingFiles = false;
    }
}

async function openDownload(item: CommunityProjectReference, kind: CommunityKind = query.kind) {
    const generation = ++openGeneration;
    fileGeneration++;
    modal.open = true;
    modal.item = item;
    modal.kind = kind;
    modal.files = [];
    modal.loadingFiles = true;
    modal.filesError = '';
    modal.fileId = '';
    modal.versionId = currentInstance.value ? instanceKey(currentInstance.value) : '';
    modal.mcVersion = query.mcVersion;
    modal.loader = usesCommunityLoader(kind) ? query.loader : '';
    try {
        const scanned = await getModTargets();
        if (disposed || generation !== openGeneration || !modal.open) return;
        allTargets.value = scanned.versions;
        if (scanned.errors.length) toast('部分目录扫描失败：' + scanned.errors.join('；'), 'error');
        await loadFiles();
    } catch (e) {
        if (!disposed && generation === openGeneration)
            modal.filesError = '获取文件列表失败：' + errText(e);
    } finally {
        if (generation === openGeneration) modal.loadingFiles = false;
    }
}

const canConfirm = computed(
    () =>
        !!selectedFile.value &&
        !modal.loadingFiles &&
        (isModpack.value || targetOptions.value.some((v) => instanceKey(v) === modal.versionId))
);

function confirmDownload() {
    const file = selectedFile.value;
    if (!file || !canConfirm.value) return;
    const target = isModpack.value
        ? undefined
        : targetOptions.value.find((v) => instanceKey(v) === modal.versionId);
    try {
        const folder =
            target?.folder ||
            store.settings?.folders.find((folder) => folder.isDefault)?.path ||
            store.settings?.gameDir ||
            '';
        const { added } = downloadQueue.enqueue({ file, kind: modal.kind, target, folder });
        modal.open = false;
        fileGeneration++;
        openGeneration++;
        toast(
            added
                ? modal.kind === 'mod'
                    ? '已加入社区队列，检测完成后请在队列确认前置与安装'
                    : '已加入社区下载队列，可继续浏览资源'
                : '该文件已在目标实例的社区队列中',
            'info'
        );
    } catch (e) {
        toast('加入队列失败：' + errText(e), 'error');
    }
}

function openSource(r: CommunityProjectReference, kind: CommunityKind = query.kind) {
    openExternal(sourceUrl(r, kind));
}

function clearModalSelections() {
    modal.open = false;
    fileGeneration++;
    openGeneration++;
}

function setModalVersionId(value: string) {
    if (modal.files.some((file) => file.fileId === value)) modal.fileId = value;
    else modal.versionId = value;
}
</script>
<template>
    <div
        data-ui="CommunityView:6e56fae5e7dd"
        class="page community-page"
        :data-design-page="query.kind"
    >
        <div data-ui="CommunityView:9346ef73457d" class="page-head">
            <h1 data-ui="CommunityView:5476a5545de6" class="page-title">社区资源</h1>
            <p data-ui="CommunityView:8ea54e89551b" class="page-sub">
                搜索并下载 Modrinth / CurseForge 上的 Mod、整合包、资源包、光影与数据包
            </p>
        </div>
        <CommunityDownloadQueue />
        <CommunitySectionTabs
            :tab="communityTab"
            :favorites-count="favorites.length"
            @change="selectTab"
        />
        <CommunityFavorites
            v-if="communityTab === 'favorites'"
            :keyword="favoriteSearch"
            @download="openDownload($event, 'mod')"
            @details="detailProject = $event"
            @browse="
                communityTab = 'browse';
                query.kind = 'mod';
                onFilterChange();
            "
        />
        <template v-else>
            <CommunitySearchCard
                :query="query"
                :version-selection="versionSelection"
                :installed-version-options="installedVersionOptions"
                :manifest-versions="manifestVersions"
                :manifest-loading="manifestLoading"
                :version-summary="versionSummary"
                :loader-summary="loaderSummary"
                :supports-loader="supportsLoader"
                :manual-loader-mismatch="!!manualLoaderMismatch"
                :filter-loader="filterInstance?.loader"
                :current-instance="currentInstance"
                :loading="loading"
                @search="onSearch"
                @reset="onReset"
                @filter="onFilterChange"
                @version-source="chooseVersionSource"
                @version-changed="customVersionChanged"
                @loader-changed="loaderChanged"
                @instance="useInstance"
                @current-instance="useCurrentInstance"
                @instance-loader="useInstanceLoader"
            />
            <CommunityResultList
                :results="results"
                :kind="query.kind"
                :loading="loading"
                :loading-more="loadingMore"
                :searched="searched"
                :load-error="loadError"
                :warnings="searchWarnings"
                :uses-pagination="usesPagination"
                :has-more="hasMore"
                :current-page="currentPage"
                :total-pages="totalPages"
                :total-results="totalResults"
                :visible-pages="visiblePages"
                :favorites="favorites"
                :favorite-busy="favoriteBusy"
                @retry="onSearch"
                @load-more="onLoadMore"
                @page="goToPage"
                @download="openDownload($event)"
                @favorite="toggleProject($event.source, $event.projectId, $event.title, $event.iconUrl)"
                @source="openSource($event)"
                @mcmod="openMcmod($event)"
            />
        </template>
        <CommunityDownloadModal
            :item="modal.open ? modal.item : null"
            :kind="modal.kind"
            :files="modal.files"
            :loading="modal.loadingFiles"
            :error="modal.filesError"
            :file-id="modal.fileId"
            :version-id="modal.versionId"
            :mc-version="modal.mcVersion"
            :loader="modal.loader"
            :target-instances="targetOptions"
            :manifest-versions="manifestVersions"
            :favorite="
                !!modal.item &&
                favorites.some((favorite) => favorite.key === itemKey(modal.item!))
            "
            :favorite-busy="!!modal.item && favoriteBusy.has(itemKey(modal.item))"
            @close="clearModalSelections"
            @links="modal.item && openSource(modal.item, modal.kind)"
            @mcmod="modal.item && openMcmod(modal.item)"
            @favorite="
                modal.item &&
                toggleProject(
                    modal.item.source,
                    modal.item.projectId,
                    modal.item.title,
                    modal.item.iconUrl
                )
            "
            @update-mc-version="modal.mcVersion = $event"
            @update-loader="modal.loader = $event"
            @update-version-id="setModalVersionId"
            @load-files="loadFiles"
            @confirm="confirmDownload"
        />
        <CommunityModDetails
            v-if="detailProject"
            :reference="detailProject"
            @close="detailProject = null"
            @download="
                detailProject = null;
                openDownload($event, 'mod');
            "
        />
    </div>
</template>
<style src="./CommunityView.css"></style>
